//! BF.5 — useful-product verifier adapter interface + the first
//! consensus-activatable adapter (B4-limited scope).
//!
//! B4: consensus verdicts may only depend on deterministic budgets — an
//! adapter without a deterministic resource meter cannot join receipt
//! consensus (`AdapterActivationSet` enforces the ban), and the first
//! activatable surface is the byte-exact digest comparison over a
//! pinned packet. The Lean verdict path already exists behind its own
//! deterministic meters (maxHeartbeats/maxRecDepth — SC.9); real
//! Circom/Rust/EVM runners need their own resource contracts first and
//! are explicit non-goals here.
//!
//! `release-digest.v0` re-derives what the packet declares:
//! deterministic caps first (a manifest DECLARING more than the budget
//! rejects before any artifact byte is read), declared-length vs file
//! metadata before hashing (compile-bomb guard), then per-file SHA-256
//! against the packet's own manifest. C7 split: a tampered PRESENT
//! file is `Rejected` (invalid wins), an ABSENT hash-referenced file
//! is `RetryableUnavailable` — availability is never a verdict.
//!
//! Node owns file IO; `boole-core` keeps only pure roots/outcomes
//! (BF.3). Rollback: removing the adapter registration leaves the
//! mode-OFF path untouched.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// B4 — the deterministic budget an adapter declares. Wall-clock is
/// containment and never appears here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicBudget {
    pub max_total_bytes: u64,
    pub max_files: u32,
}

/// The digest adapter's frozen budget: the 8 MiB packet cap from the
/// BASE-FINAL data-budget measurements, and a file-count cap generous
/// enough for the golden packets (15/19 files) with headroom.
pub const RELEASE_DIGEST_BUDGET: DeterministicBudget = DeterministicBudget {
    max_total_bytes: 8 * 1024 * 1024,
    max_files: 256,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketAuditReject {
    ManifestUnreadable { detail: String },
    BudgetExceeded { declared_bytes: u64, files: u32 },
    DeclaredLengthMismatch { path: String },
    DigestMismatch { path: String },
    UnlistedFile { path: String },
    MalformedEntry { path: String },
}

/// The adapter outcome. `RetryableUnavailable` lists the pinned files
/// the node has not received yet — the caller may re-fetch and retry;
/// it must never count as an accept OR a reject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketAuditOutcome {
    Accepted {
        files_verified: u32,
        bytes_verified: u64,
    },
    Rejected(PacketAuditReject),
    RetryableUnavailable {
        missing: Vec<String>,
    },
}

pub trait UsefulProductAdapter {
    fn adapter_id(&self) -> &'static str;
    /// `None` means the adapter has no deterministic resource meter and
    /// is therefore barred from receipt consensus (B4).
    fn resource_meter(&self) -> Option<DeterministicBudget>;
    fn audit(&self, packet_dir: &Path) -> PacketAuditOutcome;
}

#[derive(Debug, Error)]
pub enum AdapterActivationError {
    #[error("adapter {adapter_id} declares no deterministic resource meter and cannot join receipt consensus (B4)")]
    MeterlessAdapterForbidden { adapter_id: &'static str },
    #[error("adapter {adapter_id} is already active")]
    DuplicateAdapter { adapter_id: &'static str },
}

/// B4 activation ban: only metered adapters may be registered for the
/// testnet receipt path.
#[derive(Default)]
pub struct AdapterActivationSet {
    active: BTreeMap<&'static str, Box<dyn UsefulProductAdapter>>,
}

impl AdapterActivationSet {
    pub fn activate(
        &mut self,
        adapter: Box<dyn UsefulProductAdapter>,
    ) -> Result<(), AdapterActivationError> {
        let adapter_id = adapter.adapter_id();
        if adapter.resource_meter().is_none() {
            return Err(AdapterActivationError::MeterlessAdapterForbidden { adapter_id });
        }
        if self.active.contains_key(adapter_id) {
            return Err(AdapterActivationError::DuplicateAdapter { adapter_id });
        }
        self.active.insert(adapter_id, adapter);
        Ok(())
    }

    pub fn is_active(&self, adapter_id: &str) -> bool {
        self.active.contains_key(adapter_id)
    }
}

/// One pinned file entry. The two experiment packet generations name
/// the declared size differently (`bytes` vs `length`); both are
/// accepted, but one of them must be present.
#[derive(Debug, Deserialize)]
struct ManifestFileRaw {
    path: String,
    sha256: String,
    #[serde(default)]
    bytes: Option<u64>,
    #[serde(default)]
    length: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PacketManifestRaw {
    files: Vec<ManifestFileRaw>,
}

/// `release-digest.v0` — the byte-exact packet re-verification adapter.
#[derive(Debug, Default)]
pub struct PinnedPacketDigestAdapter;

impl UsefulProductAdapter for PinnedPacketDigestAdapter {
    fn adapter_id(&self) -> &'static str {
        "release-digest.v0"
    }

    fn resource_meter(&self) -> Option<DeterministicBudget> {
        Some(RELEASE_DIGEST_BUDGET)
    }

    fn audit(&self, packet_dir: &Path) -> PacketAuditOutcome {
        use PacketAuditOutcome::*;
        use PacketAuditReject::*;

        let manifest_bytes = match fs::read(packet_dir.join("manifest.json")) {
            Ok(bytes) => bytes,
            Err(err) => {
                return Rejected(ManifestUnreadable {
                    detail: err.to_string(),
                })
            }
        };
        let manifest: PacketManifestRaw = match serde_json::from_slice(&manifest_bytes) {
            Ok(manifest) => manifest,
            Err(err) => {
                return Rejected(ManifestUnreadable {
                    detail: err.to_string(),
                })
            }
        };

        // Budget check on the DECLARATION alone — before any artifact
        // byte is read, an oversize claim already breaks the budget.
        let budget = RELEASE_DIGEST_BUDGET;
        let mut declared_total: u64 = 0;
        for entry in &manifest.files {
            let Some(declared) = entry.bytes.or(entry.length) else {
                return Rejected(MalformedEntry {
                    path: entry.path.clone(),
                });
            };
            if entry.sha256.len() != 64 || entry.path.is_empty() || entry.path.contains("..") {
                return Rejected(MalformedEntry {
                    path: entry.path.clone(),
                });
            }
            declared_total = declared_total.saturating_add(declared);
        }
        if declared_total > budget.max_total_bytes
            || manifest.files.len() as u64 > budget.max_files as u64
        {
            return Rejected(BudgetExceeded {
                declared_bytes: declared_total,
                files: manifest.files.len() as u32,
            });
        }

        // No unlisted files: the packet on disk must be a subset of the
        // pinned release (manifest.json itself excepted).
        let mut on_disk = Vec::new();
        collect_files(packet_dir, packet_dir, &mut on_disk);
        for rel in &on_disk {
            if rel == "manifest.json" {
                continue;
            }
            if !manifest.files.iter().any(|entry| &entry.path == rel) {
                return Rejected(UnlistedFile { path: rel.clone() });
            }
        }

        // Byte-exact verification of every present file; absent pinned
        // files accumulate as availability (C7: invalid wins, so any
        // mismatch rejects immediately).
        let mut missing = Vec::new();
        let mut files_verified = 0u32;
        let mut bytes_verified = 0u64;
        for entry in &manifest.files {
            let declared = entry.bytes.or(entry.length).expect("checked above");
            let path = packet_dir.join(&entry.path);
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    missing.push(entry.path.clone());
                    continue;
                }
            };
            // Compile-bomb guard: the size check precedes any read.
            if metadata.len() != declared {
                return Rejected(DeclaredLengthMismatch {
                    path: entry.path.clone(),
                });
            }
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(_) => {
                    missing.push(entry.path.clone());
                    continue;
                }
            };
            let digest = hex::encode(Sha256::digest(&bytes));
            if digest != entry.sha256 {
                return Rejected(DigestMismatch {
                    path: entry.path.clone(),
                });
            }
            files_verified += 1;
            bytes_verified += metadata.len();
        }

        if !missing.is_empty() {
            return RetryableUnavailable { missing };
        }
        Accepted {
            files_verified,
            bytes_verified,
        }
    }
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}
