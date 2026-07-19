#!/usr/bin/env bash
# One-command reproduction for the dual-cert Phase 0 harness (P0-A).
#
# THROWAWAY offline experiment — never wired into self-test/consensus.
# Local tools only: python3, cadical, kissat (brew), z3-solver (pip),
# pinned Lean v4.29.1 via elan (same toolchain as lean/checker; separate
# project, no pinned consensus file is touched).
#
# Results go to a TEMP file by default; the committed result.sample.json is
# only refreshed by the explicit `./run.sh --write-sample`.
set -euo pipefail
cd "$(dirname "$0")"

for tool in cadical kissat python3; do
  command -v "$tool" >/dev/null || {
    echo "missing tool: $tool (brew install cadical kissat)" >&2
    exit 1
  }
done
python3 -c "import z3" 2>/dev/null || {
  echo "missing python module z3 (pip3 install --user z3-solver)" >&2
  exit 1
}

echo "== self-check (unit tests) =="
python3 -m unittest test_selfcheck

echo "== S0-S7 experiment (quick mode) =="
OUT="$(mktemp -t zk-dualcert-p0a.json)"
if [[ "${1:-}" == "--write-sample" ]]; then
  python3 -u experiment.py --mode quick --out "$OUT" --write-sample
else
  python3 -u experiment.py --mode quick --out "$OUT"
fi
echo "results: $OUT"
