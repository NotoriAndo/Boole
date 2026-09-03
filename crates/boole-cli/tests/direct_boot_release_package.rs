//! Offline direct-boot release packaging through the real CLI.
//!
//! The fixture uses non-production KAT roots. No upload, VM, network,
//! production signing decision or activation occurs.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, GuestArtifactRole, ProductArtifactRole, SigningKeyV2,
    OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
};
use boole_testkit::{
    write_bootable_curl_product_kat_metadata, BootableCurlProductKatInput,
    BootableCurlProductKatRelease,
};
use sha2::{Digest, Sha256};

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-direct-boot-release-package-{}-{nonce}",
            std::process::id()
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
            let Ok(metadata) = fs::metadata(path) else {
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

fn bundle(
    fixture: &FixtureDir,
    label: &str,
    release: BootableCurlProductKatRelease,
) -> (PathBuf, boole_testkit::BootableCurlProductKatRoots) {
    let sources = fixture.join(&format!("sources-{label}"));
    fs::create_dir(&sources).expect("sources");
    let mut product_artifacts = BTreeMap::new();
    for role in [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ] {
        let path = sources.join(role.as_str());
        fs::write(&path, format!("product:{}", role.as_str())).expect("product artifact");
        product_artifacts.insert(role, path);
    }
    let mut guest_artifacts = BTreeMap::new();
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        let path = sources.join(role.as_str());
        fs::write(&path, format!("guest:{}", role.as_str())).expect("guest artifact");
        guest_artifacts.insert(role, path);
    }
    let source_root = fixture.join(&format!("signed-{label}"));
    let roots = write_bootable_curl_product_kat_metadata(BootableCurlProductKatInput {
        output_dir: source_root.clone(),
        source_revision: "78".repeat(20),
        product_artifacts: product_artifacts.clone(),
        guest_artifacts: guest_artifacts.clone(),
        release,
    })
    .expect("KAT metadata");
    // The testkit writes injected public roots beside its fixture for test
    // consumers. They are operator input, not a signed release artifact and
    // therefore must not enter the hostable transport tree.
    fs::remove_file(source_root.join("TRUST-ROOTS.json")).expect("remove KAT-only roots file");
    let guest = source_root.join("guest");
    fs::create_dir(&guest).expect("guest transport root");
    for (role, path) in product_artifacts {
        fs::copy(path, source_root.join(role.as_str())).expect("product byte");
    }
    for (role, path) in guest_artifacts {
        fs::copy(path, guest.join(role.as_str())).expect("guest byte");
    }
    (source_root, roots)
}

fn digest(path: &Path) -> String {
    hex::encode(Sha256::digest(fs::read(path).expect("digest input")))
}

fn tree_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).expect("read package tree") {
            let entry = entry.expect("package entry");
            let path = entry.path();
            if entry.file_type().expect("entry type").is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root)
                        .expect("relative package path")
                        .to_path_buf(),
                    fs::read(path).expect("package bytes"),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn assert_tree_is_read_only(root: &Path) {
    assert_eq!(
        fs::metadata(root)
            .expect("tree metadata")
            .permissions()
            .mode()
            & 0o222,
        0,
        "package root is writable"
    );
    for entry in fs::read_dir(root).expect("read package permissions") {
        let path = entry.expect("package permission entry").path();
        assert_eq!(
            fs::metadata(&path)
                .expect("entry metadata")
                .permissions()
                .mode()
                & 0o222,
            0,
            "package entry is writable: {}",
            path.display()
        );
        if path.is_dir() {
            assert_tree_is_read_only(&path);
        }
    }
}

fn write_operational_trust_policy(
    fixture: &FixtureDir,
    roots: &boole_testkit::BootableCurlProductKatRoots,
) -> (PathBuf, PathBuf, PathBuf) {
    let recovery_ids = [
        "non-production-package-recovery-kat-a",
        "non-production-package-recovery-kat-b",
        "non-production-package-recovery-kat-c",
    ];
    let recovery_root = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-recovery-root.v1",
        "threshold": 2,
        "keys": recovery_ids
            .iter()
            .map(|id| serde_json::json!({
                "keyId": id,
                "publicKey": SigningKeyV2::from_dev_id(id).pk_hex()
            }))
            .collect::<Vec<_>>()
    }));
    let policy = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-trust-policy.v1",
        "generation": 1,
        "previousPolicySha256": null,
        "productRelease": {
            "status": "active",
            "keyId": roots.product_key_id,
            "publicKey": roots.product_public_key_hex
        },
        "guestRelease": {
            "status": "active",
            "keyId": roots.guest_key_id,
            "publicKey": roots.guest_public_key_hex
        },
        "recovery": {
            "threshold": 2,
            "keys": recovery_ids
                .iter()
                .map(|id| serde_json::json!({
                    "keyId": id,
                    "publicKey": SigningKeyV2::from_dev_id(id).pk_hex()
                }))
                .collect::<Vec<_>>()
        },
        "retiredKeys": []
    }));
    let policy_sha256 = hex::encode(Sha256::digest(&policy));
    let signing_payload = serde_json::json!({
        "context": OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        "policySha256": policy_sha256
    });
    let signatures = canonicalize(&serde_json::json!({
        "schema": "boole.operational-release-trust-policy-signatures.v1",
        "policySha256": policy_sha256,
        "signatures": recovery_ids[..2]
            .iter()
            .map(|id| {
                let key = SigningKeyV2::from_dev_id(id);
                let envelope = key.sign(&signing_payload).expect("policy KAT signature");
                serde_json::json!({
                    "keyId": id,
                    "publicKey": envelope.pk,
                    "signature": envelope.signature
                })
            })
            .collect::<Vec<_>>()
    }));
    let recovery_root_path = fixture.join("recovery-root.json");
    let policy_path = fixture.join("trust-policy.json");
    let signatures_path = fixture.join("trust-policy-signatures.json");
    fs::write(&recovery_root_path, recovery_root).expect("write recovery root");
    fs::write(&policy_path, policy).expect("write trust policy");
    fs::write(&signatures_path, signatures).expect("write trust policy signatures");
    (recovery_root_path, policy_path, signatures_path)
}

#[test]
fn real_cli_packages_with_recovery_authorized_policy_instead_of_raw_release_roots() {
    let fixture = FixtureDir::new();
    let (source, roots) = bundle(
        &fixture,
        "policy-authorized",
        BootableCurlProductKatRelease::default(),
    );
    let (recovery_root, policy, policy_signatures) =
        write_operational_trust_policy(&fixture, &roots);
    let output = fixture.join("policy-authorized-package");

    let packaged = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "package-direct-boot",
            "--source-root",
            source.to_str().expect("source path"),
            "--output-root",
            output.to_str().expect("output path"),
            "--recovery-root",
            recovery_root.to_str().expect("recovery root path"),
            "--trust-policy",
            policy.to_str().expect("trust policy path"),
            "--trust-policy-signatures",
            policy_signatures.to_str().expect("policy signatures path"),
            "--first-product-minimum",
            "1",
            "--first-guest-minimum",
            "1",
        ])
        .output()
        .expect("package through policy-authorized CLI");

    assert!(
        packaged.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&packaged.stdout).expect("package result JSON");
    assert_eq!(result["result"]["trustPolicyGeneration"], 1);
    assert_eq!(result["result"]["trustPolicySha256"], digest(&policy));
    assert_eq!(tree_bytes(&output), tree_bytes(&source));
}

#[test]
fn policy_below_the_recovery_threshold_leaves_no_package() {
    let fixture = FixtureDir::new();
    let (source, roots) = bundle(
        &fixture,
        "under-authorized-policy",
        BootableCurlProductKatRelease::default(),
    );
    let (recovery_root, policy, policy_signatures) =
        write_operational_trust_policy(&fixture, &roots);
    let mut signatures: serde_json::Value =
        serde_json::from_slice(&fs::read(&policy_signatures).expect("signature bytes"))
            .expect("signature JSON");
    signatures["signatures"]
        .as_array_mut()
        .expect("signature list")
        .pop();
    fs::write(&policy_signatures, canonicalize(&signatures)).expect("under-authorize policy");
    let output = fixture.join("must-not-exist");

    let rejected = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "package-direct-boot",
            "--source-root",
            source.to_str().expect("source path"),
            "--output-root",
            output.to_str().expect("output path"),
            "--recovery-root",
            recovery_root.to_str().expect("recovery root path"),
            "--trust-policy",
            policy.to_str().expect("trust policy path"),
            "--trust-policy-signatures",
            policy_signatures.to_str().expect("policy signatures path"),
            "--first-product-minimum",
            "1",
            "--first-guest-minimum",
            "1",
        ])
        .output()
        .expect("reject under-authorized policy");

    assert!(!rejected.status.success());
    let result: serde_json::Value =
        serde_json::from_slice(&rejected.stderr).expect("typed rejection");
    assert_eq!(result["error"]["reason"], "release-package-rejected");
    assert!(result["error"]["message"]
        .as_str()
        .expect("message")
        .contains("threshold"));
    assert!(!output.exists());
}

#[test]
fn real_cli_exports_only_a_fully_verified_atomic_transport_tree() {
    let fixture = FixtureDir::new();
    let (source, roots) = bundle(&fixture, "first", BootableCurlProductKatRelease::default());
    let output = fixture.join("published");
    let args = |source: &Path, output: &Path| {
        vec![
            "product".to_string(),
            "package-direct-boot".to_string(),
            "--source-root".to_string(),
            source.display().to_string(),
            "--output-root".to_string(),
            output.display().to_string(),
            "--product-trust-root-key-id".to_string(),
            roots.product_key_id.clone(),
            "--product-trust-root-public-key".to_string(),
            roots.product_public_key_hex.clone(),
            "--guest-trust-root-key-id".to_string(),
            roots.guest_key_id.clone(),
            "--guest-trust-root-public-key".to_string(),
            roots.guest_public_key_hex.clone(),
            "--first-product-minimum".to_string(),
            "1".to_string(),
            "--first-guest-minimum".to_string(),
            "1".to_string(),
        ]
    };
    let packaged = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args(&source, &output))
        .output()
        .expect("package through real CLI");
    assert!(
        packaged.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&packaged.stdout).expect("package result JSON");
    assert_eq!(result["command"], "product.package-direct-boot");
    assert_eq!(result["result"]["releaseSequence"], 1);
    assert_eq!(result["result"]["guestReleaseSequence"], 1);
    let accepted_tree = tree_bytes(&output);
    assert_eq!(accepted_tree, tree_bytes(&source));
    assert_tree_is_read_only(&output);

    fs::write(
        source
            .join("guest")
            .join(GuestArtifactRole::GuestRootDisk.as_str()),
        b"tampered after signing",
    )
    .expect("tamper guest source");
    let rejected_output = fixture.join("rejected-output");
    let rejected = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args(&source, &rejected_output))
        .output()
        .expect("reject through real CLI");
    assert!(!rejected.status.success());
    let rejection: serde_json::Value =
        serde_json::from_slice(&rejected.stderr).expect("rejection JSON");
    assert_eq!(rejection["error"]["reason"], "release-package-rejected");
    assert!(
        !rejected_output.exists(),
        "a rejected source must leave no output tree"
    );
    assert_eq!(
        tree_bytes(&output),
        accepted_tree,
        "the accepted immutable package remains unchanged"
    );
}

#[test]
fn real_cli_packages_only_a_successor_bound_to_both_authenticated_floors() {
    let fixture = FixtureDir::new();
    let (first, _) = bundle(
        &fixture,
        "predecessor",
        BootableCurlProductKatRelease::default(),
    );
    let product_predecessor = digest(&first.join("release-manifest.json"));
    let guest_predecessor = digest(&first.join("guest-update-manifest"));
    let (successor, roots) = bundle(
        &fixture,
        "successor",
        BootableCurlProductKatRelease {
            product_sequence: 2,
            product_version: "0.0.1-package-successor-kat".to_string(),
            product_previous_manifest_sha256: Some(product_predecessor.clone()),
            guest_sequence: 2,
            guest_version: "0.0.1-package-successor-guest-kat".to_string(),
            guest_previous_manifest_sha256: Some(guest_predecessor.clone()),
        },
    );
    let output = fixture.join("successor-package");
    let packaged = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "package-direct-boot",
            "--source-root",
            successor.to_str().expect("successor path"),
            "--output-root",
            output.to_str().expect("output path"),
            "--product-trust-root-key-id",
            &roots.product_key_id,
            "--product-trust-root-public-key",
            &roots.product_public_key_hex,
            "--guest-trust-root-key-id",
            &roots.guest_key_id,
            "--guest-trust-root-public-key",
            &roots.guest_public_key_hex,
            "--product-floor-sequence",
            "1",
            "--product-floor-manifest-sha256",
            &product_predecessor,
            "--guest-floor-sequence",
            "1",
            "--guest-floor-manifest-sha256",
            &guest_predecessor,
        ])
        .output()
        .expect("package successor through real CLI");
    assert!(
        packaged.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );
    let result: serde_json::Value =
        serde_json::from_slice(&packaged.stdout).expect("successor result JSON");
    assert_eq!(result["result"]["releaseSequence"], 2);
    assert_eq!(result["result"]["guestReleaseSequence"], 2);
    assert_eq!(tree_bytes(&output), tree_bytes(&successor));
    assert_tree_is_read_only(&output);

    let wrong_output = fixture.join("wrong-predecessor-output");
    let rejected = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "package-direct-boot",
            "--source-root",
            successor.to_str().expect("successor path"),
            "--output-root",
            wrong_output.to_str().expect("wrong output path"),
            "--product-trust-root-key-id",
            &roots.product_key_id,
            "--product-trust-root-public-key",
            &roots.product_public_key_hex,
            "--guest-trust-root-key-id",
            &roots.guest_key_id,
            "--guest-trust-root-public-key",
            &roots.guest_public_key_hex,
            "--product-floor-sequence",
            "1",
            "--product-floor-manifest-sha256",
            &"00".repeat(32),
            "--guest-floor-sequence",
            "1",
            "--guest-floor-manifest-sha256",
            &guest_predecessor,
        ])
        .output()
        .expect("reject wrong predecessor through real CLI");
    assert!(!rejected.status.success());
    let rejection: serde_json::Value =
        serde_json::from_slice(&rejected.stderr).expect("successor rejection JSON");
    assert_eq!(rejection["error"]["reason"], "release-package-rejected");
    assert!(!wrong_output.exists());
}
