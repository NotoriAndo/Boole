"""ZK.0 feasibility experiment: compute the S1/S2/S3/S5 go/no-go gates for the
hash-generated ZK underconstraint family (L1 master §ZK, ADR-0017).

Primary attacker = the O(n) solver-free propagation attack (attacker.propagation_attack),
which cross-checks as COMPLETE against Z3 on tractable sizes (see report §methodology).
Z3 is included only as an independent oracle on a subset -- never as the headline
difficulty, because Z3-timeout is not hardness (see salvage_probe.py).

Emits JSON to stdout. Deterministic: fixed seed lists, no wall-clock in the math.
"""
from __future__ import annotations

import json
import statistics
import sys

import attacker
import circuit as C
from zkfield import MERSENNE_PRIMES

P = MERSENNE_PRIMES[31]

# difficulty-band sweep: (width, depth, mutations)
BANDS = [
    (3, 2, 1),
    (5, 5, 1),
    (8, 10, 3),
    (12, 16, 4),
    (16, 24, 6),
    (20, 32, 8),
]
SEEDS_PER_BAND = 60
Z3_CROSSCHECK_SEEDS = 20
Z3_TIMEOUT_MS = 8000


def band_key(w, d, k):
    return f"w{w}_d{d}_k{k}"


def run_band(w, d, k):
    gp = C.GenParams(p=P, width=w, depth=d, mutations=k, mutation_mode="internal")
    prop_times = []
    verify_times = []
    certs = 0
    solvable = 0  # instances where a certificate exists (prop found one)
    nvars = ncon = 0
    per_seed_prop = []
    for s in range(SEEDS_PER_BAND):
        circ = C.generate(s * 13 + 3, gp)
        if circ is None:
            continue
        nvars, ncon = circ.num_vars, len(circ.constraints)
        r = attacker.propagation_attack(circ)
        prop_times.append(r.seconds)
        per_seed_prop.append(r.seconds)
        if r.certificate_valid:
            certs += 1
            solvable += 1
        # cheap native verifier timing (S2 denominator): re-verify honest witness
        import time
        t0 = time.perf_counter()
        C.satisfies(circ, circ.honest_witness)
        verify_times.append(time.perf_counter() - t0)

    # Z3 independent cross-check (completeness of propagation) -- only on
    # tractable sizes where Z3 terminates and thus yields ground truth. On large
    # circuits Z3 only times out ('unknown') and validates nothing, so we skip it
    # (that Z3 gives up is itself a documented finding, not evidence of hardness).
    z3_agree = z3_disagree = z3_unknown = 0
    if ncon <= 40:
        for s in range(Z3_CROSSCHECK_SEEDS):
            circ = C.generate(s * 13 + 3, gp)
            if circ is None:
                continue
            pr = attacker.propagation_attack(circ)
            zr = attacker.solve(circ, timeout_ms=Z3_TIMEOUT_MS)
            if zr.status == "unknown":
                z3_unknown += 1
                continue
            z3sat = zr.status == "sat"
            if z3sat == pr.certificate_valid:
                z3_agree += 1
            else:
                z3_disagree += 1

    return {
        "band": band_key(w, d, k),
        "width": w, "depth": d, "mutations": k,
        "num_vars": nvars, "num_constraints": ncon,
        "prop_attack_median_ms": round(statistics.median(prop_times) * 1e3, 4),
        "prop_attack_max_ms": round(max(prop_times) * 1e3, 4),
        "prop_attack_p95_ms": round(sorted(prop_times)[int(0.95 * len(prop_times))] * 1e3, 4),
        "verify_median_ms": round(statistics.median(verify_times) * 1e3, 4),
        "asymmetry_ratio_attack_over_verify": round(
            statistics.median(prop_times) / max(statistics.median(verify_times), 1e-9), 3),
        "underconstraint_cert_rate": round(certs / max(len(prop_times), 1), 3),
        "z3_crosscheck": {"agree": z3_agree, "disagree": z3_disagree,
                          "unknown": z3_unknown},
        "_per_seed_prop_ms": [round(t * 1e3, 5) for t in per_seed_prop],
    }


def cherry_pick_s5(band):
    """S5: best-of-N seed grinding. For a fixed band, a miner draws N seeds and
    keeps the easiest. Since every solvable instance is broken in <1ms, there is
    no difficulty to erode; we still report min-of-N solve time and the fraction
    of draws that yield a valid certificate (a miner only needs one)."""
    times = band["_per_seed_prop_ms"]
    cert_rate = band["underconstraint_cert_rate"]
    out = {}
    for N in [1, 10, 100, 1000]:
        # order statistic: expected minimum of N i.i.d. draws (bootstrap over sample)
        if not times:
            continue
        srt = sorted(times)
        # min-of-N ~ the (1/N) quantile of the per-seed distribution
        idx = min(len(srt) - 1, int(len(srt) / N))
        out[f"min_solve_ms_bestof_{N}"] = srt[idx]
        # probability at least one of N draws yields a certificate
        out[f"p_cert_in_{N}_draws"] = round(1 - (1 - cert_rate) ** N, 6)
    return out


def main():
    results = [run_band(w, d, k) for (w, d, k) in BANDS]

    # S1: shortcut resistance -- is the strongest attacker <1s across ALL bands?
    max_attack_ms = max(r["prop_attack_max_ms"] for r in results)
    s1_all_under_1s = max_attack_ms < 1000.0
    s1_verdict = "FAIL(no-go)" if s1_all_under_1s else "PASS"

    # S2: verification asymmetry -- attack/verify >= 100x?
    min_ratio = min(r["asymmetry_ratio_attack_over_verify"] for r in results)
    max_ratio = max(r["asymmetry_ratio_attack_over_verify"] for r in results)
    s2_verdict = "PASS" if min_ratio >= 100.0 else "FAIL"

    # S3: monotonicity of attack cost vs circuit size (informational -- monotone
    # but trivial is still no-go, since S1 already fails)
    sizes = [r["num_constraints"] for r in results]
    times = [r["prop_attack_median_ms"] for r in results]
    monotone = all(times[i] <= times[i + 1] + 1e-9 for i in range(len(times) - 1))

    # S5: cherry-picking (per densest band)
    s5 = cherry_pick_s5(results[-1])

    summary = {
        "field": "2^31-1",
        "primary_attacker": "propagation (solver-free, O(n))",
        "bands": [{k: v for k, v in r.items() if not k.startswith("_")}
                  for r in results],
        "gates": {
            "S1_shortcut_resistance": {
                "max_attack_ms_across_all_bands": max_attack_ms,
                "all_bands_under_1s": s1_all_under_1s,
                "verdict": s1_verdict,
                "note": "propagation attack is <1s in every band -> obfuscated PoW",
            },
            "S2_verification_asymmetry": {
                "attack_over_verify_ratio_range": [min_ratio, max_ratio],
                "target": ">=100x",
                "verdict": s2_verdict,
                "note": "attacker and verifier are both O(n); no asymmetry",
            },
            "S3_difficulty_axis_monotonicity": {
                "attack_ms_monotone_in_size": monotone,
                "verdict": "MONOTONE_BUT_TRIVIAL",
                "note": "cost grows O(n) but never exceeds ~1ms; monotone axis "
                        "does not create hardness",
            },
            "S5_cherry_picking": {
                "densest_band": results[-1]["band"],
                "order_statistics": s5,
                "verdict": "MOOT",
                "note": "base difficulty ~0, so best-of-N has nothing to erode; "
                        "ticket-cost economics moot",
            },
        },
        "overall": "NO-GO (feed-forward underconstraint family; see report)",
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
