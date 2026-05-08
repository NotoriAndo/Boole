#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

require_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    printf 'docs-smoke: missing required file: %s\n' "$path" >&2
    return 1
  fi
}

require_text() {
  local path="$1"
  local needle="$2"
  if ! grep -Fq -- "$needle" "$path"; then
    printf 'docs-smoke: missing %q in %s\n' "$needle" "$path" >&2
    return 1
  fi
}

require_file README.md
require_file install.sh
require_file docs/install.md
require_file docs/proof-to-block-benchmark.md
require_file docs/local-ollama-benchmark.md
require_file docs/benchmarks/proof-to-block-v0.1-sample.md
require_file fixtures/benchmarks/proof-to-block-v0.1/sample-summary.json
require_file fixtures/benchmarks/proof-to-block-v0.1/sample-leaderboard.md
require_file docs/replay-consensus.md
require_file docs/adr/0001-pofp-v2-canonical-widening.md

require_text README.md "docs/install.md"
require_text README.md "docs/replay-consensus.md"
require_text install.sh "installs required dependencies"
require_text install.sh "never prints API key values"
require_text docs/install.md 'Rust `1.95.0`'
require_text docs/install.md 'Lean `leanprover/lean4:v4.29.1`'
require_text docs/install.md "--run-safe-preflight"
require_text docs/install.md "Step 1/7"
require_text docs/install.md "wizard-report.md"
require_text docs/install.md "--allow-paid-api"
require_text docs/install.md "--target safe-core"
require_text docs/install.md "Hermes-style model/runtime picker"
require_text docs/install.md "Diagnostics and recovery"
require_text docs/install.md "Ollama readiness"
require_text docs/install.md "setup-required"
require_text docs/install.md "fix: ollama serve"
require_text docs/install.md "fix: ollama pull qwen2.5-coder:7b"
require_text README.md "Diagnostics and recovery"
require_text README.md "Ollama readiness"
require_text README.md "boole-model-benchmark.py"
require_text README.md "benchmark-rows.ndjson"
require_text README.md "Proof-to-Block Benchmark v0.1 card"
require_text README.md "Which AI agents can create verified work that becomes blocks?"
require_text README.md "fake-command CI path: PASS"
require_text README.md "docs/benchmarks/proof-to-block-v0.1-sample.md"
require_text README.md "docs/local-ollama-benchmark.md"
require_text docs/phase7-solo-preflight.md "seven-step guided plan"
require_text docs/phase7-solo-preflight.md "wizard-summary.redacted.json"
require_text docs/phase7-solo-preflight.md "--target hermes:configured"
require_text docs/proof-to-block-benchmark.md "boole-model-benchmark.py"
require_text docs/proof-to-block-benchmark.md "benchmark-summary.json"
require_text docs/proof-to-block-benchmark.md "benchmark-rows.ndjson"
require_text docs/proof-to-block-benchmark.md "--use-node-ticket"
require_text docs/proof-to-block-benchmark.md 'Rows with missing required env vars are recorded as `SKIP`'
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "Sample benchmark artifact"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "not real model performance"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "not public-network mining"
require_text docs/benchmarks/proof-to-block-v0.1-sample.md "sample-summary.json"
require_text docs/local-ollama-benchmark.md "Optional local Ollama"
require_text docs/local-ollama-benchmark.md "No automatic model pull"
require_text docs/local-ollama-benchmark.md "No automatic daemon start"
require_text docs/local-ollama-benchmark.md "--model-preset ollama"
require_text fixtures/benchmarks/proof-to-block-v0.1/sample-leaderboard.md "fixture/mock"

require_text docs/replay-consensus.md "selectedShareEvidence"
require_text docs/replay-consensus.md "minShareScoreMultiplierNanos"
require_text docs/replay-consensus.md "fixtures/protocol/replay/v1.json"
require_text docs/replay-consensus.md "fixtures/protocol/replay/v2.json"
require_text docs/replay-consensus.md "legacy/no-evidence replay compatibility"
require_text docs/replay-consensus.md "selected share evidence minShareScore mismatch"
require_text docs/replay-consensus.md "selected share evidence requires minShareScoreMultiplierNanos"

require_text docs/adr/0001-pofp-v2-canonical-widening.md "Status: Implemented"
require_text docs/adr/0001-pofp-v2-canonical-widening.md "POFP-v2 is the default canonical package emitted by the Rust Lean proof bridge"

printf 'docs-smoke: PASS\n' >&2
