//! Runtime registry of `FamilyManifest`s, keyed by `family_id`.
//!
//! Core owns the in-memory registry and manifest domain parsing. Runtime crates
//! such as `boole-node` own walking local directories and reading JSON files.
//!
//! Iteration is deterministic: `iter()` walks manifests sorted by `family_id`,
//! independent of registration order. Block production (bounty promotion)
//! consumes this walk, so every node must traverse families identically.

use std::collections::BTreeMap;

use crate::family_manifest::FamilyManifest;

#[derive(Debug, Clone, Default)]
pub struct FamilyManifestRegistry {
    by_id: BTreeMap<String, FamilyManifest>,
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
}
