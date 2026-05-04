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

cargo fmt --all --check
cargo test --workspace

echo "rust-parity: PASS"
