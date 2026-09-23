# Native exact-transfer retry qualification — 2026-09-23

Status: **BASELINE FAIL; OPTIMIZATION ON HOLD — no runtime change.** This work targets repeated
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

### Direct baseline — FAIL at the unchanged one-second criterion

Preregistration/test source `3dee1bb832dd1933728dddc117cb4e2b25bcc89e`, tree
`949e18ed0f4fc5cef2d2da6c9b5f3f617c456cda`; debug executable SHA-256
`79f05cb93eb6d692bcd33ef0a4f8b9bff58f4d5a31791edd3bf59a85fbe52e2f`.
The first explicit attempt ran at approximately 10:05 UTC without other local
build/test load:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_pending_resources-d1ef5a781f39c868 \
  --ignored --exact exact_known_transfer_retries_have_bounded_cpu_cost \
  --nocapture --test-threads=1
```

All 8,192 calls returned already-known and state/reservations/journals matched,
but the timed loop took 2.748584083s, exceeding the fixed one-second bound. No
unchanged rerun was made. The sandbox prevented `/usr/bin/time` from querying
`kern.clockrate`; therefore OS RSS/counter results are unavailable, not zero.
The Rust monotonic-clock test result is complete and remains a failure.

```text
exact-transfer-retries {"retries":8192,"pending":512,"elapsedMicros":2748584,"journalsUnchanged":true}
exact known retries repeated too much immutable signature work: 2.748584083s
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 2 filtered out; finished in 6.75s
        8.09 real         4.46 user         0.32 sys
time: sysctl kern.clockrate: Operation not permitted
```

### Safety hold and bounded supporting evidence

At approximately 10:06 UTC, automated safety review rejected the proposed
ordering change that would return an already-known ID before rechecking its
signature/format. Its stated concern was validation bypass without separate
proof of full ID binding and index integrity. The patch was not applied; the
working tree was checked and `submit_transfer` still validates before checking
the known index. A scoped operator approval question was left unanswered at the
time of this record. Do not retry that optimization, implement the peer shortcut
or reinterpret the delegation as a new approval while this hold remains.

Read-only source inspection found that `NativeTransfer::id` hashes the domain
separator and canonical serialization of the entire typed transfer, including
both schemas, network, signer, signature and all seven payload fields. No field
is skipped in serialization. NativeNode's pending/confirmed maps are private;
confirmed IDs are derived from the validated NativeChain on replay, block commit
or candidate publication, and pending IDs follow validated durable admission or
`retain_pending` reconstruction. No remote ID/checkpoint is loaded as authority.
These are source-level findings, not a proof of collision resistance or a blanket
certification of every future index mutation.

A new ordinary public-API regression test changes each of the eleven leaf
fields and requires a different ID plus rejection. It also checks a correctly
signed same-nonce replacement with a different fee, exact retries while pending,
after confirmation and after independent replay, unchanged state/journal bytes,
and known-transfer rejection after its disposable manifest is moved away.
The first focused run passed in 1.05s on the unchanged runtime. Existing expiry,
reorg and pending-publication tests remain the relevant index-lifecycle gates.
All three ordinary pending-resource tests subsequently passed in 4.61s (the
timing qualification remained ignored), and the focused test-target clippy
passed in 6.29s. Formatting, docs-smoke and diff checks also passed.

The peer timing scenario and post-optimization measurements have **not run**.
The ignored direct timing test intentionally retains its failing criterion as
the preserved baseline, not a passing product acceptance gate. Runtime behavior,
signature verification and protocol remain unchanged in this branch.
