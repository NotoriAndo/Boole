#!/usr/bin/env bash
# N3.5 — 3-peer local P2P convergence smoke (closed local gate; NOT
# public-network mining). Launches three independently-run boole-node
# processes with a static full-mesh peer list (ADR-0009 (d)), drives one
# share into node 1 and one into node 2, and asserts all three nodes
# reach the IDENTICAL head with zero replay divergence
# (replayMatchesRuntime on every node) — the N3 wave closure criterion.
#
# Convergence exercises the whole N3 line end-to-end: HTTP admission on
# two different nodes, share gossip (N3.2), block announce/pull (N3.3)
# and the initial-sync gap-filler (N3.4).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SCENARIO="fixtures/protocol/runtime-smoke/v1.json"
FIXTURE="fixtures/protocol/runtime-smoke/multiminer.v1.json"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-p2p-convergence.XXXXXX")"

# Build once so the three nodes run the same prebuilt binary instead of
# racing three `cargo run` invocations on the build lock. Inside
# self-test the binary is already built (cargo-test-build stage); a
# standalone run pays the build here.
cargo build -q -p boole-node --locked
NODE_BIN="target/debug/boole-node"

# Six ephemeral ports (3 HTTP + 3 gossip), picked by binding port 0 and
# releasing. The full-mesh peer list requires every gossip address to be
# known before the first node boots, which rules out bind-time port 0.
read -r HTTP1 HTTP2 HTTP3 P2P1 P2P2 P2P3 <<<"$(python3 -c '
import socket
socks = []
for _ in range(6):
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    socks.append(s)
print(" ".join(str(s.getsockname()[1]) for s in socks))
for s in socks:
    s.close()
')"

PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

launch_node() {
  local idx="$1" http_port="$2" p2p_port="$3" peer_a="$4" peer_b="$5"
  # No proof-dedup ledger: the admission-layer dedup cache is orthogonal
  # to what this smoke pins (convergence); dedup has its own gates
  # (no_duplicate_proof_credit + the consensus_proof_dedup suite).
  "$NODE_BIN" run-local \
    --addr "127.0.0.1:${http_port}" \
    --scenario "$SCENARIO" \
    --block-store "$WORK_DIR/node${idx}-blocks.ndjson" \
    --reward-store "$WORK_DIR/node${idx}-rewards.ndjson" \
    --p2p-listen "127.0.0.1:${p2p_port}" \
    --peer "127.0.0.1:${peer_a}" \
    --peer "127.0.0.1:${peer_b}" \
    --lean-checker-disabled \
    --allow-insecure-verifier \
    --allow-anonymous-submit \
    >"$WORK_DIR/node${idx}.out" 2>"$WORK_DIR/node${idx}.err" &
  PIDS+=("$!")
}

launch_node 1 "$HTTP1" "$P2P1" "$P2P2" "$P2P3"
launch_node 2 "$HTTP2" "$P2P2" "$P2P1" "$P2P3"
launch_node 3 "$HTTP3" "$P2P3" "$P2P1" "$P2P2"

python3 - "$FIXTURE" "$HTTP1" "$HTTP2" "$HTTP3" <<'PY'
import http.client
import json
import pathlib
import sys
import time

fixture = json.loads(pathlib.Path(sys.argv[1]).read_text())
ports = [int(p) for p in sys.argv[2:5]]
steps = fixture["steps"]


def request(port, method, path, body=None, timeout=2):
    payload = None if body is None else json.dumps(body).encode()
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    headers = {"Content-Type": "application/json"} if payload is not None else {}
    conn.request(method, path, body=payload, headers=headers)
    res = conn.getresponse()
    raw = res.read().decode()
    if res.status != 200:
        raise SystemExit(f":{port} {method} {path} failed: {res.status} {raw}")
    return json.loads(raw)


def wait_ready(port, deadline_s=60):
    deadline = time.monotonic() + deadline_s
    while time.monotonic() < deadline:
        try:
            if request(port, "GET", "/live").get("ok"):
                return
        except (OSError, SystemExit):
            pass
        time.sleep(0.1)
    raise SystemExit(f"node :{port} never became live")


def wait_converged(expected_height, deadline_s=45):
    """All three nodes at the same height with the same head `c` AND
    replayMatchesRuntime true (the zero-divergence criterion)."""
    deadline = time.monotonic() + deadline_s
    last = None
    while time.monotonic() < deadline:
        try:
            statuses = [request(p, "GET", "/status") for p in ports]
        except (OSError, SystemExit):
            time.sleep(0.2)
            continue
        last = [
            {
                "port": p,
                "height": s.get("height"),
                "c": s.get("c"),
                "replayMatchesRuntime": s.get("replayMatchesRuntime"),
            }
            for p, s in zip(ports, statuses)
        ]
        heights = {s.get("height") for s in statuses}
        heads = {s.get("c") for s in statuses}
        replays = [s.get("replayMatchesRuntime") for s in statuses]
        if heights == {expected_height} and len(heads) == 1 and all(replays):
            return last
        time.sleep(0.2)
    raise SystemExit(
        f"nodes did not converge to height {expected_height}: {json.dumps(last)}"
    )


for port in ports:
    wait_ready(port)

# Share 1 -> node 1 commits block 0; gossip must carry it to nodes 2 and 3.
step0 = steps[0]
sub0 = request(
    ports[0],
    "POST",
    "/submit",
    {"body": step0["body"], "canonTag": step0.get("canonTag", 0), "ts": step0["ts"]},
)
if not sub0.get("accepted") or "block" not in sub0:
    raise SystemExit(f"node1 must commit block 0: {sub0}")
after_block0 = wait_converged(1)

# Share 2 -> node 2 (a DIFFERENT node) commits block 1 on top of the
# gossiped head; all three must converge again.
step1 = steps[1]
body1 = dict(step1["body"])
body1["c"] = sub0["c"]
sub1 = request(
    ports[1],
    "POST",
    "/submit",
    {"body": body1, "canonTag": step1.get("canonTag", 0), "ts": step1["ts"]},
)
if not sub1.get("accepted") or "block" not in sub1:
    raise SystemExit(f"node2 must commit block 1: {sub1}")
after_block1 = wait_converged(2)

heads = {s["c"] for s in after_block1}
print(
    json.dumps(
        {
            "ok": True,
            "kind": "p2p-local-convergence-smoke",
            "claimBoundary": "closed local smoke; not public-network mining",
            "publicMiningEvidence": False,
            "publicScoringEligible": False,
            "ineligibilityReasons": [
                "three local boole-node processes on loopback",
                "fixture shares only",
                "no public network admission",
            ],
            "peers": 3,
            "sharesInjected": 2,
            "injectedInto": ["node1", "node2"],
            "convergedHeight": 2,
            "convergedHead": heads.pop(),
            "replayDivergence": 0,
            "nodesAfterBlock0": after_block0,
            "nodesAfterBlock1": after_block1,
        },
        separators=(",", ":"),
    )
)
PY

printf 'p2p-local-convergence-smoke: PASS\n' >&2
