# Boole node / CLI parity plan

**Status:** active plan document for the current Boole repo. Historical experimental family notes were removed from the active tree so old profiles and benchmark modes cannot be copied back into runtime scripts, tests, or docs by accident.

## Current active path

- Primary miner profile: `v1-lenbound`.
- Primary local command: `boole-miner start --profile v1-lenbound`.
- Benchmark runner modes: `mining` for the active local proof-to-block benchmark, and `smoke` for pipeline-only checks.
- Public/API benchmark or paid model execution requires explicit approval before running.
- Local benchmark/smoke reports are local evidence only and must not be described as public-network mining.

## Boundary rules

- Driver output is raw answer-channel text.
- `ProofIntakeV1` is the only proof-intake boundary.
- Canonicalization runs only after ProofIntake acceptance.
- Verifier acceptance is separate from driver answer and ProofIntake acceptance.
- Share/block success is separate from verifier acceptance.
- Reports must keep these counters distinct: driver answered, proof intake accepted/rejected, verifier accepted/rejected, shares, and blocks.

## Active gates

- Focused Rust gates for touched crates/tests.
- Python script tests for preflight and benchmark orchestration.
- Docs smoke through `./scripts/self-test.sh`.
- Full gate before commit: `RUST_TEST_THREADS=1 ./scripts/self-test.sh`.

## Cleanup policy

Deprecated experimental target families, profiles, benchmark modes, fixtures, and compatibility aliases are not active API. If a path is no longer supported, remove it from runtime code, scripts, fixtures, docs, and smoke metadata instead of keeping an alias. Guard tests in `scripts/test_preflight_orchestration.py` prevent deprecated family terms from re-entering the tracked repository.

## Open work

- D3.2: move `FileBountyEventLedger` ownership out of core into the node/local persistence layer in a separate slice.
- Future bounty/economics integration remains separate from this cleanup and must preserve local/public claim boundaries.
