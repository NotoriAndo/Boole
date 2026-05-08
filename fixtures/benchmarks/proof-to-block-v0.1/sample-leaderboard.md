# Proof-to-Block Benchmark v0.1 Sample Leaderboard

Sample benchmark artifact for public docs. This is fixture/mock evidence for the benchmark pipeline, not real model performance and not public-network mining.

- sampleOnly: true
- claim boundary: pipeline sample, not real model performance
- public score: blockProductionRate = blocksProduced / generatedAttempts
- blockProductionRate: 1/2 (50.00%)
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
- blocksProduced: 1
- blockProduced: true
- replayPass: true

### 2. ollama-llama3-2-fake-rejected

- provider: ollama
- model: llama3.2:fake
- source: fixture/mock
- status: REJECTED
- generatedAttempt: true
- blocksProduced: 0
- blockProduced: false
- replayPass: true
