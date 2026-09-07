#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/smoke-lifecycle.sh"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="$(smoke_fresh_path "${BLOCK_STORE:-$SMOKE_WORK_DIR/blocks.ndjson}")"
# Pin the reward ledger to a smoke-specific path so a stale
# `/tmp/boole-node-rewards.ndjson` from another self-test cannot trip
# `reward ledger divergence` at node boot.
REWARD_LEDGER="$(smoke_fresh_path "${REWARD_LEDGER:-$SMOKE_WORK_DIR/rewards.ndjson}")"
TRIALS="${TRIALS:-3}"
PROFILE="${PROFILE:-v1-lenbound}"
LEAN_DIR="${LEAN_DIR:-$ROOT/lean/checker}"
FIXED_SEED="${FIXED_SEED:-b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764}"
STATE_DIR="$SMOKE_WORK_DIR/state"
STATE="$STATE_DIR/state.json"
RESULTS_JSONL="$STATE_DIR/results.jsonl"
mkdir -p "$STATE_DIR"
MINER_INIT_OUT="$SMOKE_WORK_DIR/miner-init.out"

command -v hermes >/dev/null 2>&1 || {
  printf 'boole-miner-hermes-real-verify-smoke: SKIP hermes not found on PATH\n' >&2
  printf '{"ok":true,"kind":"boole-miner-hermes-real-verify-smoke","skipped":true,"reason":"hermes_not_found"}\n'
  exit 0
}

# Build before the node readiness budget starts.
NODE_BIN="$(smoke_build_binary boole-node boole-node)"
MINER_BIN="$(smoke_build_binary boole-miner boole-miner)"
smoke_prewarm_binary "$NODE_BIN"
smoke_prewarm_binary "$MINER_BIN"
"$NODE_BIN" run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_LEDGER" \
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
  --agent-command hermes \
  --agent-args '["chat","-Q","-t","","-q"]' \
  --force >"$MINER_INIT_OUT"

success=0
for trial in $(seq 1 "$TRIALS"); do
  out="$SMOKE_WORK_DIR/miner-start.${trial}.out"
  set +e
  "$MINER_BIN" start \
    --state "$STATE" \
    --max-shares 1 \
    --max-cycles 1 \
    --profile "$PROFILE" \
    --difficulty 0 \
    --lean-dir "$LEAN_DIR" \
    --fixed-target-seed-hex "$FIXED_SEED" \
    >"$out"
  code=$?
  set -e
  python3 - "$trial" "$code" "$out" >>"$RESULTS_JSONL" <<'PY'
import json
import re
import sys
trial = int(sys.argv[1])
code = int(sys.argv[2])
path = sys.argv[3]
raw = open(path).read() if __import__('os').path.exists(path) else ''
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
summary = json.loads(match.group(1)) if match else None
print(json.dumps({"trial": trial, "exitCode": code, "summary": summary, "tail": raw[-1200:]}, separators=(",", ":")))
PY
  if python3 - "$out" <<'PY'
import json, re, sys
raw = open(sys.argv[1]).read()
match = re.search(r"summary:\s*(\{[\s\S]*\})\s*$", raw)
if not match:
    raise SystemExit(1)
s = json.loads(match.group(1))
raise SystemExit(0 if s.get("sharesAccepted") == 1 and s.get("verifyAccepted") == 1 and s.get("networkErrors") == 0 else 1)
PY
  then
    success=1
    break
  fi
done

python3 - "$ADDR" "$RESULTS_JSONL" "$success" <<'PY'
import http.client
import json
import sys
addr, results_path, success_raw = sys.argv[1], sys.argv[2], sys.argv[3]
rows = [json.loads(line) for line in open(results_path)] if __import__('os').path.exists(results_path) else []
host, port_raw = addr.rsplit(":", 1)
conn = http.client.HTTPConnection(host, int(port_raw), timeout=2)
conn.request("GET", "/status")
res = conn.getresponse()
status = json.loads(res.read().decode())
verify_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("verifyAccepted") == 1)
shares_accepted = sum(1 for r in rows if (r.get("summary") or {}).get("sharesAccepted") == 1)
ok = success_raw == "1" and status.get("height", 0) >= 1 and status.get("replayMatchesRuntime") is True
out = {
    "ok": ok,
    "kind": "boole-miner-hermes-real-verify-smoke",
    "miner": "boole-miner agent_cli hermes chat + StructuralCanonicalizer (placeholder POFP canon) + LeanVerifier (boole-lean-runner)",
    "node": "boole-node run-local",
    "trials": len(rows),
    "aggregate": {"verifyAccepted": verify_accepted, "sharesAccepted": shares_accepted},
    "rows": rows,
    "status": status,
}
print(json.dumps(out, separators=(",", ":")))
if not ok:
    raise SystemExit("boole-miner-hermes-real-verify-smoke: no real-verify block success")
PY

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'boole-miner-hermes-real-verify-smoke: PASS\n' >&2
