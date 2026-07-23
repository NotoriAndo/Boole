//! BF.5-pre (A2) — pinned useful-product golden fixture integrity.
//!
//! The BF.5 verifier-adapter gate re-verifies a golden subset of the
//! experiment-GO artifacts. Those artifacts lived only in gitignored
//! `local-docs`, which CI cannot read — so this slice imports a pinned
//! subset into `fixtures/useful-product/golden/` under the A2 rules:
//! upstream URL + immutable commit pin, license verified and shipped
//! alongside, a SHA-256 manifest over every imported byte, and files
//! too large for the repo left as hash references (their digests are
//! carried by the packet's own manifest). CI golden input is ONLY this
//! directory — tests must never read `local-docs` (BF.5 rule).

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const GOLDEN_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/useful-product/golden"
);

/// Files above this size stay in local-docs as hash references — the
/// packet manifests carry their digests, so verification integrity is
/// preserved without vendoring megabytes into the repo.
const MAX_IMPORTED_FILE_BYTES: u64 = 64 * 1024;

fn golden_root() -> PathBuf {
    PathBuf::from(GOLDEN_DIR)
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("golden dir readable") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn relative(path: &Path) -> String {
    path.strip_prefix(golden_root())
        .expect("under golden root")
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn sha256sums_covers_every_imported_file_and_matches() {
    let sums = fs::read_to_string(golden_root().join("SHA256SUMS")).expect("SHA256SUMS present");
    let mut listed = BTreeSet::new();
    for line in sums.lines().filter(|line| !line.trim().is_empty()) {
        let (hash, rel) = line.split_once("  ").expect("`<sha256>  <path>` format");
        assert_eq!(hash.len(), 64, "sha256 hex length for {rel}");
        let bytes = fs::read(golden_root().join(rel))
            .unwrap_or_else(|_| panic!("listed file missing: {rel}"));
        let digest = hex::encode(Sha256::digest(&bytes));
        assert_eq!(digest, hash, "hash mismatch for {rel}");
        listed.insert(rel.to_string());
    }
    assert!(!listed.is_empty(), "SHA256SUMS must not be empty");

    let mut on_disk = Vec::new();
    walk_files(&golden_root(), &mut on_disk);
    for path in on_disk {
        let rel = relative(&path);
        if rel == "SHA256SUMS" {
            continue;
        }
        assert!(
            listed.contains(&rel),
            "unlisted file in golden fixtures: {rel}"
        );
        let size = fs::metadata(&path).expect("metadata").len();
        assert!(
            size <= MAX_IMPORTED_FILE_BYTES,
            "{rel} is {size} bytes — large artifacts stay hash references, not imports"
        );
    }
}

#[test]
fn provenance_pins_upstream_commit_and_license_for_every_item() {
    let provenance: Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join("PROVENANCE.json")).expect("PROVENANCE.json"),
    )
    .expect("provenance parses");
    let items = provenance["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3, "adapter card + llm product + supply chain");
    let known_licenses = ["Apache-2.0", "GPL-3.0", "MIT"];
    for item in items {
        let id = item["id"].as_str().expect("item id");
        let license = item["license"].as_str().expect("license");
        assert!(
            known_licenses.iter().any(|known| license.contains(known)),
            "{id}: unknown license {license}"
        );
        assert!(
            item["licenseFile"].as_str().is_some(),
            "{id}: the license text must ship alongside the fixture"
        );
        match item["origin"].as_str().expect("origin") {
            "upstream" => {
                let repo = item["upstream"]["repository"].as_str().expect("repository");
                let commit = item["upstream"]["commit"].as_str().expect("commit");
                assert!(!repo.is_empty(), "{id}: repository url");
                assert_eq!(commit.len(), 40, "{id}: immutable 40-hex commit pin");
                assert!(
                    commit.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{id}: commit must be hex"
                );
            }
            "boole-closed-experiment" => {
                assert!(
                    item["experiment"].as_str().is_some(),
                    "{id}: self-produced artifacts must name their experiment"
                );
            }
            other => panic!("{id}: unknown origin {other}"),
        }
    }
}

#[test]
fn golden_identity_anchors_match_the_imported_artifacts() {
    let provenance: Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join("PROVENANCE.json")).expect("PROVENANCE.json"),
    )
    .expect("provenance parses");
    let anchor = |id: &str, key: &str| -> String {
        provenance["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap_or_else(|| panic!("item {id}"))["anchors"][key]
            .as_str()
            .unwrap_or_else(|| panic!("{id} anchor {key}"))
            .to_string()
    };

    // Adapter R5 strict-blind card: the card identity and its gnark-crypto pin.
    let card: Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join("adapter-r5-gnark-crypto/r5-card.json"))
            .expect("r5 card"),
    )
    .expect("card parses");
    assert_eq!(
        card["cards"][0]["adapter_id"].as_str().expect("adapter id"),
        anchor("adapter-r5-gnark-crypto", "adapterId")
    );
    assert_eq!(
        card["cards"][0]["component"]["commit"]
            .as_str()
            .expect("card commit"),
        provenance["items"][0]["upstream"]["commit"]
            .as_str()
            .expect("upstream commit"),
        "the card must pin the same commit the provenance declares"
    );

    // LLM-mined product: the release root recorded at experiment time.
    let result: Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join("llm-mining-strong-r1/result.json"))
            .expect("llm result"),
    )
    .expect("result parses");
    assert_eq!(
        result["release_root"].as_str().expect("release root"),
        anchor("llm-mining-strong-r1", "releaseRoot")
    );
    assert_eq!(result["accepted"], Value::Bool(true));

    // Supply-chain Poseidon release: circomlib pin + release root.
    let contract: Value = serde_json::from_str(
        &fs::read_to_string(
            golden_root().join("supply-chain-poseidon/package/meta/protocol-contract.json"),
        )
        .expect("protocol contract"),
    )
    .expect("contract parses");
    assert_eq!(
        contract["source_provenance"]["repository"]
            .as_str()
            .expect("repository"),
        "iden3/circomlib"
    );
    assert_eq!(
        contract["source_provenance"]["commit"]
            .as_str()
            .expect("commit"),
        provenance["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "supply-chain-poseidon")
            .unwrap()["upstream"]["commit"]
            .as_str()
            .unwrap()
    );
    assert_eq!(
        anchor("supply-chain-poseidon", "releaseRoot").len(),
        64,
        "release root anchor recorded"
    );
}

#[test]
fn hash_referenced_large_files_keep_their_digests_in_the_packet_manifest() {
    // The supply-chain packet's own manifest carries a sha256 for every
    // release file; files we did NOT import must still be pinned there,
    // so a future BF.5 adapter can fetch + verify them independently.
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(golden_root().join("supply-chain-poseidon/package/manifest.json"))
            .expect("packet manifest"),
    )
    .expect("manifest parses");
    let mut missing_but_pinned = 0;
    for file in manifest["files"].as_array().expect("files") {
        let rel = file["path"].as_str().expect("path");
        let sha = file["sha256"].as_str().expect("sha256");
        assert_eq!(sha.len(), 64, "packet manifest pins {rel}");
        if !golden_root()
            .join("supply-chain-poseidon/package")
            .join(rel)
            .exists()
        {
            missing_but_pinned += 1;
        }
    }
    assert!(
        missing_but_pinned > 0,
        "the large artifacts are expected to stay hash references"
    );
}
