//! Public trust-bootstrap packaging and adoption through the real CLI.
//!
//! Every private key in this file is a deterministic non-production KAT key.
//! The production command receives public documents and detached signatures
//! only; it never creates or reads a private key.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, SigningKeyV2, CURL_PRODUCT_INSTALL_STATE_FILE,
    OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT, OPERATIONAL_RELEASE_TRUST_STATE_FILE,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const RECOVERY_IDS: [&str; 3] = [
    "non-production-ceremony-recovery-a",
    "non-production-ceremony-recovery-b",
    "non-production-ceremony-recovery-c",
];
const PRODUCT_ID: &str = "non-production-ceremony-product";
const GUEST_ID: &str = "non-production-ceremony-guest";
const CEREMONY_CONTEXT: &str = "boole-operational-release-key-ceremony-v1";
static FIXTURE_ATTEMPT: AtomicU64 = AtomicU64::new(0);

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-operational-trust-bootstrap-{}-{nonce}-{}",
            std::process::id(),
            FIXTURE_ATTEMPT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).expect("fixture root");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        fn unlock(path: &Path) {
            let Ok(metadata) = fs::symlink_metadata(path) else {
                return;
            };
            if metadata.is_dir() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        unlock(&entry.path());
                    }
                }
            } else {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
            }
        }
        unlock(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct CeremonyFiles {
    recovery_root: PathBuf,
    policy: PathBuf,
    policy_signatures: PathBuf,
    ceremony: PathBuf,
    ceremony_signatures: PathBuf,
    recovery_root_sha256: String,
    ceremony_sha256: String,
}

fn key(id: &str) -> SigningKeyV2 {
    SigningKeyV2::from_dev_id(id)
}

fn sha256(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

fn detached_signatures(
    schema: &str,
    digest_name: &str,
    digest: &str,
    context: &str,
    ids: &[&str],
) -> Vec<u8> {
    let payload = match digest_name {
        "policySha256" => json!({"context": context, "policySha256": digest}),
        "ceremonySha256" => json!({"context": context, "ceremonySha256": digest}),
        other => panic!("unsupported digest field: {other}"),
    };
    let mut signatures: Vec<Value> = ids
        .iter()
        .map(|id| {
            let signer = key(id);
            let envelope = signer.sign(&payload).expect("KAT signature");
            json!({
                "keyId": id,
                "publicKey": envelope.pk,
                "signature": envelope.signature,
            })
        })
        .collect();
    signatures.sort_by_key(|entry| entry["keyId"].as_str().expect("key id").to_string());
    let document = match digest_name {
        "policySha256" => json!({
            "schema": schema,
            "policySha256": digest,
            "signatures": signatures,
        }),
        "ceremonySha256" => json!({
            "schema": schema,
            "ceremonySha256": digest,
            "signatures": signatures,
        }),
        other => panic!("unsupported digest field: {other}"),
    };
    canonicalize(&document)
}

fn write_ceremony_files(fixture: &FixtureDir) -> CeremonyFiles {
    let recovery_root = canonicalize(&json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 2,
        "keys": RECOVERY_IDS.iter().map(|id| json!({
            "keyId": id,
            "publicKey": key(id).pk_hex(),
        })).collect::<Vec<_>>(),
    }));
    let recovery_root_sha256 = sha256(&recovery_root);
    let policy = canonicalize(&json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 1,
        "previousPolicySha256": null,
        "productRelease": {
            "status": "active",
            "keyId": PRODUCT_ID,
            "publicKey": key(PRODUCT_ID).pk_hex(),
        },
        "guestRelease": {
            "status": "active",
            "keyId": GUEST_ID,
            "publicKey": key(GUEST_ID).pk_hex(),
        },
        "recovery": {
            "threshold": 2,
            "keys": RECOVERY_IDS.iter().map(|id| json!({
                "keyId": id,
                "publicKey": key(id).pk_hex(),
            })).collect::<Vec<_>>(),
        },
        "retiredKeys": [],
    }));
    let policy_sha256 = sha256(&policy);
    let policy_signatures = detached_signatures(
        "boole.operational-release-trust-policy-signatures.v1",
        "policySha256",
        &policy_sha256,
        OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        &RECOVERY_IDS[..2],
    );

    let mut participants = vec![
        json!({
            "role": "product-release",
            "custodyClass": "online-signing",
            "keyId": PRODUCT_ID,
            "publicKey": key(PRODUCT_ID).pk_hex(),
        }),
        json!({
            "role": "guest-release",
            "custodyClass": "online-signing",
            "keyId": GUEST_ID,
            "publicKey": key(GUEST_ID).pk_hex(),
        }),
    ];
    participants.extend(RECOVERY_IDS.iter().map(|id| {
        json!({
            "role": "recovery",
            "custodyClass": "offline-recovery",
            "keyId": id,
            "publicKey": key(id).pk_hex(),
        })
    }));
    let ceremony = canonicalize(&json!({
        "schema": "boole.operational-release-key-ceremony.v1",
        "ceremonyId": "non-production-kat-ceremony-1",
        "environment": "non-production-kat",
        "recoveryRootSha256": recovery_root_sha256,
        "trustPolicySha256": policy_sha256,
        "participants": participants,
    }));
    let ceremony_sha256 = sha256(&ceremony);
    let mut all_ids = vec![PRODUCT_ID, GUEST_ID];
    all_ids.extend(RECOVERY_IDS);
    let ceremony_signatures = detached_signatures(
        "boole.operational-release-key-ceremony-signatures.v1",
        "ceremonySha256",
        &ceremony_sha256,
        CEREMONY_CONTEXT,
        &all_ids,
    );

    let recovery_root_path = fixture.join("recovery-root.json");
    let policy_path = fixture.join("trust-policy.json");
    let policy_signatures_path = fixture.join("trust-policy-signatures.json");
    let ceremony_path = fixture.join("key-ceremony.json");
    let ceremony_signatures_path = fixture.join("key-ceremony-signatures.json");
    fs::write(&recovery_root_path, recovery_root).expect("recovery root");
    fs::write(&policy_path, policy).expect("policy");
    fs::write(&policy_signatures_path, policy_signatures).expect("policy signatures");
    fs::write(&ceremony_path, ceremony).expect("ceremony");
    fs::write(&ceremony_signatures_path, ceremony_signatures).expect("ceremony signatures");
    CeremonyFiles {
        recovery_root: recovery_root_path,
        policy: policy_path,
        policy_signatures: policy_signatures_path,
        ceremony: ceremony_path,
        ceremony_signatures: ceremony_signatures_path,
        recovery_root_sha256,
        ceremony_sha256,
    }
}

fn package_command(files: &CeremonyFiles, output_root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "package-trust-bootstrap",
            "--recovery-root",
            files.recovery_root.to_str().expect("recovery root path"),
            "--trust-policy",
            files.policy.to_str().expect("policy path"),
            "--trust-policy-signatures",
            files
                .policy_signatures
                .to_str()
                .expect("policy signatures path"),
            "--key-ceremony",
            files.ceremony.to_str().expect("ceremony path"),
            "--key-ceremony-signatures",
            files
                .ceremony_signatures
                .to_str()
                .expect("ceremony signatures path"),
            "--output-root",
            output_root.to_str().expect("output path"),
        ])
        .output()
        .expect("run package-trust-bootstrap")
}

fn adopt_command(bootstrap_root: &Path, install_root: &Path, expected_sha256: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "adopt-trust-bootstrap",
            "--bootstrap-root",
            bootstrap_root.to_str().expect("bootstrap root path"),
            "--install-root",
            install_root.to_str().expect("install root path"),
            "--expected-recovery-root-sha256",
            expected_sha256,
        ])
        .output()
        .expect("run adopt-trust-bootstrap")
}

#[test]
fn real_cli_packages_only_public_proof_of_possession_material_atomically() {
    let fixture = FixtureDir::new();
    let files = write_ceremony_files(&fixture);
    let output_root = fixture.join("public-trust-bootstrap");

    let output = package_command(&files, &output_root);
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let result: Value = serde_json::from_slice(&output.stdout).expect("result JSON");
    assert_eq!(result["command"], "product.package-trust-bootstrap");
    assert_eq!(
        result["result"]["recoveryRootSha256"],
        files.recovery_root_sha256
    );
    assert_eq!(result["result"]["ceremonySha256"], files.ceremony_sha256);
    assert_eq!(result["result"]["signerCount"], 5);

    let mut names = fs::read_dir(&output_root)
        .expect("packaged directory")
        .map(|entry| {
            entry
                .expect("package entry")
                .file_name()
                .into_string()
                .expect("UTF-8 package name")
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            "key-ceremony-signatures.json",
            "key-ceremony.json",
            "recovery-root.json",
            "trust-policy-signatures.json",
            "trust-policy.json",
        ]
    );
    assert_eq!(
        fs::read(output_root.join("recovery-root.json")).expect("packaged recovery root"),
        fs::read(&files.recovery_root).expect("source recovery root")
    );
    assert_eq!(
        fs::metadata(&output_root)
            .expect("package metadata")
            .permissions()
            .mode()
            & 0o222,
        0,
        "published package must be read-only"
    );
}

#[test]
fn real_cli_requires_an_independently_supplied_root_pin_before_adoption() {
    let fixture = FixtureDir::new();
    let files = write_ceremony_files(&fixture);
    let bootstrap_root = fixture.join("published-bootstrap");
    let packaged = package_command(&files, &bootstrap_root);
    assert!(packaged.status.success(), "package public bootstrap");
    let install_root = fixture.join("installed-product");

    let wrong_pin = "00".repeat(32);
    assert_ne!(wrong_pin, files.recovery_root_sha256);
    let rejected = adopt_command(&bootstrap_root, &install_root, &wrong_pin);
    assert!(!rejected.status.success());
    let rejection: Value = serde_json::from_slice(&rejected.stderr).expect("typed rejection");
    assert_eq!(rejection["command"], "product.adopt-trust-bootstrap");
    assert_eq!(rejection["error"]["reason"], "trust-bootstrap-rejected");
    assert!(
        !install_root
            .join(OPERATIONAL_RELEASE_TRUST_STATE_FILE)
            .exists(),
        "a same-channel bundle cannot choose its own trust pin"
    );

    let adopted = adopt_command(&bootstrap_root, &install_root, &files.recovery_root_sha256);
    assert!(
        adopted.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&adopted.stdout),
        String::from_utf8_lossy(&adopted.stderr),
    );
    let result: Value = serde_json::from_slice(&adopted.stdout).expect("adoption result");
    assert_eq!(result["command"], "product.adopt-trust-bootstrap");
    assert_eq!(result["result"]["generation"], 1);
    assert_eq!(
        result["result"]["recoveryRootSha256"],
        files.recovery_root_sha256
    );
    assert_eq!(result["result"]["ceremonySha256"], files.ceremony_sha256);
    assert!(install_root
        .join(OPERATIONAL_RELEASE_TRUST_STATE_FILE)
        .is_file());
    assert!(
        !install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE).exists(),
        "trust adoption must not masquerade as a downloaded product release"
    );
    let reopened = boole_core::open_installed_operational_release_trust_policy(&install_root)
        .expect("reopen the durable trust-policy chain");
    assert_eq!(reopened.state().generation(), 1);
    assert_eq!(
        reopened.state().recovery_root_sha256(),
        files.recovery_root_sha256
    );
}

#[test]
fn real_cli_rejects_an_incomplete_key_ceremony_before_publishing_anything() {
    let fixture = FixtureDir::new();
    let files = write_ceremony_files(&fixture);
    let mut signatures: Value =
        serde_json::from_slice(&fs::read(&files.ceremony_signatures).expect("ceremony signatures"))
            .expect("signature JSON");
    signatures["signatures"]
        .as_array_mut()
        .expect("signature array")
        .pop();
    fs::write(&files.ceremony_signatures, canonicalize(&signatures))
        .expect("incomplete signature set");

    let output_root = fixture.join("must-not-exist");
    let rejected = package_command(&files, &output_root);
    assert!(!rejected.status.success());
    let rejection: Value = serde_json::from_slice(&rejected.stderr).expect("typed rejection");
    assert_eq!(rejection["command"], "product.package-trust-bootstrap");
    assert_eq!(rejection["error"]["reason"], "trust-bootstrap-rejected");
    assert!(
        !output_root.exists(),
        "an incomplete five-key proof of possession must publish no package"
    );
}

#[test]
fn real_cli_cannot_turn_the_rehearsal_command_into_an_operational_ceremony() {
    let fixture = FixtureDir::new();
    let files = write_ceremony_files(&fixture);
    let mut ceremony: Value = serde_json::from_slice(&fs::read(&files.ceremony).expect("ceremony"))
        .expect("ceremony JSON");
    ceremony["environment"] = Value::String("production".to_string());
    let ceremony = canonicalize(&ceremony);
    let ceremony_sha256 = sha256(&ceremony);
    fs::write(&files.ceremony, ceremony).expect("operational ceremony request");
    let mut all_ids = vec![PRODUCT_ID, GUEST_ID];
    all_ids.extend(RECOVERY_IDS);
    fs::write(
        &files.ceremony_signatures,
        detached_signatures(
            "boole.operational-release-key-ceremony-signatures.v1",
            "ceremonySha256",
            &ceremony_sha256,
            CEREMONY_CONTEXT,
            &all_ids,
        ),
    )
    .expect("operational ceremony signatures");

    let output_root = fixture.join("must-not-be-operational");
    let rejected = package_command(&files, &output_root);
    assert!(!rejected.status.success());
    let rejection: Value = serde_json::from_slice(&rejected.stderr).expect("typed rejection");
    assert_eq!(rejection["error"]["reason"], "trust-bootstrap-rejected");
    assert!(
        !output_root.exists(),
        "this command must remain a non-production rehearsal"
    );
}
