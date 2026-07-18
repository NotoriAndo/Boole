//! SC.1 (ADR-0015 (b)/(b-1)) — reward ownership binding.
//!
//! Replay must reject a block that routes rewards to destinations the
//! winning share's submitter did not authorize. The authorization is the
//! submitter-signed `boole.signer.work.v2` envelope carried in
//! `SelectedShareEvidence.signed_work`; the named invariant (ADR-0015
//! (b-1)) is asserted here as ONE property:
//!
//!   evidence.pk == envelope signer == workPayload.pk == selectedSharePks[idx]
//!   committed selectedShareRewardPks[idx] == signed rewardRecipient
//!   proposerRewardPk == the signed rewardRecipient of the proposer's own share
//!   block.proposerPk == the replay-derived qualifying winner pk
//!
//! Deliberately NOT required: reward_pk == pk (cold-wallet routing stays).
//!
//! Scope (SC.1-a): the invariant is enforced whenever `signed_work` is
//! PRESENT; evidence without an authorization stays accepted on this
//! slice (the named-network requirement flip is SC.1-d).

use boole_core::{
    block_hash, canonical_payload_hash_hex, replay_blocks, share_hash, Hex32, PersistedBlock,
    SelectedShareEvidence, ShareWorkAuthorization, SigningKeyV2,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const SHARE_N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHARE_J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NONCE_S: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
/// Cold-wallet reward destination — deliberately NOT the submitter pk.
const RECIPIENT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const OTHER_PK: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn signer() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("sc1-reward-authorization")
}

fn other_signer() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("sc1-reward-authorization-other")
}

fn valid_pofp_v2_package_hex() -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"POFP");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x11; 32]);
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x22; 32]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    hex::encode(bytes)
}

/// The exact `/submit` body shape the work envelope's `workPayload` carries.
fn work_body(pk: &str, proof_package_hex: &str) -> Value {
    json!({
        "bytes": proof_package_hex,
        "c": PREV_C,
        "j": SHARE_J,
        "n": SHARE_N,
        "nonceS": NONCE_S,
        "pk": pk,
    })
}

/// A `boole.signer.work.v2` envelope signed by `key`, authorizing
/// `reward_recipient`, over exactly `body`.
fn authorization_for(
    key: &SigningKeyV2,
    body: &Value,
    reward_recipient: &str,
) -> ShareWorkAuthorization {
    let payload = json!({
        "schema": "boole.signer.work.v2",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": canonical_payload_hash_hex(body),
        "nonce": "sc1-reward-authorization-nonce",
        "rewardRecipient": reward_recipient,
        "workPayload": body,
    });
    let envelope = key.sign(&payload).expect("dev key signs the work payload");
    ShareWorkAuthorization {
        schema: envelope.schema.to_string(),
        payload: envelope.payload,
        pk: envelope.pk,
        signature: envelope.signature,
        network_id: envelope.network_id,
    }
}

/// One-share block whose evidence carries `signed_work` and whose reward
/// routing (share reward pk + proposer reward pk) matches the signed
/// recipient. `proposer_pk` is the winning (only) share's pk.
fn authorized_block(signed_work: Option<ShareWorkAuthorization>) -> PersistedBlock {
    let pk = signer().pk_hex();
    let proof_package = valid_pofp_v2_package_hex();
    let package_bytes = hex::decode(&proof_package).expect("valid proof package hex");
    let canon_hash = hex::encode(Sha256::digest(&package_bytes));
    let share_hash = share_hash(
        &Hex32::from_hex(PREV_C).expect("prev c hex32"),
        &Hex32::from_hex(&pk).expect("pk hex32"),
        &Hex32::from_hex(SHARE_N).expect("n hex32"),
        &Hex32::from_hex(SHARE_J).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();
    let mut block = PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: pk.clone(),
        selected_share_hashes: vec![share_hash],
        selected_share_pks: vec![pk.clone()],
        selected_share_reward_pks: vec![RECIPIENT.to_string()],
        proposer_reward_pk: RECIPIENT.to_string(),
        selected_share_evidence: vec![SelectedShareEvidence {
            pk,
            n: SHARE_N.to_string(),
            j: SHARE_J.to_string(),
            c: PREV_C.to_string(),
            canon_hash,
            proof_package,
            seed_hex: String::new(),
            signed_work,
        }],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        t_share: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: "1".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

fn default_authorization() -> ShareWorkAuthorization {
    let key = signer();
    let body = work_body(&key.pk_hex(), &valid_pofp_v2_package_hex());
    authorization_for(&key, &body, RECIPIENT)
}

fn assert_replay_rejects(block: PersistedBlock, expected_fragment: &str) {
    let err = replay_blocks(&[block]).expect_err("replay must reject the unauthorized block");
    assert!(
        err.to_string().contains(expected_fragment),
        "expected error containing {expected_fragment:?}, got: {err}"
    );
}

/// ADR-0015 (b-1) — the identity invariant as one named property: the
/// baseline (all equalities hold, reward routed to a cold wallet) replays;
/// breaking ANY single equality rejects the block.
#[test]
fn reward_authorization_identity_chain_is_exact() {
    // Baseline: authorized block with reward_pk != pk (cold wallet) replays.
    let replay = replay_blocks(&[authorized_block(Some(default_authorization()))])
        .expect("winner-authorized cold-wallet routing must replay");
    assert_eq!(replay.height, 1);
    assert_eq!(replay.balances.get(RECIPIENT).copied(), Some(2));

    // Envelope signed by a DIFFERENT key: envelope signer != evidence.pk.
    let key = signer();
    let body = work_body(&key.pk_hex(), &valid_pofp_v2_package_hex());
    let foreign_signer_auth = authorization_for(&other_signer(), &body, RECIPIENT);
    let mut block = authorized_block(Some(foreign_signer_auth));
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "signedWork");

    // workPayload.pk != evidence.pk: envelope is internally consistent
    // (signed, requestHash matches) but authorizes a different mining pk.
    let foreign_work_body = work_body(OTHER_PK, &valid_pofp_v2_package_hex());
    let foreign_work_auth = authorization_for(&key, &foreign_work_body, RECIPIENT);
    let mut block = authorized_block(Some(foreign_work_auth));
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "signedWork");

    // Tampered payload after signing: signature no longer verifies.
    let mut tampered = default_authorization();
    tampered.payload["rewardRecipient"] = json!(OTHER_PK);
    let mut block = authorized_block(Some(tampered));
    block.selected_share_reward_pks = vec![OTHER_PK.to_string()];
    block.proposer_reward_pk = OTHER_PK.to_string();
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "signedWork");
}

/// GAP-05 core: a block committing a share reward destination the winner
/// never signed must be excluded, even though the commitment itself is
/// well-formed hex32 and the hash re-verifies.
#[test]
fn replay_rejects_share_reward_pk_not_authorized_by_winning_share() {
    // Committed share reward pk differs from the signed recipient.
    let mut block = authorized_block(Some(default_authorization()));
    block.selected_share_reward_pks = vec![OTHER_PK.to_string()];
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "not authorized");

    // Empty selectedShareRewardPks falls back to the mining identity in
    // credit computation — with an authorization present that fallback
    // must STILL equal the signed recipient (here it does not: the
    // envelope routes to the cold wallet, the fallback pays the pk).
    let mut block = authorized_block(Some(default_authorization()));
    block.selected_share_reward_pks = vec![];
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "not authorized");
}

/// Proposer routing half of GAP-05: proposerRewardPk must equal the signed
/// rewardRecipient of the proposer's own (winning) share evidence.
#[test]
fn replay_rejects_proposer_reward_pk_not_authorized_by_winning_share() {
    let mut block = authorized_block(Some(default_authorization()));
    block.proposer_reward_pk = OTHER_PK.to_string();
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "proposer reward");
}

/// C-05 (ADR-0015 (b-1) extension) — the antecedent equality: the block's
/// declared proposerPk must BE the qualifying winner replay re-derives.
/// Without this, "proposer routing is authorized by the proposer's own
/// share" is vacuous — any pk could claim proposerhood.
#[test]
fn replay_rejects_proposer_pk_not_equal_to_qualifying_winner() {
    let mut block = authorized_block(Some(default_authorization()));
    block.proposer_pk = OTHER_PK.to_string();
    // Route rewards to the impostor identity fallback so only the winner
    // equality can reject (proposer_reward_pk stays the signed recipient).
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "qualifying winner");

    // The check is selection-shape level: it applies with NO signed_work
    // present as well (an evidence-bearing block cannot declare a foreign
    // proposer even before the authorization-required flip lands).
    let mut block = authorized_block(None);
    block.proposer_pk = OTHER_PK.to_string();
    block.c = block_hash(&block).to_hex();
    assert_replay_rejects(block, "qualifying winner");
}
