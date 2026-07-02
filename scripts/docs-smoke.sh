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
require_file docs/settlement-report.md
require_file docs/receipt-commitment.md
require_file docs/verified-answer-local-mvp-closeout.md
require_file docs/dev-mock-payment.md

# P1.8 — the dev-only mock payment doc must carry an unmistakable banner and
# name its feature gate, and receipt-commitment.md must caveat the magic header
# as development-only (not a production payment) with a pointer to the doc.
require_text docs/dev-mock-payment.md "DEVELOPMENT-ONLY. THIS IS NOT A PRODUCTION PAYMENT PATH."
require_text docs/dev-mock-payment.md "dev-mock-payment"
require_text docs/dev-mock-payment.md "VERIFY_ANSWER_PAYMENT_SIGNATURE"
require_text docs/dev-mock-payment.md "working payment system"
require_text docs/receipt-commitment.md "development-only"
require_text docs/receipt-commitment.md "dev-mock-payment.md"

require_text README.md "docs/install.md"
require_text README.md "docs/replay-consensus.md"
require_text README.md "docs/settlement-report.md"
require_text README.md "docs/receipt-commitment.md"
require_text README.md "docs/verified-answer-local-mvp-closeout.md"
require_text README.md "Verified-answer local receipt surface"
require_text README.md "Boole can return a local verified-answer receipt commitment for machine-checkable work in a mock/local payment-gated flow."
require_text README.md "POST /verify-answer"
require_text README.md "ReceiptCommitment"
require_text README.md "boole-native-test"
require_text README.md "x402.draft-2"
require_text README.md "wallet-session-receipt-gate.sh"
require_text README.md "not real x402 settlement"
require_text README.md "not public-network mining evidence"
require_text install.sh "installs required dependencies"
require_text install.sh "never prints API key values"
require_text docs/install.md 'Rust `1.95.0`'
require_text docs/install.md 'Lean `leanprover/lean4:v4.29.1`'
require_text docs/install.md "--run-safe-preflight"
require_text docs/install.md "Step 1/7"
require_text docs/install.md "wizard-report.md"
require_text docs/install.md "--allow-paid-api"
require_text docs/install.md "--target safe-core"
require_text docs/install.md "Optional cargo-audit security scan"
require_text docs/install.md "cargo install cargo-audit"
require_text docs/install.md "cargo audit"
require_text docs/install.md "not part of the default installer or self-test gate"
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
require_text README.md "boole-miner"
require_text README.md "proof-intake, canonicalizer, verifier"
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

require_text docs/settlement-report.md "boole chain settlement-report"
require_text docs/settlement-report.md "audit-receipts = full shape-only auditor report"
require_text docs/settlement-report.md "settlement-report = read-only reward/reputation summary"
require_text docs/settlement-report.md "auditMode"
require_text docs/settlement-report.md "lineageRequired"
require_text docs/settlement-report.md "does not verify signed-work lineage"
require_text docs/settlement-report.md "--export-reputation-events"
require_text docs/settlement-report.md "boole.reputation.event.v1"
require_text docs/settlement-report.md "settlement-report-shape-only"
require_text docs/settlement-report.md "lineageVerified"
require_text docs/settlement-report.md "does not mutate reward or reputation ledgers"
require_text docs/settlement-report.md "audit failure suppresses settlement output"
require_text docs/settlement-report.md "not public-network mining"

require_text docs/receipt-commitment.md "ReceiptCommitment"
require_text docs/receipt-commitment.md "verifierHashVersion"
require_text docs/receipt-commitment.md "--receipt-commitment-ledger"
require_text docs/receipt-commitment.md "GET /receipts/{receiptId}"
require_text docs/receipt-commitment.md "POST /verify-answer"
require_text docs/receipt-commitment.md "payment_required"
require_text docs/receipt-commitment.md "boole-native-test"
require_text docs/receipt-commitment.md "x402.draft-2"
require_text docs/receipt-commitment.md "x402_version_unsupported"
require_text docs/receipt-commitment.md "boole.agent.event.v1"
require_text docs/receipt-commitment.md "workAccepted"
require_text docs/receipt-commitment.md "workRejected"
require_text docs/receipt-commitment.md "rewardCredited"
require_text docs/receipt-commitment.md "agentEvents"
require_text docs/receipt-commitment.md "wallet-session-receipt-gate.sh"
require_text docs/receipt-commitment.md "Focused local gate"
require_text docs/receipt-commitment.md "not a session key"
require_text docs/receipt-commitment.md "receipt_not_found"
require_text docs/receipt-commitment.md "humanAnswer"
require_text docs/receipt-commitment.md "not public-network mining evidence"
require_text docs/verified-answer-local-mvp-closeout.md "Verified-answer local MVP closeout"
require_text docs/verified-answer-local-mvp-closeout.md "Batch 4 — Verified Answer product surface: COMPLETE for local MVP"
require_text docs/verified-answer-local-mvp-closeout.md "Batch 5 — Gates/docs: COMPLETE"
require_text docs/verified-answer-local-mvp-closeout.md "Definition of Done status"
require_text docs/verified-answer-local-mvp-closeout.md "NEXT-BATCH.1 — Select the next official batch from operating evidence"
require_text docs/verified-answer-local-mvp-closeout.md "not a feature expansion"

# Design-decision records (ADRs) are operator-internal documents (relocated
# 2026-07-02); their gate pins live outside this public script.

# N0-pre.12 — stale tracked-docs corrections (audit R6): the migration
# status doc carries a supersede banner with current gate figures and the
# parity plan marks D3.2 done.
require_text docs/migration-status-and-next-steps.md "Superseded"
require_text docs/migration-status-and-next-steps.md "casesPassed: 7"
require_text docs/boole-node-cli-parity-plan.md "D3.2 (done"

require_file docs/boole-mcp-e2e.md
require_text docs/boole-mcp-e2e.md "boole-mcp end-to-end smoke (external-user path)"
require_text docs/boole-mcp-e2e.md "closed local smoke; not public-network mining"
require_text docs/boole-mcp-e2e.md "Rust \`1.95.0\`"
require_text docs/boole-mcp-e2e.md "cargo build --release -p boole-mcp --bin boole-mcp"
require_text docs/boole-mcp-e2e.md "boole-mcp --version"
require_text docs/boole-mcp-e2e.md "boole-mcp install --target"
require_text docs/boole-mcp-e2e.md "--dry-run"
require_text docs/boole-mcp-e2e.md "boole-mcp serve --node-url"
require_text docs/boole-mcp-e2e.md "/mcp/tools"
require_text docs/boole-mcp-e2e.md "/mcp/invoke"
require_text docs/boole-mcp-e2e.md "boole.mine"
require_text docs/boole-mcp-e2e.md "boole.status"
require_text docs/boole-mcp-e2e.md '{"state":"idle"}'
require_text docs/boole-mcp-e2e.md '"state":"completed"'
require_text docs/boole-mcp-e2e.md "last_summary"
require_text docs/boole-mcp-e2e.md "RUNTIME_SMOKE_FIXTURE_BYTES"
require_text docs/boole-mcp-e2e.md "tests/fixtures/boole-mcp-e2e/"
require_text docs/boole-mcp-e2e.md "not public-network mining"
require_text docs/boole-mcp-e2e.md "No paid-API calls"

printf 'docs-smoke: PASS\n' >&2
