use std::path::Path;

use boole_core::{work_manifests_from_list, WorkManifest, WorkManifestList};

/// Read a `WorkManifestList` from disk and validate `version == 1`.
///
/// Node owns this boot-time local file IO. Core owns the decoded schema contract
/// and version validation.
pub fn load_work_manifests_from_path(path: impl AsRef<Path>) -> anyhow::Result<Vec<WorkManifest>> {
    let raw = std::fs::read_to_string(path)?;
    let list: WorkManifestList = serde_json::from_str(&raw)?;
    work_manifests_from_list(list)
}
