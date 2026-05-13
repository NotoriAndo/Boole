use boole_core::{
    audit_submit_receipt_lineages, audit_submit_receipts, canonical_payload_hash_hex,
    SignedEnvelope, SigningKeyV2, SubmitReceipt, SubmitReceiptLineage,
};
use serde::Deserialize;
use serde_json::{json, Value};

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

fn work_payload() -> Value {
    json!({
        "schema": "boole.test.work.v1",
        "shareHash": "0101010101010101010101010101010101010101010101010101010101010101"
    })
}

fn signed_work_for_receipt(receipt: &mut SubmitReceipt, nonce: &str) -> SignedEnvelope {
    let key = SigningKeyV2::from_dev_id("submit-receipt-lineage-test");
    let work_payload = work_payload();
    let request_hash = canonical_payload_hash_hex(&work_payload);
    receipt.session_pk = key.pk_hex();
    receipt.submitted_by = key.pk_hex();
    receipt.nonce = nonce.to_string();
    receipt.request_hash = request_hash.clone();

    key.sign(&json!({
        "schema": "boole.signer.work.v1",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": request_hash,
        "nonce": nonce,
        "workPayload": work_payload,
    }))
    .expect("signed work envelope")
}

#[test]
fn audit_accepts_receipt_bound_to_replayed_block_credit() {
    let fixture = replay_fixture();
    let report = audit_submit_receipts(&fixture.blocks, &[valid_receipt()]).expect("audit passes");

    assert_eq!(report.receipts_checked, 1);
    assert_eq!(report.blocks_checked, fixture.blocks.len() as u64);
    assert!(report.ok);
    assert_eq!(report.evidence.block_heights, vec![0, 1]);
    assert_eq!(
        report.evidence.reward_recipients,
        vec![valid_receipt().reward_recipient]
    );
    assert_eq!(
        report.evidence.request_hashes,
        vec![valid_receipt().request_hash]
    );
    assert_eq!(report.evidence.signed_work_checked, 0);
    assert!(report.evidence.checks.block_chain_continuity);
    assert!(report.evidence.checks.receipt_shape);
    assert!(report.evidence.checks.block_binding);
    assert!(report.evidence.checks.selected_share_binding);
    assert!(report.evidence.checks.reward_credit_binding);
    assert!(!report.evidence.checks.signed_work_lineage);
}

#[test]
fn audit_accepts_signed_work_receipt_lineage_bound_to_replayed_block_credit() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    let signed_work = signed_work_for_receipt(&mut receipt, "n-lineage-1");

    let report = audit_submit_receipt_lineages(
        &fixture.blocks,
        &[SubmitReceiptLineage {
            receipt,
            signed_work,
        }],
    )
    .expect("lineage audit passes");

    assert!(report.ok);
    assert_eq!(report.receipts_checked, 1);
    assert_eq!(report.blocks_checked, fixture.blocks.len() as u64);
    assert_eq!(report.evidence.signed_work_checked, 1);
    assert!(report.evidence.checks.signed_work_lineage);
    assert_eq!(report.evidence.block_heights, vec![0, 1]);
    assert_eq!(
        report.evidence.reward_recipients,
        vec![receipt_reward_recipient()]
    );
}

fn receipt_reward_recipient() -> String {
    valid_receipt().reward_recipient
}

#[test]
fn audit_rejects_lineage_when_signed_work_nonce_differs_from_receipt() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    let signed_work = signed_work_for_receipt(&mut receipt, "n-lineage-1");
    receipt.nonce = "n-lineage-2".to_string();

    let err = audit_submit_receipt_lineages(
        &fixture.blocks,
        &[SubmitReceiptLineage {
            receipt,
            signed_work,
        }],
    )
    .expect_err("nonce mismatch fails");

    assert!(
        err.to_string().contains("nonce mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn audit_rejects_lineage_when_work_payload_hash_differs_from_request_hash() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    let mut signed_work = signed_work_for_receipt(&mut receipt, "n-lineage-1");
    signed_work.payload["workPayload"]["shareHash"] =
        json!("0202020202020202020202020202020202020202020202020202020202020202");
    signed_work = SigningKeyV2::from_dev_id("submit-receipt-lineage-test")
        .sign(&signed_work.payload)
        .expect("resigned tampered payload");

    let err = audit_submit_receipt_lineages(
        &fixture.blocks,
        &[SubmitReceiptLineage {
            receipt,
            signed_work,
        }],
    )
    .expect_err("work payload hash mismatch fails");

    assert!(
        err.to_string().contains("workPayload hash mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn audit_rejects_lineage_when_signed_work_signature_invalid() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    let mut signed_work = signed_work_for_receipt(&mut receipt, "n-lineage-1");
    signed_work.signature = format!("00{}", &signed_work.signature[2..]);

    let err = audit_submit_receipt_lineages(
        &fixture.blocks,
        &[SubmitReceiptLineage {
            receipt,
            signed_work,
        }],
    )
    .expect_err("invalid signature fails");

    assert!(
        err.to_string().contains("signature invalid"),
        "unexpected error: {err}"
    );
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

#[test]
fn audit_rejects_receipt_when_block_ledger_hash_is_tampered_to_match_receipt() {
    let fixture = replay_fixture();
    let mut blocks = fixture.blocks;
    let mut receipt = valid_receipt();
    let tampered_c = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    blocks[0].c = tampered_c.to_string();
    receipt.block_c = tampered_c.to_string();

    let err = audit_submit_receipts(&blocks, &[receipt]).expect_err("tampered block ledger fails");
    assert!(
        err.to_string().contains("block c mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn audit_rejects_receipt_when_reward_recipient_is_uncredited_even_with_zero_amount() {
    let fixture = replay_fixture();
    let mut receipt = valid_receipt();
    receipt.reward_recipient =
        "4444444444444444444444444444444444444444444444444444444444444444".to_string();
    receipt.reward_amount = "0".to_string();

    let err = audit_submit_receipts(&fixture.blocks, &[receipt])
        .expect_err("uncredited reward recipient fails");
    assert!(
        err.to_string().contains("rewardRecipient not credited"),
        "unexpected error: {err}"
    );
}
