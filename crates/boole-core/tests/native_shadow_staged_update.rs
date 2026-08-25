use std::collections::BTreeMap;
use std::io::Cursor;

use boole_core::{
    authenticate_staged_native_shadow_update, canonicalize, GuestArtifactRole,
    NativeShadowUpdateFloor, NativeShadowUpdateTrustRoot, NativeShadowUpdateVerifyError,
    SigningKeyV2, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
};
use ed25519_dalek::VerifyingKey;
use serde_json::json;
use sha2::{Digest, Sha256};

const KAT_KEY_ID: &str = "non-production-kat-2026-08-25";

fn artifact_bytes() -> BTreeMap<GuestArtifactRole, Vec<u8>> {
    GuestArtifactRole::ALL
        .into_iter()
        .map(|role| (role, format!("fixture:{}", role.as_str()).into_bytes()))
        .collect()
}

fn manifest_bytes(artifacts: &BTreeMap<GuestArtifactRole, Vec<u8>>) -> Vec<u8> {
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

fn signed_fixture() -> (NativeShadowUpdateTrustRoot, Vec<u8>, Vec<u8>) {
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let detached_signature = signature_for(&signing_key, KAT_KEY_ID, &manifest);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    (trust_root, manifest, detached_signature)
}

fn first_install_floor() -> NativeShadowUpdateFloor {
    NativeShadowUpdateFloor::first_install(1).expect("KAT first-install floor")
}

fn signature_for(signing_key: &SigningKeyV2, key_id: &str, manifest: &[u8]) -> Vec<u8> {
    signature_for_context(
        signing_key,
        key_id,
        manifest,
        NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    )
}

fn signature_for_context(
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

#[test]
fn noncanonical_manifest_is_rejected_before_signature_or_artifact_use() {
    let (trust_root, manifest, signature) = signed_fixture();
    let mut noncanonical = manifest;
    noncanonical.push(b'\n');

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &noncanonical,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::NonCanonicalJson(name)) if name == "manifest"
    ));
}

#[test]
fn one_bit_artifact_mutation_is_rejected() {
    let artifacts = artifact_bytes();
    let (trust_root, manifest, signature) = signed_fixture();
    let mut authenticated = authenticate_staged_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    let mut tampered = artifacts[&GuestArtifactRole::GuestRootfs].clone();
    tampered[0] ^= 1;

    assert!(matches!(
        authenticated.verify_artifact(GuestArtifactRole::GuestRootfs, Cursor::new(tampered)),
        Err(NativeShadowUpdateVerifyError::ArtifactMismatch(reason))
            if reason.contains("SHA-256 mismatch")
    ));
}

#[test]
fn signature_from_a_different_key_is_rejected() {
    let (trust_root, manifest, _) = signed_fixture();
    let attacker = SigningKeyV2::from_dev_id("different-non-production-kat");
    let forged_signature = signature_for(&attacker, KAT_KEY_ID, &manifest);

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &manifest,
            &forged_signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::UntrustedKey)
    ));
}

#[test]
fn finish_rejects_an_incomplete_runtime_authority_set() {
    let artifacts = artifact_bytes();
    let (trust_root, manifest, signature) = signed_fixture();
    let mut authenticated = authenticate_staged_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    for role in GuestArtifactRole::ALL
        .into_iter()
        .filter(|role| *role != GuestArtifactRole::ToolchainIdentity)
    {
        authenticated
            .verify_artifact(role, Cursor::new(&artifacts[&role]))
            .expect("artifact verification");
    }

    assert!(matches!(
        authenticated.finish(),
        Err(NativeShadowUpdateVerifyError::ArtifactSet(reason))
            if reason.contains("toolchain-identity")
    ));
}

#[test]
fn an_installed_sequence_cannot_be_replayed_as_an_update() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(7);
    value["previousManifestSha256"] = json!("11".repeat(32));
    let replay = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &replay);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor = NativeShadowUpdateFloor::installed(7, &"11".repeat(32)).expect("floor");

    assert!(matches!(
        authenticate_staged_native_shadow_update(&replay, &signature, &trust_root, &floor),
        Err(NativeShadowUpdateVerifyError::VersionChain(reason))
            if reason.contains("exact active manifest")
    ));
}

#[test]
fn next_sequence_must_bind_the_exact_active_manifest() {
    let artifacts = artifact_bytes();
    let active_manifest = manifest_bytes(&artifacts);
    let active_digest = hex::encode(Sha256::digest(&active_manifest));
    let mut value: serde_json::Value =
        serde_json::from_slice(&active_manifest).expect("manifest JSON");
    value["releaseSequence"] = json!(2);
    value["releaseVersion"] = json!("1.0.1");
    value["previousManifestSha256"] = json!(active_digest);
    let candidate = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor =
        NativeShadowUpdateFloor::installed(1, &hex::encode(Sha256::digest(&active_manifest)))
            .expect("floor");

    let mut authenticated =
        authenticate_staged_native_shadow_update(&candidate, &signature, &trust_root, &floor)
            .expect("bound successor");
    for role in GuestArtifactRole::ALL {
        authenticated
            .verify_artifact(role, Cursor::new(&artifacts[&role]))
            .expect("artifact verification");
    }
    assert_eq!(
        authenticated.finish().expect("complete").release_sequence(),
        2
    );
}

#[test]
fn a_fresh_install_can_start_from_the_latest_nonzero_signed_release() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(8);
    value["releaseVersion"] = json!("1.7.0");
    let latest = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &latest);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    let authenticated = authenticate_staged_native_shadow_update(
        &latest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("fresh install accepts the latest signed release");
    assert_eq!(
        authenticated.artifact_byte_length(GuestArtifactRole::GuestRootfs),
        Some(20)
    );
}

#[test]
fn a_fresh_install_rejects_a_signed_release_below_the_pinned_minimum() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(7);
    value["releaseVersion"] = json!("1.6.0");
    let old_but_signed = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &old_but_signed);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor = NativeShadowUpdateFloor::first_install(8).expect("pinned initial floor");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &old_but_signed,
            &signature,
            &trust_root,
            &floor,
        ),
        Err(NativeShadowUpdateVerifyError::VersionChain(reason))
            if reason.contains("pinned first-install minimum")
    ));
}

#[test]
fn an_update_can_skip_sequences_only_when_it_binds_the_exact_active_manifest() {
    let artifacts = artifact_bytes();
    let active_manifest = manifest_bytes(&artifacts);
    let active_digest = hex::encode(Sha256::digest(&active_manifest));
    let mut value: serde_json::Value =
        serde_json::from_slice(&active_manifest).expect("manifest JSON");
    value["releaseSequence"] = json!(8);
    value["releaseVersion"] = json!("1.7.0");
    value["previousManifestSha256"] = json!(active_digest);
    let candidate = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor =
        NativeShadowUpdateFloor::installed(1, &hex::encode(Sha256::digest(&active_manifest)))
            .expect("floor");

    authenticate_staged_native_shadow_update(&candidate, &signature, &trust_root, &floor)
        .expect("a signed direct successor may skip unused sequence numbers");
}

#[test]
fn a_higher_sequence_cannot_bind_a_different_predecessor() {
    let artifacts = artifact_bytes();
    let active_manifest = manifest_bytes(&artifacts);
    let mut value: serde_json::Value =
        serde_json::from_slice(&active_manifest).expect("manifest JSON");
    value["releaseSequence"] = json!(8);
    value["releaseVersion"] = json!("1.7.0");
    value["previousManifestSha256"] = json!("33".repeat(32));
    let candidate = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor =
        NativeShadowUpdateFloor::installed(1, &hex::encode(Sha256::digest(&active_manifest)))
            .expect("floor");

    assert!(matches!(
        authenticate_staged_native_shadow_update(&candidate, &signature, &trust_root, &floor),
        Err(NativeShadowUpdateVerifyError::VersionChain(reason))
            if reason.contains("exact active manifest")
    ));
}

#[test]
fn release_sequence_zero_is_never_installable() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(0);
    let candidate = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &candidate,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::VersionChain(reason))
            if reason.contains("non-zero")
    ));
}

#[test]
fn a_signed_non_arm_guest_is_still_rejected() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["targetArch"] = json!("x86_64");
    let wrong_target = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &wrong_target);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &wrong_target,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::WrongTarget(reason))
            if reason.contains("linux/aarch64")
    ));
}

#[test]
fn declared_download_larger_than_the_frozen_cap_is_rejected() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["artifacts"][0]["byteLength"] = json!(2_147_483_649_u64);
    let oversized = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &oversized);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &oversized,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::ArtifactTooLarge)
    ));
}

#[test]
fn one_bit_signature_mutation_is_rejected() {
    let (trust_root, manifest, signature) = signed_fixture();
    let mut value: serde_json::Value = serde_json::from_slice(&signature).expect("signature JSON");
    let encoded = value["signature"].as_str().expect("signature hex");
    let replacement = if &encoded[..1] == "0" { "1" } else { "0" };
    value["signature"] = json!(format!("{replacement}{}", &encoded[1..]));
    let tampered_signature = canonicalize(&value);

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &manifest,
            &tampered_signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::InvalidSignature)
    ));
}

#[test]
fn signed_manifest_with_an_unknown_field_is_rejected() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["downloadUrl"] = json!("https://example.invalid/not-a-trust-root");
    let extended = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &extended);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &extended,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason.contains("unknown field")
    ));
}

#[test]
fn duplicate_json_keys_cannot_change_manifest_meaning() {
    let (trust_root, manifest, signature) = signed_fixture();
    let mut duplicated = manifest;
    assert_eq!(duplicated.pop(), Some(b'}'));
    duplicated.extend_from_slice(b",\"targetArch\":\"aarch64\"}");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &duplicated,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::NonCanonicalJson(name)) if name == "manifest"
    ));
}

#[test]
fn signature_from_another_protocol_domain_is_rejected() {
    let artifacts = artifact_bytes();
    let manifest = manifest_bytes(&artifacts);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for_context(&signing_key, KAT_KEY_ID, &manifest, "boole-testnet");
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    assert!(matches!(
        authenticate_staged_native_shadow_update(
            &manifest,
            &signature,
            &trust_root,
            &first_install_floor(),
        ),
        Err(NativeShadowUpdateVerifyError::InvalidSignatureContext)
    ));
}

#[test]
fn exhausted_sequence_space_fails_closed_instead_of_replaying_u64_max() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    value["releaseSequence"] = json!(u64::MAX);
    value["previousManifestSha256"] = json!("22".repeat(32));
    let candidate = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &candidate);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");
    let floor = NativeShadowUpdateFloor::installed(u64::MAX, &"22".repeat(32)).expect("max floor");

    assert!(matches!(
        authenticate_staged_native_shadow_update(&candidate, &signature, &trust_root, &floor),
        Err(NativeShadowUpdateVerifyError::VersionChain(reason))
            if reason.contains("sequence space exhausted")
    ));
}

#[test]
fn truncated_artifact_is_rejected_even_when_its_prefix_hashes_normally() {
    let artifacts = artifact_bytes();
    let (trust_root, manifest, signature) = signed_fixture();
    let mut authenticated = authenticate_staged_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    let mut truncated = artifacts[&GuestArtifactRole::Registry].clone();
    truncated.pop();

    assert!(matches!(
        authenticated.verify_artifact(GuestArtifactRole::Registry, Cursor::new(truncated)),
        Err(NativeShadowUpdateVerifyError::ArtifactMismatch(reason))
            if reason.contains("byteLength mismatch")
    ));
}

#[test]
fn malformed_ed25519_trust_root_is_rejected_at_configuration_time() {
    let malformed_point = (0_u64..)
        .find_map(|counter| {
            let bytes: [u8; 32] = Sha256::digest(counter.to_le_bytes()).into();
            VerifyingKey::from_bytes(&bytes).is_err().then_some(bytes)
        })
        .expect("deterministically find a malformed compressed point");
    assert!(matches!(
        NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &hex::encode(malformed_point)),
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason.contains("Ed25519 point")
    ));
}

#[test]
fn weak_ed25519_trust_root_is_rejected_at_configuration_time() {
    let weak_key = hex::encode([0_u8; 32]);
    let parsed = VerifyingKey::from_bytes(&[0_u8; 32]).expect("compressed weak point");
    assert!(parsed.is_weak(), "fixture must remain a weak Ed25519 point");

    assert!(matches!(
        NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &weak_key),
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason.contains("weak")
    ));
}

#[test]
fn genesis_manifest_must_explicitly_include_the_previous_field() {
    let artifacts = artifact_bytes();
    let base = manifest_bytes(&artifacts);
    let mut value: serde_json::Value = serde_json::from_slice(&base).expect("manifest JSON");
    assert!(value
        .as_object_mut()
        .expect("manifest object")
        .remove("previousManifestSha256")
        .is_some());
    let missing_previous = canonicalize(&value);
    let signing_key = SigningKeyV2::from_dev_id(KAT_KEY_ID);
    let signature = signature_for(&signing_key, KAT_KEY_ID, &missing_previous);
    let trust_root = NativeShadowUpdateTrustRoot::new(KAT_KEY_ID, &signing_key.pk_hex())
        .expect("KAT trust root");

    let result = authenticate_staged_native_shadow_update(
        &missing_previous,
        &signature,
        &trust_root,
        &first_install_floor(),
    );
    assert!(
        matches!(
            &result,
        Err(NativeShadowUpdateVerifyError::Malformed(reason))
            if reason.contains("previousManifestSha256")
        ),
        "unexpected result: {result:?}"
    );
}

#[test]
fn canonical_signed_manifest_authenticates_exact_guest_without_adopting_it() {
    assert!(GuestArtifactRole::ALL.contains(&GuestArtifactRole::RootfsContentManifest));
    let artifacts = artifact_bytes();
    let (trust_root, manifest, signature) = signed_fixture();

    let mut authenticated = authenticate_staged_native_shadow_update(
        &manifest,
        &signature,
        &trust_root,
        &first_install_floor(),
    )
    .expect("manifest authentication");
    assert_eq!(
        authenticated.artifact_file_name(GuestArtifactRole::GuestRootfs),
        Some("guest-rootfs.bin")
    );
    for role in GuestArtifactRole::ALL {
        authenticated
            .verify_artifact(role, Cursor::new(&artifacts[&role]))
            .expect("artifact verification");
    }
    let verified = authenticated.finish().expect("complete artifact set");

    assert_eq!(verified.release_sequence(), 1);
    assert_eq!(verified.release_version(), "1.0.0");
    assert_eq!(verified.target_arch(), "aarch64");
    assert_eq!(
        verified.artifact_file_name(GuestArtifactRole::GuestRootfs),
        Some("guest-rootfs.bin")
    );
    assert_eq!(
        verified.artifact_byte_length(GuestArtifactRole::GuestRootfs),
        Some(artifacts[&GuestArtifactRole::GuestRootfs].len() as u64)
    );
    assert_eq!(
        verified.artifact_sha256(GuestArtifactRole::GuestRootfs),
        Some(hex::encode(Sha256::digest(
            &artifacts[&GuestArtifactRole::GuestRootfs]
        )))
        .as_deref()
    );
}
