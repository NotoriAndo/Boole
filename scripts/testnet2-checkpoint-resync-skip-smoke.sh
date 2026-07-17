#!/usr/bin/env bash
# SC.10-iii-c-2 — verified-prefix checkpoint `assumevalid` re-sync skip.
#
# Proves the cost-curve payoff of the checkpoint: a node that has itself
# Lean-re-verified a prefix, then re-bootstraps (block store wiped, checkpoint
# kept), re-syncs that prefix WITHOUT re-running the pinned checker —
# structural replay still runs, but the expensive Lean step is skipped for the
# trusted prefix (ADR-0016 (c), Bitcoin assumevalid shape).
#
# Topology (two boole-testnet-2 nodes):
#   * F — a checker-off producer (`--lean-checker-disabled`) that self-produces
#         a valid block from the committed honest fixture and gossips it.
#   * H — a checker-PINNED honest node. On first ingest it re-runs real Lean
#         over F's block and, on success, advances its verified-prefix
#         checkpoint (SC.10-iii-b): checkpoint height 1, skip counter 0.
#
# Then H is stopped, its block + reward store are WIPED (the checkpoint file is
# kept), and H is restarted empty. It re-syncs block 0 from F: the block falls
# within its trusted checkpoint prefix, so H adopts it WITHOUT running Lean —
# the skip counter goes 0 -> 1 and H re-converges to the same head.
#
# Closed local smoke only; not public-network mining.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENARIO="fixtures/protocol/runtime-smoke/testnet2-pinned-highrate.v1.json"
HONEST_FIXTURE="fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json"
CHECKER_DIR="lean/checker"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-testnet2-checkpoint-resync.XXXXXX")"

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
  # Best-effort: kill any lingering node bound to H's port (Python owns H, but
  # a hard failure could leak it).
  pkill -f "127.0.0.1:${P2P_H}" >/dev/null 2>&1 || true
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

# F: checker-off producer on the named network.
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

# H's launch command — Python owns its lifecycle (start, stop, restart) so it
# can wipe the store between runs while keeping the checkpoint.
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

    # --- Phase 1: F produces a valid block; H ingests it, runs REAL Lean, and
    # advances its verified-prefix checkpoint to height 1 (no skip yet).
    submit = request_json(http_f, "POST", "/submit", {
        "body": honest["body"], "canonTag": 0, "ts": int(time.time() * 1000),
    })
    if not submit.get("accepted") or submit.get("height") != 1:
        raise SystemExit(f"producer must commit block 0: {submit}")
    c_good = submit.get("c")

    deadline = time.monotonic() + 120
    first = None
    while time.monotonic() < deadline:
        st = request_json(http_h, "GET", "/status")
        if st.get("height") == 1 and st.get("c") == c_good \
                and st.get("verifiedCheckpointHeight") == 1:
            first = st
            break
        time.sleep(0.5)
    if first is None:
        raise SystemExit(
            f"H must ingest + Lean-verify block 0 and checkpoint to 1: "
            f"{request_json(http_h, 'GET', '/status')}"
        )
    skip_after_first_ingest = metric(http_h, SKIP_METRIC)
    if skip_after_first_ingest != 0:
        raise SystemExit(
            f"first ingest must RUN Lean (skip counter 0), got {skip_after_first_ingest}"
        )
finally:
    stop(h_proc)

# --- Phase 2: H is stopped; wipe its block + reward store, KEEP the checkpoint.
if not h_checkpoint.exists():
    raise SystemExit("H's checkpoint file must exist after the verified ingest")
checkpoint_kept = json.loads(h_checkpoint.read_text())
h_blocks.unlink()
h_rewards.unlink()
if not h_checkpoint.exists():
    raise SystemExit("checkpoint file must survive the store wipe")

# --- Phase 3: restart H empty (checkpoint retained) and re-sync from F. The
# re-synced block 0 falls within the trusted prefix, so H SKIPS the Lean
# re-verify: the skip counter goes to 1 and H re-converges to the same head.
h_proc = launch_h()
try:
    wait_live(http_h)
    deadline = time.monotonic() + 150
    resynced = None
    while time.monotonic() < deadline:
        st = request_json(http_h, "GET", "/status")
        skip = metric(http_h, SKIP_METRIC)
        if st.get("height") == 1 and st.get("c") == c_good and skip >= 1:
            resynced = (st, skip)
            break
        time.sleep(0.5)
    if resynced is None:
        raise SystemExit(
            f"H must re-sync block 0 with a Lean SKIP: "
            f"status={request_json(http_h, 'GET', '/status')}, "
            f"skip={metric(http_h, SKIP_METRIC)}"
        )
    resynced_status, skip_after_resync = resynced
finally:
    stop(h_proc)

print(json.dumps({
    "ok": True,
    "kind": "testnet2-checkpoint-resync-skip-smoke",
    "claimBoundary": "closed local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "two local boole-node processes on loopback",
        "committed fixture share only",
        "no public network admission",
    ],
    "networkId": "boole-testnet-2",
    "checkpointHeightBeforeRestart": checkpoint_kept.get("height"),
    "skipCounterAfterFirstIngest": skip_after_first_ingest,
    "skipCounterAfterResync": skip_after_resync,
    "reverifySkippedOnResync": skip_after_resync >= 1,
    "resyncedHeight": resynced_status.get("height"),
    "resyncedHead": resynced_status.get("c"),
    "headMatchesFirstVerified": resynced_status.get("c") == c_good,
}, separators=(",", ":")))
PY

printf 'testnet2-checkpoint-resync-skip-smoke: PASS\n' >&2
