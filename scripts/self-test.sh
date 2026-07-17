#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# P0.3 invariant: every cargo invocation in this gate runs with a single test
# thread so determinism-sensitive integration tests (durability, replay,
# ledger ordering) cannot interleave. Exported here so child processes inherit
# regardless of how the caller invoked self-test.sh.
export RUST_TEST_THREADS=1

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/boole-self-test.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

run_logged() {
  local name="$1"
  shift
  local log="$TMP_DIR/${name}.log"
  printf 'self-test check %s: RUN\n' "$name" >&2
  if "$@" >"$log" 2>&1; then
    printf 'self-test check %s: PASS\n' "$name" >&2
  else
    local status=$?
    printf 'self-test check %s: FAIL\n' "$name" >&2
    cat "$log" >&2
    return "$status"
  fi
}

run_capture_json() {
  local name="$1"
  local out="$2"
  shift 2
  local err="$TMP_DIR/${name}.err"
  printf 'self-test check %s: RUN\n' "$name" >&2
  if "$@" >"$out" 2>"$err"; then
    cat "$err" >&2
    printf 'self-test check %s: PASS\n' "$name" >&2
  else
    local status=$?
    printf 'self-test check %s: FAIL\n' "$name" >&2
    cat "$err" >&2
    cat "$out" >&2
    return "$status"
  fi
}

run_logged cargo-fmt cargo fmt --all --check
run_logged python-script-tests python3 -m unittest scripts/test_install_script.py scripts/test_ci_workflow_contract.py scripts/test_preflight_orchestration.py scripts/test_model_benchmark.py scripts/test_self_test_contract.py scripts/test_workspace_invariants_contract.py scripts/test_supply_chain_contract.py scripts/test_state_dir_contract.py scripts/test_state_dir_runtime_contract.py scripts/test_storage_durability_contract.py scripts/test_nonce_burn_before_block_contract.py scripts/test_verify_answer_payment_gate_contract.py scripts/test_mock_verify_accept_feature_gate_contract.py scripts/test_multi_store_commit_ordering_contract.py scripts/test_reuse_signal_experiment.py
run_logged docs-smoke ./scripts/docs-smoke.sh
run_logged wallet-session-receipt-gate ./scripts/wallet-session-receipt-gate.sh
# P1.8 + P1.9 — clippy verifies both the no-feature production surface
# (no magic test-payment string, no mock verifier bypass) and the
# dev-features surface that tests exercise. Splitting the clippy stage
# keeps the release-mode build honest; a regression that re-introduces
# unconditional access to either gated surface would only fail the
# dev-feature build, not production.
run_logged cargo-clippy cargo clippy --workspace --all-targets --locked -- -D warnings
run_logged cargo-clippy-dev-features cargo clippy --workspace --all-targets --locked --features boole-node/dev-mock-payment,boole-miner/dev-tools -- -D warnings
# Pre-build the cargo-test target set so the next stage can warm the
# macOS dyld codesign cache before the CLI binaries are executed. On
# macOS, cargo's atomic-rename-on-build invalidates the kernel's
# signature cache for every fresh binary; the first `execve` then
# blocks for 30-60s inside `_dyld_start` while the kernel re-verifies
# the signature. Prewarming each CLI binary's `--help` once pays that
# verification cost before downstream stages depend on the binaries.
# Scope is limited to the production CLI surface (`boole-node`,
# `boole-cli`, `boole-miner`); the test harness binaries under
# `target/debug/deps/` are not prewarmed because their startup cost is
# amortized inside `cargo test` itself and the open-ended list of
# integration test binaries is full of hang-prone entry points that
# break the prewarm budget without speeding anything up. The
# `timeout 60` is a hard safety net: a CLI binary that cannot answer
# `--help` in 60s indicates a regression (default behavior accidentally
# starts long-running work, signal handler swallows the request, etc.)
# and fails the gate rather than being silently ignored.
# P1.8 + P1.9 - cargo test runs with the dev features enabled so the
# verify-answer integration tests (magic test-payment header) and the
# boole-miner mining-loop tests (`AcceptingVerifier` bypass) can build
# and run. The no-feature production build is covered by `cargo-clippy`
# above; the cargo-test stage's job is to exercise full test coverage.
run_logged cargo-test-build cargo test --workspace --all-targets --locked --features boole-node/dev-mock-payment,boole-miner/dev-tools --no-run
run_logged cargo-test-prewarm bash -c '
  set -euo pipefail
  for bin in target/debug/boole-node target/debug/boole-cli target/debug/boole-miner; do
    if [[ ! -x "$bin" ]]; then
      printf "prewarm: %s is missing or not executable\n" "$bin" >&2
      exit 1
    fi
    if ! /usr/bin/env timeout 60 "$bin" --help >/dev/null 2>&1; then
      status=$?
      printf "prewarm: %s --help failed (exit %d)\n" "$bin" "$status" >&2
      exit "$status"
    fi
  done
'
# SC.10-iv-a — the Lean toolchain is REQUIRED by this gate. Several
# lake-gated suites (verdict_corpus, the checker-pin boot tests) self-skip
# green when lake/lean are missing, so without an explicit probe the gate
# could only fail *incidentally* (lean-checker-build exiting 127) — and a
# future removal or reordering of that build stage would let the required
# lane go green having never executed Lean ("silent skip-green"). Probe
# both binaries here, before every lake consumer, so a missing toolchain
# is a typed gate failure on a stage that names the cause.
run_logged lean-toolchain-required bash -c '
  set -euo pipefail
  lake --version
  lean --version
'
# The cargo-test stage below runs deep_verify_block_roundtrip, which re-runs
# the Lean checker (`lake exec boole_check`) on a re-derived proof that imports
# `Boole.Family.V0Helpers`. The checker's `.lake/build` is gitignored, so a
# fresh CI runner has no prebuilt olean: the import fails ("unknown module
# prefix 'Boole'"), the checker rejects the proof, and the share re-verifies as
# accepted=false (DeepVerifyDivergence). A developer's already-warm
# `.lake/build` hides this locally, so the gate passes on a laptop but the same
# commit fails on a clean runner. Prebuild the imported module's olean and the
# checker exe here so the local gate and a fresh CI runner share the same
# precondition. This is a gate-time build step only; no runtime code performs a
# `lake build`.
run_logged lean-checker-build bash -c '
  set -euo pipefail
  cd lean/checker
  lake build Boole.Family.V0Helpers boole_check
'
run_logged cargo-test cargo test --workspace --all-targets --locked --features boole-node/dev-mock-payment,boole-miner/dev-tools
# SC.8 (GAP-13) — the multiprocess suites are `#[ignore = "needs-multiprocess"]`
# so the default `cargo test` above skips them; without this stage the core
# N5.2 behaviours (genesis boot binding, share/block gossip, initial sync)
# run in NO required gate. Run each binary explicitly with --include-ignored,
# serially (RUST_TEST_THREADS=1 is exported) to avoid loopback port contention
# across binaries. Named by binary (not `--include-ignored` workspace-wide) so
# a new multiprocess test file cannot be added silently outside this lane.
for mp_test in \
  genesis_network_binding \
  p2p_block_propagation \
  p2p_initial_sync \
  p2p_share_propagation; do
  run_logged "needs-multiprocess-${mp_test}" \
    cargo test -p boole-node --test "$mp_test" --locked -- --include-ignored
done
# SC.9c (ADR-0016 (a)/(a-1)) — the verdict corpus: the three-state Lean
# verdict must match the committed golden fixture byte-for-byte. The
# cross-platform four-job matrix lives in verdict-corpus.yml (the required
# `verdict-corpus` aggregate); this stage keeps the corpus failing loudly,
# by name, inside the single-command local gate too. Already-built by the
# cargo-test stage above, so this re-run costs seconds.
run_logged verdict-corpus cargo test -p boole-lean-runner --test verdict_corpus --locked
LEGACY_POF_ROOT="${BOOLE_LEGACY_POF_ROOT:-$ROOT/../pof}"
LEGACY_CHAIN_TS="$LEGACY_POF_ROOT/dispatcher/src/chain.ts"
RUST_PARITY_STATUS="pass"
if [[ "${BOOLE_SKIP_RUST_PARITY:-0}" == "1" ]]; then
  printf 'self-test check rust-parity: SKIP (BOOLE_SKIP_RUST_PARITY=1)\n' >&2
  RUST_PARITY_STATUS="skipped"
elif [[ ! -f "$LEGACY_CHAIN_TS" ]]; then
  printf 'self-test check rust-parity: SKIP (legacy TypeScript reference not present at %s)\n' "$LEGACY_CHAIN_TS" >&2
  RUST_PARITY_STATUS="skipped"
else
  run_logged rust-parity ./scripts/check-rust-parity.sh
fi

SMOKE_JSON="$TMP_DIR/runtime-smoke-all.json"
BENCH_JSON="$TMP_DIR/proof-to-block-benchmark.json"
run_capture_json runtime-smoke-all "$SMOKE_JSON" ./scripts/runtime-smoke-all.sh
run_capture_json proof-to-block-benchmark "$BENCH_JSON" ./scripts/proof-to-block-benchmark.sh
MINING_JSON="$TMP_DIR/local-mining-smoke.json"
run_capture_json local-mining-smoke "$MINING_JSON" ./scripts/local-mining-smoke.sh
# N3.5 — three statically-peered local nodes must converge to the
# identical head with zero replay divergence (shares driven into two
# different nodes; exercises share gossip + block announce/pull + sync).
CONVERGENCE_JSON="$TMP_DIR/p2p-convergence.json"
run_capture_json p2p-convergence "$CONVERGENCE_JSON" ./scripts/p2p-local-convergence-smoke.sh
# SC.10-iv-b — first live boot of a checker-pinned named network: the
# SC.9b checker-pin + executable-toolchain gate and the N5.2 genesis gate
# run for real (bootable at all only since SC.10-iv-0), and the
# diverged-genesis refusal control proves the gate did not soften.
PINNED_BOOT_JSON="$TMP_DIR/testnet2-pinned-boot.json"
run_capture_json testnet2-pinned-boot "$PINNED_BOOT_JSON" ./scripts/testnet2-pinned-boot-smoke.sh
# SC.10-iv-c — the SC.10 completion gate (mandatory): a structurally-valid
# but proof-invalid share injected into the checker-pinned named network via
# a checker-off faulty producer must be adopted by NO honest node — every
# honest node observably rejects the gossiped invalid block at ingest Lean
# re-verify, while the honest differential control still converges. iv-b
# proves live Lean accepts; this proves it rejects.
LEAN_INVALID_JSON="$TMP_DIR/testnet2-lean-invalid-injection.json"
run_capture_json testnet2-lean-invalid-injection "$LEAN_INVALID_JSON" ./scripts/testnet2-lean-invalid-injection-smoke.sh
# SC.10-iii-c-2 — verified-prefix checkpoint assumevalid re-sync skip: a node
# that Lean-re-verified a prefix, then re-bootstraps (store wiped, checkpoint
# kept), re-syncs that prefix WITHOUT re-running the pinned checker (skip
# counter 0 -> 1), re-converging to the same head.
CHECKPOINT_RESYNC_JSON="$TMP_DIR/testnet2-checkpoint-resync.json"
run_capture_json testnet2-checkpoint-resync "$CHECKPOINT_RESYNC_JSON" ./scripts/testnet2-checkpoint-resync-skip-smoke.sh
# SC.10-iii-d — a verified-prefix checkpoint that no longer matches the actual
# chain (rollback / reorg divergence) must NOT be reused to skip
# re-verification: the node discards it and re-runs the pinned checker (skip
# counter stays 0), still converging to the real head.
CHECKPOINT_DIVERGE_JSON="$TMP_DIR/testnet2-checkpoint-diverge.json"
run_capture_json testnet2-checkpoint-diverge "$CHECKPOINT_DIVERGE_JSON" ./scripts/testnet2-checkpoint-divergence-discard-smoke.sh
run_logged git-diff-check git diff --check

GITLEAKS_STATUS="skipped"
if command -v gitleaks >/dev/null 2>&1; then
  run_logged gitleaks gitleaks detect --redact --verbose --no-banner
  GITLEAKS_STATUS="pass"
fi

python3 - "$SMOKE_JSON" "$BENCH_JSON" "$MINING_JSON" "$GITLEAKS_STATUS" "$RUST_PARITY_STATUS" "$CONVERGENCE_JSON" "$PINNED_BOOT_JSON" "$LEAN_INVALID_JSON" "$CHECKPOINT_RESYNC_JSON" "$CHECKPOINT_DIVERGE_JSON" <<'PY'
import json
import sys

smoke = json.load(open(sys.argv[1]))
benchmark = json.load(open(sys.argv[2]))
mining = json.load(open(sys.argv[3]))
gitleaks_status = sys.argv[4]
rust_parity_status = sys.argv[5]
convergence = json.load(open(sys.argv[6]))
pinned_boot = json.load(open(sys.argv[7]))
lean_invalid = json.load(open(sys.argv[8]))
checkpoint_resync = json.load(open(sys.argv[9]))
checkpoint_diverge = json.load(open(sys.argv[10]))

cases = smoke.get("cases", [])
summary = benchmark.get("summary", {})
safety = benchmark.get("safety", {})

checks = [
    {"name": "cargo-fmt", "ok": True},
    {"name": "python-script-tests", "ok": True},
    {"name": "docs-smoke", "ok": True},
    {"name": "wallet-session-receipt-gate", "ok": True},
    {"name": "cargo-clippy", "ok": True},
    {"name": "cargo-test", "ok": True},
    {"name": "rust-parity", "ok": rust_parity_status in {"pass", "skipped"}, "status": rust_parity_status},
    {
        "name": "runtime-smoke-all",
        "ok": smoke.get("ok") is True,
        "caseCount": smoke.get("caseCount"),
        "casesPassed": sum(1 for case in cases if case.get("ok") is True and case.get("accepted") is True),
    },
    {
        "name": "proof-to-block-benchmark",
        "ok": benchmark.get("ok") is True,
        "claimBoundary": benchmark.get("claimBoundary"),
        "publicMiningEvidence": benchmark.get("publicMiningEvidence"),
        "publicScoringEligible": benchmark.get("publicScoringEligible"),
        "ineligibilityReasons": benchmark.get("ineligibilityReasons"),
        "casesPassed": summary.get("casesPassed"),
        "blocksProduced": summary.get("blocksProduced"),
        "replayFailures": summary.get("replayFailures"),
        "invalidAccepted": safety.get("invalidAccepted"),
        "chainDivergence": safety.get("chainDivergence"),
    },
    {
        "name": "local-mining-smoke",
        "ok": mining.get("ok") is True,
        "claimBoundary": mining.get("claimBoundary"),
        "publicMiningEvidence": mining.get("publicMiningEvidence"),
        "publicScoringEligible": mining.get("publicScoringEligible"),
        "ineligibilityReasons": mining.get("ineligibilityReasons"),
        "miner": mining.get("miner"),
        "blocksMined": mining.get("blocksMined"),
        "finalHeight": mining.get("finalHead", {}).get("height"),
    },
    {
        "name": "p2p-convergence",
        "ok": convergence.get("ok") is True,
        "claimBoundary": convergence.get("claimBoundary"),
        "publicMiningEvidence": convergence.get("publicMiningEvidence"),
        "peers": convergence.get("peers"),
        "convergedHeight": convergence.get("convergedHeight"),
        "convergedHead": convergence.get("convergedHead"),
        "replayDivergence": convergence.get("replayDivergence"),
    },
    {
        "name": "testnet2-pinned-boot",
        # SC.10-iv gate condition: real Lean must have executed in this
        # lane (leanReverified >= 1) with ZERO skips — a skip-green run
        # must fail the aggregate, not pass silently.
        "ok": pinned_boot.get("ok") is True
        and pinned_boot.get("bootRefusedOnDivergedGenesis") is True
        and pinned_boot.get("leanReverified", 0) >= 1
        and pinned_boot.get("sharesSkipped") == 0
        and pinned_boot.get("leanProofsSkipped") == 0,
        "claimBoundary": pinned_boot.get("claimBoundary"),
        "publicMiningEvidence": pinned_boot.get("publicMiningEvidence"),
        "networkId": pinned_boot.get("networkId"),
        "checkerPinned": pinned_boot.get("checkerPinned"),
        "ready": pinned_boot.get("ready"),
        "bootRefusedOnDivergedGenesis": pinned_boot.get("bootRefusedOnDivergedGenesis"),
        "leanReverified": pinned_boot.get("leanReverified"),
        "sharesSkipped": pinned_boot.get("sharesSkipped"),
        "leanProofsSkipped": pinned_boot.get("leanProofsSkipped"),
    },
    {
        "name": "testnet2-lean-invalid-injection",
        # SC.10 mandatory gate: the injected proof-invalid block must be
        # adopted by NO honest node, every honest node must OBSERVABLY
        # reject it at ingest re-verification, and the honest differential
        # control must still converge to height 1. A run that merely
        # completed (ok=true) without these must fail the aggregate.
        # SC.10-iii-b also gates here: the ingesting honest node's
        # verified-prefix checkpoint advanced to 1 (Lean re-verify at ingest),
        # while self-produce and the rejected injection advanced nothing.
        "ok": lean_invalid.get("ok") is True
        and lean_invalid.get("invalidBlockAdoptedBy") == 0
        and lean_invalid.get("invalidBlockRejectedByIngest") is True
        and lean_invalid.get("honestConvergedHeight") == 1
        and lean_invalid.get("checkpointAdvancedOnIngest") is True
        and lean_invalid.get("checkpointNotAdvancedOnSelfProduce") is True
        and lean_invalid.get("checkpointNotAdvancedOnReject") is True,
        "claimBoundary": lean_invalid.get("claimBoundary"),
        "publicMiningEvidence": lean_invalid.get("publicMiningEvidence"),
        "networkId": lean_invalid.get("networkId"),
        "honestNodes": lean_invalid.get("honestNodes"),
        "faultyProducers": lean_invalid.get("faultyProducers"),
        "invalidBlockAdoptedBy": lean_invalid.get("invalidBlockAdoptedBy"),
        "invalidBlockRejectedByIngest": lean_invalid.get("invalidBlockRejectedByIngest"),
        "honestConvergedHeight": lean_invalid.get("honestConvergedHeight"),
        "checkpointAdvancedOnIngest": lean_invalid.get("checkpointAdvancedOnIngest"),
        "checkpointNotAdvancedOnSelfProduce": lean_invalid.get("checkpointNotAdvancedOnSelfProduce"),
        "checkpointNotAdvancedOnReject": lean_invalid.get("checkpointNotAdvancedOnReject"),
        "ingesterCheckpointHeight": lean_invalid.get("ingesterCheckpointHeight"),
    },
    {
        "name": "testnet2-checkpoint-resync",
        # SC.10-iii-c-2 gate: a node re-bootstrapping (store wiped, checkpoint
        # kept) re-syncs its verified prefix WITHOUT re-running Lean — the
        # first ingest ran the checker (skip 0), the re-sync skipped it
        # (skip >= 1), and the node re-converged to the same head.
        "ok": checkpoint_resync.get("ok") is True
        and checkpoint_resync.get("skipCounterAfterFirstIngest") == 0
        and checkpoint_resync.get("reverifySkippedOnResync") is True
        and checkpoint_resync.get("headMatchesFirstVerified") is True
        and checkpoint_resync.get("resyncedHeight") == 1,
        "claimBoundary": checkpoint_resync.get("claimBoundary"),
        "publicMiningEvidence": checkpoint_resync.get("publicMiningEvidence"),
        "networkId": checkpoint_resync.get("networkId"),
        "checkpointHeightBeforeRestart": checkpoint_resync.get("checkpointHeightBeforeRestart"),
        "skipCounterAfterFirstIngest": checkpoint_resync.get("skipCounterAfterFirstIngest"),
        "skipCounterAfterResync": checkpoint_resync.get("skipCounterAfterResync"),
        "reverifySkippedOnResync": checkpoint_resync.get("reverifySkippedOnResync"),
        "headMatchesFirstVerified": checkpoint_resync.get("headMatchesFirstVerified"),
    },
    {
        "name": "testnet2-checkpoint-diverge",
        # SC.10-iii-d gate: a checkpoint that no longer matches the actual
        # chain (rollback / reorg divergence) is NOT reused to skip — the node
        # re-verifies (skip counter stays 0) and still converges to the real
        # head. block_store_rollback_cannot_reuse_future_checkpoint.
        "ok": checkpoint_diverge.get("ok") is True
        and checkpoint_diverge.get("divergentCheckpointNotReused") is True
        and checkpoint_diverge.get("skipCounterAfterResync") == 0
        and checkpoint_diverge.get("convergedToRealHead") is True,
        "claimBoundary": checkpoint_diverge.get("claimBoundary"),
        "publicMiningEvidence": checkpoint_diverge.get("publicMiningEvidence"),
        "networkId": checkpoint_diverge.get("networkId"),
        "checkpointTampered": checkpoint_diverge.get("checkpointTampered"),
        "skipCounterAfterResync": checkpoint_diverge.get("skipCounterAfterResync"),
        "divergentCheckpointNotReused": checkpoint_diverge.get("divergentCheckpointNotReused"),
        "convergedToRealHead": checkpoint_diverge.get("convergedToRealHead"),
    },
    {"name": "git-diff-check", "ok": True},
    {"name": "gitleaks", "ok": gitleaks_status in {"pass", "skipped"}, "status": gitleaks_status},
]

out = {
    "ok": all(check.get("ok") is True for check in checks),
    "checks": checks,
}
print(json.dumps(out, separators=(",", ":")))
if not out["ok"]:
    raise SystemExit(1)
PY

printf 'self-test: PASS\n' >&2
