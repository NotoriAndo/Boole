# Boole Rust Core Migration — Current Status and Next Steps

> **Superseded (2026-06-13, N0-pre.12).** This document is a historical
> migration snapshot, not the current source of truth. The current gate
> figures live in `README.md` and `scripts/self-test.sh`; the binding
> execution order lives in `local-docs/todo/EXECUTION-ORDER.md`. The
> figures below were refreshed to the current `self-test.sh` numbers so
> this file no longer misleads an external reader, but new status must be
> read from the README and the master plans, not here.

**Updated:** N0-pre.12 figure refresh (was `53ca071 test: expand runtime smoke preflight scenarios`)

## One-line status

Boole Rust migration is **not fully complete as a production L1**, but the **Rust runtime proof-to-block spine and local preflight harness are now working end-to-end**.

Current verified local benchmark (matches `README.md` / `self-test.sh`):

```text
casesPassed: 7
caseCount: 7
blocksProduced: 17
replayFailures: 0
invalidAccepted: 0
chainDivergence: 0
```

Run:

```bash
./scripts/proof-to-block-benchmark.sh
```

---

## What is complete

### 1. Fixture/parity migration foundation

The Rust workspace follows the migration rule:

```text
TypeScript current behavior → golden fixtures → Rust implementation must match
```

Current parity gate:

```bash
./scripts/check-rust-parity.sh
```

This regenerates TypeScript-derived fixtures and runs the Rust workspace tests.

Covered fixture/parity domains include:

- block hash
- replay
- block builder
- hash/pow
- share pool
- manifests
- bounty registry
- bounty ledger
- config
- validator
- submission pow
- rejection log
- rate limiter
- admission policy fixture path

### 2. Runtime policy/admission spine

The Rust node runtime now connects the core admission path:

```text
CalibrationReport
→ CalibrationPolicy / RuntimeConfig
→ RuntimeAdmissionState
→ ticket observation
→ admission
→ share pool
→ candidate tracking
→ block selection
```

Important properties already implemented:

- typed policy/runtime config path
- stateful rate limiter and share pool
- accepted submission → candidate share tracking
- stale old-head submissions rejected by share-pool/head logic
- quota tests do not relax fixture policy just to pass

### 3. Block production, persistence, replay

The runtime can now produce and persist replayable blocks:

```text
candidate shares
→ build block selection
→ produce PersistedBlock
→ FileBlockStore append
→ recover
→ replay_blocks
```

Implemented runtime APIs include:

```rust
RuntimeAdmissionState::apply_produced_block(...)
RuntimeAdmissionState::commit_block_for_current_c(...)
RuntimeAdmissionState::commit_next_block_for_current_c(...)
RuntimeAdmissionState::boot_from_store(...)
```

### 4. Runtime head advancement and pruning

After block production, runtime head advances explicitly:

```text
current_c = produced_block.c
old c shares pruned
old c candidates pruned
```

This prevents stale old-head work from silently remaining valid after a block is committed.

### 5. Restart/replay continuation

The runtime can boot from an existing block store:

```text
FileBlockStore::recover
→ replay_blocks
→ set current_c = replay.latest_c
→ continue next block height
```

This is covered in both Rust tests and runtime-smoke fixtures.

### 6. `boole-node runtime-smoke` CLI

The actual node binary supports:

```bash
cargo run -q -p boole-node -- runtime-smoke \
  --scenario fixtures/protocol/runtime-smoke/v1.json \
  --block-store /tmp/boole-runtime-smoke.ndjson
```

And the older fixture adapter path:

```bash
cargo run -q -p boole-node -- runtime-smoke \
  --fixture fixtures/protocol/admission/v1.json \
  --block-store /tmp/boole-runtime-smoke-fixture.ndjson
```

### 7. Scenario runner library API

The smoke runner is also exposed as a library module:

```rust
run_runtime_smoke(...)
run_runtime_smoke_scenario(...)
run_runtime_smoke_multi_scenario(...)
run_runtime_smoke_scenario_file(...)
```

This lets future benchmark, self-test, and orchestrator layers reuse the same runtime path instead of shelling out only through CLI.

### 8. Multi-step scenario support

Scenario JSON supports multiple ordered steps.

Key fields:

```json
{
  "cFromRuntimeHead": true,
  "expectedPrevC": "...",
  "restartFromStore": true
}
```

Meanings:

- `cFromRuntimeHead`: replace `body.c` with the current runtime head before admission.
- `expectedPrevC`: fail fast if the current runtime head is not the expected value.
- `restartFromStore`: boot a fresh runtime from the recovered block store before this step.

### 9. Fail-fast runtime/store/replay checker

After every committed smoke step, the runner checks:

```text
block store latest c == runtime head
replay latest c == runtime head
```

If this diverges, the scenario fails immediately instead of waiting until the end.

### 10. Manifest-based runtime smoke harness

Current harness:

```bash
./scripts/runtime-smoke-all.sh
```

Case manifest:

```text
fixtures/protocol/runtime-smoke/cases.v1.json
```

Current cases:

1. `runtime-smoke-multistep`
   - 2-block scenario
2. `admission-fixture-compat`
   - 1-block fixture adapter path
3. `runtime-smoke-restart-replay`
   - 3-block scenario with runtime restart from store
4. `runtime-smoke-three-block`
   - deterministic 3-block mini-chain
5. `runtime-smoke-multiminer`
   - deterministic 4-block local multi-miner proposer rotation

### 11. Proof-to-Block Benchmark v0 seed

Current benchmark command:

```bash
./scripts/proof-to-block-benchmark.sh
```

Current expected output summary:

```json
{
  "ok": true,
  "benchmark": "proof-to-block",
  "version": 0,
  "summary": {
    "casesPassed": 5,
    "caseCount": 5,
    "blocksProduced": 13,
    "replayFailures": 0
  },
  "safety": {
    "invalidAccepted": 0,
    "chainDivergence": 0,
    "replayMatchesRuntime": true
  }
}
```

This is **not yet a model leaderboard**. It is the deterministic safety/base layer for future model-by-model Proof-to-Block runs.

---

## Current important files

### Runtime implementation

```text
crates/boole-node/src/runtime.rs
crates/boole-node/src/runtime_smoke.rs
```

### Runtime tests

```text
crates/boole-node/tests/runtime_policy_boot.rs
crates/boole-node/tests/runtime_smoke_cli.rs
crates/boole-node/tests/runtime_smoke_library.rs
```

### Runtime smoke fixtures

```text
fixtures/protocol/runtime-smoke/v1.json
fixtures/protocol/runtime-smoke/restart-replay.v1.json
fixtures/protocol/runtime-smoke/three-block.v1.json
fixtures/protocol/runtime-smoke/multiminer.v1.json
fixtures/protocol/runtime-smoke/cases.v1.json
```

### Scripts

```text
scripts/runtime-smoke.sh
scripts/runtime-smoke-all.sh
scripts/proof-to-block-benchmark.sh
scripts/check-rust-parity.sh
```

### Docs

```text
docs/runtime-smoke.md
docs/proof-to-block-benchmark.md
docs/migration-status-and-next-steps.md
```

---

## What this proves

The current Rust node/runtime can locally prove this loop:

```text
scenario/fixture input
→ runtime policy boot
→ ticket observation
→ admission
→ share/candidate tracking
→ block selection
→ block commit
→ block store persistence
→ replay
→ runtime/store/replay consistency
→ benchmark summary
```

The strongest current claim is:

```text
Boole Rust can run deterministic local Proof-to-Block preflight cases across fixture compatibility, multi-block chains, restart/replay continuation, and local multi-miner proposer rotation with zero replay divergence.
```

---

## What is not complete yet

Do **not** describe the whole migration as complete yet.

Remaining major areas:

1. Self-test command / solo preflight gate
2. Production-like long-running node loop
3. Real miner loop
4. Lean verifier production path
5. Reward/account ledger runtime integration
6. State transition and state root
7. P2P or multi-process node simulation
8. Stable user/agent CLI UX
9. Closed testnet packaging
10. Model-by-model Proof-to-Block benchmark

---

## Recommended next path

### Phase A — Self-test gate

Goal: one command verifies local core health.

Recommended command:

```bash
./scripts/self-test.sh
```

It should run and summarize:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-rust-parity.sh
./scripts/runtime-smoke-all.sh
./scripts/proof-to-block-benchmark.sh
git diff --check
gitleaks detect --redact --verbose --no-banner  # when available
```

Expected output shape:

```json
{
  "ok": true,
  "checks": [
    { "name": "rust-parity", "ok": true },
    { "name": "runtime-smoke-all", "ok": true, "casesPassed": 5 },
    { "name": "proof-to-block-benchmark", "ok": true, "blocksProduced": 13 }
  ]
}
```

Acceptance criteria:

```text
self-test: PASS
rust-parity: PASS
runtime-smoke-all: PASS
proof-to-block-benchmark: PASS
working tree clean after generated fixtures are regenerated
```

### Phase B — Production-like local node loop

Goal: move from one-shot smoke commands to a production-shaped local node loop.

Work items:

1. Add a node config file format.
2. Add `boole-node run-local` or equivalent.
3. Load runtime config and block store path from config.
4. Run a bounded local production loop.
5. Support graceful shutdown.
6. Emit machine-readable status JSON.
7. Add restart/resume test from the same block store.

Acceptance criteria:

```text
node starts from config
produces N blocks
shuts down cleanly
restarts from same store
continues from latest head
replay has zero divergence
```

### Phase C — Miner loop

Goal: introduce a production-shaped local miner loop instead of prebuilt scenario steps.

Work items:

1. Define local miner identity config.
2. Generate or load candidate work per miner.
3. Submit candidate work into runtime admission.
4. Commit selected shares into blocks.
5. Track proposer identity in output.
6. Add deterministic multi-miner test.

Acceptance criteria:

```text
multiple local miner identities submit work
blocks include proposer identity
no chain divergence after replay
```

### Phase D — Lean verifier path

Goal: connect real proof artifact checking into the runtime admission path.

Work items:

1. Define verifier artifact input schema.
2. Wire `boole-lean-runner` into a sandboxed check path.
3. Convert verifier result into canonical admission result.
4. Add valid/invalid proof fixtures.
5. Ensure invalid proof cannot become accepted share.

Acceptance criteria:

```text
valid proof artifact can enter admission
invalid proof artifact is rejected
invalidAccepted remains 0
```

### Phase E — Reward/account/state integration

Goal: connect block production to account/reward state.

Work items:

1. Define runtime account state structure.
2. Apply block reward to proposer/miner accounts.
3. Persist/replay account state.
4. Add state root or deterministic state hash.
5. Add fixture tests for reward replay.

Acceptance criteria:

```text
block replay reproduces account balances
state hash is deterministic
reward totals match emitted blocks
```

### Phase F — P2P or multi-process simulation

Goal: move beyond single-process deterministic multi-miner fixtures.

Work items:

1. Run multiple local node processes or simulated node instances.
2. Exchange block/share messages through a deterministic local transport.
3. Detect duplicate/fork/divergence cases.
4. Add canonical head/finality rule tests.

Acceptance criteria:

```text
multiple node instances converge on the same head
replay from each node store matches
fork/divergence cases are detected
```

### Phase G — Model-by-model Proof-to-Block Benchmark

Goal: turn the deterministic benchmark seed into the public hook.

Work items:

1. Add model/provider metadata fields.
2. Add per-run cost/time fields.
3. Separate infra failure from model failure.
4. Add ranking logic:

```text
blocks produced
→ verified shares
→ replay pass
→ median proof-to-share time
→ cost/share
```

5. Produce README/front-page benchmark card.

Acceptance criteria:

```text
model-by-model run emits comparable JSON
invalidAccepted == 0
chainDivergence == 0
replay PASS
```

---

## Current verification commands

Run before claiming current migration health:

```bash
./scripts/proof-to-block-benchmark.sh
./scripts/runtime-smoke-all.sh
./scripts/runtime-smoke.sh
cargo test -q -p boole-node --test runtime_smoke_cli -- --nocapture
cargo fmt --all
./scripts/check-rust-parity.sh
git diff --check
```

Expected current result:

```text
proof-to-block-benchmark: PASS
runtime-smoke-all: PASS
runtime-smoke: PASS
runtime_smoke_cli.rs: 9 passed
rust-parity: PASS
```

---

## Recommended wording

Use this wording publicly or in project notes:

```text
Boole's Rust runtime migration has reached deterministic local Proof-to-Block preflight: five checked cases, thirteen produced blocks, zero replay failures, zero invalid accepted, and zero chain divergence.
```

Avoid saying:

```text
The full L1 migration is complete.
```

More accurate:

```text
The Rust runtime spine is end-to-end locally verifiable; production L1 node, verifier integration, state/reward runtime, P2P, and closed-testnet packaging remain.
```
