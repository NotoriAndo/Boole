use std::path::{Path, PathBuf};

use boole_core::{parse_family_manifest, FamilyManifestParseResult, FamilyManifestRegistry};

/// Error from loading a family-manifest directory.
///
/// Duplicate `family_id` across files is a hard error (ADR-0015 (c) — same
/// policy as the family-root computation; no silent last-write-wins).
#[derive(Debug)]
pub enum FamilyManifestStoreError {
    Io(std::io::Error),
    DuplicateFamilyId { family_id: String, path: PathBuf },
}

impl std::fmt::Display for FamilyManifestStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "family-manifest dir read failed: {err}"),
            Self::DuplicateFamilyId { family_id, path } => write!(
                f,
                "duplicate family_id {family_id:?} at {}: each family_id may appear in exactly one manifest file",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FamilyManifestStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::DuplicateFamilyId { .. } => None,
        }
    }
}

impl From<std::io::Error> for FamilyManifestStoreError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Walk `dir`, parsing each `*.json` file as a `FamilyManifest`.
///
/// Node owns this boot-time local directory IO. Core owns the registry and
/// manifest validation. Files are processed in sorted path order so loading is
/// deterministic regardless of filesystem enumeration order. Files that fail
/// to read, fail to parse as JSON, or fail `parse_family_manifest` are skipped
/// with a `[boole-node]` stderr warning. The same `family_id` appearing in two
/// files is a hard error (`FamilyManifestStoreError::DuplicateFamilyId`).
/// `Io` is reserved for `read_dir` failure, e.g. a directory that does not
/// exist.
pub fn load_family_manifest_registry_from_dir(
    dir: impl AsRef<Path>,
) -> Result<FamilyManifestRegistry, FamilyManifestStoreError> {
    let dir = dir.as_ref();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    paths.sort();
    let mut registry = FamilyManifestRegistry::new();
    for path in paths {
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
                if registry.get(&manifest.family_id).is_some() {
                    return Err(FamilyManifestStoreError::DuplicateFamilyId {
                        family_id: manifest.family_id.clone(),
                        path,
                    });
                }
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
