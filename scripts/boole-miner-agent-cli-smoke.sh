#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/smoke-lifecycle.sh"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="$(smoke_fresh_path "${BLOCK_STORE:-$SMOKE_WORK_DIR/blocks.ndjson}")"
REWARD_STORE="$(smoke_fresh_path "${REWARD_STORE:-$SMOKE_WORK_DIR/rewards.ndjson}")"
STATE_DIR="$SMOKE_WORK_DIR/state"
STATE="$STATE_DIR/state.json"
mkdir -p "$STATE_DIR"
FAKE_AGENT="$SMOKE_WORK_DIR/fake-agent-cli.sh"
AGENT_CALL_LOG="$SMOKE_WORK_DIR/agent-call.json"
MINER_INIT_OUT="$SMOKE_WORK_DIR/miner-init.out"
MINER_START_OUT="$SMOKE_WORK_DIR/miner-start.out"

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

# Build before the node readiness budget starts.
NODE_BIN="$(smoke_build_binary boole-node boole-node)"
MINER_BIN="$(smoke_build_binary boole-miner boole-miner --features boole-miner/dev-tools)"
smoke_prewarm_binary "$NODE_BIN"
smoke_prewarm_binary "$MINER_BIN"
"$NODE_BIN" run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_STORE" \
  --allow-anonymous-submit \
  >"$SMOKE_WORK_DIR/node.out" \
  2>"$SMOKE_WORK_DIR/node.err" &
PID=$!
smoke_register_child "$PID"

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

"$MINER_BIN" init \
  --state "$STATE" \
  --dispatcher-url "http://$ADDR" \
  --llm-backend agent_cli \
  --agent-command "$FAKE_AGENT" \
  --agent-args '["--mode","boole-proof"]' \
  --force >"$MINER_INIT_OUT"
BOOLE_AGENT_CALL_LOG="$AGENT_CALL_LOG" "$MINER_BIN" start \
  --state "$STATE" \
  --max-shares 1 \
  --max-cycles 1 \
  --profile v01 \
  --difficulty 1 \
  --mock-verify-accept \
  >"$MINER_START_OUT"

python3 - "$MINER_START_OUT" "$ADDR" "$AGENT_CALL_LOG" <<'PY'
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
if summary.get("sharesAccepted") != 1 or summary.get("verifyAccepted") != 1 or summary.get("driverAnswered") != 1 or summary.get("proofIntakeAccepted") != 1 or summary.get("networkErrors") != 0:
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

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'boole-miner-agent-cli-smoke: PASS\n' >&2
