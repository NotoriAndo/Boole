#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

source "$ROOT/scripts/smoke-lifecycle.sh"
ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:$((20000 + ($$ % 20000)))}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
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

def request(method, path, body=None, attempts=1, expect_status=200):
    payload = None if body is None else json.dumps(body).encode()
    last = None
    for _ in range(attempts):
        try:
            conn = http.client.HTTPConnection(host, port, timeout=2)
            headers = {"Content-Type": "application/json"} if payload is not None else {}
            conn.request(method, path, body=payload, headers=headers)
            res = conn.getresponse()
            raw = res.read().decode()
            if res.status != expect_status:
                raise SystemExit(f"{method} {path} expected {expect_status}, got {res.status} {raw}")
            return json.loads(raw)
        except OSError as err:
            last = err
            time.sleep(0.05)
    raise SystemExit(f"{method} {path} failed: {last}")

status = request("GET", "/status", attempts=50)
if not status.get("ok") or status.get("height") != 0:
    raise SystemExit(f"bad initial status: {status}")
health = request("GET", "/health")
if not health.get("ok") or health.get("status") != "ok" or not isinstance(health.get("sharePoolSize"), int):
    raise SystemExit(f"bad initial health: {health}")
empty_latest = request("GET", "/block/latest")
if (
    not empty_latest.get("ok")
    or empty_latest.get("block") is not None
    or empty_latest.get("height") is not None
    or empty_latest.get("c") != "0" * 64
):
    raise SystemExit(f"bad empty /block/latest: {empty_latest}")
bad_height = request("GET", "/block/notanumber", expect_status=400)
if bad_height.get("ok") is not False or bad_height.get("reason") != "bad_request":
    raise SystemExit(f"bad /block/notanumber envelope: {bad_height}")
missing_height = request("GET", "/block/9999", expect_status=404)
if missing_height.get("ok") is not False or missing_height.get("reason") != "not_found":
    raise SystemExit(f"bad /block/9999 envelope: {missing_height}")
scenario = json.loads(scenario_path.read_text())
submit = request("POST", "/submit", scenario["steps"][0]["body"])
if not submit.get("accepted") or not submit.get("replayMatchesRuntime"):
    raise SystemExit(f"bad submit result: {submit}")
latest = request("GET", "/block/latest")
if (
    not latest.get("ok")
    or latest.get("height") != 0
    or latest.get("c") != submit["block"]["c"]
    or latest.get("block", {}).get("c") != submit["block"]["c"]
):
    raise SystemExit(f"bad post-submit /block/latest: {latest}")
by_zero = request("GET", "/block/0")
if (
    not by_zero.get("ok")
    or by_zero.get("height") != 0
    or by_zero.get("c") != submit["block"]["c"]
):
    raise SystemExit(f"bad /block/0: {by_zero}")
head = request("GET", "/head")
if head.get("height") != 1 or head.get("c") != submit["block"]["c"]:
    raise SystemExit(f"bad head result: {head}")
# S24e — assert the economic-signal route the benchmark depends on actually
# resolves end-to-end against the real node. The /submit above credited the
# proposer; balance must be a non-empty u128 decimal string and asOfHeight must
# match the post-submit chain head.
proposer_pk = submit["block"]["proposerPk"]
balance = request("GET", f"/account/{proposer_pk}/balance")
if not balance.get("ok") or not isinstance(balance.get("balance"), str) or balance.get("balance") in ("", "0"):
    raise SystemExit(f"bad /account/<pk>/balance: {balance}")
if balance.get("asOfHeight") != latest["height"]:
    raise SystemExit(f"balance asOfHeight mismatch: {balance} vs latest {latest}")
if balance.get("asOfC") != latest["c"]:
    raise SystemExit(f"balance asOfC mismatch: {balance} vs latest {latest}")
int(balance["balance"])  # u128 decimal string parses as int
print(json.dumps({
    "ok": True,
    "status": status,
    "health": {"sharePoolSize": health["sharePoolSize"]},
    "emptyLatest": {"c": empty_latest["c"]},
    "badHeight": {"reason": bad_height["reason"]},
    "missingHeight": {"reason": missing_height["reason"]},
    "submit": {
        "accepted": submit["accepted"],
        "blockHeight": submit["block"]["height"],
        "c": submit["block"]["c"],
        "replayMatchesRuntime": submit["replayMatchesRuntime"],
    },
    "latest": {"height": latest["height"], "c": latest["c"]},
    "byZero": {"height": by_zero["height"], "c": by_zero["c"]},
    "head": head,
    "balance": {"pk": proposer_pk, "balance": balance["balance"], "asOfHeight": balance["asOfHeight"]},
}, separators=(",", ":")))
PY

smoke_stop_and_wait "$PID"
SMOKE_CHILD_PIDS=""
printf 'local-node-smoke: PASS\n' >&2
