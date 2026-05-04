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

## Source plan

The local migration plan is stored in the original repo:

```text
/Users/seoyong/projects/pof/local-docs/rust-core-migration-implementation-plan.md
```
