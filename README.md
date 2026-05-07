# Boole

Boole is a proof-mined L1 prototype where AI agents create machine-checkable work that becomes replayable blocks.

```text
Agent → Lean proof → verifier → canonical share → block → replay
```

Current local preflight evidence is checked by `./scripts/self-test.sh`: 7 Proof-to-Block cases, 17 produced blocks, `invalidAccepted = 0`, `replayFailures = 0`, and `chainDivergence = 0`.

This repository contains the Rust protocol core and local proof-to-block runtime. A legacy TypeScript reference implementation is used only as a fixture/parity source during migration.

## Quick install

Install required local dependencies, clone/update Boole, and run the setup doctor:

```bash
curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh | bash
```

Review before running:

```bash
curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh -o install.sh
less install.sh
bash install.sh
```

The installer installs required local tools for the safe preflight path: Git, curl, Python 3, Rust `1.95.0` with `rustfmt`/`clippy`, and Lean `leanprover/lean4:v4.29.1` via `elan`. It never asks for wallet seeds/private keys, never prints API key values, never starts public mining, and never runs paid API benchmarks without explicit confirmation. See [`docs/install.md`](docs/install.md).

After install, the guided wizard gives a Hermes-style seven-step flow:

```bash
cd ~/boole
./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes
```

The wizard prints `Step 1/7` through `Step 7/7`, explains the safety/cost boundary, shows a detailed model/runtime picker, runs the selected local preflight plan, and writes `wizard-report.md`, `wizard-leaderboard.md`, and `wizard-summary.redacted.json` beside the evidence. Use `--list-models` to inspect targets such as `safe-core`, local Ollama rows, Hermes/Claude/Codex/opencode CLI rows, and frontier API rows; use repeatable `--target` flags for non-interactive selection. Frontier/API targets require `--allow-paid-api` so API-cost benchmarks cannot run accidentally.

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

## Replay consensus evidence

Replay is the reviewer-facing safety rail: persisted blocks must rebuild the same head and account state without trusting live runtime memory. New evidence-backed blocks carry `selectedShareEvidence` plus `minShareScoreMultiplierNanos` so replay can re-derive selected share hashes from canonical proof packages and verify the admission-policy multiplier used for `minShareScore`. Legacy `fixtures/protocol/replay/v1.json` remains accepted for migration compatibility; `fixtures/protocol/replay/v2.json` covers the stricter evidence-backed path.

See [`docs/replay-consensus.md`](docs/replay-consensus.md).

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

Agent-runtime output is treated as an untrusted candidate proof; deterministic verifier/canonical bytes/share hash/block replay decide acceptance. Optional live provider rows are gated by env vars so missing daemons or credentials do not create false CI failures. To generate a larger preflight model matrix from frontier API keys, local OAuth CLIs, and installed Ollama models:

```bash
./scripts/preflight-model-benchmark-setup.py --preset all --list
./scripts/preflight-model-benchmark-setup.py --preset all --output /tmp/boole-model-spec.json
PROVIDER_MODEL_BENCHMARK_SPEC="$(python3 -c 'import json; print(json.dumps(json.load(open("/tmp/boole-model-spec.json")), separators=(",",":")))')" \
  LEADERBOARD_MD=/tmp/boole-provider-model-leaderboard.md \
  ./scripts/provider-model-benchmark.sh
```

For preflight evidence collection:

```bash
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset all
./scripts/phase7-solo-preflight.sh --genesis-benchmark --run-model-benchmark --model-preset all --attempts-per-model 50
```

See [`docs/proof-to-block-benchmark.md`](docs/proof-to-block-benchmark.md).

## Agent slash-command mining foundation

The shared `/boole mine` foundation is a thin wrapper around the existing `boole-miner` + Rust `boole-node` paths:

```bash
./scripts/boole-agent-mine.sh --runtime fake
./scripts/boole-agent-mine.sh --runtime hermes --verify mock
./scripts/boole-agent-mine.sh --runtime hermes --verify real
./scripts/boole-agent-mine.sh --runtime claude-code --agent-command claude --agent-args '["-p"]'
./scripts/boole-agent-mine.sh --runtime codex --agent-command codex --agent-args '["exec"]'
```

Claude Code, Codex, OpenCode, or Hermes slash commands should call this wrapper rather than reimplementing verifier/submit/replay logic. Install command templates with:

```bash
./scripts/install-agent-slash-commands.sh --profile claude --target-dir .claude/commands --force
./scripts/install-agent-slash-commands.sh --profile codex --target-dir /tmp/boole-codex-prompts --force
```

See [`docs/agent-slash-mining.md`](docs/agent-slash-mining.md).

## Phase 7.0 solo preflight

Run the Hermes-style setup wizard for guided local preflight:

```bash
./scripts/boole-preflight-wizard.py --doctor
./scripts/boole-preflight-wizard.py --preset safe --dry-run
./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes
./scripts/boole-preflight-wizard.py --preset everything --genesis-benchmark --attempts-per-model 50 --allow-paid-api --yes
```

Or run the local evidence gate directly before closed-testnet onboarding:

```bash
./scripts/phase7-solo-preflight.sh
```

The runner captures runtime smoke, Proof-to-Block benchmark, local mining, agent wrapper checks, git metadata, and a summary JSON under ignored `artifacts/preflight/<timestamp>/`. With `--genesis-benchmark`, the runner resets a clean evidence root under `artifacts/preflight-genesis/<timestamp>/`, records `genesis-benchmark.json`, and treats the run as a controlled benchmark from the zero genesis head. Produced blocks include difficulty evidence (`difficultyEpoch`, `tBlock`, `tShare`, `difficultyWeight`) that replay/store recovery validates. Static runs keep one calibrated target; retarget-v0 runs derive epoch targets from prior block timing and record the resulting epoch/target per block. This is not a claim of Bitcoin-style cumulative-work fork choice. See [`docs/phase7-solo-preflight.md`](docs/phase7-solo-preflight.md).

## Migration history

The detailed migration plan is maintained as private/local working notes. Public reviewers should use this README, the tracked docs, fixtures, and `./scripts/self-test.sh` as the source of current repo state.
