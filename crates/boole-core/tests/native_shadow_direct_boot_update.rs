//! Direct-root boot successor that omits the unused initrd artifact.

use std::collections::BTreeMap;
use std::io::Cursor;

use boole_core::{
    authenticate_staged_direct_boot_native_shadow_update, canonicalize, GuestArtifactRole,
    NativeShadowUpdateFloor, NativeShadowUpdateTrustRoot, NativeShadowUpdateVerifyError,
    SigningKeyV2, GUEST_UPDATE_MANIFEST_SCHEMA_V3, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-direct-boot-kat-v1";

fn artifact_bytes() -> BTreeMap<GuestArtifactRole, Vec<u8>> {
    GuestArtifactRole::DIRECT_BOOT_ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("direct-boot-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect()
}

fn manifest_bytes(artifacts: &BTreeMap<GuestArtifactRole, Vec<u8>>) -> Vec<u8> {
    let descriptors: Vec<_> = GuestArtifactRole::DIRECT_BOOT_ALL
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
        "schema": GUEST_UPDATE_MANIFEST_SCHEMA_V3,
        "bootFormatVersion": 2,
        "channel": "stable",
        "releaseSequence": 1,
        "releaseVersion": "3.0.0",
        "targetOs": "linux",
        "targetArch": "aarch64",
        "previousManifestSha256": null,
        "artifacts": descriptors,
    }))
}

fn signature_bytes(signing_key: &SigningKeyV2, manifest: &[u8]) -> Vec<u8> {
    let payload: serde_json::Value = serde_json::from_slice(manifest).expect("manifest payload");
    let envelope = signing_key
        .sign_for_network(&payload, Some(NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3))
        .expect("direct-boot KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": KAT_KEY_ID,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(manifest)),
    }))
}

#[test]
fn direct_boot_v3_authenticates_exactly_eleven_artifacts_without_an_initrd() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("direct-boot KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let signature = signature_bytes(&signing_key, &manifest);

    let mut authenticated = authenticate_staged_direct_boot_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &floor,
    )
    .expect("direct-boot v3 authentication");
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        authenticated
            .verify_artifact(role, Cursor::new(&artifacts[&role]))
            .expect("direct-boot artifact verification");
    }
    let verified = authenticated.finish().expect("complete direct-boot set");

    assert_eq!(verified.manifest_schema(), GUEST_UPDATE_MANIFEST_SCHEMA_V3);
    assert_eq!(verified.boot_format_version(), Some(2));
    assert_eq!(GuestArtifactRole::DIRECT_BOOT_ALL.len(), 11);
    assert!(GuestArtifactRole::DIRECT_BOOT_ALL.contains(&GuestArtifactRole::GuestKernel));
    assert!(GuestArtifactRole::DIRECT_BOOT_ALL.contains(&GuestArtifactRole::GuestRootDisk));
    assert!(!GuestArtifactRole::DIRECT_BOOT_ALL.contains(&GuestArtifactRole::GuestInitrd));
}

#[test]
fn direct_boot_v3_rejects_an_initrd_descriptor() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("direct-boot KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&manifest).expect("manifest JSON");
    value["artifacts"][1]["role"] = json!(GuestArtifactRole::GuestInitrd.as_str());
    let malformed = canonicalize(&value);
    let signature = signature_bytes(&signing_key, &malformed);

    assert!(matches!(
        authenticate_staged_direct_boot_native_shadow_update(
            &malformed,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::ArtifactSet(reason))
            if reason == "artifact descriptors must use the fixed role order"
    ));
}
