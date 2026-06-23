#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18082}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-local-mining-smoke.ndjson}"
# Pin the reward ledger to a smoke-specific path so the run cannot inherit
# a stale `/tmp/boole-node-rewards.ndjson` left by an earlier self-test
# stage (e.g. `submit-lean` from proof-to-block-benchmark) and bail at boot
# with `reward ledger divergence: ledger=N replay=0`.
REWARD_LEDGER="${REWARD_LEDGER:-${TMPDIR:-/tmp}/boole-node-local-mining-smoke-rewards.ndjson}"
rm -f "$BLOCK_STORE" "$REWARD_LEDGER"

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_LEDGER" \
  --max-requests 9 \
  --allow-anonymous-submit \
  >/tmp/boole-node-local-mining-smoke.out \
  2>/tmp/boole-node-local-mining-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -f /tmp/boole-node-local-mining-smoke.out /tmp/boole-node-local-mining-smoke.err "$BLOCK_STORE" "$REWARD_LEDGER"' EXIT

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

wait "$PID"
printf 'local-mining-smoke: PASS\n' >&2
