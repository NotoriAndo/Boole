#!/usr/bin/env bash
set -euo pipefail

# P2.10 criterion 1 — closed-local testnet faucet → block smoke.
#
# Exercises the testnet-preset path end to end as a CLOSED LOCAL smoke
# (NOT a public-network claim):
#
#   1. spins up a single `boole-node run-local` pinned to
#      `--network-id boole-testnet` with the Lean checker explicitly
#      disabled (testnet operator acknowledgement, so `/ready` does not
#      503 on a missing checker dir);
#   2. spins up a one-shot mock faucet HTTP server on an ephemeral port;
#   3. runs `boole faucet claim --network testnet --faucet-url <mock>`
#      and asserts the unified P2.5 envelope plus the canonical
#      `network_id=boole-testnet` the CLI POSTed to the faucet;
#   4. drives the runtime-smoke scenario through `/ticket` + `/submit`
#      to append at least one block, mirroring `local-mining-smoke.sh`
#      (including its `--allow-anonymous-submit` opt-in: the scenario's
#      `/submit` candidates carry no agent-wallet `session` block, so
#      since N2.1 they need the anonymous opt-in or the node's secure
#      default rejects them `401 unauthenticated_submit`);
#   5. asserts the node head advanced and prints a JSON transcript whose
#      `claimBoundary` makes the closed-local scope explicit.
#
# The faucet is purely client-side (a thin HTTP POST), so the smoke
# stands up its own mock faucet rather than reaching any external
# service. No public network is contacted and no mining productivity is
# claimed.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ADDR="${BOOLE_NODE_ADDR:-127.0.0.1:18090}"
# N3-pre.5 — this smoke is the only one that boots with --state-dir +
# --proof-dedup-ledger, so its two /submit steps need distinct proof
# `bytes` (the dedup ledger keys on SHA-256(bytes) cross-pk). The shared
# runtime-smoke/v1.json fixture intentionally reuses one `bytes` value
# across steps (dozens of other tests/scripts depend on that), so this
# smoke uses its own dedicated copy instead of mutating the shared one.
SCENARIO="${SCENARIO:-fixtures/protocol/runtime-smoke/faucet-smoke.v1.json}"
NETWORK_ID="boole-testnet"
ADDRESS_HEX="${FAUCET_ADDRESS_HEX:-1111111111111111111111111111111111111111111111111111111111111111}"
BLOCK_STORE="${BLOCK_STORE:-${TMPDIR:-/tmp}/boole-node-faucet-smoke.ndjson}"
# Pin the reward ledger to a smoke-specific path so the run cannot
# inherit a stale ledger left by an earlier self-test stage and bail at
# boot with `reward ledger divergence`.
REWARD_LEDGER="${REWARD_LEDGER:-${TMPDIR:-/tmp}/boole-node-faucet-smoke-rewards.ndjson}"
# `--network-id` only takes effect with an opt-in state dir, so pin one
# to a smoke-specific temp path and pre-clean it.
STATE_DIR="${STATE_DIR:-${TMPDIR:-/tmp}/boole-node-faucet-smoke-state}"
# N3-pre.5 — production posture (`--state-dir` set) now requires the
# cross-pk proof-dedup ledger for `/ready`; this is the only smoke that
# boots with `--state-dir`, so it is the one that must pass the flag.
PROOF_DEDUP_LEDGER="${PROOF_DEDUP_LEDGER:-${TMPDIR:-/tmp}/boole-node-faucet-smoke-proof-dedup.ndjson}"
FAUCET_OUT="${TMPDIR:-/tmp}/boole-faucet-smoke-mock.out"
NODE_OUT="${TMPDIR:-/tmp}/boole-node-faucet-smoke.out"
NODE_ERR="${TMPDIR:-/tmp}/boole-node-faucet-smoke.err"
rm -f "$BLOCK_STORE" "$REWARD_LEDGER" "$PROOF_DEDUP_LEDGER" "$FAUCET_OUT" "$NODE_OUT" "$NODE_ERR"
rm -rf "$STATE_DIR"

# --- mock faucet -----------------------------------------------------
# One-shot HTTP server: accepts a single POST /claim, records the body
# (so the smoke can assert the canonical network_id the CLI sent) and
# replies with a queued receipt. Binds an ephemeral port and writes the
# chosen port + captured body to $FAUCET_OUT.
python3 - "$FAUCET_OUT" <<'PY' &
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

out_path = sys.argv[1]
captured = {}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b""
        try:
            captured["body"] = json.loads(raw.decode() or "{}")
        except Exception:
            captured["body"] = {"_raw": raw.decode(errors="replace")}
        captured["path"] = self.path
        body = json.dumps({"status": "queued", "tx_id": "smoke-faucet-tx-001"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


server = HTTPServer(("127.0.0.1", 0), Handler)
port = server.server_address[1]
with open(out_path, "w") as fh:
    fh.write(json.dumps({"port": port}) + "\n")
    fh.flush()
# Serve exactly one request, then persist what we captured.
server.handle_request()
with open(out_path, "a") as fh:
    fh.write(json.dumps({"captured": captured}) + "\n")
    fh.flush()
PY
FAUCET_PID=$!

# --- node ------------------------------------------------------------
cargo run -q -p boole-node -- run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$BLOCK_STORE" \
  --reward-store "$REWARD_LEDGER" \
  --state-dir "$STATE_DIR" \
  --proof-dedup-ledger "$PROOF_DEDUP_LEDGER" \
  --network-id "$NETWORK_ID" \
  --lean-checker-disabled \
  --allow-insecure-verifier \
  --allow-anonymous-submit \
  --max-requests 9 \
  >"$NODE_OUT" 2>"$NODE_ERR" &
NODE_PID=$!

trap 'kill "$NODE_PID" "$FAUCET_PID" >/dev/null 2>&1 || true; rm -f "$NODE_OUT" "$NODE_ERR" "$FAUCET_OUT" "$BLOCK_STORE" "$REWARD_LEDGER" "$PROOF_DEDUP_LEDGER"; rm -rf "$STATE_DIR"' EXIT

# Wait for the mock faucet to publish its ephemeral port.
FAUCET_URL=""
for _ in $(seq 1 100); do
  if [ -s "$FAUCET_OUT" ]; then
    PORT="$(python3 -c "import json,sys; print(json.loads(open('$FAUCET_OUT').readline())['port'])" 2>/dev/null || true)"
    if [ -n "$PORT" ]; then
      FAUCET_URL="http://127.0.0.1:${PORT}/claim"
      break
    fi
  fi
  sleep 0.05
done
if [ -z "$FAUCET_URL" ]; then
  echo "smoke-testnet-faucet-to-block: FAIL (mock faucet did not start)" >&2
  exit 1
fi

# --- faucet claim ----------------------------------------------------
CLAIM_JSON="$(cargo run -q -p boole-cli -- faucet claim \
  --network testnet \
  --address "$ADDRESS_HEX" \
  --faucet-url "$FAUCET_URL" \
  --json)"

# Give the mock faucet a beat to flush the captured body to $FAUCET_OUT.
for _ in $(seq 1 100); do
  LINES="$(wc -l <"$FAUCET_OUT" 2>/dev/null || echo 0)"
  if [ "${LINES:-0}" -ge 2 ]; then
    break
  fi
  sleep 0.05
done

# --- assertions + transcript ----------------------------------------
ADDR="$ADDR" SCENARIO="$SCENARIO" NETWORK_ID="$NETWORK_ID" \
ADDRESS_HEX="$ADDRESS_HEX" FAUCET_OUT="$FAUCET_OUT" CLAIM_JSON="$CLAIM_JSON" \
python3 - <<'PY'
import http.client
import json
import os
import pathlib
import sys
import time

addr = os.environ["ADDR"]
scenario_path = pathlib.Path(os.environ["SCENARIO"])
network_id = os.environ["NETWORK_ID"]
address_hex = os.environ["ADDRESS_HEX"]
faucet_out = pathlib.Path(os.environ["FAUCET_OUT"])
claim_json = os.environ["CLAIM_JSON"]

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


# 1. Faucet claim envelope assertions.
claim = json.loads(claim_json)
if claim.get("ok") is not True:
    raise SystemExit(f"faucet claim envelope not ok: {claim}")
if claim.get("command") != "faucet.claim":
    raise SystemExit(f"faucet claim command must be 'faucet.claim': {claim}")
result = claim.get("result") or {}
if result.get("status") != "queued":
    raise SystemExit(f"faucet result.status must be 'queued': {claim}")

# 2. The faucet body the CLI POSTed must carry the canonical network_id.
lines = [ln for ln in faucet_out.read_text().splitlines() if ln.strip()]
captured = {}
for ln in lines:
    obj = json.loads(ln)
    if "captured" in obj:
        captured = obj["captured"]
body = captured.get("body") or {}
if body.get("network_id") != network_id:
    raise SystemExit(
        f"faucet body must carry canonical network_id={network_id}; got {body}"
    )
if body.get("address") != address_hex:
    raise SystemExit(f"faucet body must echo the address; got {body}")

# 3. Drive the scenario to append at least one block (mirrors
#    local-mining-smoke.sh): the closed-local block-append path.
scenario = json.loads(scenario_path.read_text())
steps = scenario["steps"]
initial_head = request("GET", "/head", attempts=50)
if not initial_head.get("ok") or initial_head.get("height") != 0:
    raise SystemExit(f"bad initial head: {initial_head}")
# Mirror local-mining-smoke.sh's request count exactly: GET /head +
# GET /config + per-step (ticket+submit+head)×N + GET /status sums to
# the node's `--max-requests` so it self-terminates and `wait` returns.
config = request("GET", "/config")
if not config.get("ok") or not config.get("T_share"):
    raise SystemExit(f"bad node config: {config}")

mined = []
head = initial_head
for i, step in enumerate(steps):
    body_step = dict(step["body"])
    if step.get("cFromRuntimeHead"):
        body_step["c"] = head["c"]
    candidate = {
        "body": body_step,
        "ip": step.get("ip", f"192.0.2.{10 + i}"),
        "canonTag": step.get("canonTag", 0),
        "ts": step.get("ts", 1800000000000 + i),
    }
    ticket = request("POST", "/ticket", {"c": body_step["c"], "pk": body_step["pk"], "n": body_step["n"]})
    if not ticket.get("ok") or len(ticket.get("hashHex", "")) != 64:
        raise SystemExit(f"bad ticket at step {i}: {ticket}")
    submit = request("POST", "/submit", candidate)
    if not submit.get("accepted"):
        raise SystemExit(f"submit rejected at step {i}: {submit}")
    if not submit.get("replayMatchesRuntime"):
        raise SystemExit(f"submit diverged at step {i}: {submit}")
    head = request("GET", "/head")
    expected_height = i + 1
    if head.get("height") != expected_height:
        raise SystemExit(f"bad head after step {i}: expected {expected_height}, got {head}")
    mined.append({
        "step": i,
        "blockHeight": submit["block"]["height"],
        "c": submit["block"]["c"],
        "replayMatchesRuntime": submit["replayMatchesRuntime"],
    })

if len(mined) < 1:
    raise SystemExit("smoke must append at least one block")

status = request("GET", "/status")
if status.get("height") != len(steps) or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad final status: {status}")

print(json.dumps({
    "ok": True,
    "kind": "smoke-testnet-faucet-to-block",
    "claimBoundary": "closed local smoke; not public-network claim",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "single local boole-node process",
        "mock fixture faucet only",
        "no public network admission",
    ],
    "networkId": network_id,
    "faucetClaim": {
        "ok": True,
        "address": address_hex,
        "networkId": body.get("network_id"),
        "status": result.get("status"),
        "txId": result.get("tx_id"),
    },
    "node": "boole-node run-local --network-id boole-testnet --lean-checker-disabled --allow-insecure-verifier",
    "blocksMined": len(mined),
    "initialHead": initial_head,
    "finalHead": head,
    "status": status,
    "mined": mined,
}, separators=(",", ":")))
PY

wait "$NODE_PID" >/dev/null 2>&1 || true
printf 'smoke-testnet-faucet-to-block: PASS\n' >&2
