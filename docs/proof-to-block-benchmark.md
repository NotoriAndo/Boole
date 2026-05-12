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

# Generate local Claude CLI rows for both pinned Claude models.
./scripts/preflight-model-benchmark-setup.py --preset oauth --output /tmp/boole-claude-cli-spec.json
# Includes: claude-cli:claude-sonnet-4-6 and claude-cli:claude-opus-4-7.

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

For reproducible model-by-model evidence bundles, use the artifact runner:

```bash
./scripts/boole-model-benchmark.py \
  --spec /tmp/boole-model-spec.json \
  --output-dir /tmp/boole-model-benchmark
```

For a local Ollama model attempt run, use an explicit `ollama:<model>` target:

```bash
./scripts/boole-model-benchmark.py \
  --target ollama:qwen2.5-coder:7b \
  --attempts 3 \
  --output-dir /tmp/boole-ollama-benchmark
```

The Ollama path calls `ollama run <model> <prompt>` and records each generated candidate as an untrusted attempt row. It does not auto-start the Ollama daemon and does not auto-pull models. Missing command, daemon, or model setup is recorded as `SETUP_REQUIRED`, with recovery guidance such as `ollama serve` or `ollama pull <model>`, and the benchmark artifact run itself can still complete.

It writes:

```text
benchmark-summary.json
benchmark-rows.ndjson
leaderboard.md
replay-report.json
```

This runner does not auto-pull Ollama models, start daemons, or bypass paid/API confirmation. Rows with missing required env vars are recorded as `SKIP`; Ollama setup gaps are recorded as `SETUP_REQUIRED`; accepted/rejected proof attempts remain subject to verifier/replay metrics, with `invalidAccepted`, `replayFailures`, and `chainDivergence` preserved as the safety rail. Local model-generated proof attempts are evidence rows, not claims of live network mining.

By default, generated model runs use `benchmarkMode: mining` and `targetFamily: boole.calibration.pow.v1`. Each attempt receives its own deterministic lottery sample derived from `(runId, target, attemptIndex, benchmarkMode, targetFamily)`, and rows expose that sample under `lotterySample`. The `True.intro` / `theorem ... : True` contract is now isolated behind explicit `--benchmark-mode smoke` for pipeline smoke only; it is not a public model score.

The public-safe controlled local mining schema is frozen at `fixtures/benchmarks/controlled-model-mining/v1-summary.json`. It ranks models by `blocksProduced` first, keeps `verifiedShares`/`verifierAccepted` as diagnostics, and preserves the hierarchy `generatedAttempts → proofIntakeAccepted → verifierAccepted → verifiedShares → blocksProduced → replayPassed`. The fixture is an example schema contract, not a measured leaderboard. The Claude CLI schema rows are pinned separately as `claude-cli:claude-sonnet-4-6` and `claude-cli:claude-opus-4-7` so future controlled runs do not collapse Claude evidence into a vague `claude-code` bucket.

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
    "casesPassed": 7,
    "caseCount": 7,
    "blocksProduced": 17,
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
- `runtime-smoke-retarget-v0`: a deterministic retarget-v0 evidence case for controlled preflight runs.
- `runtime-smoke-multiminer`: a deterministic four-block local multi-miner scenario with three distinct proposer keys.
- `lean-submit-proof-to-block`: a deterministic Lean-backed `boole-node submit-lean` smoke case that explicitly uses `--difficulty-mode preflight-easy` to check Lean verification, admission, block append, and replay from genesis with `invalidAccepted == 0`. Model benchmarks do **not** use this easy override; they use the fixture's calibrated block-selection difficulty by default.

Optional preflight row, disabled by default so CI and local smoke remain deterministic:

```bash
BOOLE_ENABLE_AGENT_PROOF_CANDIDATE=1 ./scripts/proof-to-block-benchmark.sh
```

- `agent-fixture-submit-proof-to-block`: runs `boole-node agent-proof --backend fixture-valid`, records the generated Lean file as an explicitly untrusted `agentProofCandidate`, then routes it through the same deterministic `submit-lean` verifier/admission/block/replay path. The row must keep `trusted == false` and `invalidAccepted == 0`.

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

Both leaderboard wrappers use `scripts/benchmark-runner.py`, emit machine-readable JSON, and can write a Markdown leaderboard via `LEADERBOARD_MD`. Public rows report `blockProductionRate = blocksProduced / generatedAttempts * 100`; `accepted` and `verifiedShares` remain diagnostic-only and are not public ranking criteria.

Model benchmark artifacts also expose the controlled mining path explicitly:

```text
row.miningPath.targetIssued
row.miningPath.modelGenerated
row.miningPath.candidateWrapped
row.miningPath.submitLeanInvoked
row.miningPath.verifierAccepted
row.miningPath.canonicalPackageSubmitted
row.miningPath.shareAccepted
row.miningPath.blockProduced
row.miningPath.replayPassed
summary.attemptHierarchy = generatedAttempts -> verifierAccepted -> verifiedShares -> blocksProduced
```

When `--node-url <local-node-base-url>` is provided together with `--submit-lean-command`, the benchmark does not stop at the standalone `submit-lean` runtime result. `submit-lean` emits the deterministic canonical `submissionBody` plus `canonTag`; the benchmark can first POST candidate evidence to `<node-url>/ticket` with `--use-node-ticket`, then POST `{ body, canonTag }` to `<node-url>/submit`, records `row.verifier.ticketHttp` / `row.verifier.nodeHttp`, and uses the local node HTTP response for `shareAccepted`, `blockProduced`, and replay scoring. The same options can be forwarded from `boole-preflight-wizard.py`, `phase7-solo-preflight.sh`, and `preflight-model-benchmark.sh` so one-command preflight runs can preserve controlled-local-node `/ticket → /submit` evidence. If a model-generated attempt cannot produce a canonical `submissionBody`, `/ticket` and `/submit` are not invoked and the row records the verifier rejection before node submission. This is the controlled-local-node path, not a public-network mining claim.

This keeps the public claim precise: generation is only the first step, verifier acceptance is narrower, verified shares are diagnostic, and only calibrated block production is the public score.

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
