#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18092}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-agent-cli-smoke.ndjson}"
STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-miner-agent-cli-state.XXXXXX")"
STATE="$STATE_DIR/state.json"
FAKE_AGENT="$STATE_DIR/fake-agent-cli.sh"
AGENT_CALL_LOG="$STATE_DIR/agent-call.json"
rm -f "$BLOCK_STORE"

cat >"$FAKE_AGENT" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
log_path="${BOOLE_AGENT_CALL_LOG:?missing BOOLE_AGENT_CALL_LOG}"
prompt="${@: -1}"
python3 - "$log_path" "$prompt" "$@" <<'PY'
import json
import sys
log_path = sys.argv[1]
prompt = sys.argv[2]
argv = sys.argv[3:]
# Do not persist the full prompt in smoke output; record only the configured
# argv prefix plus metrics proving the target prompt reached the agent.
with open(log_path, "w") as f:
    json.dump({"argPrefix": argv[:-1], "promptLen": len(prompt), "promptHasBoole": "Boole" in prompt or "boole" in prompt}, f)
PY
printf 'agent_cli proof candidate:\n```lean\nfun xs => rfl\n```\n'
SH
chmod +x "$FAKE_AGENT"

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --max-requests 5 \
  >/tmp/boole-node-agent-cli-smoke.out \
  2>/tmp/boole-node-agent-cli-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -rf "$STATE_DIR"; rm -f /tmp/boole-node-agent-cli-smoke.out /tmp/boole-node-agent-cli-smoke.err /tmp/boole-miner-agent-cli-smoke-start.out /tmp/boole-miner-agent-cli-smoke-init.out' EXIT

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
  --agent-command "$FAKE_AGENT" \
  --agent-args '["--mode","boole-proof"]' \
  --force >/tmp/boole-miner-agent-cli-smoke-init.out
BOOLE_AGENT_CALL_LOG="$AGENT_CALL_LOG" cargo run -q -p boole-miner -- start \
  --state "$STATE" \
  --max-shares 1 \
  --max-cycles 1 \
  --profile v01 \
  --difficulty 1 \
  --mock-verify-accept \
  >/tmp/boole-miner-agent-cli-smoke-start.out

python3 - /tmp/boole-miner-agent-cli-smoke-start.out "$ADDR" "$AGENT_CALL_LOG" <<'PY'
import http.client
import json
import re
import sys

log_path, addr, agent_log_path = sys.argv[1], sys.argv[2], sys.argv[3]
raw = open(log_path).read()
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
if not match:
    raise SystemExit(f"missing miner summary:\n{raw}")
summary = json.loads(match.group(1))
if summary.get("sharesAccepted") != 1 or summary.get("verifyAccepted") != 1 or summary.get("llmSolved") != 1 or summary.get("networkErrors") != 0:
    raise SystemExit(f"bad miner summary: {summary}\n{raw}")
agent = json.load(open(agent_log_path))
if agent.get("argPrefix") != ["--mode", "boole-proof"] or not agent.get("promptHasBoole") or agent.get("promptLen", 0) <= 0:
    raise SystemExit(f"bad agent invocation log: {agent}")
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse()
status = json.loads(res.read().decode())
if status.get("height") != 1 or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad node status after agent_cli miner run: {status}")
print(json.dumps({
    "ok": True,
    "kind": "boole-miner-agent-cli-smoke",
    "miner": "boole-miner agent_cli fake agent + mock verify",
    "node": "boole-node run-local",
    "agent": agent,
    "summary": summary,
    "status": status,
}, separators=(",", ":")))
PY

wait "$PID"
printf 'boole-miner-agent-cli-smoke: PASS\n' >&2
