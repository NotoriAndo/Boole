# Proof-to-Block Benchmark v0.1 Sample Artifact

This is a **Sample benchmark artifact** for Boole's public documentation. It is generated from deterministic fake/local command semantics and demonstrates the benchmark pipeline shape. It is **not real model performance**, **not public-network mining**, and **not a token/reward claim**.

## What this sample proves

```text
fake/local model row → generated Lean candidate → submit-lean verifier path → accepted/rejected row → replay-safe summary
```

The sample is useful for GitHub/landing-page readers because it shows what Boole's Proof-to-Block artifacts look like without requiring Ollama, a paid API key, a wallet, or a live network.

## Safety metrics

- replay: PASS
- invalid accepted: 0
- chain divergence: 0
- replay failures: 0

## Included files

- [`sample-summary.json`](../../fixtures/benchmarks/proof-to-block-v0.1/sample-summary.json)
- [`sample-leaderboard.md`](../../fixtures/benchmarks/proof-to-block-v0.1/sample-leaderboard.md)

## Sample row interpretation

- `qwen2.5-coder:fake` is a fixture/mock model label, not a measured local model result.
- `ACCEPTED` means the fake verifier fixture returned a share/block/replay-success envelope that exercises Boole's row normalization and report path.
- `REJECTED` means generated attempts can be recorded without becoming verified shares or blocks.
- The sample demonstrates artifact semantics; live local model attempts should be labeled separately as local preflight evidence.

## Public wording boundary

Use:

```text
Sample Proof-to-Block artifact demonstrating Boole's verifier/replay benchmark pipeline.
```

Do not use:

```text
A real Ollama model mined Boole blocks.
```
