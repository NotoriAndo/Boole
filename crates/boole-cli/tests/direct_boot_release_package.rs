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

use boole_core::{GuestArtifactRole, ProductArtifactRole};
use boole_testkit::{
    write_bootable_curl_product_kat_metadata, BootableCurlProductKatInput,
    BootableCurlProductKatRelease,
};

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

fn bundle(fixture: &FixtureDir) -> (PathBuf, boole_testkit::BootableCurlProductKatRoots) {
    let sources = fixture.join("sources");
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
    let source_root = fixture.join("signed-source");
    let roots = write_bootable_curl_product_kat_metadata(BootableCurlProductKatInput {
        output_dir: source_root.clone(),
        source_revision: "78".repeat(20),
        product_artifacts: product_artifacts.clone(),
        guest_artifacts: guest_artifacts.clone(),
        release: BootableCurlProductKatRelease::default(),
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

#[test]
fn real_cli_exports_only_a_fully_verified_atomic_transport_tree() {
    let fixture = FixtureDir::new();
    let (source, roots) = bundle(&fixture);
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
