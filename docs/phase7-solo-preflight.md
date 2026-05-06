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
