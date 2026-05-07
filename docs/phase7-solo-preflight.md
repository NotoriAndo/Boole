# Phase 7.0 Solo Preflight

The Phase 7.0 solo preflight is the local evidence gate before closed-testnet onboarding. It proves the Rust `boole-node` runtime, replay, deterministic benchmark, local mining, and agent-runtime wrapper paths can all run from one reproducible command.

Run:

```bash
./scripts/phase7-solo-preflight.sh
```

The script writes an evidence bundle under:

```text
artifacts/preflight/<UTC timestamp>/
```

`artifacts/` is ignored by git. The machine-readable summary is printed to stdout and also saved as `summary.json` inside the evidence directory.

## Wizard mode

For a guided Hermes-style setup flow, use:

```bash
./scripts/boole-preflight-wizard.py
```

Useful non-interactive modes:

```bash
./scripts/boole-preflight-wizard.py --doctor
./scripts/boole-preflight-wizard.py --list-models
./scripts/boole-preflight-wizard.py --preset safe --dry-run
./scripts/boole-preflight-wizard.py --target safe-core --preset safe --genesis-benchmark --yes
./scripts/boole-preflight-wizard.py --target ollama:qwen2.5-coder:7b --preset local-models --yes
./scripts/boole-preflight-wizard.py --target hermes:configured --preset agent-local --yes
./scripts/boole-preflight-wizard.py --target anthropic:claude-opus-4-7 --preset frontier --allow-paid-api --yes
./scripts/boole-preflight-wizard.py --preset everything --genesis-benchmark --attempts-per-model 50 --allow-paid-api --yes
```

Presets remain available for quick starts, but the preferred user-facing flow is target selection. `--list-models` shows a detailed runtime/model catalog with each row's group, status, credential boundary, cost class, and action. Interactive mode prompts for numbered target choices; scripts can use repeatable `--target <id>` flags.

- `safe`: deterministic core preflight only.
- `agent-local`: installs Claude/Codex templates and includes Hermes real proof-to-block.
- `local-models`: agent-local plus installed Ollama model rows.
- `frontier`: agent-local plus frontier API model rows.
- `everything`: agent-local plus frontier API/OAuth/Ollama rows.

The wizard prints a seven-step guided plan before running it:

1. Environment check.
2. Run purpose.
3. Runtime/model selection.
4. Benchmark profile.
5. Safety and cost boundary.
6. Execution plan.
7. Evidence, report, and reproducibility.

After a successful preflight, the wizard writes `wizard-report.md`, `wizard-leaderboard.md`, and `wizard-summary.redacted.json` into the evidence directory. The redacted summary hides local paths and never contains API key values.

Frontier/API and `everything` model rows are blocked unless `--allow-paid-api` is provided. Choose provider/models/attempt count/cost budget before enabling that flag.

## Genesis preflight benchmark

Use `--genesis-benchmark` when the preflight run is intended to become controlled benchmark evidence:

```bash
./scripts/phase7-solo-preflight.sh --genesis-benchmark
./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes
```

In this mode the runner uses a clean/reset evidence directory under:

```text
artifacts/preflight-genesis/<UTC timestamp>/
```

If an explicit `--evidence-dir` is provided, it is automatically reset only when it is safely under `artifacts/` or `/tmp`. The summary includes a `genesisBenchmark` object and writes `genesis-benchmark.json` with:

```text
benchmark=proof-to-block-genesis-preflight
genesisMode=reset
genesisHash=000...000
configHash/scenarioHash/runtimeSmokeCasesHash
difficulty.mode=epoch-retarget-v0 when the tracked retarget smoke case is present; otherwise static-calibrated
difficulty.retarget=enabled when epoch boundaries are observed
difficulty.tBlock / difficulty.tShare / difficulty.difficultyWeight
replayFromGenesis=true
replayPassed=true
invalidAccepted=0
chainDivergence=0
```

For model rows, select attempts/trials with:

```bash
./scripts/phase7-solo-preflight.sh --genesis-benchmark --run-model-benchmark --model-preset all --attempts-per-model 50
./scripts/boole-preflight-wizard.py --preset everything --genesis-benchmark --attempts-per-model 50 --yes
```

This is a controlled genesis-reset benchmark, not a public-network cumulative-work/fork-choice benchmark.
Each produced block records difficulty evidence (`difficultyEpoch`, `tBlock`, `tShare`, `difficultyWeight`). Static runs keep one calibrated target; retarget-v0 runs derive epoch targets from prior block timing, record the resulting epoch/target per block, and replay/block-store recovery validates the recorded difficulty evidence.

## Required checks

The preflight runner currently captures:

- `runtime-smoke-all`
- `proof-to-block-benchmark`
- `local-mining-smoke`
- `boole-agent-mine --runtime fake`
- `boole-agent-mine --runtime hermes --verify mock`
- `agent-runtime-benchmark`

The required safety invariants are:

```text
replay failures: 0
invalid accepted: 0
chain divergence: 0
agent wrapper block path: height >= 1 and replayMatchesRuntime=true
```

## Optional real-agent check

Hermes real verifier/canonical proof-to-block can be included with:

```bash
RUN_HERMES_REAL_PREFLIGHT=1 ./scripts/phase7-solo-preflight.sh
```

or:

```bash
./scripts/phase7-solo-preflight.sh --run-hermes-real
```

This optional row is useful evidence, but it is not a deterministic default gate because live agent/model quality and runtime availability can vary by machine.

## Optional model matrix benchmark

You can also collect model-by-model provider evidence during preflight:

```bash
./scripts/preflight-model-benchmark-setup.py --preset all --list
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset all
```

Useful narrower selections:

```bash
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset frontier
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset oauth
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset ollama
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset ollama --ollama-model gemma4:26b
```

The setup script supports Anthropic/OpenAI/Google/xAI API rows, Claude CLI OAuth, and all installed Ollama models. It records whether credentials are present but never prints credential values. Missing API envs become `SKIP`; selected live rows may fail if the model cannot produce a verifier-accepted proof.

## Config

The tracked local preflight config is:

```text
fixtures/testnet/closed-preflight.v1.json
```

It records the local chain/test profile, scenario manifest, required checks, optional checks, verifier/canonicalizer label, and safety invariants. This is not a production closed-testnet genesis file; it is the local solo-preflight contract used to make evidence comparable between runs.

## Evidence contents

A typical evidence directory contains:

```text
config.json
git-head.txt
git-status.txt
git-log.txt
runtime-smoke-all.json
proof-to-block-benchmark.json
local-mining-smoke.json
boole-agent-mine-fake.json
boole-agent-mine-hermes-mock.json
agent-runtime-benchmark.json
agent-runtime-leaderboard.md
summary.json
*.stderr.txt
```

Do not commit evidence artifacts unless a release process explicitly selects a sanitized bundle.
