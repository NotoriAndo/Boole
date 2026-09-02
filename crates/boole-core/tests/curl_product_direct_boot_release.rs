//! Product successor carrying the direct-root guest v3 contract.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use boole_core::{
    authenticate_direct_boot_curl_product_release, canonicalize,
    install_direct_boot_curl_product_release,
    open_verified_installed_direct_boot_curl_product_release, CurlProductReleaseFloor,
    CurlProductReleaseTrustRoot, GuestArtifactRole, NativeShadowUpdateTrustRoot,
    ProductArtifactRole, SigningKeyV2, CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3,
    CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3, GUEST_UPDATE_MANIFEST_SCHEMA_V3,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const PRODUCT_KEY_ID: &str = "non-production-product-v3-kat";
const GUEST_KEY_ID: &str = "non-production-guest-v3-kat";

type DirectBootProductFixture = (
    CurlProductReleaseTrustRoot,
    Vec<u8>,
    Vec<u8>,
    BTreeMap<ProductArtifactRole, Vec<u8>>,
    BTreeMap<GuestArtifactRole, Vec<u8>>,
);

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

fn fixture() -> DirectBootProductFixture {
    let guest_files: BTreeMap<_, _> = GuestArtifactRole::DIRECT_BOOT_ALL
        .into_iter()
        .map(|role| (role, format!("guest-v3:{}", role.as_str()).into_bytes()))
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
        "releaseSequence": 1,
        "releaseVersion": "3.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": guest_descriptors,
    }));
    let guest_key = SigningKeyV2::from_dev_id(GUEST_KEY_ID);
    let guest_signature = signature(
        &guest_key,
        GUEST_KEY_ID,
        &guest_manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
    );

    let mut artifacts: BTreeMap<_, _> = [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ]
    .into_iter()
    .map(|role| (role, format!("host-v3:{}", role.as_str()).into_bytes()))
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
        "schema": CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "3.0.0",
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
        "guestReleaseVersion": "3.0.0",
        "artifacts": descriptors,
    }));
    let product_key = SigningKeyV2::from_dev_id(PRODUCT_KEY_ID);
    let product_signature = signature(
        &product_key,
        PRODUCT_KEY_ID,
        &product_manifest,
        CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3,
    );
    let trust = CurlProductReleaseTrustRoot::new(PRODUCT_KEY_ID, &product_key.pk_hex())
        .expect("product KAT trust root");
    (
        trust,
        product_manifest,
        product_signature,
        artifacts,
        guest_files,
    )
}

#[test]
fn product_v3_authenticates_the_direct_root_guest_successor() {
    let fixture_root: PathBuf =
        std::env::temp_dir().join(format!("boole-direct-product-{}", std::process::id()));
    let _ = fs::remove_dir_all(&fixture_root);
    fs::create_dir(&fixture_root).expect("fixture root");
    let (trust, manifest, detached, artifacts, _) = fixture();
    for role in ProductArtifactRole::ALL {
        fs::write(
            fixture_root.join(format!("{}.bin", role.as_str())),
            &artifacts[&role],
        )
        .expect("write artifact");
    }
    let mut authenticated = authenticate_direct_boot_curl_product_release(
        &manifest,
        &detached,
        &trust,
        &CurlProductReleaseFloor::first_install(1).expect("first floor"),
    )
    .expect("product v3 authentication");
    for role in ProductArtifactRole::ALL {
        authenticated
            .verify_artifact(
                role,
                File::open(fixture_root.join(format!("{}.bin", role.as_str())))
                    .expect("open artifact"),
            )
            .expect("verify artifact");
    }
    let verified = authenticated.finish().expect("complete product v3");
    assert_eq!(
        verified.manifest_schema(),
        CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3
    );
    assert_eq!(
        verified.guest_manifest_schema(),
        GUEST_UPDATE_MANIFEST_SCHEMA_V3
    );
    fs::remove_dir_all(fixture_root).expect("remove fixture root");
}

#[test]
fn product_v3_installs_and_reopens_without_an_initrd() {
    let fixture_root = std::env::temp_dir().join(format!(
        "boole-direct-install-fixture-{}",
        std::process::id()
    ));
    let install_root =
        std::env::temp_dir().join(format!("boole-direct-install-root-{}", std::process::id()));
    let product_dir = fixture_root.join("product");
    let guest_dir = fixture_root.join("guest");
    let _ = fs::remove_dir_all(&fixture_root);
    let _ = fs::remove_dir_all(&install_root);
    fs::create_dir_all(&product_dir).expect("product fixture root");
    fs::create_dir(&guest_dir).expect("guest fixture root");
    let (product_trust, manifest, detached, products, guests) = fixture();
    for role in ProductArtifactRole::ALL {
        let path = product_dir.join(format!("{}.bin", role.as_str()));
        fs::write(&path, &products[&role]).expect("write product artifact");
        if matches!(
            role,
            ProductArtifactRole::HostCli
                | ProductArtifactRole::HostNode
                | ProductArtifactRole::HostWalletAgent
                | ProductArtifactRole::HostController
        ) {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555))
                .expect("make host fixture executable");
        }
    }
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        fs::write(
            guest_dir.join(format!("{}.bin", role.as_str())),
            &guests[&role],
        )
        .expect("write guest artifact");
    }
    let guest_key = SigningKeyV2::from_dev_id(GUEST_KEY_ID);
    let guest_trust = NativeShadowUpdateTrustRoot::new(GUEST_KEY_ID, &guest_key.pk_hex())
        .expect("guest trust root");

    let installed = install_direct_boot_curl_product_release(
        &install_root,
        &manifest,
        &detached,
        &product_trust,
        1,
        &product_dir,
        &guest_trust,
        1,
        &guest_dir,
    )
    .expect("install direct-boot product");
    assert!(installed
        .guest_artifact_path(GuestArtifactRole::GuestInitrd)
        .is_none());
    for role in [
        ProductArtifactRole::HostCli,
        ProductArtifactRole::HostNode,
        ProductArtifactRole::HostWalletAgent,
        ProductArtifactRole::HostController,
    ] {
        let mode = fs::metadata(installed.product().artifact_path(role).expect("host path"))
            .expect("installed host metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o555, "{} must remain executable", role.as_str());
    }
    let active = open_verified_installed_direct_boot_curl_product_release(
        &install_root,
        &product_trust,
        &guest_trust,
    )
    .expect("reopen direct-boot product");
    assert!(active
        .guest_artifact_file(GuestArtifactRole::GuestInitrd)
        .is_none());
    assert_eq!(active.guest().boot_format_version(), Some(2));

    fs::remove_dir_all(fixture_root).expect("remove fixture root");
    fs::remove_dir_all(install_root).expect("remove install root");
}
