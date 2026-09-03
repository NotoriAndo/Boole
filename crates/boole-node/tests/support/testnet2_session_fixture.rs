use boole_core::{canonical_payload_hash_hex, SigningKeyV2};
use serde_json::{json, Value};

pub const NETWORK_ID: &str = "boole-testnet-2";
pub const OWNER_DEV_ID: &str = "testnet2-smoke-owner-v1";
const SESSION_DEV_ID: &str = "testnet2-smoke-session-v1";
const AGENT_DEV_ID: &str = "testnet2-smoke-agent-v1";
const REWARD_DEV_ID: &str = "testnet2-smoke-reward-v1";
const POLICY_ROOT: &str = "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1";

pub fn session_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id(SESSION_DEV_ID)
}

pub fn session_state() -> Value {
    let owner = SigningKeyV2::from_dev_id(OWNER_DEV_ID);
    let session = session_key();
    let agent = SigningKeyV2::from_dev_id(AGENT_DEV_ID);
    let reward = SigningKeyV2::from_dev_id(REWARD_DEV_ID);
    json!({
        "sessionPk": session.pk_hex(),
        "ownerPk": owner.pk_hex(),
        "agentPk": agent.pk_hex(),
        "fixedRewardRecipient": reward.pk_hex(),
        "allowedFamilyRoot": POLICY_ROOT,
        "maxFeePerRequest": "0",
        "activationHeight": 0,
        "expiryHeight": 17_280,
        "revoked": false,
        "policyHash": POLICY_ROOT,
    })
}

pub fn submission_session(body: &Value, nonce: &str) -> Value {
    let key = session_key();
    let reward_recipient = session_state()["fixedRewardRecipient"]
        .as_str()
        .expect("reward recipient")
        .to_string();
    let payload = json!({
        "schema": "boole.signer.work.v2",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": canonical_payload_hash_hex(body),
        "nonce": nonce,
        "rewardRecipient": reward_recipient,
        "workPayload": body,
    });
    let signed = key
        .sign_for_network(&payload, Some(NETWORK_ID))
        .expect("network-scoped work signature");
    json!({
        "submittedBy": key.pk_hex(),
        "rewardRecipient": reward_recipient,
        "nonce": nonce,
        "signedWork": {
            "schema": signed.schema,
            "payload": signed.payload,
            "pk": signed.pk,
            "signature": signed.signature,
            "network_id": signed.network_id,
        },
    })
}
