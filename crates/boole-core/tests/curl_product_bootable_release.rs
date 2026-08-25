//! Product-release successor that can carry a bootable guest-update v2.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{
    authenticate_bootable_curl_product_release, canonicalize, install_curl_product_release,
    CurlProductInstallError, CurlProductReleaseFloor, CurlProductReleaseTrustRoot,
    CurlProductReleaseVerifyError, GuestArtifactRole, ProductArtifactRole, SigningKeyV2,
    CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2, CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2, GUEST_UPDATE_MANIFEST_SCHEMA_V2,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const PRODUCT_KEY_ID: &str = "non-production-product-v2-kat-2026-08-26";
const GUEST_KEY_ID: &str = "non-production-guest-v2-kat-2026-08-26";
static NONCE: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "boole-curl-product-v2-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create v2 product fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn signature_bytes(
    signing_key: &SigningKeyV2,
    key_id: &str,
    payload_bytes: &[u8],
    context: &str,
) -> Vec<u8> {
    let payload: serde_json::Value =
        serde_json::from_slice(payload_bytes).expect("signed payload JSON");
    let envelope = signing_key
        .sign_for_network(&payload, Some(context))
        .expect("KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": key_id,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(payload_bytes)),
    }))
}

fn guest_manifest_bytes() -> Vec<u8> {
    let descriptors: Vec<_> = GuestArtifactRole::BOOTABLE_ALL
        .into_iter()
        .map(|role| {
            let bytes = format!("bootable-fixture:{}", role.as_str()).into_bytes();
            json!({
                "role": role.as_str(),
                "fileName": format!("{}.bin", role.as_str()),
                "byteLength": bytes.len(),
                "sha256": hex::encode(Sha256::digest(bytes)),
            })
        })
        .collect();
    canonicalize(&json!({
        "schema": GUEST_UPDATE_MANIFEST_SCHEMA_V2,
        "bootFormatVersion": 1,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "2.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": descriptors,
    }))
}

fn product_fixture_with_guest_manifest(
    guest_manifest: Vec<u8>,
) -> (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
) {
    product_fixture_with_guest_manifest_and_context(
        guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
    )
}

fn product_fixture_with_guest_manifest_and_context(
    guest_manifest: Vec<u8>,
    guest_signing_context: &str,
) -> (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
) {
    let guest_signing_key = SigningKeyV2::from_dev_id(GUEST_KEY_ID);
    let guest_signature = signature_bytes(
        &guest_signing_key,
        GUEST_KEY_ID,
        &guest_manifest,
        guest_signing_context,
    );
    let mut artifacts: BTreeMap<_, _> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| {
        (
            role,
            format!("host-v2-fixture:{}", role.as_str()).into_bytes(),
        )
    })
    .collect();
    artifacts.insert(ProductArtifactRole::GuestUpdateManifest, guest_manifest);
    artifacts.insert(ProductArtifactRole::GuestUpdateSignature, guest_signature);

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
    let product_manifest = canonicalize(&json!({
        "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "2.0.0",
        "sourceRevision": "11".repeat(20),
        "targetOs": "macos",
        "targetArch": "arm64",
        "minimumMacOs": "14.0",
        "previousManifestSha256": null,
        "controllerProtocolVersion": 1,
        "guestManifestSha256": hex::encode(Sha256::digest(
            &artifacts[&ProductArtifactRole::GuestUpdateManifest],
        )),
        "guestReleaseSequence": 1,
        "guestReleaseVersion": "2.0.0",
        "artifacts": descriptors,
    }));
    let product_signing_key = SigningKeyV2::from_dev_id(PRODUCT_KEY_ID);
    let product_signature = signature_bytes(
        &product_signing_key,
        PRODUCT_KEY_ID,
        &product_manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2,
    );
    let trust_root =
        CurlProductReleaseTrustRoot::new(PRODUCT_KEY_ID, &product_signing_key.pk_hex())
            .expect("product v2 KAT trust root");
    (trust_root, product_manifest, product_signature, artifacts)
}

fn product_fixture() -> (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
) {
    product_fixture_with_guest_manifest(guest_manifest_bytes())
}

#[test]
fn product_v2_authenticates_the_exact_bootable_guest_successor() {
    let dir = TestDir::new();
    let (trust_root, manifest, signature, artifacts) = product_fixture();
    for role in ProductArtifactRole::ALL {
        fs::write(
            dir.path().join(format!("{}.bin", role.as_str())),
            &artifacts[&role],
        )
        .expect("write product v2 artifact");
    }

    let mut authenticated = authenticate_bootable_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &CurlProductReleaseFloor::first_install(1).expect("first-install floor"),
    )
    .expect("bootable product v2 authentication");
    for role in ProductArtifactRole::ALL {
        authenticated
            .verify_artifact(
                role,
                File::open(dir.path().join(format!("{}.bin", role.as_str())))
                    .expect("open product v2 artifact"),
            )
            .expect("verify product v2 artifact");
    }
    let verified = authenticated.finish().expect("complete product v2");

    assert_eq!(
        verified.manifest_schema(),
        CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2
    );
    assert_eq!(
        verified.guest_manifest_schema(),
        GUEST_UPDATE_MANIFEST_SCHEMA_V2
    );
}

#[test]
fn product_v2_rejects_a_guest_v2_manifest_without_the_exact_boot_roles() {
    let dir = TestDir::new();
    let guest_manifest = guest_manifest_bytes();
    let mut value: serde_json::Value =
        serde_json::from_slice(&guest_manifest).expect("guest v2 manifest JSON");
    value["artifacts"][0]["role"] = json!(GuestArtifactRole::GuestRootfs.as_str());
    let malformed_guest_manifest = canonicalize(&value);
    let (trust_root, manifest, signature, artifacts) =
        product_fixture_with_guest_manifest(malformed_guest_manifest);
    for role in ProductArtifactRole::ALL {
        fs::write(
            dir.path().join(format!("{}.bin", role.as_str())),
            &artifacts[&role],
        )
        .expect("write malformed product v2 artifact");
    }

    let mut authenticated = authenticate_bootable_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &CurlProductReleaseFloor::first_install(1).expect("first-install floor"),
    )
    .expect("product envelope remains valid");
    for role in ProductArtifactRole::ALL {
        let result = authenticated.verify_artifact(
            role,
            File::open(dir.path().join(format!("{}.bin", role.as_str())))
                .expect("open malformed product v2 artifact"),
        );
        if role == ProductArtifactRole::GuestUpdateManifest {
            assert!(
                result.is_err(),
                "unbootable guest manifest must be rejected"
            );
            return;
        }
        result.expect("preceding product artifact remains valid");
    }
    panic!("guest-update-manifest was not visited");
}

#[test]
fn product_v2_rejects_a_signature_from_the_frozen_product_v1_domain() {
    let (trust_root, manifest, _signature, _artifacts) = product_fixture();
    let product_signing_key = SigningKeyV2::from_dev_id(PRODUCT_KEY_ID);
    let wrong_domain_signature = signature_bytes(
        &product_signing_key,
        PRODUCT_KEY_ID,
        &manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
    );

    assert!(matches!(
        authenticate_bootable_curl_product_release(
            &manifest,
            &wrong_domain_signature,
            &trust_root,
            &CurlProductReleaseFloor::first_install(1).expect("first-install floor"),
        ),
        Err(CurlProductReleaseVerifyError::InvalidSignatureContext)
    ));
}

#[test]
fn product_v2_rejects_an_embedded_guest_signature_from_the_v1_domain() {
    let dir = TestDir::new();
    let (trust_root, manifest, signature, artifacts) =
        product_fixture_with_guest_manifest_and_context(
            guest_manifest_bytes(),
            NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
        );
    for role in ProductArtifactRole::ALL {
        fs::write(
            dir.path().join(format!("{}.bin", role.as_str())),
            &artifacts[&role],
        )
        .expect("write wrong-domain product v2 artifact");
    }

    let mut authenticated = authenticate_bootable_curl_product_release(
        &manifest,
        &signature,
        &trust_root,
        &CurlProductReleaseFloor::first_install(1).expect("first-install floor"),
    )
    .expect("product v2 envelope remains valid");
    for role in ProductArtifactRole::ALL {
        let result = authenticated.verify_artifact(
            role,
            File::open(dir.path().join(format!("{}.bin", role.as_str())))
                .expect("open wrong-domain product v2 artifact"),
        );
        if role == ProductArtifactRole::GuestUpdateSignature {
            assert!(matches!(
                result,
                Err(CurlProductReleaseVerifyError::GuestBinding(reason))
                    if reason == "guest-update-signature is not in the guest-update signing domain"
            ));
            return;
        }
        result.expect("preceding product artifact remains valid");
    }
    panic!("guest-update-signature was not visited");
}

#[test]
fn frozen_v1_installer_rejects_v2_without_mutating_the_install_root() {
    let artifact_dir = TestDir::new();
    let install_parent = TestDir::new();
    let install_root = install_parent.path().join("not-created");
    let (trust_root, manifest, signature, artifacts) = product_fixture();
    for role in ProductArtifactRole::ALL {
        fs::write(
            artifact_dir.path().join(format!("{}.bin", role.as_str())),
            &artifacts[&role],
        )
        .expect("write product v2 artifact for frozen installer");
    }

    assert!(matches!(
        install_curl_product_release(
            &install_root,
            &manifest,
            &signature,
            &trust_root,
            1,
            artifact_dir.path(),
        ),
        Err(CurlProductInstallError::Verify(
            CurlProductReleaseVerifyError::InvalidSignatureContext
        ))
    ));
    assert!(
        !install_root.exists(),
        "v1 installer must leave no trace when given a v2 contract"
    );
}
