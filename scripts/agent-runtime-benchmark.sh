#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEFAULT_SPEC='[
  {"name":"hermes-agent-cli-mock-verify","kind":"agent-runtime","command":["./scripts/boole-miner-hermes-cli-smoke.sh"],"timeoutSec":300},
  {"name":"openclaw-opencode-agent-cli-mock-verify","kind":"agent-runtime","command":["./scripts/boole-miner-opencode-cli-smoke.sh"],"timeoutSec":300}
]'

BENCHMARK_KIND="agent-runtime-proof-to-block" \
BENCHMARK_TITLE="Boole Agent Runtime Proof-to-Block Leaderboard" \
BENCHMARK_SPEC_ENV="AGENT_RUNTIME_BENCHMARK_SPEC" \
BENCHMARK_DEFAULT_SPEC="$DEFAULT_SPEC" \
exec python3 "$ROOT/scripts/benchmark-runner.py"
