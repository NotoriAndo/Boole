#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# block-hash/replay fixtures are no longer TS-exported: the v2 block_hash
# preimage (ADR-0014 (a), N5-pre.1) is defined by the Rust implementation,
# not legacy-pof chain.ts — their fixtures are Rust-generated golden vectors.
npx tsx scripts/export-block-builder-fixtures.ts
npx tsx scripts/export-hash-pow-fixtures.ts
npx tsx scripts/export-share-pool-fixtures.ts
npx tsx scripts/export-manifest-fixtures.ts
npx tsx scripts/export-bounty-registry-fixtures.ts
npx tsx scripts/export-bounty-ledger-fixtures.ts
npx tsx scripts/export-config-fixtures.ts
npx tsx scripts/export-validator-fixtures.ts
npx tsx scripts/export-submission-pow-fixtures.ts
npx tsx scripts/export-rejection-log-fixtures.ts
npx tsx scripts/export-rate-limiter-fixtures.ts
# admission/v1.json is Rust-native improved policy fixture (not TS-exported)

cargo fmt --all --check
# P1.8 + P1.9 — `dev-mock-payment` enables the magic test-payment string
# the verify-answer route + passport-event tests rely on; `dev-tools`
# enables the `--mock-verify-accept` miner bypass that mining-loop /
# mine_start_cli tests rely on. Without these flags those tests
# either fail with 403 payment_invalid or fail to find the flag.
cargo test --workspace --locked --features boole-node/dev-mock-payment,boole-miner/dev-tools

echo "rust-parity: PASS"
