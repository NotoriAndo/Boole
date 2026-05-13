use boole_core::{audit_submit_receipts, SubmitReceipt};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    blocks: Vec<boole_core::PersistedBlock>,
}

fn replay_fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
        .expect("replay fixture parses")
}

fn valid_receipt() -> SubmitReceipt {
    SubmitReceipt {
        schema: "boole.submit.receipt.v1".to_string(),
        accepted: true,
        route: "/submit".to_string(),
        session_pk: "9999999999999999999999999999999999999999999999999999999999999999".to_string(),
        submitted_by: "9999999999999999999999999999999999999999999999999999999999999999"
            .to_string(),
        nonce: "n-audit-1".to_string(),
        request_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        block_height: 0,
        block_c: "4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d".to_string(),
        share_hash: "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
        proposer_pk: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        reward_recipient: "1111111111111111111111111111111111111111111111111111111111111111"
            .to_string(),
        reward_amount: "1".to_string(),
    }
}

#[test]
fn audit_accepts_receipt_bound_to_replayed_block_credit() {
    let fixture = replay_fixture();
    let report = audit_submit_receipts(&fixture.blocks, &[valid_receipt()]).expect("audit passes");

    assert_eq!(report.receipts_checked, 1);
    assert_eq!(report.blocks_checked, fixture.blocks.len() as u64);
    assert!(report.ok);
}

#[test]
fn audit_rejects_receipt_reward_amount_that_does_not_match_replay() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    receipt.reward_amount = "2".to_string();

    let err =
        audit_submit_receipts(&fixture.blocks, &[receipt]).expect_err("amount mismatch fails");
    assert!(
        err.to_string().contains("rewardAmount mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn audit_rejects_receipt_for_share_not_selected_in_block() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    receipt.share_hash =
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();

    let err =
        audit_submit_receipts(&fixture.blocks, &[receipt]).expect_err("unselected share fails");
    assert!(
        err.to_string().contains("shareHash not selected"),
        "unexpected error: {err}"
    );
}
