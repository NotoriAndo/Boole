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
- `boole-node`: Rust local node/runtime server, smoke runner, and future network daemon/RPC layer.
- `boole-lean-runner`: Rust wrapper around Lean verifier artifacts/toolchain.

## Self-test gate

Run the local core health gate before publishing changes:

```bash
./scripts/self-test.sh
```

The gate runs Rust formatting, strict clippy, TypeScript-to-Rust parity, runtime smoke cases, Proof-to-Block Benchmark v0, local mock mining smoke, diff whitespace checks, and gitleaks when available. It emits machine-readable JSON on stdout and progress/PASS lines on stderr.

## Local node

Start a local Rust node HTTP server:

```bash
cargo run -q -p boole-node -- run-local \
  --addr 127.0.0.1:8080 \
  --scenario fixtures/protocol/runtime-smoke/v1.json \
  --block-store /tmp/boole-node-local.ndjson
```

Initial local endpoints:

```text
GET  /status
GET  /head
GET  /config
POST /ticket
POST /submit
```

Run the checked local-node smoke test:

```bash
./scripts/local-node-smoke.sh
```

Run the checked local mock mining smoke test:

```bash
./scripts/local-mining-smoke.sh
```

Run the checked TypeScript `boole-miner` → Rust `boole-node` smoke test:

```bash
./scripts/boole-miner-smoke.sh
```

The mining smoke starts `boole-node run-local`, reads `/head` and `/config`, announces tickets through `/ticket`, submits two fixture-backed mock-miner candidates to `/submit`, and verifies two replayable blocks are mined. The `boole-miner` smoke starts the same Rust node, runs the TypeScript miner CLI with mock LLM/mock verifier, and verifies one accepted share becomes one replayable block.

This is the first Rust `boole-node` replacement path for the old TypeScript dispatcher shape: local HTTP submit, runtime admission, block commit, store recovery, and replay consistency.

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

The deterministic benchmark seed wraps the checked runtime-smoke harness and reports local proof-to-block metrics:

```bash
./scripts/proof-to-block-benchmark.sh
```

Two leaderboard wrappers extend that safety rail without mixing unlike backends:

```bash
# Tool-using agent runtimes: Hermes, OpenClaw/OpenCode-compatible CLIs, etc.
LEADERBOARD_MD=/tmp/boole-agent-runtime-leaderboard.md ./scripts/agent-runtime-benchmark.sh

# Raw provider/model backends: mock transport and optional OpenAI-compatible/Ollama rows.
LEADERBOARD_MD=/tmp/boole-provider-model-leaderboard.md ./scripts/provider-model-benchmark.sh
```

Agent-runtime output is treated as an untrusted candidate proof; deterministic verifier/canonical bytes/share hash/block replay decide acceptance. Optional live provider rows are gated by env vars so missing daemons or credentials do not create false CI failures. See [`docs/proof-to-block-benchmark.md`](docs/proof-to-block-benchmark.md).

## Source plan

The local migration plan is stored in the original repo:

```text
/Users/seoyong/projects/pof/local-docs/rust-core-migration-implementation-plan.md
```
