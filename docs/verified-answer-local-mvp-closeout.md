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

## Recommended next slice

`NEXT-BATCH.1 — Select the next official batch from operating evidence`

Scope:

- inspect current tracked docs/tests/gates and local MVP evidence;
- choose whether the next official batch should prioritize security review, real settlement/testnet design, richer passport indexing, or public operator UX;
- add the chosen batch to the plan with scope, non-goals, exact RED tests, focused gates, full gate, and public-claim boundaries before implementation.

This next step is deliberately a plan/amendment slice, not a feature expansion. The local MVP is now closed out; the next implementation direction should be selected from evidence, not invented ad hoc.
