//! Boot-time registry of `FamilyManifest`s, keyed by `family_id`. Loaded
//! from a flat directory of `*.json` files (one manifest per file) at
//! `LocalNodeState::from_config` time.
//!
//! S21 policy is **skip-and-warn** for unparseable files — a malformed
//! drop-in shouldn't bring the node down, and the activation gate is
//! not yet evaluated. S22 will tighten this once the gate actually
//! consumes the manifest's `activation_height` and `caps`.

use std::collections::HashMap;
use std::path::Path;

use crate::family_manifest::{parse_family_manifest, FamilyManifest, FamilyManifestParseResult};

#[derive(Debug, Default)]
pub struct FamilyManifestRegistry {
    by_id: HashMap<String, FamilyManifest>,
}

impl FamilyManifestRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, manifest: FamilyManifest) {
        self.by_id.insert(manifest.family_id.clone(), manifest);
    }

    pub fn get(&self, family_id: &str) -> Option<&FamilyManifest> {
        self.by_id.get(family_id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &FamilyManifest> {
        self.by_id.values()
    }

    /// Walk `dir`, parsing each `*.json` file as a `FamilyManifest`.
    /// Files that fail to read, fail to parse as JSON, or fail
    /// `parse_family_manifest` are skipped with a `[boole-node]` stderr
    /// warning. Same `family_id` from two files: last write wins.
    /// `Err` only for `read_dir` failure (e.g. directory doesn't exist).
    pub fn load_from_dir(dir: &Path) -> std::io::Result<Self> {
        let entries = std::fs::read_dir(dir)?;
        let mut registry = Self::default();
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
}
