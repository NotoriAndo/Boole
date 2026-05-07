# Proof-to-Block Benchmark v0.1 Sample Leaderboard

Sample benchmark artifact for public docs. This is fixture/mock evidence for the benchmark pipeline, not real model performance and not public-network mining.

- sampleOnly: true
- claim boundary: pipeline sample, not real model performance
- replay: PASS
- invalid accepted: 0
- chain divergence: 0

## Rows

### 1. ollama-qwen2-5-coder-fake

- provider: ollama
- model: qwen2.5-coder:fake
- source: fixture/mock
- status: ACCEPTED
- generatedAttempt: true
- accepted: true
- blocks: 1
- verifiedShares: 1
- replayPass: true

### 2. ollama-llama3-2-fake-rejected

- provider: ollama
- model: llama3.2:fake
- source: fixture/mock
- status: REJECTED
- generatedAttempt: true
- accepted: false
- blocks: 0
- verifiedShares: 0
- replayPass: true
