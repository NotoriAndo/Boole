# Node Runtime Responsibility Inventory

D5 candidate: node runtime route boundary inventory

This document records the first post-D4 inventory pass. D4 closed the `boole-core` / `boole-node` runtime IO boundary. D5 should not reopen that boundary. D5 should decide whether `boole-node/src/local_node.rs` needs internal route/runtime decomposition, and in what order.

## Baseline

- Base commit inspected: `a815b2f docs: mark d4 runtime boundary closed`.
- Branch: `main`.
- Working tree was clean before the inventory.
- `boole-core/src` had no production runtime IO residue for the D4 patterns scanned.

## Current shape

`crates/boole-node/src/local_node.rs` is the main concentration point for runtime HTTP behavior.

Observed size and concentration:

- `local_node.rs`: 2,386 lines.
- `runtime.rs`: 603 lines.
- `main.rs`: 562 lines.
- runtime store modules are already smaller and focused:
  - `receipt_store.rs`
  - `session_store.rs`
  - `bounty_event_store.rs`
  - `reward_store.rs`
  - `reputation_store.rs`

Approximate responsibility counts inside `local_node.rs`:

- route/router terms: high concentration
- handler/json functions: high concentration
- stores/ledgers/receipts/sessions references: high concentration
- submit/share/block terms: high concentration
- bounty/work/family terms: high concentration
- verify-answer/x402/receipt terms: concentrated in the same file

This is not a correctness bug by itself, but it is now the next structural risk after D4: `local_node.rs` mixes HTTP extraction, JSON contract parsing, session policy, receipt persistence, bounty mutation, proof submission, block JSON shaping, and server connection lifecycle helpers.

## Largest route/domain functions observed

These are the highest-risk split candidates because they combine wire parsing, domain calls, state mutation, and response shaping:

- `submit_json`
- `bounty_proof_json`
- `bounty_announce_json`
- `bounty_status_json`
- `verify_signed_submit_work`
- `verify_answer_json`
- `submit_session_gate`
- `block_json`

## Candidate slices

### Candidate A — session submit gate module

Move session-bound `/submit` gate helpers out of `local_node.rs` behind a node-owned module while keeping route behavior unchanged.

Likely functions/types:

- `CheckedSubmitSession`
- `VerifiedSubmitWork`
- `submit_session_gate`
- `burn_submit_nonce`
- `verify_signed_submit_work`

Why it matters:

- This is agent-wallet critical path logic.
- It protects fixed reward recipient, nonce replay, session validity, signed work, and request hash binding.
- It is highly coupled to public safety claims around verified work and reward receipts.

Risk:

- Needs careful tests because rejection order and error strings matter.
- It touches `/submit`, nonce persistence, and receipt append behavior.

### Candidate B — bounty route module

Move bounty announce/status/proof JSON handlers into a node-owned route module.

Likely functions:

- `bounty_announce_json`
- `bounty_status_json`
- `bounty_proof_json`
- `replay_bounty_audit_event`
- replay helpers for create/proof/status events

Why it matters:

- Bounty lane is one of Boole's main verified-work entry paths.
- The logic mixes signed envelope parsing, verifier dispatch, registry mutation, audit append, side-pool insertion, and status replay.

Risk:

- Broad surface and many existing tests.
- Better as a second D5 slice after a smaller extraction pattern is proven.

### Candidate C — verify-answer / receipt local MVP module

Move mock/local `/verify-answer` and local receipt commitment JSON handlers into a node-owned module.

Likely functions/types/constants:

- `VerifyAnswerRequest`
- `X402VersionsFixture`
- `verify_answer_json`
- `accepted_x402_versions`
- receipt commitment append/get helpers
- local x402 constants

Why it matters:

- Keeps mock x402-style pay-before-verify receipt logic separate from block/mining submit flow.
- The surface is explicitly local MVP, not public settlement.

Risk:

- Smaller than bounty/session split.
- Must preserve no-raw-answer persistence, x402 version rejection, payment-required/invalid responses, and agent event projection.

### Candidate D — HTTP server lifecycle module

Move connection counting, body cap, timeout, and fallback helpers into a server module.

Likely types/functions:

- `ConnectionCounter`
- `ConnectionLifetime`
- `ConnectionCountingMakeService`
- `ConnectionCountedFuture`
- `ConnectionCountedService`
- `connection_close_middleware`
- `body_cap_middleware`
- `fallback_handler`

Why it matters:

- Reduces local_node.rs boilerplate without changing domain behavior.

Risk:

- Lower domain value than session/bounty/verify-answer work.
- It is mostly mechanical and should not be chosen first if the goal is verified AI work chain clarity.

## Recommendation

Recommended first D5 slice: Candidate C, verify-answer / receipt local MVP module.

Why this is the best first slice:

- It is smaller than bounty and session-submit extraction.
- It has clear existing tests: `verify_answer_route.rs`, `receipt_route.rs`, and `agent_passport_events.rs`.
- It isolates local mock x402 receipt behavior from mining/block submit logic.
- It is directly aligned with the verified AI work chain boundary: request → payment gate → deterministic receipt commitment → primitive agent events.
- It avoids over-touching public mining, block production, or bounty promotion logic.

Do not start D5 by splitting runtime IO loaders. D4 already moved and closed those boundaries. The next risk is route/domain concentration inside `local_node.rs`, not core-owned filesystem IO.

## Proposed D5 TDD entry point

Start with a guard/test that proves `verify_answer_json` behavior remains identical while the implementation moves behind a module boundary.

Focused gates for the first extraction slice:

```bash
cargo test -p boole-node --test verify_answer_route -- --nocapture
cargo test -p boole-node --test receipt_route -- --nocapture
cargo test -p boole-node --test agent_passport_events -- --nocapture
python3 -m unittest scripts.test_preflight_orchestration
./scripts/docs-smoke.sh
```

Full gate remains:

```bash
RUST_TEST_THREADS=1 ./scripts/self-test.sh
```

No public mining, paid/API benchmark, or public score claim is implied by this inventory.
