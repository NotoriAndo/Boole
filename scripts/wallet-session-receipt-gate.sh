#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo test -q -p boole-core --test session_policy --test receipt
cargo test -q -p boole-cli --test keys --test keys_sign --test keys_verify --test session_key --test signer
# P1.8 — `--features dev-mock-payment` enables the magic test-payment
# string that `verify_answer_route` and `agent_passport_events` exercise.
# Without it, those tests get 403 payment_invalid and the gate fails.
cargo test -q -p boole-node --features dev-mock-payment --test session_store --test session_route --test submit_session_policy --test receipt_route --test verify_answer_route --test agent_passport_events

printf 'wallet-session-receipt-gate: PASS\n' >&2
