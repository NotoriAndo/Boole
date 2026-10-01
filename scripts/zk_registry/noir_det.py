#!/usr/bin/env python3
"""Noir DET problem generator: wave driver.

Subcommands::

    select    --ledger LEDGER-v1.jsonl --out SELECTION.jsonl     apply the wave-N1 population filter
    wave      --config wave.json [--jobs N] [--only ITEM ...]    plan, compile, decode, gate and package
    validate  --index INDEX.jsonl --packages DIR                 re-validate every record and package file

Pipeline per ledger row (deduplicated by content key first): classify the function
(:func:`noir_instantiation.classify`), pick the repository's pinned compiler
(:data:`noir_toolchain.REPO_COMPILERS`), plan instantiation candidates by tier, compile each candidate
(``nargo export`` of an in-crate or stdlib wrapper, ``nargo compile`` of a binary main or a contract),
decode the ACIR with the boole-acir-tool of the same noir version, flatten ACIR calls, apply the size
policy (2,000 opcodes), emit the Lean model and DET statement, and run the gates: G-ELAB, G-NONVAC (an
execution witness from the compiler's own ACVM solver on derived inputs, or the repository's
Prover.toml), G-FID (every real witness and single-witness mutant evaluated by Lean and by the Python
evaluator of the decoded ACIR under the real black-box function values), G-TRIV (battery P1 + P2) and a
sound counterexample search.  Every row ends in one terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE,
TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or NOT-APPLICABLE (with a reason).  The driver never writes a
proof of a DET statement; the only Lean proofs attempted are the battery forms, whose closures fail G-TRIV.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import platform
import random
import re
import shutil
import sys
import threading
import time
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C                     # noqa: E402
from zk_registry import gates as G                     # noqa: E402
from zk_registry import lean_emit as E                 # noqa: E402
from zk_registry import lean_runner as L               # noqa: E402
from zk_registry import noir_acir as A                 # noqa: E402
from zk_registry import noir_det_search as DSN         # noqa: E402
from zk_registry import noir_instantiation as I        # noqa: E402
from zk_registry import noir_lean_emit as NE           # noqa: E402
from zk_registry import noir_source as NS              # noqa: E402
from zk_registry import noir_toolchain as T            # noqa: E402
from zk_registry import package as P                   # noqa: E402

GENERATOR_NAME = "boole-zk-registry-noir-det"
GENERATOR_VERSION = "1.0"
_HERE = os.path.dirname(os.path.abspath(__file__))
NOIR_SOURCES = ["noir_det.py", "noir_acir.py", "noir_source.py", "noir_instantiation.py", "noir_lean_emit.py",
                "noir_det_search.py", "noir_toolchain.py", "noir_tool/Cargo.toml", "noir_tool/src/main.rs"]
SHARED_SOURCES = ["__init__.py", "package.py", "jsonschema_lite.py", "lean_runner.py", "lean_emit.py", "gates.py",
                  "check.py", "det_search.py", "lean/ZkReplay.lean", "schema/problem.schema.json"]

MAX_OPCODES = P.MAX_CONSTRAINTS
SIZING_LIMIT = 400000                  # flattening stops above this (size recorded as a lower bound)
REAL_WANTED = 16
MUTANTS = 16
SAMPLES = 128
PROBED_NOT_A_FINDING = ("not-a-finding: probed instantiation (generic parameters chosen by the generator, not taken "
                        "from the repository)")

# ------------------------------------------------------------------------------------------ population

EXCLUDED_UNITS_SUFFIX = ("-constraint-site", "-interaction-site")
EXCLUDED_UNITS = ("C2-operation", "U1-instruction")
EXCLUDED_FLAG_SUBSTRINGS = ("test", "not-counted", "deprecated", "NOT-ITEMIZED", "ZERO-ITEMS", "supplementary",
                            "exported-api-false", "intrinsic", "generated", "duplicate-of", "copy-of", "vendored",
                            "mapped-to", "archived", "excluded", "secondary", "program-as-circuit")
SELECTION_FILTER = {
    "framework": "noir",
    "excluded_unit_suffixes": list(EXCLUDED_UNITS_SUFFIX),
    "excluded_units": list(EXCLUDED_UNITS),
    "excluded_flag_substrings": list(EXCLUDED_FLAG_SUBSTRINGS),
}


def selected(row: dict) -> bool:
    if row.get("framework") != "noir":
        return False
    unit = row.get("unit", "")
    if unit.endswith(EXCLUDED_UNITS_SUFFIX) or unit in EXCLUDED_UNITS:
        return False
    return not any(s in f for f in row.get("flags", []) for s in EXCLUDED_FLAG_SUBSTRINGS)


def select_rows(ledger_path: str) -> list[dict]:
    with open(ledger_path, encoding="utf-8") as f:
        return [r for r in (json.loads(line) for line in f if line.strip()) if selected(r)]


def repo_id_of(url: str) -> str:
    m = re.match(r"https://github\.com/([^/]+)/([^/]+?)(\.git)?/?$", url)
    return f"{m.group(1)}/{m.group(2)}" if m else url


def repo_dir_name(repo_id: str) -> str:
    return repo_id.replace("/", "__")


# ------------------------------------------------------------------------------------------ config

@dataclass
class WaveConfig:
    collection: str
    ledger: str
    repos: dict                               # repo_id -> {"dir", "url", "commit", "release"}
    tools_root: str
    home: str
    scratch: str
    lean_env: str
    out: str
    work: str
    jobs: int = 4
    only: list = field(default_factory=list)
    keep_work: bool = False
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    det_search_budget_s: float = 20.0
    compile_timeout: float = 900
    git_pins: dict = field(default_factory=dict)

    @classmethod
    def load(cls, path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            d = json.load(f)
        return cls(**d)


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in SHARED_SOURCES + NOIR_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(_HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


def generator_files() -> list[str]:
    """Files hashed by this generator only (the circom generator hashes the shared ones too)."""
    return [f for f in NOIR_SOURCES if f.endswith((".py", ".js", ".lean", ".json"))]


NOIR_DET_SPEC_CLAUSES = [
    "Output determinism: for the compiled ACIR of the instantiated Noir function (the generated wrapper, the "
    "repository's main, or the contract entrypoint), any two witness assignments that satisfy every constrained "
    "opcode and agree on every parameter witness agree on every return-value witness.",
    "The field is the BN254 scalar field (the ACIR field of the compiler); parameters are the ACIR private and "
    "public parameter witnesses of the function, return values its return witnesses (for `&mut` parameters the "
    "wrapper returns their final values as well).",
]
NOIR_DET_SPEC = {
    "tier": "T1",
    "source": "mathematical definition of output determinism (functional dependence of outputs on inputs)",
    "clauses": NOIR_DET_SPEC_CLAUSES,
    "sha256": hashlib.sha256("\n".join(NOIR_DET_SPEC_CLAUSES).encode("utf-8")).hexdigest(),
}


def statement_assumptions(bb_keys: list[str], memory: bool, brillig: int) -> list[str]:
    out = ["[Fact (Nat.Prime p)]: primality of the BN254 scalar field order, supplied as an instance hypothesis so "
           "that field lemmas apply; p is prime, so the hypothesis does not weaken the statement."]
    if bb_keys:
        out.append("Black boxes (" + ", ".join(bb_keys) + ") are uninterpreted functions of their inputs, "
                   "universally quantified and shared by both assignments: the statement assumes each black box is a "
                   "deterministic function of its inputs, as the ACVM solver computes it (for EmbeddedCurveAdd / "
                   "MultiScalarMul this includes inputs outside the opcode's documented assumptions); a black box with "
                   "a witness predicate constrains only when the predicate is non-zero; a black box without outputs "
                   "(recursive aggregation) is the uninterpreted condition bb k inputs = [].")
    out.append("AND / XOR are interpreted as bitwise operations on num_bits-bit operands (operands outside num_bits "
               "bits do not satisfy the opcode); RANGE as value < 2^num_bits.")
    if memory:
        out.append("Memory blocks: every active read or write needs an index below the block length (as the backend's "
                   "ROM/RAM tables enforce); reads equal the current contents and writes replace them, in program "
                   "order.")
    if brillig:
        out.append(f"{brillig} Brillig call(s): their outputs are unconstrained prover hints and add no constraint.")
    return out


# ------------------------------------------------------------------------------------------ shared state

@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    tc: T.Toolchain
    ledger_sha: str
    generator: dict
    indexes: dict = field(default_factory=dict)          # repo_id -> RepoIndex
    lock: threading.Lock = field(default_factory=threading.Lock)
    deps: dict = field(default_factory=dict)             # crate dir -> [dependency records]


def log(sh: Shared, msg: str) -> None:
    with sh.lock:
        print(time.strftime("%H:%M:%S"), msg, flush=True)


def scrub_text(sh: Shared, s: str) -> str:
    for root in sorted({sh.cfg.scratch, sh.cfg.work, sh.cfg.home, sh.cfg.tools_root} |
                       {r["dir"] for r in sh.cfg.repos.values()}, key=len, reverse=True):
        if root:
            s = s.replace(root, "$LOCAL")
    return s


def nr_files(root: str) -> list[str]:
    out = []
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in (".git", "node_modules", "target", "export")]
        for f in files:
            if f.endswith(".nr"):
                out.append(os.path.relpath(os.path.join(d, f), root))
    return sorted(out)


# ------------------------------------------------------------------------------------------ targets

_MODTREE: dict = {}


def module_files(crate: str, root_file: str | None = None) -> set[str]:
    """Source files reachable from the crate root through ``mod x;`` declarations (comments ignored):
    ``x.nr`` or ``x/mod.nr`` beside a root/``mod.nr`` file, ``<file stem>/x.nr`` beside any other file."""
    src = os.path.join(crate, "src")
    roots = [root_file] if root_file else [os.path.join(src, f) for f in ("lib.nr", "main.nr")
                                           if os.path.exists(os.path.join(src, f))]
    seen: set[str] = set()
    todo = list(roots)
    while todo:
        f = todo.pop()
        if f in seen or not os.path.exists(f):
            continue
        seen.add(f)
        with open(f, encoding="utf-8", errors="replace") as fh:
            text = NS.blank_comments_strings(fh.read())
        base = os.path.dirname(f) if os.path.basename(f) in ("lib.nr", "main.nr", "mod.nr") else f[:-3]
        for m in re.finditer(r"(?<![A-Za-z0-9_])mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", text):
            for cand in (os.path.join(base, m.group(1) + ".nr"), os.path.join(base, m.group(1), "mod.nr")):
                if os.path.exists(cand):
                    todo.append(cand)
                    break
    return seen


def in_module_tree(t: I.Target) -> bool:
    if t.stdlib or t.crate is None:
        return True
    if t.crate not in _MODTREE:
        _MODTREE[t.crate] = module_files(t.crate)
    return os.path.join(t.root, t.path) in _MODTREE[t.crate]


def make_target(sh: Shared, row: dict, cache: dict) -> tuple[I.Target | None, str]:
    rid = repo_id_of(row["repo"])
    repo = sh.cfg.repos.get(rid)
    if repo is None:
        return None, f"repository {rid} not configured"
    root = repo["dir"]
    path = os.path.join(root, row["path"])
    if path not in cache:
        if not os.path.exists(path):
            cache[path] = None
        else:
            with open(path, encoding="utf-8", errors="replace") as f:
                src = f.read()
            cache[path] = (src,) + NS.scan(src)
    entry = cache[path]
    if entry is None:
        return None, "source file missing at the pin"
    src, fns, blocks, text = entry
    fn, how = NS.find_function(fns, row["symbol"], row["census"][0].get("line") if row.get("census") else None)
    if fn is None:
        return None, f"function not found ({how})"
    crate = T.crate_of(path, root)
    ctype = T.crate_info(crate)["type"] if crate else "lib"
    stdlib = rid == "noir-lang/noir" and row["path"].startswith("noir_stdlib/")
    return I.Target(row["item_id"], rid, root, row["path"], fn, text, src, crate, ctype, stdlib), how


# ------------------------------------------------------------------------------------------ content identity

def content_key(sh: Shared, t: I.Target, compiler: str) -> str:
    """sha256 over the compiler tag and the comment-free, whitespace-normalized text of the function, its
    enclosing impl header, and every declaration of the crate (and its path dependencies) whose name the
    function reaches, transitively.  Over-approximating the reached set can only separate copies."""
    idx = sh.indexes[t.repo]
    decls = crate_decls(sh, t)
    seen_names: set[str] = set()
    texts = [NS.strip_for_hash(t.src[t.fn.start:t.fn.end])]
    imp = t.fn.impl
    if imp is not None:
        texts.append(NS.strip_for_hash(imp.header))
    frontier = set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", t.text[t.fn.start:t.fn.end]))
    while frontier:
        name = frontier.pop()
        if name in seen_names:
            continue
        seen_names.add(name)
        for body in decls.get(name, []):
            texts.append(NS.strip_for_hash(body))
            frontier |= set(re.findall(r"[A-Za-z_][A-Za-z0-9_]*", NS.blank_comments_strings(body))) - seen_names
    del idx
    h = hashlib.sha256(compiler.encode() + b"\0")
    for x in sorted(set(texts)):
        h.update(x.encode() + b"\n")
    return h.hexdigest()


_DECL_CACHE: dict = {}


def crate_decls(sh: Shared, t: I.Target) -> dict:
    """name -> declaration texts (functions, structs, globals, traits, type aliases) of the target's crate and
    its path dependencies (stdlib: the stdlib itself)."""
    key = t.crate or t.root
    if key in _DECL_CACHE:
        return _DECL_CACHE[key]
    dirs = [key] + [d for d in path_dep_dirs(key)]
    out: dict[str, list[str]] = {}
    for d in dirs:
        for rel in nr_files(os.path.join(d, "src") if os.path.isdir(os.path.join(d, "src")) else d):
            p = os.path.join(d, "src", rel) if os.path.isdir(os.path.join(d, "src")) else os.path.join(d, rel)
            try:
                with open(p, encoding="utf-8", errors="replace") as f:
                    src = f.read()
            except OSError:
                continue
            fns, blocks, text = NS.scan(src)
            for fn in fns:
                out.setdefault(fn.name, []).append(src[fn.start:fn.end])
            for m in re.finditer(r"(?<![A-Za-z0-9_])(struct|trait|global|type)\s+([A-Za-z_][A-Za-z0-9_]*)", text):
                end = text.find(";", m.end()) if m.group(1) in ("global", "type") else -1
                if m.group(1) in ("struct", "trait"):
                    b = text.find("{", m.end())
                    end = NS.matching(text, b, "{", "}") if b >= 0 else m.end()
                out.setdefault(m.group(2), []).append(src[m.start():end + 1 if end >= 0 else m.end()])
    _DECL_CACHE[key] = out
    return out


def path_dep_dirs(crate: str, seen: set | None = None) -> list[str]:
    seen = set() if seen is None else seen
    out = []
    m = os.path.join(crate, "Nargo.toml")
    if not os.path.exists(m):
        return out
    for spec in T.read_toml(m).get("dependencies", {}).values():
        if isinstance(spec, dict) and "path" in spec:
            d = os.path.normpath(os.path.join(crate, spec["path"]))
            if d not in seen:
                seen.add(d)
                out.append(d)
                out += path_dep_dirs(d, seen)
    return out


# ------------------------------------------------------------------------------------------ compile

def copy_crate(src_dir: str, dest: str, as_lib: bool, strip_contract: bool = False) -> None:
    """Copy a crate (sources and manifest) with path dependencies made absolute; ``as_lib`` makes a binary or
    contract crate a library (``src/lib.nr`` = ``src/main.nr``; ``strip_contract`` removes the ``contract``
    block, keeping its sibling modules)."""
    if os.path.exists(dest):
        shutil.rmtree(dest)
    shutil.copytree(src_dir, dest, ignore=shutil.ignore_patterns(".git", "target", "export", "node_modules", "*.gz"))
    mpath = os.path.join(dest, "Nargo.toml")
    with open(mpath, encoding="utf-8") as f:
        man = f.read()

    def abs_path(m):
        p = m.group(3)
        return f'{m.group(1)}"{os.path.normpath(os.path.join(src_dir, p))}"'
    man = re.sub(r"""(path\s*=\s*)(["'])([^"']+)\2""", abs_path, man)
    if as_lib:
        man = re.sub(r'(?m)^(\s*type\s*=\s*)"(bin|contract)"', r'\1"lib"', man)
        main = os.path.join(dest, "src", "main.nr")
        lib = os.path.join(dest, "src", "lib.nr")
        if os.path.exists(main) and not os.path.exists(lib):
            with open(main, encoding="utf-8") as f:
                text = f.read()
            if strip_contract:
                text = remove_contract_block(text)
            with open(lib, "w", encoding="utf-8") as f:
                f.write(text)
    with open(mpath, "w", encoding="utf-8") as f:
        f.write(man)


def remove_contract_block(src: str) -> str:
    _, blocks, text = NS.scan(src)
    for b in sorted([b for b in blocks if b.kind == "contract"], key=lambda b: -b.start):
        _, astart = NS._attrs_before(text, text.rfind("contract", 0, b.start))
        src = src[:astart] + src[b.end:]
    return src


NARGO_ERROR = re.compile(r"(?m)^error: (.+)$")
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def first_error(out: str) -> str:
    out = ANSI.sub("", out)
    if "This is a bug" in out or "panicked at" in out:
        m = re.search(r"(?m)^\s*(?:Message|panicked at[^\n]*):?\s*(.+)$", out)
        loc = re.search(r"Location:\s*(\S+)", out)
        return ("compiler internal error (panic)" + (f": {m.group(1).strip()[:200]}" if m else "") +
                (f" [{loc.group(1)}]" if loc else ""))[:400]
    m = NARGO_ERROR.search(out)
    if not m:
        tail = [ln for ln in out.strip().splitlines() if ln.strip()][-3:]
        return " | ".join(tail)[:300]
    loc = re.search(r"┌─ ([^\n]+)", out[m.end():m.end() + 400])
    return (m.group(1) + (f" at {loc.group(1).strip()}" if loc else ""))[:400]


@dataclass
class Compiled:
    ok: bool
    compiler: str
    artifact: str = ""
    contract_fn: str | None = None
    error: str = ""
    secs: float = 0.0
    command: str = ""


def compile_candidate(sh: Shared, t: I.Target, plan: str, cand: dict, cdir: str, tag: str) -> Compiled:
    os.makedirs(cdir, exist_ok=True)
    t0 = time.time()
    if plan in ("export", "std-export"):
        crate = os.path.join(cdir, "crate")
        wid = cand["wid"]
        if plan == "std-export":
            return compile_std(sh, t, cand, crate, tag, t0)
        else:
            contract_crate = t.crate_type == "contract"
            copy_crate(t.crate, crate, as_lib=t.crate_type in ("bin", "contract"), strip_contract=contract_crate)
            target_file = os.path.join(crate, "src", t.rel_in_crate)
            if t.crate_type in ("bin", "contract") and t.rel_in_crate == "main.nr":
                target_file = os.path.join(crate, "src", "lib.nr")
            imports = assignment_imports(sh, t, cand["assign"])
            cand["imports"] = imports
            with open(target_file, "a", encoding="utf-8") as f:
                f.write("\n\n// generated by boole-zk-registry (noir DET wrapper)\n" + "".join(
                    f"use {x};\n" for x in imports) + cand["wrapper"].text)
        r = sh.tc.run_nargo(tag, ["export", "--silence-warnings"], crate, sh.cfg.compile_timeout)
        art = os.path.join(crate, "export", f"{cand['wrapper'].name}.json")
        cmd = "nargo export --silence-warnings"
        with open(os.path.join(cdir, "nargo.log"), "w", encoding="utf-8") as f:
            f.write(r.out[-20000:])
        if r.rc == 0 and os.path.exists(art):
            return Compiled(True, tag, art, None, "", round(time.time() - t0, 2), cmd)
        err = "timeout" if r.timeout else ("memory limit" if r.memkill else first_error(r.out))
        if r.rc == 0 and not err.strip():
            err = "nargo export produced no artifact for the wrapper"
        return Compiled(False, tag, error=scrub_text(sh, err), secs=round(time.time() - t0, 2), command=cmd)
    # bin-main / contract-fn: compile the crate as the repository builds it
    crate = os.path.join(cdir, "crate")
    copy_crate(t.crate, crate, as_lib=False)
    r = sh.tc.run_nargo(tag, ["compile", "--silence-warnings"], crate, sh.cfg.compile_timeout)
    cmd = "nargo compile --silence-warnings"
    with open(os.path.join(cdir, "nargo.log"), "w", encoding="utf-8") as f:
        f.write(r.out[-20000:])
    tdir = os.path.join(crate, "target")
    arts = sorted(x for x in os.listdir(tdir) if x.endswith(".json")) if os.path.isdir(tdir) else []
    if r.rc != 0 or not arts:
        err = "timeout" if r.timeout else ("memory limit" if r.memkill else first_error(r.out))
        return Compiled(False, tag, error=scrub_text(sh, err), secs=round(time.time() - t0, 2), command=cmd)
    art = os.path.join(tdir, arts[0])
    if plan == "contract-fn":
        names = [f["name"] for f in T.load_json(art).get("functions", [])]
        if t.fn.name not in names:
            return Compiled(False, tag, error=f"contract artifact has no function {t.fn.name}",
                            secs=round(time.time() - t0, 2), command=cmd)
        return Compiled(True, tag, art, t.fn.name, "", round(time.time() - t0, 2), cmd)
    return Compiled(True, tag, art, None, "", round(time.time() - t0, 2), cmd)


STD_RESOLUTION = re.compile(r"is private|not visible|Could not resolve|could not resolve|not found in|is not exported")


def compile_std(sh: Shared, t: I.Target, cand: dict, crate: str, tag: str, t0: float) -> Compiled:
    """A wrapper crate calling a public stdlib item.  Every capitalized name the wrapper uses is imported by
    its first candidate public path (:func:`noir_instantiation.std_import_candidates`); a path that does not
    resolve from outside the stdlib is replaced by the name's next candidate; a free function in a private
    module is called through the nearest ancestor module that re-exports it."""
    if os.path.exists(crate):
        shutil.rmtree(crate)
    os.makedirs(os.path.join(crate, "src"))
    with open(os.path.join(crate, "Nargo.toml"), "w", encoding="utf-8") as f:
        f.write('[package]\nname = "boole_wrapper"\ntype = "lib"\n\n[dependencies]\n')
    rel = os.path.relpath(os.path.join(t.root, t.path), os.path.join(t.root, "noir_stdlib", "src"))
    depth = len(NS.mod_path(rel))
    cmd = "nargo export --silence-warnings"
    err = ""
    for up in range(0, depth + 1):
        wr = I.wrapper(t, cand["assign"], cand["wid"], I.std_module_path(rel, up))
        cands = I.std_import_candidates(t, wr.text, rel)
        choice = {n: 0 for n in cands}
        m = None
        line = 0
        for _ in range(4 * len(cands) + 4):
            names = [n for n in cands if choice[n] < len(cands[n])]
            lines = [f"use {cands[n][choice[n]]};" for n in names]
            with open(os.path.join(crate, "src", "lib.nr"), "w", encoding="utf-8") as f:
                f.write("\n".join(lines) + "\n\n" + wr.text)
            r = sh.tc.run_nargo(tag, ["export", "--silence-warnings"], crate, sh.cfg.compile_timeout)
            art = os.path.join(crate, "export", f"{wr.name}.json")
            if r.rc == 0 and os.path.exists(art):
                cand["wrapper"], cand["call"] = wr, wr.call
                cand["std_imports"] = lines
                return Compiled(True, tag, art, None, "", round(time.time() - t0, 2), cmd)
            err = first_error(r.out)
            m = re.search(r"src/lib\.nr:(\d+):", err)
            line = int(m.group(1)) if m else 0
            bad = None
            if 1 <= line <= len(lines) and STD_RESOLUTION.search(err):
                bad = names[line - 1]
            else:
                q = re.search(r"'([A-Z][A-Za-z0-9_]*)'", err)
                if q and q.group(1) in choice and STD_RESOLUTION.search(err):
                    bad = q.group(1)
            if bad is None:
                break
            choice[bad] += 1
        in_wrapper = m is not None and line > len(cands)
        if not (in_wrapper and STD_RESOLUTION.search(err) and t.fn.impl is None):
            break
    return Compiled(False, tag, error=scrub_text(sh, err), secs=round(time.time() - t0, 2), command=cmd)


def assignment_imports(sh: Shared, t: I.Target, assign: dict) -> list[str]:
    """``crate::`` paths for repository types an assignment names that the target file neither declares nor
    imports (the wrapper is appended to that file)."""
    out = []
    idx = sh.indexes.get(t.repo)
    if idx is None or t.crate is None:
        return out
    names = set()
    for v in assign.values():
        names |= set(re.findall(r"(?<![A-Za-z0-9_:])([A-Z][A-Za-z0-9_]*)", str(v)))
    file_text = t.text
    src_root = os.path.join(t.crate, "src")
    for n in sorted(names - I.PRIMITIVES):
        if re.search(r"(?<![A-Za-z0-9_])(struct|type|trait)\s+" + n + r"\b", file_text):
            continue
        if any(re.search(r"(?<![A-Za-z0-9_])" + n + r"(?![A-Za-z0-9_])", u) for u in NS.uses(file_text, top_level=True)):
            continue
        decl = idx.struct_files.get(n, [])
        here = [r for r in decl if os.path.join(t.root, r).startswith(src_root + os.sep)]
        if len(here) == 1:
            mod = NS.mod_path(os.path.relpath(os.path.join(t.root, here[0]), src_root))
            out.append("crate::" + "".join(p + "::" for p in mod) + n)
            continue
        # declared in a path dependency of the crate: `use <dependency name>::<module path>::Name`
        for dep, ddir in direct_path_deps(t.crate).items():
            dsrc = os.path.join(ddir, "src")
            there = [r for r in decl if os.path.join(t.root, r).startswith(dsrc + os.sep)]
            if len(there) == 1:
                mod = NS.mod_path(os.path.relpath(os.path.join(t.root, there[0]), dsrc))
                out.append(dep + "::" + "".join(p + "::" for p in mod) + n)
                break
    return out


def direct_path_deps(crate: str) -> dict[str, str]:
    m = os.path.join(crate, "Nargo.toml")
    if not os.path.exists(m):
        return {}
    return {name: os.path.normpath(os.path.join(crate, spec["path"]))
            for name, spec in T.read_toml(m).get("dependencies", {}).items()
            if isinstance(spec, dict) and "path" in spec}


def decode(sh: Shared, tag: str, art: str, contract_fn: str | None, cwd: str) -> dict:
    args = ["decode", art] + (["--contract-fn", contract_fn] if contract_fn else [])
    r = sh.tc.run_tool(tag, args, cwd, 600)
    if r.rc != 0:
        raise A.Unsupported(f"decoder failed: {r.out[-300:]}")
    return json.loads(r.out)


def contract_fn_is_unconstrained(art: str, name: str) -> bool:
    for f in T.load_json(art).get("functions", []):
        if f["name"] == name:
            return bool(f.get("is_unconstrained"))
    return False


# ------------------------------------------------------------------------------------------ inputs and execution

def sample_value(t: dict, rng: random.Random, profile: str):
    k = t.get("kind")
    p = A.BN254
    if k == "field":
        if profile == "zero":
            return "0"
        if profile == "one":
            return "1"
        if profile == "max":
            return str(p - 1)
        if profile == "small":
            return str(rng.randrange(0, 256))
        return str(rng.randrange(0, p) if rng.random() < 0.5 else rng.randrange(0, 1 << 64))
    if k == "integer":
        w = int(t.get("width", 32))
        bound = 1 << (w - 1 if t.get("sign") == "signed" else w)
        if profile == "zero":
            return "0"
        if profile == "one":
            return "1" if bound > 1 else "0"
        if profile == "max":
            return str(bound - 1)
        if profile == "small":
            return str(rng.randrange(0, min(bound, 16)))
        if profile == "digits" and bound >= 256:
            return str(rng.randrange(0x30, 0x3a))
        if profile == "ascii" and bound >= 256:
            return str(rng.randrange(0x20, 0x7f))
        return str(rng.randrange(0, bound))
    if k == "boolean":
        return {"zero": False, "one": True}.get(profile, rng.random() < 0.5)
    if k == "string":
        n = int(t["length"])
        alphabet = "0123456789" if profile == "digits" else "abcdefghij0123456789"
        return "".join(rng.choice(alphabet) for _ in range(n)) if profile != "zero" else " " * n
    if k == "array":
        return [sample_value(t["type"], rng, profile if profile != "mixed" else rng.choice(PROFILES))
                for _ in range(int(t["length"]))]
    if k == "struct":
        return {f["name"]: sample_value(f["type"], rng, profile if profile != "mixed" else rng.choice(PROFILES))
                for f in t["fields"]}
    if k == "tuple":
        return [sample_value(f, rng, profile if profile != "mixed" else rng.choice(PROFILES)) for f in t["fields"]]
    raise ValueError(f"ABI type {k}")


PROFILES = ["zero", "one", "small", "random", "max", "digits", "ascii"]


def sample_inputs(abi: dict, n: int, seed: str) -> list[dict]:
    rng = random.Random(seed)
    order = ["zero", "one", "small", "small", "random", "max", "digits", "digits", "ascii"] + \
        ["small", "random", "mixed", "digits", "mixed"] * n
    out = []
    for k in range(n):
        prof = order[k]
        out.append({p["name"]: sample_value(p["type"], rng, prof) for p in abi.get("parameters", [])})
    return out


def execute(sh: Shared, tag: str, art: str, contract_fn: str | None, inputs: list[dict], work: str,
            oracle_seed: int = 0) -> list[dict]:
    os.makedirs(work, exist_ok=True)
    inp = os.path.join(work, f"inputs_{oracle_seed}.jsonl")
    outp = os.path.join(work, f"executions_{oracle_seed}.jsonl")
    with open(inp, "w", encoding="utf-8") as f:
        for x in inputs:
            f.write(json.dumps(x) + "\n")
    args = ["execute", art, inp, outp] + (["--contract-fn", contract_fn] if contract_fn else []) + \
        (["--oracle-seed", str(oracle_seed)] if oracle_seed else [])
    r = sh.tc.run_tool(tag, args, work, 1800)
    if not os.path.exists(outp):
        return [{"ok": False, "error": f"executor failed: {r.out[-200:]}"}] * len(inputs)
    with open(outp, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def to_ints(m: dict) -> dict[int, int]:
    return {int(k): int(v, 16) for k, v in m.items()}


def make_bbeval(sh: Shared, tag: str, art: str, contract_fn: str | None, work: str):
    os.makedirs(work, exist_ok=True)
    counter = {"n": 0}

    def bbeval(op: dict, w: list[int]):
        fi, k, off = op["src"]
        values = {}
        for x in op["inputs"]:
            if x[0] == "w":
                values[str(x[1] - off)] = format(w[x[1]], "064x")
        if op["predicate"] is not None and op["predicate"][0] == "w":
            values[str(op["predicate"][1] - off)] = format(w[op["predicate"][1]], "064x")
        counter["n"] += 1
        req = os.path.join(work, f"bb_{counter['n']}.jsonl")
        res = os.path.join(work, f"bb_{counter['n']}.out.jsonl")
        with open(req, "w", encoding="utf-8") as f:
            f.write(json.dumps({"function": fi, "opcode": k, "values": values}) + "\n")
        sh.tc.run_tool(tag, ["bbeval", art, req, res] + (["--contract-fn", contract_fn] if contract_fn else []),
                       work, 120)
        try:
            with open(res, encoding="utf-8") as f:
                rec = json.loads(f.readline())
        except (OSError, ValueError):
            return None
        if not rec.get("ok"):
            return None
        wm = to_ints(rec["witness"])
        try:
            return tuple(wm[o - off] for o in op["outputs"])
        except KeyError:
            return None
    return bbeval


# ------------------------------------------------------------------------------------------ gates

def write_witness(path: str, w: list[int]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(str(x) for x in w) + "\n")


def write_table(path: str, interp: A.Interp, keys: list[str]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        for (key, ins), outs in sorted(interp.table.items(), key=lambda kv: (kv[0][0], kv[0][1])):
            if key in keys:
                f.write(f"{keys.index(key)};{','.join(map(str, ins))};{','.join(map(str, outs))}\n")


def lean_verdicts(sh: Shared, build: str, ns: str, info: dict, files: list[tuple[str, str]], table: str | None,
                  work: str) -> tuple[dict, L.RunResult]:
    src = os.path.join(work, "Fid.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(NE.emit_fid_runner(ns, info["n_conjuncts"], info["bb"], files, table))
    r = L.run_lean(sh.env, NE.LEAN_OPTIONS + ["--json", src], work, 3600, extra_lean_path=[build], rss_limit_mb=16384)
    text = "\n".join(m.get("data", "") for m in L.parse_messages(r.out))
    verdicts = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
    if "FID-DONE" not in text:
        verdicts["__incomplete__"] = "; ".join(L.fmt_msg(m) for m in L.errors(L.parse_messages(r.out))[:3]) or r.out[-300:]
    return verdicts, r


def constrained_witnesses(ops: list[dict]) -> list[int]:
    ws: set[int] = set()
    for op in ops:
        k = op["kind"]
        if k == "assert_zero":
            ws |= op["expr"].witnesses()
        elif k == "range" and op["input"][0] == "w":
            ws.add(op["input"][1])
        elif k in ("and", "xor"):
            ws |= {x[1] for x in (op["lhs"], op["rhs"]) if x[0] == "w"} | {op["output"]}
        elif k == "bb":
            ws |= {x[1] for x in op["inputs"] if x[0] == "w"} | set(op["outputs"])
        elif k == "mem_init":
            ws |= set(op["init"])
        elif k == "mem_op":
            ws |= op["index"].witnesses() | op["value"].witnesses()
    return sorted(ws)


def make_mutants(flat: A.Flat, real: list[list[int]], n: int, seed: str) -> list[list[int]]:
    rng = random.Random(seed + "/mutants")
    cw = constrained_witnesses(flat.opcodes) or list(range(flat.n_witnesses))
    out = []
    for k in range(n):
        if not real:
            break
        w = list(real[k % len(real)])
        j = rng.choice(cw)
        w[j] = (w[j] + rng.choice([1, A.BN254 - 1, rng.randrange(1, A.BN254)])) % A.BN254
        out.append(w)
    return out


def g_fid_nonvac(sh: Shared, build: str, ns: str, info: dict, flat: A.Flat, real: list[list[int]],
                 muts: list[list[int]], oracle: DSN.Oracle, input_free: bool, work: str,
                 nonvac_source: str) -> tuple[G.Gate, G.Gate, dict]:
    wdir = os.path.join(work, "wit")
    os.makedirs(wdir, exist_ok=True)
    files = []
    for k, w in enumerate(real):
        p = os.path.join(wdir, f"real_{k:03d}.txt")
        write_witness(p, w)
        files.append((f"real_{k:03d}", p))
    mut_oracle = []
    for k, m in enumerate(muts):
        p = os.path.join(wdir, f"mut_{k:03d}.txt")
        write_witness(p, m)
        files.append((f"mut_{k:03d}", p))
        ok = oracle.complete(flat.opcodes, m)                    # real black-box values for changed inputs
        mut_oracle.append(ok and A.check(flat.opcodes, m, oracle.interp)[0])
    real_py = [A.check(flat.opcodes, w, oracle.interp)[0] for w in real]
    if not files:
        d = {"real_witnesses": 0, "reason": "no execution produced a witness (see evidence.witness_sampling)"}
        return G.Gate("G-FID", "FAIL", dict(d)), G.Gate("G-NONVAC", "FAIL", dict(d)), {}
    keys = A.bb_keys(flat.opcodes)
    table = os.path.join(work, "bb_table.txt") if info["bb"] else None
    if table:
        write_table(table, oracle.interp, keys)
    verdicts, r = lean_verdicts(sh, build, ns, info, files, table, work)
    real_accept = sum(verdicts.get(f"real_{k:03d}") == "ACCEPT" for k in range(len(real)))
    agree = sum(verdicts.get(f"mut_{k:03d}") == ("ACCEPT" if mut_oracle[k] else "REJECT") for k in range(len(muts)))
    detail = {"method": "Lean #eval of decide (Constraints ...) on every witness file (black boxes: the table of real "
                        "values), compared with the Python evaluator of the decoded ACIR under the same values",
              "real_witnesses": len(real), "real_lean_accept": real_accept, "real_python_accept": sum(real_py),
              "mutants": len(muts), "mutants_python_reject": sum(not x for x in mut_oracle),
              "mutants_lean_agree": agree, "lean_eval_secs": r.secs, "bb_table_entries": len(oracle.interp.table)}
    if "__incomplete__" in verdicts:
        detail["error"] = verdicts["__incomplete__"][:400]
        detail["reason"] = "the Lean evaluation did not complete (harness error); no verdict is inferred"
        return G.Gate("G-FID", "ERROR", dict(detail)), G.Gate("G-NONVAC", "ERROR", dict(detail)), {}
    need_real = 1 if input_free else G.FID_MIN_REAL
    fid_ok = (real_accept == len(real) and sum(real_py) == len(real) and len(real) >= need_real
              and len(muts) >= G.FID_MIN_MUTANTS and agree == len(muts))
    fd = dict(detail, required_real=need_real, input_free=input_free)
    if not fid_ok:
        reasons = []
        if len(real) < need_real:
            reasons.append(f"only {len(real)} distinct real witnesses (need {need_real})")
        if real_accept != len(real) or sum(real_py) != len(real):
            reasons.append("the Lean model or the Python evaluator rejected a real witness")
        if agree != len(muts) or len(muts) < G.FID_MIN_MUTANTS:
            reasons.append("Lean and Python verdicts differ on a mutant (or too few mutants)")
        fd["reason"] = "; ".join(reasons)
    nonvac_ok = real_accept >= 1 and sum(real_py) >= 1
    nv = G.Gate("G-NONVAC", "PASS" if nonvac_ok else "FAIL",
                {"real_witnesses_accepted": real_accept, "witness_source": nonvac_source})
    return G.Gate("G-FID", "PASS" if fid_ok else "FAIL", fd), nv, {"verdicts": verdicts}


def g_triv(sh: Shared, build: str, ns: str, info: dict, work: str) -> G.Gate:
    """The P1 battery (V0-V2, 33 forms) and the P2 forms (V3-V4, 7 forms) on the Noir statement."""
    cfg = sh.cfg
    os.makedirs(work, exist_ok=True)
    # P1 in one file (V0-V2, 33 forms; one Mathlib import instead of three), P2 in a second file
    files = [("Battery_P1", [(v, t) for v in E.BATTERY_VARIANTS for t in E.BATTERY_TACTICS]),
             ("Battery_P2", [(v, t) for v, ts in E.BATTERY_P2 for t in ts])]
    results, runs = {}, []

    def emit(forms):
        return NE.emit_battery_forms(ns, info["n_conjuncts"], forms, cfg.battery_heartbeats, info["bb"], info["memory"])

    for fname, forms in files:
        text = emit(forms)
        path = os.path.join(work, f"{fname}.lean")
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        r = L.run_lean(sh.env, NE.LEAN_OPTIONS + ["--json", path], work, cfg.battery_file_wall,
                       extra_lean_path=[build], rss_limit_mb=16384)
        runs.append({"variant": fname.replace("Battery_", ""), "secs": r.secs, "timeout": r.timeout,
                     "memkill": r.memkill, "peak_rss_mb": r.peak_rss_mb})
        for name, c in G.classify_battery(text, L.parse_messages(r.out), r.timeout or r.memkill).items():
            if c["status"] == "timeout":
                form = next(fm for fm in forms if E.battery_theorem_name(*fm) == name)
                single = emit([form])
                spath = os.path.join(work, f"Single_{name}.lean")
                with open(spath, "w", encoding="utf-8") as f:
                    f.write(single)
                rs = L.run_lean(sh.env, NE.LEAN_OPTIONS + ["--json", spath], work, cfg.battery_single_wall,
                                extra_lean_path=[build], rss_limit_mb=16384)
                c = G.classify_battery(single, L.parse_messages(rs.out), rs.timeout or rs.memkill)[name]
                c["rerun_single"] = {"secs": rs.secs, "timeout": rs.timeout, "memkill": rs.memkill}
            results[name] = c
    budget = {"maxHeartbeats": cfg.battery_heartbeats, "file_wall_s": cfg.battery_file_wall,
              "single_wall_s": cfg.battery_single_wall}
    return G._triv_gate(results, runs, budget, G.BATTERY_P1_P2)


# ------------------------------------------------------------------------------------------ records

def dir_name_for(t: I.Target, args: dict) -> str:
    stem = re.sub(r"\.nr$", "", t.path).replace("/", ".")
    sym = re.sub(r"[^A-Za-z0-9]+", "_", t.fn.name)
    imp = t.fn.impl
    if imp is not None:
        base = re.match(r"\s*([A-Za-z_][A-Za-z0-9_]*)", imp.self_type)
        owner = (imp.trait.split("<")[0].split("::")[-1] + "_for_" if imp.trait else "") + (
            re.sub(r"[^A-Za-z0-9]+", "_", imp.self_type).strip("_")[:40] if base is None else
            re.sub(r"[^A-Za-z0-9]+", "_", imp.self_type).strip("_")[:40])
        sym = f"{owner}.{sym}"
    name = f"{stem}.{sym}.L{t.fn.line}"
    if args:
        name += "." + P.args_slug([f"{k}{v}" for k, v in sorted(args.items())])
    name = re.sub(r"[^A-Za-z0-9._-]", "_", name)
    if len(name) > 180:
        name = name[:150] + ".h" + hashlib.sha256(name.encode()).hexdigest()[:12]
    return name


def base_record(sh: Shared, t: I.Target | None, row: dict, dir_name: str) -> dict:
    rid = repo_id_of(row["repo"])
    repo = sh.cfg.repos.get(rid, {})
    src_sha = "0" * 64
    if t is not None:
        src_sha = hashlib.sha256(t.src.encode("utf-8")).hexdigest()
    census = row.get("census") or [{}]
    return {
        "schema_version": P.SCHEMA_VERSION,
        "package_id": f"{repo_dir_name(rid)}/{dir_name}",
        "property": dict(P.DET_PROPERTY),
        "status": "NO-INSTANTIATION", "status_reason": "",
        "ids": {"ledger_item_id": row["item_id"],
                "ledger": {"file": "ledger/LEDGER-v1.jsonl", "sha256": sh.ledger_sha, "row_found": True},
                "repo": rid, "repo_url": row["repo"], "release": str(census[0].get("pin") or repo.get("release", "")),
                "commit": row["commit"], "path": row["path"], "template": row["symbol"],
                "template_line": int(t.fn.line if t is not None else (census[0].get("line") or 1)),
                "source_sha256": src_sha},
        "instantiation": {"rule": "none", "args": [], "call": "", "provenance": [], "selection": "", "candidates": []},
        "spec": dict(NOIR_DET_SPEC),
        "env": {"nargo": "", "lean": sh.env.lean_version, "mathlib": sh.env.packages.get("mathlib", ""),
                "lake_manifest_sha256": sh.env.manifest_sha256, "packages": dict(sorted(sh.env.packages.items())),
                "python": platform.python_version()},
        "generator": dict(sh.generator),
        "evidence": {"coverage": coverage_of(row)},
    }


def coverage_of(row: dict) -> dict:
    """Public machine-checked coverage of the function at the pin (from the ledger's census record)."""
    cov = row.get("coverage", "none")
    census = (row.get("census") or [{}])[0]
    raw = census.get("coverage_raw", "")
    if cov == "conditional":
        return {"class": "partial", "same_pin": False, "source": "reilabs/lampe 919a3871 (Noir stdlib proofs at "
                "v1.0.0-beta.19; the function text is identical at the v1.0.0-rc.2 pin)",
                "property": "Hoare-triple specifications over Lampe's Noir source semantics (not a DET statement over "
                            "the compiled ACIR); open soundness issue reilabs/lampe#314",
                "ledger_coverage": cov, "census": raw, "census_source": census.get("coverage_source", "")}
    if raw and "partial" in raw:
        return {"class": "none", "same_pin": False, "source": raw, "ledger_coverage": cov,
                "census_source": census.get("coverage_source", ""),
                "note": "repository-level coverage note; no proof of this function is recorded"}
    return {"class": "none", "ledger_coverage": cov, "census": raw}


def candidate_entry(c: dict) -> dict:
    e = {"tier": c["tier"], "call": c.get("call", ""), "compile": c.get("compile", "skipped")}
    for k in ("compiler", "attempts", "constraints", "wires", "error"):
        if c.get(k) not in (None, "", []):
            e[k] = c[k]
    return e


def finish(rec: dict, t0: float) -> dict:
    rec["evidence"]["wall_s"] = round(time.time() - t0, 2)
    return rec


# ------------------------------------------------------------------------------------------ one item

def process(sh: Shared, row: dict, t: I.Target | None, how: str) -> dict:
    t0 = time.time()
    try:
        return finish(_process(sh, row, t, how), t0)
    except Exception as ex:                                   # recorded, never dropped
        rec = base_record(sh, t, row, f"error.{hashlib.sha256(row['item_id'].encode()).hexdigest()[:16]}")
        rec["status"] = "COMPILE-FAIL"
        rec["status_reason"] = scrub_text(sh, f"generator error: {type(ex).__name__}: {ex}")[:500]
        return finish(rec, t0)


def _process(sh: Shared, row: dict, t: I.Target | None, how: str) -> dict:
    cfg = sh.cfg
    if t is None:
        rec = base_record(sh, None, row, f"missing.{hashlib.sha256(row['item_id'].encode()).hexdigest()[:16]}")
        rec["status"], rec["status_reason"] = "NO-INSTANTIATION", f"source not located: {how}"
        return rec
    plan, reason = I.classify(t)
    dir0 = dir_name_for(t, {})
    rec = base_record(sh, t, row, dir0)
    rec["evidence"]["locate"] = how
    rec["evidence"]["plan"] = plan
    compilers, pin_note = T.REPO_COMPILERS[t.repo]
    rec["evidence"]["compiler_selection"] = {"attempt_order": compilers, "pin": pin_note}
    if plan in ("NOT-APPLICABLE", "NO-INSTANTIATION"):
        rec["status"], rec["status_reason"] = plan, reason
        return rec
    if t.crate is None:
        rec["status"], rec["status_reason"] = "NO-INSTANTIATION", "no Nargo package contains the source file"
        return rec
    if not in_module_tree(t):
        rec["status"] = "NOT-APPLICABLE"
        rec["status_reason"] = ("the source file is not part of its crate's module tree at the pin (no `mod` "
                                "declaration reaches it), so the function is never compiled")
        return rec
    rec["ids"]["dependencies"] = deps_of(sh, t)
    idx = sh.indexes.get(t.repo)
    if plan in ("export", "std-export"):
        cands = I.candidates(t, idx)
        if t.fn.impl is not None and t.fn.impl.kind == "trait":
            cands = trait_default_candidates(t, idx, cands)
    elif plan == "bin-main":
        cands = [("repo-main", {}, [f"{os.path.relpath(t.crate, t.root)}: fn main of the binary crate"])]
    else:
        cands = [("contract-entrypoint", {}, [f"{os.path.relpath(t.crate, t.root)}: contract artifact function "
                                              f"{t.fn.name}"])]
    rec["instantiation"]["rule_order"] = list(dict.fromkeys(c[0] for c in cands))
    item_work = os.path.join(cfg.work, hashlib.sha256(row["item_id"].encode()).hexdigest()[:20])
    os.makedirs(item_work, exist_ok=True)
    results = []
    chosen = None
    for tier in rec["instantiation"]["rule_order"]:
        tier_res = []
        for k, (ctier, assign, prov) in enumerate(c for c in cands if c[0] == tier):
            cand = {"tier": ctier, "assign": assign, "provenance": prov}
            wid = hashlib.sha256(f"{row['item_id']}|{sorted(assign.items())}".encode()).hexdigest()[:10]
            cand["wid"] = wid
            if plan in ("export", "std-export"):
                try:
                    qualify = I.std_module_path(os.path.relpath(os.path.join(t.root, t.path),
                                                                os.path.join(t.root, "noir_stdlib", "src"))) \
                        if plan == "std-export" else ""
                    cand["wrapper"] = I.wrapper(t, assign, wid, qualify)    # (stdlib: rebuilt by compile_std)
                    cand["call"] = cand["wrapper"].call
                except (ValueError, KeyError) as ex:
                    cand.update(compile="skipped", error=f"no wrapper: {ex}")
                    tier_res.append(cand)
                    continue
            else:
                cand["call"] = t.fn.name
            attempts = []
            first_errors = []
            comp = None
            for tag in compilers:
                comp = compile_candidate(sh, t, plan, cand, os.path.join(item_work, f"c{len(results)}_{k}_{tag}"), tag)
                first_errors.append(comp.error)
                attempts.append({"include_context": os.path.relpath(t.crate, t.root), "compiler": tag,
                                 "result": "ok" if comp.ok else comp.error[:200]})
                if comp.ok:
                    break
            cand["compiler"] = comp.compiler
            if len(attempts) > 1:
                cand["attempts"] = attempts
            if not comp.ok:
                # the pinned compiler's own error is the candidate's error (every attempt is kept above)
                cand.update(compile="error", compiler=attempts[0]["compiler"], error=first_errors[0])
                tier_res.append(cand)
                continue
            if plan == "contract-fn" and contract_fn_is_unconstrained(comp.artifact, comp.contract_fn):
                rec["status"] = "NOT-APPLICABLE"
                rec["status_reason"] = "the contract artifact compiles this function as unconstrained (Brillig), no ACIR"
                rec["instantiation"]["candidates"] = [candidate_entry(dict(cand, compile="ok"))]
                return rec
            try:
                decoded = decode(sh, comp.compiler, comp.artifact, comp.contract_fn, item_work)
                prog = A.normalize_program(decoded)
                flat = A.flatten(prog, max_ops=SIZING_LIMIT)
                cand.update(compile="ok", constraints=flat.size, wires=max(1, flat.n_witnesses))
                cand["_compiled"], cand["_prog"], cand["_flat"], cand["_decoded"] = comp, prog, flat, decoded
            except OverflowError:
                cand.update(compile="ok", constraints=SIZING_LIMIT + 1, wires=1,
                            error=f"flattened ACIR above {SIZING_LIMIT} opcodes (size is a lower bound)")
                cand["_compiled"] = comp
            except A.Unsupported as ex:
                cand.update(compile="error", error=f"ACIR not modelled: {ex}")
                cand["_unsupported"] = True
            tier_res.append(cand)
        results += tier_res
        ok = [c for c in tier_res if c.get("compile") == "ok"]
        if ok:
            within = [c for c in ok if c["constraints"] <= MAX_OPCODES and "_flat" in c]
            if within:
                chosen = max(within, key=lambda c: (c["constraints"], c["wid"]))
                sel = f"tier {tier}: largest compiled instantiation within {MAX_OPCODES} opcodes"
            else:
                chosen = min(ok, key=lambda c: c["constraints"])
                sel = f"tier {tier}: every compiled instantiation exceeds {MAX_OPCODES} opcodes; smallest recorded"
            rec["instantiation"]["selection"] = sel
            break
    rec["instantiation"]["candidates"] = [candidate_entry(c) for c in results]
    if chosen is None:
        errs = [c for c in results if c.get("compile") in ("error", "skipped")]
        non_abi = [c for c in errs if "cannot exist as a parameter to main" in c.get("error", "")
                   or "Only sized types may be used in the entry point" in c.get("error", "")]
        if not results:
            rec["status"], rec["status_reason"] = "NO-INSTANTIATION", "no candidate instantiation"
        elif errs and len(non_abi) == len(errs):
            rec["status"] = "NO-INSTANTIATION"
            rec["status_reason"] = ("every candidate's parameter types contain a type that is not an ABI type "
                                    "(function/closure or unsized), so no circuit takes them as inputs: "
                                    + non_abi[0]["error"][:200])
        elif all(c.get("_unsupported") for c in errs if c.get("compile") == "error") and any(
                c.get("_unsupported") for c in errs):
            rec["status"] = "COMPILE-FAIL"
            rec["status_reason"] = next(c["error"] for c in errs if c.get("_unsupported"))
        else:
            first = next((c for c in errs if c.get("compile") == "error"), errs[0] if errs else None)
            rec["status"] = "COMPILE-FAIL"
            rec["status_reason"] = (f"{len(results)} candidate(s) failed; first ({first['tier']}): "
                                    f"{first.get('error', '')}")[:600]
        cleanup(sh, item_work)
        return rec
    assign = chosen["assign"]
    rec["instantiation"].update(rule=chosen["tier"], args=[f"{k}={v}" for k, v in sorted(assign.items())],
                                params=[g.name for _, g in t.generics], call=chosen.get("call", ""),
                                provenance=list(chosen["provenance"]))
    comp: Compiled = chosen["_compiled"]
    if assign:
        rec["package_id"] = f"{repo_dir_name(t.repo)}/{dir_name_for(t, assign)}"
    tc_info = sh.tc.binary_info(comp.compiler)
    rec["env"]["nargo"] = f"{T.COMPILERS[comp.compiler].version} ({comp.compiler})"
    rec["env"]["nargo_binary_sha256"] = tc_info["nargo_sha256"]
    rec["env"]["acir_tool_sha256"] = tc_info["acir_tool_sha256"]
    if "wrapper" in chosen:
        rec["instantiation"]["main_sha256"] = hashlib.sha256(chosen["wrapper"].text.encode()).hexdigest()
    if chosen.get("std_imports"):
        rec["evidence"]["std_imports"] = chosen["std_imports"]
    if chosen.get("imports"):
        rec["evidence"]["wrapper_imports"] = chosen["imports"]
    flat = chosen.get("_flat")
    prog = chosen.get("_prog")
    circuit = {"compiler": {"name": "nargo", "version": T.COMPILERS[comp.compiler].version,
                            "flags": comp.command.split()[1:], "binary_sha256": tc_info["nargo_sha256"],
                            "source": T.COMPILERS[comp.compiler].source},
               "prime": str(A.BN254), "prime_name": "bn254", "n_constraints": int(chosen["constraints"]),
               "n_wires": int(chosen["wires"]),
               "size_policy": {"max_constraints": MAX_OPCODES, "within": chosen["constraints"] <= MAX_OPCODES}}
    if flat is not None:
        circuit.update(n_inputs=len(flat.inputs), n_outputs=len(flat.outputs))
        circuit["acir"] = {"sha256": prog.raw_sha256, "functions": len(prog.functions),
                           "opcodes": A.stats(flat.opcodes), "black_boxes": A.bb_keys(flat.opcodes),
                           "brillig_calls": sum(1 for op in flat.opcodes if op["kind"] == "brillig"),
                           "brillig_functions": prog.n_brillig, "oracles": prog.oracles,
                           "noir_version": prog.noir_version, "flattened_calls": len(flat.call_sites)}
    rec["circuit"] = circuit
    if flat is None or chosen["constraints"] > MAX_OPCODES:
        rec["status"] = "TOO-LARGE"
        rec["status_reason"] = (f"{chosen['constraints']} ACIR opcodes (flattened) > {MAX_OPCODES}"
                                + (" (lower bound)" if flat is None else ""))
        rec["evidence"]["callees"] = callee_names(t)
        cleanup(sh, item_work)
        return rec
    return build_package(sh, rec, t, chosen, item_work)


def trait_default_candidates(t: I.Target, idx, cands):
    """A trait's default method needs an implementing type: the repository's non-generic impls of the trait."""
    imp = t.fn.impl
    out = []
    if idx is not None:
        for st in idx.trait_impls.get(imp.self_type, [])[:I.MAX_CANDIDATES_PER_TIER]:
            for tier, a, prov in cands:
                if any(g.name in a for g in imp.generics) or not imp.generics:
                    out.append(("repo-derived" if tier != "probed" else tier, dict(a, __implementor__=st),
                                prov + [f"implementing type {st} (impl {imp.self_type} for {st} in the repository)"]))
    return out


def callee_names(t: I.Target) -> list[str]:
    body = t.text[t.fn.body_start:t.fn.end]
    names = re.findall(r"(?<![A-Za-z0-9_])([a-z_][A-Za-z0-9_]*)\s*(?:::\s*<[^()]*>)?\s*\(", body)
    kw = {"if", "for", "while", "assert", "assert_eq", "match", "return", "let", "fn", "else", "in"}
    return sorted(set(n for n in names if n not in kw))[:200]


_PREFETCH_LOCK = threading.Lock()


def deps_of(sh: Shared, t: I.Target) -> list[dict]:
    with _PREFETCH_LOCK:                       # one git fetch at a time (shared nargo cache)
        if t.crate in sh.deps:
            return sh.deps[t.crate]
        recs = T.prefetch_git_deps(t.crate, sh.cfg.home, pins=sh.cfg.git_pins or None) if not t.stdlib else {}
        out = [scrub_dep(sh, r.to_json()) for r in recs.values() if r is not None]
        sh.deps[t.crate] = out
        return out


def scrub_dep(sh: Shared, d: dict) -> dict:
    d = dict(d)
    d["path"] = scrub_text(sh, d["path"])
    return d


def cleanup(sh: Shared, item_work: str) -> None:
    if not sh.cfg.keep_work and item_work.startswith(sh.cfg.work + os.sep):
        shutil.rmtree(item_work, ignore_errors=True)


def build_package(sh: Shared, rec: dict, t: I.Target, chosen: dict, work: str) -> dict:
    cfg = sh.cfg
    comp: Compiled = chosen["_compiled"]
    flat: A.Flat = chosen["_flat"]
    prog: A.Program = chosen["_prog"]
    decoded = chosen["_decoded"]
    dir_name = rec["package_id"].split("/", 1)[1]
    collection = rec["package_id"].split("/", 1)[0]
    ns = P.lean_namespace(collection, dir_name)
    names = {i: f"param `{n}`" for i, n in enumerate(NE.abi_witness_names(prog.abi)) if i in set(flat.inputs)}
    for k, o in enumerate(flat.outputs):
        names[o] = f"return value [{k}]"
    meta = {"repo_id": t.repo, "instantiation": rec["instantiation"]["call"] or t.fn.name,
            "generator": f"{sh.generator['name']} v{sh.generator['version']}", "repo_url": rec["ids"]["repo_url"],
            "commit": rec["ids"]["commit"], "path": t.path, "template": t.fn.name,
            "rule": rec["instantiation"]["rule"], "nargo_version": rec["env"]["nargo"],
            "nargo_command": comp.command, "acir_sha256": prog.raw_sha256}
    stage = os.path.join(work, "pkg")
    shutil.rmtree(stage, ignore_errors=True)
    model_rel = NE.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    try:
        model, info = NE.emit_model(ns, meta, flat, names)
    except A.Unsupported as ex:
        rec.pop("circuit", None)
        rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"ACIR not modelled: {ex}"
        return rec
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(model)
    statement_text = NE.emit_statement(ns, meta, info["bb"])
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{NE.STATEMENT_THEOREM}"
    n_brillig = rec["circuit"]["acir"]["brillig_calls"]
    rec["statement"] = {"file": "Statement.lean", "theorem": NE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": NE.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": statement_assumptions(info["black_boxes"], info["memory"], n_brillig),
                        "truth": "unknown"}
    gates: dict = {}
    evidence = rec["evidence"]

    # executions (the compiler's ACVM solver through boole-acir-tool)
    inputs = sample_inputs(prog.abi, SAMPLES, rec["package_id"])
    sources = ["sampled"] * len(inputs)
    prover = os.path.join(t.crate, "Prover.toml")
    if comp.contract_fn is None and os.path.exists(prover) and rec["instantiation"]["rule"] == "repo-main":
        inputs.insert(0, {"__prover_toml__": prover})
        sources.insert(0, "repository Prover.toml")
    execs = execute(sh, comp.compiler, comp.artifact, comp.contract_fn, inputs, os.path.join(work, "exec"))
    real, seen, errors, oracles = [], set(), {}, set()
    real_sources, real_inputs = [], []
    for src_kind, ex, inp in zip(sources, execs, inputs):
        if not ex.get("ok"):
            key = re.sub(r"\d{3,}", "N", ex.get("error", ""))[:120]
            errors[key] = errors.get(key, 0) + 1
            continue
        oracles |= set(ex.get("oracles", []))
        w = A.flat_witness(flat, to_ints(ex["witness"]), [dict(c, witness=to_ints(c["witness"])) for c in ex["calls"]])
        if w is None:
            errors["call record missing"] = errors.get("call record missing", 0) + 1
            continue
        key = tuple(w)
        if key in seen:
            continue
        seen.add(key)
        real.append(w)
        real_sources.append(src_kind)
        real_inputs.append(inp)
    evidence["witness_sampling"] = {"attempts": len(inputs), "executed_ok": sum(1 for e in execs if e.get("ok")),
                                    "distinct_witnesses": len(real), "errors": dict(sorted(errors.items())[:8]),
                                    "oracles_answered_with_zero": sorted(oracles),
                                    "repository_inputs": "repository Prover.toml" in sources,
                                    "repository_inputs_executed": "repository Prover.toml" in real_sources}
    interp = A.Interp()
    for w in real:
        for key, ins, outs, _ in A.bb_calls(flat.opcodes, w):
            interp.add(key, ins, outs)
    oracle = DSN.Oracle(interp, make_bbeval(sh, comp.compiler, comp.artifact, comp.contract_fn, os.path.join(work, "bb")))
    real_w = real[:REAL_WANTED]
    muts = make_mutants(flat, real_w, MUTANTS, rec["package_id"])

    # G-ELAB
    build = os.path.join(work, "build")
    shutil.rmtree(build, ignore_errors=True)
    elab = G.g_elab(sh.env, stage, build, ns, os.path.join(work, "elab"))
    gates["G-ELAB"] = elab.to_json()
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build, model_rel[:-len(".lean")] + ".olean"))
    input_free = not flat.inputs
    nonvac_source = ("the compiler's own ACVM solver (noir " + T.COMPILERS[comp.compiler].version +
                     ", via boole-acir-tool) on " + ("the repository's Prover.toml and " if "repository Prover.toml" in
                                                    real_sources else "") + "derived inputs")
    if model_ok:
        fid, nonvac, _ = g_fid_nonvac(sh, build, ns, info, flat, real_w, muts, oracle, input_free,
                                      os.path.join(work, "fid"), nonvac_source)
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()

    # DET search
    ce, slog = DSN.search(flat, real, oracle, rec["package_id"], cfg.det_search_budget_s) if real else \
        (None, {"bases": 0})
    if ce is None and real and (oracles - {"print"}) and flat.outputs:
        ce = oracle_variation(sh, comp, flat, real, real_inputs, oracle, os.path.join(work, "oracle-var"), slog)
    det_gate = {"status": "PASS", "truth": "unknown", "log": slog,
                "note": "PASS means no counterexample was found by the cheap searches; DET truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        write_witness(os.path.join(cdir, "w1.txt"), ce.base)
        write_witness(os.path.join(cdir, "w2.txt"), ce.other)
        table = None
        if info["bb"]:
            table = os.path.join(cdir, "bb_table.txt")
            write_table(table, oracle.interp, A.bb_keys(flat.opcodes))
        verdicts, _ = (lean_verdicts(sh, build, ns, info, [("w1", os.path.join(cdir, "w1.txt")),
                                                          ("w2", os.path.join(cdir, "w2.txt"))], table, cdir)
                       if model_ok else ({}, None))
        lean_ok = verdicts.get("w1") == "ACCEPT" and verdicts.get("w2") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if lean_ok else "unknown",
                    "method": ce.method, "changed_outputs": ce.changed_outputs[:20],
                    "lean_confirms_both_witnesses": lean_ok, "python_confirms": True, "log": slog,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"] +
                             (["evidence/counterexample/bb_table.txt"] if table else [])}
        evidence["counterexample_dir"] = cdir
    gates["DET-SEARCH"] = det_gate

    # G-TRIV
    if elab.status != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement did not elaborate"}
    elif det_gate["truth"] == "false-counterexample-found":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement refuted by a confirmed counterexample"}
    elif not flat.outputs:
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "no return witnesses: the statement is vacuous"}
    elif gates["G-NONVAC"]["status"] != "PASS" or gates["G-FID"]["status"] != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "not run: the package already fails G-NONVAC or G-FID"}
    else:
        gates["G-TRIV"] = g_triv(sh, build, ns, info, os.path.join(work, "triv")).to_json()
    rec["gates"] = gates
    set_status(rec, gates, bool(flat.outputs))
    ref = elab.detail["reference_type_sha256"] if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": NE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(NE.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": P.sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": NE.model_module(ns),
                                 "sha256": P.sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    write_package(sh, rec, stage, work, chosen, decoded, flat, real_w)
    cleanup(sh, work)
    return rec


def oracle_variation(sh: Shared, comp: "Compiled", flat: A.Flat, real: list[list[int]], inputs: list[dict],
                     oracle: DSN.Oracle, work: str, slog: dict):
    """Re-execute the witnesses' inputs with pseudo-random oracle answers (unconstrained prover hints): an
    execution with the same parameters and different return values is a counterexample made of two real
    executions (confirmed like every other candidate)."""
    base = {tuple(w[i] for i in flat.inputs): w for w in real}
    tried = 0
    for seed in (1, 2, 3):
        execs = execute(sh, comp.compiler, comp.artifact, comp.contract_fn, inputs[:16], work, oracle_seed=seed)
        for ex in execs:
            if not ex.get("ok"):
                continue
            tried += 1
            w2 = A.flat_witness(flat, to_ints(ex["witness"]),
                                [dict(c, witness=to_ints(c["witness"])) for c in ex["calls"]])
            if w2 is None:
                continue
            w1 = base.get(tuple(w2[i] for i in flat.inputs))
            if w1 is None:
                continue
            for w in (w1, w2):
                for key, ins, outs, _ in A.bb_calls(flat.opcodes, w):
                    oracle.interp.add(key, ins, outs)
            diff = DSN.confirm(flat, w1, w2, oracle)
            if diff:
                slog["oracle_variation"] = f"found (seed {seed})"
                return DSN.Counterexample("oracle-variation", w1, w2, diff)
    slog["oracle_variation"] = f"no-counterexample(executions={tried})"
    return None


def set_status(rec: dict, gates: dict, has_outputs: bool) -> None:
    det_gate = gates["DET-SEARCH"]
    if det_gate["truth"] == "false-counterexample-found":
        rec["status"] = "DET-FALSE-CANDIDATE"
        rec["status_reason"] = (f"{det_gate['method']} found two constraint-satisfying assignments with equal parameters "
                                f"and different return values (confirmed by the Python evaluator and the Lean model); "
                                f"private finding")
        if rec["instantiation"]["rule"] == "probed":
            det_gate["finding"] = PROBED_NOT_A_FINDING
            rec["status_reason"] += f"; {PROBED_NOT_A_FINDING}"
        rec["statement"]["truth"] = "false-counterexample-found"
        return
    failed = [g for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV") if gates[g]["status"] != "PASS"]
    if gates["G-TRIV"].get("status") == "SKIPPED" and len(failed) > 1:
        failed = [g for g in failed if g != "G-TRIV"]
    if not has_outputs:
        failed = [g for g in failed if g != "G-TRIV"] + ["NO-OUTPUTS"]
    if det_gate["status"] == "FAIL":
        failed.append("DET-SEARCH(unconfirmed-in-Lean)")
    if failed:
        rec["status"] = "GATE-FAIL"
        reasons = []
        for g in failed:
            gd = gates.get(g, {})
            if g == "NO-OUTPUTS":
                reasons.append("the function has no return values (and no &mut parameters), so DET is vacuous")
            elif g == "G-TRIV" and gd.get("closed_by"):
                reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
                rec["statement"]["truth"] = "closed-by-automation"
            else:
                reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:600]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def write_package(sh: Shared, rec: dict, stage: str, work: str, chosen: dict, decoded: dict, flat: A.Flat,
                  real: list[list[int]]) -> None:
    dest = os.path.join(sh.cfg.out, rec["package_id"].split("/", 1)[0], rec["package_id"].split("/", 1)[1])
    if os.path.exists(dest):
        shutil.rmtree(dest)
    os.makedirs(dest)
    shutil.copyfile(os.path.join(stage, "Statement.lean"), os.path.join(dest, "Statement.lean"))
    model_rel = rec["statement"]["model_file"]
    os.makedirs(os.path.dirname(os.path.join(dest, model_rel)), exist_ok=True)
    shutil.copyfile(os.path.join(stage, model_rel), os.path.join(dest, model_rel))
    ev = os.path.join(dest, "evidence")
    os.makedirs(ev)
    if "wrapper" in chosen:
        with open(os.path.join(ev, "wrapper.nr"), "w", encoding="utf-8") as f:
            f.write(f"// appended to {rec['ids']['path']} (pinned commit {rec['ids']['commit']})\n"
                    + chosen["wrapper"].text)
    with open(os.path.join(ev, "acir.json"), "w", encoding="utf-8") as f:
        json.dump({"noir_version": decoded.get("noir_version"), "abi": decoded.get("abi"),
                   "functions": decoded.get("functions")}, f, sort_keys=True, separators=(",", ":"))
    os.makedirs(os.path.join(ev, "witnesses"))
    for k, w in enumerate(real[:4]):
        write_witness(os.path.join(ev, "witnesses", f"real_{k:03d}.txt"), w)
    for name in ("elab/reference.type.txt",):
        src = os.path.join(work, name)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(ev, os.path.basename(name)))
    if "counterexample_dir" in rec["evidence"]:
        src = rec["evidence"].pop("counterexample_dir")
        os.makedirs(os.path.join(ev, "counterexample"))
        for fn in ("w1.txt", "w2.txt", "bb_table.txt"):
            if os.path.exists(os.path.join(src, fn)):
                shutil.copyfile(os.path.join(src, fn), os.path.join(ev, "counterexample", fn))
    triv = os.path.join(work, "triv")
    if os.path.isdir(triv):
        os.makedirs(os.path.join(ev, "triv"))
        for fn in sorted(os.listdir(triv)):
            if fn.endswith(".lean"):
                shutil.copyfile(os.path.join(triv, fn), os.path.join(ev, "triv", fn))
    P.write_json(os.path.join(dest, "problem.json"), rec)


# ------------------------------------------------------------------------------------------ wave

def run_wave(cfg: WaveConfig, log_every: int = 25) -> list[dict]:
    os.makedirs(cfg.out, exist_ok=True)
    os.makedirs(cfg.work, exist_ok=True)
    env = L.load_env(cfg.lean_env)
    tc = T.Toolchain(cfg.tools_root, cfg.home, cfg.scratch)
    sh = Shared(cfg, env, tc, P.sha256_file(cfg.ledger), generator_info())
    for rid, repo in cfg.repos.items():
        head = T._git(["rev-parse", "HEAD"], repo["dir"]).stdout.strip()
        if head != repo["commit"]:
            raise ValueError(f"{rid} is at {head}, the wave pins {repo['commit']}")
    rows = select_rows(cfg.ledger)
    if cfg.only:
        rows = [r for r in rows if r["item_id"] in set(cfg.only)]
    for rid, repo in cfg.repos.items():
        sh.indexes[rid] = I.RepoIndex.build(repo["dir"], nr_files(repo["dir"]))
    cache: dict = {}
    targets = [(r,) + make_target(sh, r, cache) for r in rows]
    # content dedup
    groups: dict[str, list] = {}
    keys = {}
    for row, t, how in targets:
        if t is None:
            keys[row["item_id"]] = "missing:" + row["item_id"]
        else:
            comp = T.REPO_COMPILERS[t.repo][0][0]
            keys[row["item_id"]] = content_key(sh, t, comp)
        groups.setdefault(keys[row["item_id"]], []).append((row, t, how))
    canon = []
    dedup = []
    for key, members in groups.items():
        members.sort(key=lambda m: m[0]["item_id"])
        canon.append(members[0])
        for m in members[1:]:
            dedup.append({"item_id": m[0]["item_id"], "canonical_item_id": members[0][0]["item_id"],
                          "content_sha256": key, "relation": "duplicate-of-n1"})
    log(sh, f"selection {len(rows)} rows, {len(canon)} distinct by content, {len(dedup)} duplicates")
    records: list[dict] = []
    done = 0
    with cf.ThreadPoolExecutor(max_workers=cfg.jobs) as pool:
        futs = {pool.submit(process, sh, row, t, how): row for row, t, how in canon}
        for fut in cf.as_completed(futs):
            rec = fut.result()
            rec["evidence"]["content_sha256"] = keys[futs[fut]["item_id"]]
            records.append(rec)
            done += 1
            append_jsonl(os.path.join(cfg.out, "PROGRESS.jsonl"), rec)
            if done % log_every == 0 or done == len(canon):
                log(sh, f"{done}/{len(canon)} done")
    by_id = {r["ids"]["ledger_item_id"]: r for r in records}
    for d in dedup:
        c = by_id.get(d["canonical_item_id"])
        d["canonical_package_id"] = c["package_id"] if c else None
        d["canonical_status"] = c["status"] if c else None
    write_outputs(cfg.out, records, dedup)
    return records


def append_jsonl(path: str, rec: dict) -> None:
    with open(path, "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, sort_keys=True, ensure_ascii=False) + "\n")


def write_outputs(out: str, records: list[dict], dedup: list[dict]) -> None:
    records = sorted(records, key=lambda r: r["ids"]["ledger_item_id"])
    by_repo: dict[str, list] = {}
    for r in records:
        by_repo.setdefault(r["package_id"].split("/", 1)[0], []).append(r)
    for repo, recs in by_repo.items():
        write_index(os.path.join(out, repo, "INDEX.jsonl"), recs)
    write_index(os.path.join(out, "INDEX.jsonl"), records)
    write_index(os.path.join(out, "DEDUP.jsonl"), sorted(dedup, key=lambda d: d["item_id"]))


def write_index(path: str, recs: list[dict]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for r in recs:
            f.write(json.dumps(r, sort_keys=True, ensure_ascii=False) + "\n")


def validate_all(index_path: str, packages_dir: str) -> list[str]:
    problems = []
    schema = P.load_schema()
    with open(index_path, encoding="utf-8") as f:
        recs = [json.loads(line) for line in f if line.strip()]
    for r in recs:
        pkg = None
        if r["status"] in P.PACKAGED_STATUSES:
            repo, name = r["package_id"].split("/", 1)
            pkg = os.path.join(packages_dir, repo, name)
            if not os.path.isdir(pkg):
                problems.append(f"{r['package_id']}: package directory missing")
                continue
            with open(os.path.join(pkg, "problem.json"), encoding="utf-8") as f:
                if json.load(f) != r:
                    problems.append(f"{r['package_id']}: problem.json differs from the index record")
        for e in P.validate_problem(r, pkg, schema):
            problems.append(f"{r['package_id']}: {e}")
    return problems


# ------------------------------------------------------------------------------------------ decomposition

def setup_shared(cfg: WaveConfig) -> Shared:
    env = L.load_env(cfg.lean_env)
    tc = T.Toolchain(cfg.tools_root, cfg.home, cfg.scratch)
    sh = Shared(cfg, env, tc, P.sha256_file(cfg.ledger), generator_info())
    for rid, repo in cfg.repos.items():
        sh.indexes[rid] = I.RepoIndex.build(repo["dir"], nr_files(repo["dir"]))
    return sh


_FN_INDEX: dict = {}


def crate_functions(t: I.Target) -> dict[str, list[tuple[str, NS.Function, str, str]]]:
    """name -> [(abs path, function, blanked text, source)] over the target's crate and its path dependencies."""
    key = t.crate
    if key in _FN_INDEX:
        return _FN_INDEX[key]
    out: dict = {}
    for d in [t.crate] + path_dep_dirs(t.crate):
        base = os.path.join(d, "src")
        if not os.path.isdir(base):
            continue
        for rel in nr_files(base):
            p = os.path.join(base, rel)
            with open(p, encoding="utf-8", errors="replace") as f:
                src = f.read()
            fns, _, text = NS.scan(src)
            for fn in fns:
                if fn.attr("test") or any(b.kind == "mod" and "test" in b.self_type for b in fn.blocks):
                    continue
                out.setdefault(fn.name, []).append((p, fn, text, src))
    _FN_INDEX[key] = out
    return out


CALL_RE = re.compile(r"(?<![A-Za-z0-9_])(\.)?\s*([a-z_][A-Za-z0-9_]*)\s*(?:::\s*<[^()]*?>)?\s*\(")
KEYWORDS = {"if", "for", "while", "assert", "assert_eq", "match", "return", "let", "fn", "else", "in", "unsafe",
            "static_assert", "loop", "quote", "comptime"}


def callees(t: I.Target, cap: int = 40) -> tuple[list[I.Target], list[str]]:
    """Functions of the crate (and its path dependencies) the target's body calls, resolved by name: a method
    call resolves to impl functions, a plain call to free functions; names with more than three
    resolutions keep only those in the target's file.  Unresolved names are returned too."""
    index = crate_functions(t)
    body = t.text[t.fn.body_start:t.fn.end]
    out, unresolved, seen = [], [], set()
    for m in CALL_RE.finditer(body):
        method, name = bool(m.group(1)), m.group(2)
        if name in KEYWORDS or name == t.fn.name:
            continue
        cands = [(p, fn, text, src) for p, fn, text, src in index.get(name, [])
                 if (fn.impl is not None) == method or (not method and fn.impl is not None and
                                                        re.search(r"::\s*" + name + r"\b", body))]
        cands = [c for c in cands if not c[1].unconstrained and not c[1].comptime and c[1].has_body]
        if len(cands) > 3:
            here = [c for c in cands if c[0] == os.path.join(t.root, t.path)]
            cands = here
        if not cands:
            if name not in unresolved:
                unresolved.append(name)
            continue
        for p, fn, text, src in cands:
            k = (p, fn.line)
            if k in seen:
                continue
            seen.add(k)
            root = next((r for r in [t.root] if p.startswith(r + os.sep)), None)
            if root is None:
                continue
            crate = T.crate_of(p, root)
            out.append(I.Target(f"decomposition:{t.repo}:{os.path.relpath(p, root)}#{fn.name}@L{fn.line}", t.repo,
                                root, os.path.relpath(p, root), fn, text, src, crate,
                                T.crate_info(crate)["type"] if crate else "lib", t.stdlib))
            if len(out) >= cap:
                return out, unresolved
    return out, unresolved


def run_decompose(cfg: WaveConfig, index_path: str, max_depth: int = 4) -> tuple[list[dict], list[dict]]:
    """Every TOO-LARGE record (and every TOO-LARGE child, up to ``max_depth`` levels) is decomposed into the
    functions its body calls.  A callee that is a ledger row of the wave, or equal by content to an existing
    record, is mapped to it (edge only); the others are instantiated with the same planner and gates
    (``instantiation.decomposition`` names their parents).  No compositional statement is made."""
    sh = setup_shared(cfg)
    with open(index_path, encoding="utf-8") as f:
        wave = [json.loads(x) for x in f if x.strip()]
    rows = {r["item_id"]: r for r in select_rows(cfg.ledger)}
    cache: dict = {}
    by_loc, by_content = {}, {}
    for rec in wave:
        by_loc[(rec["ids"]["repo"], rec["ids"]["path"], rec["ids"]["template_line"])] = rec["package_id"]
        if rec["evidence"].get("content_sha256"):
            by_content[rec["evidence"]["content_sha256"]] = rec["package_id"]
    frontier = []
    for rec in wave:
        if rec["status"] == "TOO-LARGE" and rec["ids"]["ledger_item_id"] in rows:
            t, _ = make_target(sh, rows[rec["ids"]["ledger_item_id"]], cache)
            if t is not None:
                frontier.append((rec["package_id"], t, 0))
    edges, records = [], []
    nodes: dict[str, dict] = {}
    while frontier:
        new = []
        for parent_id, t, depth in frontier:
            kids, unresolved = callees(t)
            for name in unresolved:
                edges.append({"parent": parent_id, "call": name, "resolution": "unresolved (not a function of the "
                              "crate or its path dependencies)", "depth": depth + 1})
            for c in kids:
                loc = (c.repo, c.path, c.fn.line)
                if loc in by_loc:
                    edges.append({"parent": parent_id, "call": c.fn.name, "child": by_loc[loc],
                                  "resolution": "ledger row of the wave", "depth": depth + 1})
                    continue
                comp = T.REPO_COMPILERS[c.repo][0][0]
                key = content_key(sh, c, comp)
                if key in by_content:
                    edges.append({"parent": parent_id, "call": c.fn.name, "child": by_content[key],
                                  "resolution": "equal by content to an existing record", "depth": depth + 1})
                    continue
                if key in nodes:
                    nodes[key]["parents"].append({"package_id": parent_id, "call": c.fn.name})
                    edges.append({"parent": parent_id, "call": c.fn.name, "child_key": key,
                                  "resolution": "decomposition record", "depth": depth + 1})
                    continue
                nodes[key] = {"target": c, "parents": [{"package_id": parent_id, "call": c.fn.name}],
                              "depth": depth + 1, "key": key}
                new.append(key)
                edges.append({"parent": parent_id, "call": c.fn.name, "child_key": key,
                              "resolution": "decomposition record", "depth": depth + 1})
        log(sh, f"decomposition level: {len(frontier)} parents, {len(new)} new children")
        level_recs = []
        with cf.ThreadPoolExecutor(max_workers=cfg.jobs) as pool:
            futs = {}
            for key in new:
                node = nodes[key]
                c = node["target"]
                row = {"item_id": c.item_id, "repo": cfg.repos[c.repo]["url"], "commit": cfg.repos[c.repo]["commit"],
                       "path": c.path, "symbol": c.fn.name, "census": [{"line": c.fn.line}], "coverage": "none"}
                futs[pool.submit(process, sh, row, c, "decomposition")] = key
            for fut in cf.as_completed(futs):
                key = futs[fut]
                rec = fut.result()
                node = nodes[key]
                rec["ids"]["ledger"]["row_found"] = False
                rec["evidence"]["content_sha256"] = key
                if rec["status"] in P.PACKAGED_STATUSES or rec["status"] == "TOO-LARGE":
                    rec["instantiation"]["decomposition"] = {
                        "parents": node["parents"], "n_parents": len(node["parents"]), "depth": node["depth"],
                        "content_sha256": key, "variants": 1, "instance_key": key}
                    if rec["status"] in P.PACKAGED_STATUSES:
                        rewrite_problem(cfg, rec)
                node["record"] = rec
                level_recs.append(rec)
                append_jsonl(os.path.join(cfg.out, "PROGRESS.jsonl"), rec)
        records += level_recs
        frontier = [(nodes[k]["record"]["package_id"], nodes[k]["target"], nodes[k]["depth"]) for k in new
                    if nodes[k]["record"]["status"] == "TOO-LARGE" and nodes[k]["depth"] < max_depth]
    for e in edges:
        if "child_key" in e:
            e["child"] = nodes[e.pop("child_key")]["record"]["package_id"]
    write_outputs(cfg.out, records, [])
    write_index(os.path.join(cfg.out, "EDGES.jsonl"), edges)
    return records, edges


def rewrite_problem(cfg: WaveConfig, rec: dict) -> None:
    repo, name = rec["package_id"].split("/", 1)
    path = os.path.join(cfg.out, repo, name, "problem.json")
    if os.path.exists(path):
        P.write_json(path, rec)


# ------------------------------------------------------------------------------------------ nargo execute cross-check

def toml_value(v) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, list):
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {toml_value(x)}" for k, x in v.items()) + " }"
    return json.dumps(str(v))


def crosscheck_nargo_execute(cfg: WaveConfig, index_path: str, limit: int = 24) -> list[dict]:
    """``nargo execute`` against the boole-acir-tool executor on the same artifact and inputs.  Standard-library
    packages: the wrapper becomes the ``main`` of a binary crate; binary mains with a repository Prover.toml:
    the repository's own inputs.  The two witness maps must be equal."""
    sh = setup_shared(cfg)
    with open(index_path, encoding="utf-8") as f:
        recs = [json.loads(x) for x in f if x.strip()]
    rows = {r["item_id"]: r for r in select_rows(cfg.ledger)}
    out = []
    work = os.path.join(cfg.work, "crosscheck")
    os.makedirs(work, exist_ok=True)
    picked = [r for r in recs if r["status"] in P.PACKAGED_STATUSES and r["ids"]["repo"] == "noir-lang/noir"][:limit]
    picked += [r for r in recs if r["instantiation"]["rule"] == "repo-main" and r["status"] != "COMPILE-FAIL"][:limit]
    cache: dict = {}
    for k, rec in enumerate(picked):
        t, _ = make_target(sh, rows[rec["ids"]["ledger_item_id"]], cache)
        tag = rec["circuit"]["compiler"]["version"]
        tag = next(x for x, c in T.COMPILERS.items() if c.version == tag and x in T.REPO_COMPILERS[t.repo][0])
        d = os.path.join(work, f"x{k}")
        if os.path.exists(d):
            shutil.rmtree(d)
        res = {"package_id": rec["package_id"], "compiler": tag}
        try:
            if rec["instantiation"]["rule"] == "repo-main":
                copy_crate(t.crate, d, as_lib=False)
                prover = os.path.join(t.crate, "Prover.toml")
                if not os.path.exists(prover):
                    res["result"] = "skipped: no repository Prover.toml"
                    out.append(res)
                    continue
                shutil.copyfile(prover, os.path.join(d, "Prover.toml"))
                inputs = {"__prover_toml__": prover}
            else:
                pkg = os.path.join(cfg.out, *rec["package_id"].split("/", 1))
                wrapper = open(os.path.join(pkg, "evidence", "wrapper.nr"), encoding="utf-8").read()
                imports = rec["evidence"].get("std_imports", [])
                body = re.sub(r"#\[export\]\nfn boole_det_\w+\(", "fn main(", wrapper)
                body = re.sub(r"\) -> (.+) \{\n", r") -> pub \1 {\n", body, count=1)
                os.makedirs(os.path.join(d, "src"))
                with open(os.path.join(d, "Nargo.toml"), "w", encoding="utf-8") as f:
                    f.write('[package]\nname = "xcheck"\ntype = "bin"\n\n[dependencies]\n')
                with open(os.path.join(d, "src", "main.nr"), "w", encoding="utf-8") as f:
                    f.write("\n".join(imports) + "\n" + body)
                abi = json.load(open(os.path.join(pkg, "evidence", "acir.json")))["abi"]
                inputs = sample_inputs(abi, 1, rec["package_id"] + "/xcheck")[0]
                names = [p["name"] for p in abi["parameters"]]
                with open(os.path.join(d, "Prover.toml"), "w", encoding="utf-8") as f:
                    for n in names:
                        f.write(f"{n} = {toml_value(inputs[n])}\n")
            r = sh.tc.run_nargo(tag, ["execute", "--silence-warnings", "xw"], d, cfg.compile_timeout)
            gz = os.path.join(d, "target", "xw.gz")
            arts = [x for x in os.listdir(os.path.join(d, "target")) if x.endswith(".json")] \
                if os.path.isdir(os.path.join(d, "target")) else []
            if r.rc != 0 or not os.path.exists(gz) or not arts:
                res["result"] = "nargo execute failed: " + scrub_text(sh, first_error(r.out))[:200]
                out.append(res)
                continue
            art = os.path.join(d, "target", arts[0])
            w = sh.tc.run_tool(tag, ["witness", gz], d, 300)
            nargo_w = json.loads(w.out)["stack"][-1]["witness"]
            ex = execute(sh, tag, art, None, [inputs], os.path.join(d, "tool"))[0]
            res["result"] = "equal" if ex.get("ok") and ex["witness"] == nargo_w else "DIFFERENT"
            res["witnesses"] = len(nargo_w)
        except Exception as exn:                                  # recorded
            res["result"] = f"error: {type(exn).__name__}: {str(exn)[:200]}"
        out.append(res)
        log(sh, f"crosscheck {rec['package_id']}: {res['result']}")
    write_index(os.path.join(cfg.out, "CROSSCHECK-nargo-execute.jsonl"), out)
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("select")
    s.add_argument("--ledger", required=True)
    s.add_argument("--out", required=True)
    w = sub.add_parser("wave")
    w.add_argument("--config", required=True)
    w.add_argument("--jobs", type=int)
    w.add_argument("--only", nargs="*")
    dc = sub.add_parser("decompose")
    dc.add_argument("--config", required=True)
    dc.add_argument("--index", required=True)
    dc.add_argument("--jobs", type=int)
    xc = sub.add_parser("crosscheck")
    xc.add_argument("--config", required=True)
    xc.add_argument("--index", required=True)
    v = sub.add_parser("validate")
    v.add_argument("--index", required=True)
    v.add_argument("--packages", required=True)
    a = ap.parse_args(argv)
    if a.cmd == "select":
        rows = select_rows(a.ledger)
        write_index(a.out, rows)
        print(json.dumps({"rows": len(rows), "filter": SELECTION_FILTER}, indent=1))
        return 0
    if a.cmd == "wave":
        cfg = WaveConfig.load(a.config)
        if a.jobs:
            cfg.jobs = min(a.jobs, 4)
        if a.only:
            cfg.only = a.only
        recs = run_wave(cfg)
        counts: dict[str, int] = {}
        for r in recs:
            counts[r["status"]] = counts.get(r["status"], 0) + 1
        print(json.dumps(counts, indent=1, sort_keys=True))
        return 0
    if a.cmd == "decompose":
        cfg = WaveConfig.load(a.config)
        if a.jobs:
            cfg.jobs = min(a.jobs, 4)
        recs, edges = run_decompose(cfg, a.index)
        counts: dict[str, int] = {}
        for r in recs:
            counts[r["status"]] = counts.get(r["status"], 0) + 1
        print(json.dumps({"records": counts, "edges": len(edges)}, indent=1, sort_keys=True))
        return 0
    if a.cmd == "crosscheck":
        res = crosscheck_nargo_execute(WaveConfig.load(a.config), a.index)
        counts = {}
        for r in res:
            k = r["result"].split(":")[0]
            counts[k] = counts.get(k, 0) + 1
        print(json.dumps(counts, indent=1, sort_keys=True))
        return 0
    if a.cmd == "validate":
        probs = validate_all(a.index, a.packages)
        for p in probs[:50]:
            print(p)
        print(f"{len(probs)} problem(s)")
        return 1 if probs else 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
