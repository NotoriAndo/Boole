#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
source "$ROOT/scripts/smoke-lifecycle.sh"

# A per-invocation default avoids colliding with another local smoke. Callers
# may still supply BOOLE_NODE_ADDR when they need a fixed controlled address.
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="$(smoke_fresh_path "${BLOCK_STORE:-$SMOKE_WORK_DIR/blocks.ndjson}")"
REWARD_STORE="$(smoke_fresh_path "${REWARD_STORE:-$SMOKE_WORK_DIR/rewards.ndjson}")"
STATE="$SMOKE_WORK_DIR/state.json"
NODE_OUT="$SMOKE_WORK_DIR/node.out"
NODE_ERR="$SMOKE_WORK_DIR/node.err"
MINER_INIT_OUT="$SMOKE_WORK_DIR/miner-init.out"
MINER_START_OUT="$SMOKE_WORK_DIR/miner-start.out"
# Build before the readiness budget starts: a cold Cargo compilation is not a
# node-readiness failure.  The dev-only smoke bypass is opt-in for this binary
# only and does not change the default production build.
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
  >"$NODE_OUT" \
  2>"$NODE_ERR" &
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
  --llm-backend mock \
  --force >"$MINER_INIT_OUT"
"$MINER_BIN" start \
  --state "$STATE" \
  --max-shares 1 \
  --max-cycles 1 \
  --profile v1-lenbound \
  --difficulty 1 \
  --mock-verify-accept \
  --mock-llm-response $'```lean\nfun xs => rfl\n```' \
  >"$MINER_START_OUT"

python3 - "$MINER_START_OUT" "$ADDR" <<'PY'
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
if summary.get("sharesAccepted") != 1 or summary.get("verifyAccepted") != 1 or summary.get("networkErrors") != 0:
    raise SystemExit(f"bad miner summary: {summary}\n{raw}")
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse()
status = json.loads(res.read().decode())
if status.get("height") != 1 or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad node status after miner run: {status}")
print(json.dumps({
    "ok": True,
    "kind": "boole-miner-smoke",
    "miner": "boole-miner mock llm + mock verify",
    "node": "boole-node run-local",
    "summary": summary,
    "status": status,
}, separators=(",", ":")))
PY

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'boole-miner-smoke: PASS\n' >&2
