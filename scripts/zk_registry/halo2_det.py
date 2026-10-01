#!/usr/bin/env python3
"""halo2 DET problem generator: wave driver.

Subcommands::

    select    --ledger LEDGER-v1.jsonl                          the wave-H1 population filter (counts, plan)
    wave      --config wave.json [--jobs N] [--only ITEM ...]   plan, build, export, model, gate and package
    validate  --index INDEX.jsonl --packages DIR                re-validate every record and package file
    rerun     --config wave.json --items FILE                   regenerate named records, merge into the wave output
    decompose --index INDEX.jsonl --out DIR                     TOO-LARGE layouts -> region edges to wave records

Pipeline: every filtered ledger row is located in its pinned checkout, deduplicated by content (the symbol and the
comment-free text of its file) and planned (:mod:`halo2_targets`): a wrapper of a built adapter, or a static
terminal status with its reason.  Per adapter the repository's ``halo2_proofs`` gets the exporter and the wrapper
modules are injected (:mod:`halo2_harness`); the crate's test binary is built once with the pinned toolchain.  Per
row the wrapper test runs ``MockProver::run`` on sampled inputs and exports the layout (``boole-halo2-ir/v1``);
:mod:`halo2_ir` flattens it into the model over the concrete layout; the Lean model and the DET statement
(:mod:`halo2_lean_emit`) are emitted and gated: G-ELAB, G-NONVAC (a real MockProver witness satisfies the model),
G-FID (every real witness and single-cell mutant evaluated by Lean, by the Python evaluator and by
``MockProver::verify``), a sound counterexample search, and G-TRIV (battery P2 then P1).  Every row ends in one
terminal status: OPEN, GATE-FAIL, DET-FALSE-CANDIDATE, TOO-LARGE, NO-INSTANTIATION, COMPILE-FAIL or NOT-APPLICABLE
(with a reason).  The driver never writes a proof of a DET statement.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import glob
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

from zk_registry import air_ir as AIR                 # noqa: E402
from zk_registry import air_lean as AL                # noqa: E402
from zk_registry import air_search as AS              # noqa: E402
from zk_registry import check as C                    # noqa: E402
from zk_registry import gates as G                    # noqa: E402
from zk_registry import halo2_harness as HH           # noqa: E402
from zk_registry import halo2_ir as H                 # noqa: E402
from zk_registry import halo2_lean_emit as HE         # noqa: E402
from zk_registry import halo2_targets as T            # noqa: E402
from zk_registry import lean_emit as E                # noqa: E402
from zk_registry import lean_runner as L              # noqa: E402
from zk_registry import package as P                  # noqa: E402

GENERATOR_NAME = "boole-zk-registry-halo2-det"
GENERATOR_VERSION = "1.0"
_HERE = os.path.dirname(os.path.abspath(__file__))
HALO2_SOURCES = ["halo2_det.py", "halo2_ir.py", "halo2_lean_emit.py", "halo2_harness.py", "halo2_targets.py",
                 "halo2_harness/export/zcash_v03.rs"]
SHARED_SOURCES = ["__init__.py", "package.py", "jsonschema_lite.py", "lean_runner.py", "lean_emit.py", "gates.py",
                  "check.py", "air_ir.py", "air_lean.py", "air_search.py", "det_search.py", "lean/ZkReplay.lean",
                  "schema/problem.schema.json"]
MAX_CONSTRAINTS = P.MAX_CONSTRAINTS          # model items (gate instances + copies + lookups)
REAL_WANTED = 16
MUTANTS = 16
MAX_JOBS = 12
DISK_FLOOR = 15_000_000_000
EXCLUDED_UNITS_SUFFIX = ("-constraint-site", "-interaction-site")
EXCLUDED_UNITS = ("C2-operation", "U1-instruction")
EXCLUDED_FLAG_SUBSTRINGS = ("test", "not-counted", "deprecated", "NOT-ITEMIZED", "ZERO-ITEMS", "supplementary",
                            "exported-api-false", "intrinsic", "generated", "duplicate-of", "copy-of", "vendored",
                            "mapped-to", "archived", "excluded", "secondary", "program-as-circuit")
SELECTION_FILTER = {"framework": "halo2", "excluded_unit_suffixes": list(EXCLUDED_UNITS_SUFFIX),
                    "excluded_units": list(EXCLUDED_UNITS), "excluded_flag_substrings": list(EXCLUDED_FLAG_SUBSTRINGS)}


def selected(row: dict, framework: str = "halo2") -> bool:
    if row.get("framework") != framework:
        return False
    unit = row.get("unit", "")
    if unit.endswith(EXCLUDED_UNITS_SUFFIX) or unit in EXCLUDED_UNITS:
        return False
    return not any(s in f for f in row.get("flags", []) for s in EXCLUDED_FLAG_SUBSTRINGS)


def select_rows(ledger_path: str, framework: str = "halo2") -> list[dict]:
    with open(ledger_path, encoding="utf-8") as f:
        return [r for r in (json.loads(line) for line in f if line.strip()) if selected(r, framework)]


def repo_id_of(row: dict) -> str:
    m = re.match(r"https://github\.com/([^/]+)/([^/]+?)(\.git)?/?$", row["repo"])
    return f"{m.group(1)}/{m.group(2)}" if m else row["repo"]


def repo_key(row: dict) -> str:
    """Checkout key: repository and pin (zcash/halo2 has rows at two pins)."""
    return f"{repo_id_of(row)}@{row['commit'][:12]}"


# ------------------------------------------------------------------------------------------ config

@dataclass
class WaveConfig:
    collection: str
    ledger: str
    repos: dict                  # repo key -> {"dir", "url", "commit"}
    scratch: str
    lean_env: str
    out: str
    work: str
    target_dir: str
    adapters: list = field(default_factory=list)
    jobs: int = 12
    lean_jobs: int = 6
    only: list = field(default_factory=list)
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    det_search_budget_s: float = 20.0
    samples: int = 16
    run_timeout: float = 1800

    @classmethod
    def load(cls, path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            return cls(**json.load(f))


def _source_files() -> list[str]:
    files = list(HALO2_SOURCES)
    for root, _, names in os.walk(HH.WRAPPER_DIR):
        for n in sorted(names):
            if n.endswith((".rs", ".in")):
                files.append(os.path.relpath(os.path.join(root, n), _HERE))
    return sorted(set(files))


def generator_files() -> list[str]:
    """Files hashed by this generator only (the circom generator hashes the shared ones too)."""
    return [f for f in _source_files() if f.endswith((".py", ".js", ".lean", ".json"))]


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in SHARED_SOURCES + _source_files():
        h.update(rel.encode() + b"\0" + P.sha256_file(os.path.join(_HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


HALO2_DET_SPEC_CLAUSES = [
    "Output determinism: for the constraint system of the generated wrapper circuit of the halo2 instruction, chip or "
    "circuit, instantiated over its concrete layout (MockProver::run at the recorded k: every gate polynomial at "
    "every row with fixed cells and compressed selectors substituted, every copy constraint of the permutation "
    "argument, every lookup over the usable rows), any two assignments of the advice and instance cells that satisfy "
    "every constraint and agree on every input cell agree on every output cell.",
    "Inputs are the instance cells and the cells the wrapper names as witnessed from the instruction's value "
    "parameters; outputs are the instruction's result cells, copied into the wrapper's output region.",
]
HALO2_DET_SPEC = {
    "tier": "T1",
    "source": "mathematical definition of output determinism (functional dependence of outputs on inputs)",
    "clauses": HALO2_DET_SPEC_CLAUSES,
    "sha256": hashlib.sha256("\n".join(HALO2_DET_SPEC_CLAUSES).encode("utf-8")).hexdigest(),
}


def statement_assumptions(model: H.Model) -> list[str]:
    out = [f"[Fact (Nat.Prime p)]: primality of the {model.field_name} modulus, supplied as an instance hypothesis so "
           "that field lemmas apply; p is prime, so the hypothesis does not weaken the statement."]
    kinds = sorted({model.tables[ti].kind for _, ti, _ in model.lookups})
    if kinds:
        out.append("Lookups are modelled exactly over the layout: fixed tables are their constant contents (a "
                   "one-column table equal to {0, .., m-1} as `x.val < m`)" +
                   ("; advice-defined tables as membership in the tuples of model terms over the usable rows"
                    if "advice" in kinds else "") + "; no lookup is assumed.")
    out.append("Cells the constraints do not reference (unassigned or witnessed but unconstrained cells) are free in "
               "the model, as for a malicious prover; fixed cells are the circuit's constants (unassigned fixed cells "
               "are 0, as keygen leaves them).")
    return out


# ------------------------------------------------------------------------------------------ shared state

@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    ledger_sha: str
    generator: dict
    lock: threading.Lock = field(default_factory=threading.Lock)
    lean_slots: threading.Semaphore | None = None
    builds: dict = field(default_factory=dict)          # adapter -> {"exe", "secs", "lock_diff", "instrument", ...}


def log(sh: Shared, msg: str) -> None:
    with sh.lock:
        print(time.strftime("%H:%M:%S"), msg, flush=True)


def cargo_env(cfg: WaveConfig) -> dict:
    s = cfg.scratch
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "HOME": os.environ.get("HOME", s),
           "CARGO_HOME": os.path.join(s, "cache", "cargo"), "CARGO_TARGET_DIR": cfg.target_dir,
           "RUSTUP_HOME": os.path.expanduser("~/.rustup"), "RUSTUP_AUTO_INSTALL": "0",
           "TMPDIR": os.path.join(s, "tmp"), "CARGO_TERM_COLOR": "never", "GIT_TERMINAL_PROMPT": "0",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1", "XDG_CACHE_HOME": os.path.join(s, "cache", "xdg")}
    return env


def free_bytes(path: str) -> int:
    st = os.statvfs(path)
    return st.f_bavail * st.f_frsize


# ------------------------------------------------------------------------------------------ records

def dir_name_for(row: dict) -> str:
    stem = re.sub(r"\.(rs|sol)$", "", row["path"]).replace("/", ".")
    name = re.sub(r"[^A-Za-z0-9._-]", "_", f"{stem}.{row['symbol']}")
    if len(name) > 150:
        name = name[:120] + ".h" + hashlib.sha256(name.encode()).hexdigest()[:12]
    return name


IRONWOOD = ("zcash/ironwood at 86e3c702 (2026-09-08): Lean 4 hand port of halo2_gadgets 0.5.0 chips with soundness and "
            "completeness proofs at Orchard parameters (census P1-D4); not a DET statement over the MockProver layout")


def coverage_of(row: dict) -> dict:
    """Public machine-checked coverage of the code at the pin (from the ledger's census record)."""
    cov = row.get("coverage", "none")
    census = (row.get("census") or [{}])[0]
    out = {"class": "partial" if cov in ("partial", "conditional") else ("full" if cov == "full" else "none"),
           "ledger_coverage": cov, "census": census.get("coverage_raw", ""),
           "census_source": census.get("coverage_source", "")}
    if row["project"] == "halo2-gadgets" and cov == "partial":
        out.update(same_pin=True, source=IRONWOOD)
    elif cov == "conditional":
        out["note"] = "conditional coverage in the census (unpinned or partial hand models); kept, not dropped"
    return out


def base_record(sh: Shared, row: dict, src_sha: str, located: bool) -> dict:
    census = (row.get("census") or [{}])[0]
    rid = repo_id_of(row)
    return {
        "schema_version": P.SCHEMA_VERSION,
        "package_id": f"{rid.replace('/', '__')}/{dir_name_for(row)}",
        "property": dict(P.DET_PROPERTY),
        "status": "NO-INSTANTIATION", "status_reason": "",
        "ids": {"ledger_item_id": row["item_id"],
                "ledger": {"file": "ledger/LEDGER-v1.jsonl", "sha256": sh.ledger_sha, "row_found": True},
                "repo": rid, "repo_url": row["repo"], "release": str(census.get("pin") or ""),
                "commit": row["commit"], "path": row["path"], "template": row["symbol"],
                "template_line": max(1, int(census.get("line") or 1)), "source_sha256": src_sha},
        "instantiation": {"rule": "none", "args": [], "call": "", "provenance": [], "selection": "", "candidates": []},
        "spec": dict(HALO2_DET_SPEC),
        "env": {"halo2": "", "rust": "", "lean": sh.env.lean_version, "mathlib": sh.env.packages.get("mathlib", ""),
                "lake_manifest_sha256": sh.env.manifest_sha256, "packages": dict(sorted(sh.env.packages.items())),
                "python": platform.python_version()},
        "generator": dict(sh.generator),
        "evidence": {"coverage": coverage_of(row), "located_in_checkout": located},
    }


def locate(cfg: WaveConfig, row: dict) -> tuple[bool, str, str]:
    """(found, sha256 of the file at the pinned commit, content key: the symbol, its census line and the
    comment-free text of its file); read from git, so scratch-side injections never change it."""
    repo = cfg.repos.get(repo_key(row))
    if not repo:
        return False, "0" * 64, "missing:" + row["item_id"]
    p = subprocess.run(["git", "-C", repo["dir"], "show", f"HEAD:{row['path']}"], capture_output=True,
                       env={"PATH": "/usr/bin:/bin", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"})
    if p.returncode != 0:
        return False, "0" * 64, "missing:" + row["item_id"]
    raw = p.stdout
    text = raw.decode("utf-8", "replace")
    text = re.sub(r"//[^\n]*", "", re.sub(r"/\*.*?\*/", "", text, flags=re.S))
    text = re.sub(r"\s+", " ", text).strip()
    line = str((row.get("census") or [{}])[0].get("line") or "")
    key = hashlib.sha256((row["symbol"] + "\0" + line + "\0" + text).encode()).hexdigest()
    return True, hashlib.sha256(raw).hexdigest(), key


def finish(rec: dict, t0: float) -> dict:
    rec["evidence"]["wall_s"] = round(time.time() - t0, 2)
    return rec


# ------------------------------------------------------------------------------------------ adapters

def prepare_adapter(sh: Shared, name: str) -> dict:
    cfg = sh.cfg
    ad = T.ADAPTERS[name]
    key = next(k for k in cfg.repos if k.startswith(ad.repo_id + "@"))
    root = cfg.repos[key]["dir"]
    info = {"adapter": name, "repo": ad.repo_id, "toolchain": ad.toolchain, "toolchain_reason": ad.toolchain_reason,
            "halo2_line": ad.halo2_line}
    tc = HH.Toolchain(ad.toolchain, ad.toolchain_reason)
    if not tc.available():
        raise HH.HarnessError(f"toolchain {ad.toolchain} is not installed")
    os.makedirs(os.path.join(cfg.work, "logs"), exist_ok=True)
    log_path = os.path.join(cfg.work, "logs", f"build-{name}.log")
    if ad.kind == "vendored-bin":
        v = ad.vendor
        fkey = v["fork"] if v["fork"] in cfg.repos else next(k for k in cfg.repos if k.startswith(v["fork"] + "@"))
        fork = cfg.repos[fkey]["dir"]
        proofs = os.path.join(fork, v["fork_proofs"])
        info["fork"] = {"repo": v["fork"], "commit": cfg.repos[fkey]["commit"]}
        info["instrument"] = HH.instrument_proofs(proofs, ad.halo2_line)
        crate = os.path.join(cfg.work, "vendored", name)
        info["vendored_files"] = HH.assemble_vendored(crate, root, v["cargo_toml"], {"fork": fork, "repo": root},
                                                      v["files"], v.get("lockfile", ""), ad.driver_field)
        b = HH.build_tests(crate, ad.package, tc, cargo_env(cfg), log_path, bin_name=v["bin"])
    elif ad.patch:
        # start from the pinned manifest and lockfile (a previous run may have added the patch section)
        restore_manifest(root)
        dest_root = os.path.join(cfg.work, "patched")
        os.makedirs(dest_root, exist_ok=True)
        info["instrument"] = HH.patch_registry_crate(root, ad.patch["crate"], ad.patch["version"], tc, cargo_env(cfg),
                                                     dest_root, ad.halo2_line)
        proofs = os.path.join(dest_root, f"{ad.patch['crate']}-{ad.patch['version']}")
        info["injections"] = HH.inject_wrappers(os.path.join(root, ad.crate_dir), ad.injections, ad.driver_field)
        b = HH.build_tests(root, ad.package, tc, cargo_env(cfg), log_path)
    elif ad.git_patch:
        gp = ad.git_patch
        restore_manifest(root)
        fork = cfg.repos[gp["fork"]]["dir"]
        proofs = os.path.join(fork, "halo2_proofs")
        info["fork"] = {"repo": gp["fork"], "commit": cfg.repos[gp["fork"]]["commit"]}
        info["instrument"] = HH.instrument_proofs(proofs, ad.halo2_line)
        with open(os.path.join(root, "Cargo.toml"), encoding="utf-8") as f:
            text = f.read()
        for crate in gp["crates"]:
            spec = gp.get("spec") or (r'rev\s*=\s*"' + cfg.repos[gp["fork"]]["commit"][:12] + r'[0-9a-f]*"')
            pat = re.compile(r'^' + crate + r'\s*=\s*\{\s*git\s*=\s*"' + re.escape(gp["url"]) + r'",\s*' + spec
                             + r'\s*\}', re.M)
            text, n = pat.subn(f'{crate} = {{ path = "{os.path.join(fork, crate)}" }}', text)
            if n != 1:
                raise HH.HarnessError(f"[patch] entry of {crate} at the fork commit not found")
        with open(os.path.join(root, "Cargo.toml"), "w", encoding="utf-8") as f:
            f.write(text)
        info["instrument"]["patched"] = f"[patch] {', '.join(gp['crates'])} -> checkout of {gp['fork']}"
        if gp.get("note"):
            info["instrument"]["note"] = gp["note"]
        info["injections"] = HH.inject_wrappers(os.path.join(root, ad.crate_dir), ad.injections, ad.driver_field)
        b = HH.build_tests(root, ad.package, tc, cargo_env(cfg), log_path)
    else:
        proofs = os.path.join(root, ad.proofs_dir)
        info["instrument"] = HH.instrument_proofs(proofs, ad.halo2_line)
        info["injections"] = HH.inject_wrappers(os.path.join(root, ad.crate_dir), ad.injections, ad.driver_field)
        b = HH.build_tests(root, ad.package, tc, cargo_env(cfg), log_path)
    info.update(exe=b.exe, build_secs=round(b.secs, 1), lock_diff=b.lock_diff, exe_sha256=P.sha256_file(b.exe),
                proofs_version=_crate_version(proofs), bin_mode=ad.kind == "vendored-bin")
    return info


def restore_manifest(root: str) -> None:
    """The pinned Cargo.toml / Cargo.lock of the checkout (a previous run may have added the patch section; an
    untracked lockfile, generated by a build of a repository without one, is removed)."""
    genv = {"PATH": "/usr/bin:/bin", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}
    for name in ("Cargo.toml", "Cargo.lock"):
        tracked = subprocess.run(["git", "-C", root, "ls-files", "--error-unmatch", name], capture_output=True,
                                 env=genv).returncode == 0
        if tracked:
            subprocess.run(["git", "-C", root, "checkout", "--", name], capture_output=True, env=genv)
        elif name == "Cargo.lock" and os.path.exists(os.path.join(root, name)):
            os.remove(os.path.join(root, name))


def _crate_version(crate_dir: str) -> str:
    try:
        with open(os.path.join(crate_dir, "Cargo.toml"), encoding="utf-8") as f:
            m = re.search(r'^version\s*=\s*"([^"]+)"', f.read(), re.M)
        return m.group(1) if m else ""
    except OSError:
        return ""


# ------------------------------------------------------------------------------------------ per-row processing

def process(sh: Shared, row: dict, plan, located: tuple) -> dict:
    t0 = time.time()
    found, src_sha, _ = located
    rec = base_record(sh, row, src_sha, found)
    if isinstance(plan, T.Static):
        rec["status"], rec["status_reason"] = plan.status, plan.reason
        return finish(rec, t0)
    build = sh.builds.get(plan.adapter)
    if build is None or "error" in build:
        rec["status"] = "COMPILE-FAIL"
        rec["status_reason"] = "the wrapper test binary does not build: " + str((build or {}).get("error", "not built"))[:500]
        return finish(rec, t0)
    try:
        return finish(_process_wrapper(sh, row, plan, build, rec), t0)
    except Exception as ex:                                   # recorded, never silently dropped
        rec["status"] = "COMPILE-FAIL"
        rec["status_reason"] = f"generator error: {type(ex).__name__}: {str(ex)[:400]}"
        return finish(rec, t0)


def _ir_dir(sh: Shared, plan) -> str:
    return os.path.join(sh.cfg.work, "exports", plan.adapter)


def _process_wrapper(sh: Shared, row: dict, plan, build: dict, rec: dict) -> dict:
    cfg = sh.cfg
    ad = T.ADAPTERS[plan.adapter]
    out_root = _ir_dir(sh, plan)
    tdir = os.path.join(out_root, plan.name)
    shutil.rmtree(tdir, ignore_errors=True)
    # cheapest first: one sample sizes the layout; the other samples are run only for a model within the policy
    run = HH.run_target(build["exe"], plan.test, out_root, cargo_env(cfg), 1, 1, timeout=cfg.run_timeout,
                        bin_mode=build.get("bin_mode", False))
    first = sorted(glob.glob(os.path.join(tdir, "sample_*.json")))
    if first and cfg.samples > 1:
        try:
            small = H.flatten(H.load(first[0])).n_items <= MAX_CONSTRAINTS
        except H.IrError:
            small = False
        if small:
            run = HH.run_target(build["exe"], plan.test, out_root, cargo_env(cfg), cfg.samples, 1,
                                timeout=cfg.run_timeout, bin_mode=build.get("bin_mode", False))
    rec["env"]["halo2"] = f"{ad.halo2_line} (halo2_proofs {build.get('proofs_version', '')})"
    rec["env"]["rust"] = f"{ad.toolchain} ({ad.toolchain_reason})"
    docs_p = sorted(glob.glob(os.path.join(tdir, "sample_*.json")))
    errs = sorted(glob.glob(os.path.join(tdir, "sample_*.err")))
    rec["evidence"]["harness_run"] = {"test": plan.test, "rc": run["rc"], "secs": run["secs"],
                                      "samples_exported": len(docs_p), "samples_failed": len(errs)}
    if not docs_p:
        why = open(errs[0], encoding="utf-8").read()[:300] if errs else run["tail"][-300:]
        rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"the wrapper did not synthesize: {why}"
        return rec
    try:
        docs = [H.load(p) for p in docs_p]
        H.flatten(docs[0])
    except H.IrError as ex:
        rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"the exported layout is not modelled: {ex}"
        return rec
    d0 = docs[0]
    meta = d0["meta"]
    rec["instantiation"].update(rule=meta.get("rule", "repo-test"), call=meta.get("call", ""),
                                provenance=[meta.get("provenance", "")],
                                selection=f"wrapper test {plan.test}; k = {d0['k']}")
    struct = H.structure_sha256(d0)
    same = [d for d in docs if H.structure_sha256(d) == struct]
    model = H.flatten(d0)
    ir_sha = P.sha256_file(docs_p[0])
    size = model.size()
    within = model.n_items <= MAX_CONSTRAINTS
    rec["circuit"] = {
        "compiler": {"name": "halo2", "version": rec["env"]["halo2"], "flags": ["MockProver::run", f"k={d0['k']}"],
                     "binary_sha256": build["exe_sha256"], "source": f"wrapper test binary of {ad.package} "
                     f"({plan.adapter})"},
        "prime": str(model.p), "prime_name": model.field_name, "n_constraints": model.n_items,
        "n_wires": max(1, model.n_vars), "n_inputs": len(model.inputs), "n_outputs": len(model.outputs),
        "r1cs_sha256": model.sha256(),
        "halo2": {"k": d0["k"], "rows": d0["n"], "usable_rows": d0["usable_rows"], "ir_sha256": ir_sha,
                  "structure_sha256": struct, "columns": {"fixed": d0["num_fixed"], "advice": d0["num_advice"],
                                                          "instance": d0["num_instance"]},
                  "gates": len(d0["gates"]), "lookup_arguments": len(d0["lookups"]), **size,
                  "mockprover_verify_sample0": "ok" if not d0["verify"] else d0["verify"][:3],
                  "samples_same_layout": len(same), "samples": len(docs),
                  "unsat_constant_items": model.unsat_constants[:5]},
        "size_policy": {"max_constraints": MAX_CONSTRAINTS, "within": within},
    }
    names: dict = {}
    for reg in d0["regions"]:
        if not reg[0].startswith("boole-"):
            names[reg[0]] = names.get(reg[0], 0) + 1
    rec["evidence"]["layout_regions"] = dict(sorted(names.items()))
    if not within:
        rec["status"] = "TOO-LARGE"
        rec["status_reason"] = (f"{model.n_items} model items (gate instances + copies + lookups) above the "
                                f"{MAX_CONSTRAINTS}-item policy")
        return rec
    real, seen = [], set()
    for d in same:
        if d["verify"]:
            continue
        w = H.witness(model, d)
        if tuple(w) not in seen:
            seen.add(tuple(w))
            real.append(w)
    return build_package(sh, rec, row, plan, build, model, meta, real, d0, docs_p[0], tdir)


def write_witness(path: str, w: list[int]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(str(v) for v in w) + "\n")


def make_mutants(model: H.Model, real: list[list[int]], n: int, seed: str) -> list[dict]:
    """Single-cell mutants of the first real witness on advice cells of the model."""
    if not real:
        return []
    rng = random.Random(seed + "/mutants")
    cand = [v for v, c in enumerate(model.cells) if c[0] == "a"]
    if not cand:
        return []
    p = model.p
    out = []
    for k in range(n):
        v = cand[k % len(cand)] if k < len(cand) and k < n // 2 else rng.choice(cand)
        w = list(real[0])
        new = (w[v] + rng.choice([1, p - 1, rng.randrange(1, p)])) % p
        if new == w[v]:
            new = (new + 1) % p
        w[v] = new
        out.append({"var": v, "cell": list(model.cells[v]), "witness": w, "python": H.satisfies(model, w)})
    return out


def mockprover_verdicts(sh: Shared, plan, build: dict, model: H.Model, muts: list[dict], tdir: str) -> dict:
    with open(os.path.join(tdir, "mutants.txt"), "w", encoding="utf-8") as f:
        for m in muts:
            _, col, row = m["cell"]
            f.write(f"{col} {row} {m['witness'][m['var']]:064x}\n")
    outp = os.path.join(tdir, "mutants.out")
    if os.path.exists(outp):
        os.remove(outp)
    HH.run_target(build["exe"], plan.test, _ir_dir(sh, plan), cargo_env(sh.cfg), 1, 1, mutants=True,
                  timeout=sh.cfg.run_timeout, bin_mode=build.get("bin_mode", False))
    verdicts = {}
    if os.path.exists(outp):
        for k, ln in enumerate(open(outp, encoding="utf-8").read().split("\n")):
            parts = ln.split()
            if len(parts) == 3:
                verdicts[k] = parts[2] == "ACCEPT"
                if parts[2] == "PANIC":
                    verdicts.setdefault("panics", []).append(k)
    return verdicts


def lean_eval(sh: Shared, build_dir: str, ns: str, summary: dict, files: list[tuple[str, str]], work: str):
    src = os.path.join(work, "Fid.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(HE.emit_fid_runner(ns, summary, files))
    r = L.run_lean(sh.env, HE.LEAN_OPTIONS + ["--json", src], work, 3600, extra_lean_path=[build_dir],
                   rss_limit_mb=16384)
    text = "\n".join(m.get("data", "") for m in L.parse_messages(r.out))
    verdicts = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
    if "FID-DONE" not in text:
        verdicts["__incomplete__"] = "; ".join(L.fmt_msg(m) for m in L.errors(L.parse_messages(r.out))[:3]) or r.out[-300:]
    return verdicts, r


def g_fid_nonvac(sh: Shared, build_dir: str, ns: str, summary: dict, model: H.Model, real: list, muts: list,
                 mp: dict, work: str) -> tuple[G.Gate, G.Gate]:
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
    if not real:
        d = {"real_witnesses": 0, "reason": "no sample passed MockProver::verify with the layout of sample 0"}
        return G.Gate("G-FID", "FAIL", dict(d)), G.Gate("G-NONVAC", "FAIL", dict(d))
    with sh.lean_slots:
        verdicts, r = lean_eval(sh, build_dir, ns, summary, files, work)
    real_py = sum(H.satisfies(model, w) for w in real)
    real_lean = sum(verdicts.get(f"real_{k:03d}") == "ACCEPT" for k in range(len(real)))
    agree_lean = sum(verdicts.get(f"mut_{k:03d}") == ("ACCEPT" if m["python"] else "REJECT") for k, m in enumerate(muts))
    agree_mp = sum(mp.get(k) is not None and mp.get(k) == m["python"] for k, m in enumerate(muts))
    detail = {"method": "Lean #eval of decide (Constraints w) on every witness file, compared with the Python "
                        "evaluator and with MockProver::verify on the same single-cell mutants; every real witness "
                        "is a MockProver run that verifies",
              "real_witnesses": len(real), "real_lean_accept": real_lean, "real_python_accept": real_py,
              "real_mockprover_accept": len(real), "mutants": len(muts),
              "mutants_python_reject": sum(not m["python"] for m in muts), "mutants_lean_agree": agree_lean,
              "mutants_mockprover_agree": agree_mp, "lean_eval_secs": r.secs}
    if "__incomplete__" in verdicts:
        detail.update(error=verdicts["__incomplete__"][:400],
                      reason="the Lean evaluation did not complete (harness error); no verdict is inferred")
        return G.Gate("G-FID", "ERROR", dict(detail)), G.Gate("G-NONVAC", "ERROR", dict(detail))
    input_free = not model.inputs
    need = 1 if input_free else G.FID_MIN_REAL
    ok = (real_lean == len(real) and real_py == len(real) and len(real) >= need and len(muts) >= G.FID_MIN_MUTANTS
          and agree_lean == len(muts) and agree_mp == len(muts))
    fd = dict(detail, required_real=need, input_free=input_free)
    if not ok:
        reasons = []
        if len(real) < need:
            reasons.append(f"only {len(real)} distinct real witnesses (need {need})")
        if real_lean != len(real) or real_py != len(real):
            reasons.append("the Lean model or the Python evaluator rejected a real witness")
        if agree_lean != len(muts) or agree_mp != len(muts) or len(muts) < G.FID_MIN_MUTANTS:
            reasons.append("Lean / Python / MockProver verdicts differ on a mutant (or too few mutants)")
        fd["reason"] = "; ".join(reasons)
    nv = G.Gate("G-NONVAC", "PASS" if real_lean >= 1 and real_py >= 1 else "FAIL",
                {"real_witnesses_accepted": real_lean,
                 "witness_source": "MockProver::run of the wrapper on sampled inputs (MockProver::verify passes)"})
    return G.Gate("G-FID", "PASS" if ok else "FAIL", fd), nv


def search_context(model: H.Model) -> tuple[AS.Context, bool]:
    """The shared AIR search over the model: polynomial constraints and copies as constraints, fixed-table lookups as
    exact table predicates, inputs / outputs as messages of multiplicity 1.  False if advice tables occur."""
    air = HE.to_air(model)
    one = len(air.nodes)
    air.nodes.append(["const", 1])
    roles = AL.Roles()

    def var_node(v: int) -> int:
        air.nodes.append(["main", 0, v])
        return len(air.nodes) - 1
    roles.inputs = [AL.Message(-1, one, [var_node(v)], model.cell_name(v)) for v in model.inputs]
    roles.outputs = [AL.Message(-1, one, [var_node(v)], model.cell_name(v)) for v in model.outputs]
    supported = True
    sets = {}
    for ins, ti, label in model.lookups:
        t = model.tables[ti]
        if t.kind == "advice":
            supported = False
            continue
        roles.assumptions.append(AL.Assumption(-1, f"T{ti}", one, list(ins), label))
        if t.kind == "set" and ti not in sets:
            sets[ti] = set(map(tuple, t.rows))

    def assume(table: str, values: list[int]) -> bool:
        ti = int(table[1:])
        t = model.tables[ti]
        return values[0] < t.bound if t.kind == "range" else tuple(values) in sets[ti]
    return AS.Context(air, AIR.Layout.of(air), roles, assume), supported


def g_triv(sh: Shared, build_dir: str, ns: str, summary: dict, work: str) -> G.Gate:
    cfg = sh.cfg
    os.makedirs(work, exist_ok=True)
    files = [("Battery_P2", [(v, t) for v, ts in E.BATTERY_P2 for t in ts]),
             ("Battery_P1", [(v, t) for v in E.BATTERY_VARIANTS for t in E.BATTERY_TACTICS])]
    results, runs = {}, []

    def emit(forms):
        return HE.emit_battery_forms(ns, summary, forms, cfg.battery_heartbeats)
    for fname, forms in files:
        if any(c["closed"] for c in results.values()):
            runs.append({"variant": fname.replace("Battery_", ""), "skipped": "an earlier battery file closed the statement"})
            continue
        text = emit(forms)
        path = os.path.join(work, f"{fname}.lean")
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        r = L.run_lean(sh.env, HE.LEAN_OPTIONS + ["--json", path], work, cfg.battery_file_wall,
                       extra_lean_path=[build_dir], rss_limit_mb=16384)
        runs.append({"variant": fname.replace("Battery_", ""), "secs": r.secs, "timeout": r.timeout,
                     "memkill": r.memkill, "peak_rss_mb": r.peak_rss_mb})
        for name, c in G.classify_battery(text, L.parse_messages(r.out), r.timeout or r.memkill).items():
            if c["status"] == "timeout":
                form = next(fm for fm in forms if E.battery_theorem_name(*fm) == name)
                single = emit([form])
                spath = os.path.join(work, f"Single_{name}.lean")
                with open(spath, "w", encoding="utf-8") as f:
                    f.write(single)
                rs = L.run_lean(sh.env, HE.LEAN_OPTIONS + ["--json", spath], work, cfg.battery_single_wall,
                                extra_lean_path=[build_dir], rss_limit_mb=16384)
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
                reasons.append("the instruction has no result cells, so DET is vacuous")
            elif g == "G-TRIV" and gd.get("closed_by"):
                reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
                rec["statement"]["truth"] = "closed-by-automation"
            else:
                reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:600]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def build_package(sh: Shared, rec: dict, row: dict, plan, build: dict, model: H.Model, meta: dict, real: list,
                  d0: dict, ir_path: str, tdir: str) -> dict:
    cfg = sh.cfg
    collection, dir_name = rec["package_id"].split("/", 1)
    ns = P.lean_namespace(collection, dir_name)
    work = os.path.join(cfg.work, "items", collection, dir_name)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    stage = os.path.join(work, "pkg")
    lmeta = {"repo_id": rec["ids"]["repo"], "target": row["symbol"], "generator": f"{sh.generator['name']} "
             f"v{sh.generator['version']}", "repo_url": row["repo"], "commit": row["commit"], "path": row["path"],
             "symbol": row["symbol"], "wrapper": plan.test, "rule": rec["instantiation"]["rule"],
             "call": rec["instantiation"]["call"], "halo2_line": rec["env"]["halo2"],
             "ir_sha256": rec["circuit"]["halo2"]["ir_sha256"], "model_sha256": model.sha256()}
    model_rel = HE.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    text, summary = HE.emit_model(ns, lmeta, model)
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(text)
    statement_text = HE.emit_statement(ns, lmeta)
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{HE.STATEMENT_THEOREM}"
    rec["statement"] = {"file": "Statement.lean", "theorem": HE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": HE.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": statement_assumptions(model), "truth": "unknown"}
    rec["evidence"]["io"] = {"inputs": [model.cell_name(v) for v in model.inputs][:64],
                             "outputs": [model.cell_name(v) for v in model.outputs][:64],
                             "input_notes": model.input_notes}
    gates: dict = {}
    real_w = real[:REAL_WANTED]
    build_dir = os.path.join(work, "build")
    with sh.lean_slots:
        elab = G.g_elab(sh.env, stage, build_dir, ns, os.path.join(work, "elab"))
    gates["G-ELAB"] = elab.to_json()
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build_dir, model_rel[:-len(".lean")] + ".olean"))
    muts = make_mutants(model, real_w, MUTANTS, rec["package_id"])
    mp = mockprover_verdicts(sh, plan, build, model, muts, tdir) if muts else {}
    rec["evidence"]["mutants"] = [{"cell": m["cell"], "python": m["python"], "mockprover": mp.get(k),
                                   "mockprover_panic": k in mp.get("panics", [])} for k, m in enumerate(muts)]
    if model_ok:
        fid, nonvac = g_fid_nonvac(sh, build_dir, ns, summary, model, real_w, muts, mp, os.path.join(work, "fid"))
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()
    cheap_ok = all(gates[g]["status"] == "PASS" for g in ("G-ELAB", "G-NONVAC", "G-FID"))
    ce = None
    if not model.outputs:
        det_gate = {"status": "SKIPPED", "truth": "unknown", "reason": "no outputs: the statement is vacuous"}
    elif not cheap_ok:
        det_gate = {"status": "SKIPPED", "truth": "unknown",
                    "reason": "not run: the package already fails G-ELAB, G-NONVAC or G-FID"}
    else:
        ctx, supported = search_context(model)
        if not supported:
            det_gate = {"status": "SKIPPED", "truth": "unknown",
                        "reason": "advice-defined lookup tables are not supported by the shared search"}
        else:
            ce, slog = AS.search(ctx, real, rec["package_id"], cfg.det_search_budget_s)
            det_gate = {"status": "PASS", "truth": "unknown", "log": slog,
                        "note": "PASS means no counterexample was found by the cheap searches; DET truth is not "
                                "established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        write_witness(os.path.join(cdir, "w1.txt"), ce.base)
        write_witness(os.path.join(cdir, "w2.txt"), ce.other)
        py_ok = (H.satisfies(model, ce.base) and H.satisfies(model, ce.other)
                 and all(ce.base[v] == ce.other[v] for v in model.inputs)
                 and any(ce.base[v] != ce.other[v] for v in model.outputs))
        with sh.lean_slots:
            verdicts, _ = lean_eval(sh, build_dir, ns, summary, [("w1", os.path.join(cdir, "w1.txt")),
                                                                  ("w2", os.path.join(cdir, "w2.txt"))], cdir)
        lean_ok = verdicts.get("w1") == "ACCEPT" and verdicts.get("w2") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if (lean_ok and py_ok) else "unknown",
                    "method": ce.method, "changed_outputs": ce.changed_outputs[:20],
                    "lean_confirms_both_witnesses": lean_ok, "python_confirms": py_ok,
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
            gates["G-TRIV"] = g_triv(sh, build_dir, ns, summary, os.path.join(work, "triv")).to_json()
    rec["gates"] = gates
    set_status(rec, gates, bool(model.outputs))
    ref = elab.detail["reference_type_sha256"] if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": HE.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(HE.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": P.sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": HE.model_module(ns),
                                 "sha256": P.sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    write_package(sh, rec, stage, work, plan, real_w, ir_path)
    shutil.rmtree(work, ignore_errors=True)
    return rec


def write_package(sh: Shared, rec: dict, stage: str, work: str, plan, real: list, ir_path: str) -> None:
    dest = os.path.join(sh.cfg.out, *rec["package_id"].split("/", 1))
    if os.path.exists(dest):
        shutil.rmtree(dest)
    os.makedirs(dest)
    shutil.copyfile(os.path.join(stage, "Statement.lean"), os.path.join(dest, "Statement.lean"))
    model_rel = rec["statement"]["model_file"]
    os.makedirs(os.path.dirname(os.path.join(dest, model_rel)), exist_ok=True)
    shutil.copyfile(os.path.join(stage, model_rel), os.path.join(dest, model_rel))
    ev = os.path.join(dest, "evidence")
    os.makedirs(os.path.join(ev, "witnesses"))
    shutil.copyfile(ir_path, os.path.join(ev, "ir_sample_000.json"))
    for k, w in enumerate(real[:4]):
        write_witness(os.path.join(ev, "witnesses", f"real_{k:03d}.txt"), w)
    ad = T.ADAPTERS[plan.adapter]
    sources = [i.source for i in ad.injections] + [s for s, _ in ad.vendor.get("files", []) if not s.startswith("repo:")]
    with open(os.path.join(ev, "wrapper.txt"), "w", encoding="utf-8") as f:
        f.write(f"wrapper {plan.test} in {ad.package} ({plan.adapter}, {ad.kind}); wrapper sources under "
                f"scripts/zk_registry/halo2_harness/wrappers: {', '.join(sources)}\n")
    ref = os.path.join(work, "elab", "reference.type.txt")
    if os.path.exists(ref):
        shutil.copyfile(ref, os.path.join(ev, "reference.type.txt"))
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


# ------------------------------------------------------------------------------------------ wave

def run_wave(cfg: WaveConfig) -> list[dict]:
    os.makedirs(cfg.out, exist_ok=True)
    os.makedirs(cfg.work, exist_ok=True)
    env = L.load_env(cfg.lean_env)
    sh = Shared(cfg, env, P.sha256_file(cfg.ledger), generator_info())
    sh.lean_slots = threading.Semaphore(max(1, cfg.lean_jobs))
    rows = select_rows(cfg.ledger)
    if cfg.only:
        rows = [r for r in rows if r["item_id"] in set(cfg.only)]
    for key, repo in cfg.repos.items():
        head = _git_head(repo["dir"])
        if head != repo["commit"]:
            raise ValueError(f"{key} is at {head}, the wave pins {repo['commit']}")
    located = {r["item_id"]: locate(cfg, r) for r in rows}
    plans = {r["item_id"]: T.plan(r) for r in rows}
    groups: dict[str, list] = {}
    for r in rows:
        groups.setdefault(located[r["item_id"]][2], []).append(r)
    canon, dedup = [], []
    for k, members in groups.items():
        members.sort(key=lambda m: m["item_id"])
        canon.append(members[0])
        for m in members[1:]:
            dedup.append({"item_id": m["item_id"], "canonical_item_id": members[0]["item_id"], "content_sha256": k,
                          "relation": "duplicate-of-h1"})
    log(sh, f"selection {len(rows)} rows, {sum(v[0] for v in located.values())} located, {len(canon)} distinct by "
            f"content, {len(dedup)} duplicates")
    records: list[dict] = []
    static = [r for r in canon if isinstance(plans[r["item_id"]], T.Static)]
    for r in static:
        records.append(process(sh, r, plans[r["item_id"]], located[r["item_id"]]))
    by_adapter: dict[str, list] = {}
    for r in canon:
        p = plans[r["item_id"]]
        if isinstance(p, T.Wrapper):
            by_adapter.setdefault(p.adapter, []).append(r)
    for name in cfg.adapters:
        arows = by_adapter.pop(name, [])
        if not arows:
            continue
        if free_bytes(cfg.scratch) < DISK_FLOOR:
            raise RuntimeError("free space on the data volume below 15 GB; stopping")
        log(sh, f"adapter {name}: build ({len(arows)} rows)")
        try:
            sh.builds[name] = prepare_adapter(sh, name)
            log(sh, f"adapter {name}: built in {sh.builds[name]['build_secs']} s")
        except Exception as ex:
            sh.builds[name] = {"error": f"{type(ex).__name__}: {str(ex)[:600]}"}
            log(sh, f"adapter {name}: build failed: {str(ex)[:300]}")
        with cf.ThreadPoolExecutor(max_workers=min(cfg.jobs, MAX_JOBS)) as pool:
            futs = [pool.submit(process, sh, r, plans[r["item_id"]], located[r["item_id"]]) for r in arows]
            for fut in cf.as_completed(futs):
                rec = fut.result()
                records.append(rec)
                append_jsonl(os.path.join(cfg.out, "PROGRESS.jsonl"), rec)
                log(sh, f"{rec['ids']['template']}: {rec['status']} {rec['status_reason'][:120]}")
        if os.path.isdir(cfg.target_dir) and cfg.target_dir.startswith(cfg.scratch + os.sep):
            shutil.rmtree(cfg.target_dir)
    for name, arows in by_adapter.items():        # adapters not enabled in the configuration
        for r in arows:
            rec = base_record(sh, r, located[r["item_id"]][1], located[r["item_id"]][0])
            rec["status"], rec["status_reason"] = "COMPILE-FAIL", f"adapter {name} is not enabled in this wave"
            records.append(rec)
    for rec in records:
        rec["evidence"]["content_sha256"] = located[rec["ids"]["ledger_item_id"]][2]
        if rec["status"] in P.PACKAGED_STATUSES:
            P.write_json(os.path.join(cfg.out, *rec["package_id"].split("/", 1), "problem.json"), rec)
    by_id = {r["ids"]["ledger_item_id"]: r for r in records}
    for d in dedup:
        c = by_id.get(d["canonical_item_id"])
        d["canonical_package_id"] = c["package_id"] if c else None
        d["canonical_status"] = c["status"] if c else None
    write_outputs(cfg.out, records, dedup)
    with open(os.path.join(cfg.out, "BUILDS.json"), "w", encoding="utf-8") as f:
        json.dump(sh.builds, f, indent=1, sort_keys=True)
    return records


def _git_head(d: str) -> str:
    return L._git_head(d)


def append_jsonl(path: str, rec: dict) -> None:
    with open(path, "a", encoding="utf-8") as f:
        f.write(json.dumps(rec, sort_keys=True, ensure_ascii=False) + "\n")


def write_index(path: str, recs: list[dict]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for r in recs:
            f.write(json.dumps(r, sort_keys=True, ensure_ascii=False) + "\n")


def write_outputs(out: str, records: list[dict], dedup: list[dict]) -> None:
    records = sorted(records, key=lambda r: r["ids"]["ledger_item_id"])
    by_repo: dict[str, list] = {}
    for r in records:
        by_repo.setdefault(r["package_id"].split("/", 1)[0], []).append(r)
    for repo, recs in by_repo.items():
        write_index(os.path.join(out, repo, "INDEX.jsonl"), recs)
    write_index(os.path.join(out, "INDEX.jsonl"), records)
    write_index(os.path.join(out, "DEDUP.jsonl"), sorted(dedup, key=lambda d: d["item_id"]))


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


# ------------------------------------------------------------------------------------------ rerun

def run_rerun(cfg: WaveConfig, items: list[str]) -> list[dict]:
    """Regenerate the named records with the current generator into a side directory and merge them into the wave
    output (replaced package directories removed; every replacement logged in RERUN-MERGE.jsonl)."""
    main_out = cfg.out
    side = cfg.out.rstrip("/") + ".rerun"
    if os.path.isdir(side):
        shutil.rmtree(side)
    cfg.out, cfg.only = side, list(items)
    new = run_wave(cfg)
    cfg.out = main_out
    with open(os.path.join(main_out, "INDEX.jsonl"), encoding="utf-8") as f:
        old = {r["ids"]["ledger_item_id"]: r for r in (json.loads(line) for line in f if line.strip())}
    for rec in new:
        iid = rec["ids"]["ledger_item_id"]
        prev = old.get(iid)
        if prev and prev["status"] in P.PACKAGED_STATUSES:
            d = os.path.join(main_out, *prev["package_id"].split("/", 1))
            if os.path.isdir(d):
                shutil.rmtree(d)
        if rec["status"] in P.PACKAGED_STATUSES:
            src = os.path.join(side, *rec["package_id"].split("/", 1))
            dst = os.path.join(main_out, *rec["package_id"].split("/", 1))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.move(src, dst)
        append_jsonl(os.path.join(main_out, "RERUN-MERGE.jsonl"),
                     {"item_id": iid, "before": prev["status"] if prev else None, "after": rec["status"],
                      "before_reason": (prev or {}).get("status_reason", "")[:300],
                      "generator_sources_sha256": rec["generator"]["sources_sha256"]})
        old[iid] = rec
    with open(os.path.join(main_out, "DEDUP.jsonl"), encoding="utf-8") as f:
        dedup = [json.loads(line) for line in f if line.strip()]
    write_outputs(main_out, list(old.values()), dedup)
    shutil.rmtree(side)
    return new


# ------------------------------------------------------------------------------------------ decomposition

GENERIC_REGIONS = {"load private", "constants", "", "table_idx"}


def decompose(index_path: str, out_dir: str) -> dict:
    """Region-level decomposition of TOO-LARGE records: every named region of a TOO-LARGE layout is mapped to the
    packaged wave records whose wrapper layout has a region of the same name (the instruction that assigns it);
    ``same_source`` marks children built from the same gadget source (same repository and pin, or the crates.io
    release the census identifies with that pin).  No new package and no compositional statement is made."""
    with open(index_path, encoding="utf-8") as f:
        recs = [json.loads(line) for line in f if line.strip()]
    by_region: dict = {}
    for r in recs:
        if r["status"] in P.PACKAGED_STATUSES:
            for name in r["evidence"].get("layout_regions", {}):
                by_region.setdefault(name, []).append(r)
    edges, parents = [], 0
    same_family = {("zcash/orchard", "zcash/halo2")}       # orchard builds halo2_gadgets 0.5.0 = zcash/halo2 d751768a
    for r in recs:
        if r["status"] != "TOO-LARGE":
            continue
        parents += 1
        for name, count in r["evidence"].get("layout_regions", {}).items():
            if name in GENERIC_REGIONS:
                continue
            kids = by_region.get(name, [])
            edges.append({"parent": r["package_id"], "region": name, "region_count": count,
                          "children": [{"package_id": c["package_id"], "status": c["status"],
                                        "same_source": c["ids"]["repo"] == r["ids"]["repo"] and
                                        c["ids"]["commit"] == r["ids"]["commit"] or
                                        (r["ids"]["repo"], c["ids"]["repo"]) in same_family} for c in kids]})
    os.makedirs(out_dir, exist_ok=True)
    write_index(os.path.join(out_dir, "EDGES.jsonl"), edges)
    summary = {"parents": parents, "region_edges": len(edges),
               "mapped_to_wave_records": sum(1 for e in edges if e["children"]),
               "mapped_same_source": sum(1 for e in edges if any(c["same_source"] for c in e["children"])),
               "unmapped": sum(1 for e in edges if not e["children"])}
    with open(os.path.join(out_dir, "SUMMARY.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=1, sort_keys=True)
    return summary


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("select")
    s.add_argument("--ledger", required=True)
    s = sub.add_parser("wave")
    s.add_argument("--config", required=True)
    s.add_argument("--jobs", type=int)
    s.add_argument("--only", nargs="*")
    s = sub.add_parser("validate")
    s.add_argument("--index", required=True)
    s.add_argument("--packages", required=True)
    s = sub.add_parser("rerun")
    s.add_argument("--config", required=True)
    s.add_argument("--items", required=True)
    s = sub.add_parser("decompose")
    s.add_argument("--index", required=True)
    s.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    if a.cmd == "decompose":
        print(json.dumps(decompose(a.index, a.out), indent=1, sort_keys=True))
        return 0
    if a.cmd == "select":
        rows = select_rows(a.ledger)
        by_repo: dict[str, int] = {}
        plan_counts: dict[str, int] = {}
        for r in rows:
            by_repo[repo_key(r)] = by_repo.get(repo_key(r), 0) + 1
            p = T.plan(r)
            k = f"wrapper:{p.adapter}" if isinstance(p, T.Wrapper) else p.status
            plan_counts[k] = plan_counts.get(k, 0) + 1
        mixed = len(select_rows(a.ledger, "plonky3-AIR+halo2"))
        print(json.dumps({"rows": len(rows), "by_repo": by_repo, "plan": plan_counts, "filter": SELECTION_FILTER,
                          "plonky3_air_halo2_rows": mixed}, indent=1, sort_keys=True))
        return 0
    if a.cmd == "validate":
        problems = validate_all(a.index, a.packages)
        for p in problems[:50]:
            print(p)
        print(f"{len(problems)} problem(s)")
        return 1 if problems else 0
    cfg = WaveConfig.load(a.config)
    if a.cmd == "rerun":
        with open(a.items, encoding="utf-8") as f:
            recs = run_rerun(cfg, [x.strip() for x in f if x.strip()])
        print(json.dumps({r["ids"]["ledger_item_id"]: r["status"] for r in recs}, indent=1, sort_keys=True))
        return 0
    if a.jobs:
        cfg.jobs = min(a.jobs, MAX_JOBS)
    if a.only:
        cfg.only = a.only
    recs = run_wave(cfg)
    counts: dict[str, int] = {}
    for r in recs:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    print(json.dumps(counts, indent=1, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
