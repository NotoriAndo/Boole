#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18081}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-local-smoke.ndjson}"
rm -f "$BLOCK_STORE"

cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --max-requests 3 \
  >/tmp/boole-node-local-smoke.out \
  2>/tmp/boole-node-local-smoke.err &
PID=$!
trap 'kill "$PID" >/dev/null 2>&1 || true; rm -f /tmp/boole-node-local-smoke.out /tmp/boole-node-local-smoke.err' EXIT

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

status = request("GET", "/status", attempts=50)
if not status.get("ok") or status.get("height") != 0:
    raise SystemExit(f"bad initial status: {status}")
scenario = json.loads(scenario_path.read_text())
submit = request("POST", "/submit", scenario["steps"][0]["body"])
if not submit.get("accepted") or not submit.get("replayMatchesRuntime"):
    raise SystemExit(f"bad submit result: {submit}")
head = request("GET", "/head")
if head.get("height") != 1 or head.get("c") != submit["block"]["c"]:
    raise SystemExit(f"bad head result: {head}")
print(json.dumps({
    "ok": True,
    "status": status,
    "submit": {
        "accepted": submit["accepted"],
        "blockHeight": submit["block"]["height"],
        "c": submit["block"]["c"],
        "replayMatchesRuntime": submit["replayMatchesRuntime"],
    },
    "head": head,
}, separators=(",", ":")))
PY

wait "$PID"
printf 'local-node-smoke: PASS\n' >&2
