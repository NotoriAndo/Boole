# Proof-to-Block Benchmark v0

`proof-to-block-benchmark.sh` is the current deterministic benchmark seed for Boole's Rust node migration.

It is intentionally small. It does not rank external AI models yet. It first proves that the local runtime can turn checked work into replayable blocks without store/runtime/replay divergence.

```text
runtime-smoke case manifest
→ boole-node runtime-smoke
→ persisted block store
→ replay verification
→ aggregate proof-to-block metrics
```

## Run

Deterministic runtime safety benchmark:

```bash
./scripts/proof-to-block-benchmark.sh
```

Agent-runtime leaderboard, for tool-using CLIs such as Hermes and OpenClaw/OpenCode-compatible runners:

```bash
LEADERBOARD_MD=/tmp/boole-agent-runtime-leaderboard.md ./scripts/agent-runtime-benchmark.sh
```

Provider/model leaderboard, for raw LLM backends such as mock transport and optional Ollama/OpenAI-compatible models:

```bash
LEADERBOARD_MD=/tmp/boole-provider-model-leaderboard.md ./scripts/provider-model-benchmark.sh
```

To select a broader model matrix for solo preflight, generate a spec first. The setup script supports frontier API rows, local OAuth CLI rows, and installed Ollama models without printing secret values:

```bash
# Safe list: prints credential presence, never values.
./scripts/preflight-model-benchmark-setup.py --preset all --list

# Generate all known frontier API + OAuth + installed Ollama rows.
./scripts/preflight-model-benchmark-setup.py --preset all --output /tmp/boole-model-spec.json

# Generate only Ollama rows, auto-detected from `ollama list`.
./scripts/preflight-model-benchmark-setup.py --preset ollama --output /tmp/boole-ollama-spec.json

# Generate one explicit Ollama model row.
./scripts/preflight-model-benchmark-setup.py --preset ollama --ollama-model gemma4:26b --output /tmp/boole-gemma-spec.json
```

Run a generated spec:

```bash
PROVIDER_MODEL_BENCHMARK_SPEC="$(python3 -c 'import json; print(json.dumps(json.load(open("/tmp/boole-model-spec.json")), separators=(",",":")))')" \
  LEADERBOARD_MD=/tmp/boole-provider-model-leaderboard.md \
  ./scripts/provider-model-benchmark.sh
```

Or let the preflight runner collect it into the evidence bundle:

```bash
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset all
./scripts/phase7-solo-preflight.sh --genesis-benchmark --run-model-benchmark --model-preset all --attempts-per-model 50
./scripts/phase7-solo-preflight.sh --run-model-benchmark --model-preset ollama --ollama-model gemma4:26b
```

The generated frontier rows currently cover Anthropic API, OpenAI API, Google/Gemini API, xAI/Grok via OpenAI-compatible API, and Claude CLI OAuth. Missing API env vars become `SKIP`; selected live rows with present credentials may fail if the model does not produce a verifier-accepted proof.

The deterministic benchmark wraps:

```bash
./scripts/runtime-smoke-all.sh
```

and emits JSON to stdout. Human PASS lines go to stderr. Leaderboard scripts emit JSON to stdout and optionally write Markdown when `LEADERBOARD_MD` is set.

## Current metrics

Expected v0 summary:

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

## Current scope

Current cases come from:

```text
fixtures/protocol/runtime-smoke/cases.v1.json
```

They cover:

- `runtime-smoke-multistep`: a two-block scenario fixture.
- `admission-fixture-compat`: the one-block admission fixture adapter path.
- `runtime-smoke-restart-replay`: a three-block scenario that restarts the runtime from recovered store before continuing.
- `runtime-smoke-three-block`: a deterministic three-block mini-chain.
- `runtime-smoke-multiminer`: a deterministic four-block local multi-miner scenario with three distinct proposer keys.

## Why this exists before model benchmarking

The model-by-model Proof-to-Block leaderboard should not start from an unverified benchmark shell. This v0 script locks the local safety rail first:

```text
blocks produced > 0
replayFailures == 0
invalidAccepted == 0
chainDivergence == 0
```

The current benchmark stack now separates two dimensions:

- **Agent runtime benchmark**: Hermes/OpenClaw/OpenCode-style CLIs invoked through `boole-miner`'s `agent_cli` backend. The runtime may use tools, edit files, call Lean/Lake, or do multi-step proof search. Its output is still treated only as an untrusted candidate proof; deterministic verification, canonical bytes, share hash, block commit, and replay decide acceptance.
- **Provider/model benchmark**: raw model/provider backends such as mock transport and optional OpenAI-compatible/Ollama rows. Optional live rows should be gated by environment variables so missing local daemons/API credentials do not create false CI failures.

Both leaderboard wrappers use `scripts/benchmark-runner.py`, emit machine-readable JSON, and can write a Markdown leaderboard via `LEADERBOARD_MD`. Rows are ranked by successful non-skipped run, blocks, verified shares, and lower elapsed time.

## Genesis preflight benchmark

For GitHub/VC-facing controlled evidence, run the full preflight from a clean genesis state:

```bash
./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes
./scripts/boole-preflight-wizard.py --preset everything --genesis-benchmark --attempts-per-model 50 --yes
```

This writes `genesis-benchmark.json` beside `summary.json` and records:

```text
benchmark: proof-to-block-genesis-preflight
genesisMode: reset
genesisHash: zero genesis head
configHash / scenarioHash / runtimeSmokeCasesHash
replayFromGenesis: true
replayPassed: true
difficulty: static-calibrated or epoch-retarget-v0 block target evidence
invalidAccepted: 0
chainDivergence: 0
```

The genesis-reset run is a controlled benchmark: every run starts from the same empty/zero head and must replay deterministically. It is not a claim of Bitcoin-style cumulative-work fork choice or public-network difficulty governance. Current difficulty evidence records `difficultyEpoch`, `tBlock`, `tShare`, and `difficultyWeight` for every produced block. Static runs keep one calibrated target; retarget-v0 runs derive epoch targets from prior block timing, record the resulting epoch/target in block evidence, and validate that evidence during replay/store recovery.
