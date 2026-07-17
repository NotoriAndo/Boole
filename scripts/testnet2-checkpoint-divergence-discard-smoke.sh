#!/usr/bin/env bash
# SC.10-iii-d — a verified-prefix checkpoint that no longer matches the actual
# chain must NOT be reused to skip re-verification; it is discarded and the
# blocks are fully re-verified (ADR-0016 (c-1):
# block_store_rollback_cannot_reuse_future_checkpoint). This is the same
# divergence guard the reorg path uses (checkpoint_survives_reorg), exercised
# here through the ingest path because it needs no competing chain.
#
# Same two-node topology as the resync-skip smoke (F checker-off producer, H
# checker-pinned). H Lean-verifies F's block (checkpoint 1). H is stopped, its
# store is wiped, and its checkpoint file is TAMPERED so its block hash points
# at a DIFFERENT prefix (as if a rollback then a divergent chain). On restart H
# re-syncs the real block 0: because the block at the checkpoint height does
# NOT match the tampered checkpoint, H does NOT skip — it discards the
# checkpoint and re-runs the pinned checker, still converging to the real head.
#
# Closed local smoke only; not public-network mining.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENARIO="fixtures/protocol/runtime-smoke/testnet2-pinned-highrate.v1.json"
HONEST_FIXTURE="fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json"
CHECKER_DIR="lean/checker"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-testnet2-checkpoint-diverge.XXXXXX")"

cargo build -q -p boole-node --locked
NODE_BIN="$ROOT/target/debug/boole-node"

read -r HTTP_F HTTP_H P2P_F P2P_H <<<"$(python3 -c '
import socket
socks = [socket.socket() for _ in range(4)]
for s in socks:
    s.bind(("127.0.0.1", 0))
print(" ".join(str(s.getsockname()[1]) for s in socks))
for s in socks:
    s.close()
')"

F_PID=""
cleanup() {
  if [[ -n "$F_PID" ]]; then kill "$F_PID" >/dev/null 2>&1 || true; fi
  pkill -f "127.0.0.1:${P2P_H}" >/dev/null 2>&1 || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

"$NODE_BIN" run-local \
  --addr "127.0.0.1:${HTTP_F}" \
  --scenario "$SCENARIO" \
  --block-store "$WORKDIR/F-blocks.ndjson" \
  --reward-store "$WORKDIR/F-rewards.ndjson" \
  --network-id boole-testnet-2 \
  --lean-checker-disabled --allow-insecure-verifier --allow-anonymous-submit \
  --p2p-listen "127.0.0.1:${P2P_F}" --peer "127.0.0.1:${P2P_H}" \
  >"$WORKDIR/F.out" 2>"$WORKDIR/F.err" &
F_PID="$!"

H_CMD=(
  "$NODE_BIN" run-local
  --addr "127.0.0.1:${HTTP_H}"
  --scenario "$SCENARIO"
  --block-store "$WORKDIR/H-blocks.ndjson"
  --reward-store "$WORKDIR/H-rewards.ndjson"
  --network-id boole-testnet-2
  --lean-checker-dir "$CHECKER_DIR"
  --allow-anonymous-submit
  --p2p-listen "127.0.0.1:${P2P_H}"
  --peer "127.0.0.1:${P2P_F}"
)

python3 - "$HTTP_F" "$HTTP_H" "$HONEST_FIXTURE" "$WORKDIR" "${H_CMD[@]}" <<'PY'
import http.client
import json
import pathlib
import signal
import subprocess
import sys
import time

http_f, http_h = int(sys.argv[1]), int(sys.argv[2])
honest = json.loads(pathlib.Path(sys.argv[3]).read_text())
workdir = pathlib.Path(sys.argv[4])
h_cmd = sys.argv[5:]
SKIP_METRIC = "boole_p2p_ingress_blocks_reverify_skipped_via_checkpoint_total"

h_blocks = workdir / "H-blocks.ndjson"
h_rewards = workdir / "H-rewards.ndjson"
h_checkpoint = workdir / "H-blocks.checkpoint.json"


def request(port, method, path, body=None, timeout=15):
    payload = None if body is None else json.dumps(body).encode()
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    headers = {"Content-Type": "application/json"} if payload is not None else {}
    conn.request(method, path, body=payload, headers=headers)
    res = conn.getresponse()
    return res.status, res.read().decode()


def request_json(port, method, path, body=None, timeout=15):
    status, raw = request(port, method, path, body=body, timeout=timeout)
    if status != 200:
        raise SystemExit(f":{port} {method} {path} -> {status} {raw}")
    return json.loads(raw)


def metric(port, name):
    status, raw = request(port, "GET", "/metrics")
    if status != 200:
        return 0
    for line in raw.splitlines():
        if line.startswith(name + " "):
            return int(line.split()[1])
    return 0


def wait_live(port, deadline_s=200):
    deadline = time.monotonic() + deadline_s
    while time.monotonic() < deadline:
        try:
            status, raw = request(port, "GET", "/live", timeout=5)
            if status == 200 and json.loads(raw).get("ok"):
                return
        except (OSError, ValueError):
            pass
        time.sleep(1.0)
    raise SystemExit(f"node :{port} never became live")


def launch_h():
    return subprocess.Popen(
        h_cmd,
        stdout=open(workdir / "H.out", "a"),
        stderr=open(workdir / "H.err", "a"),
    )


def stop(proc):
    proc.send_signal(signal.SIGTERM)
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


wait_live(http_f)
h_proc = launch_h()
try:
    wait_live(http_h)
    submit = request_json(http_f, "POST", "/submit", {
        "body": honest["body"], "canonTag": 0, "ts": int(time.time() * 1000),
    })
    if not submit.get("accepted") or submit.get("height") != 1:
        raise SystemExit(f"producer must commit block 0: {submit}")
    c_good = submit.get("c")

    deadline = time.monotonic() + 120
    ready = False
    while time.monotonic() < deadline:
        st = request_json(http_h, "GET", "/status")
        if st.get("height") == 1 and st.get("c") == c_good \
                and st.get("verifiedCheckpointHeight") == 1:
            ready = True
            break
        time.sleep(0.5)
    if not ready:
        raise SystemExit(
            f"H must ingest + Lean-verify block 0 and checkpoint to 1: "
            f"{request_json(http_h, 'GET', '/status')}"
        )
finally:
    stop(h_proc)

# Wipe the store, KEEP but TAMPER the checkpoint: point its block hash at a
# DIFFERENT prefix (as if a rollback then a divergent chain re-grew).
if not h_checkpoint.exists():
    raise SystemExit("H's checkpoint file must exist after the verified ingest")
checkpoint = json.loads(h_checkpoint.read_text())
original_hash = checkpoint["block_hash"]
tampered_hash = ("f" * len(original_hash)) if original_hash != ("f" * len(original_hash)) \
    else ("0" * len(original_hash))
checkpoint["block_hash"] = tampered_hash
h_checkpoint.write_text(json.dumps(checkpoint))
h_blocks.unlink()
h_rewards.unlink()

# Restart H empty with the TAMPERED checkpoint and re-sync the real block 0.
# The block at the checkpoint height will NOT match the tampered hash, so H
# must NOT skip: it discards the checkpoint and re-runs Lean, still converging.
h_proc = launch_h()
try:
    wait_live(http_h)
    deadline = time.monotonic() + 150
    resynced = None
    while time.monotonic() < deadline:
        st = request_json(http_h, "GET", "/status")
        if st.get("height") == 1 and st.get("c") == c_good:
            resynced = st
            break
        time.sleep(0.5)
    if resynced is None:
        raise SystemExit(
            f"H must re-verify + adopt the real block despite the tampered "
            f"checkpoint: {request_json(http_h, 'GET', '/status')}"
        )
    skip_after_resync = metric(http_h, SKIP_METRIC)
    if skip_after_resync != 0:
        raise SystemExit(
            f"a divergent checkpoint must NOT be reused to skip; skip counter "
            f"should stay 0, got {skip_after_resync}"
        )
finally:
    stop(h_proc)

print(json.dumps({
    "ok": True,
    "kind": "testnet2-checkpoint-divergence-discard-smoke",
    "claimBoundary": "closed local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "two local boole-node processes on loopback",
        "committed fixture share only",
        "no public network admission",
    ],
    "networkId": "boole-testnet-2",
    "checkpointTampered": True,
    "skipCounterAfterResync": skip_after_resync,
    "divergentCheckpointNotReused": skip_after_resync == 0,
    "reverifiedNotSkipped": skip_after_resync == 0,
    "resyncedHeight": resynced.get("height"),
    "resyncedHead": resynced.get("c"),
    "convergedToRealHead": resynced.get("c") == c_good,
}, separators=(",", ":")))
PY

printf 'testnet2-checkpoint-divergence-discard-smoke: PASS\n' >&2
