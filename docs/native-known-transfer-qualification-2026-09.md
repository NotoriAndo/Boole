# Native exact-transfer retry qualification — 2026-09-23

Status: **PREREGISTERED — baseline pending.** This work targets repeated
immutable signature checks for an exact transfer already retained in the
verified pending or confirmed index. It grants no new spending, nonce, signature
or network authority and adds no caller-supplied trust/cache key.

## Fixed scope and bounds

Use disposable deterministic keys/state and loopback only, on the existing
developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), Rust 1.95 debug.
Run explicit ignored measurements without other local build/test load. Keep
source/executable identity, baseline failure and every later result. These
criteria cannot change after their measurement. Ordinary correctness tests run
in CI; machine-specific timings do not become a cross-platform CI SLA.

1. Direct retry scenario: mine ten empty blocks, then durably admit a full queue
   of 512 valid owner-signed transfers with consecutive nonces. Save canonical
   state, pending reservations and exact journal bytes. Retry all 512 transfers
   sixteen times (8,192 exact retries) through `NativeNode::submit_transfer`.
   Every call must return already-known, with no journal/state/reservation change.
   The timed retry loop must finish within one second. Fixture creation is
   outside this bound. First capture the unchanged implementation's result.
2. Peer scenario, after direct-path verification: one mutually pinned loopback
   TLS peer advertises the same canonical head and the same already-retained
   512 transfers in eight sequential successful polls. It must still serve all
   four bounded pending pages on every round: no new wire digest or trust claim.
   All eight polls must finish within five seconds, including seven 500ms
   poll delays. Journals/head/nonce/balances/pending remain unchanged; status has
   eight successful rounds and zero failures. Stop ≤1s, zero outgoing workers.
   This separately measures the remaining peer-path signature work.
3. Functional gates: unknown/tampered signatures, same nonce with different
   signed content and wrong-network input remain rejected; known pending and
   confirmed retries remain idempotent after restart; expiry/reorg updates the
   known index; external storage/ownership loss still rejects known retries.
   New transaction admission and block/replay validation remain unchanged.

The optimization may reuse only IDs derived locally from the complete typed
signed transfer and present in this node's independently validated canonical or
pending state. It must not trust a remote txid, sender/nonce pair or advertised
pending digest. A hash match cannot authorize a debit; new block validation still
checks every transfer independently. Collision resistance is the same native
transaction-identity assumption already used for confirmed/pending indexing.

These are bounded unchanged-input developer measurements, not large-account
qualification, reduced wire bandwidth, protection against changing invalid
inputs or public denial-of-service resistance. No operator data, non-loopback
network, model/VM/paid run, GitHub publication or release is involved.

## Results

Pending direct baseline.
