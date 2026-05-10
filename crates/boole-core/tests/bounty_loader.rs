//! S11 — `BountyList` envelope + `load_bounties` loader.
//!
//! Mirrors the S10 work-manifest loader: read JSON, validate
//! `version == 1`, return the inner `Vec<Bounty>`. The `BountyList`
//! type is intentionally read-only — POST /bounties announce + ledger
//! writes ship in a later slice.

use std::path::Path;

use boole_core::{load_bounties, BountyList};

fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1.json")
        .canonicalize()
        .expect("bounty fixture path")
}

#[test]
fn loads_v1_bounties_fixture_with_two_entries() {
    let bounties = load_bounties(&fixture_path()).expect("fixture loads");
    assert_eq!(bounties.len(), 2);
    assert_eq!(bounties[0].id, "alpha-1");
    assert_eq!(bounties[0].domain, "lean.protocol-invariant");
    assert_eq!(bounties[0].status, "open");
    assert_eq!(bounties[0].reward, "42");
    assert_eq!(
        bounties[0]
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
fn rejects_bad_version() {
    let dir = std::env::temp_dir().join(format!(
        "boole-s11-bounty-loader-bad-version-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("bad.json");
    std::fs::write(&path, br#"{"version":2,"bounties":[]}"#).expect("write bad fixture");
    let err = load_bounties(&path).expect_err("must reject version != 1");
    let msg = err.to_string();
    assert!(
        msg.contains("version") || msg.contains("2"),
        "error must mention version: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accepts_empty_list() {
    let dir = std::env::temp_dir().join(format!(
        "boole-s11-bounty-loader-empty-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("empty.json");
    std::fs::write(&path, br#"{"version":1,"bounties":[]}"#).expect("write empty fixture");
    let bounties = load_bounties(&path).expect("empty list parses");
    assert!(bounties.is_empty());

    // Also confirm the envelope type itself round-trips an empty list.
    let parsed: BountyList = serde_json::from_str(r#"{"version":1,"bounties":[]}"#).unwrap();
    assert_eq!(parsed.version, 1);
    assert!(parsed.bounties.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}
