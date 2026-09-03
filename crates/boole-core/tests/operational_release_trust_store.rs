//! Durable operational release trust-policy chain.
//!
//! All keys are deterministic, visibly non-production KAT material. The
//! tests exercise persisted public metadata only; no private key is written.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    adopt_operational_release_trust_policy, canonicalize,
    open_installed_operational_release_trust_policy,
    prepare_operational_release_trust_policy_update, SigningKeyV2,
    OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const RECOVERY_A: &str = "store-recovery-kat-a";
const RECOVERY_B: &str = "store-recovery-kat-b";
const RECOVERY_C: &str = "store-recovery-kat-c";
const RECOVERY_D: &str = "store-recovery-kat-d";
const PRODUCT: &str = "store-product-kat-a";
const GUEST: &str = "store-guest-kat-a";

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-operational-trust-store-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn key(id: &str) -> SigningKeyV2 {
    SigningKeyV2::from_dev_id(id)
}

fn recovery_keys(ids: &[&str]) -> Vec<Value> {
    ids.iter()
        .map(|id| json!({"keyId": id, "publicKey": key(id).pk_hex()}))
        .collect()
}

fn recovery_root() -> Vec<u8> {
    canonicalize(&json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 2,
        "keys": recovery_keys(&[RECOVERY_A, RECOVERY_B, RECOVERY_C])
    }))
}

fn initial_policy() -> Vec<u8> {
    canonicalize(&json!({
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
            "keys": recovery_keys(&[RECOVERY_A, RECOVERY_B, RECOVERY_C])
        },
        "retiredKeys": []
    }))
}

fn successor_policy(previous_sha256: &str) -> Vec<u8> {
    canonicalize(&json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 2,
        "previousPolicySha256": previous_sha256,
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
            "keys": recovery_keys(&[RECOVERY_B, RECOVERY_C, RECOVERY_D])
        },
        "retiredKeys": [{
            "role": "recovery",
            "keyId": RECOVERY_A,
            "publicKey": key(RECOVERY_A).pk_hex(),
            "retiredAtGeneration": 2
        }]
    }))
}

fn signatures(policy: &[u8], signers: &[&str]) -> Vec<u8> {
    let policy_sha256 = hex::encode(Sha256::digest(policy));
    let payload = json!({
        "context": OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        "policySha256": policy_sha256
    });
    let values: Vec<Value> = signers
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
    canonicalize(&json!({
        "schema": "boole.operational-release-trust-policy-signatures.v1",
        "policySha256": policy_sha256,
        "signatures": values
    }))
}

fn install_initial(root: &Path) {
    let recovery = recovery_root();
    let policy = initial_policy();
    let prepared = prepare_operational_release_trust_policy_update(
        root,
        Some(&recovery),
        Some(&policy),
        Some(&signatures(&policy, &[RECOVERY_A, RECOVERY_B])),
    )
    .expect("prepare initial policy");
    assert!(prepared.needs_adoption());
    adopt_operational_release_trust_policy(root, prepared).expect("adopt initial policy");
}

#[test]
fn initial_policy_is_durable_and_reopens_without_external_release_keys() {
    let fixture = FixtureDir::new("initial");
    install_initial(fixture.path());

    let installed = open_installed_operational_release_trust_policy(fixture.path())
        .expect("reopen installed policy");
    assert_eq!(installed.verified_policy().generation(), 1);
    assert_eq!(installed.state().generation(), 1);
    assert_eq!(installed.state().previous_policy_sha256(), None);
    assert_eq!(installed.policy_directory_count(), 1);
    assert_eq!(installed.unreferenced_policy_directory_count(), 0);
    assert_eq!(
        installed
            .verified_policy()
            .product_release_trust_root()
            .expect("product role")
            .key_id(),
        PRODUCT
    );
}

#[test]
fn successor_extends_the_persisted_chain_and_survives_restart() {
    let fixture = FixtureDir::new("successor");
    install_initial(fixture.path());
    let first = open_installed_operational_release_trust_policy(fixture.path())
        .expect("first installed policy");
    let second_raw = successor_policy(first.verified_policy().policy_sha256());
    let prepared = prepare_operational_release_trust_policy_update(
        fixture.path(),
        None,
        Some(&second_raw),
        Some(&signatures(
            &second_raw,
            &[RECOVERY_A, RECOVERY_B, RECOVERY_C],
        )),
    )
    .expect("prepare successor");
    assert!(prepared.needs_adoption());
    adopt_operational_release_trust_policy(fixture.path(), prepared).expect("adopt successor");

    let reopened = open_installed_operational_release_trust_policy(fixture.path())
        .expect("reopen successor chain");
    assert_eq!(reopened.state().generation(), 2);
    assert_eq!(
        reopened.state().previous_policy_sha256(),
        Some(first.verified_policy().policy_sha256())
    );
    assert_eq!(reopened.policy_directory_count(), 2);
    assert_eq!(reopened.unreferenced_policy_directory_count(), 0);
}

#[test]
fn tampered_persisted_policy_fails_closed_instead_of_falling_back() {
    let fixture = FixtureDir::new("tamper");
    install_initial(fixture.path());
    let installed =
        open_installed_operational_release_trust_policy(fixture.path()).expect("installed policy");
    fs::set_permissions(
        installed.policy_path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o644),
    )
    .expect("make policy writable for attacker simulation");
    fs::write(installed.policy_path(), b"{}\n").expect("tamper policy");

    let error = open_installed_operational_release_trust_policy(fixture.path())
        .expect_err("tampered policy cannot reopen");
    assert!(error.to_string().contains("policy"));
}

#[test]
fn reopening_current_policy_is_read_only_and_requires_no_candidate_files() {
    let fixture = FixtureDir::new("current");
    install_initial(fixture.path());
    let state_path = fixture.path().join("installed-trust-policy.json");
    let before = fs::read(&state_path).expect("state before reopen");

    let prepared =
        prepare_operational_release_trust_policy_update(fixture.path(), None, None, None)
            .expect("reuse current installed policy");
    assert!(!prepared.needs_adoption());
    assert_eq!(prepared.verified_policy().generation(), 1);
    assert_eq!(fs::read(state_path).expect("state after reopen"), before);
}
