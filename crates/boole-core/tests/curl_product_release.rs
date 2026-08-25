use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    authenticate_curl_product_release, canonicalize, CurlProductReleaseFloor,
    CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError, GuestArtifactRole,
    ProductArtifactRole, SigningKeyV2, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    MAX_CURL_PRODUCT_HOST_PAYLOAD_BYTES, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
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

fn first_install_floor() -> CurlProductReleaseFloor {
    CurlProductReleaseFloor::first_install(1).expect("KAT first-install floor")
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
            "boole-curl-product-release-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture dir");
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, bytes).expect("write fixture artifact");
        path
    }

    fn open(&self, name: &str) -> File {
        File::open(self.path(name)).expect("open fixture artifact")
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stage_all(dir: &FixtureDir, artifacts: &BTreeMap<ProductArtifactRole, Vec<u8>>) {
    for (role, bytes) in artifacts {
        dir.write(&format!("{}.bin", role.as_str()), bytes);
    }
}

#[test]
fn canonical_signed_product_release_verifies_end_to_end() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("success");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    assert_eq!(
        authenticated.artifact_file_name(ProductArtifactRole::HostCli),
        Some("host-cli.bin")
    );
    for role in ProductArtifactRole::ALL {
        authenticated
            .verify_artifact(role, dir.open(&format!("{}.bin", role.as_str())))
            .expect("artifact verification");
    }
    let verified = authenticated.finish().expect("complete artifact set");

    assert_eq!(verified.release_sequence(), 1);
    assert_eq!(verified.release_version(), "1.0.0");
    assert_eq!(verified.source_revision(), KAT_SOURCE_REVISION);
    assert_eq!(verified.guest_release_sequence(), 1);
    assert_eq!(verified.guest_release_version(), "1.0.0");
    assert_eq!(
        verified.manifest_sha256(),
        hex::encode(Sha256::digest(&manifest))
    );
    assert_eq!(
        verified.artifact_file_name(ProductArtifactRole::HostController),
        Some("host-controller.bin")
    );
    assert_eq!(
        verified.artifact_byte_length(ProductArtifactRole::HostCli),
        Some(artifacts[&ProductArtifactRole::HostCli].len() as u64)
    );
    assert_eq!(
        verified.artifact_sha256(ProductArtifactRole::HostCli),
        Some(hex::encode(Sha256::digest(
            &artifacts[&ProductArtifactRole::HostCli]
        )))
        .as_deref()
    );
    for role in ProductArtifactRole::ALL {
        assert!(verified.artifact_file(role).is_some());
    }
}

fn assert_missing_role_is_rejected(role_name: &str) {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        let descriptors = value["artifacts"].as_array_mut().expect("artifacts array");
        descriptors.retain(|descriptor| descriptor["role"] != role_name);
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::ArtifactSet(reason)) if reason.contains("expected 6")
    ));
}

#[test]
fn missing_host_node_descriptor_is_rejected() {
    assert_missing_role_is_rejected("host-node");
}

#[test]
fn missing_host_wallet_agent_descriptor_is_rejected() {
    assert_missing_role_is_rejected("host-wallet-agent");
}

#[test]
fn missing_host_controller_descriptor_is_rejected() {
    assert_missing_role_is_rejected("host-controller");
}

#[test]
fn wrong_target_os_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["targetOs"] = json!("linux");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::WrongTarget(reason))
            if reason.contains("macos/arm64")
    ));
}

#[test]
fn wrong_target_arch_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["targetArch"] = json!("x86_64");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::WrongTarget(reason))
            if reason.contains("macos/arm64")
    ));
}

#[test]
fn wrong_minimum_macos_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["minimumMacOs"] = json!("13.0");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::WrongTarget(reason)) if reason.contains("14.0")
    ));
}

#[test]
fn wrong_controller_protocol_version_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["controllerProtocolVersion"] = json!(2);
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::WrongTarget(reason))
            if reason.contains("controllerProtocolVersion")
    ));
}

#[test]
fn malformed_source_revision_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["sourceRevision"] = json!("C0D1E4F");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::Malformed(reason))
            if reason.contains("sourceRevision")
    ));
}

#[test]
fn product_guest_sequence_mismatch_is_rejected() {
    let (trust_root, manifest, signature, artifacts) = resigned_manifest(|value| {
        value["guestReleaseSequence"] = json!(9);
    });
    let dir = FixtureDir::new("guest-sequence");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(
            ProductArtifactRole::GuestUpdateManifest,
            dir.open("guest-update-manifest.bin"),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("releaseSequence")
    ));
}

#[test]
fn product_guest_version_mismatch_is_rejected() {
    let (trust_root, manifest, signature, artifacts) = resigned_manifest(|value| {
        value["guestReleaseVersion"] = json!("9.9.9");
    });
    let dir = FixtureDir::new("guest-version");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(
            ProductArtifactRole::GuestUpdateManifest,
            dir.open("guest-update-manifest.bin"),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("releaseVersion")
    ));
}

#[test]
fn guest_manifest_digest_field_mismatch_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["guestManifestSha256"] = json!("44".repeat(32));
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("guest-update-manifest")
    ));
}

#[test]
fn guest_signature_over_a_different_guest_manifest_is_rejected() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let mut artifacts = product_artifact_bytes();
    let guest_manifest = guest_manifest_bytes();
    let mut divergent: serde_json::Value =
        serde_json::from_slice(&guest_manifest).expect("guest manifest JSON");
    divergent["releaseVersion"] = json!("1.0.1");
    let divergent_bytes = canonicalize(&divergent);
    let mismatched_signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &divergent_bytes,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );
    artifacts.insert(
        ProductArtifactRole::GuestUpdateSignature,
        mismatched_signature,
    );
    let manifest = product_manifest_bytes(&artifacts);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let dir = FixtureDir::new("guest-signature-binding");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(
            ProductArtifactRole::GuestUpdateSignature,
            dir.open("guest-update-signature.bin"),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("guest-update-signature")
    ));
}

#[test]
fn noncanonical_embedded_guest_manifest_is_rejected() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let mut artifacts = product_artifact_bytes();
    let mut noncanonical = artifacts[&ProductArtifactRole::GuestUpdateManifest].clone();
    noncanonical.push(b'\n');
    artifacts.insert(ProductArtifactRole::GuestUpdateManifest, noncanonical);
    let manifest = product_manifest_bytes(&artifacts);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let dir = FixtureDir::new("guest-noncanonical");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(
            ProductArtifactRole::GuestUpdateManifest,
            dir.open("guest-update-manifest.bin"),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("canonical")
    ));
}

#[test]
fn embedded_guest_manifest_for_a_non_arm_guest_is_rejected() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let mut artifacts = product_artifact_bytes();
    let guest_manifest = guest_manifest_bytes();
    let mut wrong_target: serde_json::Value =
        serde_json::from_slice(&guest_manifest).expect("guest manifest JSON");
    wrong_target["targetArch"] = json!("x86_64");
    let wrong_target_bytes = canonicalize(&wrong_target);
    let wrong_target_signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &wrong_target_bytes,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );
    artifacts.insert(ProductArtifactRole::GuestUpdateManifest, wrong_target_bytes);
    artifacts.insert(
        ProductArtifactRole::GuestUpdateSignature,
        wrong_target_signature,
    );
    let manifest = product_manifest_bytes(&artifacts);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let dir = FixtureDir::new("guest-wrong-target");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(
            ProductArtifactRole::GuestUpdateManifest,
            dir.open("guest-update-manifest.bin"),
        ),
        Err(CurlProductReleaseVerifyError::GuestBinding(reason))
            if reason.contains("linux/aarch64")
    ));
}

#[test]
fn host_payload_above_the_frozen_cap_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["artifacts"][0]["byteLength"] = json!(MAX_CURL_PRODUCT_HOST_PAYLOAD_BYTES);
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::HostPayloadTooLarge)
    ));
}

#[test]
fn host_payload_exactly_at_the_frozen_cap_authenticates() {
    let artifacts = product_artifact_bytes();
    let other_host_bytes: u64 = [
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| artifacts[&role].len() as u64)
    .sum();
    let (trust_root, manifest, signature, _) = resigned_manifest(move |value| {
        value["artifacts"][0]["byteLength"] =
            json!(MAX_CURL_PRODUCT_HOST_PAYLOAD_BYTES - other_host_bytes);
    });

    authenticate_curl_product_release(&manifest, &signature, &trust_root, &first_install_floor())
        .expect("host payload exactly at the cap authenticates");
}

#[test]
fn duplicate_role_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["artifacts"][1]["role"] = json!("host-cli");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::ArtifactSet(reason))
            if reason.contains("fixed role order")
    ));
}

#[test]
fn duplicate_file_name_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["artifacts"][1]["fileName"] = json!("host-cli.bin");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::ArtifactSet(reason)) if reason.contains("unique")
    ));
}

#[test]
fn unsafe_artifact_file_names_are_rejected() {
    for unsafe_name in [
        "../host-cli.bin",
        "/usr/bin/host-cli",
        "host\u{0}cli",
        "",
        "..",
        ".hidden-host-cli",
    ] {
        let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
            value["artifacts"][0]["fileName"] = json!(unsafe_name);
        });

        let result = authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        );
        assert!(
            matches!(&result, Err(CurlProductReleaseVerifyError::Malformed(_))),
            "fileName {unsafe_name:?} must be rejected, got {result:?}"
        );
    }
}

#[test]
fn unknown_transport_url_field_is_rejected() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["downloadUrl"] = json!("https://example.invalid/not-a-trust-root");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::Malformed(reason))
            if reason.contains("unknown field")
    ));
}

#[test]
fn apple_team_identity_cannot_enter_the_authority_surface() {
    let (trust_root, manifest, signature, _) = resigned_manifest(|value| {
        value["teamId"] = json!("ABCDE12345");
    });

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::Malformed(reason))
            if reason.contains("unknown field")
    ));
}

#[test]
fn duplicate_json_keys_cannot_change_release_meaning() {
    let (trust_root, manifest, signature, _) = signed_product_fixture();
    let mut duplicated = manifest;
    assert_eq!(duplicated.pop(), Some(b'}'));
    duplicated.extend_from_slice(b",\"targetArch\":\"arm64\"}");

    assert!(matches!(
        authenticate_curl_product_release(
            &duplicated,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::NonCanonicalJson(name)) if name == "manifest"
    ));
}

#[test]
fn noncanonical_manifest_is_rejected() {
    let (trust_root, manifest, signature, _) = signed_product_fixture();
    let mut noncanonical = manifest;
    noncanonical.push(b'\n');

    assert!(matches!(
        authenticate_curl_product_release(
            &noncanonical,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::NonCanonicalJson(name)) if name == "manifest"
    ));
}

#[test]
fn signature_from_a_different_key_is_rejected() {
    let (trust_root, manifest, _, _) = signed_product_fixture();
    let attacker = SigningKeyV2::from_dev_id("different-non-production-kat");
    let forged_signature = product_signature_for(&attacker, KAT_KEY_ID, &manifest);

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &forged_signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::UntrustedKey)
    ));
}

#[test]
fn signature_from_the_guest_update_domain_is_rejected() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let manifest = product_manifest_bytes(&artifacts);
    let cross_domain_signature = envelope_for(
        &signing_key,
        KAT_KEY_ID,
        &manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &cross_domain_signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::InvalidSignatureContext)
    ));
}

#[test]
fn one_bit_signature_mutation_is_rejected() {
    let (trust_root, manifest, signature, _) = signed_product_fixture();
    let mut value: serde_json::Value = serde_json::from_slice(&signature).expect("signature JSON");
    let encoded = value["signature"].as_str().expect("signature hex");
    let replacement = if &encoded[..1] == "0" { "1" } else { "0" };
    value["signature"] = json!(format!("{replacement}{}", &encoded[1..]));
    let tampered_signature = canonicalize(&value);

    assert!(matches!(
        authenticate_curl_product_release(
            &manifest,
            &tampered_signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(CurlProductReleaseVerifyError::InvalidSignature)
    ));
}

#[test]
fn one_bit_host_artifact_mutation_is_rejected() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("tamper");
    let mut tampered = artifacts[&ProductArtifactRole::HostCli].clone();
    tampered[0] ^= 1;
    dir.write("host-cli.bin", &tampered);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(ProductArtifactRole::HostCli, dir.open("host-cli.bin")),
        Err(CurlProductReleaseVerifyError::ArtifactMismatch(reason))
            if reason.contains("SHA-256 mismatch")
    ));
}

#[test]
fn truncated_host_artifact_is_rejected() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("truncated");
    let mut truncated = artifacts[&ProductArtifactRole::HostNode].clone();
    truncated.pop();
    dir.write("host-node.bin", &truncated);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(ProductArtifactRole::HostNode, dir.open("host-node.bin")),
        Err(CurlProductReleaseVerifyError::ArtifactMismatch(reason))
            if reason.contains("byteLength mismatch")
    ));
}

#[test]
fn overlong_host_artifact_is_rejected() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("overlong");
    let mut overlong = artifacts[&ProductArtifactRole::HostNode].clone();
    overlong.push(b'x');
    dir.write("host-node.bin", &overlong);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");

    assert!(matches!(
        authenticated.verify_artifact(ProductArtifactRole::HostNode, dir.open("host-node.bin")),
        Err(CurlProductReleaseVerifyError::ArtifactMismatch(reason))
            if reason.contains("exceeds declared byteLength")
    ));
}

#[test]
fn an_installed_sequence_cannot_be_replayed() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let base = product_manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(7);
    value["previousManifestSha256"] = json!("11".repeat(32));
    let replay = canonicalize(&value);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &replay);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor = CurlProductReleaseFloor::installed(7, &"11".repeat(32)).expect("installed floor");

    assert!(matches!(
        authenticate_curl_product_release(&replay, &signature, &trust_root, &floor),
        Err(CurlProductReleaseVerifyError::VersionChain(reason))
            if reason.contains("advance the sequence")
    ));
}

#[test]
fn a_successor_cannot_bind_a_different_predecessor() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let active_manifest = product_manifest_bytes(&artifacts);
    let mut value: serde_json::Value =
        serde_json::from_slice(&active_manifest).expect("manifest JSON");
    value["releaseSequence"] = json!(2);
    value["releaseVersion"] = json!("1.0.1");
    value["previousManifestSha256"] = json!("33".repeat(32));
    let candidate = canonicalize(&value);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor =
        CurlProductReleaseFloor::installed(1, &hex::encode(Sha256::digest(&active_manifest)))
            .expect("installed floor");

    assert!(matches!(
        authenticate_curl_product_release(&candidate, &signature, &trust_root, &floor),
        Err(CurlProductReleaseVerifyError::VersionChain(reason))
            if reason.contains("exact active manifest")
    ));
}

#[test]
fn a_signed_successor_binding_the_active_manifest_authenticates() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = product_artifact_bytes();
    let active_manifest = product_manifest_bytes(&artifacts);
    let active_digest = hex::encode(Sha256::digest(&active_manifest));
    let mut value: serde_json::Value =
        serde_json::from_slice(&active_manifest).expect("manifest JSON");
    value["releaseSequence"] = json!(2);
    value["releaseVersion"] = json!("1.0.1");
    value["previousManifestSha256"] = json!(active_digest);
    let candidate = canonicalize(&value);
    let signature = product_signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = CurlProductReleaseTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor = CurlProductReleaseFloor::installed(1, &active_digest).expect("installed floor");

    authenticate_curl_product_release(&candidate, &signature, &trust_root, &floor)
        .expect("a signed direct successor authenticates");
}

#[test]
fn first_install_below_the_pinned_minimum_is_rejected() {
    let (trust_root, manifest, signature, _) = signed_product_fixture();
    let floor = CurlProductReleaseFloor::first_install(8).expect("pinned initial floor");

    assert!(matches!(
        authenticate_curl_product_release(&manifest, &signature, &trust_root, &floor),
        Err(CurlProductReleaseVerifyError::VersionChain(reason))
            if reason.contains("pinned first-install minimum")
    ));
}

#[test]
fn an_artifact_cannot_be_supplied_twice() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("double-supply");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    authenticated
        .verify_artifact(ProductArtifactRole::HostCli, dir.open("host-cli.bin"))
        .expect("first supply verifies");

    assert!(matches!(
        authenticated.verify_artifact(ProductArtifactRole::HostCli, dir.open("host-cli.bin")),
        Err(CurlProductReleaseVerifyError::ArtifactSet(reason))
            if reason.contains("more than once")
    ));
}

#[test]
fn finish_rejects_an_incomplete_product_set() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("incomplete");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    for role in ProductArtifactRole::ALL
        .into_iter()
        .filter(|role| *role != ProductArtifactRole::HostController)
    {
        authenticated
            .verify_artifact(role, dir.open(&format!("{}.bin", role.as_str())))
            .expect("artifact verification");
    }

    assert!(matches!(
        authenticated.finish(),
        Err(CurlProductReleaseVerifyError::ArtifactSet(reason))
            if reason.contains("host-controller")
    ));
}

#[test]
fn verified_artifact_handle_survives_path_replacement() {
    let (trust_root, manifest, signature, artifacts) = signed_product_fixture();
    let dir = FixtureDir::new("toctou");
    stage_all(&dir, &artifacts);

    let mut authenticated = authenticate_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    for role in ProductArtifactRole::ALL {
        authenticated
            .verify_artifact(role, dir.open(&format!("{}.bin", role.as_str())))
            .expect("artifact verification");
    }

    let replacement = dir.write("replacement.bin", b"post-verification-swap");
    fs::rename(&replacement, dir.path("host-cli.bin")).expect("swap the verified path");

    let verified = authenticated.finish().expect("complete artifact set");
    assert_eq!(
        fs::read(dir.path("host-cli.bin")).expect("read swapped path"),
        b"post-verification-swap"
    );

    let mut handle = verified
        .artifact_file(ProductArtifactRole::HostCli)
        .expect("retained verified handle");
    handle
        .seek(SeekFrom::Start(0))
        .expect("rewind retained handle");
    let mut observed = Vec::new();
    handle
        .read_to_end(&mut observed)
        .expect("read retained handle");
    assert_eq!(observed, artifacts[&ProductArtifactRole::HostCli]);
}
