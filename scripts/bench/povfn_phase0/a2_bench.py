"""PoVFN Phase 0-A / A2-A3 — zkVM measurement matrix driver.

THROWAWAY offline experiment. Runs execute/prove/prove-succinct per band,
the fail-closed tamper probe, and the sequential batch (k_max) measurement.
Writes one JSON. Long-running; intended for nohup execution.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
Z = os.path.join(HERE, "zkguest")
W = os.path.join(HERE, "work")
HOST = os.path.join(Z, "target", "release", "host")
CLI = os.path.join(Z, "target", "release", "stmt_hash_cli")
THM = "BooleVerifyMod.instance_thm"

BANDS = [
    ("Real", "checker-export/ProofReal.ndjson", "real-fixture-seed"),
    ("Syn0", "checker-export/ProofSyn0.ndjson", "synthetic-seed"),
    ("Syn1", "checker-export/ProofSyn1.ndjson", "synthetic-seed-large-closure"),
]


def run(cmd, timeout=7200, env=None):
    t0 = time.perf_counter()
    proc = subprocess.run(
        cmd, capture_output=True, text=True, timeout=timeout, env=env, cwd=Z
    )
    return proc, time.perf_counter() - t0


def time_l(cmd, timeout=7200):
    proc, dt = run(["/usr/bin/time", "-l", *cmd], timeout=timeout)
    peak = None
    m = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
    if m:
        peak = int(m.group(1))
    return proc, dt, peak


def expected_hash(ndjson):
    proc, _ = run([CLI, ndjson, THM])
    assert proc.returncode == 0, proc.stderr
    return proc.stdout.strip()


def parse_json_stdout(proc):
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError:
        return {"error": proc.stdout[-500:] + proc.stderr[-500:]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--skip-syn1-prove", action="store_true")
    ap.add_argument("--batch-n", type=int, default=4)  # k_max
    args = ap.parse_args()

    binding = os.path.join(W, "binding-real.json")
    result = {"experiment": "povfn-phase0-a2a3", "bands": {}, "batch": {}, "tamper": {}}

    for tag, rel, label in BANDS:
        ndjson = os.path.join(W, rel)
        if not os.path.exists(ndjson):
            result["bands"][tag] = {"error": "export missing"}
            continue
        exp = expected_hash(ndjson)
        band = {"label": label, "export_bytes": os.path.getsize(ndjson)}
        print(f"[a2] {tag} execute", file=sys.stderr, flush=True)
        proc, dt = run([HOST, ndjson, binding, "execute", exp])
        band["execute"] = parse_json_stdout(proc)
        print(f"[a2] {tag} prove composite", file=sys.stderr, flush=True)
        if tag == "Syn1" and args.skip_syn1_prove:
            band["prove"] = {"skipped": "timebox; cycles from execute only"}
        else:
            proc, dt, peak = time_l([HOST, ndjson, binding, "prove", exp])
            band["prove"] = parse_json_stdout(proc)
            band["prove"]["prover_peak_rss"] = peak
            band["prove"]["wall_total_s"] = dt
        print(f"[a2] {tag} prove succinct", file=sys.stderr, flush=True)
        if tag in ("Syn1",):
            band["prove_succinct"] = {"skipped": "timebox; composite covers the point"}
        else:
            proc, dt, peak = time_l([HOST, ndjson, binding, "prove-succinct", exp])
            band["prove_succinct"] = parse_json_stdout(proc)
            band["prove_succinct"]["prover_peak_rss"] = peak
            band["prove_succinct"]["wall_total_s"] = dt
        result["bands"][tag] = band

    # fail-closed tamper probe: statement-literal tampered export must yield
    # NO proof (guest panic -> prover error)
    print("[a2] tamper fail-closed", file=sys.stderr, flush=True)
    tampered = os.path.join(W, "checker-export", "tamper-stmt-literal.ndjson")
    if os.path.exists(tampered):
        exp = expected_hash(os.path.join(W, "checker-export/ProofReal.ndjson"))
        proc, dt = run([HOST, tampered, binding, "execute", exp])
        # kernel-valid but different statement: journal stmt_hash must differ
        j = parse_json_stdout(proc)
        result["tamper"]["stmt-literal"] = {
            "guest_ran": proc.returncode == 0,
            "stmt_hash_matches_expected": j.get("stmt_hash_matches_expected"),
            "pass": proc.returncode == 0
            and j.get("stmt_hash_matches_expected") is False,
        }
    corrupted = os.path.join(W, "checker-export", "tamper-proof-node.ndjson")
    if os.path.exists(corrupted):
        proc, dt = run([HOST, corrupted, binding, "execute"])
        result["tamper"]["proof-node"] = {
            "guest_rejected": proc.returncode != 0,
            "pass": proc.returncode != 0,
        }

    # batch: sequential k_max proves of the Real band (block-worth of work)
    print(f"[a3] batch x{args.batch_n}", file=sys.stderr, flush=True)
    ndjson = os.path.join(W, "checker-export/ProofReal.ndjson")
    exp = expected_hash(ndjson)
    times = []
    for i in range(args.batch_n):
        proc, dt = run([HOST, ndjson, binding, "prove", exp])
        j = parse_json_stdout(proc)
        times.append(j.get("prove_s"))
    result["batch"] = {
        "n": args.batch_n,
        "per_prove_s": times,
        "total_s": sum(t for t in times if t),
        "note": "sequential; independent proofs parallelize across cores/machines",
    }

    with open(args.out, "w") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(f"[done] -> {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
