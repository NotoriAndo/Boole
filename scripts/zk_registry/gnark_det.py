#!/usr/bin/env python3
"""gnark DET problem generator: wave driver.

Subcommands::

    select    --ledger LEDGER-v1.jsonl                          the wave-G1 population filter (count)
    setup     --config wave.json                                tool module, catalog and Go toolchain per repo
    wave      --config wave.json [--jobs N] [--only ITEM ...]   plan, compile, model, gate and package
    rerun     --config wave.json --index INDEX.jsonl --items F  regenerate named records, merge
    decompose --config wave.json --index INDEX.jsonl            TOO-LARGE records -> the functions they call
    validate  --index INDEX.jsonl --packages DIR                re-validate every record and package file

Pipeline per ledger row (deduplicated by content key first): the catalog (go/packages over the
repository at its pin, :mod:`gnark_tool.catalog`) locates the declaration and its types; the planner
(:mod:`gnark_instantiation`) classifies it and generates wrapper circuits per candidate instantiation,
tier by tier; the wrappers are built against the pinned repository (Go module ``replace``) with the Go
version its ``go.mod`` requires; ``gnarkx run`` compiles each with gnark's R1CS builder (the commitment
model of :mod:`gnark_tool.harness`), solves sampled assignments with gnark's solver and gnark's test
engine, and exports JSON; :mod:`gnark_r1cs` assembles the model; the Lean model and the DET statement
(:mod:`gnark_lean_emit`) are emitted and gated: G-ELAB, G-NONVAC (a real solver witness satisfies the
model), G-FID (every real witness and single-wire mutant evaluated by Lean and by the Python R1CS
evaluator; emulated output values compared; every real witness is a gnark solver solution), G-TRIV
(battery P1 + P2) and a sound counterexample search.  Every row ends in one terminal status: OPEN,
GATE-FAIL, DET-FALSE-CANDIDATE, TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or NOT-APPLICABLE (with a
reason).  The driver never writes a proof of a DET statement.
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
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import check as C                     # noqa: E402
from zk_registry import det_search as DS               # noqa: E402
from zk_registry import gates as G                     # noqa: E402
from zk_registry import gnark_instantiation as I       # noqa: E402
from zk_registry import gnark_lean_emit as GE          # noqa: E402
from zk_registry import gnark_r1cs as GR               # noqa: E402
from zk_registry import lean_emit as E                 # noqa: E402
from zk_registry import lean_runner as L               # noqa: E402
from zk_registry import package as P                   # noqa: E402
from zk_registry import r1cs as R                      # noqa: E402
from zk_registry import witness as W                   # noqa: E402

GENERATOR_NAME = "boole-zk-registry-gnark-det"
GENERATOR_VERSION = "1.0"
_HERE = os.path.dirname(os.path.abspath(__file__))
TOOL_DIR = os.path.join(_HERE, "gnark_tool")
GNARK_SOURCES = ["gnark_det.py", "gnark_instantiation.py", "gnark_r1cs.py", "gnark_lean_emit.py", "gnark_decompose.py",
                 "gnark_tool/main.go", "gnark_tool/harness/builder.go", "gnark_tool/harness/expose.go",
                 "gnark_tool/harness/registry.go", "gnark_tool/harness/sample.go", "gnark_tool/harness/run.go",
                 "gnark_tool/catalog/main.go"]
SHARED_SOURCES = ["__init__.py", "r1cs.py", "package.py", "jsonschema_lite.py", "lean_runner.py", "lean_emit.py",
                  "gates.py", "check.py", "det_search.py", "witness.py", "lean/ZkReplay.lean",
                  "schema/problem.schema.json"]

MAX_CONSTRAINTS = P.MAX_CONSTRAINTS
SIZING_LIMIT = 100000                 # guard of every compile (sizes above it are lower bounds)
SAMPLES = 48
REAL_WANTED = 16
MUTANTS = 16
PROBED_NOT_A_FINDING = ("not-a-finding: probed instantiation (a slice length or constant chosen by the generator, not "
                        "taken from the repository)")
X_TOOLS_DEFAULT = "v0.38.0"
MAX_JOBS = 12                          # item workers; Lean processes are limited separately (WaveConfig.lean_jobs)

# ------------------------------------------------------------------------------------------ population

EXCLUDED_UNITS_SUFFIX = ("-constraint-site", "-interaction-site")
EXCLUDED_UNITS = ("C2-operation", "U1-instruction")
EXCLUDED_FLAG_SUBSTRINGS = ("test", "not-counted", "deprecated", "NOT-ITEMIZED", "ZERO-ITEMS", "supplementary",
                            "exported-api-false", "intrinsic", "generated", "duplicate-of", "copy-of", "vendored",
                            "mapped-to", "archived", "excluded", "secondary", "program-as-circuit")
SELECTION_FILTER = {
    "framework": "gnark",
    "excluded_unit_suffixes": list(EXCLUDED_UNITS_SUFFIX),
    "excluded_units": list(EXCLUDED_UNITS),
    "excluded_flag_substrings": list(EXCLUDED_FLAG_SUBSTRINGS),
}


def selected(row: dict) -> bool:
    if row.get("framework") != "gnark":
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
    repos: dict          # repo_id -> {"dir", "url", "commit", "release", "module_dir"}
    go: dict             # name -> {"goroot", "version", "source", "sha256"}
    scratch: str
    lean_env: str
    out: str
    work: str
    build: str
    jobs: int = 6
    lean_jobs: int = 6                         # concurrent Lean processes (memory: battery runs peak at ~8 GB)
    only: list = field(default_factory=list)
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    det_search_budget_s: float = 20.0
    run_timeout: float = 900
    samples: int = SAMPLES

    @classmethod
    def load(cls, path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            return cls(**json.load(f))


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in SHARED_SOURCES + GNARK_SOURCES:
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(_HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


def generator_files() -> list[str]:
    """Files hashed by this generator only (the circom generator hashes the shared ones too)."""
    return [f for f in GNARK_SOURCES if f.endswith((".py", ".js", ".lean", ".json"))]


GNARK_DET_SPEC_CLAUSES = [
    "Output determinism: for the compiled constraint system of the generated wrapper circuit of the gnark gadget "
    "(gnark's R1CS builder at the pinned commit), any two wire assignments that satisfy every constraint and agree on "
    "every input wire (the wrapper's public and secret variables) agree on every output wire (the wires exposing the "
    "gadget's results); results that are emulated field elements are compared as integers modulo the emulated modulus.",
    "Commitment model: range checks use gnark's own non-commitment range checker (bit decomposition); a check that "
    "consumes a commitment challenge must be a polynomial identity of degree D in the challenge, and the model holds it "
    "at D + 1 distinct fixed challenges, which is equivalent to the identity holding for every challenge.",
]
GNARK_DET_SPEC = {
    "tier": "T1",
    "source": "mathematical definition of output determinism (functional dependence of outputs on inputs)",
    "clauses": GNARK_DET_SPEC_CLAUSES,
    "sha256": hashlib.sha256("\n".join(GNARK_DET_SPEC_CLAUSES).encode("utf-8")).hexdigest(),
}


def statement_assumptions(curve: str, commitment: dict, hints: dict, emulated: bool) -> list[str]:
    out = [f"[Fact (Nat.Prime p)]: primality of the {curve} scalar field order, supplied as an instance hypothesis so "
           "that field lemmas apply; p is prime, so the hypothesis does not weaken the statement."]
    out.append("Range checks are gnark's non-commitment range checker (bit decomposition, rangecheck.New on a builder "
               "without Committer); the production builder uses a log-derivative argument for the same predicate "
               "(value < 2^bits).")
    if commitment.get("commits"):
        out.append(f"The circuit takes one commitment (Fiat-Shamir challenge); every check that consumes it is a "
                   f"polynomial identity of degree {commitment['degree']} in the challenge, held at "
                   f"{commitment['points']} distinct fixed challenges {commitment['challenges'][:3]}..., which is "
                   "equivalent to the identity holding for every challenge (the commitment is modelled as sound: no "
                   "challenge is a free value of the assignment).")
    if hints:
        out.append(f"{sum(hints.values())} hint output wire(s) ({', '.join(sorted(h.rsplit('/', 1)[-1] for h in hints))}) "
                   "are unconstrained prover inputs: they are free in the model and add no constraint.")
    if emulated:
        out.append("Emulated outputs are compared by value: the integer of the little-endian limbs modulo the emulated "
                   "modulus (gnark does not keep emulated elements in a canonical representation).")
    return out


# ------------------------------------------------------------------------------------------ shared state

@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    ledger_sha: str
    generator: dict
    repos: dict = field(default_factory=dict)            # repo_id -> RepoBuild
    lock: threading.Lock = field(default_factory=threading.Lock)
    lean_slots: threading.Semaphore | None = None
    repo_failures: dict = field(default_factory=dict)
    sizing: dict = field(default_factory=dict)            # item -> production size (sizing fallback)


def log(sh: Shared, msg: str) -> None:
    with sh.lock:
        print(time.strftime("%H:%M:%S"), msg, flush=True)


def scrub_text(sh: Shared | None, s: str) -> str:
    roots = []
    if sh is not None:
        roots = [sh.cfg.scratch, sh.cfg.work, sh.cfg.build] + [r["dir"] for r in sh.cfg.repos.values()]
    roots.append(os.path.expanduser("~"))
    for root in sorted({r for r in roots if r}, key=len, reverse=True):
        s = s.replace(root, "$LOCAL")
    return s


# ------------------------------------------------------------------------------------------ toolchain

def go_env(cfg: WaveConfig, go_name: str) -> dict:
    g = cfg.go[go_name]
    s = cfg.scratch
    env = {
        "PATH": os.path.join(g["goroot"], "bin") + ":/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": os.path.join(s, "cache", "home"), "TMPDIR": os.path.join(s, "tmp"),
        "GOROOT": g["goroot"], "GOPATH": os.path.join(s, "cache", "gopath"),
        "GOMODCACHE": os.path.join(s, "cache", "gomod"), "GOCACHE": os.path.join(s, "cache", "gobuild"),
        "GOTOOLCHAIN": "local", "GOPROXY": "https://proxy.golang.org", "GOSUMDB": "sum.golang.org",
        "GOTELEMETRY": "off", "GOENV": "off", "GOFLAGS": "-mod=mod", "CGO_ENABLED": "1",
        "GIT_TERMINAL_PROMPT": "0", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
        "XDG_CACHE_HOME": os.path.join(s, "cache", "xdg"),
    }
    for d in ("HOME", "TMPDIR", "GOPATH", "GOMODCACHE", "GOCACHE"):
        os.makedirs(env[d], exist_ok=True)
    return env


def parse_version(v: str) -> tuple:
    return tuple(int(x) for x in re.findall(r"\d+", v)[:3]) + (0,) * (3 - len(re.findall(r"\d+", v)[:3]))


def go_requirement(gomod_text: str) -> tuple[str, str]:
    """(go directive, toolchain directive or '')."""
    m = re.search(r"^go\s+(\S+)", gomod_text, re.M)
    t = re.search(r"^toolchain\s+(\S+)", gomod_text, re.M)
    return (m.group(1) if m else "1.0"), (t.group(1) if t else "")


def pick_go(cfg: WaveConfig, required: str) -> str:
    """The configured Go toolchain to build a module whose go.mod requires ``required``: the installed one
    when it is at least the requirement, otherwise the smallest configured release that is."""
    ok = [(parse_version(g["version"]), name) for name, g in cfg.go.items()
          if parse_version(g["version"]) >= parse_version(required)]
    if not ok:
        raise RuntimeError(f"no configured Go toolchain satisfies go {required}")
    installed = [x for x in ok if cfg.go[x[1]].get("installed")]
    return (min(installed) if installed else min(ok))[1]


def run_cmd(args, cwd, env, timeout=1800) -> subprocess.CompletedProcess:
    return subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)


@dataclass
class RepoBuild:
    repo_id: str
    root: str                    # checkout
    module_dir: str              # directory of the module holding the targets (relative to root)
    module_path: str
    gomod: str
    go_required: str
    go_toolchain_directive: str
    go_name: str
    build_dir: str
    catalog: I.Catalog | None = None
    binary: str = ""
    binary_sha256: str = ""
    gosum_sha256: str = ""
    version_deviations: list = field(default_factory=list)
    gnark_version: str = ""
    wrapper_errors: dict = field(default_factory=dict)
    setup_notes: list = field(default_factory=list)
    in_module: bool = False      # wrappers built inside a copy of the repository module (targets in internal packages)
    modinfo: dict = field(default_factory=dict)
    sizing: bool = False         # the wrapper tool does not build: production sizes only


CATALOG_XTOOLS = "v0.48.0"
_CATALOG_LOCK = threading.Lock()


def catalog_binary(cfg: WaveConfig) -> str:
    """The catalog tool, built once in its own module (it imports only golang.org/x/tools; the repository's
    packages are loaded by go/packages with the repository's own module and Go toolchain)."""
    with _CATALOG_LOCK:
        cdir = os.path.join(cfg.build, "_catalog")
        binary = os.path.join(cdir, "catalogx")
        if os.path.exists(binary):
            return binary
        os.makedirs(os.path.join(cdir, "catalog"), exist_ok=True)
        shutil.copyfile(os.path.join(TOOL_DIR, "catalog", "main.go"), os.path.join(cdir, "catalog", "main.go"))
        name = min(cfg.go, key=lambda n: (not cfg.go[n].get("installed"), parse_version(cfg.go[n]["version"])))
        env = go_env(cfg, name)
        with open(os.path.join(cdir, "go.mod"), "w", encoding="utf-8") as f:
            f.write(f"module boole.local/gnarkcatalog\n\ngo 1.24\n\nrequire golang.org/x/tools {CATALOG_XTOOLS}\n")
        for args in (["go", "mod", "tidy"], ["go", "build", "-o", binary, "./catalog"]):
            r = run_cmd(args, cdir, env)
            if r.returncode != 0:
                raise RuntimeError(f"catalog tool: {' '.join(args)} failed: " + r.stderr[-1500:])
        return binary


def prepare_catalog(cfg: WaveConfig, repo_id: str, targets: list[dict]) -> RepoBuild:
    """Repository facts (module, Go requirement, chosen Go) and the catalog of the targets."""
    rc = cfg.repos[repo_id]
    root = rc["dir"]
    mdir = os.path.join(root, rc.get("module_dir", ""))
    with open(os.path.join(mdir, "go.mod"), encoding="utf-8") as f:
        gomod = f.read()
    module_path = re.search(r"^module\s+(\S+)", gomod, re.M).group(1)
    go_req, tc = go_requirement(gomod)
    go_name = pick_go(cfg, go_req)
    bdir = os.path.join(cfg.build, repo_dir_name(repo_id))
    rb = RepoBuild(repo_id, root, rc.get("module_dir", ""), module_path, gomod, go_req, tc, go_name, bdir)
    env = go_env(cfg, go_name)
    os.makedirs(bdir, exist_ok=True)
    mj = run_cmd(["go", "mod", "edit", "-json", os.path.join(mdir, "go.mod")], bdir, env)
    if mj.returncode != 0:
        raise RuntimeError("go mod edit -json failed: " + mj.stderr[-500:])
    rb.modinfo = json.loads(mj.stdout)
    req = [(r["Path"], r["Version"]) for r in rb.modinfo.get("Require") or []]
    rb.gnark_version = next((v for m, v in req if m == "github.com/consensys/gnark"), "") or "pinned checkout"
    for rp in rb.modinfo.get("Replace") or []:
        if rp["Old"]["Path"] == "github.com/consensys/gnark":
            rb.gnark_version += f" (replaced by {rp['New']['Path']} {rp['New'].get('Version', '')})".rstrip()
    tpath = os.path.join(bdir, "targets.json")
    with open(tpath, "w", encoding="utf-8") as f:
        json.dump([{"path": os.path.relpath(os.path.join(root, t["path"]), mdir), "symbol": t["symbol"],
                    "line": t["line"]} for t in targets], f)
    catx = catalog_binary(cfg)
    cenv = dict(env, GOFLAGS="-mod=readonly")
    status_before = run_cmd(["git", "status", "--porcelain"], root, env).stdout
    r = run_cmd([catx, "-dir", ".", "-targets", tpath, "-out", os.path.join(bdir, "catalog.json"), "./..."], mdir, cenv)
    if r.returncode != 0:
        rb.setup_notes.append("catalog with -mod=readonly failed; retried with -mod=mod: " + r.stderr[-300:])
        r = run_cmd([catx, "-dir", ".", "-targets", tpath, "-out", os.path.join(bdir, "catalog.json"), "./..."], mdir, env)
        if r.returncode != 0:
            raise RuntimeError("catalog failed: " + r.stderr[-1500:])
    if run_cmd(["git", "status", "--porcelain"], root, env).stdout != status_before:
        run_cmd(["git", "checkout", "--", "."], root, env)
        rb.setup_notes.append("the catalog run touched the checkout; restored with git checkout")
    rb.catalog = I.Catalog.load(os.path.join(bdir, "catalog.json"), mdir)
    rb.in_module = any("/internal/" in "/" + os.path.relpath(os.path.join(root, t["path"]), mdir) for t in targets)
    if rb.in_module:
        rb.setup_notes.append("targets in internal packages: wrappers are built inside a copy of the repository module "
                              "(its own go.mod and go.sum)")
    return rb


def prepare_harness(cfg: WaveConfig, rb: RepoBuild) -> None:
    """The wrapper tool module against the pinned checkout (Go ``replace``), with the repository's own
    replace directives, and the check that every module the repository requires resolves to its version."""
    bdir, mdir = rb.build_dir, os.path.join(rb.root, rb.module_dir)
    env = go_env(cfg, rb.go_name)
    for sub in ("harness", "wrappers"):
        os.makedirs(os.path.join(bdir, sub), exist_ok=True)
    shutil.copyfile(os.path.join(TOOL_DIR, "main.go"), os.path.join(bdir, "main.go"))
    for fn in sorted(os.listdir(os.path.join(TOOL_DIR, "harness"))):
        if fn.endswith(".go"):
            shutil.copyfile(os.path.join(TOOL_DIR, "harness", fn), os.path.join(bdir, "harness", fn))
    req = [(r["Path"], r["Version"]) for r in rb.modinfo.get("Require") or []]
    gnark_req = next((v for m, v in req if m == "github.com/consensys/gnark"), "")
    lines = ["module boole.local/gnarkx", "", f"go {rb.go_required}", "", "require ("]
    lines.append(f"\t{rb.module_path} v0.0.0-00010101000000-000000000000")
    if rb.module_path != "github.com/consensys/gnark" and gnark_req:
        lines.append(f"\tgithub.com/consensys/gnark {gnark_req}")
    lines += [")", "", f"replace {rb.module_path} => {mdir}"]
    for rp in rb.modinfo.get("Replace") or []:
        old, new = rp["Old"], rp["New"]
        src = old["Path"] + (f" {old['Version']}" if old.get("Version") else "")
        dst = new["Path"]
        if not new.get("Version"):                      # local directory replacement
            dst = os.path.normpath(os.path.join(mdir, dst))
            if not os.path.isdir(dst):
                rb.setup_notes.append(f"replace {old['Path']} => {new['Path']} points outside the checkout; the "
                                      "required version is fetched from the module proxy instead")
                continue
        lines.append(f"replace {src} => {dst}" + (f" {new['Version']}" if new.get("Version") else ""))
        rb.setup_notes.append(f"repository replace kept: {old['Path']} => {new['Path']} {new.get('Version', '')}".strip())
    with open(os.path.join(bdir, "go.mod"), "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    _write_placeholder(bdir)
    r = run_cmd(["go", "mod", "tidy"], bdir, env)
    if r.returncode != 0:
        raise RuntimeError("go mod tidy of the wrapper tool failed: " + _first_go_error(r.stderr))
    rb.gosum_sha256 = P.sha256_file(os.path.join(bdir, "go.sum"))
    lm = run_cmd(["go", "list", "-m", "-json", "all"], bdir, env)
    resolved = {}
    for blob in re.findall(r"\{.*?\n\}", lm.stdout, re.S):
        d = json.loads(blob)
        resolved[d["Path"]] = (d.get("Replace") or {}).get("Version") or d.get("Version", "")
    for m, v in req:
        got = resolved.get(m)
        if got is not None and got != v and m != rb.module_path:
            rb.version_deviations.append({"module": m, "repository": v, "resolved": got})
    sem = semantic_deviations(rb.version_deviations)
    if sem:
        raise RuntimeError("the wrapper tool would compile against " + ", ".join(
            f"{d['module']} {d['resolved']} instead of the pinned {d['repository']}" for d in sem))


SEMANTIC_MODULES = ("github.com/consensys/gnark", "github.com/consensys/gnark-crypto")


def semantic_deviations(devs: list[dict]) -> list[dict]:
    """Version deviations that change the compiled circuit (gnark and gnark-crypto): the wrapper tool is
    not used then (production sizing in a copy of the repository module instead)."""
    return [d for d in devs if d["module"] in SEMANTIC_MODULES]


def _first_go_error(stderr: str) -> str:
    lines = [ln for ln in stderr.splitlines() if ln.strip() and not ln.startswith("go: downloading")
             and not ln.startswith("go: finding") and not ln.startswith("go: found")]
    return " | ".join(lines[:3])[:700]


def _write_placeholder(bdir: str) -> None:
    wdir = os.path.join(bdir, "wrappers")
    for fn in os.listdir(wdir):
        if fn.endswith(".go"):
            os.remove(os.path.join(wdir, fn))
    with open(os.path.join(wdir, "doc.go"), "w", encoding="utf-8") as f:
        f.write("// Package wrappers holds generated wrapper circuits.\npackage wrappers\n\n"
                "import (\n\t_ \"github.com/consensys/gnark-crypto/ecc\"\n\t_ \"boole.local/gnarkx/harness\"\n)\n")


_ERR_RE = re.compile(r"^(?:\./)?(?:boolegnarkx/)?wrappers/(w[0-9a-f]{12}_\d+)\.go:\d+:\d+: (.*)$", re.M)


def _in_module_tree(cfg: WaveConfig, rb: RepoBuild) -> tuple[str, str]:
    """(module copy directory, tool directory inside it) for in-module builds; the tool's import path is
    rewritten to ``<module>/boolegnarkx``."""
    mcopy = os.path.join(rb.build_dir, "modcopy")
    if not os.path.isdir(mcopy):
        shutil.copytree(os.path.join(rb.root, rb.module_dir), mcopy, ignore=shutil.ignore_patterns(".git"))
    tool = os.path.join(mcopy, "boolegnarkx")
    os.makedirs(os.path.join(tool, "harness"), exist_ok=True)
    os.makedirs(os.path.join(tool, "wrappers"), exist_ok=True)
    imp = f"{rb.module_path}/boolegnarkx"
    for rel in ["main.go"] + [os.path.join("harness", f) for f in sorted(os.listdir(os.path.join(TOOL_DIR, "harness")))
                              if f.endswith(".go")]:
        with open(os.path.join(TOOL_DIR, rel), encoding="utf-8") as f:
            text = f.read().replace("boole.local/gnarkx/", imp + "/")
        with open(os.path.join(tool, rel), "w", encoding="utf-8") as f:
            f.write(text)
    return mcopy, tool


def build_wrappers(cfg: WaveConfig, rb: RepoBuild, wrappers: dict[str, str]) -> None:
    """Write the wrappers, build ``gnarkx``; wrappers that do not compile are removed (their first error
    recorded) and the build is repeated."""
    env = go_env(cfg, rb.go_name)
    if rb.in_module:
        cwd, tool = _in_module_tree(cfg, rb)
        _write_placeholder(tool)
        wdir = os.path.join(tool, "wrappers")
        imp = f"{rb.module_path}/boolegnarkx"
        wrappers = {k: v.replace("boole.local/gnarkx/", imp + "/") for k, v in wrappers.items()}
        with open(os.path.join(wdir, "doc.go"), "w", encoding="utf-8") as f:
            f.write(f"// Package wrappers holds generated wrapper circuits.\npackage wrappers\n\n"
                    f"import (\n\t_ \"github.com/consensys/gnark-crypto/ecc\"\n\t_ \"{imp}/harness\"\n)\n")
        build_args = ["go", "build", f"-gcflags={imp}/wrappers=-e", "-o", os.path.join(rb.build_dir, "gnarkx"),
                      "./boolegnarkx"]
    else:
        cwd = rb.build_dir
        wdir = os.path.join(rb.build_dir, "wrappers")
        _write_placeholder(rb.build_dir)
        build_args = ["go", "build", "-gcflags=boole.local/gnarkx/wrappers=-e", "-o", "gnarkx", "."]
    for wid, src in sorted(wrappers.items()):
        with open(os.path.join(wdir, f"{wid}.go"), "w", encoding="utf-8") as f:
            f.write(src)
    for _ in range(20):
        r = run_cmd(build_args, cwd, env)
        if r.returncode == 0:
            rb.binary = os.path.join(rb.build_dir, "gnarkx")
            rb.binary_sha256 = P.sha256_file(rb.binary)
            return
        bad = {}
        for wid, msg in _ERR_RE.findall(r.stderr):
            bad.setdefault(wid, msg)
        if not bad:
            raise RuntimeError("wrapper build failed: " + _first_go_error(r.stderr))
        for wid, msg in bad.items():
            rb.wrapper_errors[wid] = "Go compile error in the wrapper: " + msg[:300]
            os.remove(os.path.join(wdir, f"{wid}.go"))
    raise RuntimeError("wrapper build did not converge")


# ------------------------------------------------------------------------------------------ records

def dir_name_for(row: dict, label: str = "") -> str:
    stem = re.sub(r"\.go$", "", row["path"]).replace("/", ".")
    name = f"{stem}.{row['symbol']}"
    if label:
        name += ".h" + hashlib.sha256(label.encode()).hexdigest()[:10]
    name = re.sub(r"[^A-Za-z0-9._-]", "_", name)
    if len(name) > 180:
        name = name[:150] + ".h" + hashlib.sha256(name.encode()).hexdigest()[:12]
    return name


def coverage_of(row: dict) -> dict:
    """Public machine-checked coverage of the code at the pin (from the ledger's census record)."""
    cov = row.get("coverage", "none")
    census = (row.get("census") or [{}])[0]
    if cov in ("conditional", "partial", "full"):
        return {"class": "partial" if cov != "full" else "full", "same_pin": True,
                "source": census.get("coverage_source", ""),
                "property": "in-repository Lean 4 formal verification of the circuit extracted by gnark-lean-extractor "
                            "(reilabs proven-zk); not a DET statement over the compiled R1CS",
                "ledger_coverage": cov, "census": census.get("coverage_raw", "")}
    out = {"class": "none", "ledger_coverage": cov, "census": census.get("coverage_raw", ""),
           "census_source": census.get("coverage_source", "")}
    if repo_id_of(row.get("repo", "")) == "Consensys/gnark":
        out["note"] = ("no public machine-checked artifact of the gnark standard library was found (web search, "
                       "2026-10-02); reilabs gnark-lean-extractor / proven-zk verify application circuits (e.g. the "
                       "Semaphore Merkle tree batcher, light-protocol), not std gadgets")
    return out


def base_record(sh: Shared, row: dict, decl: dict | None, dir_name: str) -> dict:
    rid = repo_id_of(row["repo"])
    census = (row.get("census") or [{}])[0]
    src_sha = (decl or {}).get("sha") or "0" * 64
    return {
        "schema_version": P.SCHEMA_VERSION,
        "package_id": f"{repo_dir_name(rid)}/{dir_name}",
        "property": dict(P.DET_PROPERTY),
        "status": "NO-INSTANTIATION", "status_reason": "",
        "ids": {"ledger_item_id": row["item_id"],
                "ledger": {"file": "ledger/LEDGER-v1.jsonl", "sha256": sh.ledger_sha, "row_found": True},
                "repo": rid, "repo_url": row["repo"],
                "release": str(census.get("pin") or sh.cfg.repos.get(rid, {}).get("release", "")),
                "commit": row["commit"], "path": row["path"], "template": row["symbol"],
                "template_line": int((decl or {}).get("line") or census.get("line") or 1),
                "source_sha256": src_sha},
        "instantiation": {"rule": "none", "args": [], "call": "", "provenance": [], "selection": "", "candidates": []},
        "spec": dict(GNARK_DET_SPEC),
        "env": {"gnark": "", "lean": sh.env.lean_version, "mathlib": sh.env.packages.get("mathlib", ""),
                "lake_manifest_sha256": sh.env.manifest_sha256, "packages": dict(sorted(sh.env.packages.items())),
                "python": platform.python_version()},
        "generator": dict(sh.generator),
        "evidence": {"coverage": coverage_of(row)},
    }


def finish(rec: dict, t0: float) -> dict:
    rec["evidence"]["wall_s"] = round(time.time() - t0, 2)
    return rec


# ------------------------------------------------------------------------------------------ content key

def content_key(rb: RepoBuild, tgt: dict) -> str:
    """sha256 over the go version line, the comment-free declaration and, transitively, every function of
    the module it calls, and the underlying types of the named types its signature reaches."""
    cat = rb.catalog
    decl = tgt["decl"]
    seen, stack, shas = set(), list(decl.get("callees") or []), []
    while stack:
        k = stack.pop()
        if k in seen:
            continue
        seen.add(k)
        fi = cat.data.get("index", {}).get(k)
        if fi is None:
            shas.append("ext:" + k)
            continue
        shas.append(fi["sha"])
        stack += fi.get("callees") or []
    types, tstack, tseen = [], [], set()

    def walk(t):
        if not isinstance(t, dict):
            return
        if t.get("k") == "named":
            tstack.append(t)
        for a in t.get("args") or []:
            walk(a)
        walk(t.get("elem"))
    for p in (decl.get("sig") or {}).get("params", []) + (decl.get("sig") or {}).get("results", []):
        walk(p["type"])
    walk(decl.get("recv"))
    walk(decl.get("type"))
    while tstack:
        t = tstack.pop()
        k = I.key_of(t)
        if k in tseen:
            continue
        tseen.add(k)
        u = I.underlying(cat, t)
        if u is None:
            types.append("ext:" + I.show(t))
            continue
        fields = I.field_types(cat, t) or []
        types.append(I.key_of(u) + json.dumps([[n, ft, tag] for n, ft, tag in fields], sort_keys=True))
        walk(u)
        for _, ft, _ in fields:
            walk(ft)
    blob = json.dumps({"go": rb.go_required, "gnark": rb.gnark_version, "decl": decl.get("sha"),
                       "kind": decl.get("kind"), "name": decl.get("name"),
                       "callees": sorted(shas), "types": sorted(types)}, sort_keys=True)
    return hashlib.sha256(blob.encode()).hexdigest()


# ------------------------------------------------------------------------------------------ one item

@dataclass
class Cand:
    wid: str
    choice: I.Choice
    wrapper: I.Wrapper | None
    tier: str
    result: dict | None = None
    status: str = "pending"
    error: str = ""


def plan_item(sh: Shared, rb: RepoBuild, row: dict) -> tuple[dict | None, I.Plan | None, list[Cand], str, str]:
    """(target, plan, candidates, terminal status, reason) before compiling."""
    cat = rb.catalog
    rel = os.path.relpath(os.path.join(rb.root, row["path"]), os.path.join(rb.root, rb.module_dir))
    tgt = cat.targets.get((rel, row["symbol"]))
    if tgt is None or not tgt.get("found"):
        return tgt, None, [], "NO-INSTANTIATION", f"declaration not located: {(tgt or {}).get('why', 'not in catalog')}"
    try:
        plan = I.plan_target(cat, tgt)
    except I.NotApplicable as ex:
        return tgt, None, [], "NOT-APPLICABLE", str(ex)
    except I.NoInstantiation as ex:
        return tgt, None, [], "NO-INSTANTIATION", str(ex)
    cands = []
    errors = []
    for k, ch in enumerate(plan.choices):
        wid = I.wrapper_id(row["item_id"], k)
        try:
            w = I.render(cat, plan, ch, wid)
        except I.NoInstantiation as ex:
            errors.append(str(ex))
            continue
        tier = "probed" if w.probed_length else ch.tier
        cands.append(Cand(wid, ch, w, tier))
    if not cands:
        return tgt, plan, [], "NO-INSTANTIATION", "no candidate wrapper: " + "; ".join(dict.fromkeys(errors))[:500]
    return tgt, plan, cands, "", ""


def run_candidate(sh: Shared, rb: RepoBuild, cand: Cand, work: str) -> None:
    if cand.wid in rb.wrapper_errors:
        cand.status, cand.error = "compile-error", rb.wrapper_errors[cand.wid]
        return
    out = os.path.join(work, f"{cand.wid}.json")
    env = go_env(sh.cfg, rb.go_name)
    try:
        r = run_cmd([rb.binary, "run", cand.wid, out, "-limit", str(SIZING_LIMIT), "-size-policy", str(MAX_CONSTRAINTS),
                     "-samples", str(sh.cfg.samples)], work, env, timeout=sh.cfg.run_timeout)
    except subprocess.TimeoutExpired:
        cand.status, cand.error = "timeout", f"gnarkx run exceeded {sh.cfg.run_timeout:.0f} s"
        return
    if r.returncode != 0 or not os.path.exists(out):
        msg = (r.stderr or r.stdout).strip().splitlines()
        first = next((ln for ln in msg if ln.startswith(("panic:", "fatal error"))), msg[0] if msg else "no output")
        cand.status, cand.error = "crash", scrub_text(sh, first)[:400]
        return
    with open(out, encoding="utf-8") as f:
        cand.result = json.load(f)
    cand.status = cand.result["status"]
    cand.error = scrub_text(sh, cand.result.get("error", ""))[:600]


def candidate_entry(c: Cand) -> dict:
    res = c.result or {}
    compiled = c.status in ("ok", "too-large")
    e = {"tier": c.tier, "call": c.wrapper.call if c.wrapper else "",
         "compile": "ok" if compiled else ("skipped" if c.status == "pending" else "error"),
         "compiler": f"gnark ({res.get('gnark_version') or 'pinned'}) {res.get('curve', '')}".strip()}
    if compiled:
        e["constraints"] = int(res.get("model_size") or 0)
        sym = res.get("symbolic") or {}
        e["wires"] = max(1, int(sym.get("n_wires") or 1))
    if c.error and not compiled:
        e["error"] = c.error
    elif res.get("size_lower_bound"):
        e["error"] = f"size is a lower bound (compile stopped above {res.get('model_size')})"
    return e


def process(sh: Shared, row: dict) -> dict:
    t0 = time.time()
    try:
        return finish(_process(sh, row), t0)
    except Exception as ex:                                   # recorded, never dropped
        rec = base_record(sh, row, None, f"error.{hashlib.sha256(row['item_id'].encode()).hexdigest()[:16]}")
        rec["status"] = "COMPILE-FAIL"
        rec["status_reason"] = scrub_text(sh, f"generator error: {type(ex).__name__}: {ex}")[:500]
        return finish(rec, t0)


def _process(sh: Shared, row: dict) -> dict:
    rid = repo_id_of(row["repo"])
    tgt, plan, cands, status, reason = sh.plans[row["item_id"]]
    decl = (tgt or {}).get("decl")
    rec = base_record(sh, row, decl, dir_name_for(row))
    rb: RepoBuild | None = sh.repos.get(rid)
    if rb is None:
        rec["status"], rec["status_reason"] = status, reason
        return rec
    rec["env"]["gnark"] = rb.gnark_version
    rec["env"]["go"] = f"{sh.cfg.go[rb.go_name]['version']} ({sh.cfg.go[rb.go_name]['source']})"
    rec["env"]["go_required"] = rb.go_required + (f" (toolchain {rb.go_toolchain_directive})" if rb.go_toolchain_directive else "")
    if decl and decl.get("doc"):
        rec["evidence"]["doc_excerpt"] = decl["doc"][:600]
    if tgt and tgt.get("key"):
        rec["evidence"]["catalog_key"] = tgt["key"]
    if row.get("_decomposition"):
        rec["instantiation"]["decomposition"] = dict(row["_decomposition"])
    if status:
        rec["status"], rec["status_reason"] = status, reason
        sz = sh.sizing.get(row["item_id"])
        if sz is not None:
            rec["evidence"]["production_sizing"] = sz
            if status == "TOO-LARGE":
                rec["circuit"] = {"compiler": {"name": "gnark", "version": rb.gnark_version,
                                               "flags": ["frontend.Compile", "r1cs.NewBuilder (production)", "field bn254"],
                                               "binary_sha256": P.sha256_file(os.path.join(rb.build_dir, "gnarksize")),
                                               "source": f"{rb.repo_id}@{row['commit'][:12]} (module copy)"},
                                  "prime": str(R.BN254_SCALAR), "prime_name": "bn254", "n_constraints": sz["n"],
                                  "n_wires": 1, "size_policy": {"max_constraints": MAX_CONSTRAINTS, "within": False},
                                  "gnark": {"production_constraints": sz["n"], "sizing_only": True}}
                rec["evidence"]["callees"] = list((decl or {}).get("callees") or [])[:60]
        return rec
    rec["evidence"]["curve_selection"] = plan.curve_why
    rec["instantiation"]["rule_order"] = list(dict.fromkeys(c.tier for c in cands))
    work = os.path.join(sh.cfg.work, hashlib.sha256(row["item_id"].encode()).hexdigest()[:20])
    os.makedirs(work, exist_ok=True)
    chosen = None
    tried: list[Cand] = []
    for tier in rec["instantiation"]["rule_order"]:
        tier_c = [c for c in cands if c.tier == tier]
        for c in tier_c:
            run_candidate(sh, rb, c, work)
        tried += tier_c
        ok = [c for c in tier_c if c.status in ("ok", "too-large")]
        if ok:
            within = [c for c in ok if c.status == "ok"]
            if within:
                chosen = max(within, key=lambda c: (c.result["model_size"], c.wid))
                sel = f"tier {tier}: largest compiled instantiation within {MAX_CONSTRAINTS} constraints"
            else:
                chosen = min(ok, key=lambda c: (c.result["model_size"], c.wid))
                sel = f"tier {tier}: every compiled instantiation exceeds {MAX_CONSTRAINTS} constraints; smallest recorded"
            rec["instantiation"]["selection"] = sel
            break
    rec["instantiation"]["candidates"] = [candidate_entry(c) for c in tried] + [
        dict(candidate_entry(c), compile="skipped") for c in cands if c not in tried]
    if chosen is None:
        nm = [c for c in tried if c.status == "not-modelled"]
        first = next((c for c in tried if c.status != "not-modelled"), tried[0] if tried else None)
        rec["status"] = "COMPILE-FAIL"
        if nm and len(nm) == len(tried):
            rec["status_reason"] = "not modelled: " + nm[0].error
        else:
            rec["status_reason"] = (f"{len(tried)} candidate(s) failed; first ({first.tier}): {first.error}")[:600]
        shutil.rmtree(work, ignore_errors=True)
        return rec
    ch = chosen.choice
    rec["instantiation"].update(rule=chosen.tier, args=[a for a in ch.label().split(", ") if a], call=chosen.wrapper.call,
                                params=list(plan.tparams), provenance=list(ch.provenance)[:12])
    if row.get("_decomposition"):
        rec["instantiation"]["provenance"].insert(0, f"decomposition child (depth {row['_decomposition']['depth']}) "
                                                     f"instantiated by the planner's tier {chosen.tier}")
    rec["instantiation"]["main_sha256"] = hashlib.sha256(chosen.wrapper.source.encode()).hexdigest()
    if ch.label():
        rec["package_id"] = f"{repo_dir_name(rid)}/{dir_name_for(row, ch.label())}"
    res = chosen.result
    sym = res.get("symbolic") or {}
    prod = res.get("production") or {}
    circuit = {"compiler": {"name": "gnark", "version": rb.gnark_version,
                            "flags": ["frontend.Compile", "r1cs.NewBuilder (wrapped: model builder)", f"field {res['curve']}"],
                            "binary_sha256": rb.binary_sha256, "source": f"{rb.repo_id}@{row['commit'][:12]} (replace)"},
               "prime": res["field"], "prime_name": res["curve"], "n_constraints": int(res["model_size"]),
               "n_wires": max(1, int(sym.get("n_wires") or 1)),
               "size_policy": {"max_constraints": MAX_CONSTRAINTS, "within": chosen.status == "ok"}}
    circuit["gnark"] = {"production_constraints": prod.get("n_constraints"),
                        "production_commitments": prod.get("commits"),
                        "production_error": scrub_text(sh, res.get("production_error", ""))[:300] or None,
                        "symbolic_constraints": sym.get("n_constraints"), "commitments": sym.get("commits"),
                        "challenge_degree": res.get("degree"), "challenge_points": res.get("points"),
                        "range_checks": sym.get("range_checks"), "range_check_bits": sym.get("range_check_bits"),
                        "size_lower_bound": bool(res.get("size_lower_bound")),
                        "wrapper_id": chosen.wid, "curve": res["curve"]}
    rec["circuit"] = circuit
    if chosen.status == "too-large":
        rec["status"] = "TOO-LARGE"
        rec["status_reason"] = (f"{res['model_size']} model constraints" + (" (lower bound)" if res.get("size_lower_bound")
                                                                            else "") + f" > {MAX_CONSTRAINTS}")
        rec["evidence"]["callees"] = list(decl.get("callees") or [])[:60]
        shutil.rmtree(work, ignore_errors=True)
        return rec
    try:
        model = GR.build(res)
    except GR.ModelError as ex:
        rec.pop("circuit", None)
        rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"model not assembled: {ex}"
        shutil.rmtree(work, ignore_errors=True)
        return rec
    rec["circuit"].update(n_constraints=model.r.n_constraints, n_wires=model.r.n_wires, n_inputs=len(model.inputs),
                          n_outputs=len(model.outputs),
                          r1cs_sha256=hashlib.sha256(R.encode_r1cs(model.r)).hexdigest())
    return build_package(sh, rb, rec, row, plan, chosen, model, work)


# ------------------------------------------------------------------------------------------ package

def write_witness(path: str, w: list[int]) -> None:
    W.write_witness(path, w)


def make_mutants(model: GR.Model, real: list[list[int]], n: int, seed: str) -> list[dict]:
    r = model.r
    used = sorted({i for cons in r.constraints for lc in cons for i, _ in lc} | {0})
    rng = random.Random(seed + "/mutants")
    out = []
    if not real:
        return out
    p = r.prime
    for k in range(n):
        bi = k % len(real)
        w = list(real[bi])
        wire = 0 if k == 0 else rng.choice(used)
        old = w[wire]
        new = (old + rng.choice([1, p - 1, rng.randrange(1, p)])) % p
        if new == old:
            new = (old + 1) % p
        w[wire] = new
        out.append({"base": bi, "wire": wire, "witness": w, "oracle": R.satisfies(r, w)})
    return out


def lean_eval(sh: Shared, build: str, ns: str, model: GR.Model, files: list[tuple[str, str]], em_tags: list[str],
              work: str) -> tuple[dict, dict, L.RunResult]:
    groups = model.group_wires()
    src = os.path.join(work, "Fid.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(GE.emit_fid_runner(ns, model.r.n_constraints, files, groups, em_tags))
    r = L.run_lean(sh.env, GE.LEAN_OPTIONS + ["--json", src], work, 3600, extra_lean_path=[build], rss_limit_mb=16384)
    text = "\n".join(m.get("data", "") for m in L.parse_messages(r.out))
    verdicts = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
    emv: dict[str, list[int]] = {}
    for tag, v in re.findall(r"^EMV (\S+) (\d+)$", text, re.M):
        emv.setdefault(tag, []).append(int(v))
    if "FID-DONE" not in text or (groups and em_tags and "EMV-DONE" not in text):
        verdicts["__incomplete__"] = "; ".join(L.fmt_msg(m) for m in L.errors(L.parse_messages(r.out))[:3]) or r.out[-300:]
    return verdicts, emv, r


def g_fid_nonvac(sh: Shared, build: str, ns: str, model: GR.Model, real: list[list[int]], muts: list[dict],
                 input_free: bool, work: str) -> tuple[G.Gate, G.Gate]:
    wdir = os.path.join(work, "wit")
    os.makedirs(wdir, exist_ok=True)
    files = []
    for k, w in enumerate(real):
        p = os.path.join(wdir, f"real_{k:03d}.txt")
        write_witness(p, w)
        files.append((f"real_{k:03d}", p))
    for k, m in enumerate(muts):
        p = os.path.join(wdir, f"mut_{k:03d}.txt")
        write_witness(p, m["witness"])
        files.append((f"mut_{k:03d}", p))
    if not files:
        d = {"real_witnesses": 0, "reason": "gnark's solver solved no sampled assignment",
             "solver_errors": model.solver_errors}
        return G.Gate("G-FID", "FAIL", dict(d)), G.Gate("G-NONVAC", "FAIL", dict(d))
    groups = model.group_wires()
    em_tags = [f"real_{k:03d}" for k in range(min(len(real), 4))] if groups else []
    verdicts, emv, r = lean_eval(sh, build, ns, model, files, em_tags, work)
    real_py = [R.satisfies(model.r, w) for w in real]
    real_accept = sum(verdicts.get(f"real_{k:03d}") == "ACCEPT" for k in range(len(real)))
    agree = sum(verdicts.get(f"mut_{k:03d}") == ("ACCEPT" if m["oracle"] else "REJECT") for k, m in enumerate(muts))
    emv_ok = all(emv.get(t) == [GR.em_value(real[int(t[5:])], limbs, bits) % mod for limbs, bits, mod in groups]
                 for t in em_tags)
    engine = model.test_engine
    detail = {"method": "Lean #eval of decide (Constraints w) on every witness file, compared with the Python R1CS "
                        "evaluator; every real witness is a solution of gnark's solver for the compiled system(s)",
              "real_witnesses": len(real), "real_lean_accept": real_accept, "real_python_accept": sum(real_py),
              "gnark_solver_solutions": len(real), "gnark_test_engine": engine,
              "mutants": len(muts), "mutants_python_reject": sum(not m["oracle"] for m in muts),
              "mutants_lean_agree": agree, "lean_eval_secs": r.secs}
    if groups:
        detail["emulated_values_checked"] = len(em_tags)
        detail["emulated_values_lean_python_agree"] = emv_ok
    if "__incomplete__" in verdicts:
        detail["error"] = verdicts["__incomplete__"][:400]
        detail["reason"] = "the Lean evaluation did not complete (harness error); no verdict is inferred"
        return G.Gate("G-FID", "ERROR", dict(detail)), G.Gate("G-NONVAC", "ERROR", dict(detail))
    need_real = 1 if input_free else G.FID_MIN_REAL
    fid_ok = (real_accept == len(real) and sum(real_py) == len(real) and len(real) >= need_real
              and len(muts) >= G.FID_MIN_MUTANTS and agree == len(muts) and emv_ok)
    fd = dict(detail, required_real=need_real, input_free=input_free)
    if not fid_ok:
        reasons = []
        if len(real) < need_real:
            reasons.append(f"only {len(real)} distinct real witnesses (need {need_real})")
        if real_accept != len(real) or sum(real_py) != len(real):
            reasons.append("the Lean model or the Python evaluator rejected a real witness")
        if agree != len(muts) or len(muts) < G.FID_MIN_MUTANTS:
            reasons.append("Lean and Python verdicts differ on a mutant (or too few mutants)")
        if not emv_ok:
            reasons.append("Lean and Python disagree on an emulated output value")
        fd["reason"] = "; ".join(reasons)
    nonvac_ok = real_accept >= 1 and sum(real_py) >= 1
    nv = G.Gate("G-NONVAC", "PASS" if nonvac_ok else "FAIL",
                {"real_witnesses_accepted": real_accept,
                 "witness_source": "gnark's solver (constraint/<curve>.Solve) on sampled assignments of the wrapper "
                                   "inputs (gnark-crypto domain samplers for points, GT elements and bytes)",
                 "sample_profiles": model.sample_profiles, "domain_samplers": model.domains})
    return G.Gate("G-FID", "PASS" if fid_ok else "FAIL", fd), nv


def g_triv(sh: Shared, build: str, ns: str, model: GR.Model, work: str) -> G.Gate:
    cfg = sh.cfg
    os.makedirs(work, exist_ok=True)
    emulated = bool(model.groups)
    # P2 first (the forms that close most statements), then P1; stop at the first file with a closing form
    files = [("Battery_P2", [(v, t) for v, ts in E.BATTERY_P2 for t in ts]),
             ("Battery_P1", [(v, t) for v in E.BATTERY_VARIANTS for t in E.BATTERY_TACTICS])]
    results, runs = {}, []

    def emit(forms):
        return GE.emit_battery_forms(ns, model.r.n_constraints, forms, cfg.battery_heartbeats, emulated)

    for fname, forms in files:
        if any(c["closed"] for c in results.values()):
            runs.append({"variant": fname.replace("Battery_", ""), "skipped": "an earlier battery file closed the statement"})
            continue
        text = emit(forms)
        path = os.path.join(work, f"{fname}.lean")
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        r = L.run_lean(sh.env, GE.LEAN_OPTIONS + ["--json", path], work, cfg.battery_file_wall,
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
                rs = L.run_lean(sh.env, GE.LEAN_OPTIONS + ["--json", spath], work, cfg.battery_single_wall,
                                extra_lean_path=[build], rss_limit_mb=16384)
                c = G.classify_battery(single, L.parse_messages(rs.out), rs.timeout or rs.memkill)[name]
                c["rerun_single"] = {"secs": rs.secs, "timeout": rs.timeout, "memkill": rs.memkill}
            results[name] = c
    budget = {"maxHeartbeats": cfg.battery_heartbeats, "file_wall_s": cfg.battery_file_wall,
              "single_wall_s": cfg.battery_single_wall}
    return G._triv_gate(results, runs, budget, "P2 (V3-V4, 7 forms), then P1 (V0-V2, 33 forms) unless P2 closed it")


def set_status(rec: dict, gates: dict, has_outputs: bool) -> None:
    det_gate = gates["DET-SEARCH"]
    if det_gate["truth"] == "false-counterexample-found":
        rec["status"] = "DET-FALSE-CANDIDATE"
        rec["status_reason"] = (f"{det_gate['method']} found two constraint-satisfying assignments with equal inputs and "
                                "different outputs (confirmed by the Python evaluator and the Lean model); private finding")
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
    if has_outputs and det_gate["status"] == "SKIPPED" and not failed:
        failed.append("DET-SEARCH")
    if failed:
        rec["status"] = "GATE-FAIL"
        reasons = []
        for g in failed:
            gd = gates.get(g, {})
            if g == "NO-OUTPUTS":
                reasons.append("the gadget has no circuit-variable results, so DET is vacuous")
            elif g == "G-TRIV" and gd.get("closed_by"):
                reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
                rec["statement"]["truth"] = "closed-by-automation"
            else:
                reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:600]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def build_package(sh: Shared, rb: RepoBuild, rec: dict, row: dict, plan: I.Plan, chosen: Cand, model: GR.Model,
                  work: str) -> dict:
    cfg = sh.cfg
    dir_name = rec["package_id"].split("/", 1)[1]
    collection = rec["package_id"].split("/", 1)[0]
    ns = P.lean_namespace(collection, dir_name)
    res = chosen.result
    com = model.commitment
    commitment_txt = ("no commitment" if not com.get("commits") else
                      f"one commitment; challenge degree {com['degree']}, held at {com['points']} fixed challenges")
    meta = {"repo_id": rb.repo_id, "instantiation": chosen.wrapper.call, "generator": f"{sh.generator['name']} "
            f"v{sh.generator['version']}", "repo_url": rec["ids"]["repo_url"], "commit": rec["ids"]["commit"],
            "path": rec["ids"]["path"], "template": rec["ids"]["template"], "wrapper_id": chosen.wid,
            "rule": rec["instantiation"]["rule"], "call": chosen.wrapper.call, "gnark_version": rb.gnark_version,
            "go_version": cfg.go[rb.go_name]["version"], "curve": res["curve"], "commitment": commitment_txt,
            "r1cs_sha256": rec["circuit"]["r1cs_sha256"]}
    stage = os.path.join(work, "pkg")
    shutil.rmtree(stage, ignore_errors=True)
    model_rel = GE.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    groups = model.group_wires()
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(GE.emit_model(ns, meta, model.r, model.inputs, model.native_outputs, groups, model.wire_names))
    emulated = bool(groups)
    statement_text = GE.emit_statement(ns, meta, emulated)
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{GE.STATEMENT_THEOREM}"
    rec["statement"] = {"file": "Statement.lean", "theorem": GE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": GE.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": statement_assumptions(res["curve"], com, model.hints, emulated),
                        "truth": "unknown"}
    if emulated:
        rec["statement"]["emulated_outputs"] = [{"wires": limbs, "bits": bits, "modulus": str(mod), "field": g["field"],
                                                 "result": g["path"]} for (limbs, bits, mod), g in zip(groups, model.groups)]
    rec["evidence"]["hints"] = model.hints
    rec["evidence"]["commitment"] = com
    rec["evidence"]["witness_sampling"] = {"samples": len(res.get("samples") or []), "distinct_witnesses": len(model.witnesses),
                                           "solver_errors": dict(list(model.solver_errors.items())[:6]),
                                           "test_engine": model.test_engine, "profiles": model.sample_profiles}
    rec["evidence"]["wrapper_inputs"] = chosen.wrapper.inputs
    gates: dict = {}
    real = model.witnesses
    real_w = real[:REAL_WANTED]
    muts = make_mutants(model, real_w, MUTANTS, rec["package_id"])
    build = os.path.join(work, "build")
    shutil.rmtree(build, ignore_errors=True)
    with sh.lean_slots:
        elab = G.g_elab(sh.env, stage, build, ns, os.path.join(work, "elab"))
    gates["G-ELAB"] = elab.to_json()
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build, model_rel[:-len(".lean")] + ".olean"))
    input_free = not model.inputs
    if model_ok:
        with sh.lean_slots:
            fid, nonvac = g_fid_nonvac(sh, build, ns, model, real_w, muts, input_free, os.path.join(work, "fid"))
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()
    differ = GR.differ(model)
    cheap_ok = all(gates[g]["status"] == "PASS" for g in ("G-ELAB", "G-NONVAC", "G-FID"))
    ce, slog = None, {"bases": len(real)}
    if not model.outputs:
        det_gate = {"status": "SKIPPED", "truth": "unknown", "reason": "no outputs: the statement is vacuous"}
    elif not cheap_ok:
        det_gate = {"status": "SKIPPED", "truth": "unknown",
                    "reason": "not run: the package already fails G-ELAB, G-NONVAC or G-FID"}
    else:
        ce, slog = DS.search(model.r, real, model.inputs, model.outputs, rec["package_id"], cfg.det_search_budget_s,
                             differ=differ)
        det_gate = {"status": "PASS", "truth": "unknown", "log": slog,
                    "note": "PASS means no counterexample was found by the cheap searches; DET truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        write_witness(os.path.join(cdir, "w1.txt"), ce.base)
        write_witness(os.path.join(cdir, "w2.txt"), ce.other)
        with sh.lean_slots:
            verdicts, emv, _ = lean_eval(sh, build, ns, model, [("w1", os.path.join(cdir, "w1.txt")),
                                                                ("w2", os.path.join(cdir, "w2.txt"))],
                                         ["w1", "w2"] if groups else [], cdir)
        lean_ok = verdicts.get("w1") == "ACCEPT" and verdicts.get("w2") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if lean_ok else "unknown",
                    "method": ce.method, "changed_outputs": ce.changed_outputs[:20],
                    "lean_confirms_both_witnesses": lean_ok, "python_confirms": True, "log": slog,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"]}
        if groups:
            det_gate["emulated_values_lean"] = {"w1": emv.get("w1"), "w2": emv.get("w2")}
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
            gates["G-TRIV"] = g_triv(sh, build, ns, model, os.path.join(work, "triv")).to_json()
    rec["gates"] = gates
    set_status(rec, gates, bool(model.outputs))
    ref = elab.detail["reference_type_sha256"] if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": GE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(GE.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": P.sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": GE.model_module(ns),
                                 "sha256": P.sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    write_package(sh, rec, stage, work, chosen, real_w)
    shutil.rmtree(work, ignore_errors=True)
    return rec


def write_package(sh: Shared, rec: dict, stage: str, work: str, chosen: Cand, real: list[list[int]]) -> None:
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
    with open(os.path.join(ev, "wrapper.go"), "w", encoding="utf-8") as f:
        f.write(f"// wrapper compiled against {rec['ids']['repo']} at {rec['ids']['commit']}\n" + chosen.wrapper.source)
    res = dict(chosen.result)
    res.pop("samples", None)
    with open(os.path.join(ev, "harness_result.json"), "w", encoding="utf-8") as f:
        json.dump(res, f, sort_keys=True, separators=(",", ":"))
    os.makedirs(os.path.join(ev, "witnesses"))
    for k, w in enumerate(real[:4]):
        write_witness(os.path.join(ev, "witnesses", f"real_{k:03d}.txt"), w)
    src = os.path.join(work, "elab", "reference.type.txt")
    if os.path.exists(src):
        shutil.copyfile(src, os.path.join(ev, "reference.type.txt"))
    if "counterexample_dir" in rec["evidence"]:
        cdir = rec["evidence"].pop("counterexample_dir")
        os.makedirs(os.path.join(ev, "counterexample"))
        for fn in ("w1.txt", "w2.txt"):
            if os.path.exists(os.path.join(cdir, fn)):
                shutil.copyfile(os.path.join(cdir, fn), os.path.join(ev, "counterexample", fn))
    triv = os.path.join(work, "triv")
    if os.path.isdir(triv):
        os.makedirs(os.path.join(ev, "triv"))
        for fn in sorted(os.listdir(triv)):
            if fn.endswith(".lean"):
                shutil.copyfile(os.path.join(triv, fn), os.path.join(ev, "triv", fn))
    P.write_json(os.path.join(dest, "problem.json"), rec)


def rewrite_problem(cfg: WaveConfig, rec: dict) -> None:
    dest = os.path.join(cfg.out, *rec["package_id"].split("/", 1), "problem.json")
    if os.path.exists(dest):
        P.write_json(dest, rec)


# ------------------------------------------------------------------------------------------ wave

def size_repo(sh: Shared, rb: RepoBuild, rows: list[dict], why: str) -> None:
    """Production sizes of circuit types when the wrapper tool does not build against the repository's
    gnark: a program inside a copy of the repository module (its own go.mod) compiles each circuit with
    gnark's R1CS builder (``frontend.Compile`` with ``r1cs.NewBuilder``, BN254) and prints its constraint
    count.  Above the size policy the row is TOO-LARGE (production count); otherwise COMPILE-FAIL."""
    cfg = sh.cfg
    env = go_env(cfg, rb.go_name)
    mcopy = os.path.join(rb.build_dir, "modcopy")
    if not os.path.isdir(mcopy):
        shutil.copytree(os.path.join(rb.root, rb.module_dir), mcopy, ignore=shutil.ignore_patterns(".git"))
    cases, items = [], {}
    g = I.GoFile()
    g.imports.pop(I.HARNESS)
    g.used.discard(I.HARNESS)
    for r in rows:
        tgt, plan, cands, status, reason = sh.plans[r["item_id"]]
        if plan is None or plan.kind != "circuit":
            if not status:
                sh.plans[r["item_id"]] = (tgt, plan, [], "COMPILE-FAIL",
                                          "the wrapper tool does not build against the repository's gnark: " + why[:400])
            continue
        d = plan.decl
        rt = {"k": "named", "pkg": d["pkg"], "name": d["name"] if d["kind"] == "type" else d["recv"]["name"]}
        try:
            shape = I.shape_expr(rb.catalog, g, rt, I.PROBED_LENGTHS[0])
            expr = "&" + (shape or g.typ(rt) + "{}")
        except I.NoInstantiation as ex:
            sh.plans[r["item_id"]] = (tgt, plan, [], "COMPILE-FAIL", f"circuit type cannot be written: {ex}")
            continue
        sid = I.wrapper_id(r["item_id"], 0)
        items[sid] = r
        cases.append((sid, expr))
    if not cases:
        return
    tool = os.path.join(mcopy, "boolesize")
    os.makedirs(tool, exist_ok=True)
    first_err = ""
    for old_api in (False, True):
        g.alias("os")
        g.alias("fmt")
        r1cs_alias = g.alias("github.com/consensys/gnark/frontend/cs/r1cs")
        ecc = g.alias(I.ECC)
        field = f"{ecc}.BN254" if old_api else f"{ecc}.BN254.ScalarField()"
        body = ["func main() {", "\tdefer func() {\n\t\tif r := recover(); r != nil {\n\t\t\tfmt.Println(\"ERR panic:\", r)\n\t\t}\n\t}()",
                "\tvar c frontend.Circuit", "\tswitch os.Args[1] {"]
        for sid, expr in cases:
            body += [f'\tcase "{sid}":', f"\t\tc = {expr}"]
        body += ["\t}", f"\tcs, err := frontend.Compile({field}, {r1cs_alias}.NewBuilder, c)",
                 '\tif err != nil {\n\t\tfmt.Println("ERR", err)\n\t\treturn\n\t}',
                 '\tfmt.Println("N", cs.GetNbConstraints())', "}"]
        src = g.header().replace("package wrappers", "package main") + "\n" + "\n".join(body) + "\n"
        with open(os.path.join(tool, "main.go"), "w", encoding="utf-8") as f:
            f.write(src)
        b = run_cmd(["go", "build", "-o", os.path.join(rb.build_dir, "gnarksize"), "./boolesize"], mcopy, env)
        if b.returncode == 0:
            break
        first_err = first_err or _first_go_error(b.stderr)
        if "ScalarField" not in b.stderr:          # only the Compile signature differs between gnark versions
            break
    if b.returncode != 0:
        for sid, r in items.items():
            sh.plans[r["item_id"]] = (sh.plans[r["item_id"]][0], None, [], "COMPILE-FAIL",
                                      "neither the wrapper tool nor a sizing program builds against the repository's "
                                      "gnark: " + scrub_text(sh, first_err)[:300])
        return
    for sid, r in items.items():
        tgt = sh.plans[r["item_id"]][0]
        t0 = time.time()
        try:
            out = run_cmd([os.path.join(rb.build_dir, "gnarksize"), sid], mcopy, env, timeout=cfg.run_timeout)
            line = (out.stdout.strip().splitlines() or [""])[-1]
            err = "" if line.startswith("N ") else (line or next((x for x in out.stderr.splitlines()
                                                                  if x.startswith(("panic", "fatal"))), "")
                                                     or _first_go_error(out.stderr))
        except subprocess.TimeoutExpired:
            line, err = "", f"production compile exceeded {cfg.run_timeout:.0f} s"
        if line.startswith("N "):
            n = int(line.split()[1])
            sh.sizing[r["item_id"]] = {"n": n, "secs": round(time.time() - t0, 2)}
            if n > MAX_CONSTRAINTS:
                sh.plans[r["item_id"]] = (tgt, None, [], "TOO-LARGE",
                                          f"{n} constraints of gnark's production R1CS builder (slice fields of length "
                                          f"{I.PROBED_LENGTHS[0]}) > {MAX_CONSTRAINTS}; the model is not built: the "
                                          f"wrapper tool does not build against the repository's gnark ({why[:200]})")
            else:
                sh.plans[r["item_id"]] = (tgt, None, [], "COMPILE-FAIL",
                                          f"{n} production constraints (within the size policy) but the wrapper tool "
                                          f"does not build against the repository's gnark ({why[:200]})")
        else:
            sh.plans[r["item_id"]] = (tgt, None, [], "COMPILE-FAIL", "production compile failed: " +
                                      scrub_text(sh, err)[:400])


DISK_FLOOR = 15_000_000_000           # stop when free space on the data volume drops below this (bytes)


def trim_caches(sh: Shared, rb: RepoBuild | None) -> None:
    """After a repository's binaries are built: drop the Go build and module caches and the module copy
    (the binaries are self-contained), so that the scratch directory stays within its budget; stop when the
    data volume is short of space."""
    cfg = sh.cfg
    env = go_env(cfg, rb.go_name if rb else next(iter(cfg.go)))
    for args in (["go", "clean", "-cache"], ["go", "clean", "-modcache"]):
        run_cmd(args, cfg.scratch, env)
    if rb is not None:
        mcopy = os.path.join(rb.build_dir, "modcopy")
        if os.path.isdir(mcopy) and mcopy.startswith(cfg.build + os.sep):
            shutil.rmtree(mcopy)
    st = os.statvfs(cfg.scratch)
    if st.f_bavail * st.f_frsize < DISK_FLOOR:
        raise RuntimeError("free space on the data volume below 15 GB; stopping")


def setup_shared(cfg: WaveConfig, rows: list[dict]) -> Shared:
    env = L.load_env(cfg.lean_env)
    sh = Shared(cfg, env, P.sha256_file(cfg.ledger), generator_info())
    sh.lean_slots = threading.Semaphore(max(1, cfg.lean_jobs))
    sh.plans = {}
    by_repo: dict[str, list] = {}
    for r in rows:
        by_repo.setdefault(repo_id_of(r["repo"]), []).append(r)
    for rid, rrows in sorted(by_repo.items()):
        repo = cfg.repos[rid]
        head = run_cmd(["git", "rev-parse", "HEAD"], repo["dir"], dict(os.environ)).stdout.strip()
        if head != repo["commit"]:
            raise ValueError(f"{rid} is at {head}, the wave pins {repo['commit']}")
        targets = [{"path": r["path"], "symbol": r["symbol"], "line": int((r.get("census") or [{}])[0].get("line") or 0)}
                   for r in rrows]
        log(sh, f"{rid}: catalog ({len(rrows)} rows)")
        try:
            rb = prepare_catalog(cfg, rid, targets)
        except Exception as ex:                   # no catalog: every row records it
            msg = scrub_text(sh, f"{type(ex).__name__}: {ex}")
            log(sh, f"{rid}: catalog failed: {msg[:300]}")
            sh.repo_failures[rid] = msg
            for r in rrows:
                sh.plans[r["item_id"]] = (None, None, [], "COMPILE-FAIL",
                                          "the repository's packages do not load at the pin: " + msg[:400])
            continue
        sh.repos[rid] = rb
        wrappers = {}
        for r in rrows:
            plan = plan_item(sh, rb, r)
            sh.plans[r["item_id"]] = plan
            for c in plan[2]:
                wrappers[c.wid] = c.wrapper.source
        log(sh, f"{rid}: {len(wrappers)} wrappers")
        try:
            prepare_harness(cfg, rb)
            build_wrappers(cfg, rb, wrappers)
            log(sh, f"{rid}: built ({len(rb.wrapper_errors)} wrappers removed by the Go compiler)")
        except Exception as ex:                   # the wrapper tool does not build against this gnark: sizes only
            msg = scrub_text(sh, f"{ex}")
            log(sh, f"{rid}: wrapper tool does not build ({msg[:200]}); production sizing")
            rb.sizing = True
            rb.setup_notes.append("wrapper tool does not build against the repository's gnark: " + msg[:400])
            size_repo(sh, rb, rrows, msg)
        trim_caches(sh, rb)
    return sh


def run_wave(cfg: WaveConfig, log_every: int = 25) -> list[dict]:
    os.makedirs(cfg.out, exist_ok=True)
    os.makedirs(cfg.work, exist_ok=True)
    rows = select_rows(cfg.ledger)
    if cfg.only:
        rows = [r for r in rows if r["item_id"] in set(cfg.only)]
    sh = setup_shared(cfg, rows)
    keys = {}
    groups: dict[str, list] = {}
    for r in rows:
        tgt = sh.plans[r["item_id"]][0]
        rb = sh.repos.get(repo_id_of(r["repo"]))
        k = content_key(rb, tgt) if rb and tgt and tgt.get("found") else "missing:" + r["item_id"]
        keys[r["item_id"]] = k
        groups.setdefault(k, []).append(r)
    canon, dedup = [], []
    for k, members in groups.items():
        members.sort(key=lambda m: m["item_id"])
        canon.append(members[0])
        for m in members[1:]:
            dedup.append({"item_id": m["item_id"], "canonical_item_id": members[0]["item_id"], "content_sha256": k,
                          "relation": "duplicate-of-g1"})
    log(sh, f"selection {len(rows)} rows, {len(canon)} distinct by content, {len(dedup)} duplicates")
    records: list[dict] = []
    done = 0
    with cf.ThreadPoolExecutor(max_workers=min(cfg.jobs, MAX_JOBS)) as pool:
        futs = {pool.submit(process, sh, row): row for row in canon}
        for fut in cf.as_completed(futs):
            rec = fut.result()
            rec["evidence"]["content_sha256"] = keys[futs[fut]["item_id"]]
            if rec["status"] in P.PACKAGED_STATUSES:
                rewrite_problem(cfg, rec)
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
    write_toolchains(cfg, sh)
    return records


def write_toolchains(cfg: WaveConfig, sh: Shared) -> None:
    out = []
    for rid, rb in sorted(sh.repos.items()):
        g = cfg.go[rb.go_name]
        out.append({"repo": rid, "module": rb.module_path, "module_dir": rb.module_dir, "go_required": rb.go_required,
                    "go_toolchain_directive": rb.go_toolchain_directive, "go_used": g["version"], "go_source": g["source"],
                    "go_sha256": g.get("sha256", ""), "gnark": rb.gnark_version, "gosum_sha256": rb.gosum_sha256,
                    "gnarkx_sha256": rb.binary_sha256, "version_deviations": rb.version_deviations,
                    "wrappers_removed_by_compiler": len(rb.wrapper_errors), "notes": rb.setup_notes})
    with open(os.path.join(cfg.out, "TOOLCHAINS.json"), "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1, sort_keys=True)


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
            pkg = os.path.join(packages_dir, *r["package_id"].split("/", 1))
            if not os.path.isdir(pkg):
                problems.append(f"{r['package_id']}: package directory missing")
                continue
            with open(os.path.join(pkg, "problem.json"), encoding="utf-8") as f:
                if json.load(f) != r:
                    problems.append(f"{r['package_id']}: problem.json differs from the index record")
        for e in P.validate_problem(r, pkg, schema):
            problems.append(f"{r['package_id']}: {e}")
    return problems


def run_rerun(cfg: WaveConfig, index_path: str, item_ids: list[str]) -> list[dict]:
    """Regenerate the named records with the current generator and merge them (package directories of
    replaced records removed; every replacement logged in RERUN-MERGE.jsonl)."""
    with open(index_path, encoding="utf-8") as f:
        wave = [json.loads(x) for x in f if x.strip()]
    by_id = {r["ids"]["ledger_item_id"]: r for r in wave}
    rows = [r for r in select_rows(cfg.ledger) if r["item_id"] in set(item_ids) and r["item_id"] in by_id]
    sh = setup_shared(cfg, rows)
    merged = []
    with cf.ThreadPoolExecutor(max_workers=min(cfg.jobs, MAX_JOBS)) as pool:
        futs = {pool.submit(process, sh, r): r["item_id"] for r in rows}
        for fut in cf.as_completed(futs):
            i = futs[fut]
            new, old = fut.result(), by_id[i]
            new["evidence"]["content_sha256"] = old["evidence"].get("content_sha256")
            if new["status"] in P.PACKAGED_STATUSES:
                rewrite_problem(cfg, new)
            if old["package_id"] != new["package_id"] and old["status"] in P.PACKAGED_STATUSES:
                stale = os.path.join(cfg.out, *old["package_id"].split("/", 1))
                if stale.startswith(cfg.out + os.sep) and os.path.isdir(stale):
                    shutil.rmtree(stale)
            by_id[i] = new
            entry = {"item_id": i, "old_status": old["status"], "old_reason": old["status_reason"][:300],
                     "old_generator": old["generator"]["sources_sha256"], "new_status": new["status"],
                     "new_reason": new["status_reason"][:300], "new_generator": new["generator"]["sources_sha256"],
                     "package_id": new["package_id"]}
            merged.append(entry)
            append_jsonl(os.path.join(cfg.out, "RERUN-MERGE.jsonl"), entry)
            log(sh, f"rerun {len(merged)}/{len(rows)}: {old['status']} -> {new['status']}")
    dedup = []
    dpath = os.path.join(cfg.out, "DEDUP.jsonl")
    if os.path.exists(dpath):
        with open(dpath, encoding="utf-8") as f:
            dedup = [json.loads(x) for x in f if x.strip()]
        for d in dedup:
            c = by_id.get(d["canonical_item_id"])
            d["canonical_package_id"], d["canonical_status"] = (c["package_id"], c["status"]) if c else (None, None)
    write_outputs(cfg.out, list(by_id.values()), dedup)
    return merged


# ------------------------------------------------------------------------------------------ main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("select")
    s.add_argument("--ledger", required=True)
    s = sub.add_parser("wave")
    s.add_argument("--config", required=True)
    s.add_argument("--jobs", type=int)
    s.add_argument("--only", nargs="*")
    s = sub.add_parser("rerun")
    s.add_argument("--config", required=True)
    s.add_argument("--index", required=True)
    s.add_argument("--items", required=True)
    s.add_argument("--jobs", type=int)
    s = sub.add_parser("decompose")
    s.add_argument("--config", required=True)
    s.add_argument("--index", required=True)
    s.add_argument("--jobs", type=int)
    s = sub.add_parser("validate")
    s.add_argument("--index", required=True)
    s.add_argument("--packages", required=True)
    a = ap.parse_args(argv)
    if a.cmd == "select":
        rows = select_rows(a.ledger)
        by_repo: dict[str, int] = {}
        for r in rows:
            by_repo[repo_id_of(r["repo"])] = by_repo.get(repo_id_of(r["repo"]), 0) + 1
        print(json.dumps({"rows": len(rows), "by_repo": by_repo, "filter": SELECTION_FILTER}, indent=1, sort_keys=True))
        return 0
    if a.cmd == "validate":
        problems = validate_all(a.index, a.packages)
        for p in problems[:50]:
            print(p)
        print(f"{len(problems)} problem(s)")
        return 1 if problems else 0
    cfg = WaveConfig.load(a.config)
    if getattr(a, "jobs", None):
        cfg.jobs = min(a.jobs, MAX_JOBS)
    if a.cmd == "wave":
        if a.only:
            cfg.only = a.only
        recs = run_wave(cfg)
        counts: dict[str, int] = {}
        for r in recs:
            counts[r["status"]] = counts.get(r["status"], 0) + 1
        print(json.dumps(counts, indent=1, sort_keys=True))
        return 0
    if a.cmd == "rerun":
        with open(a.items, encoding="utf-8") as f:
            items = [x.strip() for x in f if x.strip()]
        res = run_rerun(cfg, a.index, items)
        counts = {}
        for e in res:
            k = f"{e['old_status']} -> {e['new_status']}"
            counts[k] = counts.get(k, 0) + 1
        print(json.dumps(counts, indent=1, sort_keys=True))
        return 0
    if a.cmd == "decompose":
        from zk_registry import gnark_decompose as GD
        recs, edges = GD.run_decompose(cfg, a.index)
        counts = {}
        for r in recs:
            counts[r["status"]] = counts.get(r["status"], 0) + 1
        print(json.dumps({"records": counts, "edges": len(edges)}, indent=1, sort_keys=True))
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
