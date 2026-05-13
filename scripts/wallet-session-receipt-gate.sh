#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo test -q -p boole-core --test session_policy --test receipt
cargo test -q -p boole-cli --test keys --test keys_sign --test keys_verify --test session_key --test signer
cargo test -q -p boole-node --test session_store --test session_route --test submit_session_policy --test receipt_route --test verify_answer_route --test agent_passport_events

printf 'wallet-session-receipt-gate: PASS\n' >&2
