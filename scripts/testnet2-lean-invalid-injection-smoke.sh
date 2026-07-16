#!/usr/bin/env bash
# SC.10-iv-c — the SC.10 completion gate (L1 master §SC.10 커밋/Full gate,
# promoted recommended -> MANDATORY by the 2026-07-12 third review):
# a structurally-valid but proof-invalid share injected into a
# checker-pinned named network must be adopted by NO honest node.
#
# iv-b proved the POSITIVE path: an honest lean-bound share is re-verified
# by real Lean and accepted. This smoke is the NEGATIVE differential the
# gate was missing: without it the required lane never observes a live
# rejection, so a regression that fail-opened the consensus Lean re-verify
# gate would stay green.
#
# Topology (three boole-testnet-2 nodes, static full mesh):
#   * F  — a FAULTY producer booted `--lean-checker-disabled`
#          `--allow-insecure-verifier`. It boots the SAME named network (so
#          its genesis hash matches and honest peers accept its Hello), but
#          with no checker its own admission Lean gate is inactive — so it
#          admits the tampered share and self-produces a block carrying it,
#          then gossips that block.
#   * H1, H2 — HONEST nodes booted `--lean-checker-dir lean/checker`, i.e.
#          checker-PINNED. The SC.9b pin + executable-toolchain gate and the
#          SC.10-ii ingest Lean re-verify are live for them.
#
# Injection (the mandatory assertion): F self-produces the invalid block and
# gossips it; H1 and H2 re-derive each share's canonical source from its seed,
# recompute the canon and find it != the committed (tampered) package ->
# `ShareEvidenceVerdict::CanonMismatch` -> `BlockReverifyOutcome::Deterministic
# Reject` -> the block is refused at ingest and `boole_p2p_ingress_blocks_
# rejected_total` increments. The rejection is OBSERVED (the counter moved),
# not merely inferred from a block that silently never arrived, and neither
# honest node's head ever becomes the invalid block's `c`.
#
# Honest differential control: an honest lean-bound share (the iv-b fixture)
# driven into H1 self-produces a valid block that BOTH honest nodes adopt
# (H2 re-runs real Lean at ingest and accepts), converging them to height 1.
# This proves the injection was rejected because it is proof-invalid, not
# because the honest network path is broken.
#
# Closed local smoke only; not public-network mining.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# A boole-testnet-2 scenario with a raised per-IP admission quota: on
# loopback every node shares 127.0.0.1, so gossip traffic would otherwise
# exhaust the default quota (1/60s) before the honest control submit lands.
# perIpRateLimitPer60s is a node-local Tier-3 knob (not part of GenesisParams),
# so the genesis identity — and the diverged-genesis refusal — is unchanged.
SCENARIO="fixtures/protocol/runtime-smoke/testnet2-pinned-highrate.v1.json"
HONEST_FIXTURE="fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json"
INVALID_FIXTURE="fixtures/protocol/runtime-smoke/testnet2-lean-invalid.v1.json"
CHECKER_DIR="lean/checker"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-testnet2-lean-invalid.XXXXXX")"

cargo build -q -p boole-node --locked
NODE_BIN="$ROOT/target/debug/boole-node"

# Six ephemeral ports (3 HTTP + 3 gossip): the full-mesh peer list needs
# every gossip address known before the first node boots, so bind port 0
# and release rather than bind-time port 0.
read -r HTTP_F HTTP_H1 HTTP_H2 P2P_F P2P_H1 P2P_H2 <<<"$(python3 -c '
import socket
socks = [socket.socket() for _ in range(6)]
for s in socks:
    s.bind(("127.0.0.1", 0))
print(" ".join(str(s.getsockname()[1]) for s in socks))
for s in socks:
    s.close()
')"

PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" >/dev/null 2>&1 || true
  done
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

# The faulty producer: checker-off on the named network, so its admission
# Lean gate is inactive and it will assemble + gossip the invalid block.
"$NODE_BIN" run-local \
  --addr "127.0.0.1:${HTTP_F}" \
  --scenario "$SCENARIO" \
  --block-store "$WORKDIR/F-blocks.ndjson" \
  --reward-store "$WORKDIR/F-rewards.ndjson" \
  --network-id boole-testnet-2 \
  --lean-checker-disabled \
  --allow-insecure-verifier \
  --allow-anonymous-submit \
  --p2p-listen "127.0.0.1:${P2P_F}" \
  --peer "127.0.0.1:${P2P_H1}" \
  --peer "127.0.0.1:${P2P_H2}" \
  >"$WORKDIR/F.out" 2>"$WORKDIR/F.err" &
PIDS+=("$!")

launch_honest() {
  local name="$1" http="$2" p2p="$3" peer_a="$4" peer_b="$5"
  "$NODE_BIN" run-local \
    --addr "127.0.0.1:${http}" \
    --scenario "$SCENARIO" \
    --block-store "$WORKDIR/${name}-blocks.ndjson" \
    --reward-store "$WORKDIR/${name}-rewards.ndjson" \
    --network-id boole-testnet-2 \
    --lean-checker-dir "$CHECKER_DIR" \
    --allow-anonymous-submit \
    --p2p-listen "127.0.0.1:${p2p}" \
    --peer "127.0.0.1:${peer_a}" \
    --peer "127.0.0.1:${peer_b}" \
    >"$WORKDIR/${name}.out" 2>"$WORKDIR/${name}.err" &
  PIDS+=("$!")
}

launch_honest H1 "$HTTP_H1" "$P2P_H1" "$P2P_F" "$P2P_H2"
launch_honest H2 "$HTTP_H2" "$P2P_H2" "$P2P_F" "$P2P_H1"

python3 - \
  "$HTTP_F" "$HTTP_H1" "$HTTP_H2" \
  "$HONEST_FIXTURE" "$INVALID_FIXTURE" "$WORKDIR" <<'PY'
import http.client
import json
import pathlib
import sys
import time

http_f, http_h1, http_h2 = (int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]))
honest = json.loads(pathlib.Path(sys.argv[4]).read_text())
invalid = json.loads(pathlib.Path(sys.argv[5]).read_text())
workdir = pathlib.Path(sys.argv[6])
HONEST_PORTS = [http_h1, http_h2]


def request(port, method, path, body=None, timeout=15):
    payload = None if body is None else json.dumps(body).encode()
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    headers = {"Content-Type": "application/json"} if payload is not None else {}
    conn.request(method, path, body=payload, headers=headers)
    res = conn.getresponse()
    raw = res.read().decode()
    return res.status, raw


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
    # Checker-pinned boot re-hashes the checker sources and resolves the
    # executable toolchain (`lake env lean`), so allow a generous window.
    deadline = time.monotonic() + deadline_s
    while time.monotonic() < deadline:
        try:
            status, raw = request(port, "GET", "/live", timeout=5)
            if status == 200 and json.loads(raw).get("ok"):
                return
        except (OSError, ValueError):
            pass
        time.sleep(1.0)
    err = (workdir / "F.err").read_text() if port == http_f else ""
    raise SystemExit(f"node :{port} never became live; F.err tail:\n{err[-2000:]}")


for port in (http_f, http_h1, http_h2):
    wait_live(port)

# The honest nodes start with a clean rejection counter; snapshot so the
# assertion proves the invalid block MOVED it (observed rejection).
baseline_rejects = {p: metric(p, "boole_p2p_ingress_blocks_rejected_total") for p in HONEST_PORTS}
for p, v in baseline_rejects.items():
    if v != 0:
        raise SystemExit(f"honest node :{p} started with a nonzero reject counter: {v}")

# ---- Phase 1: injection. The faulty producer admits the tampered share
# (its admission Lean gate is off) and self-produces the invalid block at
# height 0 on the genesis anchor, then gossips it.
inject = request_json(
    http_f, "POST", "/submit",
    {"body": invalid["body"], "canonTag": 0, "ts": int(time.time() * 1000)},
)
if not inject.get("accepted") or "block" not in inject:
    raise SystemExit(f"faulty producer must admit + self-produce the invalid block: {inject}")
bad_head = request_json(http_f, "GET", "/head")
c_bad = bad_head.get("c")
if bad_head.get("height") != 1 or not c_bad:
    raise SystemExit(f"faulty producer head must carry the invalid block: {bad_head}")

# Both honest nodes must OBSERVABLY reject the gossiped invalid block AND
# never adopt it (their height stays 0, their head never becomes c_bad).
deadline = time.monotonic() + 90
rejected = {}
while time.monotonic() < deadline:
    ok = True
    for p in HONEST_PORTS:
        st = request_json(p, "GET", "/status")
        rej = metric(p, "boole_p2p_ingress_blocks_rejected_total")
        rejected[p] = rej
        adopted = st.get("height", 0) != 0 or st.get("c") == c_bad
        if adopted:
            raise SystemExit(
                f"honest node :{p} ADOPTED the invalid block "
                f"(height={st.get('height')}, c={st.get('c')}, c_bad={c_bad})"
            )
        if rej <= baseline_rejects[p]:
            ok = False
    if ok:
        break
    time.sleep(0.5)
else:
    raise SystemExit(
        f"honest nodes never observably rejected the invalid block: "
        f"rejects={rejected}, baseline={baseline_rejects}"
    )

invalid_block_rejected_by_ingest = all(
    rejected[p] > baseline_rejects[p] for p in HONEST_PORTS
)
invalid_block_adopted_by = sum(
    1 for p in HONEST_PORTS
    if request_json(p, "GET", "/status").get("c") == c_bad
)


def checkpoint_height(port):
    # SC.10-iii — the node-local verified-prefix checkpoint height, or None.
    return request_json(port, "GET", "/status").get("verifiedCheckpointHeight")


# SC.10-iii-b — a REJECTED block must not advance any verified-prefix
# checkpoint: both honest nodes refused the injected block, so neither has
# Lean-re-verified anything, so neither has recorded a checkpoint.
checkpoint_after_reject = {p: checkpoint_height(p) for p in HONEST_PORTS}
if any(v is not None for v in checkpoint_after_reject.values()):
    raise SystemExit(
        f"a rejected injection must not advance a checkpoint: {checkpoint_after_reject}"
    )

# ---- Phase 2: honest differential control. An honest lean-bound share into
# H1 self-produces a valid block on the same genesis anchor; H2 re-runs real
# Lean at ingest and accepts, so both honest nodes converge to height 1.
control = request_json(
    http_h1, "POST", "/submit",
    {"body": honest["body"], "canonTag": 0, "ts": int(time.time() * 1000)},
)
if not control.get("accepted") or control.get("height") != 1:
    raise SystemExit(f"honest control share must commit a block on H1: {control}")
c_good = control.get("c")

deadline = time.monotonic() + 150
converged = None
while time.monotonic() < deadline:
    statuses = [request_json(p, "GET", "/status") for p in HONEST_PORTS]
    heights = {s.get("height") for s in statuses}
    heads = {s.get("c") for s in statuses}
    replays = [s.get("replayMatchesRuntime") for s in statuses]
    if heights == {1} and len(heads) == 1 and all(replays) and c_bad not in heads:
        converged = statuses
        break
    time.sleep(0.5)
else:
    raise SystemExit(
        f"honest nodes did not converge on the valid block: "
        f"{json.dumps([request_json(p, 'GET', '/status') for p in HONEST_PORTS])}"
    )

honest_converged_height = converged[0].get("height")
converged_head = converged[0].get("c")
if converged_head != c_good:
    raise SystemExit(
        f"converged head {converged_head} != honest-produced head {c_good}"
    )

# SC.10-iii-b — the INGESTING honest node (H2) re-ran real Lean over the valid
# block at ingest and durably committed it, so its verified-prefix checkpoint
# advanced to height 1. The PRODUCER (H1) self-produced via HTTP submit, which
# is not a Lean re-verify path, so its checkpoint did NOT advance: the
# checkpoint records only what THIS node itself re-verified (ADR-0016 (c)).
ingester_checkpoint = checkpoint_height(http_h2)
producer_checkpoint = checkpoint_height(http_h1)
if ingester_checkpoint != 1:
    raise SystemExit(
        f"ingesting node's checkpoint must advance to 1 after Lean re-verify, "
        f"got {ingester_checkpoint}"
    )
if producer_checkpoint is not None:
    raise SystemExit(
        f"self-producing node must not advance a verified-prefix checkpoint, "
        f"got {producer_checkpoint}"
    )

print(json.dumps({
    "ok": True,
    "kind": "testnet2-lean-invalid-injection-smoke",
    "claimBoundary": "closed local smoke; not public-network mining",
    "publicMiningEvidence": False,
    "publicScoringEligible": False,
    "ineligibilityReasons": [
        "three local boole-node processes on loopback",
        "committed fixture shares only",
        "no public network admission",
    ],
    "networkId": "boole-testnet-2",
    "honestNodes": len(HONEST_PORTS),
    "faultyProducers": 1,
    "invalidBlockAdoptedBy": invalid_block_adopted_by,
    "invalidBlockRejectedByIngest": invalid_block_rejected_by_ingest,
    "honestRejectCounters": rejected,
    "honestConvergedHeight": honest_converged_height,
    "convergedHead": converged_head,
    "invalidHead": c_bad,
    # SC.10-iii-b — verified-prefix checkpoint advance semantics.
    "checkpointAdvancedOnIngest": ingester_checkpoint == 1,
    "checkpointNotAdvancedOnSelfProduce": producer_checkpoint is None,
    "checkpointNotAdvancedOnReject": all(
        v is None for v in checkpoint_after_reject.values()
    ),
    "ingesterCheckpointHeight": ingester_checkpoint,
}, separators=(",", ":")))
PY

printf 'testnet2-lean-invalid-injection-smoke: PASS\n' >&2
