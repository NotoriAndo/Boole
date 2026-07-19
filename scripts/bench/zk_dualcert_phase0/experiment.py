"""S0–S7 gate experiment runner for `zk-circuit-uniqueness-dual-cert.v0` P0-A.

THROWAWAY offline experiment code — never wired into self-test/consensus.
Local tools only (cadical, kissat, z3-python, pinned Lean v4.29.1). No paid
APIs, no network at run time.

Honesty rules baked in (spec + tasks/lessons.md):
  * solver `unknown`/timeout is recorded as UNDECIDED, never as hardness and
    never as a solve;
  * the difficulty verdict is always the PORTFOLIO attacker (fastest of the
    structural attacks and the solvers), not any single solver;
  * results go to a temp/explicit --out path by default; the committed
    result.sample.json is only written with the explicit --write-sample flag;
  * failed gates are recorded as-is.

Usage:
  python3 experiment.py --mode quick --out /tmp/zkdc.json
  python3 experiment.py --hash-instance <seed> <n> <pub> <out> <ncl> <nxor> <ngate> <k>
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import tempfile
import time

import attackers
import lean_lrat
import solvers
from encode import (
    canon_binding,
    circuit_canonical_bytes,
    d_dimacs_bytes,
    witness_bytes,
)
from gen import CANDIDATE, Params, generate
from lrat_native import check_lrat
from verify import verify_bug
from xof import Xof

HERE = os.path.dirname(os.path.abspath(__file__))
SAMPLE_PATH = os.path.join(HERE, "result.sample.json")


# --------------------------------------------------------------------------
# band definitions
# --------------------------------------------------------------------------

def _mk(n, rc, rx, rg, out, k, pub_ratio=0.2) -> Params:
    return Params(
        n_vars=n,
        n_pub=max(1, round(pub_ratio * n)),
        n_out=out,
        n_clause=round(rc * n),
        n_xor=round(rx * n),
        n_gate=round(rg * n),
        clause_width=k,
    )


BASE = dict(n=120, rc=3.0, rx=0.15, rg=0.15, out=4, k=3)


def band_axes(mode: str) -> dict[str, list[Params]]:
    """One axis varies per sweep (S3); everything else stays at BASE."""
    sizes = [60, 120, 240] + ([480] if mode == "full" else [])
    axes = {
        "size": [_mk(n, BASE["rc"], BASE["rx"], BASE["rg"], BASE["out"], BASE["k"]) for n in sizes],
        "density": [
            _mk(BASE["n"], rc, BASE["rx"], BASE["rg"], BASE["out"], BASE["k"])
            for rc in ([2.0, 3.0, 4.0, 5.0] if mode != "full" else [1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 5.0, 6.0])
        ],
        "xor": [
            _mk(BASE["n"], BASE["rc"], rx, BASE["rg"], BASE["out"], BASE["k"])
            for rx in [0.0, 0.2, 0.5]
        ],
        "gate": [
            _mk(BASE["n"], BASE["rc"], BASE["rx"], rg, BASE["out"], BASE["k"])
            for rg in [0.0, 0.2, 0.5]
        ],
        "out": [
            _mk(BASE["n"], BASE["rc"], BASE["rx"], BASE["rg"], out, BASE["k"])
            for out in [1, 4, 12]
        ],
        "width": [
            _mk(BASE["n"], BASE["rc"], BASE["rx"], BASE["rg"], BASE["out"], k)
            for k in [2, 3, 4]
        ],
    }
    if mode == "full":
        # Escalation probe: can ANY size push the portfolio attacker into a
        # non-trivial regime near the BUG/SAFE boundary density? (S1/S3/S6)
        axes["escalation"] = [
            _mk(n, rc, BASE["rx"], BASE["rg"], BASE["out"], BASE["k"])
            for n, rc in [(480, 3.0), (480, 3.5), (960, 3.0), (960, 3.5), (1920, 3.2)]
        ]
    return axes


TINY = Params(n_vars=14, n_pub=3, n_out=2, n_clause=30, n_xor=2, n_gate=2, clause_width=3)


# --------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------

def pct(sorted_vals: list[float], p: float):
    if not sorted_vals:
        return None
    idx = min(len(sorted_vals) - 1, max(0, round(p / 100 * (len(sorted_vals) - 1))))
    return sorted_vals[idx]


def summary(vals: list[float]) -> dict:
    vs = sorted(v for v in vals if v is not None)
    if not vs:
        return {"count": 0}
    return {
        "count": len(vs),
        "median": pct(vs, 50),
        "p90": pct(vs, 90),
        "p95": pct(vs, 95),
        "p99": pct(vs, 99),
        "max": vs[-1],
        "min": vs[0],
    }


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


# --------------------------------------------------------------------------
# per-seed pipeline
# --------------------------------------------------------------------------

def run_seed(
    seed: str,
    params: Params,
    axis: str,
    timeout_s: float,
    warm_witnesses: list[list[int]],
    skip_lean: bool,
    lean_timeout_s: float,
) -> dict:
    rec: dict = {"seed": seed, "band": params.label(), "axis": axis}

    t0 = time.perf_counter()
    c = generate(seed, params)
    rec["gen_s"] = time.perf_counter() - t0
    rec["xof_draws"] = c.xof_draws
    rec["xof_rejections"] = c.xof_rejections
    d_bytes = d_dimacs_bytes(c)
    d_clauses = c.d_clauses()
    rec["cnf_clauses"] = len(d_clauses)
    rec["canonical_sha"] = sha(circuit_canonical_bytes(c))
    rec["d_sha"] = sha(d_bytes)

    # --- solver portfolio -------------------------------------------------
    res_cad = solvers.run_cadical(d_bytes, c.n_vars, timeout_s, want_lrat=True)
    res_kis = solvers.run_kissat(d_bytes, c.n_vars, timeout_s)
    res_z3 = solvers.run_z3(d_clauses, c.n_vars, timeout_s)
    rec["solvers"] = {
        r.solver: {"status": r.status, "wall_s": r.wall_s, "note": r.note}
        for r in (res_cad, res_kis, res_z3)
    }

    statuses = {r.status for r in (res_cad, res_kis, res_z3)} - {"UNKNOWN", "ERROR"}
    if statuses == {"SAT", "UNSAT"}:
        raise RuntimeError(f"solver disagreement on {seed}")
    outcome = None
    if "SAT" in statuses:
        outcome = "BUG"
    elif "UNSAT" in statuses:
        outcome = "SAFE"
    rec["outcome"] = outcome or "UNDECIDED"

    # --- structural attackers --------------------------------------------
    a_prop = attackers.attack_propagation(c)
    a_gauss = attackers.attack_gauss(c)
    a_free = attackers.attack_free_output(c)
    a_flip = attackers.attack_local_flip(c)
    a_warm = attackers.attack_warm_cache(c, warm_witnesses)
    attack_results = [a_prop, a_gauss, a_free, a_flip, a_warm]

    structural_times = []
    rec["attacks"] = {}
    for a in attack_results:
        entry = {"decided": a.decided, "wall_s": a.wall_s, "note": a.note}
        if a.decided == "BUG":
            v = verify_bug(seed, params, a.witness)
            entry["witness_verified"] = v.accepted
            if not v.accepted:
                raise RuntimeError(f"attacker {a.name} produced bad witness: {v.reason}")
        if a.decided is not None:
            if outcome is not None and a.decided != outcome:
                raise RuntimeError(
                    f"attacker {a.name} decided {a.decided} but solvers say {outcome}"
                )
            structural_times.append(a.wall_s)
        rec["attacks"][a.name] = entry

    # BCP-derived SAFE certificate (is the certificate free for a shortcut?)
    t0 = time.perf_counter()
    bcp_lrat = attackers.propagation_safe_lrat(c)
    bcp_lrat_s = time.perf_counter() - t0
    if bcp_lrat is not None:
        nat = check_lrat(c.n_vars, d_clauses, bcp_lrat)
        rec["bcp_lrat"] = {
            "wall_s": bcp_lrat_s,
            "bytes": len(bcp_lrat),
            "native_ok": nat.ok,
        }
        if not nat.ok:
            raise RuntimeError(f"BCP LRAT rejected by native checker: {nat.reason}")
    else:
        rec["bcp_lrat"] = None

    rec["structural_solve_s"] = min(structural_times) if structural_times else None
    solver_times = [
        r.wall_s for r in (res_cad, res_kis, res_z3) if r.status in ("SAT", "UNSAT")
    ]
    all_times = structural_times + solver_times
    rec["portfolio_solve_s"] = min(all_times) if all_times else None

    # --- certificate paths ------------------------------------------------
    if outcome == "BUG":
        model = next(
            r.model
            for r in sorted(
                (r for r in (res_cad, res_kis, res_z3) if r.status == "SAT"),
                key=lambda r: r.wall_s,
            )
        )
        v = verify_bug(seed, params, model)
        if not v.accepted:
            raise RuntimeError(f"solver SAT model rejected by BUG verifier: {v.reason}")
        wb = witness_bytes(model)
        bug_search_candidates = [
            r.wall_s for r in (res_cad, res_kis, res_z3) if r.status == "SAT"
        ] + [a.wall_s for a in attack_results if a.decided == "BUG"]
        rec["bug"] = {
            "search_s": min(bug_search_candidates),
            "verify_s": v.wall_s,
            "witness_bytes": len(wb),
        }
        canon = canon_binding(seed, "BUG", wb)
        flipped = bytearray(wb)
        flipped[0] ^= 1
        rec["canon"] = {
            "hash": canon,
            "flip_changes_hash": canon_binding(seed, "BUG", bytes(flipped)) != canon,
            "empty_cert_differs": canon_binding(seed, "BUG", b"") != canon,
        }
        return rec, model

    if outcome == "SAFE":
        if res_cad.status != "UNSAT" or res_cad.lrat_text is None:
            rec["safe"] = {
                "prove_s": res_cad.wall_s if res_cad.status == "UNSAT" else None,
                "note": res_cad.note or "no cadical LRAT (cadical undecided)",
            }
            return rec, None
        lrat_text = res_cad.lrat_text
        safe = {
            "prove_s": res_cad.wall_s,
            "lrat_bytes": len(lrat_text),
        }
        if len(lrat_text) <= 64 * 1024 * 1024:
            t0 = time.perf_counter()
            nat = check_lrat(c.n_vars, d_clauses, lrat_text)
            safe["native_check_s"] = time.perf_counter() - t0
            safe["native_ok"] = nat.ok
            safe["native_note"] = nat.reason
        else:
            nat = None
            safe["native_note"] = "skipped: proof exceeds native-checker size guard"
        if not skip_lean:
            with tempfile.TemporaryDirectory(prefix="zkdc-lean-") as td:
                cnf_p = os.path.join(td, "d.cnf")
                lrat_p = os.path.join(td, "d.lrat")
                with open(cnf_p, "wb") as f:
                    f.write(d_bytes)
                with open(lrat_p, "w") as f:
                    f.write(lrat_text)
                lr = lean_lrat.check(cnf_p, lrat_p, timeout_s=lean_timeout_s)
            safe["lean"] = {
                "status": lr.status,
                "wall_s": lr.wall_s,
                "check_s": lr.check_s,
                "peak_rss_bytes": lr.peak_rss_bytes,
                "proof_steps": lr.proof_steps,
                "note": lr.note,
            }
            if lr.status in ("ACCEPT", "REJECT") and nat is not None and nat.ok != lr.ok:
                raise RuntimeError(
                    f"native/Lean disagreement on {seed}: native={nat.ok} lean={lr.ok}"
                )
        rec["safe"] = safe
        canon = canon_binding(seed, "SAFE", lrat_text.encode())
        flipped = bytearray(lrat_text.encode())
        flipped[0] ^= 1
        rec["canon"] = {
            "hash": canon,
            "flip_changes_hash": canon_binding(seed, "SAFE", bytes(flipped)) != canon,
            "empty_cert_differs": canon_binding(seed, "SAFE", b"") != canon,
        }
        return rec, None

    return rec, None


# --------------------------------------------------------------------------
# S0 — determinism + totality
# --------------------------------------------------------------------------

def hash_instance(seed: str, params: Params) -> tuple[str, str]:
    c = generate(seed, params)
    return sha(circuit_canonical_bytes(c)), sha(d_dimacs_bytes(c))


def s0_determinism(params_list: list[Params]) -> dict:
    checks = []
    for params in params_list:
        seed = f"s0-{params.label()}"
        h1 = hash_instance(seed, params)
        h2 = hash_instance(seed, params)
        args = [
            sys.executable,
            os.path.abspath(__file__),
            "--hash-instance",
            seed,
            str(params.n_vars),
            str(params.n_pub),
            str(params.n_out),
            str(params.n_clause),
            str(params.n_xor),
            str(params.n_gate),
            str(params.clause_width),
        ]
        outs = []
        for _ in range(2):
            proc = subprocess.run(args, capture_output=True, text=True, timeout=300)
            if proc.returncode != 0:
                raise RuntimeError(f"hash-instance subprocess failed: {proc.stderr}")
            outs.append(proc.stdout.strip())
        ok = (
            h1 == h2
            and outs[0] == outs[1]
            and outs[0] == f"{h1[0]} {h1[1]}"
        )
        checks.append({"band": params.label(), "byte_identical": ok})
    return {
        "checks": checks,
        "pass": all(ch["byte_identical"] for ch in checks),
    }


def s0_totality(n_seeds: int, timeout_s: float, skip_lean: bool) -> dict:
    """Exhaustive ground truth on TINY instances: exactly one of BUG/SAFE
    holds, our certificate paths agree with brute force, and the R1CS-free
    Boolean semantics match the CNF encoding."""
    results = []
    for i in range(n_seeds):
        seed = f"s0-tot-{i:04d}"
        c = generate(seed, TINY)
        d_clauses = c.d_clauses()
        n = c.n_vars
        alt = None
        n_solutions = 0
        for bits in range(1 << n):
            w = [0] + [(bits >> j) & 1 for j in range(n)]
            if all(
                any((w[abs(l)] == 1) == (l > 0) for l in cl) for cl in d_clauses
            ):
                n_solutions += 1
                if alt is None:
                    alt = w
        truth = "BUG" if n_solutions > 0 else "SAFE"

        # certificate paths must agree with brute force
        detail = {"seed": seed, "truth": truth, "d_solutions": n_solutions}
        if truth == "BUG":
            v = verify_bug(seed, TINY, alt)
            detail["bug_verifier_accepts_brute_witness"] = v.accepted
            # reference witness itself must NOT be accepted (same output)
            vref = verify_bug(seed, TINY, list(c.ref_witness))
            detail["bug_verifier_rejects_reference"] = not vref.accepted
            ok = v.accepted and not vref.accepted
        else:
            res = solvers.run_cadical(
                d_dimacs_bytes(c), n, timeout_s, want_lrat=True
            )
            nat_ok = lean_ok = None
            if res.status == "UNSAT" and res.lrat_text:
                nat = check_lrat(n, d_clauses, res.lrat_text)
                nat_ok = nat.ok
                if not skip_lean:
                    with tempfile.TemporaryDirectory(prefix="zkdc-s0-") as td:
                        cnf_p = os.path.join(td, "d.cnf")
                        lrat_p = os.path.join(td, "d.lrat")
                        with open(cnf_p, "wb") as f:
                            f.write(d_dimacs_bytes(c))
                        with open(lrat_p, "w") as f:
                            f.write(res.lrat_text)
                        lr = lean_lrat.check(cnf_p, lrat_p)
                    lean_ok = lr.ok
            detail["cadical_status"] = res.status
            detail["native_lrat_ok"] = nat_ok
            detail["lean_lrat_ok"] = lean_ok
            ok = res.status == "UNSAT" and nat_ok is True and (
                skip_lean or lean_ok is True
            )
        # cross-check: solver decision matches brute force
        dec = solvers.run_cadical(d_dimacs_bytes(c), n, timeout_s, want_lrat=False)
        detail["solver_agrees_with_brute_force"] = (
            dec.status == ("SAT" if truth == "BUG" else "UNSAT")
        )
        detail["ok"] = bool(ok and detail["solver_agrees_with_brute_force"])
        results.append(detail)
    n_bug = sum(1 for r in results if r["truth"] == "BUG")
    return {
        "band": TINY.label(),
        "seeds": n_seeds,
        "bug_count": n_bug,
        "safe_count": n_seeds - n_bug,
        "details": results,
        "pass": all(r["ok"] for r in results),
    }


# --------------------------------------------------------------------------
# gate aggregation
# --------------------------------------------------------------------------

def aggregate_band(records: list[dict]) -> dict:
    outcomes = [r["outcome"] for r in records]
    portfolio = [r["portfolio_solve_s"] for r in records if r["portfolio_solve_s"] is not None]
    structural = [r["structural_solve_s"] for r in records if r["structural_solve_s"] is not None]
    struct_decided = sum(1 for r in records if r["structural_solve_s"] is not None)
    bcp_free_cert = sum(1 for r in records if r.get("bcp_lrat"))
    agg = {
        "seeds": len(records),
        "outcomes": {
            o: outcomes.count(o) for o in ("BUG", "SAFE", "UNDECIDED")
        },
        "portfolio_solve_s": summary(portfolio),
        "structural_solve_s": summary(structural),
        "structural_decided_frac": struct_decided / len(records) if records else None,
        "bcp_free_safe_cert_frac": bcp_free_cert / len(records) if records else None,
        "undecided_frac": outcomes.count("UNDECIDED") / len(records) if records else None,
        "bug_search_s": summary([r["bug"]["search_s"] for r in records if "bug" in r]),
        "bug_verify_s": summary([r["bug"]["verify_s"] for r in records if "bug" in r]),
        "witness_bytes": summary([r["bug"]["witness_bytes"] for r in records if "bug" in r]),
        "safe_prove_s": summary(
            [r["safe"]["prove_s"] for r in records if "safe" in r and r["safe"].get("prove_s") is not None]
        ),
        "lrat_bytes": summary(
            [r["safe"]["lrat_bytes"] for r in records if "safe" in r and "lrat_bytes" in r["safe"]]
        ),
        "native_check_s": summary(
            [r["safe"]["native_check_s"] for r in records if "safe" in r and "native_check_s" in r["safe"]]
        ),
        "lean_wall_s": summary(
            [r["safe"]["lean"]["wall_s"] for r in records if "safe" in r and "lean" in r["safe"]]
        ),
        "lean_check_s": summary(
            [
                r["safe"]["lean"]["check_s"]
                for r in records
                if "safe" in r and "lean" in r["safe"] and r["safe"]["lean"]["check_s"] is not None
            ]
        ),
        "lean_peak_rss_bytes": summary(
            [
                r["safe"]["lean"]["peak_rss_bytes"]
                for r in records
                if "safe" in r and "lean" in r["safe"] and r["safe"]["lean"]["peak_rss_bytes"] is not None
            ]
        ),
    }
    return agg


def branch_predictor(records: list[dict]) -> dict:
    """Can generator branch counts alone predict BUG/SAFE? (S1 leak check)"""
    labelled = [
        (r["xof_draws"], r["xof_rejections"], r["outcome"])
        for r in records
        if r["outcome"] in ("BUG", "SAFE")
    ]
    if len(labelled) < 4:
        return {"n": len(labelled), "note": "too few decided seeds"}
    outcomes = [l[2] for l in labelled]
    base = max(outcomes.count("BUG"), outcomes.count("SAFE")) / len(outcomes)
    best = base
    best_feat = "majority"
    for feat_idx, feat_name in ((0, "xof_draws"), (1, "xof_rejections")):
        vals = sorted({l[feat_idx] for l in labelled})
        for thr in vals:
            for direction in (True, False):
                acc = sum(
                    1
                    for l in labelled
                    if (l[feat_idx] >= thr) == direction
                    and l[2] == "BUG"
                    or (l[feat_idx] >= thr) != direction
                    and l[2] == "SAFE"
                ) / len(labelled)
                if acc > best:
                    best = acc
                    best_feat = f"{feat_name}>={thr}" + ("" if direction else " (inv)")
    return {
        "n": len(labelled),
        "majority_baseline": base,
        "best_threshold_accuracy": best,
        "best_feature": best_feat,
    }


def s5_cherry_pick(records: list[dict], band: str) -> dict:
    """Bootstrap min-of-N from measured per-seed portfolio solve times."""
    times = [r["portfolio_solve_s"] for r in records]
    outcomes = [r["outcome"] for r in records]
    usable = [(t if t is not None else float("inf"), o) for t, o in zip(times, outcomes)]
    if not usable:
        return {"band": band, "note": "no data"}
    rng = Xof(f"s5|{band}", "bootstrap")
    B = 2000
    out = {"band": band, "seeds": len(usable)}
    for N in (1, 10, 100, 1000):
        mins = []
        argmin_bug = 0
        for _ in range(B):
            best_t, best_o = float("inf"), None
            for _ in range(N):
                t, o = usable[rng.randbelow(len(usable))]
                if t < best_t:
                    best_t, best_o = t, o
            if best_t == float("inf"):
                continue
            mins.append(best_t)
            if best_o == "BUG":
                argmin_bug += 1
        out[f"min_of_{N}"] = summary(mins)
        out[f"min_of_{N}_bug_frac"] = argmin_bug / len(mins) if mins else None
    m1 = out.get("min_of_1", {}).get("median")
    m1000 = out.get("min_of_1000", {}).get("median")
    out["grinding_gain_1000"] = (m1 / m1000) if m1 and m1000 and m1000 > 0 else None
    return out


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

def machine_info() -> dict:
    info = {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": sys.version.split()[0],
        "cpu_count": os.cpu_count(),
    }
    try:
        info["mem_bytes"] = int(
            subprocess.run(
                ["sysctl", "-n", "hw.memsize"], capture_output=True, text=True
            ).stdout.strip()
        )
    except (OSError, ValueError):
        info["mem_bytes"] = None
    return info


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--mode", choices=["quick", "full"], default="quick")
    ap.add_argument("--seeds", type=int, default=None, help="seeds per band")
    ap.add_argument("--timeout", type=float, default=None, help="solver timeout (s)")
    ap.add_argument("--lean-timeout", type=float, default=600.0)
    ap.add_argument("--out", type=str, default=None, help="output JSON path")
    ap.add_argument(
        "--write-sample",
        action="store_true",
        help="ALSO overwrite the committed result.sample.json (explicit opt-in)",
    )
    ap.add_argument("--skip-lean", action="store_true")
    ap.add_argument("--totality-seeds", type=int, default=None)
    ap.add_argument("--hash-instance", nargs=8, metavar="X", default=None)
    args = ap.parse_args()

    if args.hash_instance:
        seed, n, pub, out, ncl, nxor, ngate, k = args.hash_instance
        p = Params(int(n), int(pub), int(out), int(ncl), int(nxor), int(ngate), int(k))
        h = hash_instance(seed, p)
        print(f"{h[0]} {h[1]}")
        return 0

    seeds_per_band = args.seeds or (10 if args.mode == "quick" else 30)
    timeout_s = args.timeout or (30.0 if args.mode == "quick" else 120.0)
    totality_seeds = args.totality_seeds or (30 if args.mode == "quick" else 80)

    t_start = time.perf_counter()
    result: dict = {
        "candidate": CANDIDATE,
        "phase": "P0-A (boolean reduced model)",
        "mode": args.mode,
        "machine": machine_info(),
        "tools": solvers.tool_versions(),
        "config": {
            "seeds_per_band": seeds_per_band,
            "solver_timeout_s": timeout_s,
            "lean_timeout_s": args.lean_timeout,
            "skip_lean": args.skip_lean,
            "totality_seeds": totality_seeds,
        },
    }
    if not args.skip_lean:
        result["tools"]["lean-toolchain"] = lean_lrat.ensure_built()

    print("[s0] determinism...", file=sys.stderr, flush=True)
    result["S0_determinism"] = s0_determinism([TINY, _mk(**BASE)])
    print("[s0] totality (exhaustive)...", file=sys.stderr, flush=True)
    result["S0_totality"] = s0_totality(totality_seeds, timeout_s, args.skip_lean)

    axes = band_axes(args.mode)
    per_seed: list[dict] = []
    band_aggs: dict[str, dict] = {}
    axis_tables: dict[str, list[dict]] = {}

    for axis, plist in axes.items():
        axis_tables[axis] = []
        for params in plist:
            band = params.label()
            if band in band_aggs:
                axis_tables[axis].append({"band": band, "duplicate": True})
                continue
            print(f"[bands] {axis}/{band}", file=sys.stderr, flush=True)
            records = []
            warm: list[list[int]] = []
            for i in range(seeds_per_band):
                seed = f"dc-{band}-{i:04d}"
                rec, bug_witness = run_seed(
                    seed,
                    params,
                    axis,
                    timeout_s,
                    warm,
                    args.skip_lean,
                    args.lean_timeout,
                )
                if bug_witness is not None:
                    warm.append(bug_witness)
                records.append(rec)
                per_seed.append(rec)
            agg = aggregate_band(records)
            agg["branch_predictor"] = branch_predictor(records)
            agg["s5"] = s5_cherry_pick(records, band)
            band_aggs[band] = agg
            axis_tables[axis].append({"band": band, **agg})

    result["bands"] = band_aggs
    result["axes"] = axis_tables
    result["per_seed"] = per_seed

    # ---- gate verdicts ---------------------------------------------------
    gates: dict = {}

    # S1 — shortcut resistance (portfolio = structural + solvers)
    s1_rows = []
    for band, agg in band_aggs.items():
        med = agg["portfolio_solve_s"].get("median")
        smed = agg["structural_solve_s"].get("median")
        s1_rows.append(
            {
                "band": band,
                "portfolio_median_s": med,
                "structural_median_s": smed,
                "structural_decided_frac": agg["structural_decided_frac"],
                "bcp_free_safe_cert_frac": agg["bcp_free_safe_cert_frac"],
                "sub_1s_portfolio": med is not None and med < 1.0,
            }
        )
    all_sub_1s = all(r["sub_1s_portfolio"] for r in s1_rows if r["portfolio_median_s"] is not None)
    gates["S1_shortcut_resistance"] = {
        "rows": s1_rows,
        "all_bands_portfolio_below_1s": all_sub_1s,
        "verdict": "FAIL (all bands < 1s for the portfolio attacker)" if all_sub_1s else "see rows",
    }

    # S2 — asymmetry per path
    bug_search = [r["bug"]["search_s"] for r in per_seed if "bug" in r]
    bug_verify = [r["bug"]["verify_s"] for r in per_seed if "bug" in r]
    safe_prove = [
        r["safe"]["prove_s"] for r in per_seed if "safe" in r and r["safe"].get("prove_s")
    ]
    lean_wall = [
        r["safe"]["lean"]["wall_s"] for r in per_seed if "safe" in r and "lean" in r["safe"]
    ]
    lean_check = [
        r["safe"]["lean"]["check_s"]
        for r in per_seed
        if "safe" in r and "lean" in r["safe"] and r["safe"]["lean"]["check_s"] is not None
    ]

    def _ratio(a, b):
        sa, sb = summary(a), summary(b)
        if sa.get("median") and sb.get("median") and sb["median"] > 0:
            return sa["median"] / sb["median"]
        return None

    gates["S2_asymmetry"] = {
        "bug_search_over_verify": _ratio(bug_search, bug_verify),
        "safe_prove_over_lean_wall": _ratio(safe_prove, lean_wall),
        "safe_prove_over_lean_check_phase": _ratio(safe_prove, lean_check),
        "target": ">= 100x on each certified path",
        "note": "gate uses whole-process lean wall (conservative); check-phase shown too",
    }

    # S3 — per-axis difficulty control (tables already in axes)
    s3 = {}
    for axis, rows in axis_tables.items():
        meds = [
            r["portfolio_solve_s"].get("median")
            for r in rows
            if "portfolio_solve_s" in r and r["portfolio_solve_s"].get("median") is not None
        ]
        mono = all(a <= b for a, b in zip(meds, meds[1:])) or all(
            a >= b for a, b in zip(meds, meds[1:])
        )
        s3[axis] = {"medians": meds, "monotone": mono if len(meds) >= 2 else None}
    gates["S3_difficulty_axes"] = s3

    # S4 — outcome balance / liveness
    gates["S4_outcome_balance"] = {
        band: agg["outcomes"] for band, agg in band_aggs.items()
    }

    # S5 — cherry picking (per band, already computed)
    gates["S5_cherry_picking"] = {band: agg["s5"] for band, agg in band_aggs.items()}

    # S6 — certificate size / consensus cost
    gates["S6_certificate_cost"] = {
        band: {
            "witness_bytes": agg["witness_bytes"],
            "lrat_bytes": agg["lrat_bytes"],
            "lean_wall_s": agg["lean_wall_s"],
            "lean_peak_rss_bytes": agg["lean_peak_rss_bytes"],
        }
        for band, agg in band_aggs.items()
    }

    # S7 — answer binding prototype
    canon_checks = [r["canon"] for r in per_seed if "canon" in r]
    gates["S7_answer_binding"] = {
        "checked": len(canon_checks),
        "flip_changes_hash_all": all(cc["flip_changes_hash"] for cc in canon_checks),
        "empty_cert_differs_all": all(cc["empty_cert_differs"] for cc in canon_checks),
        "note": (
            "canon = blake2b(len-prefixed seed|outcome_tag|certificate). The seed"
            " alone cannot reproduce canon for either path; identity binding of"
            " certificates (anti-theft) needs ticket/miner signature — recorded"
            " as a report finding, not prototyped here."
        ),
    }

    result["gates"] = gates
    result["total_wall_s"] = time.perf_counter() - t_start

    out_path = args.out
    if out_path is None:
        fd, out_path = tempfile.mkstemp(prefix="zk-dualcert-p0a-", suffix=".json")
        os.close(fd)
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2, sort_keys=False)
        f.write("\n")
    print(f"[done] results -> {out_path}", file=sys.stderr)

    if args.write_sample:
        slim = dict(result)
        slim["per_seed"] = f"omitted in sample ({len(per_seed)} records; full JSON via --out)"
        with open(SAMPLE_PATH, "w") as f:
            json.dump(slim, f, indent=2, sort_keys=False)
            f.write("\n")
        print(f"[done] sample -> {SAMPLE_PATH}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
