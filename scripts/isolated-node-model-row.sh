#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BENCHMARK_COMMAND="python3 scripts/boole-model-benchmark.py"
TARGET=""
ATTEMPTS="1"
OUTPUT_DIR=""
RUN_ID=""
OLLAMA_COMMAND=""
CLAUDE_COMMAND=""
SUBMIT_LEAN_COMMAND=""
NODE_PORT="18140"
SCENARIO="fixtures/protocol/runtime-smoke/v1.json"
TIMEOUT_SEC="600"
USE_NODE_TICKET=0

usage() {
  cat <<'EOF'
Usage: isolated-node-model-row.sh --target TARGET --output-dir DIR --run-id ID [--benchmark-command CMD] [--ollama-command CMD] [--claude-command CMD] [--submit-lean-command CMD] [--attempts N] [--node-port PORT] [--timeout-sec N] [--use-node-ticket]

Runs one model benchmark row against a fresh local boole-node, block store, reward store, and quota state. This is controlled local evidence, not public-network mining.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --benchmark-command) BENCHMARK_COMMAND="${2:?missing --benchmark-command value}"; shift 2 ;;
    --target) TARGET="${2:?missing --target value}"; shift 2 ;;
    --attempts) ATTEMPTS="${2:?missing --attempts value}"; shift 2 ;;
    --output-dir) OUTPUT_DIR="${2:?missing --output-dir value}"; shift 2 ;;
    --run-id) RUN_ID="${2:?missing --run-id value}"; shift 2 ;;
    --ollama-command) OLLAMA_COMMAND="${2:?missing --ollama-command value}"; shift 2 ;;
    --claude-command) CLAUDE_COMMAND="${2:?missing --claude-command value}"; shift 2 ;;
    --submit-lean-command) SUBMIT_LEAN_COMMAND="${2:?missing --submit-lean-command value}"; shift 2 ;;
    --node-port) NODE_PORT="${2:?missing --node-port value}"; shift 2 ;;
    --scenario) SCENARIO="${2:?missing --scenario value}"; shift 2 ;;
    --timeout-sec) TIMEOUT_SEC="${2:?missing --timeout-sec value}"; shift 2 ;;
    --use-node-ticket) USE_NODE_TICKET=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'isolated-node-model-row: unknown argument: %s\n' "$1" >&2; usage >&2; exit 64 ;;
  esac
done

if [[ -z "$TARGET" || -z "$OUTPUT_DIR" || -z "$RUN_ID" ]]; then
  usage >&2
  exit 64
fi
if ! [[ "$NODE_PORT" =~ ^[0-9]+$ ]]; then
  printf 'isolated-node-model-row: --node-port must be an integer\n' >&2
  exit 64
fi

mkdir -p "$OUTPUT_DIR"
NODE_DIR="$OUTPUT_DIR/isolated-node"
mkdir -p "$NODE_DIR"
BLOCK_STORE="$NODE_DIR/blocks.ndjson"
REWARD_STORE="$NODE_DIR/rewards.ndjson"
NODE_OUT="$NODE_DIR/node.out"
NODE_ERR="$NODE_DIR/node.err"
SUBMIT_WRAPPER="$NODE_DIR/submit-lean-wrapper.sh"
rm -f "$BLOCK_STORE" "$REWARD_STORE" "$NODE_OUT" "$NODE_ERR"

if [[ -z "$SUBMIT_LEAN_COMMAND" ]]; then
  cat > "$SUBMIT_WRAPPER" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
exec target/debug/boole-node "$@"
SH
  chmod +x "$SUBMIT_WRAPPER"
  SUBMIT_LEAN_COMMAND="$SUBMIT_WRAPPER"
fi

cargo build -q -p boole-node
# Prime macOS dyld codesign cache: cargo's atomic-rename-on-build replaces the
# binary's inode/mtime even on no-op builds, which makes the next launch hang
# in _dyld_start while the kernel re-verifies the signature. A throwaway
# --version invocation pays that cost up front instead of inside the polling
# window.
target/debug/boole-node --version >/dev/null
EXISTING="$(lsof -tiTCP:${NODE_PORT} -sTCP:LISTEN || true)"
if [[ -n "$EXISTING" ]]; then
  printf 'isolated-node-model-row: port %s already in use\n' "$NODE_PORT" >&2
  exit 69
fi

target/debug/boole-node run-local \
  --addr "127.0.0.1:${NODE_PORT}" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_STORE" \
  --lean-checker-dir lean/checker \
  --max-requests 80 \
  >"$NODE_OUT" \
  2>"$NODE_ERR" &
NODE_PID=$!
cleanup() {
  kill "$NODE_PID" >/dev/null 2>&1 || true
  wait "$NODE_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

python3 - "127.0.0.1:${NODE_PORT}" <<'PY'
import http.client, json, sys, time
host, port_raw = sys.argv[1].split(":")
port = int(port_raw)
last = None
for _ in range(100):
    try:
        conn = http.client.HTTPConnection(host, port, timeout=1)
        conn.request("GET", "/head")
        res = conn.getresponse()
        raw = res.read().decode()
        if res.status == 200 and json.loads(raw).get("ok"):
            raise SystemExit(0)
        last = (res.status, raw)
    except Exception as err:
        last = repr(err)
    time.sleep(0.05)
raise SystemExit(f"node not ready: {last}")
PY

CMD=()
# shellcheck disable=SC2206
CMD=($BENCHMARK_COMMAND)
CMD+=(--target "$TARGET" --attempts "$ATTEMPTS" --output-dir "$OUTPUT_DIR" --run-id "$RUN_ID" --timeout-sec "$TIMEOUT_SEC" --submit-lean-command "$SUBMIT_LEAN_COMMAND" --node-url "http://127.0.0.1:${NODE_PORT}")
if [[ -n "$OLLAMA_COMMAND" ]]; then
  CMD+=(--ollama-command "$OLLAMA_COMMAND")
fi
if [[ -n "$CLAUDE_COMMAND" ]]; then
  CMD+=(--claude-command "$CLAUDE_COMMAND")
fi
if [[ "$USE_NODE_TICKET" == "1" ]]; then
  CMD+=(--use-node-ticket)
fi

"${CMD[@]}"
