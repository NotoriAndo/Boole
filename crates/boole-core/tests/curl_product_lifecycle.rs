//! Installed direct-boot product update and recovery lifecycle.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, install_curl_product_release, install_direct_boot_curl_product_release,
    open_verified_installed_direct_boot_curl_product_release, read_installed_curl_product_state,
    recover_corrupt_installed_direct_boot_curl_product_release,
    rollback_installed_direct_boot_curl_product_release, CurlProductReleaseTrustRoot,
    GuestArtifactRole, NativeShadowUpdateTrustRoot, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY, CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY,
    CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA, CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3,
    CURL_PRODUCT_RELEASE_SIGNING_CONTEXT, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3,
    GUEST_UPDATE_MANIFEST_SCHEMA_V3, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const PRODUCT_KEY_ID: &str = "non-production-product-lifecycle-kat";
const GUEST_KEY_ID: &str = "non-production-guest-lifecycle-kat";

struct DirectBootFixture {
    product_trust: CurlProductReleaseTrustRoot,
    guest_trust: NativeShadowUpdateTrustRoot,
    product_manifest: Vec<u8>,
    product_signature: Vec<u8>,
    product_files: BTreeMap<ProductArtifactRole, Vec<u8>>,
    guest_files: BTreeMap<GuestArtifactRole, Vec<u8>>,
}

fn signature(key: &SigningKeyV2, key_id: &str, raw: &[u8], context: &str) -> Vec<u8> {
    let payload = serde_json::from_slice(raw).expect("payload JSON");
    let envelope = key
        .sign_for_network(&payload, Some(context))
        .expect("KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": key_id,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(raw)),
    }))
}

fn direct_boot_fixture(
    sequence: u64,
    previous_product_manifest: Option<&str>,
    previous_guest_manifest: Option<&str>,
) -> DirectBootFixture {
    let version = format!("3.0.{}", sequence - 1);
    let guest_files: BTreeMap<_, _> = GuestArtifactRole::DIRECT_BOOT_ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("guest-v3-{sequence}:{}", role.as_str()).into_bytes(),
            )
        })
        .collect();
    let guest_descriptors: Vec<_> = GuestArtifactRole::DIRECT_BOOT_ALL
        .into_iter()
        .map(|role| {
            let bytes = &guest_files[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    let guest_manifest = canonicalize(&json!({
        "schema": GUEST_UPDATE_MANIFEST_SCHEMA_V3,
        "bootFormatVersion": 2,
        "channel": "stable",
        "releaseSequence": sequence,
        "releaseVersion": version,
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": previous_guest_manifest,
        "artifacts": guest_descriptors,
    }));
    let guest_key = SigningKeyV2::from_dev_id(GUEST_KEY_ID);
    let guest_signature = signature(
        &guest_key,
        GUEST_KEY_ID,
        &guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
    );

    let mut product_files: BTreeMap<_, _> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| {
        (
            role,
            format!("host-v3-{sequence}:{}", role.as_str()).into_bytes(),
        )
    })
    .collect();
    product_files.insert(ProductArtifactRole::GuestUpdateManifest, guest_manifest);
    product_files.insert(ProductArtifactRole::GuestUpdateSignature, guest_signature);
    let product_descriptors: Vec<_> = ProductArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &product_files[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    let product_manifest = canonicalize(&json!({
        "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3,
        "channel": "stable",
        "releaseSequence": sequence,
        "releaseVersion": version,
        "sourceRevision": format!("{:040x}", sequence),
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": previous_product_manifest,
        "controllerProtocolVersion": 1,
        "guestManifestSha256": hex::encode(Sha256::digest(
            &product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
        "guestReleaseSequence": sequence,
        "guestReleaseVersion": version,
        "artifacts": product_descriptors,
    }));
    let product_key = SigningKeyV2::from_dev_id(PRODUCT_KEY_ID);
    let product_signature = signature(
        &product_key,
        PRODUCT_KEY_ID,
        &product_manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3,
    );
    DirectBootFixture {
        product_trust: CurlProductReleaseTrustRoot::new(PRODUCT_KEY_ID, &product_key.pk_hex())
            .expect("product trust root"),
        guest_trust: NativeShadowUpdateTrustRoot::new(GUEST_KEY_ID, &guest_key.pk_hex())
            .expect("guest trust root"),
        product_manifest,
        product_signature,
        product_files,
        guest_files,
    }
}

fn digest(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-curl-product-lifecycle-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture root");
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_sources(root: &Path, fixture: &DirectBootFixture) -> (PathBuf, PathBuf) {
    let product = root.join("product");
    let guest = root.join("guest");
    fs::create_dir_all(&product).expect("product source");
    fs::create_dir_all(&guest).expect("guest source");
    for role in ProductArtifactRole::ALL {
        fs::write(
            product.join(format!("{}.bin", role.as_str())),
            &fixture.product_files[&role],
        )
        .expect("product artifact");
    }
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        fs::write(
            guest.join(format!("{}.bin", role.as_str())),
            &fixture.guest_files[&role],
        )
        .expect("guest artifact");
    }
    (product, guest)
}

fn install(root: &Path, sources: &Path, fixture: &DirectBootFixture) {
    let (product, guest) = write_sources(sources, fixture);
    install_direct_boot_curl_product_release(
        root,
        &fixture.product_manifest,
        &fixture.product_signature,
        &fixture.product_trust,
        1,
        &product,
        &fixture.guest_trust,
        1,
        &guest,
    )
    .expect("direct-boot install");
}

#[test]
fn signed_update_can_roll_back_without_lowering_the_release_floor() {
    let dir = FixtureDir::new("rollback");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);

    let first_product_digest = digest(&first.product_manifest);
    let first_guest_digest =
        digest(&first.product_files[&ProductArtifactRole::GuestUpdateManifest]);
    let second = direct_boot_fixture(2, Some(&first_product_digest), Some(&first_guest_digest));
    install(&install_root, &dir.0.join("second"), &second);

    rollback_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("verified rollback");

    let state = read_installed_curl_product_state(&install_root)
        .expect("read state")
        .expect("installed state");
    assert_eq!(state.release_sequence(), 1, "generation one is active");
    assert_eq!(
        state.release_floor_sequence(),
        2,
        "the accepted update remains the anti-rollback floor"
    );
    assert_eq!(state.rollback_release_sequence(), Some(2));

    let active = open_verified_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("rolled-back generation reopens through both trust roots");
    assert_eq!(active.product().release_sequence(), 1);
    assert_eq!(active.guest().release_sequence(), 1);
}

#[test]
fn a_corrupt_active_guest_recovers_only_to_the_verified_rollback_generation() {
    let dir = FixtureDir::new("corrupt-recovery");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);

    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    let before = read_installed_curl_product_state(&install_root)
        .expect("state")
        .expect("installed");
    fs::write(
        install_root
            .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(before.version_directory())
            .join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY)
            .join(format!("{}.bin", GuestArtifactRole::GuestRootDisk.as_str())),
        b"corrupt active guest root disk",
    )
    .expect("corrupt active guest");

    let recovered = recover_corrupt_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("explicit recovery");
    assert_eq!(recovered.release_sequence(), 1);
    assert_eq!(recovered.release_floor_sequence(), 2);
    assert_eq!(recovered.guest_release_floor_sequence(), Some(2));
    assert_eq!(recovered.rollback_release_sequence(), None);

    let active = open_verified_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("recovered generation reopens");
    assert_eq!(active.product().release_sequence(), 1);
    assert_eq!(active.guest().release_sequence(), 1);
}

#[test]
fn recovery_can_advance_from_the_preserved_floor_without_retaining_corrupt_rollback() {
    let dir = FixtureDir::new("recover-then-update");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);
    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    let second_state = read_installed_curl_product_state(&install_root)
        .expect("state")
        .expect("installed");
    fs::write(
        install_root
            .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(second_state.version_directory())
            .join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY)
            .join(format!("{}.bin", GuestArtifactRole::GuestRootDisk.as_str())),
        b"corrupt active guest root disk",
    )
    .expect("corrupt active guest");
    recover_corrupt_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("recover first generation");

    let third = direct_boot_fixture(
        3,
        Some(&digest(&second.product_manifest)),
        Some(&digest(
            &second.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("third"), &third);

    let state = read_installed_curl_product_state(&install_root)
        .expect("state")
        .expect("installed");
    assert_eq!(state.release_sequence(), 3);
    assert_eq!(state.release_floor_sequence(), 3);
    assert_eq!(state.guest_release_floor_sequence(), Some(3));
    assert_eq!(
        state.rollback_release_sequence(),
        Some(1),
        "the corrupt second generation must not become rollback material"
    );
    let version_count = fs::read_dir(install_root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY))
        .expect("versions")
        .count();
    assert_eq!(
        version_count, 2,
        "only the active and one verified rollback generation are retained"
    );
}

#[test]
fn a_tampered_rollback_target_cannot_change_the_active_release() {
    let dir = FixtureDir::new("tampered-rollback");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);
    let first_state = read_installed_curl_product_state(&install_root)
        .expect("state")
        .expect("installed");
    let first_directory = first_state.version_directory().to_string();
    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    fs::write(
        install_root
            .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(first_directory)
            .join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY)
            .join(format!("{}.bin", GuestArtifactRole::GuestRootDisk.as_str())),
        b"tampered retained rollback",
    )
    .expect("tamper rollback target");
    let state_path = install_root.join("installed-release.json");
    let state_before = fs::read(&state_path).expect("state bytes");

    rollback_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect_err("tampered rollback target is rejected");

    assert_eq!(fs::read(&state_path).expect("state after"), state_before);
    let active = open_verified_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("second generation remains active");
    assert_eq!(active.product().release_sequence(), 2);
}

#[test]
fn rollback_does_not_make_the_rolled_back_sequence_installable_again() {
    let dir = FixtureDir::new("rollback-replay");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);
    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    rollback_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("rollback");
    let state_path = install_root.join("installed-release.json");
    let before = fs::read(&state_path).expect("state before replay");
    let (product, guest) = write_sources(&dir.0.join("replay"), &second);

    install_direct_boot_curl_product_release(
        &install_root,
        &second.product_manifest,
        &second.product_signature,
        &second.product_trust,
        1,
        &product,
        &second.guest_trust,
        1,
        &guest,
    )
    .expect_err("the accepted second generation cannot be replayed");

    assert_eq!(fs::read(&state_path).expect("state after replay"), before);
}

#[test]
fn lifecycle_state_cannot_drop_the_authenticated_guest_floor() {
    let dir = FixtureDir::new("missing-guest-floor");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);
    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    let state_path = install_root.join("installed-release.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("state bytes")).expect("state JSON");
    state
        .as_object_mut()
        .expect("state object")
        .remove("guestReleaseFloor");
    fs::write(&state_path, canonicalize(&state)).expect("tampered state");

    let error = read_installed_curl_product_state(&install_root)
        .expect_err("v2 lifecycle state without a guest floor must fail closed");
    assert!(error.to_string().contains("guestReleaseFloor"));
}

#[test]
fn lifecycle_state_rejects_two_identities_for_the_same_sequence() {
    let dir = FixtureDir::new("equivocated-active-rollback");
    let install_root = dir.0.join("install");
    let first = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("first"), &first);
    let second = direct_boot_fixture(
        2,
        Some(&digest(&first.product_manifest)),
        Some(&digest(
            &first.product_files[&ProductArtifactRole::GuestUpdateManifest],
        )),
    );
    install(&install_root, &dir.0.join("second"), &second);
    rollback_installed_direct_boot_curl_product_release(
        &install_root,
        &second.product_trust,
        &second.guest_trust,
    )
    .expect("rollback creates v2 active and rollback identities");

    let state_path = install_root.join("installed-release.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).expect("state bytes")).expect("state JSON");
    let mut conflicting = state["activeRelease"].clone();
    conflicting["manifestSha256"] = serde_json::Value::String("00".repeat(32));
    conflicting["versionDirectory"] =
        serde_json::Value::String("000000000001-000000000000".to_string());
    state["rollbackRelease"] = conflicting;
    fs::write(&state_path, canonicalize(&state)).expect("tampered state");

    let error = read_installed_curl_product_state(&install_root)
        .expect_err("one sequence cannot name two different releases");
    assert!(error.to_string().contains("same release sequence"));
}

#[test]
fn product_only_update_cannot_replace_a_direct_boot_installation() {
    let dir = FixtureDir::new("weaker-product-only-update");
    let install_root = dir.0.join("install");
    let direct = direct_boot_fixture(1, None, None);
    install(&install_root, &dir.0.join("direct"), &direct);
    let state_path = install_root.join("installed-release.json");
    let state_before = fs::read(&state_path).expect("state before weaker update");

    let descriptors: Vec<_> = ProductArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &direct.product_files[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    let guest_manifest = &direct.product_files[&ProductArtifactRole::GuestUpdateManifest];
    let weaker_manifest = canonicalize(&json!({
        "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA,
        "channel": "stable",
        "releaseSequence": 2,
        "releaseVersion": "3.0.1",
        "sourceRevision": format!("{:040x}", 2),
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": digest(&direct.product_manifest),
        "controllerProtocolVersion": 1,
        "guestManifestSha256": digest(guest_manifest),
        "guestReleaseSequence": 1,
        "guestReleaseVersion": "3.0.0",
        "artifacts": descriptors,
    }));
    let product_key = SigningKeyV2::from_dev_id(PRODUCT_KEY_ID);
    let weaker_signature = signature(
        &product_key,
        PRODUCT_KEY_ID,
        &weaker_manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    );
    let (product_sources, _) = write_sources(&dir.0.join("weaker"), &direct);

    install_curl_product_release(
        &install_root,
        &weaker_manifest,
        &weaker_signature,
        &direct.product_trust,
        1,
        &product_sources,
    )
    .expect_err("the product-only signing context cannot replace direct boot");

    assert_eq!(
        fs::read(&state_path).expect("state after weaker update"),
        state_before,
        "the direct-boot installation remains selected byte-for-byte"
    );
    open_verified_installed_direct_boot_curl_product_release(
        &install_root,
        &direct.product_trust,
        &direct.guest_trust,
    )
    .expect("the original direct-boot installation still reopens");
}
