#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PRESET="${MODEL_BENCHMARK_PRESET:-all}"
OUTPUT_SPEC=""
LEADERBOARD_MD="${LEADERBOARD_MD:-}"
ATTEMPTS_PER_MODEL="${ATTEMPTS_PER_MODEL:-}"
BENCHMARK_COMMAND="${MODEL_BENCHMARK_COMMAND:-}"
OLLAMA_COMMAND="${BOOLE_OLLAMA_COMMAND:-}"
CLAUDE_COMMAND="${BOOLE_CLAUDE_COMMAND:-}"
SUBMIT_LEAN_COMMAND="${BOOLE_SUBMIT_LEAN_COMMAND:-}"
NODE_URL="${BOOLE_NODE_URL:-}"
USE_NODE_TICKET="${BOOLE_USE_NODE_TICKET:-0}"
ISOLATED_NODE_PER_ROW="${BOOLE_ISOLATED_NODE_PER_ROW:-0}"
ISOLATED_NODE_BASE_PORT="${BOOLE_ISOLATED_NODE_BASE_PORT:-18140}"
PROVER_PK="${BOOLE_PROVER_PK:-}"
BOUNTY_ID="${BOOLE_BOUNTY_ID:-}"
INCLUDES=()
OLLAMA_MODELS=()

usage() {
  cat <<'EOF'
Usage: preflight-model-benchmark.sh [--preset mock|frontier|oauth|ollama|all] [--include TERM] [--ollama-model MODEL] [--output-spec PATH] [--leaderboard-md PATH] [--attempts-per-model N] [--benchmark-command CMD] [--ollama-command CMD] [--claude-command CMD] [--submit-lean-command CMD] [--node-url URL] [--use-node-ticket] [--isolated-node-per-row] [--isolated-node-base-port PORT]

Generates a provider/model benchmark spec from available frontier API envs,
local OAuth CLIs, and Ollama models, then runs provider-model-benchmark.sh.
Secrets are never printed; API rows only record credential presence via requireEnv.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --preset)
      PRESET="${2:?missing --preset value}"
      shift 2
      ;;
    --include)
      INCLUDES+=(--include "${2:?missing --include value}")
      shift 2
      ;;
    --ollama-model)
      OLLAMA_MODELS+=(--ollama-model "${2:?missing --ollama-model value}")
      shift 2
      ;;
    --output-spec)
      OUTPUT_SPEC="${2:?missing --output-spec value}"
      shift 2
      ;;
    --leaderboard-md)
      LEADERBOARD_MD="${2:?missing --leaderboard-md value}"
      shift 2
      ;;
    --attempts-per-model)
      ATTEMPTS_PER_MODEL="${2:?missing --attempts-per-model value}"
      if ! [[ "$ATTEMPTS_PER_MODEL" =~ ^[1-9][0-9]*$ ]]; then
        printf 'preflight-model-benchmark: --attempts-per-model must be a positive integer\n' >&2
        exit 64
      fi
      shift 2
      ;;
    --benchmark-command)
      BENCHMARK_COMMAND="${2:?missing --benchmark-command value}"
      shift 2
      ;;
    --ollama-command)
      OLLAMA_COMMAND="${2:?missing --ollama-command value}"
      shift 2
      ;;
    --claude-command)
      CLAUDE_COMMAND="${2:?missing --claude-command value}"
      shift 2
      ;;
    --submit-lean-command)
      SUBMIT_LEAN_COMMAND="${2:?missing --submit-lean-command value}"
      shift 2
      ;;
    --node-url)
      NODE_URL="${2:?missing --node-url value}"
      shift 2
      ;;
    --use-node-ticket)
      USE_NODE_TICKET=1
      shift
      ;;
    --isolated-node-per-row)
      ISOLATED_NODE_PER_ROW=1
      shift
      ;;
    --isolated-node-base-port)
      ISOLATED_NODE_BASE_PORT="${2:?missing --isolated-node-base-port value}"
      shift 2
      ;;
    --prover-pk)
      PROVER_PK="${2:?missing --prover-pk value}"
      shift 2
      ;;
    --bounty-id)
      BOUNTY_ID="${2:?missing --bounty-id value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'preflight-model-benchmark: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

TMP_SPEC=""
if [[ -z "$OUTPUT_SPEC" ]]; then
  TMP_SPEC="$(mktemp "${TMPDIR:-/tmp}/boole-model-benchmark-spec.XXXXXX.json")"
  OUTPUT_SPEC="$TMP_SPEC"
fi
trap 'if [[ -n "$TMP_SPEC" ]]; then rm -f "$TMP_SPEC"; fi' EXIT

if [[ -n "$ATTEMPTS_PER_MODEL" ]]; then
  export TRIALS="$ATTEMPTS_PER_MODEL"
fi

# S24e — when running against a live node, probe the economic-signal routes
# (/head, /account/<pk>/balance, /bounties/<id>) up front. The benchmark
# silently records empty reward fields if any of these 404, so a misconfigured
# node would otherwise produce a green run with no economic signal. Failing
# fast here is cheap and surfaces the failing route to the operator.
if [[ -n "$NODE_URL" && "$ISOLATED_NODE_PER_ROW" != "1" ]]; then
  PREFLIGHT_ARGS=(--preflight-node --node-url "$NODE_URL")
  if [[ -n "$PROVER_PK" ]]; then
    PREFLIGHT_ARGS+=(--prover-pk "$PROVER_PK")
  fi
  if [[ -n "$BOUNTY_ID" ]]; then
    PREFLIGHT_ARGS+=(--bounty-id "$BOUNTY_ID")
  fi
  printf 'preflight-model-benchmark: probing %s economic routes...\n' "$NODE_URL" >&2
  python3 ./scripts/boole-model-benchmark.py "${PREFLIGHT_ARGS[@]}"
fi

SETUP_ARGS=()
if [[ -n "$BENCHMARK_COMMAND" ]]; then
  SETUP_ARGS+=(--benchmark-command "$BENCHMARK_COMMAND")
fi
if [[ -n "$OLLAMA_COMMAND" ]]; then
  SETUP_ARGS+=(--ollama-command "$OLLAMA_COMMAND")
fi
if [[ -n "$CLAUDE_COMMAND" ]]; then
  SETUP_ARGS+=(--claude-command "$CLAUDE_COMMAND")
fi
if [[ -n "$SUBMIT_LEAN_COMMAND" ]]; then
  SETUP_ARGS+=(--submit-lean-command "$SUBMIT_LEAN_COMMAND")
fi
if [[ -n "$NODE_URL" ]]; then
  SETUP_ARGS+=(--node-url "$NODE_URL")
fi
case "$USE_NODE_TICKET" in
  1|true|TRUE|yes|YES)
    SETUP_ARGS+=(--use-node-ticket)
    ;;
esac
case "$ISOLATED_NODE_PER_ROW" in
  1|true|TRUE|yes|YES)
    SETUP_ARGS+=(--isolated-node-per-row --isolated-node-base-port "$ISOLATED_NODE_BASE_PORT")
    ;;
esac
if [[ -n "$OUTPUT_SPEC" ]]; then
  SETUP_ARGS+=(--artifact-root "$(dirname "$OUTPUT_SPEC")/model-benchmark-artifacts")
fi

./scripts/preflight-model-benchmark-setup.py \
  --preset "$PRESET" \
  ${INCLUDES[@]+"${INCLUDES[@]}"} \
  ${OLLAMA_MODELS[@]+"${OLLAMA_MODELS[@]}"} \
  ${SETUP_ARGS[@]+"${SETUP_ARGS[@]}"} \
  --output "$OUTPUT_SPEC"

spec_json="$(python3 - "$OUTPUT_SPEC" <<'PY'
from pathlib import Path
import json, sys
print(json.dumps(json.loads(Path(sys.argv[1]).read_text()), separators=(",", ":")))
PY
)"

if [[ -n "$LEADERBOARD_MD" ]]; then
  PROVIDER_MODEL_BENCHMARK_SPEC="$spec_json" LEADERBOARD_MD="$LEADERBOARD_MD" ./scripts/provider-model-benchmark.sh
else
  PROVIDER_MODEL_BENCHMARK_SPEC="$spec_json" ./scripts/provider-model-benchmark.sh
fi
