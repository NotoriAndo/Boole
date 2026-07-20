"""Run the cheap+lethal gates first (prereg kill order): S0, S1, S2.

THROWAWAY offline experiment. If S1 (leakage) or S2 (automation shortcut)
fires, we stop and report a candidate-limited NO-GO — that is the whole point
of the kill order.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time

from generator import generate, BANDS
from oracle import evaluate
from automation import auto_solve, structural_predict


def seeds_for(prefix: str, per_band: int):
    out = {}
    for band in BANDS:
        out[band] = [f"{prefix}-{band}-{i:04d}" for i in range(per_band)]
    return out


def s0_determinism(seed: str, band: str) -> bool:
    a = generate(seed, band)
    b = generate(seed, band)
    # cross-process determinism
    proc = subprocess.run(
        [sys.executable, "-c",
         f"from generator import generate; p=generate({seed!r},{band!r}); print(p.canonical_bytes_sha); print(p.statement)"],
        capture_output=True, text=True, cwd=".",
    )
    lines = proc.stdout.strip().splitlines()
    return (a.canonical_bytes_sha == b.canonical_bytes_sha
            and a.statement == b.statement
            and lines and lines[0] == a.canonical_bytes_sha
            and lines[1] == a.statement)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--prefix", default="pilot")
    ap.add_argument("--per-band", type=int, default=10)
    ap.add_argument("--out", required=True)
    ap.add_argument("--timeout", type=float, default=20.0)
    args = ap.parse_args()

    result = {"experiment": "zk-por-s0-s2", "prefix": args.prefix,
              "per_band": args.per_band, "bands": list(BANDS), "problems": []}

    seeds = seeds_for(args.prefix, args.per_band)

    # ---- S0 determinism + totality (oracle label well-defined) --------------
    s0_ok = True
    for band, sl in seeds.items():
        for seed in sl[:3]:
            if not s0_determinism(seed, band):
                s0_ok = False
    result["S0"] = {"determinism_pass": s0_ok}

    # ---- generate all + oracle labels + auto + structural -------------------
    labels = []
    struct_correct = 0
    struct_total = 0
    auto_solved = 0
    auto_total = 0
    per_band_auto = {b: {"solved": 0, "total": 0} for b in BANDS}
    for band, sl in seeds.items():
        for seed in sl:
            p = generate(seed, band)
            truth = evaluate(p.oracle_spec)
            labels.append(truth["label"])
            # S1: structural attacker predicts label from mutation only
            pred = structural_predict(p)
            struct_total += 1
            if pred == truth["label"]:
                struct_correct += 1
            # S2: automation
            a = auto_solve(p, p.oracle_spec, timeout=args.timeout)
            auto_total += 1
            per_band_auto[band]["total"] += 1
            if a["solved"]:
                auto_solved += 1
                per_band_auto[band]["solved"] += 1
            result["problems"].append({
                "seed": seed, "band": band, "base": p.base, "mutation": p.mutation,
                "statement": p.statement, "label": truth["label"],
                "vacuous_true": truth["vacuous_true"],
                "auto_solved": a["solved"], "auto_polarity": a["polarity"],
                "auto_wall_s": a["wall_s"],
                "label_discovery_only": a["label_discovery_only"],
            })

    n = len(labels)
    n_true = labels.count("TRUE")
    n_false = labels.count("FALSE")
    struct_acc = struct_correct / struct_total if struct_total else 0
    majority = max(n_true, n_false) / n if n else 0
    auto_rate = auto_solved / auto_total if auto_total else 0

    result["label_balance"] = {"true": n_true, "false": n_false, "n": n,
                               "true_frac": n_true / n, "false_frac": n_false / n}
    result["S1_leakage"] = {
        "structural_label_accuracy": struct_acc,
        "majority_baseline": majority,
        "verdict": ("NO-GO" if struct_acc >= 0.90 else "see-report"),
        "note": "mutation-only label prediction; >=0.90 treated as easy predictability",
    }
    result["S2_automation"] = {
        "auto_solve_rate": auto_rate,
        "per_band": {b: (per_band_auto[b]["solved"] / per_band_auto[b]["total"]
                         if per_band_auto[b]["total"] else None) for b in BANDS},
        "verdict": ("NO-GO" if auto_rate >= 0.50 else "PASS"),
    }

    with open(args.out, "w") as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"[s0-s2] S0={s0_ok} S1_struct_acc={struct_acc:.2f} "
          f"balance(T/F)={n_true}/{n_false} S2_auto={auto_rate:.2f} -> {args.out}",
          file=sys.stderr)


if __name__ == "__main__":
    main()
