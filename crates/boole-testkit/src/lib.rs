//! Shared test helpers for the Boole workspace.
//!
//! P0.1a — minimal first slice. Exposes three helpers that the master plan
//! L10 contract names and that are duplicated across 30+ test files today:
//! `rand_suffix`, `repo_root`, `lake_and_lean_available`. Later P0.1 slices
//! add `TempStateDir`, `start_node`, `FixtureCatalog`, `MockBountyVerifier`,
//! `MockSubmitter`, `MockChainHead`.
//!
//! Production crates must not depend on this crate. It is a `dev-dependencies`
//! target only (via `[dev-dependencies] boole-testkit = { path = ... }` in
//! each consuming crate). Keeping it out of the production dep graph means
//! a release build does not link the mock surface.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, GuestArtifactRole, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_INSTALLED_MANIFEST_FILE, CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2,
    GUEST_UPDATE_MANIFEST_SCHEMA_V2, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};

pub const INSTALLED_MAC_E2E_PRODUCT_KAT_KEY_ID: &str =
    "non-production-installed-mac-e2e-product-kat-v1";
pub const INSTALLED_MAC_E2E_GUEST_KAT_KEY_ID: &str =
    "non-production-installed-mac-e2e-guest-kat-v1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BootableCurlProductKatInput {
    pub output_dir: PathBuf,
    pub source_revision: String,
    pub product_artifacts: BTreeMap<ProductArtifactRole, PathBuf>,
    pub guest_artifacts: BTreeMap<GuestArtifactRole, PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootableCurlProductKatRoots {
    pub product_key_id: String,
    pub product_public_key_hex: String,
    pub guest_key_id: String,
    pub guest_public_key_hex: String,
}

fn artifact_descriptor(role: &str, file_name: &str, path: &Path) -> io::Result<serde_json::Value> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "KAT artifact {} is not a regular non-symlink file",
                path.display()
            ),
        ));
    }
    let mut source = File::open(path)?;
    let mut digest = Sha256::new();
    let mut observed = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        observed = observed.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "KAT artifact length overflow")
        })?;
        digest.update(&buffer[..read]);
    }
    if observed != metadata.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("KAT artifact {} changed while hashing", path.display()),
        ));
    }
    Ok(json!({
        "role": role,
        "fileName": file_name,
        "byteLength": observed,
        "sha256": hex::encode(digest.finalize()),
    }))
}

fn detached_signature(
    signing_key: &SigningKeyV2,
    key_id: &str,
    payload: &[u8],
    context: &str,
) -> io::Result<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let envelope = signing_key
        .sign_for_network(&value, Some(context))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": key_id,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(payload)),
    })))
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::options().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Write only the signed metadata for a closed-local installed-Mac E2E
/// bundle. The caller supplies real artifact paths and later exposes those
/// bytes under the signed basenames. This helper lives in `boole-testkit`, so
/// no release binary can derive or carry either deterministic KAT key.
pub fn write_bootable_curl_product_kat_metadata(
    input: BootableCurlProductKatInput,
) -> io::Result<BootableCurlProductKatRoots> {
    if input.source_revision.len() != 40
        || !input
            .source_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "KAT source revision must be 40 lowercase hexadecimal characters",
        ));
    }
    let expected_product: BTreeSet<_> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .collect();
    if input
        .product_artifacts
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != expected_product
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "KAT product inputs must contain the exact four host roles",
        ));
    }
    let expected_guest: BTreeSet<_> = GuestArtifactRole::BOOTABLE_ALL.into_iter().collect();
    if input
        .guest_artifacts
        .keys()
        .copied()
        .collect::<BTreeSet<_>>()
        != expected_guest
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "KAT guest inputs must contain the exact twelve bootable roles",
        ));
    }
    fs::create_dir(&input.output_dir)?;
    let result = (|| {
        let guest_descriptors = GuestArtifactRole::BOOTABLE_ALL
            .into_iter()
            .map(|role| {
                artifact_descriptor(role.as_str(), role.as_str(), &input.guest_artifacts[&role])
            })
            .collect::<io::Result<Vec<_>>>()?;
        let guest_manifest = canonicalize(&json!({
            "schema": GUEST_UPDATE_MANIFEST_SCHEMA_V2,
            "bootFormatVersion": 1,
            "channel": "stable",
            "releaseSequence": 1,
            "releaseVersion": "0.0.0-installed-mac-e2e-kat",
            "targetOs": "linux",
            "targetArch": "aarch64",
            "previousManifestSha256": null,
            "artifacts": guest_descriptors,
        }));
        let guest_key = SigningKeyV2::from_dev_id(INSTALLED_MAC_E2E_GUEST_KAT_KEY_ID);
        let guest_signature = detached_signature(
            &guest_key,
            INSTALLED_MAC_E2E_GUEST_KAT_KEY_ID,
            &guest_manifest,
            NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
        )?;
        write_new(
            &input.output_dir.join("guest-update-manifest"),
            &guest_manifest,
        )?;
        write_new(
            &input.output_dir.join("guest-update-signature"),
            &guest_signature,
        )?;

        let mut product_descriptors = Vec::with_capacity(ProductArtifactRole::ALL.len());
        for role in ProductArtifactRole::ALL {
            let descriptor = match role {
                ProductArtifactRole::GuestUpdateManifest => json!({
                    "role": role.as_str(),
                    "fileName": "guest-update-manifest",
                    "byteLength": guest_manifest.len(),
                    "sha256": hex::encode(Sha256::digest(&guest_manifest)),
                }),
                ProductArtifactRole::GuestUpdateSignature => json!({
                    "role": role.as_str(),
                    "fileName": "guest-update-signature",
                    "byteLength": guest_signature.len(),
                    "sha256": hex::encode(Sha256::digest(&guest_signature)),
                }),
                _ => artifact_descriptor(
                    role.as_str(),
                    role.as_str(),
                    &input.product_artifacts[&role],
                )?,
            };
            product_descriptors.push(descriptor);
        }
        let product_manifest = canonicalize(&json!({
            "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2,
            "channel": "stable",
            "releaseSequence": 1,
            "releaseVersion": "0.0.0-installed-mac-e2e-kat",
            "sourceRevision": input.source_revision,
            "targetOs": "macos",
            "targetArch": "arm64",
            "minimumMacOs": "14.0",
            "previousManifestSha256": null,
            "controllerProtocolVersion": 1,
            "guestManifestSha256": hex::encode(Sha256::digest(&guest_manifest)),
            "guestReleaseSequence": 1,
            "guestReleaseVersion": "0.0.0-installed-mac-e2e-kat",
            "artifacts": product_descriptors,
        }));
        let product_key = SigningKeyV2::from_dev_id(INSTALLED_MAC_E2E_PRODUCT_KAT_KEY_ID);
        let product_signature = detached_signature(
            &product_key,
            INSTALLED_MAC_E2E_PRODUCT_KAT_KEY_ID,
            &product_manifest,
            CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2,
        )?;
        write_new(
            &input.output_dir.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
            &product_manifest,
        )?;
        write_new(
            &input.output_dir.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
            &product_signature,
        )?;
        let roots = BootableCurlProductKatRoots {
            product_key_id: INSTALLED_MAC_E2E_PRODUCT_KAT_KEY_ID.to_string(),
            product_public_key_hex: product_key.pk_hex(),
            guest_key_id: INSTALLED_MAC_E2E_GUEST_KAT_KEY_ID.to_string(),
            guest_public_key_hex: guest_key.pk_hex(),
        };
        write_new(
            &input.output_dir.join("TRUST-ROOTS.json"),
            &canonicalize(
                &serde_json::to_value(&roots)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            ),
        )?;
        File::open(&input.output_dir)?.sync_all()?;
        Ok(roots)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&input.output_dir);
    }
    result
}

#[cfg(test)]
mod bootable_curl_product_kat_tests {
    use super::{write_bootable_curl_product_kat_metadata, BootableCurlProductKatInput};
    use boole_core::{
        GuestArtifactRole, ProductArtifactRole, CURL_PRODUCT_INSTALLED_MANIFEST_FILE,
        CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    };
    use sha2::Digest;
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn kat_metadata_binds_every_real_input_and_emits_both_public_roots() {
        let root = std::env::temp_dir().join(format!(
            "boole-bootable-kat-metadata-{}-{}",
            std::process::id(),
            super::rand_suffix()
        ));
        fs::create_dir(&root).expect("fixture root");
        let mut product = BTreeMap::new();
        for role in [
            ProductArtifactRole::HostCli,
            ProductArtifactRole::HostNode,
            ProductArtifactRole::HostWalletAgent,
            ProductArtifactRole::HostController,
        ] {
            let path = root.join(role.as_str());
            fs::write(&path, format!("product:{}", role.as_str())).expect("product fixture");
            product.insert(role, path);
        }
        let mut guest = BTreeMap::new();
        for role in GuestArtifactRole::BOOTABLE_ALL {
            let path = root.join(role.as_str());
            fs::write(&path, format!("guest:{}", role.as_str())).expect("guest fixture");
            guest.insert(role, path);
        }
        let output = root.join("metadata");
        let result = write_bootable_curl_product_kat_metadata(BootableCurlProductKatInput {
            output_dir: output.clone(),
            source_revision: "12".repeat(20),
            product_artifacts: product,
            guest_artifacts: guest,
        })
        .expect("write KAT metadata");

        assert_eq!(
            result.product_key_id,
            "non-production-installed-mac-e2e-product-kat-v1"
        );
        assert_eq!(
            result.guest_key_id,
            "non-production-installed-mac-e2e-guest-kat-v1"
        );
        assert_eq!(result.product_public_key_hex.len(), 64);
        assert_eq!(result.guest_public_key_hex.len(), 64);
        for path in [
            output.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
            output.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
            output.join("guest-update-manifest"),
            output.join("guest-update-signature"),
            output.join("TRUST-ROOTS.json"),
        ] {
            assert!(path.is_file(), "missing {}", path.display());
        }
        let product_manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(output.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE)).unwrap(),
        )
        .unwrap();
        let guest_manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("guest-update-manifest")).unwrap())
                .unwrap();
        assert_eq!(product_manifest["artifacts"].as_array().unwrap().len(), 6);
        assert_eq!(guest_manifest["artifacts"].as_array().unwrap().len(), 12);
        assert_eq!(
            product_manifest["guestManifestSha256"],
            hex::encode(sha2::Sha256::digest(
                fs::read(output.join("guest-update-manifest")).unwrap()
            ))
        );
        let _ = fs::remove_dir_all(root);
    }
}

/// Return the workspace root as an absolute path.
///
/// The crate lives at `<root>/crates/boole-testkit`, so the workspace root
/// is two parents up from `CARGO_MANIFEST_DIR`. This avoids `canonicalize`
/// to keep the path stable when test directories live on symlinked volumes.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("boole-testkit lives at crates/boole-testkit; workspace root is two parents up")
        .to_path_buf()
}

/// Return a nanosecond-resolution monotonic-ish suffix suitable for naming
/// tempdirs and per-test state directories. Wall-clock nanos are mixed with
/// a process-local atomic counter so two successive calls inside the same
/// test binary cannot collide even when wall-clock resolution is coarser
/// than nanoseconds (this happened on macOS — see history of
/// `tests/reward_store_divergence.rs`).
///
/// Not cryptographically random; callers that need collision-free names
/// across processes should still pair this with a process-id prefix.
pub fn rand_suffix() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed);
    nanos.wrapping_add(bump.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// True iff both `lake` and `lean` are on `PATH` and respond to `--version`.
///
/// Used by Lean-bridge tests to gate themselves: if the toolchain is missing
/// the test must be `#[ignore = "needs-lean"]`-style annotated; the early
/// `if !lake_and_lean_available() { return; }` pattern is being phased out
/// (see master plan L10).
pub fn lake_and_lean_available() -> bool {
    let lake_ok = Command::new("lake")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    let lean_ok = Command::new("lean")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    lake_ok && lean_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_points_at_workspace() {
        let root = repo_root();
        assert!(
            root.join("Cargo.toml").is_file(),
            "repo_root() should resolve to a directory containing the workspace Cargo.toml, got {}",
            root.display()
        );
        assert!(
            root.join("crates").join("boole-testkit").is_dir(),
            "repo_root() should contain crates/boole-testkit, got {}",
            root.display()
        );
    }

    #[test]
    fn rand_suffix_two_calls_never_collide_in_same_process() {
        // The atomic-counter mix guarantees inequality even when wall-clock
        // resolution would otherwise duplicate the value.
        let a = rand_suffix();
        let b = rand_suffix();
        assert_ne!(a, b, "rand_suffix must not return duplicates: {a} == {b}");
    }
}
