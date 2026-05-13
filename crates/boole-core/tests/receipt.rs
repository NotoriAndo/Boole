use boole_core::{ReceiptCommitment, ReceiptCommitmentInput};
use serde_json::json;

const AGENT_PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ARTIFACT_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REQUEST_HASH: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const REWARD_RECIPIENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

fn fixture_input() -> ReceiptCommitmentInput {
    ReceiptCommitmentInput {
        agent_pk: AGENT_PK.to_string(),
        family_id: "v1-lenbound".to_string(),
        verifier_id: "lean-runner-v01".to_string(),
        verifier_hash_version: "v0".to_string(),
        artifact_hash: ARTIFACT_HASH.to_string(),
        request_hash: REQUEST_HASH.to_string(),
        result: "accepted".to_string(),
        fee_charged: "1".to_string(),
        reward_recipient: REWARD_RECIPIENT.to_string(),
    }
}

fn fixture() -> ReceiptCommitment {
    ReceiptCommitment::new(fixture_input()).expect("valid receipt commitment")
}

#[test]
fn receipt_commitment_id_is_deterministic_from_canonical_fields() {
    let a = fixture();
    let b = ReceiptCommitment::new(fixture_input()).expect("same fields valid");

    assert_eq!(a.receipt_id, b.receipt_id);
    assert_eq!(a.compute_id(), b.compute_id());
    assert_eq!(a.receipt_id, a.compute_id());
}

#[test]
fn receipt_commitment_rejects_raw_human_answer_field() {
    let mut value = serde_json::to_value(fixture()).expect("serialize commitment");
    value["humanAnswer"] = json!("raw proof or model answer must stay off-chain");

    let err = serde_json::from_value::<ReceiptCommitment>(value)
        .expect_err("raw humanAnswer field must be rejected");
    assert!(
        err.to_string().contains("unknown field") || err.to_string().contains("humanAnswer"),
        "unexpected error: {err}"
    );
}

#[test]
fn receipt_commitment_id_changes_when_verifier_hash_version_changes() {
    let mut a = fixture();
    a.verifier_hash_version = "v0".to_string();
    a.receipt_id = a.compute_id();

    let mut b = a.clone();
    b.verifier_hash_version = "v1".to_string();
    b.receipt_id = b.compute_id();

    assert_ne!(a.receipt_id, b.receipt_id);
    assert_ne!(a.compute_id(), b.compute_id());
}

#[test]
fn receipt_commitment_validates_hex_commitment_fields() {
    let mut input = fixture_input();
    input.agent_pk = "not-hex".to_string();
    let err = ReceiptCommitment::new(input).expect_err("agent pk must be hex32");

    assert!(
        err.to_string().contains("agentPk") && err.to_string().contains("hex32"),
        "unexpected error: {err}"
    );
}
