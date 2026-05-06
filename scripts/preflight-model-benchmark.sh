#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PRESET="${MODEL_BENCHMARK_PRESET:-all}"
OUTPUT_SPEC=""
LEADERBOARD_MD="${LEADERBOARD_MD:-}"
ATTEMPTS_PER_MODEL="${ATTEMPTS_PER_MODEL:-}"
INCLUDES=()
OLLAMA_MODELS=()

usage() {
  cat <<'EOF'
Usage: preflight-model-benchmark.sh [--preset mock|frontier|oauth|ollama|all] [--include TERM] [--ollama-model MODEL] [--output-spec PATH] [--leaderboard-md PATH] [--attempts-per-model N]

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

./scripts/preflight-model-benchmark-setup.py \
  --preset "$PRESET" \
  ${INCLUDES[@]+"${INCLUDES[@]}"} \
  ${OLLAMA_MODELS[@]+"${OLLAMA_MODELS[@]}"} \
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
