# Boole Rust Core Migration Workspace

This is a separate Rust workspace for migrating Boole's core L1 protocol implementation while preserving the behavior of the existing TypeScript/Lean/Python codebase at:

```text
/Users/seoyong/projects/pof
```

## Target language split

```text
Rust:
  core L1 node, block/state/replay, reward/account ledger, canonicalization, native CLI, verifier runner

Lean:
  proof obligations, checker artifacts, formal specs

TypeScript:
  MCP, agent bridge, frontend/dashboard, JS SDK, provider/model adapters

Python:
  calibration, benchmarks, difficulty analysis, reports
```

## Migration rule

Do not translate code by intuition. Preserve behavior through fixtures and parity tests:

```text
TypeScript current behavior -> golden fixtures -> Rust implementation must match
```

Initial success target:

```text
Rust boole-core can replay TypeScript-produced block/reward fixtures and produce the same final head and account balances.
```

## Crates

- `boole-core`: protocol types, hashes, canonical encoding, replay, ledger state.
- `boole-cli`: native CLI with stable JSON output and exit codes.
- `boole-node`: future Rust node daemon/RPC layer.
- `boole-lean-runner`: Rust wrapper around Lean verifier artifacts/toolchain.

## Runtime smoke

Run the checked node smoke harness:

```bash
./scripts/runtime-smoke-all.sh
```

The harness drives the actual `boole-node` binary through every case in `fixtures/protocol/runtime-smoke/cases.v1.json`; today that covers both the tracked two-block scenario and the admission-fixture compatibility path. Each case validates runtime policy boot, admission, block commit, block-store recovery, replay, and consistency fields. The aggregate JSON reports `ok: true` with per-case `storeSize`, `replayHeight`, `latestMatchesRuntime`, and `replayMatchesRuntime`.

For the single tracked two-block scenario only:

```bash
./scripts/runtime-smoke.sh
```

Equivalent direct command:

```bash
cargo run -q -p boole-node -- runtime-smoke \
  --scenario fixtures/protocol/runtime-smoke/v1.json \
  --block-store /tmp/boole-runtime-smoke.ndjson
```

See [`docs/runtime-smoke.md`](docs/runtime-smoke.md) for the scenario format and output fields.

## Proof-to-Block Benchmark v0

The current benchmark seed wraps the checked runtime-smoke harness and reports local proof-to-block metrics:

```bash
./scripts/proof-to-block-benchmark.sh
```

This is not a public model leaderboard yet. It is the deterministic base layer for later model-by-model runs: checked cases, blocks produced, replay failures, and safety badges. See [`docs/proof-to-block-benchmark.md`](docs/proof-to-block-benchmark.md).

## Source plan

The local migration plan is stored in the original repo:

```text
/Users/seoyong/projects/pof/local-docs/rust-core-migration-implementation-plan.md
```
