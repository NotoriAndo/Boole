//! P1.10 — `signing_digest_hex` is the exact 32-byte digest that
//! `sign_for_network` raw-ed25519-signs.
//!
//! This is the contract that lets `boole-wallet-agent sign --message <hex>`
//! (which raw-ed25519-signs the decoded `--message` bytes) substitute for the
//! in-process `SigningKeyV2::sign_for_network` WITHOUT changing the wire
//! signature: pass `hex(signing_digest_hex(payload, network_id))` as the
//! agent's `--message`, pair the returned signature with the vault pubkey, and
//! the assembled `boole.signed.v1` envelope verifies unchanged.

use boole_core::{signing_digest_hex, verify_signature_with_network, SigningKeyV2};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

fn assert_external_raw_sign_matches(network_id: Option<&str>) {
    let key = SigningKeyV2::from_dev_id("p1-10-signing-digest");
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": "11".repeat(32),
        "prover": key.pk_hex(),
        "validBefore": 1_900_000_000_u64,
        "nonce": "cafecafecafecafecafecafecafecafe",
    });

    // In-process signature (the path the wallet-agent must reproduce).
    let envelope = key.sign_for_network(&payload, network_id).expect("sign");

    // Reconstruct the raw ed25519 key from the exposed seed and sign the digest
    // EXACTLY as `boole-wallet-agent sign --message <hex(digest)>` would.
    let seed = hex::decode(key.sk_seed_hex()).expect("seed hex");
    let seed_arr: [u8; 32] = seed.as_slice().try_into().expect("32-byte seed");
    let raw_key = SigningKey::from_bytes(&seed_arr);
    let digest_bytes = hex::decode(signing_digest_hex(&payload, network_id)).expect("digest hex");
    let raw_sig = raw_key.sign(&digest_bytes);

    assert_eq!(
        hex::encode(raw_sig.to_bytes()),
        envelope.signature,
        "raw-signing signing_digest_hex must reproduce sign_for_network's signature \
         (network_id={network_id:?})"
    );
    assert!(
        verify_signature_with_network(&envelope.pk, &envelope.signature, &payload, network_id)
            .expect("verify ran"),
        "the externally-reproduced signature must verify as the envelope"
    );
}

#[test]
fn external_raw_sign_of_digest_matches_legacy_unscoped() {
    assert_external_raw_sign_matches(None);
}

#[test]
fn external_raw_sign_of_digest_matches_network_scoped() {
    assert_external_raw_sign_matches(Some("boole-testnet"));
}

#[test]
fn network_scoped_digest_differs_from_unscoped() {
    // Domain separation: the network-bound digest must not equal the legacy one,
    // else a cross-network replay would be possible.
    let payload = json!({"schema": "boole.bounty.proof.v1", "bountyId": "x"});
    assert_ne!(
        signing_digest_hex(&payload, None),
        signing_digest_hex(&payload, Some("boole-testnet")),
        "network scoping must change the signing digest"
    );
}
