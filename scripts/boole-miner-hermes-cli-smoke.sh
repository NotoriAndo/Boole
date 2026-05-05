#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MINER_ROOT="${BOOLE_MINER_ROOT:-$(cd "$ROOT/../pof/boole-miner" && pwd)}"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18093}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-hermes-cli-smoke.ndjson}"
STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-miner-hermes-cli-state.XXXXXX")"
STATE="$STATE_DIR/state.json"
rm -f "$BLOCK_STORE"

command -v hermes >/dev/null 2>&1 || {
  printf 'boole-miner-hermes-cli-smoke: SKIP hermes not found on PATH\n' >&2
  printf '{"ok":true,"kind":"boole-miner-hermes-cli-smoke","skipped":true,"reason":"hermes_not_found"}\n'
  exit 0
}

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --max-requests 5 \
  >/tmp/boole-node-hermes-cli-smoke.out \
  2>/tmp/boole-node-hermes-cli-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; rm -f /tmp/boole-node-hermes-cli-smoke.out /tmp/boole-node-hermes-cli-smoke.err /tmp/boole-miner-hermes-cli-smoke-start.out /tmp/boole-miner-hermes-cli-smoke-init.out' EXIT

python3 - "$ADDR" <<'PY'
import http.client
import sys
import time
host, port_raw = sys.argv[1].rsplit(":", 1)
port = int(port_raw)
last = None
for _ in range(80):
    try:
        conn = http.client.HTTPConnection(host, port, timeout=1)
        conn.request("GET", "/head")
        res = conn.getresponse()
        res.read()
        if res.status == 200:
            raise SystemExit(0)
    except OSError as err:
        last = err
        time.sleep(0.05)
raise SystemExit(f"boole-node did not become ready: {last}")
PY

(
  cd "$MINER_ROOT"
  npx tsx src/cli.ts --state "$STATE" init \
    --dispatcher-url "http://$ADDR" \
    --llm-backend agent_cli \
    --agent-command hermes \
    --agent-args '["chat","-Q","-t","","-q"]' \
    --force >/tmp/boole-miner-hermes-cli-smoke-init.out
  npx tsx src/cli.ts --state "$STATE" start \
    --max-shares 1 \
    --max-cycles 1 \
    --profile v01 \
    --difficulty 1 \
    --placeholder-canon \
    --mock-verify-accept \
    >/tmp/boole-miner-hermes-cli-smoke-start.out
)

python3 - /tmp/boole-miner-hermes-cli-smoke-start.out "$ADDR" <<'PY'
import http.client
import json
import re
import sys

log_path, addr = sys.argv[1], sys.argv[2]
raw = open(log_path).read()
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
if not match:
    raise SystemExit(f"missing miner summary:\n{raw}")
summary = json.loads(match.group(1))
if summary.get("sharesAccepted") != 1 or summary.get("verifyAccepted") != 1 or summary.get("llmSolved") != 1 or summary.get("networkErrors") != 0:
    raise SystemExit(f"bad miner summary: {summary}\n{raw}")
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse()
status = json.loads(res.read().decode())
if status.get("height") != 1 or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad node status after hermes_cli miner run: {status}")
print(json.dumps({
    "ok": True,
    "kind": "boole-miner-hermes-cli-smoke",
    "miner": "boole-miner agent_cli hermes chat + mock verify",
    "node": "boole-node run-local",
    "summary": summary,
    "status": status,
}, separators=(",", ":")))
PY

wait "$PID"
printf 'boole-miner-hermes-cli-smoke: PASS\n' >&2
