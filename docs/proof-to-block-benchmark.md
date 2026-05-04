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

```bash
./scripts/proof-to-block-benchmark.sh
```

The script wraps:

```bash
./scripts/runtime-smoke-all.sh
```

and emits JSON to stdout. Human PASS lines go to stderr.

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

Later model benchmark runs can reuse the same JSON shape and add model/provider/cost/time fields without weakening the runtime consistency checks.
