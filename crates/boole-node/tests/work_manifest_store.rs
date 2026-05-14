//! Work manifest file loading is node-owned runtime IO.
//!
//! Core owns the `WorkManifest` and `WorkManifestList` data contracts. Node owns
//! reading a local catalog file at boot and validating the supported envelope
//! version before serving `/work`.

use boole_core::WorkManifest;
use boole_node::load_work_manifests_from_path;
use std::path::PathBuf;

#[test]
fn node_work_manifest_store_loads_v1_fixture_with_two_manifests() {
    let path = repo_root().join("fixtures/protocol/work/v1.json");
    let manifests = load_work_manifests_from_path(&path).expect("load v1 work manifests");

    assert_eq!(manifests.len(), 2, "fixture contains 2 manifests");
    let first: &WorkManifest = &manifests[0];
    assert_eq!(first.version, "1");
    assert_eq!(first.work_id, "lean-bounty-1");
    assert_eq!(first.source, "bounty");
    assert_eq!(first.family_id, "lean.protocol-invariant");
    assert_eq!(first.status, "open");
    assert!(first.retryable);

    let second: &WorkManifest = &manifests[1];
    assert_eq!(second.work_id, "smart-contract-invariant-v01-direct");
    assert_eq!(second.source, "direct");
    assert_eq!(second.family_id, "smart-contract-invariant-v01");
}

#[test]
fn node_work_manifest_store_rejects_bad_version() {
    let dir = std::env::temp_dir().join(format!(
        "boole-work-store-bad-version-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("bad.json");
    std::fs::write(&path, r#"{"version": 2, "work": []}"#).expect("write bad fixture");

    let err = load_work_manifests_from_path(&path).expect_err("non-1 version must error");
    let message = format!("{err:#}");
    assert!(
        message.contains("version") && message.contains("1"),
        "error must mention expected version: {message}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_work_manifest_store_accepts_empty_list() {
    let dir = std::env::temp_dir().join(format!(
        "boole-work-store-empty-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("empty.json");
    std::fs::write(&path, r#"{"version": 1, "work": []}"#).expect("write empty fixture");

    let manifests = load_work_manifests_from_path(&path).expect("empty list parses");

    assert!(manifests.is_empty(), "empty list returned as empty Vec");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_work_manifest_store_returns_err_for_missing_file() {
    let path = std::env::temp_dir().join("boole-work-store-missing-xyz-1234567890.json");
    let _ = std::fs::remove_file(&path);

    assert!(load_work_manifests_from_path(&path).is_err());
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos()
}
