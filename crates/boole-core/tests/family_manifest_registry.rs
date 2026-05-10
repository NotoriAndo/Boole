//! S21 — `FamilyManifestRegistry` is the runtime structure that holds
//! parsed `FamilyManifest` entries keyed by `family_id`. It is populated
//! at boot via `load_from_dir(&Path)` over a flat directory of
//! `*.json` files (one manifest per file) and queried at proof-submit
//! time via `get(&family_id)`.
//!
//! Boot policy is **skip-and-warn** for malformed files in S21 — see
//! the `skips_*` tests below. S22 will tighten this to fatal once the
//! activation gate evaluates the manifest's `activation_height`.

use boole_core::FamilyManifestRegistry;
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
        "boole-s21-fmr-{label}-{}-{nanos}",
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
fn registry_starts_empty() {
    let registry = FamilyManifestRegistry::new();
    assert_eq!(registry.len(), 0);
    assert!(registry.get("anything").is_none());
}

#[test]
fn load_from_dir_reads_all_manifest_files() {
    let dir = fresh_dir("ok");
    write_manifest(&dir, "alpha.json", "alpha");
    write_manifest(&dir, "beta.json", "beta");
    let registry = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    assert_eq!(registry.len(), 2);
    assert_eq!(registry.get("alpha").map(|m| m.family_id.as_str()), Some("alpha"));
    assert_eq!(registry.get("beta").map(|m| m.family_id.as_str()), Some("beta"));
}

#[test]
fn load_from_dir_empty_dir_returns_empty_registry() {
    let dir = fresh_dir("empty");
    let registry = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    assert_eq!(registry.len(), 0);
}

#[test]
fn load_from_dir_skips_invalid_json_with_warning() {
    let dir = fresh_dir("bad-json");
    write_manifest(&dir, "alpha.json", "alpha");
    fs::write(dir.join("garbage.json"), b"{not json").expect("write garbage");
    let registry = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    assert_eq!(registry.len(), 1);
    assert!(registry.get("alpha").is_some());
}

#[test]
fn load_from_dir_skips_well_formed_json_that_fails_parse() {
    let dir = fresh_dir("bad-parse");
    write_manifest(&dir, "alpha.json", "alpha");
    fs::write(dir.join("not-a-manifest.json"), b"{\"hello\":\"world\"}").expect("write");
    let registry = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    assert_eq!(registry.len(), 1);
    assert!(registry.get("alpha").is_some());
}

#[test]
fn load_from_dir_ignores_non_json_files() {
    let dir = fresh_dir("non-json");
    write_manifest(&dir, "alpha.json", "alpha");
    fs::write(dir.join("README.md"), b"# notes").expect("write readme");
    fs::write(dir.join("alpha.bak"), b"backup").expect("write backup");
    let registry = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    assert_eq!(registry.len(), 1);
}

#[test]
fn load_from_dir_nonexistent_returns_err() {
    let dir = std::env::temp_dir().join("boole-s21-does-not-exist-xyz-1234567890");
    let _ = fs::remove_dir_all(&dir);
    assert!(FamilyManifestRegistry::load_from_dir(&dir).is_err());
}

#[test]
fn register_overwrites_same_family_id() {
    let dir = fresh_dir("overwrite");
    write_manifest(&dir, "first.json", "alpha");
    write_manifest(&dir, "second.json", "alpha");
    let loaded = FamilyManifestRegistry::load_from_dir(&dir).expect("load");
    // load_from_dir collapses duplicates: last one wins, len == 1.
    assert_eq!(loaded.len(), 1);
}
