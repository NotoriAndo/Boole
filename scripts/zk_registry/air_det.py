#!/usr/bin/env python3
"""AIR DET problem generator (zkVM chips): wave driver.

Subcommands::

    prepare   --zkvm Z --src DIR                       add the extractor crate (``air_harness/``) to a scratch copy
                                                       of the zkVM workspace at the census pin
    wave      --config wave.json [--jobs N]            package every extracted AIR of one zkVM
    validate  --index INDEX.jsonl --packages DIR       re-validate every record and package
    summary   --index INDEX.jsonl                      counts by status, gates, closures

The extractor (a Rust harness built against the zkVM's own crates) writes ``manifest.jsonl`` (every AIR of
the machine, extracted or failed with the reason), ``airs/<index>.json`` (``boole-air-ir/v1``) and
``rows/<index>.json`` (real rows from the zkVM's trace generation on the sample programs).  For each AIR the
driver classifies the interactions with the zkVM's bus model (``air_bus``), emits the Lean model and the DET
statement (``air_lean``), and runs the gates: G-ELAB, G-NONVAC (a real active window satisfies the model and
the assumptions), G-FID (real windows and mutants: Lean verdicts equal the independent Python evaluator's),
G-TRIV (battery P1 + P2) and a sound counterexample search (``air_search``).  The driver never writes a
proof: the only Lean proofs attempted are the battery forms, whose closures fail G-TRIV.
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
import time
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import air_bus as B              # noqa: E402
from zk_registry import air_ir as IR              # noqa: E402
from zk_registry import air_lean as AL            # noqa: E402
from zk_registry import air_search as AS          # noqa: E402
from zk_registry import check as C                # noqa: E402
from zk_registry import gates as G                # noqa: E402
from zk_registry import lean_emit as E            # noqa: E402
from zk_registry import lean_runner as L          # noqa: E402
from zk_registry import package as P              # noqa: E402

GENERATOR_NAME = "boole-zk-registry-air-det"
GENERATOR_VERSION = "1.0"
GENERATOR_SOURCES = ["air_det.py", "air_ir.py", "air_lean.py", "air_bus.py", "air_search.py", "det_search.py",
                     "gates.py", "lean_emit.py", "lean_runner.py", "check.py", "package.py", "jsonschema_lite.py",
                     "lean/ZkReplay.lean", "schema/air_problem.schema.json",
                     "air_harness/common/boole_air_ir.rs"]
HERE = os.path.dirname(os.path.abspath(__file__))
HARNESS_DIR = os.path.join(HERE, "air_harness")
# size policy of a packaged AIR: constraint polynomials and distinct expression nodes (Lean elaboration and the
# evaluation harness stay within a few minutes and 16 GB per process below these bounds)
MAX_CONSTRAINTS = 4000
MAX_NODES = 100000
FID_REAL = 12
FID_MUTANTS = 12
SEARCH_POOL = 64

DET_SPEC_CLAUSES = [
    "Output determinism of an AIR row window: for the constraint polynomials of the AIR as recorded by the zkVM's "
    "own symbolic builder, any two assignments to the window's variables that satisfy every constraint and the "
    "stated bus assumptions, agree on the fixed variables (preprocessed cells, public values, row selectors) and "
    "make equal input-message contributions make equal output-message contributions.",
    "Messages are the AIR's bus interactions (direction, bus, values, multiplicity) as recorded by the zkVM's own "
    "symbolic builder; the recorded bus model assigns every message field the role input, output or table-lookup "
    "fact. Two messages contribute equally iff their multiplicities are equal and, where the multiplicity is "
    "non-zero, their values are equal.",
    "The field is the zkVM's configured prime field (BabyBear or KoalaBear). A window is one row, or the local and "
    "next rows when a constraint references the next row.",
]
DET_SPEC = {
    "tier": "T1",
    "source": "mathematical definition of output determinism (functional dependence of an AIR's output messages on "
              "its input messages)",
    "clauses": DET_SPEC_CLAUSES,
    "sha256": hashlib.sha256("\n".join(DET_SPEC_CLAUSES).encode("utf-8")).hexdigest(),
}
DET_PROPERTY = {"template": "DET", "name": "output determinism of an AIR row window (no under-constrained output)"}
WINDOW_NOT_A_FINDING = ("not-a-finding: two-row window counterexample; the window statement is stronger than trace-level "
                        "determinism (the local row's history is not in the window), so it does not refute the AIR")
LEAN_FIELD_PRIME = {"BabyBear": "BabyBear prime 2^31 - 2^27 + 1", "KoalaBear": "KoalaBear prime 2^31 - 2^24 + 1"}


def sha256_file(path: str) -> str:
    return P.sha256_file(path)


def generator_files() -> list[str]:
    """Every file the AIR generator hash covers: the listed sources and the per-zkVM extractor sources."""
    out = list(GENERATOR_SOURCES)
    for z in sorted(os.listdir(HARNESS_DIR)):
        d = os.path.join(HARNESS_DIR, z)
        if z != "common" and os.path.isdir(d):
            out += [f"air_harness/{z}/{fn}" for fn in sorted(os.listdir(d))]
    return out


def generator_info() -> dict:
    h = hashlib.sha256()
    for rel in generator_files():
        h.update(rel.encode() + b"\0" + sha256_file(os.path.join(HERE, rel)).encode() + b"\n")
    return {"name": GENERATOR_NAME, "version": GENERATOR_VERSION, "sources_sha256": h.hexdigest()}


def statement_assumptions(field_name: str) -> list[str]:
    return [f"[Fact (Nat.Prime p)]: primality of the {LEAN_FIELD_PRIME[field_name]}, supplied as an instance hypothesis "
            "so that field lemmas apply; p is prime, so the hypothesis does not weaken the statement."]


# ------------------------------------------------------------------------------------------ configuration

@dataclass
class WaveConfig:
    zkvm: str
    release: str
    commit: str
    repo: str
    repo_url: str
    extract: str                      # extractor output directory
    lean_env: str
    out: str                          # packages/<wave>/<zkvm>
    work: str
    collection: str                   # package id prefix, e.g. "sp1-v6.8.1"
    ledger: str | None = None
    jobs: int = 4
    only: list[str] = field(default_factory=list)
    battery_heartbeats: int = 200000
    battery_file_wall: float = 900
    battery_single_wall: float = 180
    search_budget_s: float = 20.0
    rust: str = ""

    @staticmethod
    def load(path: str) -> "WaveConfig":
        with open(path, encoding="utf-8") as f:
            return WaveConfig(**json.load(f))


@dataclass
class Shared:
    cfg: WaveConfig
    env: L.LeanEnv
    bus: "B.BusModel"
    build: dict
    ledger_ids: set
    ledger_sha: str | None
    generator: dict
    python: str = platform.python_version()
    seen_content: dict = field(default_factory=dict)

    def env_record(self) -> dict:
        pins = self.env.pins()
        return {"lean": pins["lean"], "mathlib": pins["mathlib"], "lake_manifest_sha256": pins["lake_manifest_sha256"],
                "packages": pins["packages"], "python": self.python, "rust": self.build.get("rustc", self.cfg.rust)}


# ------------------------------------------------------------------------------------------ records

def dir_name(index: int, name: str) -> str:
    slug = re.sub(r"[^A-Za-z0-9]+", "_", name).strip("_")
    if len(slug) > 60:
        slug = slug[:48] + "_" + hashlib.sha256(name.encode()).hexdigest()[:8]
    return f"{index:03d}.{slug or 'air'}"


def ledger_item(sh: Shared, rust_type: str) -> tuple[str, bool]:
    base = re.sub(r"<.*$", "", rust_type).split("::")[-1].strip()
    item = f"{sh.cfg.zkvm}:{base}"
    return item, item in sh.ledger_ids


def base_record(sh: Shared, m: dict) -> dict:
    m = dict(m, rust_type=sh.bus.chip_type_at(m["name"], m["index"]) if (m.get("rust_type") or m["name"]) == m["name"]
             else m["rust_type"])
    item, found = ledger_item(sh, m["rust_type"])
    rec = {"schema_version": P.AIR_SCHEMA_VERSION,
           "package_id": f"{sh.cfg.collection}/{dir_name(m['index'], m['name'])}",
           "property": dict(DET_PROPERTY), "status": "EXTRACTION-FAILED", "status_reason": "",
           "ids": {"ledger_item_id": item, "zkvm": sh.cfg.zkvm, "repo": sh.cfg.repo, "repo_url": sh.cfg.repo_url,
                   "release": sh.cfg.release, "commit": sh.cfg.commit, "air_name": m["name"],
                   "rust_type": m.get("rust_type") or m["name"], "air_index": m["index"], "group": m.get("group", "")},
           "spec": dict(DET_SPEC), "coverage": B.coverage(sh.cfg.zkvm, m["name"], m.get("rust_type") or ""),
           "env": sh.env_record(), "generator": sh.generator}
    if sh.cfg.ledger:
        rec["ids"]["ledger"] = {"file": os.path.basename(sh.cfg.ledger), "sha256": sh.ledger_sha, "row_found": found}
    if sh.bus.label(m["index"]):
        rec["ids"]["label"] = sh.bus.label(m["index"])
    return rec


def air_record(sh: Shared, air: IR.Air, ir_path: str, within: bool, layout: IR.Layout | None) -> dict:
    return {"field": air.field_name, "p": air.p, "width": air.width, "preprocessed_width": air.prep_width,
            "num_public_values": air.n_public, "window_rows": air.window_rows(),
            "n_vars": layout.n_vars if layout else 0, "n_constraints": len(air.constraints),
            "n_nodes": len(air.nodes), "degree": air.degree(), "n_interactions": len(air.interactions),
            "ir_sha256": sha256_file(ir_path), "content_sha256": air.content_sha256(),
            "selectors": sorted(air.uses()["selectors"]),
            "extractor": {"harness_sha256": sh.build.get("harness_sha256", "0" * 64),
                          "builder": sh.build.get("builder", ""), "rust_toolchain": sh.build.get("rustc", ""),
                          "toolchain_deviation": sh.build.get("toolchain_deviation", ""),
                          "features": sh.build.get("features", "")},
            "size_policy": {"max_constraints": MAX_CONSTRAINTS, "max_nodes": MAX_NODES, "within": within}}


# ------------------------------------------------------------------------------------------ rows

def load_rows(path: str) -> dict | None:
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8") as f:
        d = json.load(f)
    return d


def real_windows(air: IR.Air, layout: IR.Layout, rows: dict | None) -> list[tuple[int, list[int]]]:
    """(row index, window) for every dumped row whose window rows were dumped too."""
    if not rows or not rows.get("height"):
        return []
    h = rows["height"]
    main = {int(k): v for k, v in rows["main"].items()}
    prep = {int(k): v for k, v in rows.get("prep", {}).items()}
    pub = rows.get("public_values", [])
    out = []
    for i in sorted(main):
        nxt = (i + 1) % h
        if layout.main_next and nxt not in main:
            continue
        if layout.prep_width and (i not in prep or (layout.prep_next and nxt not in prep)):
            continue
        need_pub = max(layout.public, default=-1)
        if need_pub >= len(pub):
            continue
        w = layout.window(main, prep if layout.prep_width else None, pub, i, h)
        out.append((i, [x % air.p for x in w]))
    return out


def negative_multiplicities(air: IR.Air, layout: IR.Layout, real: list[tuple[int, list[int]]]) -> frozenset:
    """Interactions whose multiplicity takes a negated value (above p/2) on a real window that satisfies the
    constraints (a read sent with a negated count from a column)."""
    out = set()
    half = air.p // 2
    for _, w in real:
        vals = IR.eval_nodes(air, layout, w)
        if IR.failing_constraints(air, layout, w, vals):
            continue
        for k, it in enumerate(air.interactions):
            if vals[it.mult] > half:
                out.add(k)
    return frozenset(out)


def mutants(air: IR.Air, bases: list[list[int]], n: int, seed: str, fixed: set[int]) -> list[list[int]]:
    rng = random.Random(seed + "/air-mutants")
    out = []
    free = [i for i in range(len(bases[0])) if i not in fixed] if bases else []
    if not free:
        return out
    for k in range(n):
        w = list(bases[k % len(bases)])
        var = free[rng.randrange(len(free))]
        new = (w[var] + rng.choice([1, air.p - 1, rng.randrange(1, air.p)])) % air.p
        w[var] = new if new != w[var] else (w[var] + 1) % air.p
        out.append(w)
    return out


# ------------------------------------------------------------------------------------------ Lean evaluation

def lean_eval(env: L.LeanEnv, build: str, text: str, work: str, name: str, timeout: float = 3600) -> tuple[str, L.RunResult]:
    os.makedirs(work, exist_ok=True)
    src = os.path.join(work, f"{name}.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(text)
    r = L.run_lean(env, AL.LEAN_OPTIONS + ["--json", src], work, timeout, extra_lean_path=[build])
    msgs = L.parse_messages(r.out)
    out = "\n".join(m.get("data", "") for m in msgs)
    if r.rc != 0 and not out:
        out = r.out
    return out, r


def fid_nonvac(sh: Shared, air: IR.Air, layout: IR.Layout, roles: AL.Roles, summary: dict, ns: str, build: str,
               real: list[tuple[int, list[int]]], work: str, seed: str) -> tuple[G.Gate, G.Gate, list[list[int]]]:
    """Returns (G-FID, G-NONVAC, the real windows accepted by the Python evaluator)."""
    ctx = AS.Context(air, layout, roles, sh.bus.table_fn)
    py_ok, active_ok = [], []
    rejected = []
    seen = set()
    for i, w in real:
        key = tuple(w)
        if key in seen:
            continue
        seen.add(key)
        vals = IR.eval_nodes(air, layout, w)
        cons = not IR.failing_constraints(air, layout, w, vals)
        asm = AS.assumptions_hold(ctx, vals, w)
        if cons and asm:
            py_ok.append((i, w))
            if any(vals[m.mult] != 0 for m in roles.outputs):
                active_ok.append((i, w))
        else:
            rejected.append({"row": i, "constraints": cons, "assumptions": asm,
                             "failing": IR.failing_constraints(air, layout, w, vals)[:5]})
    # real windows for FID: active first, then the rest (padding rows are real rows too)
    chosen = active_ok[:FID_REAL // 2]
    for x in py_ok:
        if len(chosen) >= FID_REAL:
            break
        if x not in chosen:
            chosen.append(x)
    muts = mutants(air, [w for _, w in chosen], FID_MUTANTS, seed, set(layout.fixed())) if chosen else []
    files = []
    wdir = os.path.join(work, "win")
    os.makedirs(wdir, exist_ok=True)
    # every real window the Python evaluator rejects is evaluated too (the model must reject it as well)
    rej_windows = [w for i, w in real if any(r["row"] == i for r in rejected)][:4]
    items = [("real", k, w) for k, (_, w) in enumerate(chosen)] + [("mut", k, w) for k, w in enumerate(muts)] + \
            [("rej", k, w) for k, w in enumerate(rej_windows)]
    for kind, k, w in items:
        p = os.path.join(wdir, f"{kind}_{k:03d}.txt")
        AL.write_window(p, w)
        files.append((f"{kind}_{k:03d}", p))
    detail: dict = {"method": "Lean #eval of decide (Constraints w) and decide (Assumptions w) on window files, "
                              "compared with the independent Python evaluator of the extracted polynomials",
                    "real_rows_dumped": len(real), "real_windows_distinct": len(seen),
                    "python_accepted": len(py_ok), "python_rejected_real": len(rejected),
                    "python_rejected_examples": rejected[:3], "active_accepted": len(active_ok)}
    if not files:
        detail["reason"] = "no real rows from the zkVM's trace generation on the sample programs"
        return G.Gate("G-FID", "FAIL", dict(detail)), G.Gate("G-NONVAC", "FAIL", dict(detail)), []
    if kernel_mode(air, roles):
        detail["method"] = ("Lean kernel evaluation (decide +kernel) of Constraints and Assumptions on every window, "
                            "asserting the verdict of the independent Python evaluator (compiled evaluation would "
                            "re-evaluate shared subterms exponentially)")
        verdict, asmv, r, err = kernel_verdicts(sh, air, layout, ctx, summary, ns, build, items, work)
    else:
        text, r = lean_eval(sh.env, build, AL.emit_fid_runner(ns, summary, files), work, "Fid")
        verdict = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
        asmv = dict(re.findall(r"^ASM (\S+) (ACCEPT|REJECT)$", text, re.M))
        err = None if "FID-DONE" in text else text[-400:]
    detail["lean_eval_secs"] = r.secs
    if err is not None:
        detail["error"] = err
        detail["reason"] = "the Lean evaluation did not complete (harness error); no verdict is inferred"
        return G.Gate("G-FID", "ERROR", dict(detail)), G.Gate("G-NONVAC", "ERROR", dict(detail)), []
    agree_c = agree_a = 0
    for kind, k, w in items:
        tag = f"{kind}_{k:03d}"
        vals = IR.eval_nodes(air, layout, w)
        pc = not IR.failing_constraints(air, layout, w, vals)
        pa = AS.assumptions_hold(ctx, vals, w)
        agree_c += verdict.get(tag) == ("ACCEPT" if pc else "REJECT")
        agree_a += asmv.get(tag) == ("ACCEPT" if pa else "REJECT")
    n_real = len(chosen)
    real_lean = sum(verdict.get(f"real_{k:03d}") == "ACCEPT" and asmv.get(f"real_{k:03d}") == "ACCEPT"
                    for k in range(n_real))
    detail.update(real_windows=n_real, real_lean_accept=real_lean, mutants=len(muts),
                  mutants_python_reject=sum(1 for w in muts if not IR.satisfies(air, layout, w)),
                  evaluated=len(items), constraints_verdicts_agree=agree_c, assumptions_verdicts_agree=agree_a)
    fid_ok = (n_real >= min(FID_REAL, 10) and real_lean == n_real and len(muts) >= 10
              and agree_c == len(items) and agree_a == len(items) and not rejected)
    reasons = []
    if n_real < 10:
        reasons.append(f"only {n_real} distinct real windows accepted by the Python evaluator (need 10)")
    if real_lean != n_real:
        reasons.append("the Lean model rejected a real window")
    if agree_c != len(items) or agree_a != len(items):
        reasons.append("Lean and Python verdicts differ")
    if rejected:
        reasons.append(f"{len(rejected)} real window(s) violate the extracted constraints or the assumptions in Python")
    if len(muts) < 10:
        reasons.append("too few mutants")
    if reasons:
        detail["reason"] = "; ".join(reasons)
    nonvac_active = [k for k, (i, w) in enumerate(chosen) if (i, w) in active_ok
                     and verdict.get(f"real_{k:03d}") == "ACCEPT" and asmv.get(f"real_{k:03d}") == "ACCEPT"]
    nonvac = {"active_windows_accepted": len(nonvac_active),
              "method": "real window from the zkVM's trace generation with a non-zero output multiplicity, accepted by "
                        "Constraints and Assumptions in Lean and Python"}
    if not nonvac_active:
        nonvac["reason"] = ("no real window with an active output message satisfies the model"
                            if active_ok or not py_ok else "the sample programs produced no active row for this AIR")
    return (G.Gate("G-FID", "PASS" if fid_ok else "FAIL", detail),
            G.Gate("G-NONVAC", "PASS" if nonvac_active else "FAIL", nonvac), [w for _, w in py_ok])


KERNEL_EVAL_COST = 10_000_000_000


def eval_cost(air: IR.Air, roles: AL.Roles) -> int:
    """Work of evaluating the model's terms without sharing (compiled code recomputes every shared subterm at each
    use): the summed tree sizes of the constraint and assumption roots, capped."""
    sizes = IR.tree_sizes(air, cap=10**12)
    roots = list(air.constraints)
    for a in roles.assumptions:
        roots += [a.mult] + a.values
    return min(sum(sizes[r] for r in roots), 10**12)


def kernel_mode(air: IR.Air, roles: AL.Roles) -> bool:
    return eval_cost(air, roles) > KERNEL_EVAL_COST


def kernel_verdicts(sh: Shared, air: IR.Air, layout: IR.Layout, ctx, summary: dict, ns: str, build: str,
                    items: list, work: str):
    """Python verdicts asserted as kernel-checked theorems; a theorem that fails is a disagreement (its verdict is
    recorded as the opposite).  Returns (constraint verdicts, assumption verdicts, run, error)."""
    windows, claims, expect = [], [], {}
    for kind, k, w in items:
        tag = f"{kind}_{k:03d}"
        vals = IR.eval_nodes(air, layout, w)
        pc = not IR.failing_constraints(air, layout, w, vals)
        pa = AS.assumptions_hold(ctx, vals, w)
        windows.append((tag, w))
        for what, ok in (("Constraints", pc), ("Assumptions", pa)):
            name = f"{what[0].lower()}_{tag}"
            claims.append((name, f"{'' if ok else '¬ '}{what} w_{tag}"))
            expect[name] = (tag, what, ok)
    text, ranges = AL.emit_kernel_checks(ns, summary, windows, claims)
    os.makedirs(work, exist_ok=True)
    src = os.path.join(work, "FidKernel.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(text)
    r = L.run_lean(sh.env, AL.LEAN_OPTIONS + ["--json", src], work, 3600, extra_lean_path=[build])
    msgs = L.parse_messages(r.out)
    if r.timeout or (r.rc != 0 and not L.errors(msgs)):
        return {}, {}, r, (r.out[-400:] or "timeout")
    bad = set()
    for m in L.errors(msgs):
        line = (m.get("pos") or {}).get("line")
        for name, (lo, hi) in ranges.items():
            if line is not None and lo <= line <= hi:
                bad.add(name)
        if line is None or not any(lo <= line <= hi for lo, hi in ranges.values()):
            return {}, {}, r, L.fmt_msg(m)
    verdict, asmv = {}, {}
    for name, (tag, what, ok) in expect.items():
        held = name not in bad
        accept = ok if held else not ok
        (verdict if what == "Constraints" else asmv)[tag] = "ACCEPT" if accept else "REJECT"
    return verdict, asmv, r, None


def confirm_in_lean(sh: Shared, ns: str, summary: dict, build: str, w1: list[int], w2: list[int], work: str,
                    kernel: bool = False) -> dict:
    os.makedirs(work, exist_ok=True)
    if kernel:
        claims = [("pair_c1", "Constraints w_a"), ("pair_c2", "Constraints w_b"), ("pair_a1", "Assumptions w_a"),
                  ("pair_a2", "Assumptions w_b"), ("pair_fixed", "∀ i ∈ Fixed, w_a i = w_b i"),
                  ("pair_in", "BusEq (In w_a) (In w_b)"), ("pair_out", "¬ BusEq (Out w_a) (Out w_b)")]
        text, ranges = AL.emit_kernel_checks(ns, summary, [("a", w1), ("b", w2)], claims)
        src = os.path.join(work, "PairKernel.lean")
        with open(src, "w", encoding="utf-8") as f:
            f.write(text)
        r = L.run_lean(sh.env, AL.LEAN_OPTIONS + ["--json", src], work, 3600, extra_lean_path=[build])
        errs = L.errors(L.parse_messages(r.out))
        ok = r.rc == 0 and not r.timeout and not errs
        return {"lean_confirms": ok, "lean": {"mode": "kernel", "errors": [L.fmt_msg(m) for m in errs[:3]]}}
    a, b = os.path.join(work, "w1.txt"), os.path.join(work, "w2.txt")
    AL.write_window(a, w1)
    AL.write_window(b, w2)
    text, _ = lean_eval(sh.env, build, AL.emit_pair_runner(ns, summary, a, b), work, "Pair")
    got = dict(re.findall(r"^PAIR (\w+) (.*)$", text, re.M))
    ok = (got.get("constraints") == "true true" and got.get("assumptions") == "true true" and got.get("fixed") == "true"
          and got.get("inputs") == "true" and got.get("outputs") == "false" and "PAIR-DONE" in text)
    return {"lean_confirms": ok, "lean": got}


# ------------------------------------------------------------------------------------------ per AIR

def process_air(sh: Shared, m: dict) -> dict:
    t0 = time.time()
    rec = base_record(sh, m)
    try:
        return _done(_process_air(sh, m, rec), t0)
    except Exception as exc:        # recorded, never silently dropped
        rec.update(status="EXTRACTION-FAILED", status_reason=f"generator error: {type(exc).__name__}: {exc}"[:600])
        for k in ("air", "interface", "statement", "gates", "checker"):
            rec.pop(k, None)
        return _done(rec, t0)


def _done(rec: dict, t0: float) -> dict:
    rec.setdefault("evidence", {})["wall_secs"] = round(time.time() - t0, 1)
    return rec


def _process_air(sh: Shared, m: dict, rec: dict) -> dict:
    cfg = sh.cfg
    if m["status"] != "extracted":
        rec["status_reason"] = f"extractor: {m.get('detail', '')}"[:600]
        return rec
    ir_path = os.path.join(cfg.extract, "airs", f"{m['index']}.json")
    air = IR.load(ir_path)
    within = len(air.constraints) <= MAX_CONSTRAINTS and len(air.nodes) <= MAX_NODES
    layout = IR.Layout.of(air)
    rec["air"] = air_record(sh, air, ir_path, within, layout)
    first = sh.seen_content.setdefault(air.content_sha256(), rec["package_id"])
    if first != rec["package_id"]:
        rec["ids"]["copy_of"] = first
    if not within:
        rec.update(status="TOO-LARGE", status_reason=(f"{len(air.constraints)} constraints / {len(air.nodes)} expression "
                                                      f"nodes exceed the size policy ({MAX_CONSTRAINTS} / {MAX_NODES})"))
        return rec
    rows = load_rows(os.path.join(cfg.extract, "rows", f"{m['index']}.json"))
    real = real_windows(air, layout, rows)
    roles, iface = sh.bus.roles(air, negative_multiplicities(air, layout, real))
    rec["interface"] = iface
    dname = rec["package_id"].split("/", 1)[1]
    work = os.path.join(cfg.work, dname)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    ns = "ZkDet." + E.lean_ident(f"{cfg.collection}_{dname}")
    meta = {"zkvm_name": sh.bus.display, "release": cfg.release, "generator": f"{sh.generator['name']} v{sh.generator['version']}",
            "repo_url": cfg.repo_url, "extractor": sh.build.get("builder", "the zkVM's symbolic builders"),
            "ir_sha256": rec["air"]["ir_sha256"]}
    stage = os.path.join(work, "pkg")
    model_rel = AL.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    model_text, summary = AL.emit_model(ns, meta, air, layout, roles, sh.bus.tables)
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(model_text)
    statement_text = AL.emit_statement(ns, meta, air)
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{AL.STATEMENT_THEOREM}"
    asm_text = statement_assumptions(air.field_name) + sh.bus.assumption_texts(roles, iface)
    if roles.selectors:
        asm_text.append("Row selectors are modelled normalized (is_first_row, is_last_row ∈ {0, 1}, is_transition = "
                        "1 - is_last_row); every constraint is homogeneous of degree ≤ 1 in each selector it uses, so "
                        "the constraint zero sets equal the prover's.")
    rec["statement"] = {"file": "Statement.lean", "theorem": AL.STATEMENT_THEOREM, "theorem_fqn": fqn,
                        "model_module": AL.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": asm_text, "truth": "unknown", "window_rows": air.window_rows()}
    gates: dict[str, dict] = {}
    evidence: dict = {"lean_summary": {"hoisted": len(summary["hoisted"]), "blocks": len(summary["blocks"]),
                                       "tables": summary["tables"]}}
    homo = selector_homogeneous(air)
    if not homo:
        evidence["selector_non_homogeneous"] = True

    build = os.path.join(work, "build")
    elab = G.g_elab(sh.env, stage, build, ns, os.path.join(work, "elab"))
    gates["G-ELAB"] = elab.to_json()
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build, model_rel[:-len(".lean")] + ".olean"))

    evidence["rows"] = {"source": (rows or {}).get("source", "none"), "height": (rows or {}).get("height", 0),
                        "windows": len(real)}
    if model_ok:
        fid, nonvac, pool = fid_nonvac(sh, air, layout, roles, summary, ns, build, real, os.path.join(work, "fid"),
                                       rec["package_id"])
    else:
        fid = G.Gate("G-FID", "SKIPPED", {"reason": "model did not compile"})
        nonvac = G.Gate("G-NONVAC", "SKIPPED", {"reason": "model did not compile"})
        pool = []
    gates["G-FID"], gates["G-NONVAC"] = fid.to_json(), nonvac.to_json()

    ctx = AS.Context(air, layout, roles, sh.bus.table_fn)
    ce, log = AS.search(ctx, pool[:SEARCH_POOL], rec["package_id"], cfg.search_budget_s) if pool and roles.outputs \
        else (None, {"bases": len(pool)})
    det_gate = {"status": "PASS", "truth": "unknown", "log": log,
                "note": "PASS means no counterexample was found by the cheap searches; DET truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        lc = (confirm_in_lean(sh, ns, summary, build, ce.base, ce.other, cdir, kernel_mode(air, roles))
              if model_ok else {"lean_confirms": False})
        window = air.window_rows() == 2
        truth = ("window-false-counterexample-found" if window else "false-counterexample-found") \
            if lc["lean_confirms"] else "unknown"
        det_gate = {"status": "FAIL", "truth": truth, "method": ce.method,
                    "changed_outputs": [roles.outputs[k].note for k in ce.changed_outputs][:10],
                    "lean_confirms_pair": lc["lean_confirms"], "python_confirms": True, "log": log,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"]}
        if window:
            det_gate["finding"] = WINDOW_NOT_A_FINDING
        evidence["counterexample_dir"] = cdir
    gates["DET-SEARCH"] = det_gate

    if elab.status != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement did not elaborate"}
    elif det_gate["truth"] != "unknown":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement refuted by a confirmed counterexample"}
    else:
        def emit(forms):
            return AL.emit_battery_forms(ns, summary, forms, cfg.battery_heartbeats)
        gates["G-TRIV"] = G.g_triv(sh.env, build, ns, len(air.constraints), os.path.join(work, "triv"),
                                   cfg.battery_heartbeats, cfg.battery_file_wall, cfg.battery_single_wall,
                                   emit=emit).to_json()
    rec["gates"] = gates
    set_status(rec, gates, bool(roles.outputs), homo)
    ref = elab.detail["reference_type_sha256"] if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": AL.STATEMENT_THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(AL.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": AL.model_module(ns),
                                 "sha256": sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    rec["evidence"] = evidence
    write_package(sh, rec, stage, work, dname)
    return rec


def selector_homogeneous(air: IR.Air) -> bool:
    """Every constraint is homogeneous of degree ≤ 1 in each row selector (it uses the selector only as a guard
    factor), so normalized selector values give the prover's zero sets."""
    degs: list[dict[str, frozenset]] = []
    zero = {s: frozenset([0]) for s in IR.SELECTORS}
    for n in air.nodes:
        op = n[0]
        if op in IR.SELECTORS:
            d = dict(zero)
            d[op] = frozenset([1])
        elif op in IR.LEAF_OPS:
            d = zero
        elif op == "neg":
            d = degs[n[1]]
        elif op in ("add", "sub"):
            a, b = degs[n[1]], degs[n[2]]
            d = {s: a[s] | b[s] for s in IR.SELECTORS}
        else:
            a, b = degs[n[1]], degs[n[2]]
            d = {s: frozenset(x + y for x in a[s] for y in b[s]) for s in IR.SELECTORS}
        degs.append(d)
    for c in air.constraints:
        for s in IR.SELECTORS:
            if degs[c][s] not in (frozenset([0]), frozenset([1])):
                return False
    return True


def set_status(rec: dict, gates: dict, has_outputs: bool, homogeneous: bool = True) -> None:
    det_gate = gates["DET-SEARCH"]
    st = rec["statement"]
    if det_gate["truth"] == "false-counterexample-found":
        rec["status"] = "DET-FALSE-CANDIDATE"
        rec["status_reason"] = (f"{det_gate['method']} found two row assignments satisfying the constraints and the "
                                "assumptions with equal inputs and different outputs (confirmed in Python and Lean); "
                                "private finding")
        st["truth"] = "false-counterexample-found"
        return
    failed = [g for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV") if gates[g]["status"] != "PASS"]
    reasons = []
    if det_gate["truth"] == "window-false-counterexample-found":
        st["truth"] = "window-false-counterexample-found"
        reasons.append(f"DET-SEARCH: the two-row window statement is false ({det_gate['method']}); {WINDOW_NOT_A_FINDING}")
        failed = [g for g in failed if g != "G-TRIV"]
    elif det_gate["status"] == "FAIL":
        reasons.append("DET-SEARCH: a Python counterexample was not confirmed in Lean")
    if not has_outputs:
        reasons.append("the AIR has no output messages in the bus model, so DET is vacuous")
    if not homogeneous:
        reasons.append("a constraint is not homogeneous in a row selector (normalized selectors would change it)")
    for g in failed:
        gd = gates.get(g, {})
        if g == "G-TRIV" and gd.get("closed_by"):
            reasons.append(f"G-TRIV closed by {', '.join(gd['closed_by'][:4])}")
            st["truth"] = "closed-by-automation"
        else:
            reasons.append(f"{g}: {gd.get('reason') or gd.get('errors') or gd.get('status')}")
    if reasons:
        rec["status"] = "GATE-FAIL"
        rec["status_reason"] = "; ".join(str(x) for x in reasons)[:700]
    else:
        rec["status"] = "OPEN"
        rec["status_reason"] = "all gates pass; DET truth unknown"


def write_package(sh: Shared, rec: dict, stage: str, work: str, dname: str) -> None:
    dest = os.path.join(sh.cfg.out, dname)
    shutil.rmtree(dest, ignore_errors=True)
    os.makedirs(dest)
    shutil.copyfile(os.path.join(stage, "Statement.lean"), os.path.join(dest, "Statement.lean"))
    model_rel = rec["statement"]["model_file"]
    os.makedirs(os.path.dirname(os.path.join(dest, model_rel)), exist_ok=True)
    shutil.copyfile(os.path.join(stage, model_rel), os.path.join(dest, model_rel))
    ev = os.path.join(dest, "evidence")
    os.makedirs(ev)
    shutil.copyfile(os.path.join(sh.cfg.extract, "airs", f"{rec['ids']['air_index']}.json"), os.path.join(ev, "air.ir.json"))
    src = os.path.join(work, "elab", "reference.type.txt")
    if os.path.exists(src):
        shutil.copyfile(src, os.path.join(ev, "reference.type.txt"))
    cdir = rec["evidence"].pop("counterexample_dir", None)
    if cdir:
        os.makedirs(os.path.join(ev, "counterexample"))
        for fn in ("w1.txt", "w2.txt"):
            shutil.copyfile(os.path.join(cdir, fn), os.path.join(ev, "counterexample", fn))
    triv = os.path.join(work, "triv")
    if os.path.isdir(triv):
        os.makedirs(os.path.join(ev, "triv"))
        for fn in sorted(os.listdir(triv)):
            if fn.endswith(".lean"):
                shutil.copyfile(os.path.join(triv, fn), os.path.join(ev, "triv", fn))


# ------------------------------------------------------------------------------------------ wave

_LOCAL_PATH_RE = re.compile(r"(?<![A-Za-z0-9_$.:/])/(?:private|tmp|var|Users|home)(?:/[^\s\\\"',;)]*)?")


def scrub(sh: Shared, rec: dict) -> dict:
    text = json.dumps(rec, sort_keys=True, ensure_ascii=False)
    for path, tag in ((sh.cfg.work, "$WORK"), (sh.cfg.extract, "$EXTRACT"), (sh.env.scratch, "$SCRATCH"),
                      (sh.env.toolchain, "$TOOLCHAIN"), (os.path.expanduser("~"), "$HOME")):
        for variant in {path, os.path.realpath(path)}:
            if variant and len(variant) > 1:
                text = text.replace(json.dumps(variant)[1:-1], tag)
    return json.loads(_LOCAL_PATH_RE.sub("$LOCAL", text))


def load_ledger(path: str | None, zkvm: str) -> tuple[set, str | None]:
    if not path:
        return set(), None
    ids = set()
    with open(path, encoding="utf-8") as f:
        for line in f:
            if f'"item_id": "{zkvm}:' in line:
                ids.add(json.loads(line)["item_id"])
    return ids, sha256_file(path)


def prepare(cfg: WaveConfig) -> tuple[Shared, list[dict]]:
    with open(os.path.join(cfg.extract, "build.json"), encoding="utf-8") as f:
        build = json.load(f)
    if build.get("commit") != cfg.commit:
        raise ValueError(f"extract was built at {build.get('commit')}, the wave pins {cfg.commit}")
    manifest = []
    with open(os.path.join(cfg.extract, "manifest.jsonl"), encoding="utf-8") as f:
        for line in f:
            if line.strip():
                manifest.append(json.loads(line))
    if cfg.only:
        manifest = [m for m in manifest if m["name"] in cfg.only or str(m["index"]) in cfg.only]
    ids, sha = load_ledger(cfg.ledger, cfg.zkvm)
    sh = Shared(cfg, L.load_env(cfg.lean_env), B.model_for(cfg.zkvm), build, ids, sha, generator_info())
    # content duplicates are resolved in manifest order, independent of worker scheduling; the bus model sees
    # every AIR (bus names from their owners) before any role is assigned
    airs = []
    full = [json.loads(x) for x in open(os.path.join(cfg.extract, "manifest.jsonl"), encoding="utf-8") if x.strip()]
    for m in full:
        if m["status"] == "extracted":
            air = IR.load(os.path.join(cfg.extract, "airs", f"{m['index']}.json"))
            airs.append(air)
            sh.seen_content.setdefault(air.content_sha256(), f"{cfg.collection}/{dir_name(m['index'], m['name'])}")
    sh.bus.observe(airs)
    return sh, manifest


def run_wave(cfg: WaveConfig, jobs: int | None = None) -> list[dict]:
    """Package every AIR of the extract (or the ``only`` subset: its records replace the same package ids of an
    existing index, the other records are kept)."""
    sh, manifest = prepare(cfg)
    os.makedirs(cfg.out, exist_ok=True)
    os.makedirs(cfg.work, exist_ok=True)
    kept: list[dict] = []
    index = os.path.join(cfg.out, "INDEX.jsonl")
    if cfg.only and os.path.exists(index):
        rerun = {f"{cfg.collection}/{dir_name(m['index'], m['name'])}" for m in manifest}
        with open(index, encoding="utf-8") as f:
            kept = [r for r in (json.loads(x) for x in f if x.strip()) if r["package_id"] not in rerun]
    results = []
    order = interleave_by_size(manifest, lambda m: _ir_size(cfg, m))
    with cf.ThreadPoolExecutor(max_workers=jobs or cfg.jobs) as ex:
        futs = {ex.submit(process_air, sh, m): m for m in order}
        for k, fut in enumerate(cf.as_completed(futs), 1):
            rec = scrub(sh, fut.result())
            if rec["status"] in P.PACKAGED_STATUSES:
                P.write_json(os.path.join(cfg.out, rec["package_id"].split("/", 1)[1], "problem.json"), rec)
            results.append(rec)
            print(f"[{k}/{len(order)}] {rec['package_id']}: {rec['status']} ({rec['evidence'].get('wall_secs')} s)",
                  flush=True)
            write_index(cfg.out, kept + results)
    write_index(cfg.out, kept + results)
    return results


def _ir_size(cfg: WaveConfig, m: dict) -> int:
    path = os.path.join(cfg.extract, "airs", f"{m['index']}.json")
    return os.path.getsize(path) if m["status"] == "extracted" and os.path.exists(path) else 0


def interleave_by_size(items: list, size) -> list:
    """Largest first for an even drain, alternating with the smallest so that few large models (memory-heavy Lean
    runs) are processed at the same time."""
    ordered = sorted(items, key=lambda m: (-size(m), m["index"]))
    out, lo, hi = [], 0, len(ordered) - 1
    while lo <= hi:
        out.append(ordered[lo])
        lo += 1
        if lo <= hi:
            out.append(ordered[hi])
            hi -= 1
    return out


def write_index(out: str, records: list[dict]) -> None:
    records = sorted(records, key=lambda r: r["ids"]["air_index"])
    tmp = os.path.join(out, "INDEX.jsonl.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        for rec in records:
            f.write(json.dumps(rec, sort_keys=True, ensure_ascii=False) + "\n")
    os.replace(tmp, os.path.join(out, "INDEX.jsonl"))


def validate_all(index_path: str, packages_dir: str) -> list[str]:
    problems = []
    with open(index_path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            rec = json.loads(line)
            pkg = os.path.join(packages_dir, rec["package_id"].split("/", 1)[1])
            has_dir = os.path.isdir(pkg)
            errs = P.validate_problem(rec, pkg if has_dir else None)
            if rec["status"] in P.PACKAGED_STATUSES:
                if not has_dir:
                    errs.append("package directory missing")
                else:
                    with open(os.path.join(pkg, "problem.json"), encoding="utf-8") as g:
                        if json.load(g) != rec:
                            errs.append("problem.json differs from the index record")
            elif has_dir:
                errs.append(f"{rec['status']} record has a package directory")
            problems += [f"line {n} {rec['package_id']}: {e}" for e in errs]
    return problems


AIR_STATUSES = ("OPEN", "GATE-FAIL", "DET-FALSE-CANDIDATE", "TOO-LARGE", "EXTRACTION-FAILED")


def summarize(records: list[dict]) -> dict:
    by_status = {s: 0 for s in AIR_STATUSES}
    for r in records:
        by_status[r["status"]] += 1
    gates: dict = {g: {} for g in ("G-ELAB", "G-NONVAC", "G-FID", "G-TRIV", "DET-SEARCH")}
    for r in records:
        for g, res in r.get("gates", {}).items():
            gates[g][res["status"]] = gates[g].get(res["status"], 0) + 1
    closures = [{"package_id": r["package_id"], "closed_by": r["gates"]["G-TRIV"]["closed_by"]}
                for r in records if r.get("gates", {}).get("G-TRIV", {}).get("closed_by")]
    coverage: dict = {}
    for r in records:
        coverage[r["coverage"]["status"]] = coverage.get(r["coverage"]["status"], 0) + 1
    window: dict = {}
    for r in records:
        if "air" in r:
            window[r["air"]["window_rows"]] = window.get(r["air"]["window_rows"], 0) + 1
    walls = sorted(r.get("evidence", {}).get("wall_secs", 0) for r in records)
    return {"records": len(records), "by_status": by_status, "gates": gates, "triv_closures": closures,
            "coverage": coverage, "window_rows": window,
            "item_wall_secs": {"sum": round(sum(walls), 1), "median": walls[len(walls) // 2] if walls else 0,
                               "max": walls[-1] if walls else 0}}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="AIR DET problem generator")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a0 = sub.add_parser("prepare")
    a0.add_argument("--zkvm", required=True,
                    choices=sorted(d for d in os.listdir(HARNESS_DIR) if d != "common" and os.path.isdir(os.path.join(HARNESS_DIR, d))))
    a0.add_argument("--src", required=True)
    a1 = sub.add_parser("wave")
    a1.add_argument("--config", required=True)
    a1.add_argument("--jobs", type=int)
    a2 = sub.add_parser("validate")
    a2.add_argument("--index", required=True)
    a2.add_argument("--packages", required=True)
    a3 = sub.add_parser("summary")
    a3.add_argument("--index", required=True)
    a4 = sub.add_parser("record-build", help="write <extract>/build.json after the extractor ran")
    a4.add_argument("--zkvm", required=True)
    a4.add_argument("--src", required=True)
    a4.add_argument("--extract", required=True)
    a4.add_argument("--toolchain", required=True, help="rustup toolchain name used")
    a4.add_argument("--pinned", required=True, help="toolchain the repository pins")
    a4.add_argument("--features", default="")
    a4.add_argument("--builder", required=True)
    a = ap.parse_args(argv)
    if a.cmd == "prepare":
        print(json.dumps(prepare_harness(a.zkvm, a.src), indent=1))
    elif a.cmd == "wave":
        run_wave(WaveConfig.load(a.config), a.jobs)
    elif a.cmd == "validate":
        errs = validate_all(a.index, a.packages)
        for e in errs:
            print(e)
        print(f"{len(errs)} problem(s)")
        return 1 if errs else 0
    elif a.cmd == "summary":
        with open(a.index, encoding="utf-8") as f:
            print(json.dumps(summarize([json.loads(x) for x in f]), indent=1))
    elif a.cmd == "record-build":
        P.write_json(os.path.join(a.extract, "build.json"),
                     build_record(a.zkvm, a.src, a.toolchain, a.pinned, a.features, a.builder))
    return 0


def build_record(zkvm: str, src: str, toolchain: str, pinned: str, features: str, builder: str) -> dict:
    """Pins of an extractor run: source commit, harness hash, rustc, features, toolchain deviation."""
    import subprocess
    commit = subprocess.run(["git", "-C", src, "rev-parse", "HEAD"], capture_output=True, text=True,
                            check=True).stdout.strip()
    rustc = subprocess.run(["rustc", f"+{toolchain}", "--version"], capture_output=True, text=True,
                           check=True).stdout.strip()
    installed = toolchain.split("-aarch64")[0].split("-x86_64")[0]
    deviation = "" if installed == pinned else (f"repository pins {pinned}, which is not installed; used the installed "
                                                f"{installed} ({rustc})")
    return {"zkvm": zkvm, "commit": commit, "harness_sha256": harness_sha256(zkvm), "rustc": rustc,
            "toolchain": toolchain, "pinned_toolchain": pinned, "toolchain_deviation": deviation,
            "features": features, "builder": builder}


def harness_sha256(zkvm: str) -> str:
    h = hashlib.sha256()
    for d in ("common", zkvm):
        for fn in sorted(os.listdir(os.path.join(HARNESS_DIR, d))):
            h.update(f"{d}/{fn}".encode() + b"\0" + sha256_file(os.path.join(HARNESS_DIR, d, fn)).encode() + b"\n")
    return h.hexdigest()


def prepare_harness(zkvm: str, src: str) -> dict:
    """Install the extractor into the workspace ``src`` (a scratch copy at the census pin).

    ``install.json`` of the harness directory selects the mode: ``crate`` (default; a new workspace member
    ``boole-air-extract``) or ``module`` (a ``#[cfg(test)]`` module of an existing crate, for zkVMs whose test
    helpers are crate-private)."""
    zdir = os.path.join(HARNESS_DIR, zkvm)
    inst_path = os.path.join(zdir, "install.json")
    inst = {"mode": "crate"}
    if os.path.exists(inst_path):
        with open(inst_path, encoding="utf-8") as f:
            inst = json.load(f)
    ir_src = os.path.join(HARNESS_DIR, "common", "boole_air_ir.rs")
    if inst["mode"] == "module":
        d = os.path.join(src, inst["dir"])
        shutil.copyfile(os.path.join(zdir, "main.rs"), os.path.join(d, f"{inst['module']}.rs"))
        shutil.copyfile(ir_src, os.path.join(d, "boole_air_ir.rs"))
        lib = os.path.join(src, inst["lib"])
        with open(lib, encoding="utf-8") as f:
            text = f.read()
        lines = ["#[cfg(test)]\npub mod boole_air_ir;", f"#[cfg(test)]\npub mod {inst['module']};"]
        add = [x for x in lines if x not in text]
        if add:
            with open(lib, "a", encoding="utf-8") as f:
                f.write("\n" + "\n".join(add) + "\n")
        return {"module": os.path.join(d, f"{inst['module']}.rs"), "harness_sha256": harness_sha256(zkvm)}
    crate = os.path.join(src, "boole-air-extract")
    shutil.rmtree(crate, ignore_errors=True)
    os.makedirs(os.path.join(crate, "src"))
    for fn in os.listdir(zdir):
        if fn == "install.json":
            continue
        dst = os.path.join(crate, "Cargo.toml" if fn == "Cargo.toml.in" else os.path.join("src", fn))
        shutil.copyfile(os.path.join(zdir, fn), dst)
    shutil.copyfile(ir_src, os.path.join(crate, "src", "boole_air_ir.rs"))
    ws = os.path.join(src, "Cargo.toml")
    with open(ws, encoding="utf-8") as f:
        text = f.read()
    if '"boole-air-extract"' not in text:
        new, n = re.subn(r"(\[workspace\][^\[]*?members\s*=\s*\[)", r'\1\n  "boole-air-extract",', text, count=1, flags=re.S)
        if n != 1:
            raise ValueError("workspace members list not found")
        with open(ws, "w", encoding="utf-8") as f:
            f.write(new)
    return {"crate": crate, "harness_sha256": harness_sha256(zkvm)}

if __name__ == "__main__":
    sys.exit(main())
