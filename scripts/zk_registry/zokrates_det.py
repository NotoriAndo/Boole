#!/usr/bin/env python3
"""ZoKrates DET problem generator: wave driver.

Subcommands::

    select  --ledger LEDGER-v1.jsonl                         the wave-K1 population filter (count)
    wave    --config wave.json [--jobs N] [--only ITEM ...]   plan, compile, model, gate and package
    validate --index INDEX.jsonl --packages DIR               re-validate every record and package file

Pipeline per ledger row (deduplicated by content key first): the declaration is located in the
repository at its pin (:mod:`zokrates_source`); the planner (:mod:`zokrates_instantiation`) grounds its
generic constants (``main`` with no generics compiles as the file itself; every other declaration gets a
generated wrapper), tier by tier.  The pinned ZoKrates compiler (:mod:`zokrates_toolchain`) compiles each
candidate: ZoKrates 0.8.8 emits the iden3/circom ``.r1cs``/``.wtns`` directly (:mod:`zokrates_r1cs`);
ZoKrates 0.6.1 (ethereum-oasis-op/baseline's pin; no R1CS export at that version) is read from its own
``.ztf`` constraint listing and a plain-text witness (:mod:`zokrates_legacy`).  Either way the result is
the shared :class:`r1cs.R1cs` model the Circom generator already gates: G-ELAB, G-NONVAC, G-FID, G-TRIV
(battery P1 + P2) and a sound counterexample search, all via the shared :mod:`gates`/:mod:`det_search`
(framework-agnostic; this generator needs no tag preconditions and no commitment/ACIR special-casing).
Every row ends in one terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE, TOO-LARGE, NO-INSTANTIATION,
COMPILE-FAIL or NOT-APPLICABLE (with a reason).  The driver never writes a proof of a DET statement.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import platform
import random
import shutil
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C                        # noqa: E402
from zk_registry import det_search as DS                  # noqa: E402
from zk_registry import gates as G                         # noqa: E402
from zk_registry import lean_emit as E                     # noqa: E402
from zk_registry import lean_runner as L                   # noqa: E402
from zk_registry import package as P                       # noqa: E402
from zk_registry import r1cs as R                          # noqa: E402
from zk_registry import witness as Wmod                    # noqa: E402
from zk_registry import zokrates_instantiation as I         # noqa: E402
from zk_registry import zokrates_lean_emit as ZE            # noqa: E402
from zk_registry import zokrates_legacy as ZL               # noqa: E402
from zk_registry import zokrates_r1cs as ZR                 # noqa: E402
from zk_registry import zokrates_source as S                # noqa: E402
from zk_registry import zokrates_toolchain as T             # noqa: E402
from zk_registry import zokrates_witness as ZW              # noqa: E402

GENERATOR_NAME = "boole-zk-registry-zokrates-det"
GENERATOR_VERSION = "1.0"
_HERE = os.path.dirname(os.path.abspath(__file__))
ZOKRATES_SOURCES = ["zokrates_det.py", "zokrates_instantiation.py", "zokrates_source.py", "zokrates_r1cs.py",
                    "zokrates_legacy.py", "zokrates_lean_emit.py", "zokrates_toolchain.py", "zokrates_witness.py"]
SHARED_SOURCES = ["__init__.py", "r1cs.py", "package.py", "jsonschema_lite.py", "lean_runner.py", "lean_emit.py",
                  "gates.py", "check.py", "det_search.py", "witness.py", "lean/ZkReplay.lean",
                  "schema/problem.schema.json"]

MAX_CONSTRAINTS = P.MAX_CONSTRAINTS         # 2000
SIZE_GUARD = 20000                          # stop probing larger generics once a candidate is this far over budget
SAMPLES = 48
REAL_WANTED = 16
MUTANTS = 16
PROBED_NOT_A_FINDING = ("not-a-finding: probed instantiation (a generic constant chosen by the generator, not "
                        "taken from the repository)")
MAX_JOBS = 12

# ------------------------------------------------------------------------------------------ population

EXCLUDED_UNITS_SUFFIX = ("-constraint-site", "-interaction-site")
EXCLUDED_UNITS = ("C2-operation", "U1-instruction")
EXCLUDED_FLAG_SUBSTRINGS = ("test", "not-counted", "deprecated", "NOT-ITEMIZED", "ZERO-ITEMS", "supplementary",
                            "exported-api-false", "intrinsic", "generated", "duplicate-of", "copy-of", "vendored",
                            "mapped-to", "archived", "excluded", "secondary", "program-as-circuit")
SELECTION_FILTER = {"framework": "zokrates", "excluded_unit_suffixes": list(EXCLUDED_UNITS_SUFFIX),
                    "excluded_units": list(EXCLUDED_UNITS), "excluded_flag_substrings": list(EXCLUDED_FLAG_SUBSTRINGS)}


def selected(row: dict) -> bool:
    if row.get("framework") != "zokrates":
        return False
    unit = row.get("unit", "")
    if unit.endswith(EXCLUDED_UNITS_SUFFIX) or unit in EXCLUDED_UNITS:
        return False
    return not any(s in f for f in row.get("flags", []) for s in EXCLUDED_FLAG_SUBSTRINGS)


def select_rows(ledger_path: str) -> list[dict]:
    with open(ledger_path, encoding="utf-8") as f:
        return [r for r in (json.loads(line) for line in f if line.strip()) if selected(r)]


def content_key(text: str, symbol: str, compiler_version: str) -> str:
    """sha256 over the compiler version and the comment-free declaration text (dedup key)."""
    h = hashlib.sha256()
    h.update(compiler_version.encode() + b"\0" + symbol.encode() + b"\0" + S.strip_comments(text).encode("utf-8"))
    return h.hexdigest()


def dedup(rows: list[dict], repo_text, repo_version) -> tuple[list[dict], list[dict]]:
    """``(canonical rows, dedup records)``; a row whose declaration cannot be located is kept (its
    content key falls back to the ledger path/symbol, so it is never silently dropped)."""
    seen: dict[str, dict] = {}
    canon: list[dict] = []
    dup: list[dict] = []
    for r in sorted(rows, key=lambda r: r["item_id"]):
        try:
            text = repo_text(r)
            decl = I.find_symbol(text, r["symbol"]) if r["symbol"] != "main" or "<" in text else None
            key = content_key(text, r["symbol"], repo_version(r))
        except Exception:
            key = "path:" + r["item_id"]
        if key in seen:
            dup.append({"item_id": r["item_id"], "canonical_item_id": seen[key]["item_id"], "content_sha256": key})
        else:
            seen[key] = r
            canon.append(r)
    return canon, dup


# ------------------------------------------------------------------------------------------ config

@dataclass
class WaveConfig:
    collection: str
    ledger: str
    repos: dict               # repo_id -> {"dir", "url", "commit"}
    compilers: dict           # repo_id -> toolchain pin record
    scratch: str
    lean_env: str
    out: str
    work: str
    jobs: int = 6
    lean_jobs: int = 6
    only: list = field(default_factory=list)
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    det_search_budget_s: float = 20.0

    @classmethod
    def load(cls, path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            return cls(**json.load(f))


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in SHARED_SOURCES + ZOKRATES_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(_HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


def generator_files() -> list[str]:
    """Files hashed by this generator only (the circom generator hashes the shared ones too)."""
    return [f for f in ZOKRATES_SOURCES if f.endswith((".py", ".js", ".lean", ".json"))]


DET_SPEC_CLAUSES = [
    "Output determinism: for the compiled constraint system of the instantiated ZoKrates function (its "
    "generic constants grounded by the repository or, failing that, by a small probed value), any two wire "
    "assignments that satisfy every constraint (including the constant-one wire) and agree on every input "
    "wire of the wrapper's main (its public and private arguments) agree on every output wire (the return "
    "value's flattened leaves).",
    "The field is the ZoKrates compilation curve's scalar field order; inputs and outputs are the "
    "compiled function's argument and return leaves, located from the compiler's own ABI specification.",
]
DET_SPEC = {"tier": "T1", "source": "mathematical definition of output determinism (functional dependence of "
           "outputs on inputs)", "clauses": DET_SPEC_CLAUSES,
           "sha256": hashlib.sha256("\n".join(DET_SPEC_CLAUSES).encode("utf-8")).hexdigest()}
STATEMENT_ASSUMPTIONS = ["[Fact (Nat.Prime p)]: primality of the ZoKrates curve's scalar field order, supplied "
                         "as an instance hypothesis so that field lemmas apply; p is prime, so the hypothesis "
                         "does not weaken the statement.",
                         "ZoKrates directive/solver outputs (division, bit-decomposition and similar prover "
                         "hints) that no constraint pins down are free in the model: ZoKrates's own R1CS export "
                         "allocates a wire only for a variable some constraint mentions."]

# ------------------------------------------------------------------------------------------ shared state


@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    ledger_sha: str
    generator: dict
    lock: threading.Lock = field(default_factory=threading.Lock)
    lean_slots: threading.Semaphore | None = None


def log(sh: Shared, msg: str) -> None:
    with sh.lock:
        print(time.strftime("%H:%M:%S"), msg, flush=True)


def scrub_text(sh: Shared, s: str) -> str:
    roots = [sh.cfg.scratch, sh.cfg.work] + [r["dir"] for r in sh.cfg.repos.values()]
    for root in sorted({r for r in roots if r}, key=len, reverse=True):
        s = s.replace(root, "$LOCAL")
    return s


# ------------------------------------------------------------------------------------------ repository access

def repo_id_of(url: str) -> str:
    import re
    m = re.match(r"https://github\.com/([^/]+)/([^/]+?)(\.git)?/?$", url)
    return f"{m.group(1)}/{m.group(2)}" if m else url


def repo_dir_name(repo_id: str) -> str:
    return repo_id.replace("/", "__")


def compiler_obj(pin: dict, flags: list[str]) -> dict:
    return {"name": "zokrates", "version": pin["version"], "flags": flags,
            "binary_sha256": pin["binary_sha256"], "source": pin["source"]}


def read_source(cfg: WaveConfig, row: dict) -> str:
    repo = cfg.repos[row["project"]]
    with open(os.path.join(repo["dir"], row["path"]), encoding="utf-8") as f:
        return f.read()


# ------------------------------------------------------------------------------------------ compile + model

@dataclass
class Compiled:
    model: object                 # zokrates_r1cs.Model
    r1cs_sha256: str
    compiler_flags: list[str]
    source: str                   # the compiled wrapper / file text


def compile_candidate(pin: dict, style: str, src_path: str, src_text: str, workdir: str) -> tuple[Compiled | None, str | None]:
    """Compile the already-written ``src_path`` (the repository's own file for a bare ``main``, or a
    wrapper written next to it so its relative imports resolve); every output artifact goes to the
    isolated ``workdir``, never next to the source."""
    os.makedirs(workdir, exist_ok=True)
    out = os.path.join(workdir, "out")
    abi = os.path.join(workdir, "abi.json")
    if style == "brace":
        r1cs = os.path.join(workdir, "out.r1cs")
        r = T.run(pin, ["compile", "-i", src_path, "-o", out, "-s", abi, "-r", r1cs], workdir, timeout=180)
        if r.returncode != 0 or not os.path.isfile(r1cs):
            return None, (r.stderr or r.stdout or "compile failed").strip().splitlines()[-1][:400]
        with open(abi, encoding="utf-8") as f:
            abi_doc = json.load(f)
        try:
            model = ZR.build_native(r1cs, abi_doc)
        except ZR.AbiError as ex:
            return None, f"model not assembled: {ex}"
        return Compiled(model, P.sha256_file(r1cs), ["compile", "-i", "main.zok"], src_text), None
    else:
        r = T.run(pin, ["compile", "-i", src_path, "-o", out, "-s", abi], workdir, timeout=180)
        ztf = out + ".ztf"
        if r.returncode != 0 or not os.path.isfile(ztf):
            return None, (r.stderr or r.stdout or "compile failed").strip().splitlines()[-1][:400]
        with open(abi, encoding="utf-8") as f:
            abi_doc = json.load(f)
        with open(ztf, encoding="utf-8") as f:
            ztf_text = f.read()
        try:
            model, renumber = ZL.build_model(ztf_text, abi_doc)
        except (ZL.ZtfFormatError, ZR.AbiError) as ex:
            return None, f"model not assembled: {ex}"
        model._renumber = renumber
        return Compiled(model, hashlib.sha256(ztf_text.encode()).hexdigest(), ["compile", "-i", "main.zok"],
                        src_text), None


def compute_real_witnesses(pin: dict, style: str, workdir: str, model, want: int, seed: str) -> tuple[list, dict]:
    p = model.r.prime
    flat = ZW.sample_flat_values(model.input_leaf_types, p, seed, SAMPLES)
    accepted, errors, seen = [], {}, set()
    for k, args in enumerate(flat):
        if len(accepted) >= want:
            break
        wd = os.path.join(workdir, f"w{k}")
        os.makedirs(wd, exist_ok=True)
        out_bin = os.path.join(workdir, "out")
        abi_path = os.path.join(workdir, "abi.json")
        if style == "brace":
            w, err = ZW.compute_witness_modern(pin, out_bin, abi_path, args, wd)
        else:
            w, err = ZW.compute_witness_legacy(pin, out_bin, abi_path, args, wd, model._renumber,
                                               model.r.n_wires, p)
        if w is None:
            errors[(err or "")[:120]] = errors.get((err or "")[:120], 0) + 1
            continue
        key = tuple(args)
        if key in seen or not R.satisfies(model.r, w):
            continue
        seen.add(key)
        accepted.append(w)
    return accepted, errors


def make_mutants(r: R.R1cs, real: list[list[int]], n: int, seed: str) -> list[dict]:
    if not real:
        return []
    return Wmod.mutants(r, real, n, seed)


# ------------------------------------------------------------------------------------------ per-row pipeline

def new_record(sh: Shared, row: dict, dir_name: str) -> dict:
    repo = sh.cfg.repos[row["project"]]
    return {"schema_version": P.SCHEMA_VERSION, "package_id": f"{sh.cfg.collection}/{dir_name}",
            "property": P.DET_PROPERTY, "status": "NO-INSTANTIATION", "status_reason": "",
            "ids": {"ledger_item_id": row["item_id"], "repo": row["project"], "repo_url": repo["url"],
                    "release": sh.cfg.compilers[row["project"]]["version"], "commit": row["commit"],
                    "path": row["path"], "template": row["symbol"], "template_line": 0,
                    "source_sha256": hashlib.sha256(open(os.path.join(repo["dir"], row["path"]), "rb").read()).hexdigest()},
            "instantiation": {"rule": "", "args": [], "call": "", "provenance": [], "selection": "", "candidates": []},
            "spec": DET_SPEC,
            "env": {"lean": sh.env.lean_version, "mathlib": sh.env.packages.get("mathlib", ""),
                    "lake_manifest_sha256": sh.env.manifest_sha256, "python": platform.python_version(),
                    "zokrates": sh.cfg.compilers[row["project"]]["version"]},
            "generator": sh.generator}


def process_row(sh: Shared, row: dict) -> dict:
    repo_id = row["project"]
    pin = sh.cfg.compilers[repo_id]
    style = pin["style"]
    dir_name = P.package_dir_name(row["path"], row["symbol"], (), strip_prefix="")
    rec = new_record(sh, row, dir_name)
    try:
        text = read_source(sh.cfg, row)
    except OSError as ex:
        rec["status"], rec["status_reason"] = "NO-INSTANTIATION", f"source not found: {ex}"
        return rec
    try:
        decl = I.find_symbol(text, row["symbol"])
    except I.InstantiationError as ex:
        rec["status"], rec["status_reason"] = "NO-INSTANTIATION", str(ex)
        return rec
    rec["ids"]["template_line"] = decl.line
    if decl.style != style:
        rec["status"], rec["status_reason"] = "NOT-APPLICABLE", (
            f"declaration syntax ({decl.style}) does not match the repository's pinned compiler style ({style})")
        return rec

    imports = S.find_imports(text)
    module_path = "./" + os.path.splitext(os.path.basename(row["path"]))[0]
    is_bare_main = row["symbol"] == "main" and not decl.generics
    repo_dir = sh.cfg.repos[repo_id]["dir"]
    abs_path = os.path.join(repo_dir, row["path"])
    src_dir = os.path.dirname(abs_path)

    tiers = I.candidates_for(decl, text)
    attempts: list[dict] = []
    best: tuple[Compiled, I.Candidate, str, str] | None = None
    smallest: tuple[Compiled, I.Candidate, str, str] | None = None
    chosen_tier = None
    work_root = os.path.join(sh.cfg.work, dir_name)
    wrapper_paths: list[str] = []
    for tier, cands in tiers:
        tier_live: list[tuple[Compiled, I.Candidate, str, str]] = []     # successfully compiled, this tier
        for k, cand in enumerate(cands):
            free_gb = shutil.disk_usage(sh.cfg.scratch).free / 1e9
            if free_gb < 20:
                attempts.append({"tier": tier, "call": row["symbol"], "compile": "skipped",
                                 "error": f"skipped: only {free_gb:.1f} GB free on scratch's volume"[:500]})
                break
            wd = os.path.join(work_root, f"{tier}_{k}")
            if is_bare_main and cand.tier == "parameter-free":
                src_path, src = abs_path, text
            else:
                src = I.wrapper_source(decl, cand, module_path, imports, style)
                src_path = os.path.join(src_dir, f".detgen_{dir_name}_{tier}_{k}.zok")
                wrapper_paths.append(src_path)
                with open(src_path, "w", encoding="utf-8") as f:
                    f.write(src)
            compiled, err = compile_candidate(pin, style, src_path, src, wd)
            call = f"{row['symbol']}({', '.join(f'{g}={v}' for g, v in cand.values.items())})" if cand.values else row["symbol"]
            attempt = {"tier": tier, "call": call, "compile": "ok" if compiled else "error"}
            if compiled:
                attempt["constraints"] = compiled.model.r.n_constraints
                attempt["wires"] = compiled.model.r.n_wires
            else:
                attempt["error"] = scrub_text(sh, err)[:500]
            attempts.append(attempt)
            if compiled is None:
                shutil.rmtree(wd, ignore_errors=True)
                continue
            tier_live.append((compiled, cand, call, wd))
            # A circuit already far beyond the size policy only grows with a larger probed value (the
            # population's probe tiers are monotonic in each generic): stop climbing the probe ladder
            # instead of compiling, and writing to scratch, an ever larger one for no packageable gain.
            if compiled.model.r.n_constraints > SIZE_GUARD:
                break
        tier_best = max((c for c in tier_live if c[0].model.r.n_constraints <= MAX_CONSTRAINTS),
                        key=lambda c: c[0].model.r.n_constraints, default=None)
        tier_smallest = min(tier_live, key=lambda c: c[0].model.r.n_constraints, default=None)
        for compiled, cand, call, wd in tier_live:
            if (tier_best and wd == tier_best[3]) or (tier_best is None and tier_smallest and wd == tier_smallest[3]):
                continue
            shutil.rmtree(wd, ignore_errors=True)
        if tier_best is None and tier_smallest is not None and smallest is None:
            smallest = tier_smallest
        if tier_best is not None:
            best, chosen_tier = tier_best, tier
            break
    for wp in wrapper_paths:
        try:
            os.remove(wp)
        except OSError:
            pass
    if best is None:
        if smallest is not None:
            compiled, cand, call, _wd = smallest
            rec["status"], rec["status_reason"] = "TOO-LARGE", (
                f"every compiled instantiation exceeds {MAX_CONSTRAINTS} constraints; smallest {compiled.model.r.n_constraints}")
            rec["instantiation"].update(rule=cand.tier, args=[str(v) for v in cand.values.values()], call=call,
                                        provenance=[cand.provenance], selection="smallest (none within the size policy)",
                                        candidates=attempts)
            rec["circuit"] = {"compiler": compiler_obj(pin, compiled.compiler_flags), "prime": str(compiled.model.r.prime),
                              "prime_name": compiled.model.r.prime_name or "bn128",
                              "n_constraints": compiled.model.r.n_constraints, "n_wires": compiled.model.r.n_wires,
                              "size_policy": {"max_constraints": MAX_CONSTRAINTS, "within": False}}
        else:
            errs = sorted({a["error"] for a in attempts if a.get("error")})
            if attempts:
                rec["status"] = "COMPILE-FAIL"
                rec["status_reason"] = "; ".join(errs)[:600]
            else:
                rec["status"] = "NO-INSTANTIATION"
                rec["status_reason"] = decl.generics and \
                    "no generic value could be derived from a call site or probed within an assert bound" or \
                    "no candidate"
            rec["instantiation"].update(rule="none", candidates=attempts)
        return rec

    compiled, cand, call, chosen_wd = best
    rec["instantiation"].update(rule=cand.tier, args=[str(v) for v in cand.values.values()], call=call,
                                provenance=[cand.provenance],
                                selection=f"tier {chosen_tier}: largest compiled instantiation within {MAX_CONSTRAINTS} constraints",
                                candidates=attempts)
    model = compiled.model
    rec["circuit"] = {"compiler": compiler_obj(pin, compiled.compiler_flags), "prime": str(model.r.prime),
                      "prime_name": model.r.prime_name or "bn128", "n_constraints": model.r.n_constraints,
                      "n_wires": model.r.n_wires, "n_inputs": len(model.inputs), "n_outputs": len(model.outputs),
                      "r1cs_sha256": compiled.r1cs_sha256,
                      "size_policy": {"max_constraints": MAX_CONSTRAINTS, "within": True}}
    return build_package(sh, rec, row, pin, style, compiled, chosen_wd)


def build_package(sh: Shared, rec: dict, row: dict, pin: dict, style: str, compiled: Compiled, work: str) -> dict:
    model = compiled.model
    dir_name = rec["package_id"].split("/", 1)[1]
    ns = P.lean_namespace(sh.cfg.collection, dir_name)
    meta = {"repo_id": row["project"], "instantiation": rec["instantiation"]["call"],
            "generator": f"{sh.generator['name']} v{sh.generator['version']}", "repo_url": rec["ids"]["repo_url"],
            "commit": row["commit"], "path": row["path"], "symbol": row["symbol"],
            "rule": rec["instantiation"]["rule"], "zokrates_version": pin["version"],
            "zokrates_flags": compiled.compiler_flags, "r1cs_sha256": compiled.r1cs_sha256,
            "prime_name": model.r.prime_name or "bn128"}
    stage = os.path.join(work, "pkg")
    shutil.rmtree(stage, ignore_errors=True)
    model_rel = E.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(ZE.emit_model(ns, meta, model.r, model.outputs, model.inputs, model.wire_names))
    statement_text = E.emit_statement(ns, meta, preconditions=False)
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{E.STATEMENT_THEOREM}"
    rec["statement"] = {"file": "Statement.lean", "theorem": E.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": E.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": STATEMENT_ASSUMPTIONS, "truth": "unknown"}
    rec["evidence"] = {"wrapper_source": compiled.source[:20000]}

    real, errors = compute_real_witnesses(pin, style, work, model, REAL_WANTED, rec["package_id"])
    muts = make_mutants(model.r, real, MUTANTS, rec["package_id"])
    rec["evidence"]["witness_sampling"] = {"samples": SAMPLES, "distinct_witnesses": len(real),
                                           "solver_errors": dict(list(errors.items())[:6])}
    build = os.path.join(work, "build")
    shutil.rmtree(build, ignore_errors=True)
    with sh.lean_slots:
        elab = G.g_elab(sh.env, stage, build, ns, os.path.join(work, "elab"))
    gates: dict = {"G-ELAB": elab.to_json()}
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build, model_rel[:-len(".lean")] + ".olean"))
    input_free = not model.inputs

    def write_w(path: str, w: list[int]) -> None:
        Wmod.write_witness(path, w)

    if model_ok:
        with sh.lean_slots:
            fid, nonvac = G.g_fid_nonvac(sh.env, build, ns, model.r.n_constraints, real, muts, input_free,
                                         os.path.join(work, "fid"), write_w)
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()
    cheap_ok = all(gates[g]["status"] == "PASS" for g in ("G-ELAB", "G-NONVAC", "G-FID"))
    ce, slog = None, {"bases": len(real)}
    if not model.outputs:
        det_gate = {"status": "SKIPPED", "truth": "unknown", "reason": "no outputs: the statement is vacuous"}
    elif not cheap_ok:
        det_gate = {"status": "SKIPPED", "truth": "unknown",
                    "reason": "not run: the package already fails G-ELAB, G-NONVAC or G-FID"}
    else:
        ce, slog = DS.search(model.r, real, model.inputs, model.outputs, rec["package_id"], sh.cfg.det_search_budget_s)
        det_gate = {"status": "PASS", "truth": "unknown", "log": slog,
                    "note": "PASS means no counterexample was found by the cheap searches; DET truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        write_w(os.path.join(cdir, "w1.txt"), ce.base)
        write_w(os.path.join(cdir, "w2.txt"), ce.other)
        with sh.lean_slots:
            verdicts, r = G.lean_verdicts(sh.env, build, ns, model.r.n_constraints,
                                          [("w1", os.path.join(cdir, "w1.txt")), ("w2", os.path.join(cdir, "w2.txt"))],
                                          cdir)
        lean_ok = verdicts.get("w1") == "ACCEPT" and verdicts.get("w2") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if lean_ok else "unknown",
                    "method": ce.method, "changed_outputs": ce.changed_outputs[:20],
                    "lean_confirms_both_witnesses": lean_ok, "python_confirms": True, "log": slog,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"]}
        rec["evidence"]["counterexample_dir"] = cdir
    gates["DET-SEARCH"] = det_gate
    if elab.status != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement did not elaborate"}
    elif det_gate["truth"] == "false-counterexample-found":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement refuted by a confirmed counterexample"}
    elif not model.outputs:
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "no outputs: the statement is vacuous"}
    elif gates["G-NONVAC"]["status"] != "PASS" or gates["G-FID"]["status"] != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "not run: the package already fails G-NONVAC or G-FID"}
    else:
        with sh.lean_slots:
            gates["G-TRIV"] = G.g_triv(sh.env, build, ns, model.r.n_constraints, os.path.join(work, "triv"),
                                       sh.cfg.battery_heartbeats, sh.cfg.battery_file_wall,
                                       sh.cfg.battery_single_wall).to_json()
    rec["gates"] = gates
    set_status(rec, gates, bool(model.outputs))
    ref = elab.detail.get("reference_type_sha256", "0" * 64) if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": E.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(E.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": P.sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": E.model_module(ns),
                                 "sha256": P.sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(G.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    write_package(sh, rec, stage, work, real[:4])
    shutil.rmtree(work, ignore_errors=True)
    return rec


def set_status(rec: dict, gates: dict, has_outputs: bool) -> None:
    det_gate = gates["DET-SEARCH"]
    if det_gate["truth"] == "false-counterexample-found":
        rec["status"] = "DET-FALSE-CANDIDATE"
        rec["status_reason"] = (f"{det_gate['method']} found two constraint-satisfying assignments with equal "
                                "inputs and different outputs (confirmed by the Python evaluator and the Lean "
                                "model); private finding")
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
                reasons.append("the function has no circuit-variable outputs, so DET is vacuous")
            elif g == "G-TRIV" and gd.get("closed_by"):
                reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
                rec["statement"]["truth"] = "closed-by-automation"
            else:
                reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:600]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def write_package(sh: Shared, rec: dict, stage: str, work: str, real: list[list[int]]) -> None:
    dest = os.path.join(sh.cfg.out, *rec["package_id"].split("/", 1))
    if os.path.exists(dest):
        shutil.rmtree(dest)
    os.makedirs(dest)
    shutil.copyfile(os.path.join(stage, "Statement.lean"), os.path.join(dest, "Statement.lean"))
    model_rel = rec["statement"]["model_file"]
    os.makedirs(os.path.dirname(os.path.join(dest, model_rel)), exist_ok=True)
    shutil.copyfile(os.path.join(stage, model_rel), os.path.join(dest, model_rel))
    ev = os.path.join(dest, "evidence")
    os.makedirs(ev)
    with open(os.path.join(ev, "wrapper.zok"), "w", encoding="utf-8") as f:
        f.write(rec["evidence"].pop("wrapper_source"))
    os.makedirs(os.path.join(ev, "witnesses"))
    for k, w in enumerate(real):
        Wmod.write_witness(os.path.join(ev, "witnesses", f"real_{k:03d}.txt"), w)
    if "counterexample_dir" in rec["evidence"]:
        cdir = rec["evidence"].pop("counterexample_dir")
        os.makedirs(os.path.join(ev, "counterexample"))
        for fn in ("w1.txt", "w2.txt"):
            if os.path.exists(os.path.join(cdir, fn)):
                shutil.copyfile(os.path.join(cdir, fn), os.path.join(ev, "counterexample", fn))
    P.write_json(os.path.join(dest, "problem.json"), rec)
    errs = P.validate_problem(rec, dest)
    if errs:
        raise RuntimeError(f"{rec['package_id']}: package does not validate: {errs}")


# ------------------------------------------------------------------------------------------ CLI

def cmd_select(a: argparse.Namespace) -> int:
    rows = select_rows(a.ledger)
    print(f"selected {len(rows)} rows")
    by_project: dict[str, int] = {}
    for r in rows:
        by_project[r["project"]] = by_project.get(r["project"], 0) + 1
    for k, v in sorted(by_project.items()):
        print(f"  {k}: {v}")
    return 0


def run_wave(cfg: WaveConfig) -> None:
    env = L.load_env(cfg.lean_env)
    rows = select_rows(cfg.ledger)
    canon, dup = dedup(rows, lambda r: read_source(cfg, r), lambda r: cfg.compilers[r["project"]]["version"])
    if cfg.only:
        only = set(cfg.only)
        canon = [r for r in canon if r["item_id"] in only]
    os.makedirs(cfg.out, exist_ok=True)
    gen = generator_info()
    sh = Shared(cfg, env, P.sha256_file(cfg.ledger), gen, lean_slots=threading.Semaphore(cfg.lean_jobs))
    log(sh, f"selection {len(rows)} rows, {len(canon)} distinct by content, {len(dup)} duplicates")
    records: list[dict] = []
    t0 = time.time()
    with cf.ThreadPoolExecutor(max_workers=cfg.jobs) as ex:
        futs = {ex.submit(process_row, sh, r): r for r in canon}
        done = 0
        for fut in cf.as_completed(futs):
            r = futs[fut]
            try:
                rec = fut.result()
            except Exception as ex:
                rec = new_record(sh, r, P.package_dir_name(r["path"], r["symbol"], ()))
                rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"driver error: {ex}"
            records.append(rec)
            done += 1
            if done % 10 == 0 or done == len(canon):
                log(sh, f"{done}/{len(canon)} ({rec['status']})")
    log(sh, f"wave {time.time() - t0:.0f}s, {len(records)} records")
    write_outputs(cfg.out, records, dup)


def write_outputs(out: str, records: list[dict], dedup_list: list[dict]) -> None:
    def write_index(path: str, recs: list[dict]) -> None:
        with open(path, "w", encoding="utf-8") as f:
            for r in recs:
                slim = {k: v for k, v in r.items() if k not in ("statement", "evidence")}
                f.write(json.dumps(slim, sort_keys=True, separators=(",", ":")) + "\n")
    write_index(os.path.join(out, "INDEX.jsonl"), sorted(records, key=lambda r: r["package_id"]))
    by_repo: dict[str, list[dict]] = {}
    for r in records:
        by_repo.setdefault(r["ids"]["repo"], []).append(r)
    for repo_id, recs in by_repo.items():
        rd = os.path.join(out, repo_id.replace("/", "__"))
        os.makedirs(rd, exist_ok=True)
        write_index(os.path.join(rd, "INDEX.jsonl"), sorted(recs, key=lambda r: r["package_id"]))
    with open(os.path.join(out, "DEDUP.jsonl"), "w", encoding="utf-8") as f:
        for d in sorted(dedup_list, key=lambda d: d["item_id"]):
            f.write(json.dumps(d, sort_keys=True) + "\n")


def cmd_wave(a: argparse.Namespace) -> int:
    cfg = WaveConfig.load(a.config)
    if a.jobs:
        cfg.jobs = a.jobs
    if a.only:
        cfg.only = a.only
    run_wave(cfg)
    return 0


def cmd_validate(a: argparse.Namespace) -> int:
    problems = 0
    with open(a.index, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            rec = json.loads(line)
            if rec["status"] not in P.PACKAGED_STATUSES:
                errs = P.validate_problem(rec)
            else:
                pkg_dir = os.path.join(a.packages, *rec["package_id"].split("/", 1))
                full = json.load(open(os.path.join(pkg_dir, "problem.json"), encoding="utf-8"))
                errs = P.validate_problem(full, pkg_dir)
            if errs:
                problems += 1
                print(rec["package_id"], errs)
    print(f"{problems} problem(s)")
    return 1 if problems else 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("select")
    s.add_argument("--ledger", required=True)
    s = sub.add_parser("wave")
    s.add_argument("--config", required=True)
    s.add_argument("--jobs", type=int, default=0)
    s.add_argument("--only", nargs="*", default=[])
    s = sub.add_parser("validate")
    s.add_argument("--index", required=True)
    s.add_argument("--packages", required=True)
    a = ap.parse_args(argv)
    return {"select": cmd_select, "wave": cmd_wave, "validate": cmd_validate}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
