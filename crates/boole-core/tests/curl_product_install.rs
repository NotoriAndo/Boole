use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    canonicalize, install_curl_product_release, read_installed_curl_product_state,
    CurlProductInstallError, CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError,
    GuestArtifactRole, InstalledCurlProduct, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_INSTALLED_MANIFEST_FILE, CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    CURL_PRODUCT_INSTALL_STAGING_DIRECTORY, CURL_PRODUCT_INSTALL_STATE_FILE,
    CURL_PRODUCT_INSTALL_STATE_SCHEMA, CURL_PRODUCT_INSTALL_STATE_TEMP_FILE,
    CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-kat-2026-08-25";
const KAT_SOURCE_REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn guest_artifact_bytes() -> BTreeMap<GuestArtifactRole, Vec<u8>> {
    GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("guest-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect()
}

fn guest_manifest_bytes() -> Vec<u8> {
    let artifacts = guest_artifact_bytes();
    let descriptors: Vec<_> = GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    canonicalize(&json!({
        "schema": "boole.native-shadow.guest-update-manifest.v1",
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": descriptors,
    }))
}

fn envelope_for(
    signing_key: &SigningKeyV2,
    key_id: &str,
    manifest: &[u8],
    context: &str,
) -> Vec<u8> {
    let payload: serde_json::Value = serde_json::from_slice(manifest).expect("manifest payload");
    let envelope = signing_key
        .sign_for_network(&payload, Some(context))
        .expect("KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": key_id,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(manifest)),
    }))
}

fn product_artifact_bytes() -> BTreeMap<ProductArtifactRole, Vec<u8>> {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let guest_manifest = guest_manifest_bytes();
    let guest_signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );
    let mut artifacts: BTreeMap<ProductArtifactRole, Vec<u8>> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| (role, format!("host-fixture:{}", role.as_str()).into_bytes()))
    .collect();
    artifacts.insert(ProductArtifactRole::GuestUpdateManifest, guest_manifest);
    artifacts.insert(ProductArtifactRole::GuestUpdateSignature, guest_signature);
    artifacts
}

fn product_manifest_bytes(artifacts: &BTreeMap<ProductArtifactRole, Vec<u8>>) -> Vec<u8> {
    let descriptors: Vec<_> = ProductArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = &artifacts[&role];
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    let guest_manifest_sha256 = hex::encode(Sha256::digest(
        &artifacts[&ProductArtifactRole::GuestUpdateManifest],
    ));
    canonicalize(&json!({
        "schema": "boole.curl-product-release.v1",
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "sourceRevision": KAT_SOURCE_REVISION,
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": null,
        "controllerProtocolVersion": 1,
        "guestManifestSha256": guest_manifest_sha256,
        "guestReleaseSequence": 1,
        "guestReleaseVersion": "1.0.0",
        "artifacts": descriptors,
    }))
}

fn product_signature_for(signing_key: &SigningKeyV2, key_id: &str, manifest: &[u8]) -> Vec<u8> {
    envelope_for(
        signing_key,
        key_id,
        manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    )
}

type ProductFixture = (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
);

fn signed_product_fixture() -> ProductFixture {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let manifest = product_manifest_bytes(&artifacts);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    (trust_root, manifest, signature, artifacts)
}

fn resigned_manifest(mutate: impl FnOnce(&mut serde_json::Value)) -> ProductFixture {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let base = product_manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    mutate(&mut value);
    let manifest = canonicalize(&value);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    (trust_root, manifest, signature, artifacts)
}

fn signed_successor_fixture(previous_manifest_sha256: &str) -> ProductFixture {
    let previous = previous_manifest_sha256.to_string();
    resigned_manifest(move |value| {
        value["releaseSequence"] = json!(2);
        value["releaseVersion"] = json!("1.0.1");
        value["previousManifestSha256"] = json!(previous);
    })
}

struct FixtureDir {
    root: PathBuf,
}

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "boole-curl-product-install-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture dir");
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_source_artifacts(source_dir: &Path, artifacts: &BTreeMap<ProductArtifactRole, Vec<u8>>) {
    fs::create_dir_all(source_dir).expect("create source dir");
    for (role, bytes) in artifacts {
        fs::write(source_dir.join(format!("{}.bin", role.as_str())), bytes)
            .expect("write source artifact");
    }
}

fn install(
    root: &Path,
    fixture: &ProductFixture,
    first_install_minimum_sequence: u64,
    source_dir: &Path,
) -> Result<InstalledCurlProduct, CurlProductInstallError> {
    let (trust_root, manifest, signature, _) = fixture;
    install_curl_product_release(
        root,
        manifest,
        signature,
        trust_root,
        first_install_minimum_sequence,
        source_dir,
    )
}

fn manifest_sha256_hex(fixture: &ProductFixture) -> String {
    hex::encode(Sha256::digest(&fixture.1))
}

fn expected_version_directory(sequence: u64, manifest_sha256: &str) -> String {
    format!("{sequence:012}-{}", &manifest_sha256[..12])
}

fn expected_state_bytes(
    sequence: u64,
    version: &str,
    manifest_sha256: &str,
    version_directory: &str,
) -> Vec<u8> {
    canonicalize(&json!({
        "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
        "releaseSequence": sequence,
        "releaseVersion": version,
        "manifestSha256": manifest_sha256,
        "versionDirectory": version_directory,
    }))
}

fn version_entries(root: &Path) -> Vec<String> {
    let versions = root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY);
    if !versions.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = fs::read_dir(&versions)
        .expect("read versions dir")
        .map(|entry| {
            entry
                .expect("versions dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn assert_root_untouched(root: &Path) {
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_FILE).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_TEMP_FILE).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());
}

#[test]
fn a_first_install_adopts_a_verified_release_end_to_end() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("first-install");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);

    let installed = install(&root, &fixture, 1, &source).expect("first install succeeds");

    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let version_directory = expected_version_directory(1, &manifest_sha256);
    assert_eq!(installed.release_sequence(), 1);
    assert_eq!(installed.release_version(), "1.0.0");
    assert_eq!(installed.manifest_sha256(), manifest_sha256);
    assert_eq!(
        installed.version_directory(),
        root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
            .join(&version_directory)
            .as_path()
    );
    for role in ProductArtifactRole::ALL {
        let path = installed
            .artifact_path(role)
            .expect("installed artifact path");
        assert_eq!(
            fs::read(&path).expect("read installed artifact"),
            fixture.3[&role]
        );
    }
    assert_eq!(
        fs::read(
            installed
                .version_directory()
                .join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE)
        )
        .expect("read installed manifest copy"),
        fixture.1
    );
    assert_eq!(
        fs::read(
            installed
                .version_directory()
                .join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE)
        )
        .expect("read installed signature copy"),
        fixture.2
    );
    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state"),
        expected_state_bytes(1, "1.0.0", &manifest_sha256, &version_directory)
    );
    assert!(!root.join(CURL_PRODUCT_INSTALL_STATE_TEMP_FILE).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());

    let state = read_installed_curl_product_state(&root)
        .expect("state reads back")
        .expect("state is present after install");
    assert_eq!(state.release_sequence(), 1);
    assert_eq!(state.release_version(), "1.0.0");
    assert_eq!(state.manifest_sha256(), manifest_sha256);
    assert_eq!(state.version_directory(), version_directory);
}

#[test]
fn reading_state_without_an_install_returns_none() {
    let dir = FixtureDir::new("read-none");
    let missing_root = dir.path("missing-root");
    assert!(read_installed_curl_product_state(&missing_root)
        .expect("missing root is a clean first-install signal")
        .is_none());

    let empty_root = dir.path("empty-root");
    fs::create_dir_all(&empty_root).expect("create empty root");
    assert!(read_installed_curl_product_state(&empty_root)
        .expect("empty root is a clean first-install signal")
        .is_none());
}

#[test]
fn a_signed_successor_installs_over_the_active_release() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("successor");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    let first = install(&root, &fixture, 1, &source).expect("first install succeeds");
    let first_sha256 = manifest_sha256_hex(&fixture);

    let successor = signed_successor_fixture(&first_sha256);
    let upgraded = install(&root, &successor, 1, &source).expect("successor install succeeds");

    let successor_sha256 = manifest_sha256_hex(&successor);
    let successor_directory = expected_version_directory(2, &successor_sha256);
    assert_eq!(upgraded.release_sequence(), 2);
    assert_eq!(upgraded.release_version(), "1.0.1");
    assert_eq!(upgraded.manifest_sha256(), successor_sha256);
    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state"),
        expected_state_bytes(2, "1.0.1", &successor_sha256, &successor_directory)
    );
    for role in ProductArtifactRole::ALL {
        let path = upgraded
            .artifact_path(role)
            .expect("upgraded artifact path");
        assert_eq!(
            fs::read(&path).expect("read upgraded artifact"),
            successor.3[&role]
        );
    }
    // The previous version directory is retained as rollback material.
    assert!(first.version_directory().is_dir());
    assert_eq!(
        fs::read(
            first
                .version_directory()
                .join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE)
        )
        .expect("read retained previous manifest"),
        fixture.1
    );
    assert_eq!(version_entries(&root).len(), 2);
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());
}

#[test]
fn replaying_the_installed_sequence_is_rejected() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("replay");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    install(&root, &fixture, 1, &source).expect("first install succeeds");
    let state_before =
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state");

    let first_sha256 = manifest_sha256_hex(&fixture);
    let replay = {
        let previous = first_sha256.clone();
        resigned_manifest(move |value| {
            value["previousManifestSha256"] = json!(previous);
        })
    };
    let error = install(&root, &replay, 1, &source).expect_err("replay is rejected");
    match &error {
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::VersionChain(message)) => {
            assert!(
                message.contains("advance the sequence"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let rerun_error =
        install(&root, &fixture, 1, &source).expect_err("identical re-install is rejected");
    assert!(matches!(
        rerun_error,
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::VersionChain(_))
    ));

    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state"),
        state_before
    );
    assert_eq!(version_entries(&root).len(), 1);
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());
}

#[test]
fn a_successor_binding_a_different_predecessor_is_rejected() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("wrong-predecessor");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    install(&root, &fixture, 1, &source).expect("first install succeeds");
    let state_before =
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state");

    let forked = signed_successor_fixture(&"3".repeat(64));
    let error = install(&root, &forked, 1, &source).expect_err("forked successor is rejected");
    match &error {
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::VersionChain(message)) => {
            assert!(
                message.contains("exact active manifest"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read install state"),
        state_before
    );
    assert_eq!(version_entries(&root).len(), 1);
}

#[test]
fn a_first_install_below_the_pinned_minimum_is_rejected() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("below-minimum");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);

    let error = install(&root, &fixture, 8, &source).expect_err("below-minimum is rejected");
    match &error {
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::VersionChain(message)) => {
            assert!(
                message.contains("pinned first-install minimum"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
}

#[test]
fn a_tampered_host_artifact_aborts_without_touching_the_root() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("tampered-artifact");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    let target = source.join("host-cli.bin");
    let mut bytes = fs::read(&target).expect("read staged artifact");
    bytes[0] ^= 0xff;
    fs::write(&target, &bytes).expect("tamper staged artifact");

    let error = install(&root, &fixture, 1, &source).expect_err("tampered artifact is rejected");
    match &error {
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::ArtifactMismatch(
            message,
        )) => {
            assert!(
                message.contains("SHA-256 mismatch"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
}

#[test]
fn a_missing_artifact_file_is_rejected_as_a_source_error() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("missing-artifact");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::remove_file(source.join("host-node.bin")).expect("remove staged artifact");

    let error = install(&root, &fixture, 1, &source).expect_err("missing artifact is rejected");
    match &error {
        CurlProductInstallError::ArtifactSource(message) => {
            assert!(
                message.contains("host-node.bin") && message.contains("cannot be opened"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
    assert_root_untouched(&root);
}

#[test]
fn a_forged_signature_is_rejected() {
    let (trust_root, manifest, _, artifacts) = signed_product_fixture();
    let attacker = SigningKeyV2::from_dev_id("attacker-not-the-trust-root");
    let forged_signature = product_signature_for(&attacker, KAT_KEY_ID, &manifest);
    let dir = FixtureDir::new("forged-signature");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &artifacts);

    let error =
        install_curl_product_release(&root, &manifest, &forged_signature, &trust_root, 1, &source)
            .expect_err("forged signature is rejected");
    assert!(matches!(
        error,
        CurlProductInstallError::Verify(CurlProductReleaseVerifyError::UntrustedKey)
    ));
    assert_root_untouched(&root);
}

#[test]
fn a_corrupt_install_state_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("corrupt-state");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let junk = b"{not json".to_vec();
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &junk).expect("write junk state");

    let install_error = install(&root, &fixture, 1, &source).expect_err("corrupt state aborts");
    assert!(matches!(install_error, CurlProductInstallError::State(_)));
    let read_error =
        read_installed_curl_product_state(&root).expect_err("corrupt state fails the reader too");
    assert!(matches!(read_error, CurlProductInstallError::State(_)));

    // Fail-closed means the corrupt evidence is preserved, never repaired.
    assert_eq!(
        fs::read(root.join(CURL_PRODUCT_INSTALL_STATE_FILE)).expect("read junk state"),
        junk
    );
    assert!(!root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY).exists());
    assert!(!root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY).exists());
}

#[test]
fn a_noncanonical_install_state_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("noncanonical-state");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let mut state = expected_state_bytes(
        1,
        "1.0.0",
        &manifest_sha256,
        &expected_version_directory(1, &manifest_sha256),
    );
    state.push(b'\n');
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &state).expect("write state");

    let error = install(&root, &fixture, 1, &source).expect_err("non-canonical state aborts");
    match &error {
        CurlProductInstallError::State(message) => {
            assert!(
                message.contains("canonical"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn an_unknown_install_state_field_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("unknown-state-field");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let state = canonicalize(&json!({
        "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "manifestSha256": manifest_sha256,
        "versionDirectory": expected_version_directory(1, &manifest_sha256),
        "downloadUrl": "https://example.invalid/release",
    }));
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &state).expect("write state");

    let error = install(&root, &fixture, 1, &source).expect_err("unknown state field aborts");
    match &error {
        CurlProductInstallError::State(message) => {
            assert!(
                message.contains("unknown field"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn an_unexpected_install_state_schema_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("wrong-state-schema");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let state = canonicalize(&json!({
        "schema": "boole.some-other-state.v1",
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "manifestSha256": manifest_sha256,
        "versionDirectory": expected_version_directory(1, &manifest_sha256),
    }));
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &state).expect("write state");

    let error = install(&root, &fixture, 1, &source).expect_err("wrong state schema aborts");
    match &error {
        CurlProductInstallError::State(message) => {
            assert!(
                message.contains("unexpected install state schema"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn a_zero_sequence_install_state_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("zero-sequence-state");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let state = canonicalize(&json!({
        "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
        "releaseSequence": 0,
        "releaseVersion": "1.0.0",
        "manifestSha256": manifest_sha256,
        "versionDirectory": expected_version_directory(1, &manifest_sha256),
    }));
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &state).expect("write state");

    let error = install(&root, &fixture, 1, &source).expect_err("zero-sequence state aborts");
    match &error {
        CurlProductInstallError::State(message) => {
            assert!(
                message.contains("non-zero"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn a_malformed_install_state_digest_fails_closed() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("bad-state-digest");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    fs::create_dir_all(&root).expect("create install root");
    let manifest_sha256 = manifest_sha256_hex(&fixture).to_uppercase();
    let state = canonicalize(&json!({
        "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
        "releaseSequence": 1,
        "releaseVersion": "1.0.0",
        "manifestSha256": manifest_sha256,
        "versionDirectory": expected_version_directory(1, &manifest_sha256_hex(&fixture)),
    }));
    fs::write(root.join(CURL_PRODUCT_INSTALL_STATE_FILE), &state).expect("write state");

    let error = install(&root, &fixture, 1, &source).expect_err("malformed state digest aborts");
    match &error {
        CurlProductInstallError::State(message) => {
            assert!(
                message.contains("lowercase"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn an_orphan_version_directory_is_replaced_atomically() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("orphan-version-dir");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    let manifest_sha256 = manifest_sha256_hex(&fixture);
    let orphan = root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join(expected_version_directory(1, &manifest_sha256));
    fs::create_dir_all(&orphan).expect("create orphan version dir");
    fs::write(orphan.join("junk.bin"), b"crash residue").expect("write orphan junk");

    let installed = install(&root, &fixture, 1, &source).expect("install replaces the orphan");

    assert!(!orphan.join("junk.bin").exists());
    for role in ProductArtifactRole::ALL {
        let path = installed
            .artifact_path(role)
            .expect("installed artifact path");
        assert_eq!(
            fs::read(&path).expect("read installed artifact"),
            fixture.3[&role]
        );
    }
}

#[test]
fn leftover_staging_never_survives_a_successful_install() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("leftover-staging");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);
    let staging = root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY);
    fs::create_dir_all(&staging).expect("create leftover staging");
    fs::write(staging.join("leftover.bin"), b"crash residue").expect("write leftover");

    install(&root, &fixture, 1, &source).expect("install succeeds despite leftover staging");

    assert!(!staging.exists());
}

#[test]
fn installed_bytes_are_independent_of_the_source_directory() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("source-independence");
    let root = dir.path("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);

    let installed = install(&root, &fixture, 1, &source).expect("first install succeeds");

    for role in ProductArtifactRole::ALL {
        fs::write(
            source.join(format!("{}.bin", role.as_str())),
            b"tampered after install",
        )
        .expect("overwrite source artifact");
    }
    for role in ProductArtifactRole::ALL {
        let path = installed
            .artifact_path(role)
            .expect("installed artifact path");
        assert_eq!(
            fs::read(&path).expect("read installed artifact"),
            fixture.3[&role]
        );
    }
}

#[test]
fn the_install_root_is_created_when_missing() {
    let fixture = signed_product_fixture();
    let dir = FixtureDir::new("nested-root");
    let root = dir.path("nested").join("deeper").join("root");
    let source = dir.path("source");
    write_source_artifacts(&source, &fixture.3);

    let installed = install(&root, &fixture, 1, &source).expect("install creates the root");

    assert!(root.join(CURL_PRODUCT_INSTALL_STATE_FILE).is_file());
    assert!(installed.version_directory().is_dir());
}
