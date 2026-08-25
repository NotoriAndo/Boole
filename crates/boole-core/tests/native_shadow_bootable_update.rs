//! Successor contract for a bootable Linux/arm64 native-shadow guest.
//!
//! The frozen v1 contract carries an OCI-style `guest-rootfs`.  A Mac host
//! using `VZLinuxBootLoader` instead needs three separately pinned boot
//! inputs.  These tests keep v1 intact while specifying the bootable v2
//! successor.

use std::collections::BTreeMap;
use std::io::Cursor;

use boole_core::{
    authenticate_staged_bootable_native_shadow_update, authenticate_staged_native_shadow_update,
    canonicalize, GuestArtifactRole, GuestBootArtifactRole, NativeShadowUpdateFloor,
    NativeShadowUpdateTrustRoot, NativeShadowUpdateVerifyError, SigningKeyV2,
    GUEST_UPDATE_MANIFEST_SCHEMA_V2, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-bootable-kat-2026-08-26";

fn artifact_bytes() -> BTreeMap<GuestArtifactRole, Vec<u8>> {
    GuestArtifactRole::BOOTABLE_ALL
        .into_iter()
        .map(|role| {
            (
                role,
                format!("bootable-fixture:{}", role.as_str()).into_bytes(),
            )
        })
        .collect()
}

fn manifest_bytes(artifacts: &BTreeMap<GuestArtifactRole, Vec<u8>>) -> Vec<u8> {
    let descriptors: Vec<_> = GuestArtifactRole::BOOTABLE_ALL
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

fn signature_bytes(signing_key: &SigningKeyV2, manifest: &[u8]) -> Vec<u8> {
    signature_bytes_for_context(
        signing_key,
        manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
    )
}

fn signature_bytes_for_context(
    signing_key: &SigningKeyV2,
    manifest: &[u8],
    context: &str,
) -> Vec<u8> {
    let payload: serde_json::Value =
        serde_json::from_slice(manifest).expect("bootable manifest payload");
    let envelope = signing_key
        .sign_for_network(&payload, Some(context))
        .expect("bootable KAT signature");
    canonicalize(&json!({
        "schema": envelope.schema,
        "keyId": KAT_KEY_ID,
        "pk": envelope.pk,
        "signature": envelope.signature,
        "networkId": envelope.network_id,
        "manifestSha256": hex::encode(Sha256::digest(manifest)),
    }))
}

fn frozen_v1_manifest_bytes() -> Vec<u8> {
    let artifacts: Vec<_> = GuestArtifactRole::ALL
        .into_iter()
        .map(|role| {
            let bytes = format!("v1-fixture:{}", role.as_str()).into_bytes();
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
        "artifacts": artifacts,
    }))
}

#[test]
fn bootable_v2_authenticates_and_verifies_the_exact_twelve_artifacts() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let signature = signature_bytes(&signing_key, &manifest);

    let mut authenticated = authenticate_staged_bootable_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &floor,
    )
    .expect("bootable v2 authentication");
    for role in GuestArtifactRole::BOOTABLE_ALL {
        authenticated
            .verify_artifact(role, Cursor::new(&artifacts[&role]))
            .expect("bootable artifact verification");
    }
    let verified = authenticated.finish().expect("complete bootable set");

    assert_eq!(verified.manifest_schema(), GUEST_UPDATE_MANIFEST_SCHEMA_V2);
    assert_eq!(verified.boot_format_version(), Some(1));
    assert_eq!(GuestArtifactRole::BOOTABLE_ALL.len(), 12);
    assert!(!GuestArtifactRole::BOOTABLE_ALL.contains(&GuestArtifactRole::GuestRootfs));
    assert!(GuestArtifactRole::BOOTABLE_ALL.contains(&GuestArtifactRole::GuestKernel));
    assert!(GuestArtifactRole::BOOTABLE_ALL.contains(&GuestArtifactRole::GuestInitrd));
    assert!(GuestArtifactRole::BOOTABLE_ALL.contains(&GuestArtifactRole::GuestRootDisk));
}

#[test]
fn frozen_v1_manifest_cannot_be_reinterpreted_as_a_bootable_release() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let manifest = frozen_v1_manifest_bytes();
    let signature = signature_bytes(&signing_key, &manifest);

    assert!(matches!(
        authenticate_staged_bootable_native_shadow_update(
            &manifest,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason == "unexpected manifest schema"
    ));
}

#[test]
fn update_and_canary_share_one_frozen_boot_role_vocabulary() {
    let update_roles: Vec<_> = GuestArtifactRole::BOOTABLE_ALL[..3]
        .iter()
        .map(|role| role.as_str())
        .collect();
    let boot_roles: Vec<_> = GuestBootArtifactRole::ALL
        .iter()
        .map(|role| role.as_str())
        .collect();

    assert_eq!(update_roles, boot_roles);
}

#[test]
fn bootable_v2_rejects_the_legacy_guest_rootfs_role() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let mut value: serde_json::Value =
        serde_json::from_slice(&manifest).expect("bootable manifest JSON");
    value["artifacts"][0]["role"] = json!(GuestArtifactRole::GuestRootfs.as_str());
    let legacy_mixed = canonicalize(&value);
    let signature = signature_bytes(&signing_key, &legacy_mixed);

    assert!(matches!(
        authenticate_staged_bootable_native_shadow_update(
            &legacy_mixed,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::ArtifactSet(reason))
            if reason == "artifact descriptors must use the fixed role order"
    ));
}

#[test]
fn bootable_v2_counts_all_twelve_artifacts_against_the_guest_cap() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let mut value: serde_json::Value =
        serde_json::from_slice(&manifest).expect("bootable manifest JSON");
    value["artifacts"][0]["byteLength"] = json!(2_147_483_648_u64);
    let over_cap = canonicalize(&value);
    let signature = signature_bytes(&signing_key, &over_cap);

    assert!(matches!(
        authenticate_staged_bootable_native_shadow_update(
            &over_cap,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::ArtifactTooLarge)
    ));
}

#[test]
fn bootable_v2_rejects_a_signature_from_the_frozen_v1_domain() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let manifest = manifest_bytes(&artifact_bytes());
    let signature = signature_bytes_for_context(
        &signing_key,
        &manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );

    assert!(matches!(
        authenticate_staged_bootable_native_shadow_update(
            &manifest,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::InvalidSignatureContext)
    ));
}

#[test]
fn frozen_v1_authenticator_rejects_a_resigned_v2_manifest() {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("bootable KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(1).expect("first-install floor");
    let manifest = manifest_bytes(&artifact_bytes());
    let signature = signature_bytes_for_context(
        &signing_key,
        &manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    );

    assert!(matches!(
        authenticate_staged_native_shadow_update(&manifest, &signature, &trust_root, &floor),
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason == "unexpected manifest schema"
    ));
}
