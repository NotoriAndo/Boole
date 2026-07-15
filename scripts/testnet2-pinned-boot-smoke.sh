#!/usr/bin/env bash
# SC.10-iv-b — live checker-pinned named-network coverage in the gate.
#
# Positive: a node booted as `boole-testnet-2` with the canonical
# `lean/checker` and a scenario whose Tier-1 params match the compiled
# preset must come up: the boot runs the SC.9b checker-pin gate for real
# (source artifact hash AND the executable toolchain via `lake env lean`),
# then the N5.2 genesis gate (possible at all only since SC.10-iv-0 made
# `genesis_spec` adopt the preset's identity fields), and `/ready` must be
# 200 because a Lean checker is configured.
#
# Live Lean: the committed lean-bound share fixture (pinned to the
# consensus generator by testnet2_lenbound_fixture.rs) is driven into the
# pinned node, must commit block 1, and the strict CLI deep verify
# (`state verify --deep`, with no skip opt-out) must then re-derive the
# share from its seed and re-run the pinned checker: `leanReverified == 1`
# with ZERO skips is the machine evidence that real Lean executed in this
# lane (the SC.10 "no silent skip-green" gate condition).
#
# Negative (differential control): the same launch with the plain
# runtime-smoke scenario — whose effective genesis diverges from the preset
# (no retarget schedule) — must REFUSE to boot naming the genesis gate, so
# this smoke also proves the preset-aware change did not soften the gate.
#
# Closed local smoke only; not public-network mining.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENARIO="fixtures/protocol/runtime-smoke/testnet2-pinned.v1.json"
DIVERGED_SCENARIO="fixtures/protocol/runtime-smoke/v1.json"
SHARE_FIXTURE="fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json"
CHECKER_DIR="lean/checker"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-testnet2-pinned-boot.XXXXXX")"

cargo build -q -p boole-node -p boole-cli
NODE_BIN="$ROOT/target/debug/boole-node"
CLI_BIN="$ROOT/target/debug/boole-cli"

PORT="$(python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
)"
ADDR="127.0.0.1:${PORT}"

NODE_PID=""
cleanup() {
  if [[ -n "$NODE_PID" ]]; then kill "$NODE_PID" >/dev/null 2>&1 || true; fi
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

"$NODE_BIN" run-local \
  --addr "$ADDR" \
  --scenario "$SCENARIO" \
  --block-store "$WORKDIR/blocks.ndjson" \
  --reward-store "$WORKDIR/rewards.ndjson" \
  --network-id boole-testnet-2 \
  --lean-checker-dir "$CHECKER_DIR" \
  --allow-anonymous-submit \
  >"$WORKDIR/node.out" 2>"$WORKDIR/node.err" &
NODE_PID=$!

# Negative control runs while the pinned node finishes its (toolchain-
# checking) boot: the diverged scenario must exit non-zero and name the
# genesis gate.
set +e
"$NODE_BIN" run-local \
  --addr "127.0.0.1:0" \
  --scenario "$DIVERGED_SCENARIO" \
  --block-store "$WORKDIR/diverged-blocks.ndjson" \
  --reward-store "$WORKDIR/diverged-rewards.ndjson" \
  --network-id boole-testnet-2 \
  --lean-checker-dir "$CHECKER_DIR" \
  >"$WORKDIR/diverged.out" 2>"$WORKDIR/diverged.err"
DIVERGED_EXIT=$?
set -e
if [[ "$DIVERGED_EXIT" -eq 0 ]]; then
  echo "diverged-genesis control must refuse to boot, but it exited 0" >&2
  exit 1
fi
if ! grep -q "refusing to boot a diverged genesis" "$WORKDIR/diverged.err"; then
  echo "diverged-genesis refusal must name the genesis gate; stderr was:" >&2
  cat "$WORKDIR/diverged.err" >&2
  exit 1
fi

python3 - "$ADDR" "$NODE_PID" "$WORKDIR" "$SHARE_FIXTURE" <<'PY'
import http.client
import json
import pathlib
import sys
import time

addr, node_pid, workdir = sys.argv[1], int(sys.argv[2]), pathlib.Path(sys.argv[3])
share_fixture = json.loads(pathlib.Path(sys.argv[4]).read_text())
host, port_raw = addr.rsplit(":", 1)
port = int(port_raw)

def request(method, path, body=None):
    payload = None if body is None else json.dumps(body).encode()
    conn = http.client.HTTPConnection(host, port, timeout=10)
    headers = {"Content-Type": "application/json"} if payload is not None else {}
    conn.request(method, path, body=payload, headers=headers)
    res = conn.getresponse()
    return res.status, json.loads(res.read().decode())

def node_died():
    try:
        import os
        os.kill(node_pid, 0)
        return False
    except OSError:
        return True

# The pinned boot re-hashes the checker sources and resolves the
# executable toolchain (`lake env lean`), so allow a generous window.
deadline = time.time() + 180
live = None
while time.time() < deadline:
    if node_died():
        err = (workdir / "node.err").read_text()
        raise SystemExit(f"pinned node exited during boot; stderr:\n{err}")
    try:
        status, live = request("GET", "/live")
        if status == 200 and live.get("ok"):
            break
    except OSError:
        pass
    time.sleep(1.0)
else:
    err = (workdir / "node.err").read_text()
    raise SystemExit(f"pinned node never became live; stderr:\n{err}")

ready_status, ready = request("GET", "/ready")
if ready_status != 200 or not ready.get("ok"):
    raise SystemExit(f"checker-pinned node must be ready, got {ready_status}: {ready}")

status_code, status = request("GET", "/status")
if status_code != 200 or status.get("height") != 0 or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad pinned node status: {status_code} {status}")

# Live lean-bound flow: the committed fixture share (seed-bound canon
# against the canonical checker) must clear structural admission and
# commit block 1 on the pinned network (seed binding is REQUIRED here).
# The envelope ts must be the real wall clock: the committed block's ts
# is checked against the N3-pre.3 future-drift bound.
submit_status, submit = request("POST", "/submit", {
    "body": share_fixture["body"],
    "canonTag": 0,
    "ts": int(time.time() * 1000),
})
if submit_status != 200 or not submit.get("accepted"):
    raise SystemExit(f"lean-bound share must be admitted, got {submit_status}: {submit}")
if not submit.get("replayMatchesRuntime"):
    raise SystemExit(f"commit replay diverged: {submit}")
if submit.get("height") != 1 or submit.get("block", {}).get("height") != 0:
    raise SystemExit(f"lean-bound share must commit the first block: {submit}")

head_status, head = request("GET", "/head")
if head_status != 200 or head.get("height") != 1:
    raise SystemExit(f"pinned node head must be 1 after commit: {head_status} {head}")
PY

kill "$NODE_PID" >/dev/null 2>&1 || true
wait "$NODE_PID" 2>/dev/null || true
NODE_PID=""

# Strict deep verify (SC.10-i semantics, no skip opt-out): re-derive the
# committed share from its seed and RE-RUN the pinned checker — the live
# Lean execution this lane must prove. An empty bounty ledger keeps the
# bounty side at zero events (and zero skips).
: > "$WORKDIR/bounty-events.ndjson"
"$CLI_BIN" state verify --deep \
  --bounty-events "$WORKDIR/bounty-events.ndjson" \
  --blocks "$WORKDIR/blocks.ndjson" \
  --lean-checker-dir "$CHECKER_DIR" \
  --json >"$WORKDIR/deep-verify.json"

python3 - "$WORKDIR/deep-verify.json" <<'PY'
import json
import sys

deep = json.load(open(sys.argv[1]))
checks = {
    "ok": deep.get("ok") is True,
    "leanBoundShares": deep.get("leanBoundShares") == 1,
    "canonReverified": deep.get("canonReverified") == 1,
    "leanReverified": deep.get("leanReverified") == 1,
    "sharesSkipped": deep.get("sharesSkipped") == 0,
    "leanProofsSkipped": deep.get("leanProofsSkipped") == 0,
}
if not all(checks.values()):
    raise SystemExit(f"strict deep verify must prove live Lean ran: {checks}; envelope: {deep}")

print(json.dumps({
    "ok": True,
    "kind": "testnet2-pinned-boot-smoke",
    "claimBoundary": "closed local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "single local boole-node process",
        "committed fixture share only",
        "no public network admission",
    ],
    "networkId": "boole-testnet-2",
    "checkerPinned": True,
    "toolchainVerifiedAtBoot": True,
    "ready": True,
    "height": 1,
    "bootRefusedOnDivergedGenesis": True,
    "leanBoundShares": deep.get("leanBoundShares"),
    "canonReverified": deep.get("canonReverified"),
    "leanReverified": deep.get("leanReverified"),
    "sharesSkipped": deep.get("sharesSkipped"),
    "leanProofsSkipped": deep.get("leanProofsSkipped"),
}, separators=(",", ":")))
PY

printf 'testnet2-pinned-boot-smoke: PASS\n' >&2
