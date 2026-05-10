#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18095}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-opencode-cli-smoke.ndjson}"
STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-miner-opencode-cli-state.XXXXXX")"
STATE="$STATE_DIR/state.json"
RUNTIME_NAME="${AGENT_RUNTIME_NAME:-opencode-compatible}"

if [[ -n "${AGENT_RUNTIME_COMMAND:-}" ]]; then
  AGENT_CMD="$AGENT_RUNTIME_COMMAND"
elif command -v openclaw >/dev/null 2>&1; then
  AGENT_CMD="openclaw"
elif command -v opencode >/dev/null 2>&1; then
  AGENT_CMD="opencode"
else
  printf 'boole-miner-opencode-cli-smoke: SKIP openclaw/opencode not found on PATH\n' >&2
  printf '{"ok":true,"kind":"boole-miner-opencode-cli-smoke","skipped":true,"reason":"agent_runtime_not_found","runtime":"%s"}\n' "$RUNTIME_NAME"
  rm -rf "$STATE_DIR"
  exit 0
fi

AGENT_ARGS_JSON="${AGENT_RUNTIME_ARGS:-["'"'"run"'"'","'"'"--print"'"'"]}"
rm -f "$BLOCK_STORE"

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --max-requests 5 \
  >/tmp/boole-node-opencode-cli-smoke.out \
  2>/tmp/boole-node-opencode-cli-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; rm -f /tmp/boole-node-opencode-cli-smoke.out /tmp/boole-node-opencode-cli-smoke.err /tmp/boole-miner-opencode-cli-smoke-start.out /tmp/boole-miner-opencode-cli-smoke-init.out' EXIT

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

cargo run -q -p boole-miner -- init \
  --state "$STATE" \
  --dispatcher-url "http://$ADDR" \
  --llm-backend agent_cli \
  --agent-command "$AGENT_CMD" \
  --agent-args "$AGENT_ARGS_JSON" \
  --force >/tmp/boole-miner-opencode-cli-smoke-init.out
cargo run -q -p boole-miner -- start \
  --state "$STATE" \
  --max-shares 1 \
  --max-cycles 1 \
  --profile v01 \
  --difficulty 1 \
  --mock-verify-accept \
  >/tmp/boole-miner-opencode-cli-smoke-start.out

python3 - /tmp/boole-miner-opencode-cli-smoke-start.out "$ADDR" "$RUNTIME_NAME" "$AGENT_CMD" <<'PY'
import http.client
import json
import os
import re
import sys

log_path, addr, runtime_name, agent_cmd = sys.argv[1:5]
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
    raise SystemExit(f"bad node status after opencode-compatible miner run: {status}")
print(json.dumps({
    "ok": True,
    "kind": "boole-miner-opencode-cli-smoke",
    "runtime": runtime_name,
    "agentCommand": os.path.basename(agent_cmd),
    "miner": "boole-miner agent_cli OpenClaw/OpenCode-compatible CLI + mock verify",
    "node": "boole-node run-local",
    "summary": summary,
    "status": status,
}, separators=(",", ":")))
PY

wait "$PID"
printf 'boole-miner-opencode-cli-smoke: PASS\n' >&2
