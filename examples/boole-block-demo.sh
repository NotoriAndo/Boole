#!/usr/bin/env bash
# End-to-end demo of the S3 boole-cli surface:
#   boole node start (background) -> boole block latest -> boole block get --height 0
#
# Drives a single submit through the local node so `block latest`/`block get`
# return real block envelopes. Output is JSON suitable for the frontpage card.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${PORT:-18091}"
DATA_DIR="${DATA_DIR:-${TMPDIR:-/tmp}/boole-block-demo-$$}"
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/v1.json}"
NODE_URL="http://127.0.0.1:${PORT}"

rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"

# The boole-cli + boole-node binaries from the dev profile target dir. Both
# binaries are built once up front so timing measurements below cover only
# request latency, not cargo overhead.
cargo build -q -p boole-cli -p boole-node
CLI_BIN="$ROOT/target/debug/boole-cli"
NODE_BIN="$ROOT/target/debug/boole-node"

# Cap requests at exactly the number this script issues. The readiness probe
# loop below uses the typed `block latest` call, so it counts towards the
# served budget. Calls: latest-empty + submit + latest-after + get-height-0
# = 4. Bump if you add steps.
MAX_REQUESTS=4

BOOLE_NODE_BIN="$NODE_BIN" "$CLI_BIN" node start \
  --port "$PORT" \
  --data-dir "$DATA_DIR" \
  --scenario "$SCENARIO" \
  --max-requests "$MAX_REQUESTS" \
  >"$DATA_DIR/node.out" 2>"$DATA_DIR/node.err" &
NODE_PID=$!

cleanup() {
  if kill -0 "$NODE_PID" 2>/dev/null; then
    kill "$NODE_PID" >/dev/null 2>&1 || true
  fi
  wait "$NODE_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Block until the typed `block latest` call succeeds — empty-chain returns
# `{ok:true, c:"0"*64}` with status 200, which doubles as a readiness probe.
ATTEMPTS=200
until LATEST_EMPTY="$("$CLI_BIN" block latest --node "$NODE_URL" --json 2>/dev/null)"; do
  ATTEMPTS=$((ATTEMPTS - 1))
  if [ "$ATTEMPTS" -le 0 ]; then
    echo "boole-node did not become ready on $NODE_URL" >&2
    exit 1
  fi
  sleep 0.05
done

python3 - "$NODE_URL" "$SCENARIO" >"$DATA_DIR/submit.json" <<'PY'
import http.client
import json
import pathlib
import sys
import urllib.parse

url, scenario_path = sys.argv[1], pathlib.Path(sys.argv[2])
parsed = urllib.parse.urlparse(url)
host, port = parsed.hostname, parsed.port
scenario = json.loads(scenario_path.read_text())
body = json.dumps(scenario["steps"][0]["body"]).encode()
conn = http.client.HTTPConnection(host, port, timeout=2)
conn.request("POST", "/submit", body=body, headers={"Content-Type": "application/json"})
res = conn.getresponse()
raw = res.read().decode()
if res.status != 200:
    raise SystemExit(f"submit failed: {res.status} {raw}")
sys.stdout.write(raw)
PY

LATEST_AFTER="$("$CLI_BIN" block latest --node "$NODE_URL" --json)"
GET_ZERO="$("$CLI_BIN" block get --height 0 --node "$NODE_URL" --json)"

python3 - <<PY
import json
import sys

empty = json.loads('''$LATEST_EMPTY''')
submit = json.load(open("$DATA_DIR/submit.json"))
after = json.loads('''$LATEST_AFTER''')
zero = json.loads('''$GET_ZERO''')
out = {
    "ok": True,
    "demo": "boole-block",
    "node": "$NODE_URL",
    "emptyLatest": {"height": empty["height"], "c": empty["c"]},
    "submitted": {
        "accepted": submit["accepted"],
        "height": submit["block"]["height"],
        "c": submit["block"]["c"],
    },
    "latestAfter": {"height": after["height"], "c": after["c"]},
    "byHeightZero": {"height": zero["height"], "c": zero["c"]},
}
print(json.dumps(out, separators=(",", ":")))
PY

echo "boole-block-demo: PASS" >&2
