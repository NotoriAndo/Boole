#!/usr/bin/env bash
# SC.10-iv-b — first live boot of a checker-pinned named network.
#
# Positive: a node booted as `boole-testnet-2` with the canonical
# `lean/checker` and a scenario whose Tier-1 params match the compiled
# preset must come up: the boot runs the SC.9b checker-pin gate for real
# (source artifact hash AND the executable toolchain via `lake env lean`),
# then the N5.2 genesis gate (possible at all only since SC.10-iv-0 made
# `genesis_spec` adopt the preset's identity fields), and `/ready` must be
# 200 because a Lean checker is configured.
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
CHECKER_DIR="lean/checker"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-testnet2-pinned-boot.XXXXXX")"

cargo build -q -p boole-node
NODE_BIN="$ROOT/target/debug/boole-node"

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

python3 - "$ADDR" "$NODE_PID" "$WORKDIR" <<'PY'
import http.client
import json
import pathlib
import sys
import time

addr, node_pid, workdir = sys.argv[1], int(sys.argv[2]), pathlib.Path(sys.argv[3])
host, port_raw = addr.rsplit(":", 1)
port = int(port_raw)

def request(path):
    conn = http.client.HTTPConnection(host, port, timeout=5)
    conn.request("GET", path)
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
        status, live = request("/live")
        if status == 200 and live.get("ok"):
            break
    except OSError:
        pass
    time.sleep(1.0)
else:
    err = (workdir / "node.err").read_text()
    raise SystemExit(f"pinned node never became live; stderr:\n{err}")

ready_status, ready = request("/ready")
if ready_status != 200 or not ready.get("ok"):
    raise SystemExit(f"checker-pinned node must be ready, got {ready_status}: {ready}")

status_code, status = request("/status")
if status_code != 200 or status.get("height") != 0 or not status.get("replayMatchesRuntime"):
    raise SystemExit(f"bad pinned node status: {status_code} {status}")

print(json.dumps({
    "ok": True,
    "kind": "testnet2-pinned-boot-smoke",
    "claimBoundary": "closed local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "single local boole-node process",
        "no shares submitted",
        "no public network admission",
    ],
    "networkId": "boole-testnet-2",
    "checkerPinned": True,
    "toolchainVerifiedAtBoot": True,
    "ready": True,
    "height": status.get("height"),
    "bootRefusedOnDivergedGenesis": True,
}, separators=(",", ":")))
PY

kill "$NODE_PID" >/dev/null 2>&1 || true
wait "$NODE_PID" 2>/dev/null || true
NODE_PID=""
printf 'testnet2-pinned-boot-smoke: PASS\n' >&2
