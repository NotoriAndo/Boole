#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

npx tsx scripts/export-block-hash-fixtures.ts
npx tsx scripts/export-replay-fixtures.ts
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
cargo test --workspace

echo "rust-parity: PASS"
