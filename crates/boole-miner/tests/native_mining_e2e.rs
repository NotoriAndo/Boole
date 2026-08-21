// A-ROOTED-NATIVE-MINING-E2E-V1 (msg 4206 section 4) -- the five required
// accept/reject shapes for the closed-local, non-consensus native-mining
// pipeline wiring over the frozen RUST-TUPLE-STRUCT-PROJECT-V1 template
// (local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/FREEZE.json). No new
// family and no new checker judgment logic is exercised here -- the checker
// verdict is always supplied by a fixture `NativeChecker`, standing in for
// the already-sealed, byte-for-byte-reused Python checker used for the one
// real closed-local pass. These tests exist to prove the wiring (proof
// intake, binding, receipt assembly, share accounting) is correct, not to
// re-litigate whether the checker itself is correct.

use boole_miner::{
    record_share_if_accepted, run_native_mining_e2e, NativeCandidate, NativeChecker,
    NativeCheckerVerdict, NativeTaskContext, NativeVerdict,
};

// Frozen identity, re-read verbatim from
// local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/FREEZE.json -- not
// invented for this test file.
const FAMILY_VERSION: &str = "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1";
const TEMPLATE_ID: &str = "04dd8453f52dd4da8af1736ad6eceeb82ded2e65dcc1c0600c4b37ef7cf6307a";
const ANCHOR_SHA256: &str = "693f62acfa0626a0831c9133a26fcfc1dbb30922c1ab2036231c42a363cfd7fe";
const CHALLENGE_SHA256: &str = "34ae2f0d0d9af2747c59b444c17b83cb264fce8ead0879378e6aa0eae54e246f";
const CHECKER_DIGEST: &str = "633c6ec891a508813960d6f7d86d446752461f8002aef424290f33550de9cb01";
const POLICY_DIGEST: &str = "cd90c313fad6488307f9fc8c09ecebab6155688699c7777c7ed8c6e1b7be3313";

struct FixedVerdictChecker(NativeCheckerVerdict);

impl NativeChecker for FixedVerdictChecker {
    fn run(&self, _candidate_text: &str, _ctx: &NativeTaskContext) -> NativeCheckerVerdict {
        self.0.clone()
    }
}

fn frozen_ctx() -> NativeTaskContext {
    NativeTaskContext {
        family_version: FAMILY_VERSION.to_string(),
        template_id: TEMPLATE_ID.to_string(),
        anchor_sha256: ANCHOR_SHA256.to_string(),
        challenge_sha256: CHALLENGE_SHA256.to_string(),
        epoch: 0,
        checker_digest: CHECKER_DIGEST.to_string(),
        policy_digest: POLICY_DIGEST.to_string(),
    }
}

fn well_formed_answer() -> String {
    "ACTION: FINAL\n```rust\n// <<< ACFR-PATCH-BEGIN >>>\nlet mut acc: i64 = a0;\nfor it in items {\n    acc = acc.wrapping_mul(mul).wrapping_add(it.0 as i64);\n}\nacc\n// <<< ACFR-PATCH-END >>>\n```\n".to_string()
}

fn valid_candidate() -> NativeCandidate {
    NativeCandidate {
        raw_model_response: well_formed_answer(),
        claimed_template_id: TEMPLATE_ID.to_string(),
        claimed_challenge_sha256: CHALLENGE_SHA256.to_string(),
        claimed_checker_digest: CHECKER_DIGEST.to_string(),
        claimed_policy_digest: POLICY_DIGEST.to_string(),
    }
}

// Required shape 1 -- normal answer, ACCEPT through the full pipeline.
#[test]
fn native_mining_e2e_accepts_valid_candidate_through_full_pipeline() {
    let ctx = frozen_ctx();
    let candidate = valid_candidate();
    let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);

    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    assert!(
        report.proof_intake_accepted,
        "well-formed fenced-rust answer must pass proof intake, got {:?}",
        report.proof_intake_reject_reason
    );
    assert!(
        report.binding_accepted,
        "candidate bound to the frozen template must pass binding, got {:?}",
        report.binding_reject_reason
    );
    assert!(
        report.checker_accepted,
        "checker fixture returns Accept, so checker_accepted must be true"
    );
    assert!(
        report.verify_accepted,
        "intake+binding+checker all agree, so the node verifier must accept"
    );
    assert!(
        matches!(report.receipt.verdict, NativeVerdict::Accept),
        "receipt verdict must be Accept, got {:?}",
        report.receipt.verdict
    );
    assert_eq!(report.receipt.family_version, FAMILY_VERSION);
    assert_eq!(report.receipt.template_id, TEMPLATE_ID);
    assert_eq!(report.receipt.anchor_digest, ANCHOR_SHA256);
    assert_eq!(
        report.receipt.challenge_epoch.challenge_sha256,
        CHALLENGE_SHA256
    );
    assert_eq!(report.receipt.checker_digest, CHECKER_DIGEST);
    assert_eq!(report.receipt.policy_digest, POLICY_DIGEST);
    assert!(
        !report.receipt.candidate_digest.is_empty(),
        "candidate_digest must be bound to the actually-extracted candidate bytes"
    );

    let ledger_path = std::env::temp_dir().join(format!(
        "boole-native-e2e-accept-{}-{}.ndjson",
        std::process::id(),
        line!()
    ));
    let recorded = record_share_if_accepted(&report, &ledger_path);
    assert!(
        recorded.is_some(),
        "an accepted verdict must record exactly one local share ledger entry"
    );
    let _ = std::fs::remove_file(&ledger_path);
}

// Required shape 2 -- a 1-bit tampered answer must REJECT, and the receipt's
// candidate_digest must reflect the tampered bytes (not a stale digest of the
// original answer). The checker fixture stands in for the real compiler,
// which is what would actually detect a tampered patch; this test's own
// scope is that the wiring correctly propagates a checker REJECT into the
// receipt and records no share, not that this specific tamper always fails
// to compile (that is the already-sealed checker's own responsibility).
#[test]
fn native_mining_e2e_rejects_one_bit_tampered_candidate() {
    let ctx = frozen_ctx();
    let mut candidate = valid_candidate();
    let mut bytes = candidate.raw_model_response.into_bytes();
    let flip_at = bytes
        .iter()
        .position(|b| b.is_ascii_alphanumeric())
        .expect("fixture answer has at least one alphanumeric byte to flip");
    bytes[flip_at] ^= 0x01;
    candidate.raw_model_response = String::from_utf8(bytes).expect("still valid utf-8");

    let checker = FixedVerdictChecker(NativeCheckerVerdict::Reject {
        reason: "compile: tampered candidate does not typecheck".to_string(),
    });
    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    assert!(
        report.proof_intake_accepted,
        "tamper is inside the fenced block, intake still runs"
    );
    assert!(
        report.binding_accepted,
        "tamper does not touch the claimed identity fields"
    );
    assert!(
        !report.checker_accepted,
        "checker fixture returns Reject for the tampered candidate"
    );
    assert!(
        !report.verify_accepted,
        "a checker Reject must not flip to verify_accepted=true"
    );
    assert!(
        matches!(report.receipt.verdict, NativeVerdict::Reject { .. }),
        "receipt verdict must be Reject, got {:?}",
        report.receipt.verdict
    );

    let accepted_digest = {
        let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);
        run_native_mining_e2e(&valid_candidate(), &ctx, &checker)
            .receipt
            .candidate_digest
    };
    assert_ne!(
        report.receipt.candidate_digest, accepted_digest,
        "a tampered candidate must bind a different candidate_digest than the untampered one"
    );

    let ledger_path = std::env::temp_dir().join(format!(
        "boole-native-e2e-tamper-{}-{}.ndjson",
        std::process::id(),
        line!()
    ));
    assert!(
        record_share_if_accepted(&report, &ledger_path).is_none(),
        "a rejected verdict must record no share ledger entry"
    );
    assert!(
        !ledger_path.exists(),
        "no ledger file should be created on reject"
    );
}

// Required shape 3 -- an answer actually produced for a different task must
// REJECT even though it is otherwise well-formed. Mirrors the house pattern
// in bounty_lean_verifier.rs's problem_hash check: a proof of an unrelated
// statement must reject, not error and not silently pass.
#[test]
fn native_mining_e2e_rejects_candidate_answered_for_a_different_task() {
    let ctx = frozen_ctx();
    let mut candidate = valid_candidate();
    // A real template_id from the same wave's Stage A/B representative set
    // (STEP3-FREEZE-REPRESENTATIVES.json), not the frozen template this
    // context commits to.
    candidate.claimed_template_id =
        "0033a13fb95b3babf7615d82f6f87465b8ab6824591023f6625433de28013c42".to_string();

    let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);
    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    assert!(
        report.proof_intake_accepted,
        "intake does not inspect claimed identity"
    );
    assert!(
        !report.binding_accepted,
        "a candidate claiming a different template_id must fail binding, got {:?}",
        report.binding_reject_reason
    );
    assert!(
        !report.checker_accepted,
        "binding rejects before the checker is ever consulted"
    );
    assert!(
        !report.verify_accepted,
        "cross-task reuse must not be accepted"
    );
    assert!(matches!(
        report.receipt.verdict,
        NativeVerdict::Reject { .. }
    ));

    let ledger_path = std::env::temp_dir().join(format!(
        "boole-native-e2e-crosstask-{}-{}.ndjson",
        std::process::id(),
        line!()
    ));
    assert!(record_share_if_accepted(&report, &ledger_path).is_none());
    assert!(!ledger_path.exists());
}

// Required shape 4 -- challenge reuse: same template_id, but the candidate
// claims to answer a different challenge digest (the shape policy.py's own
// `alt_challenge_seed` control exists to catch -- swap the requirement while
// leaving the anchor in place). Must REJECT.
#[test]
fn native_mining_e2e_rejects_challenge_reused_candidate() {
    let ctx = frozen_ctx();
    let mut candidate = valid_candidate();
    candidate.claimed_challenge_sha256 =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();

    let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);
    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    assert!(
        !report.binding_accepted,
        "a candidate claiming a different challenge digest must fail binding, got {:?}",
        report.binding_reject_reason
    );
    assert!(
        !report.verify_accepted,
        "challenge reuse must not be accepted"
    );
    assert!(matches!(
        report.receipt.verdict,
        NativeVerdict::Reject { .. }
    ));
}

// Required shape 5 -- checker/policy digest tamper: the candidate's claimed
// checker_digest or policy_digest no longer matches the frozen context, i.e.
// the candidate was produced (or is being replayed) against a checker or
// policy build that is not the one this context commits to. Must REJECT for
// each, independent of what the checker itself would say.
#[test]
fn native_mining_e2e_rejects_checker_or_policy_digest_tamper() {
    let ctx = frozen_ctx();
    let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);

    let mut checker_digest_tampered = valid_candidate();
    checker_digest_tampered.claimed_checker_digest =
        "1111111111111111111111111111111111111111111111111111111111111111".to_string();
    let report_a = run_native_mining_e2e(&checker_digest_tampered, &ctx, &checker);
    assert!(
        !report_a.binding_accepted,
        "a tampered checker_digest claim must fail binding, got {:?}",
        report_a.binding_reject_reason
    );
    assert!(!report_a.verify_accepted);

    let mut policy_digest_tampered = valid_candidate();
    policy_digest_tampered.claimed_policy_digest =
        "2222222222222222222222222222222222222222222222222222222222222222".to_string();
    let report_b = run_native_mining_e2e(&policy_digest_tampered, &ctx, &checker);
    assert!(
        !report_b.binding_accepted,
        "a tampered policy_digest claim must fail binding, got {:?}",
        report_b.binding_reject_reason
    );
    assert!(!report_b.verify_accepted);
}

// Not one of the five required shapes -- a supplementary sanity check that
// an empty/unfenced answer is rejected at proof intake, before binding or
// the checker ever run. Kept minimal (one assertion group), not a new
// required shape.
#[test]
fn native_mining_e2e_rejects_empty_answer_at_proof_intake() {
    let ctx = frozen_ctx();
    let mut candidate = valid_candidate();
    candidate.raw_model_response = "   \n".to_string();
    let checker = FixedVerdictChecker(NativeCheckerVerdict::Accept);

    let report = run_native_mining_e2e(&candidate, &ctx, &checker);

    assert!(!report.proof_intake_accepted);
    assert!(
        !report.binding_accepted,
        "binding must not run once intake rejects"
    );
    assert!(
        !report.checker_accepted,
        "checker must not run once intake rejects"
    );
    assert!(!report.verify_accepted);
}
