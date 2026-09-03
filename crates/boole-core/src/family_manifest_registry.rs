//! Runtime registry of `FamilyManifest`s, keyed by `family_id`.
//!
//! Core owns the in-memory registry and manifest domain parsing. Runtime crates
//! such as `boole-node` own walking local directories and reading JSON files.
//!
//! Iteration is deterministic: `iter()` walks manifests sorted by `family_id`,
//! independent of registration order. Block production (bounty promotion)
//! consumes this walk, so every node must traverse families identically.

use std::collections::BTreeMap;

use crate::{canonical_json::canonicalize, family_manifest::FamilyManifest, Hex32};

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

    /// Canonical JSON for the complete manifest set, encoded as one array in
    /// ascending `family_id` order. This is the exact byte string committed by
    /// [`Self::root`].
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let manifests: Vec<&FamilyManifest> = self.by_id.values().collect();
        let value = serde_json::to_value(manifests)
            .expect("a validated FamilyManifest registry always serializes");
        canonicalize(&value)
    }

    /// ADR-0015 (c): BLAKE3 over the sorted manifest set's canonical JSON.
    pub fn root(&self) -> Hex32 {
        Hex32::from_bytes(*blake3::hash(&self.canonical_bytes()).as_bytes())
    }
}
