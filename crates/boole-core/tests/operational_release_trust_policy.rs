//! Recovery-authorized product and guest release trust policy.
//!
//! Every key in this test is a deterministic, visibly non-production KAT key.

use boole_core::{
    canonicalize, verify_initial_operational_release_trust_policy,
    verify_operational_release_trust_policy_successor, OperationalReleaseRecoveryRoot,
    SigningKeyV2, OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const RECOVERY_A: &str = "recovery-kat-a";
const RECOVERY_B: &str = "recovery-kat-b";
const RECOVERY_C: &str = "recovery-kat-c";
const RECOVERY_D: &str = "recovery-kat-d";
const PRODUCT: &str = "product-release-kat-a";
const PRODUCT_B: &str = "product-release-kat-b";
const GUEST: &str = "guest-release-kat-a";

fn key(id: &str) -> SigningKeyV2 {
    SigningKeyV2::from_dev_id(id)
}

fn canonical(value: Value) -> Vec<u8> {
    canonicalize(&value)
}

fn recovery_root() -> OperationalReleaseRecoveryRoot {
    let raw = canonical(json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 2,
        "keys": [
            {"keyId": RECOVERY_A, "publicKey": key(RECOVERY_A).pk_hex()},
            {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
            {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()}
        ]
    }));
    OperationalReleaseRecoveryRoot::from_canonical_json(&raw).expect("KAT recovery root")
}

fn initial_policy() -> Vec<u8> {
    canonical(json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 1,
        "previousPolicySha256": null,
        "productRelease": {
            "status": "active",
            "keyId": PRODUCT,
            "publicKey": key(PRODUCT).pk_hex()
        },
        "guestRelease": {
            "status": "active",
            "keyId": GUEST,
            "publicKey": key(GUEST).pk_hex()
        },
        "recovery": {
            "threshold": 2,
            "keys": [
                {"keyId": RECOVERY_A, "publicKey": key(RECOVERY_A).pk_hex()},
                {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
                {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()}
            ]
        },
        "retiredKeys": []
    }))
}

fn signatures(policy: &[u8], signers: &[&str]) -> Vec<u8> {
    let policy_sha256 = hex::encode(Sha256::digest(policy));
    let payload = json!({
        "context": OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        "policySha256": policy_sha256
    });
    let signatures: Vec<Value> = signers
        .iter()
        .map(|id| {
            let signing_key = key(id);
            let envelope = signing_key.sign(&payload).expect("KAT signature");
            json!({
                "keyId": id,
                "publicKey": envelope.pk,
                "signature": envelope.signature
            })
        })
        .collect();
    canonical(json!({
        "schema": "boole.operational-release-trust-policy-signatures.v1",
        "policySha256": policy_sha256,
        "signatures": signatures
    }))
}

fn rotated_policy(first_policy_sha256: &str) -> Vec<u8> {
    canonical(json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 2,
        "previousPolicySha256": first_policy_sha256,
        "productRelease": {
            "status": "active",
            "keyId": PRODUCT_B,
            "publicKey": key(PRODUCT_B).pk_hex()
        },
        "guestRelease": {
            "status": "active",
            "keyId": GUEST,
            "publicKey": key(GUEST).pk_hex()
        },
        "recovery": {
            "threshold": 2,
            "keys": [
                {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
                {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()},
                {"keyId": RECOVERY_D, "publicKey": key(RECOVERY_D).pk_hex()}
            ]
        },
        "retiredKeys": [
            {
                "role": "product-release",
                "keyId": PRODUCT,
                "publicKey": key(PRODUCT).pk_hex(),
                "retiredAtGeneration": 2
            },
            {
                "role": "recovery",
                "keyId": RECOVERY_A,
                "publicKey": key(RECOVERY_A).pk_hex(),
                "retiredAtGeneration": 2
            }
        ]
    }))
}

#[test]
fn recovery_threshold_authorizes_distinct_product_and_guest_roots() {
    let policy = initial_policy();
    let verified = verify_initial_operational_release_trust_policy(
        &policy,
        &signatures(&policy, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("two recovery signatures authorize the initial policy");

    assert_eq!(verified.generation(), 1);
    assert_eq!(
        verified.policy_sha256(),
        hex::encode(Sha256::digest(&policy))
    );
    assert_eq!(
        verified
            .product_release_trust_root()
            .expect("active product root")
            .key_id(),
        PRODUCT
    );
    assert_eq!(
        verified
            .guest_release_trust_root()
            .expect("active guest root")
            .key_id(),
        GUEST
    );
    assert_ne!(
        verified
            .product_release_trust_root()
            .expect("active product root")
            .public_key_hex(),
        verified
            .guest_release_trust_root()
            .expect("active guest root")
            .public_key_hex()
    );
}

#[test]
fn successor_needs_old_and_new_recovery_thresholds_and_retires_removed_keys() {
    let first_raw = initial_policy();
    let first = verify_initial_operational_release_trust_policy(
        &first_raw,
        &signatures(&first_raw, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("initial policy");
    let successor_raw = rotated_policy(first.policy_sha256());
    let successor = verify_operational_release_trust_policy_successor(
        &first,
        &successor_raw,
        &signatures(&successor_raw, &[RECOVERY_A, RECOVERY_B, RECOVERY_C]),
    )
    .expect("old A+B and new B+C thresholds authorize the transition");

    assert_eq!(successor.generation(), 2);
    assert_eq!(
        successor
            .product_release_trust_root()
            .expect("rotated product root")
            .key_id(),
        PRODUCT_B
    );
    assert_eq!(
        successor
            .guest_release_trust_root()
            .expect("unchanged guest root")
            .key_id(),
        GUEST
    );
    assert_eq!(successor.retired_key_count(), 2);
}

#[test]
fn one_of_one_recovery_configuration_is_rejected() {
    let root_raw = canonical(json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 1,
        "keys": [
            {"keyId": RECOVERY_A, "publicKey": key(RECOVERY_A).pk_hex()}
        ]
    }));
    let error = OperationalReleaseRecoveryRoot::from_canonical_json(&root_raw)
        .expect_err("one compromised recovery key must not control release authority");
    assert!(error.to_string().contains("exactly three"));
}

#[test]
fn fewer_than_two_recovery_signatures_cannot_authorize_a_policy() {
    let policy = initial_policy();
    let error = verify_initial_operational_release_trust_policy(
        &policy,
        &signatures(&policy, &[RECOVERY_A]),
        &recovery_root(),
    )
    .expect_err("one of three recovery signatures is below threshold");
    assert!(error
        .to_string()
        .contains("previous recovery threshold is not satisfied"));
}

#[test]
fn initial_policy_cannot_replace_a_recovery_key_without_retirement_history() {
    let mut policy: Value = serde_json::from_slice(&initial_policy()).expect("initial policy JSON");
    policy["recovery"]["keys"][2] = json!({
        "keyId": RECOVERY_D,
        "publicKey": key(RECOVERY_D).pk_hex()
    });
    let policy = canonical(policy);

    let error = verify_initial_operational_release_trust_policy(
        &policy,
        &signatures(&policy, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect_err("generation one cannot silently remove recovery key C");
    assert!(error
        .to_string()
        .contains("initial recovery role must exactly match"));
}

#[test]
fn recovery_rotation_needs_the_new_threshold_as_well_as_the_old_threshold() {
    let first_raw = initial_policy();
    let first = verify_initial_operational_release_trust_policy(
        &first_raw,
        &signatures(&first_raw, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("initial policy");
    let successor = rotated_policy(first.policy_sha256());

    let error = verify_operational_release_trust_policy_successor(
        &first,
        &successor,
        &signatures(&successor, &[RECOVERY_A, RECOVERY_B]),
    )
    .expect_err("old A+B has only one signature from the new B+C+D role");
    assert!(error
        .to_string()
        .contains("next recovery threshold is not satisfied"));
}

#[test]
fn a_removed_key_must_be_recorded_in_the_monotonic_retirement_history() {
    let first_raw = initial_policy();
    let first = verify_initial_operational_release_trust_policy(
        &first_raw,
        &signatures(&first_raw, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("initial policy");
    let mut successor: Value =
        serde_json::from_slice(&rotated_policy(first.policy_sha256())).expect("successor JSON");
    successor["retiredKeys"]
        .as_array_mut()
        .expect("retired keys")
        .pop();
    let successor = canonical(successor);

    let error = verify_operational_release_trust_policy_successor(
        &first,
        &successor,
        &signatures(&successor, &[RECOVERY_A, RECOVERY_B, RECOVERY_C]),
    )
    .expect_err("removed recovery key A must remain retired");
    assert!(error.to_string().contains("record every removed key"));
}

#[test]
fn a_retired_release_key_can_never_be_reactivated() {
    let first_raw = initial_policy();
    let first = verify_initial_operational_release_trust_policy(
        &first_raw,
        &signatures(&first_raw, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("initial policy");
    let second_raw = rotated_policy(first.policy_sha256());
    let second = verify_operational_release_trust_policy_successor(
        &first,
        &second_raw,
        &signatures(&second_raw, &[RECOVERY_A, RECOVERY_B, RECOVERY_C]),
    )
    .expect("rotated policy");
    let resurrected = canonical(json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 3,
        "previousPolicySha256": second.policy_sha256(),
        "productRelease": {
            "status": "active",
            "keyId": PRODUCT,
            "publicKey": key(PRODUCT).pk_hex()
        },
        "guestRelease": {
            "status": "active",
            "keyId": GUEST,
            "publicKey": key(GUEST).pk_hex()
        },
        "recovery": {
            "threshold": 2,
            "keys": [
                {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
                {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()},
                {"keyId": RECOVERY_D, "publicKey": key(RECOVERY_D).pk_hex()}
            ]
        },
        "retiredKeys": [
            {
                "role": "product-release",
                "keyId": PRODUCT,
                "publicKey": key(PRODUCT).pk_hex(),
                "retiredAtGeneration": 2
            },
            {
                "role": "product-release",
                "keyId": PRODUCT_B,
                "publicKey": key(PRODUCT_B).pk_hex(),
                "retiredAtGeneration": 3
            },
            {
                "role": "recovery",
                "keyId": RECOVERY_A,
                "publicKey": key(RECOVERY_A).pk_hex(),
                "retiredAtGeneration": 2
            }
        ]
    }));

    let error = verify_operational_release_trust_policy_successor(
        &second,
        &resurrected,
        &signatures(&resurrected, &[RECOVERY_B, RECOVERY_C]),
    )
    .expect_err("retired product key A must stay retired forever");
    assert!(error.to_string().contains("cannot become active again"));
}

#[test]
fn product_guest_and_recovery_roles_cannot_share_key_material() {
    let mut policy: Value = serde_json::from_slice(&initial_policy()).expect("initial policy JSON");
    policy["guestRelease"]["publicKey"] = policy["productRelease"]["publicKey"].clone();
    let policy = canonical(policy);

    let error = verify_initial_operational_release_trust_policy(
        &policy,
        &signatures(&policy, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect_err("product and guest roles must be independent");
    assert!(error.to_string().contains("distinct keys"));
}

#[test]
fn recovery_role_can_disable_a_compromised_domain_then_enable_only_a_fresh_key() {
    let first_raw = initial_policy();
    let first = verify_initial_operational_release_trust_policy(
        &first_raw,
        &signatures(&first_raw, &[RECOVERY_A, RECOVERY_B]),
        &recovery_root(),
    )
    .expect("initial policy");
    let disabled_raw = canonical(json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 2,
        "previousPolicySha256": first.policy_sha256(),
        "productRelease": {"status": "disabled"},
        "guestRelease": {
            "status": "active",
            "keyId": GUEST,
            "publicKey": key(GUEST).pk_hex()
        },
        "recovery": {
            "threshold": 2,
            "keys": [
                {"keyId": RECOVERY_A, "publicKey": key(RECOVERY_A).pk_hex()},
                {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
                {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()}
            ]
        },
        "retiredKeys": [{
            "role": "product-release",
            "keyId": PRODUCT,
            "publicKey": key(PRODUCT).pk_hex(),
            "retiredAtGeneration": 2
        }]
    }));
    let disabled = verify_operational_release_trust_policy_successor(
        &first,
        &disabled_raw,
        &signatures(&disabled_raw, &[RECOVERY_A, RECOVERY_B]),
    )
    .expect("recovery threshold disables compromised product role");
    assert!(disabled.product_release_trust_root().is_none());

    let recovered_raw = canonical(json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 3,
        "previousPolicySha256": disabled.policy_sha256(),
        "productRelease": {
            "status": "active",
            "keyId": PRODUCT_B,
            "publicKey": key(PRODUCT_B).pk_hex()
        },
        "guestRelease": {
            "status": "active",
            "keyId": GUEST,
            "publicKey": key(GUEST).pk_hex()
        },
        "recovery": {
            "threshold": 2,
            "keys": [
                {"keyId": RECOVERY_A, "publicKey": key(RECOVERY_A).pk_hex()},
                {"keyId": RECOVERY_B, "publicKey": key(RECOVERY_B).pk_hex()},
                {"keyId": RECOVERY_C, "publicKey": key(RECOVERY_C).pk_hex()}
            ]
        },
        "retiredKeys": [{
            "role": "product-release",
            "keyId": PRODUCT,
            "publicKey": key(PRODUCT).pk_hex(),
            "retiredAtGeneration": 2
        }]
    }));
    let recovered = verify_operational_release_trust_policy_successor(
        &disabled,
        &recovered_raw,
        &signatures(&recovered_raw, &[RECOVERY_A, RECOVERY_B]),
    )
    .expect("recovery threshold enables a fresh product key");
    assert_eq!(
        recovered
            .product_release_trust_root()
            .expect("recovered product role")
            .key_id(),
        PRODUCT_B
    );
}
