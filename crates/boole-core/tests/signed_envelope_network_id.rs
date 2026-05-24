//! P2.10 — network-id binding on `boole.signed.v1` envelopes.
//!
//! Goal: a signature produced for one network (e.g. `boole-testnet`)
//! must NOT verify when transplanted onto another network's wire shape
//! (e.g. `boole-mainnet`). This blocks cross-network replay where an
//! attacker captures a high-value signed message on a low-stakes
//! testnet and re-broadcasts the byte-identical signature on mainnet.
//!
//! Design:
//!   * `SignedEnvelope` grows `network_id: Option<String>`.
//!   * `SigningKeyV2::sign_for_network(payload, Some("..."))` folds
//!     `network_id` into the signing digest under a domain-separated
//!     tag (`boole.signed.v1.network::<network_id>\n` || canonical).
//!   * Legacy callers using `sign(payload)` continue to produce
//!     `network_id: None` envelopes — the digest stays
//!     `SHA-256(canonical(payload))`, so every pre-P2.10 signature
//!     keeps verifying with no fixture rewrites.
//!   * Tampering with the wire `network_id` flips the digest the
//!     verifier re-computes, so `verify()` returns `Ok(false)`.
//!     This is the property that defeats replay.
//!
//! Why a top-level field rather than burying network_id inside the
//! caller's payload: every signed route (work submit, bounty proof,
//! receipt commitment, session-key issue, …) would otherwise have to
//! independently remember to embed `network_id` in its own schema.
//! A single envelope-level field, bound into the digest, is the only
//! place a verifier can check uniformly without per-schema knowledge.

use boole_core::{SignedEnvelope, SigningKeyV2};
use serde_json::json;

#[test]
fn sign_for_network_round_trips_with_explicit_network_id() {
    let key = SigningKeyV2::from_dev_id("p2.10-network-roundtrip");
    let payload = json!({"msg": "hello testnet"});
    let envelope = key
        .sign_for_network(&payload, Some("boole-testnet"))
        .expect("sign with network_id");

    assert_eq!(
        envelope.network_id.as_deref(),
        Some("boole-testnet"),
        "envelope must carry the network_id the signer pinned",
    );
    assert!(
        envelope
            .verify()
            .expect("well-formed envelope must not surface Err"),
        "an envelope verified with the same network_id used to sign \
         must round-trip true",
    );
}

#[test]
fn legacy_sign_produces_envelope_without_network_id() {
    // Pre-P2.10 call sites continue to use `sign()`. They must keep
    // producing `network_id: None` envelopes with the legacy digest
    // (no domain-separation tag) so existing signatures and on-disk
    // ledger entries keep verifying.
    let key = SigningKeyV2::from_dev_id("p2.10-legacy-sign");
    let payload = json!({"msg": "legacy"});
    let envelope = key.sign(&payload).expect("legacy sign path");

    assert!(
        envelope.network_id.is_none(),
        "legacy sign() must not silently attach a network_id",
    );
    assert!(
        envelope.verify().expect("legacy verify path"),
        "legacy envelopes must keep verifying after P2.10",
    );
}

#[test]
fn tampering_with_network_id_rejects_signature() {
    // Capture a testnet-signed envelope and rewrite its `network_id`
    // to look like a mainnet message. Because the network_id is
    // bound into the digest, the verifier recomputes a different
    // hash than the signer hashed, so the ed25519 check fails. This
    // is the property that prevents cross-network replay.
    let key = SigningKeyV2::from_dev_id("p2.10-cross-network-replay");
    let payload = json!({"to": "alice", "amount": "1.0"});
    let testnet_envelope = key
        .sign_for_network(&payload, Some("boole-testnet"))
        .expect("sign for testnet");
    assert!(
        testnet_envelope.verify().expect("baseline verify"),
        "baseline testnet envelope must verify before tampering",
    );

    let replayed = SignedEnvelope {
        schema: testnet_envelope.schema,
        payload: testnet_envelope.payload.clone(),
        pk: testnet_envelope.pk.clone(),
        signature: testnet_envelope.signature.clone(),
        network_id: Some("boole-mainnet".to_string()),
    };
    let outcome = replayed
        .verify()
        .expect("hex shapes still well-formed after tampering");
    assert!(
        !outcome,
        "rewriting envelope network_id from testnet to mainnet must \
         break verification — otherwise an attacker can replay a \
         testnet signature against mainnet",
    );
}

#[test]
fn stripping_network_id_to_none_rejects_signature() {
    // Symmetric to the cross-network attack: an attacker can't just
    // delete the `network_id` field to make the envelope look like a
    // legacy one. The digest domain-separation tag is presence-bound,
    // so signing with Some(...) and verifying with None still fails.
    let key = SigningKeyV2::from_dev_id("p2.10-strip-network-id");
    let payload = json!({"msg": "scoped to network"});
    let with_network = key
        .sign_for_network(&payload, Some("boole-testnet"))
        .expect("sign with network");

    let stripped = SignedEnvelope {
        schema: with_network.schema,
        payload: with_network.payload.clone(),
        pk: with_network.pk.clone(),
        signature: with_network.signature.clone(),
        network_id: None,
    };
    assert!(
        !stripped
            .verify()
            .expect("hex still well-formed after stripping network_id"),
        "stripping network_id from a network-bound signature must reject",
    );
}
