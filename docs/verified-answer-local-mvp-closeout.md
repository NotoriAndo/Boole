# Verified-answer local MVP closeout

This document records the closeout state for the local verified-answer receipt MVP. It is a tracked, reviewer-facing summary of the implementation slices, gates, and boundaries that were previously captured in private planning notes.

## Current status

- Batch 4 — Verified Answer product surface: COMPLETE for local MVP
- Batch 5 — Gates/docs: COMPLETE

This means Boole has a local, replayable verified-answer receipt surface with a mock/local payment-required flow and focused regression gates. It does not mean live public settlement, autonomous production wallets, universal answer verification, or network reward distribution.

## Completed slices

- `S-preV1.R3.2` — `settlement-report` can export read-only `boole.reputation.event.v1` NDJSON without mutating the durable ledger.
- `V1.1` — `ReceiptCommitment` core type exists and rejects raw answer/prompt/proof payload fields.
- `V1.2` — local node can persist and query `ReceiptCommitment` rows with disabled-store, not-found, and raw-payload boundaries.
- `X1.1` — `/verify-answer` demonstrates a mock/local 402 pay-before-verify flow and receipt creation.
- `P1.1` — local receipt rows project primitive `workAccepted`, `workRejected`, and `rewardCredited` events for a future passport indexer.
- `G1.1` — `./scripts/wallet-session-receipt-gate.sh` is wired into `./scripts/self-test.sh` and self-test JSON.
- `G1.2` — README/frontpage wording describes only the mock/local verified-answer receipt surface and is guarded by public-wording tests/docs-smoke.

## Definition of Done status

The first-pass Definition of Done is foundation-complete for the local/node MVP:

1. Default CLI never prints sk — covered by key/secret-output tests and full gate.
2. Secret export exists only through explicit unsafe command — covered by key export tests.
3. Session policies validate canWithdraw=false and canTransfer=false — covered by `session_policy` tests.
4. Local signer denies over-cap/unknown-family/unknown-verifier requests — covered by signer/session-key tests.
5. Node can persist and query session state — covered by `session_store` and `session_route` tests.
6. Node can reject session-bound submissions that violate session policy — covered by `submit_session_policy` tests.
7. Rewards credit rewardRecipient, not session key — covered by receipt/passport primitive event tests.
8. Node can persist receipt commitments without raw prompt/artifact data — covered by `receipt` and `receipt_route` tests.
9. Mock /verify-answer demonstrates local payment-required flow and receipt creation — covered by `verify_answer_route` tests.
10. Agent passport remains indexer/primitive-event based, not rich consensus state — covered by `agent_passport_events` tests and docs boundary wording.
11. Full workspace tests, docs smoke, and gitleaks pass — covered by the serial full gate.

## Verification gates

Focused local gate:

```bash
./scripts/wallet-session-receipt-gate.sh
```

Expected evidence:

```text
wallet-session-receipt-gate: PASS
```

Public wording/closeout docs focused tests:

```bash
python3 -m unittest scripts.test_verified_answer_closeout
python3 -m unittest scripts.test_public_benchmark_artifacts
./scripts/docs-smoke.sh
```

Full gate:

```bash
RUST_TEST_THREADS=1 ./scripts/self-test.sh
```

Expected evidence:

```text
self-test: PASS
wallet-session-receipt-gate: PASS
```

## Claim boundary

Safe wording:

```text
Boole can return a local verified-answer receipt commitment for machine-checkable work in a mock/local payment-gated flow.
```

Do not treat this closeout as evidence for:

- real public payment-facilitator integration;
- production autonomous wallet authority;
- arbitrary AI-answer truth verification;
- live network reward distribution;
- rich passport dashboard or durable reputation scoring;
- public-network mining evidence;
- paid/API benchmark evidence.

## Deferred non-goals

The following stay deferred until a new reviewed plan/batch exists:

- real BOOLE token economics;
- real public payment facilitator integration;
- OS keychain / secure enclave storage;
- browser wallet;
- mainnet/testnet payments;
- full passport dashboard;
- arbitrary natural-language truth verification.

## Superseded selection record

`NEXT-BATCH.1 — Select the next official batch from operating evidence`

This was the unresolved choice at local-MVP closeout. The 2026-09-03 roadmap
review has now selected the successor order below; the label remains here so the
historical closeout gate continues to identify the decision that was open.

## Selected successor sequence (2026-09-03)

The next product work does not jump straight to payment or public operation:

1. Close the existing N5.3 named-network authority, canonical-state and bounded
   P2P prerequisites, then prove static-bootstrap join and head-synced readiness
   on a controlled three-node testnet using synthetic/test-only work; this must
   not consume public-eligible problem inventory.
2. Close the third-party verifier process boundary before accepting external
   MCP-submitted code.
3. Add the `boole.verify_native` MCP vertical: accept only the native service's
   strict six-field request, use a separately configured loopback native URL,
   and forward its verdict and BF.3 `VerificationReceipt` without rewriting
   either into the legacy `ReceiptCommitment` store.
4. Exercise ACCEPT, deterministic rejection, durable redelivery and MCP restart
   without a second checker execution. Keep payment, wallet automation, block,
   reward, BF.7 and activation out of this milestone. Wallet automation, faucet
   and real payment/reward wiring remain a later economic/operational decision
   gate rather than an implicit next step.

The implementation milestones are deliberately large functional boundaries:
one branch, one PR and one full CI run per 4–8 hour milestone, with two to four
direct behavior tests and no more than three E2E scenarios. Current-code truth
checks happen inside the implementation milestone rather than as separate audit
or documentation PRs.

The selected sequence does not change this closeout's claim boundary. The
existing `/verify-answer` remains a mock/local `ReceiptCommitment` demonstration;
the native checker and BF.3 receipt are a separate service until the MCP
successor lands. Real payment, public settlement, production wallet authority,
rewards and activation remain deferred.

## Successor progress (2026-09-04)

- `N5.3-M1-NAMED-NETWORK-AUTHORITY`: COMPLETE. Explicitly named HTTP nodes
  reject unscoped signed envelopes; `boole-testnet-2` requires a matching
  session-bound work authorization at HTTP boot/submit, peer-share ingress,
  block selection and genesis-aware replay. The four checker-pinned smoke
  paths now submit through network-scoped sessions. Unnamed local embeddings
  retain an explicit legacy compatibility path. Landed in PR #361 at main
  `a52d5fa299e5eaf37eb4f72056eede2845acd4f5`.
- `M2-CANONICAL-STATE-SAFETY`: COMPLETE. Observation-only status/ready/verify
  paths no longer truncate torn storage; named genesis can bind the canonical
  family-manifest root; and the block store is the recovery authority for the
  reward, bounty and proof-dedup projections. A failure after a canonical
  block/reorg write now poisons readiness and blocks further mutations until a
  restart performs an exact rebuild. Direct HTTP and P2P fault-injection tests
  cover the partially published state rather than treating it as a clean
  rejection. Implemented by PR #362.
- `M3-P2P-LIFECYCLE`: COMPLETE. Outbound gossip now uses a bounded queue and
  independent worker per peer, so one slow peer cannot retain unbounded work or
  delay healthy peers. Initial/reorg sync charges exact received wire bytes and
  returned blocks against one cumulative per-peer round budget before state
  mutation; response count and an absolute round deadline also bound byte
  trickles, and the remaining wire allowance caps allocation before JSON parse.
  One lifecycle registry closes blocked ingress, egress, sync and
  package-fetch sockets before thread joins. Direct tests cover slow-peer
  shedding, over-return rejection with zero mutation, and prompt shutdown of
  partial-frame and response-less peers. Full competing-chain replay is
  deliberately deferred when it cannot fit the bounded round; resumable deep
  reorg/snapshot sync remains a later pre-long-running-testnet capability, not
  something this closed three-node milestone claims.
- `M4-CONTROLLED-BOOTSTRAP-JOIN`: COMPLETE. The supported `boole node start
  --network testnet` path selects the compiled `boole-testnet-2` identity and
  a static numeric-loopback bootstrap configuration. The named three-process
  E2E first commits one network-authorized synthetic fixture block to the
  seed's durable state, proves that two joiners remain live but return
  `/ready` 503 while that seed is absent, then restarts the seed and requires
  all three nodes to converge on the same `(height, c)`, genesis-spec hash and
  replay-consistent block before readiness opens and remains green longer than
  the configured five-second inter-cycle sleep on healthy loopback peers.
  Readiness means that every configured bootstrap
  peer's last completed outbound observation matches the node's current head;
  `/ready` rechecks that observation against the live local head, but a peer
  that advances without an announcement can leave a stale-green window until
  the next outbound polling cycle reaches that peer; with multiple peers this
  includes the bounded rounds spent on peers earlier in the cycle. This
  milestone therefore proves controlled bootstrap/join, not continuous
  global-head knowledge. It consumes no public-eligible problem inventory and
  grants no public P2P exposure or activation.
- Completed milestone: `M5-THIRD-PARTY-VERIFIER-BOUNDARY`. The verifier now
  uses three contained stages: a request-private trusted-helper compile, direct
  source elaboration, and an artifact-only independent audit. The inactive
  `boole-testnet-2` identity is rebound to that checker release; this does not
  activate a network.
- Current milestone: `M6-HTTP-P2P-VERIFICATION-PARITY`. HTTP and P2P share,
  block and reorg ingress must use the same bounded semantic verifier without
  holding the node-state write lock during Lean execution, then revalidate the
  latest canonical state before any durable effect. The `boole.verify_native`
  MCP vertical remains a separate later integration boundary.

This progress is closed-local protocol hardening. It neither activates a public
testnet nor authorizes mining, rewards, payment, consensus, P2P exposure or use
of the public-eligible problem inventory.

The former selection scope was:

- inspect current tracked docs/tests/gates and local MVP evidence;
- choose whether the next official batch should prioritize security review, real settlement/testnet design, richer passport indexing, or public operator UX;
- add the chosen batch to the plan with scope, non-goals, exact RED tests, focused gates, full gate, and public-claim boundaries before implementation.

That historical selection step was not a feature expansion. The selection is
now complete: the local MVP remains closed out, and the next implementation
direction is the ordered successor above.
