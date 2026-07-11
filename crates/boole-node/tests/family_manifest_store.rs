//! D4 — family manifest directory loading is node-owned runtime IO.
//!
//! Core owns `FamilyManifestRegistry` and manifest parsing. Node owns walking a
//! local directory of JSON files at boot and applying the skip-and-warn policy.

use boole_node::{load_family_manifest_registry_from_dir, FamilyManifestStoreError};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn fresh_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "boole-d4-family-manifest-store-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn write_manifest(dir: &Path, name: &str, family_id: &str) {
    let v = json!({
        "version": "1",
        "familyId": family_id,
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "no_protocol_reward", "maxBlockRewardShareBps": 0 },
        "activationHeight": u64::MAX,
        "status": "experimental"
    });
    let path = dir.join(name);
    fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).expect("write manifest");
}

#[test]
fn node_family_manifest_store_reads_all_manifest_files() {
    let dir = fresh_dir("ok");
    write_manifest(&dir, "alpha.json", "alpha");
    write_manifest(&dir, "beta.json", "beta");

    let registry = load_family_manifest_registry_from_dir(&dir).expect("load");

    assert_eq!(registry.len(), 2);
    assert_eq!(
        registry.get("alpha").map(|m| m.family_id.as_str()),
        Some("alpha")
    );
    assert_eq!(
        registry.get("beta").map(|m| m.family_id.as_str()),
        Some("beta")
    );
}

#[test]
fn node_family_manifest_store_skips_invalid_json_and_non_json_files() {
    let dir = fresh_dir("skip");
    write_manifest(&dir, "alpha.json", "alpha");
    fs::write(dir.join("garbage.json"), b"{not json").expect("write garbage");
    fs::write(dir.join("README.md"), b"# notes").expect("write readme");

    let registry = load_family_manifest_registry_from_dir(&dir).expect("load");

    assert_eq!(registry.len(), 1);
    assert!(registry.get("alpha").is_some());
}

#[test]
fn node_family_manifest_store_returns_err_for_missing_dir() {
    let dir = std::env::temp_dir().join("boole-d4-family-manifest-missing-xyz-1234567890");
    let _ = fs::remove_dir_all(&dir);

    assert!(load_family_manifest_registry_from_dir(&dir).is_err());
}

// SC.6: duplicate `family_id` across files is a hard error (ADR-0015 (c) —
// same policy as the family-root computation; no silent last-write-wins).
#[test]
fn manifest_store_rejects_duplicate_family_id() {
    let dir = fresh_dir("duplicate");
    write_manifest(&dir, "first.json", "alpha");
    write_manifest(&dir, "second.json", "alpha");

    let err = load_family_manifest_registry_from_dir(&dir)
        .expect_err("duplicate family_id must be a hard error");

    match err {
        FamilyManifestStoreError::DuplicateFamilyId { family_id, path } => {
            assert_eq!(family_id, "alpha");
            // Files load in sorted path order, so the duplicate is detected
            // at the lexicographically later file.
            assert_eq!(
                path.file_name().and_then(|s| s.to_str()),
                Some("second.json")
            );
        }
        other => panic!("expected DuplicateFamilyId, got: {other:?}"),
    }
}
