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

# EVM census P0 — case-task-binding eligibility freeze (append-only attestation).
# Records the frozen contract, conservation identity, ledger/proof hashes, and the
# closed-local boundary. Originals stay in the git-ignored sandbox; only hashes +
# lineage are tracked here. These pins keep the record's ceiling label, conservation
# identity, and non-activation boundary from silently rotting.
require_file docs/evm-census-p0-eligibility-freeze.md
require_text docs/evm-census-p0-eligibility-freeze.md "EVM-P0-MINEABLE-ELIGIBLE = 6,767"
require_text docs/evm-census-p0-eligibility-freeze.md "S-CONTRACT-FREEZE-v1.5"
require_text docs/evm-census-p0-eligibility-freeze.md "6,855 = 6,767 emitted + 79 duplicate + 7 deferred-lossy + 2 deferred-provenance"
require_text docs/evm-census-p0-eligibility-freeze.md "mineable_now stays 0"
require_text docs/evm-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/evm-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Solidity census P0 — generative task-family eligibility freeze (append-only
# attestation). Records the two sub-families, the pinned soljson.js 0.8.36 verifier,
# the mandatory trivial-construction gate (N=0), the two-stage conservation identity
# (Stage A 12,931 / Stage B 6,652), the 709 successor split, corpus fingerprints, and
# the closed-local boundary. Corpora and family impl stay in the git-ignored sandbox;
# only hashes + lineage are tracked here. These pins keep the record's ceiling label,
# conservation identities, and non-activation boundary from silently rotting.
require_file docs/solidity-census-p0-eligibility-freeze.md
require_text docs/solidity-census-p0-eligibility-freeze.md "SOLIDITY-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/solidity-census-p0-eligibility-freeze.md "INELIGIBLE-TRIVIAL-CONSTRUCTION"
require_text docs/solidity-census-p0-eligibility-freeze.md "pinned soljson.js 0.8.36"
require_text docs/solidity-census-p0-eligibility-freeze.md "12,931 = 6,652 test-file bundle"
require_text docs/solidity-census-p0-eligibility-freeze.md "0 MINEABLE-ELIGIBLE"
require_text docs/solidity-census-p0-eligibility-freeze.md "2,628 NO-FRESH-INSTANCE"
require_text docs/solidity-census-p0-eligibility-freeze.md "1,670 DEFERRED-EVM-REQUIRED"
require_text docs/solidity-census-p0-eligibility-freeze.md "709 SUCCESSOR-OUT-OF-SCOPE"
require_text docs/solidity-census-p0-eligibility-freeze.md "1,670 = 1,498"
require_text docs/solidity-census-p0-eligibility-freeze.md "mineable_now"
require_text docs/solidity-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/solidity-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# zk-native release-audit census P0 — anchor eligibility freeze (append-only
# attestation). Records the two separate ceiling labels (zk-native N=0 and Lean
# corpus-not-materialized), the 16,763-anchor conservation identity, the genuine
# anchor->source binding basis, the reason the v1-lenbound Lean checker is not
# applied, and the closed-local boundary. The anchor ledger / source snapshots /
# emitter stay in the git-ignored sandbox; only hashes + lineage + conservation are
# tracked here. These pins keep the two ceiling labels, the conservation identity,
# and the non-activation boundary from silently rotting.
require_file docs/zk-native-release-audit-census-p0-eligibility-freeze.md
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "LEAN-P0 = CORPUS-NOT-MATERIALIZED"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "16,763 NEEDS-SPEC"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "boole.zk-native.release-audit.v1"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "AuditExisting"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "not a fake seed"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "fab8439a4fa9b5062cebf931e155d68cc469661e3f7e2c9556b7bd07c7792bc6"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "v1-lenbound"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "mineable_now"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "confirmed numeric subtotal"
require_text docs/zk-native-release-audit-census-p0-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Solidity semantic P1 — EVM execution-proof task eligibility freeze (append-only
# attestation). Successor to the compile-only Solidity census P0: materializes the
# 1,670 deferred semanticTests into real EVM execution cases and decides eligibility by
# whether a compressed SP1 proof of correct execution can be produced within an 8M-cycle
# ceiling. Records the ceiling label (N=1,396), the fixed task unit, the frozen guest
# ELF/vk binding, the three-level conservation identity, the corpus fingerprint, and the
# closed-local boundary. Guest/host impl, corpus, materialized cases, run ledgers, and
# the representative proof stay in the git-ignored sandbox; only hashes + lineage +
# conservation are tracked here. These pins keep the ceiling label, the conservation
# identities, the engine binding, and the non-activation boundary from silently rotting.
require_file docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "public expected outputs are not answer leakage"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "plus its ordered full call bundle"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,670 total_files = 1,519 CASES-MATERIALIZED"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,519 = 1,474 COMPILE-MATERIALIZED-CANDIDATE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1,474 = 1,396 MINEABLE-ELIGIBLE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "1599d54fd75ef48742a9ec460628b6caba68d7a4f33a9c615707b713465d37a2"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "0x004a748560e6b44075bd4fc72a0e88bcef34a91c6d3a47b37a4416d13126b207"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "f0af98e63cde61a6399929f38daa70e694aa929f65c28e7071c624ddf9661f28"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "mineable_now"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"
# Entry 2 — execution-mismatch-reclaim-v1 (append-only successor; Entry 1 N=1396 unchanged)
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE-SUCCESSOR = 1408"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "12 EXECUTION-MISMATCH = 5 AUTHOR-ORACLE-MISREAD"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "+ 0 PINNED-ENGINE-DIVERGENCE"
require_text docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md "successor MINEABLE-ELIGIBLE                       = 1408"

# Rust execution-proof P1 — frozen-snapshot task eligibility freeze (append-only
# attestation). Separate successor to the compile-only Rust Reference P0 (which stays
# at RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0, untouched): wraps each runnable rust-lang/
# rust test into a per-edition SP1 guest, executes it under an 8M-cycle ceiling, and
# counts eligibility by whether the guest runs to a fixed 20-byte completion sentinel.
# Records the ceiling label (N = 2,461 files / 2,504 tasks), the fixed task unit, the
# per-task-differing guest ELF/vk binding, the full + census conservation identities at
# both units, the corpus fingerprint, and the closed-local boundary. Guest/host impl,
# corpus, wrapped guests, census ledgers, and the representative proof stay in the
# git-ignored sandbox; only hashes + lineage + conservation are tracked here. These pins
# keep the ceiling label, conservation identities, engine binding, and non-activation
# boundary from silently rotting.
require_file docs/rust-execution-proof-p1-eligibility-freeze.md
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 2,461 files / 2,504 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-FROZEN-SNAPSHOT-MINEABLE-ELIGIBLE = 2,461 (files)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Why public test source is not answer leakage"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "files 26,235 == 26,235"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "tasks 28,735 == 28,735"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2,895 candidates = 2,462 MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Per-task ELF/vk differ"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Task-unit 2,504 is a projection"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "af8c179cf544f544c80eb9a23f19be2006fe2bea931e7f965f1ed77b09f7299c"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0x0038de3b51dcfe81fa915df141a0b5c88e73b727b52f601f2561fe472b947c96"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "e7795af6d2449fb05a6393c3320ced873a999eb3"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "7b7225bf6279fe827e314bb75e6a754ff3c733c444aa581cdea9e46a26df74dd"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "mineable_now"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "issuable problem count, not network activation"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"

# Entry 2 (append-only successor: proof-statement dedup). Pins the successor headline,
# the statement digest definition, the proof-reuse binding gap, the 8-bucket task
# conservation, the canonical constants, and the per-task binding-manifest fingerprints
# so the re-audited count and its evidence cannot silently drift. Entry 1 pins above are
# untouched — both units stay recorded side by side.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "proof-statement-dedup-v1 / N = 687 distinct verifiable proof statements"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 687 distinct verifiable proof statements"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "proof_statement_digest = SHA-256( vk"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "is a **bijection with vk**"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Proof reuse across same-vk tasks is therefore **possible**"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "687  MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "1,690  DUPLICATE-PROOF-STATEMENT"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "122  ANSWERED-PROOF-FIXTURE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "5  NON-EXECUTION-ORACLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "e3b0c442"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "48b1336c"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "79febbe6"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "c1785e0f"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "8229606a"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "-Zrandomize-layout -Zlayout-seed=2"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entry 1 and the preamble above are **byte-unchanged**"

# Entry 3 (append-only retraction of Entry 2's headline). Pins the reclassification of 687
# to a diagnostic UNBOUND-PROOF-STATEMENT-COUNT, the root-cause label, the task-binding v2
# public-values layout, and the redefined content-duplicate criterion. Entries 1 and 2 stay
# byte-unchanged; these pins guard the correction from silently rotting.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task-binding-v2-retraction"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "UNBOUND-PROOF-STATEMENT-COUNT = 687"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "HARNESS-UNBOUND-TASK-IDENTITY"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "687 is NOT a MINEABLE-ELIGIBLE count"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task_binding_digest_committed (32B) || completion_sentinel (20B)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "TRUE-CONTENT-DUPLICATE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entry 1 and Entry 2 remain byte-unchanged"

# Entry 4 (append-only successor to Entry 2's retracted 687). Pins the task-binding v2
# re-census successor value 2,499, the 8-bucket conservation, the pinned identity scheme
# and anchors, the representative verifier battery, and the ledger fingerprints. Entries
# 1-3 and the preamble stay byte-unchanged; these pins guard the successor from rotting.
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "task-binding-v2-recensus"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE-SUCCESSOR = 2,499 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "does not diverge from 2,499"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2,504 = 2,499 MINEABLE-ELIGIBLE"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "TRUE-CONTENT-DUPLICATE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "ANSWERED-PROOF-FIXTURE = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "integrity_failures = 0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "v=rustexec-task-binding-v2"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "038d31ee96e45e0e6fb9b78a6a3670c3851a5197e4704f7c03c257741b2f46c0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "82b17533145056cb19fd89c2c3d3d69b1b691685daf59329888ea2635bae7f21"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0e3fd2261daf27f542764acf78ecbeeb3b51075aec12dc2b87dff7780489b465"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "8529068f51fc37bef8df5d135148218b783667fea3114fcd18e5044548dc1a9a"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "0x00916c22e8283a8801c8e75c3beb6a7511974816130d8e939973badb51389f39"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "4ed8e2e774bb19fc2d2107168aa6c5e208936d7ce7750fc8f2582facb038e6ca"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "9d7662622cc57e07e4d176166a5f213dca68c3a97f2b58650af882c162124af7"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "cross-use-to-other-task"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entries 1-3 and the preamble remain byte-unchanged"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Persistent-worker equivalence gate (execution-method lineage)"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "pure-persistent re-execution of all 2,504 tasks"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "two cryptographically-equivalent execution methods"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "56bed1635849c2195ef189f0ba2c5df3313ead5e0c5dc7cdc3b29cad155cdc99"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "2c9e6221bb7e6e61340745ef201dc3b09cb9570896ab624e730a65b58dbff2da"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "b3060c0ac92510618486b6ace38dc7096d4368e4c3720e8f4909254b3104fe7b"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "12f40f111fb8c9e54f9dc1f15ac4b875e27f4716a95b37d6b8469b456e63beb0"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "execution-method framing correction"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "authoritative execution running each of the 2,504 tasks exactly once"
require_text docs/rust-execution-proof-p1-eligibility-freeze.md "Entries 1-4 and the preamble remain"

# Ethereum-consensus execution-proof P1 — STF-guest gate closure freeze (append-only
# attestation). Records the SP1 riscv64 guest build closure for ONE exact combination
# (ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest):
# corpus materialized (7,111 STF candidates), native gate 279/279, guest build rc=101
# with no official SP1 drop-in for blst 0.3.17 or c-kzg 2.1.8. The domain is recorded as
# NOT-YET-DETERMINED (never 0); the integrated confirmed subtotal 10,674 and
# mineable_now=0 are unchanged; the 7,111 candidates stay a candidate inventory.
require_file docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "ETHEREUM-CONSENSUS-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "CORPUS-MATERIALIZED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-NATIVE-GATE-VALIDATED-279-OF-279"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "5031d31e318dd861cf3373702c5d92f085d926e4"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "de67682195ca04869a61eeb1a57320153fe891ff3092e2ff0b946a66dbbb99fb"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "54,452 = 7,111 STF-CANDIDATE + 47,341 NON-STF-NO-RUNNABLE-TASK"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "integrated confirmed subtotal    10,674"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "does **not** state \"the STF axis is closed\""
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "mineable_now"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "issuable-problem-count investigation, not network activation"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "not a public-network / leaderboard / paid-API / production claim"
# Entry 2 — STF crypto-call-path census (append-only). The frozen instrumented native
# harness ran the 4,231 executable STF candidates exactly once and classified all 7,111
# by observed crypto-call path into six conserving buckets. 2,293 NO-CRYPTO is a candidate
# count, NOT mineable; domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-CRYPTO-CALL-PATH-CENSUS-COMPLETE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "7,111 = 2,293 NO-CRYPTO-REACHED-FOR-FROZEN-INPUT + 1,830 BLS-REQUIRED + 0 KZG-REQUIRED + 0 BLS-AND-KZG-REQUIRED + 2,880 CLIENT-FORK-UNSUPPORTED + 108 UNRESOLVED"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 is a candidate count, not a mineable count"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "d752378350bc3f0aec857ec236ab734e0b834b490ddd426fcadaa4dc6fb69a84"
# Entry 3 — successor one-proof gate PASS (append-only). One SP1 compressed proof of the
# frozen crypto-fail-closed v2 guest on the out-of-corpus calibration fixture was produced
# exactly once (0 retries) within the wall/RSS/size envelope; real vk ACCEPT, cross-task and
# tampered REJECT. Establishes proof-issuance feasibility + resource cost only — NOT a 2,293
# result, NOT mineable. Domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-SUCCESSOR-ONE-PROOF-GATE-PASS"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "proof-issuance feasibility"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "run2-lowmem-serial-d026acb446de"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "50616853d5055eebed2ccadfb843d3429150f04f3f133f0af2585efa645b6b36"
# Entry 4 — 2,293 EXECUTE census conservation PASS (append-only). The frozen shared guest
# ran the full census (1,126 frozen partial + 1 operator-aborted 8,192-slot case + 1,166
# continuation, no --resume); exact partition conserved: 2,292 executed rows all ACCEPT +
# crypto_call_free, the aborted case held as CHUNKING-CANDIDATE. The uniform resource policy
# splits by cycle cost only; the 2,206 are recorded ONLY as MONOLITHIC-CYCLE-BAND-CANDIDATE —
# NO mineable determination. Domain stays NOT-YET-DETERMINED, 10,674 and mineable_now=0 unchanged.
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "STF-SUCCESSOR-2293-EXECUTE-CENSUS-CONSERVATION-PASS"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 = 2,292 MONOLITHIC-EXECUTE-ELIGIBLE + 1 CHUNKING-CANDIDATE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "2,293 = 2,206 MONOLITHIC-CYCLE-BAND-CANDIDATE + 86 CHUNKING-REQUIRED + 1 CHUNKING-CANDIDATE"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "991,194,325"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "NO mineable determination"
require_text docs/ethereum-consensus-execution-proof-p1-eligibility-freeze.md "4cb7d24fbe38ef5f84ec7f3105b688c2cff306a0e7e3bd1efe3d11f13c62726f"

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
