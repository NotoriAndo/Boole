//! Gate-fitness representative: carry ONE official EVM state-transition
//! test through a common adapter seam and re-verify it against a pinned,
//! already-verified engine (no new EVM engine is implemented here).
//!
//! Scope (operator msg 3459): this is a REPRESENTATIVE feasibility slice,
//! not a supply/mineable claim. The only success verdict this crate can
//! justify is `EVM-ADAPTER-REPRESENTATIVE-PASS`. It is NOT wired into the
//! consensus/runtime path; `mineable_now` stays 0.
//!
//! ## Correspondence to the consensus seam
//!
//! [`UsefulProductAdapter`] mirrors, variant-for-variant where they
//! overlap, `boole_node::UsefulProductAdapter` /
//! `boole_node::PacketAuditOutcome` / `boole_node::DeterministicBudget`.
//! It is kept as a separate minimal crate on purpose:
//!
//! * it must not drag the consensus crate (`boole-node` → `boole-p2p`,
//!   `boole-lean-runner`, axum/tokio) into a focused representative test,
//!   and
//! * it must not touch the mature Lean verification path in any way.
//!
//! Promotion path (out of scope here): the outcomes map 1:1 onto
//! `boole_node::PacketAuditOutcome` — `Accepted → Accepted`, every
//! `Rejected(_) → Rejected(PacketAuditReject::DigestMismatch { path })`
//! with the reject's field as `path`, `RetryableUnavailable →
//! RetryableUnavailable` — so wiring it into the real seam later is a
//! mechanical mapping, which is exactly what "the gate shape fits EVM"
//! means.
//!
//! ## What the adapter verifies (state-transition, three gates)
//!
//! A packet pins a state test as three t8n input files (`alloc.json`,
//! `env.json`, `txs.json`) plus a manifest declaring the input digest and
//! the expected post-state (`post_alloc` + `state_root`) and the engine /
//! fork / chain pins. `audit` runs the pinned engine and enforces, in
//! order:
//!
//! 1. **input-digest** — the raw input-file bytes must hash to the pinned
//!    `input_digest`; a tampered input rejects BEFORE the engine runs.
//! 2. **expected-output** — the engine's post `alloc` must equal the
//!    declared `post_alloc`.
//! 3. **final-state** — the engine's `stateRoot` must equal the declared
//!    `state_root`.
//!
//! Each tamper therefore rejects with its own distinct reason. Engine
//! absence/crash is `RetryableUnavailable` / `EngineError` — availability
//! is never counted as an accept or a reject (mirrors the digest
//! adapter's C7 split).

/// Verifier-only compressed-STARK check for the frozen EVM zkVM feasibility
/// proof (ADR-0018). Off-consensus, default OFF; no proof-generation call and
/// no DIRECT prover / proving-key dependency (indirect prover code is still in
/// the build graph via upstream packaging); `mineable_now` stays 0.
pub mod zk_verify;

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The deterministic budget an adapter declares (mirror of
/// `boole_node::DeterministicBudget`). Wall-clock is containment and
/// never appears here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicBudget {
    pub max_total_bytes: u64,
    pub max_files: u32,
}

/// The EVM state-test representative budget: a small cap generous enough
/// for one t8n triple (alloc/env/txs) plus its manifest, with headroom.
pub const EVM_STATETEST_BUDGET: DeterministicBudget = DeterministicBudget {
    max_total_bytes: 1024 * 1024,
    max_files: 8,
};

/// Distinct reject reasons. The three tamper reasons are deliberately
/// separate so a caller can prove that input / expected-output /
/// final-state tampering each fail their own gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditReject {
    /// The manifest or an input file is missing / unreadable / unparseable.
    PacketUnreadable { detail: String },
    /// The manifest parsed but violates the schema (missing pins, etc.).
    MalformedPacket { detail: String },
    /// INPUT TAMPER — raw input bytes do not hash to the pinned digest.
    InputDigestMismatch { expected: String, actual: String },
    /// EXPECTED-OUTPUT TAMPER — engine post `alloc` ≠ declared `post_alloc`.
    ExpectedPostAllocMismatch { detail: String },
    /// FINAL-STATE TAMPER — engine `stateRoot` ≠ declared `state_root`.
    ExpectedStateRootMismatch { expected: String, actual: String },
    /// The engine ran but rejected the transaction or produced no usable
    /// result — an engine/fixture problem, not a tamper.
    EngineError { detail: String },
}

/// Adapter outcome (mirror of `boole_node::PacketAuditOutcome`).
/// `RetryableUnavailable` — the engine binary is absent/unrunnable — is
/// never an accept OR a reject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOutcome {
    Accepted { state_root: String },
    Rejected(AuditReject),
    RetryableUnavailable { detail: String },
}

/// The common seam (mirror of `boole_node::UsefulProductAdapter`).
pub trait UsefulProductAdapter {
    fn adapter_id(&self) -> &'static str;
    /// `None` means no deterministic resource meter and therefore barred
    /// from receipt consensus (B4). This adapter always meters.
    fn resource_meter(&self) -> Option<DeterministicBudget>;
    fn audit(&self, packet_dir: &Path) -> AuditOutcome;
}

#[derive(Debug, Deserialize)]
struct ChainPin {
    fork: String,
    chain_id: u64,
}

#[derive(Debug, Deserialize)]
struct Expected {
    post_alloc: Value,
    state_root: String,
}

/// The `engine` object in the manifest (name / commit / lockfile digest)
/// is pinned provenance recorded in the committed packet; it is ignored
/// by serde here because this adapter takes the engine binary path
/// explicitly (from the operator, pointed at the pinned checkout).
/// Verifying the running binary against the pinned commit is a promotion
/// item, not part of this representative.
#[derive(Debug, Deserialize)]
struct PacketManifest {
    chain: ChainPin,
    input_files: Vec<String>,
    input_digest: String,
    expected: Expected,
}

/// The pinned-engine EVM state-test adapter. It shells out to an
/// already-verified engine binary (the Ethereum execution-specs
/// `ethereum-spec-evm t8n`, pinned by commit in the packet manifest); it
/// implements no EVM semantics itself.
#[derive(Debug, Clone)]
pub struct EvmStateTestAdapter {
    engine_bin: PathBuf,
    budget: DeterministicBudget,
}

impl EvmStateTestAdapter {
    /// Construct with the path to the pinned engine binary. The engine
    /// identity (name + commit) is additionally pinned per-packet and
    /// checked by provenance, not by this path.
    pub fn new(engine_bin: PathBuf) -> Self {
        Self {
            engine_bin,
            budget: EVM_STATETEST_BUDGET,
        }
    }

    fn read_manifest(packet_dir: &Path) -> Result<PacketManifest, AuditReject> {
        let bytes = std::fs::read(packet_dir.join("packet.json")).map_err(|e| {
            AuditReject::PacketUnreadable {
                detail: e.to_string(),
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|e| AuditReject::MalformedPacket {
            detail: e.to_string(),
        })
    }
}

/// Recursively lowercase every string (keys and values) so hex/address
/// casing cannot mask an equality. Object keys go through a `BTreeMap`
/// (serde_json's default `Map`) so key order is irrelevant.
fn normalize(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(s.to_lowercase()),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        Value::Object(o) => {
            let mut m = serde_json::Map::new();
            for (k, val) in o {
                m.insert(k.to_lowercase(), normalize(val));
            }
            Value::Object(m)
        }
        other => other.clone(),
    }
}

impl UsefulProductAdapter for EvmStateTestAdapter {
    fn adapter_id(&self) -> &'static str {
        "evm-statetest.v0"
    }

    fn resource_meter(&self) -> Option<DeterministicBudget> {
        Some(self.budget)
    }

    fn audit(&self, packet_dir: &Path) -> AuditOutcome {
        use AuditOutcome::*;
        use AuditReject::*;

        let manifest = match Self::read_manifest(packet_dir) {
            Ok(m) => m,
            Err(reject) => return Rejected(reject),
        };

        // Budget on the DECLARATION alone — refuse an over-count packet
        // before reading any artifact byte.
        if manifest.input_files.len() as u64 > u64::from(self.budget.max_files) {
            return Rejected(MalformedPacket {
                detail: format!("too many input files: {}", manifest.input_files.len()),
            });
        }

        // Read the pinned input files as raw bytes and recompute the
        // combined digest over them, in the manifest's declared order.
        let mut hasher = Sha256::new();
        let mut total_bytes: u64 = 0;
        for name in &manifest.input_files {
            if name.is_empty() || name.contains("..") || name.contains('/') {
                return Rejected(MalformedPacket {
                    detail: format!("unsafe input file name: {name}"),
                });
            }
            let bytes = match std::fs::read(packet_dir.join(name)) {
                Ok(b) => b,
                Err(e) => {
                    return Rejected(PacketUnreadable {
                        detail: format!("{name}: {e}"),
                    })
                }
            };
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            if total_bytes > self.budget.max_total_bytes {
                return Rejected(MalformedPacket {
                    detail: "input bytes exceed budget".to_string(),
                });
            }
            hasher.update(&bytes);
        }
        let actual_digest = format!("sha256:{}", hex::encode(hasher.finalize()));

        // GATE 1 — input tamper: raw input bytes ≠ pinned digest.
        if actual_digest != manifest.input_digest {
            return Rejected(InputDigestMismatch {
                expected: manifest.input_digest,
                actual: actual_digest,
            });
        }

        // Engine availability is never a verdict (mirrors the digest
        // adapter's C7 split): absence/crash → RetryableUnavailable.
        if !self.engine_bin.exists() {
            return RetryableUnavailable {
                detail: format!("engine binary not found: {}", self.engine_bin.display()),
            };
        }

        // Run the pinned engine's t8n. The input triple is the same three
        // files whose bytes were just digest-verified.
        let run = Command::new(&self.engine_bin)
            .arg("t8n")
            .arg("--state.fork")
            .arg(&manifest.chain.fork)
            .arg("--state.chainid")
            .arg(manifest.chain.chain_id.to_string())
            .arg("--input.alloc")
            .arg(packet_dir.join("alloc.json"))
            .arg("--input.env")
            .arg(packet_dir.join("env.json"))
            .arg("--input.txs")
            .arg(packet_dir.join("txs.json"))
            .arg("--output.result")
            .arg("stdout")
            .arg("--output.alloc")
            .arg("stdout")
            .output();
        let run = match run {
            Ok(o) => o,
            Err(e) => {
                return RetryableUnavailable {
                    detail: format!("engine spawn failed: {e}"),
                }
            }
        };
        if !run.status.success() {
            return RetryableUnavailable {
                detail: format!(
                    "engine exited {}: {}",
                    run.status,
                    String::from_utf8_lossy(&run.stderr).trim()
                ),
            };
        }
        let parsed: Value = match serde_json::from_slice(&run.stdout) {
            Ok(v) => v,
            Err(e) => {
                return Rejected(EngineError {
                    detail: format!("engine output is not JSON: {e}"),
                })
            }
        };
        let result = &parsed["result"];
        // A representative the engine itself rejects is a fixture defect,
        // not a tamper.
        if let Some(rejected) = result.get("rejected").and_then(Value::as_array) {
            if !rejected.is_empty() {
                return Rejected(EngineError {
                    detail: format!("engine rejected the transaction: {rejected:?}"),
                });
            }
        }

        // GATE 2 — expected-output tamper: engine post alloc ≠ declared.
        if normalize(&parsed["alloc"]) != normalize(&manifest.expected.post_alloc) {
            return Rejected(ExpectedPostAllocMismatch {
                detail: "engine post alloc does not match declared post_alloc".to_string(),
            });
        }

        // GATE 3 — final-state tamper: engine state root ≠ declared.
        let engine_root = result
            .get("stateRoot")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if engine_root.to_lowercase() != manifest.expected.state_root.to_lowercase() {
            return Rejected(ExpectedStateRootMismatch {
                expected: manifest.expected.state_root.clone(),
                actual: engine_root.to_string(),
            });
        }

        Accepted {
            state_root: engine_root.to_string(),
        }
    }
}
