#!/usr/bin/env bash
set -euo pipefail

ORIGINAL_ARGS=("$@")
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

RUNTIME="${BOOLE_AGENT_RUNTIME:-hermes}"
VERIFY="${BOOLE_AGENT_VERIFY:-mock}"
ADDR="${BOOLE_NODE_ADDR:-}"
RUNTIME_COMMAND="${AGENT_RUNTIME_COMMAND:-}"
RUNTIME_ARGS="${AGENT_RUNTIME_ARGS:-}"
EVIDENCE_DIR="${BOOLE_AGENT_MINE_EVIDENCE_DIR:-}"

usage() {
  cat <<'EOF'
Usage: boole-agent-mine.sh [--runtime fake|hermes|opencode|openclaw|claude-code|codex] [--verify mock|real] [--addr HOST:PORT] [--agent-command CMD] [--agent-args JSON] [--evidence-dir DIR]

Slash-command foundation for agent-native Boole mining. The script is a thin UX
wrapper around boole-miner + boole-node smoke paths; consensus acceptance still
comes from deterministic verification/canonicalization/submit/replay.

Examples:
  ./scripts/boole-agent-mine.sh --runtime fake
  ./scripts/boole-agent-mine.sh --runtime hermes --verify mock
  ./scripts/boole-agent-mine.sh --runtime hermes --verify real
  ./scripts/boole-agent-mine.sh --runtime claude-code --agent-args '["-p"]'
  ./scripts/boole-agent-mine.sh --runtime codex --agent-command codex --agent-args '["exec"]'
EOF
}

json_skip() {
  local reason="$1"
  local runtime="$2"
  python3 - "$reason" "$runtime" <<'PY'
import json, sys
print(json.dumps({
    "ok": True,
    "kind": "boole-agent-mine",
    "skipped": True,
    "reason": sys.argv[1],
    "runtime": sys.argv[2],
}, separators=(",", ":")))
PY
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runtime)
      RUNTIME="${2:?missing --runtime value}"
      shift 2
      ;;
    --verify)
      VERIFY="${2:?missing --verify value}"
      shift 2
      ;;
    --addr)
      ADDR="${2:?missing --addr value}"
      shift 2
      ;;
    --agent-command)
      RUNTIME_COMMAND="${2:?missing --agent-command value}"
      shift 2
      ;;
    --agent-args)
      RUNTIME_ARGS="${2:?missing --agent-args value}"
      shift 2
      ;;
    --evidence-dir)
      EVIDENCE_DIR="${2:?missing --evidence-dir value}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'boole-agent-mine: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

case "$VERIFY" in
  mock|real) ;;
  *)
    printf 'boole-agent-mine: --verify must be mock or real, got %s\n' "$VERIFY" >&2
    exit 64
    ;;
esac

if [[ -n "$EVIDENCE_DIR" && -z "${BOOLE_AGENT_MINE_CAPTURED:-}" ]]; then
  mkdir -p "$EVIDENCE_DIR"
  set +e
  BOOLE_AGENT_MINE_CAPTURED=1 "$0" "${ORIGINAL_ARGS[@]}" >"$EVIDENCE_DIR/stdout.json" 2>"$EVIDENCE_DIR/stderr.txt"
  code=$?
  set -e
  python3 - "$EVIDENCE_DIR" "$code" <<'PY'
import datetime
import json
import pathlib
import sys

evidence_dir = pathlib.Path(sys.argv[1])
exit_code = int(sys.argv[2])
stdout_path = evidence_dir / "stdout.json"
stderr_path = evidence_dir / "stderr.txt"
raw_stdout = stdout_path.read_text(encoding="utf-8") if stdout_path.exists() else ""
raw_stderr = stderr_path.read_text(encoding="utf-8") if stderr_path.exists() else ""
try:
    result = json.loads(raw_stdout)
except json.JSONDecodeError:
    result = {"ok": False, "parseError": "stdout_not_json", "stdoutTail": raw_stdout[-1200:]}
summary = {
    "ok": exit_code == 0 and bool(result.get("ok")),
    "kind": "boole-agent-mine-evidence",
    "schemaVersion": 1,
    "generatedAt": datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z"),
    "evidenceDir": "[REDACTED_LOCAL_PATH]",
    "claimBoundary": "local controlled-smoke UX artifact, not public mining evidence",
    "publicMiningEvidence": False,
    "paidApiBenchmark": False,
    "redaction": {
        "localPaths": "redacted in summary only; raw stdout/stderr stay local under evidenceDir",
    },
    "result": result,
}
if exit_code != 0:
    summary["exitCode"] = exit_code
if raw_stderr.strip():
    summary["stderrTail"] = raw_stderr[-1200:]
(evidence_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY
  cat "$EVIDENCE_DIR/stdout.json"
  exit "$code"
fi

run_with_addr() {
  if [[ -n "$ADDR" ]]; then
    BOOLE_NODE_ADDR="$ADDR" "$@"
  else
    "$@"
  fi
}

run_opencode_compatible() {
  local runtime_name="$1"
  local default_cmd="$2"
  local default_args="$3"

  local cmd="$RUNTIME_COMMAND"
  if [[ -z "$cmd" ]]; then
    if command -v "$default_cmd" >/dev/null 2>&1; then
      cmd="$default_cmd"
    else
      json_skip "agent_runtime_not_found" "$runtime_name"
      exit 0
    fi
  fi

  if ! command -v "$cmd" >/dev/null 2>&1; then
    json_skip "agent_runtime_not_found" "$runtime_name"
    exit 0
  fi

  local args="$RUNTIME_ARGS"
  if [[ -z "$args" ]]; then
    args="$default_args"
  fi

  if [[ -n "$ADDR" ]]; then
    BOOLE_NODE_ADDR="$ADDR" AGENT_RUNTIME_NAME="$runtime_name" AGENT_RUNTIME_COMMAND="$cmd" AGENT_RUNTIME_ARGS="$args" \
      ./scripts/boole-miner-opencode-cli-smoke.sh
  else
    AGENT_RUNTIME_NAME="$runtime_name" AGENT_RUNTIME_COMMAND="$cmd" AGENT_RUNTIME_ARGS="$args" \
      ./scripts/boole-miner-opencode-cli-smoke.sh
  fi
}

case "$RUNTIME" in
  fake|fake-agent)
    if [[ "$VERIFY" != "mock" ]]; then
      printf 'boole-agent-mine: fake runtime only supports --verify mock\n' >&2
      exit 64
    fi
    run_with_addr ./scripts/boole-miner-agent-cli-smoke.sh
    ;;
  hermes)
    if [[ "$VERIFY" == "real" ]]; then
      run_with_addr ./scripts/boole-miner-hermes-real-verify-smoke.sh
    else
      run_with_addr ./scripts/boole-miner-hermes-cli-smoke.sh
    fi
    ;;
  opencode|openclaw|opencode-compatible)
    if [[ "$VERIFY" != "mock" ]]; then
      json_skip "real_verify_not_supported_for_runtime_wrapper" "$RUNTIME"
      exit 0
    fi
    if [[ "$RUNTIME" == "openclaw" ]]; then
      run_opencode_compatible "openclaw" "openclaw" '["run","--print"]'
    else
      run_opencode_compatible "opencode-compatible" "opencode" '["run","--print"]'
    fi
    ;;
  claude-code|claude)
    if [[ "$VERIFY" != "mock" ]]; then
      json_skip "real_verify_not_supported_for_runtime_wrapper" "$RUNTIME"
      exit 0
    fi
    run_opencode_compatible "claude-code" "claude" '["-p"]'
    ;;
  codex)
    if [[ "$VERIFY" != "mock" ]]; then
      json_skip "real_verify_not_supported_for_runtime_wrapper" "$RUNTIME"
      exit 0
    fi
    run_opencode_compatible "codex" "codex" '["exec"]'
    ;;
  *)
    printf 'boole-agent-mine: unsupported runtime: %s\n' "$RUNTIME" >&2
    usage >&2
    exit 64
    ;;
esac
