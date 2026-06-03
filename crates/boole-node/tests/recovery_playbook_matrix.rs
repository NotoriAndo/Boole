//! P2.8 closure criterion 2 — §7 Recovery-playbook coverage matrix.
//!
//! The production-readiness master plan §7 lists a failure→signal→action
//! runbook. Criterion 2 asks that every row that maps to an IMPLEMENTED
//! recovery path emits the durable signal the table promises, and that
//! rows blocked on unbuilt features are tracked, not silently assumed.
//!
//! The §7 table itself lives in `local-docs/` (gitignored planning
//! space), so this test cannot `include_str!` the canonical table.
//! Instead it pins, in one place, that each IMPLEMENTED row's durable
//! signal string is still present in the production source that emits it
//! — a regression guard so a refactor cannot silently rename a runbook
//! signal out from under an operator following the playbook — and it
//! documents the rows still blocked on unimplemented features so the
//! deferral is explicit rather than an unstated gap.
//!
//! Each implemented row already has a behavioural fault-injection test
//! (named in the comments below). This file is the consolidated runbook
//! contract on top of those, not a replacement for them.

/// Production source that emits the node-side recovery signals.
const LOCAL_NODE_SRC: &str = include_str!("../src/local_node.rs");
/// Production source for the boot-time `state-dir-locked` / lean-checker
/// boot signals.
const NODE_MAIN_SRC: &str = include_str!("../src/main.rs");
/// `boole state verify` CLI that emits the `replay-mismatch` report
/// signal an operator runs at the "reward/block divergence" row.
const CLI_MAIN_SRC: &str = include_str!("../../boole-cli/src/main.rs");

// ---------------------------------------------------------------------
// Implemented rows: the durable signal string must stay present in the
// source that emits it. Each assertion names the behavioural test that
// actually fault-injects the row.
// ---------------------------------------------------------------------

#[test]
fn row_two_nodes_one_state_dir_signal_present() {
    // §7 row "Two nodes on one state dir" → signal `state-dir-locked`.
    // Behavioural test: state_dir_lock_blocks_second_node.rs (asserts the
    // second node exits non-zero with `state-dir-locked` on stderr).
    assert!(
        NODE_MAIN_SRC.contains("state-dir-locked"),
        "recovery playbook: the `state-dir-locked` boot signal must stay \
         present in boole-node/src/main.rs; an operator following §7 \
         relies on this exact string to identify a duplicate-node boot."
    );
}

#[test]
fn row_reward_block_divergence_signals_present() {
    // §7 row "Reward / block divergence at boot" → `/ready` 503 with
    // reason `replay_runtime_mismatch`, plus `boole state verify`
    // reporting `replay-mismatch`.
    // Behavioural tests: ready_returns_503_when_replay_runtime_mismatched.rs
    // and state_verify_cli.rs (cli_state_verify_rejects_a_tampered_block_log).
    assert!(
        LOCAL_NODE_SRC.contains("replay_runtime_mismatch"),
        "recovery playbook: the `/ready` divergence signal \
         `replay_runtime_mismatch` must stay present in local_node.rs."
    );
    assert!(
        CLI_MAIN_SRC.contains("replay-mismatch"),
        "recovery playbook: `boole state verify` must still report \
         `replay-mismatch` so the §7 escalation step (report-json) works."
    );
}

#[test]
fn row_lean_checker_missing_signal_present() {
    // §7 row "Lean checker dir missing" → `/ready` 503 reason
    // `lean_checker_not_configured`.
    // Behavioural tests: ready_returns_503_when_lean_checker_dir_path_missing.rs
    // and ready_returns_503_when_lean_checker_not_configured.rs.
    assert!(
        LOCAL_NODE_SRC.contains("lean_checker_not_configured"),
        "recovery playbook: the lean-checker-missing readiness signal \
         `lean_checker_not_configured` must stay present in local_node.rs."
    );
}

#[test]
fn row_torn_last_line_truncation_helper_present() {
    // §7 row "Torn last line on any ledger" → automatic truncation.
    // Behavioural tests: every store's *_durability.rs / store_fixtures.rs
    // (block, reward, receipt, session, reputation, bounty_event) plus the
    // internal #[cfg(test)] mods in nonce_ledger.rs / signed_nonce_ledger.rs.
    // The shared mechanism is `read_stable_prefix`; if it disappears the
    // whole row regresses, so pin its presence as the single source of the
    // truncate-torn-tail behaviour.
    assert!(
        LOCAL_NODE_SRC.contains("read_stable_prefix")
            || include_str!("../src/durability.rs").contains("fn read_stable_prefix"),
        "recovery playbook: the torn-tail truncation primitive \
         `read_stable_prefix` must remain the durable-read entry point."
    );
}

// ---------------------------------------------------------------------
// Deferred rows: blocked on features not yet built. This test documents
// the deferral so the §7 gap is explicit. When the blocking wave lands,
// replace the corresponding assertion with a real fault-injection test
// and delete the entry here.
// ---------------------------------------------------------------------

#[test]
fn deferred_rows_are_tracked_against_their_blocking_wave() {
    // These four §7 rows cannot be fault-injected yet because the feature
    // that produces their signal is unimplemented. Confirm the signal
    // strings are INDEED absent from the node source so this test starts
    // failing (forcing an update) the moment someone wires the feature in.
    //
    //   * "Power loss mid-commit" — needs P1.3b staging-commit resume.
    //   * "Operator lost master password" — `boole wallet restore`, deferred
    //     to P3 by ADR-0002 (P2.9 scope narrowed to init|address|sign|migrate).
    //   * "Schema version drift" — needs `boole storage migrate`.
    let unimplemented_signals = ["staging commit", "schema-version-unsupported"];
    for sig in unimplemented_signals {
        assert!(
            !LOCAL_NODE_SRC.contains(sig),
            "recovery playbook: `{sig}` now appears in local_node.rs — the \
             feature was implemented. Replace the deferred-row entry in \
             recovery_playbook_matrix.rs with a real fault-injection test."
        );
    }
}

/// P2.6 e — "operator's disk fills up mid-mining" is no longer deferred: the
/// `disk_full_sentinel` readiness reason landed with a fault-injection test
/// (`ready_fault_injection::ready_returns_503_when_disk_full_sentinel_is_set`).
#[test]
fn row_disk_full_signal_present() {
    assert!(
        LOCAL_NODE_SRC.contains("disk_full_sentinel"),
        "P2.6: the disk-full readiness reason must be wired into local_node.rs"
    );
}
