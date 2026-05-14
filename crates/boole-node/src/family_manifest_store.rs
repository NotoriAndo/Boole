use std::path::Path;

use boole_core::{parse_family_manifest, FamilyManifestParseResult, FamilyManifestRegistry};

/// Walk `dir`, parsing each `*.json` file as a `FamilyManifest`.
///
/// Node owns this boot-time local directory IO. Core owns the registry and
/// manifest validation. Files that fail to read, fail to parse as JSON, or fail
/// `parse_family_manifest` are skipped with a `[boole-node]` stderr warning.
/// Same `family_id` from two files: last write wins. `Err` is reserved for
/// `read_dir` failure, e.g. a directory that does not exist.
pub fn load_family_manifest_registry_from_dir(
    dir: impl AsRef<Path>,
) -> std::io::Result<FamilyManifestRegistry> {
    let dir = dir.as_ref();
    let entries = std::fs::read_dir(dir)?;
    let mut registry = FamilyManifestRegistry::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                eprintln!(
                    "[boole-node] family-manifest read failed {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        let value = match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(v) => v,
            Err(err) => {
                eprintln!(
                    "[boole-node] family-manifest not valid JSON {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        match parse_family_manifest(&value) {
            FamilyManifestParseResult::Ok(manifest) => {
                registry.register(*manifest);
            }
            FamilyManifestParseResult::Err(reason) => {
                eprintln!(
                    "[boole-node] family-manifest rejected {}: {reason}",
                    path.display()
                );
            }
        }
    }
    Ok(registry)
}
