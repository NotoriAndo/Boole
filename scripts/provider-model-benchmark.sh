#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEFAULT_SPEC='[
  {"name":"mock-model-transport","kind":"provider-model","command":["./scripts/boole-miner-smoke.sh"],"timeoutSec":300},
  {"name":"ollama-gemma4-26b-openai-compat","kind":"provider-model","command":["./scripts/boole-miner-ollama-gemma-smoke.sh"],"requireEnv":["RUN_OLLAMA_BENCHMARK"],"timeoutSec":900}
]'

BENCHMARK_KIND="provider-model-proof-to-block" \
BENCHMARK_TITLE="Boole Provider/Model Proof-to-Block Leaderboard" \
BENCHMARK_SPEC_ENV="PROVIDER_MODEL_BENCHMARK_SPEC" \
BENCHMARK_DEFAULT_SPEC="$DEFAULT_SPEC" \
exec python3 "$ROOT/scripts/benchmark-runner.py"
