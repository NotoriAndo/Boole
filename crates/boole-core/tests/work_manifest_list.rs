//! Pure `WorkManifestList` schema validation.
//!
//! Runtime crates own file IO. Core owns the decoded work-manifest envelope
//! contract and version gate.

use boole_core::{work_manifests_from_list, WorkManifestList};

#[test]
fn work_manifest_list_accepts_empty_v1_list() {
    let manifests = work_manifests_from_list(WorkManifestList {
        version: 1,
        work: Vec::new(),
    })
    .expect("v1 empty list parses");

    assert!(manifests.is_empty(), "empty list returned as empty Vec");
}

#[test]
fn work_manifest_list_rejects_bad_version() {
    let err = work_manifests_from_list(WorkManifestList {
        version: 2,
        work: Vec::new(),
    })
    .expect_err("non-1 version must error");
    let message = format!("{err:#}");

    assert!(
        message.contains("version") && message.contains("1"),
        "error must mention expected version: {message}"
    );
}
