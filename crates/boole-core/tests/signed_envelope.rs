//! S13a — ed25519 sign/verify primitives wrapped in `boole.signed.v1`.
//!
//! Sign/verify operates on SHA-256(canonicalize(payload)). The wrapper
//! envelope itself is NOT canonicalized — it's a transport shell.

use boole_core::{verify_signature, SignedEnvelope, SigningKeyV2, SIGNED_ENVELOPE_SCHEMA};
use serde_json::json;

#[test]
fn sign_verify_round_trip_returns_true_for_random_key() {
    let key = SigningKeyV2::from_random().expect("generate ed25519 key");
    let payload = json!({"msg": "hello", "n": 42});
    let envelope = key.sign(&payload).expect("sign payload");

    assert_eq!(envelope.schema, SIGNED_ENVELOPE_SCHEMA);
    assert_eq!(envelope.pk, key.pk_hex());
    assert_eq!(envelope.signature.len(), 128, "ed25519 sig is 64 bytes hex");

    let valid = envelope
        .verify()
        .expect("verify must not error on well-formed envelope");
    assert!(valid, "round-trip sign+verify must accept");
}

#[test]
fn verify_with_wrong_pk_returns_ok_false_not_err() {
    let key_a = SigningKeyV2::from_random().expect("key A");
    let key_b = SigningKeyV2::from_random().expect("key B");
    let payload = json!({"msg": "swap"});
    let envelope_a = key_a.sign(&payload).expect("sign");

    // Forge an envelope claiming key B signed it but with key A's signature.
    let forged = SignedEnvelope {
        schema: SIGNED_ENVELOPE_SCHEMA,
        payload: payload.clone(),
        pk: key_b.pk_hex(),
        signature: envelope_a.signature.clone(),
        network_id: None,
    };
    let result = forged
        .verify()
        .expect("well-formed hex shapes must not error");
    assert!(!result, "wrong-pk forge must reject (Ok(false))");
}

#[test]
fn verify_with_tampered_payload_returns_ok_false() {
    let key = SigningKeyV2::from_random().expect("key");
    let original = json!({"msg": "original", "n": 1});
    let envelope = key.sign(&original).expect("sign");

    let tampered = SignedEnvelope {
        schema: envelope.schema,
        payload: json!({"msg": "tampered", "n": 1}),
        pk: envelope.pk.clone(),
        signature: envelope.signature.clone(),
        network_id: None,
    };
    let result = tampered.verify().expect("hex shapes well-formed");
    assert!(!result, "tampered payload must reject (Ok(false))");
}

#[test]
fn verify_signature_with_malformed_pk_returns_err() {
    let payload = json!({"msg": "x"});
    for pk in ["not-hex-at-all".to_string(), "A".repeat(64)] {
        let result = verify_signature(
            &pk,
            // 64 hex chars = 32 bytes; needs 128 chars for an ed25519 signature.
            &"00".repeat(64),
            &payload,
        );
        assert!(
            result.is_err(),
            "malformed/noncanonical pk hex must surface as Err, not Ok(false): {result:?}",
        );
    }
}

#[test]
fn signing_key_seed_rejects_uppercase_noncanonical_hex32() {
    let result = SigningKeyV2::from_seed_hex(&"A".repeat(64));
    assert!(
        result.is_err(),
        "stored sk seed hex must use canonical lowercase Hex32"
    );
}

#[test]
fn verify_signature_rejects_uppercase_noncanonical_signature_hex64() {
    let payload = json!({"msg": "x"});
    let key = SigningKeyV2::from_dev_id("uppercase-sig-policy");
    let envelope = key.sign(&payload).expect("sign");
    let uppercase_sig = envelope.signature.to_uppercase();

    let result = verify_signature(&envelope.pk, &uppercase_sig, &payload);

    assert!(
        result.is_err(),
        "uppercase signature hex must be wire-malformed, not Ok(false): {result:?}"
    );
}
