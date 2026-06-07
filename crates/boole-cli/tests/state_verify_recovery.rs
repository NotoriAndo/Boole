//! P2.8 closure criterion 1 — the `boole state verify` deep-verify path
//! must not carry stale "follow-up sub-slice" comments after commit
//! `c56b084` wired the real `LeanRunner` re-execution in.
//!
//! Before `c56b084`, `deep_verify.rs` was a read-only inventory pass and
//! its doc comments truthfully said the Lean re-execution "lands in a
//! follow-up sub-slice" and that the inventory "never produces
//! divergences". That work has since landed: `deep_verify_bounty_events`
//! now accepts a `Some(checker_dir)` and routes accepted-lean proof
//! events through `reverify_lean_event`, which both re-runs Lean and
//! emits `DeepVerifyDivergence` records. The stale future-tense comments
//! contradict the live code, so they must be removed or rewritten.
//!
//! This is a source-contract test (not a runtime test): it pins the
//! absence of the stale phrasing so a future reader is never told the
//! deep re-run is unimplemented when it is. §6.5 P2.8 closure criterion 1
//! ("every 'follow-up' / 'TODO' comment in boole state verify paths is
//! either resolved or replaced with a tracked TODO that names the closing
//! slice").

const DEEP_VERIFY_SRC: &str = include_str!("../../boole-node/src/deep_verify.rs");

#[test]
fn deep_verify_source_carries_no_stale_follow_up_sub_slice_comment() {
    let stale = DEEP_VERIFY_SRC.matches("follow-up sub-slice").count();
    assert_eq!(
        stale, 0,
        "stale 'follow-up sub-slice' comments must be removed from \
         deep_verify.rs: commit c56b084 already wired LeanRunner in, so \
         claiming the Lean re-execution is a future sub-slice is false. \
         Found {stale} occurrence(s)."
    );
}

#[test]
fn deep_verify_source_does_not_claim_inventory_never_diverges() {
    // The pre-c56b084 comment asserted the pass "never produces
    // divergences". reverify_lean_event now pushes DeepVerifyDivergence
    // records, so that claim is false and must not reappear.
    assert!(
        !DEEP_VERIFY_SRC.contains("never produces divergences"),
        "deep_verify.rs must not claim it never produces divergences: \
         reverify_lean_event emits DeepVerifyDivergence records on the \
         Some(checker_dir) path."
    );
}

#[test]
fn deep_verify_source_does_not_claim_none_is_only_supported_path() {
    // The pre-c56b084 comment said supplying None "is the only supported
    // path". The Some(checker_dir) branch is now fully wired, so this
    // must not be asserted anywhere in the file.
    assert!(
        !DEEP_VERIFY_SRC.contains("only supported path"),
        "deep_verify.rs must not claim None is the only supported path: \
         the Some(checker_dir) branch re-runs Lean via reverify_lean_event."
    );
}

/// The CLI surface (`boole state verify --deep`) carried the same stale
/// future-tense comments as `deep_verify.rs` — the `StateCommand::Verify` doc,
/// the `--lean-checker-dir` arg doc, and the `state_verify_deep` fn doc each
/// claimed the Lean re-run "lands in a follow-up sub-slice" / the flag is
/// "Accepted today but unused" / "today every accepted-lean proof event is
/// reported under `leanProofsSkipped`". Those are false now that
/// `--lean-checker-dir` routes through `reverify_lean_event`. Pin their absence
/// in the CLI source too, not just the node-side `deep_verify.rs`.
const CLI_MAIN_SRC: &str = include_str!("../src/main.rs");

#[test]
fn cli_state_verify_doc_carries_no_stale_follow_up_or_unused_flag_comment() {
    for stale in [
        "follow-up sub-slice",
        "Accepted today but unused",
        "today every accepted-lean",
        "today every eligible event",
    ] {
        assert_eq!(
            CLI_MAIN_SRC.matches(stale).count(),
            0,
            "stale deep-verify doc comment {stale:?} must be removed from \
             boole-cli/src/main.rs: `--lean-checker-dir` already re-executes \
             Lean and fills `leanProofsReverified`, so the future-tense / \
             unused-flag phrasing is false."
        );
    }
}
