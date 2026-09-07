#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/smoke-lifecycle.sh"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
# This smoke validates the HTTP proof-to-block path, not rate-limit policy.
# Its derived fixture keeps the canonical scenario intact and raises only the
# node-local per-IP quota enough for both immediate loopback submissions.
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/local-mining-smoke.v1.json}"
BLOCK_STORE="$(smoke_fresh_path "${BLOCK_STORE:-$SMOKE_WORK_DIR/blocks.ndjson}")"
REWARD_LEDGER="$(smoke_fresh_path "${REWARD_LEDGER:-$SMOKE_WORK_DIR/rewards.ndjson}")"

# Finish compilation before the HTTP readiness budget starts.
NODE_BIN="$(smoke_build_binary boole-node boole-node)"
smoke_prewarm_binary "$NODE_BIN"
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

python3 - "$ADDR" "$SCENARIO" <<'PY'
import http.client
import json
import pathlib
import sys
import time

addr, scenario_path = sys.argv[1], pathlib.Path(sys.argv[2])
host, port_raw = addr.rsplit(":", 1)
port = int(port_raw)

def request(method, path, body=None, attempts=1):
    payload = None if body is None else json.dumps(body).encode()
    last = None
    for _ in range(attempts):
        try:
            conn = http.client.HTTPConnection(host, port, timeout=2)
            headers = {"Content-Type": "application/json"} if payload is not None else {}
            conn.request(method, path, body=payload, headers=headers)
            res = conn.getresponse()
            raw = res.read().decode()
            if res.status != 200:
                raise SystemExit(f"{method} {path} failed: {res.status} {raw}")
            return json.loads(raw)
        except OSError as err:
            last = err
            time.sleep(0.05)
    raise SystemExit(f"{method} {path} failed: {last}")

scenario = json.loads(scenario_path.read_text())
steps = scenario["steps"]
initial_head = request("GET", "/head", attempts=50)
config = request("GET", "/config")
if not initial_head.get("ok") or initial_head.get("height") != 0:
    raise SystemExit(f"bad initial head: {initial_head}")
if not config.get("ok") or not config.get("T_share"):
    raise SystemExit(f"bad node config: {config}")

mined = []
head = initial_head
for i, step in enumerate(steps):
    body = dict(step["body"])
    if step.get("cFromRuntimeHead"):
        body["c"] = head["c"]
    candidate = {
        "body": body,
        "ip": step.get("ip", f"192.0.2.{10 + i}"),
        "canonTag": step.get("canonTag", 0),
        "ts": step.get("ts", 1800000000000 + i),
    }
    ticket = request("POST", "/ticket", {"c": body["c"], "pk": body["pk"], "n": body["n"]})
    if not ticket.get("ok") or len(ticket.get("hashHex", "")) != 64:
        raise SystemExit(f"bad ticket result at step {i}: {ticket}")
    submit = request("POST", "/submit", candidate)
    if not submit.get("accepted"):
        raise SystemExit(f"mock miner submit rejected at step {i}: {submit}")
    if not submit.get("replayMatchesRuntime"):
        raise SystemExit(f"mock miner submit diverged at step {i}: {submit}")
    head = request("GET", "/head")
    expected_height = i + 1
    if head.get("height") != expected_height:
        raise SystemExit(f"bad head after step {i}: expected height {expected_height}, got {head}")
    if head.get("c") != submit["block"]["c"]:
        raise SystemExit(f"head/block mismatch after step {i}: head={head} submit={submit}")
    mined.append({
        "step": i,
        "accepted": True,
        "blockHeight": submit["block"]["height"],
        "c": submit["block"]["c"],
        "replayMatchesRuntime": submit["replayMatchesRuntime"],
    })

status = request("GET", "/status")
if status.get("height") != len(steps) or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad final status: {status}")
print(json.dumps({
    "ok": True,
    "kind": "local-mining-smoke",
    "claimBoundary": "controlled local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "single local boole-node process",
        "mock fixture miner only",
        "no public network admission",
    ],
    "miner": "mock-fixture-miner",
    "node": "boole-node run-local",
    "blocksMined": len(mined),
    "initialHead": initial_head,
    "finalHead": head,
    "status": status,
    "mined": mined,
}, separators=(",", ":")))
PY

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'local-mining-smoke: PASS\n' >&2
