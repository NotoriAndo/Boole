//! Bounty catalog file loading is node-owned runtime IO.
//!
//! Core owns the `Bounty` and `BountyList` data contracts. Node owns reading a
//! local catalog file at boot and validating the supported envelope version
//! before serving `/bounties`.

use boole_core::{Bounty, BountyList};
use boole_node::load_bounties_from_path;
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1.json")
        .canonicalize()
        .expect("bounty fixture path")
}

#[test]
fn node_bounty_catalog_store_loads_v1_fixture_with_two_entries() {
    let bounties = load_bounties_from_path(fixture_path()).expect("fixture loads");

    assert_eq!(bounties.len(), 2);
    let first: &Bounty = &bounties[0];
    assert_eq!(first.id, "alpha-1");
    assert_eq!(first.domain, "lean.protocol-invariant");
    assert_eq!(first.status, "open");
    assert_eq!(first.reward, "42");
    assert_eq!(
        first
            .verifier
            .metadata
            .get("verifierHash")
            .and_then(|v| v.as_str()),
        Some("cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd")
    );
    assert_eq!(bounties[1].id, "beta-1");
    assert_eq!(bounties[1].status, "solved");
}

#[test]
fn node_bounty_catalog_store_rejects_bad_version() {
    let dir = std::env::temp_dir().join(format!(
        "boole-bounty-store-bad-version-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("bad.json");
    std::fs::write(&path, br#"{"version":2,"bounties":[]}"#).expect("write bad fixture");

    let err = load_bounties_from_path(&path).expect_err("must reject version != 1");
    let msg = err.to_string();
    assert!(
        msg.contains("version") || msg.contains("2"),
        "error must mention version: {msg}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_bounty_catalog_store_accepts_empty_list() {
    let dir = std::env::temp_dir().join(format!(
        "boole-bounty-store-empty-{}-{}",
        std::process::id(),
        unique_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("empty.json");
    std::fs::write(&path, br#"{"version":1,"bounties":[]}"#).expect("write empty fixture");

    let bounties = load_bounties_from_path(&path).expect("empty list parses");

    assert!(bounties.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_bounty_catalog_store_returns_err_for_missing_file() {
    let path = std::env::temp_dir().join("boole-bounty-store-missing-xyz-1234567890.json");
    let _ = std::fs::remove_file(&path);

    assert!(load_bounties_from_path(&path).is_err());
}

#[test]
fn bounty_list_contract_still_decodes_empty_envelope() {
    let parsed: BountyList = serde_json::from_str(r#"{"version":1,"bounties":[]}"#).unwrap();
    assert_eq!(parsed.version, 1);
    assert!(parsed.bounties.is_empty());
}

fn unique_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos()
}
