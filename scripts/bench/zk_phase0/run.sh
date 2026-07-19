#!/usr/bin/env bash
# ZK.0 offline feasibility spike -- one-command reproduction (L1 master §ZK).
#
# THROWAWAY research harness. Never wired into self-test / consensus / any
# production path. Requires only python3 + z3-solver (pip install --user z3-solver).
# No paid API, no network at run time, no model calls (dev constitution §7).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

echo "== ZK.0 dependency check =="
python3 -c "import z3; print('z3', z3.get_version_string())" || {
  echo "missing z3-solver: run 'pip3 install --user z3-solver'" >&2; exit 1; }

echo
echo "== determinism self-check (same seed -> byte-identical problem) =="
python3 - <<'PY'
import circuit as C
from zkfield import MERSENNE_PRIMES
gp = C.GenParams(p=MERSENNE_PRIMES[31], width=8, depth=10, mutations=3,
                 mutation_mode="internal")
a = C.generate(999, gp); b = C.generate(999, gp)
ok = a.seed_hex == b.seed_hex and a.honest_witness == b.honest_witness \
    and len(a.constraints) == len(b.constraints)
print("deterministic:", ok)
assert ok
PY

echo
echo "== S1/S2/S3/S5 gate experiment (writes result.sample.json) =="
python3 -u experiment.py > result.sample.json
python3 - result.sample.json <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print("OVERALL:", d["overall"])
for k, v in d["gates"].items():
    print(f"  {k}: {v['verdict']}")
PY

echo
echo "== salvage probe (checkpoint-inversion: Z3 'hard' vs structural O(1)) =="
python3 -u salvage_probe.py

echo
echo "done."
