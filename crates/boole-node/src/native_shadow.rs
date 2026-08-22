//! Node-native shadow binding and containment — identity keys, registry
//! binding, the `nonIssuable` bootstrap rule, the challenge state machine,
//! its durable journal, and restart recovery.
//!
//! Implements
//! `docs/node-native-shadow-binding-containment-implementation-spec-v1.md`
//! sections 4, 6 and 7 (concurrency, section 8, and the containment
//! execution layer, section 9, are separate, later slices): JSON registry
//! parsing, the four-tuple operational state key with `registryDigest`
//! bound as a field (not a key component, closing that document's F4
//! registry-drift gap), the five-tuple idempotency key, the
//! two-check bootstrap rule that resolves every previously unseen
//! registry-declared four-tuple to `Disabled` or `Active(fresh)`, the
//! `Active(fresh)` -> `InFlight` -> `Consumed` state machine with its
//! durable NDJSON journal, the route-free admission view that derives
//! `challenge_exhausted` from `Consumed` plus its terminal projection, and
//! boot-time recovery that replays that journal
//! and retains any row still `InFlight` as non-bootstrapable (fails closed —
//! see `NativeShadowJournalReplay`) pending the later containment slice that
//! can actually confirm its cleanup. This module is not wired into
//! `local_node.rs` or any HTTP route, consistent with that document's
//! section 1 non-goals (no route, no `boole-node` server change, until
//! implementation of that route itself is undertaken); section 7's
//! OS-level `flock` (step 1) and "begin serving requests" (step 5) are
//! process/route-wiring concerns that belong to that later work too.

use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

/// Operational state key and permanent exhaustion-projection key (spec
/// section 4, items 1 and 2 — the two share one identity):
/// `(familyVersion, templateId, challengeSha256, epoch)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct NativeShadowFourTuple {
    #[serde(rename = "familyVersion")]
    pub family_version: String,
    #[serde(rename = "templateId")]
    pub template_id: String,
    #[serde(rename = "challengeSha256")]
    pub challenge_sha256: String,
    pub epoch: u64,
}

/// Idempotency / redelivery-detection key (spec section 4, item 3): the
/// four-tuple plus the candidate's own answer digest. Two different
/// `candidate_digest` values against the same four-tuple are distinct keys —
/// they identify the challenge and the submitted answer together, not the
/// challenge alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NativeShadowIdempotencyKey {
    pub four_tuple: NativeShadowFourTuple,
    pub candidate_digest: String,
}

/// Challenge lifecycle states (spec section 5). `InFlight` and `Consumed` are
/// reached via `NativeShadowStateStore::begin_execution` and
/// `complete_consumed` respectively. `Expired` is declared unreachable on
/// the `nonIssuable` path (RED gate 8): no function in this module ever
/// returns it, by construction — see `bootstrap_challenge_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ChallengeState {
    ActiveFresh,
    InFlight,
    Consumed,
    Disabled,
    Expired,
}

/// One template row from `fixtures/native-shadow/registry-v1.json` (or an
/// equivalent test-only fixture, RED gate 7). Only the fields this module's
/// bootstrap logic acts on are modeled; the registry's other pinned fields
/// (`semanticLocator`, `anchorSha256`, `taskPath`, ...) are read by other,
/// later slices and are ignored here — serde drops unmodeled JSON fields by
/// default, so this is not a lossy round-trip concern for this module's job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NativeShadowTemplate {
    #[serde(rename = "familyVersion")]
    pub family_version: String,
    #[serde(rename = "templateId")]
    pub template_id: String,
    #[serde(rename = "challengeSha256")]
    pub challenge_sha256: String,
    pub epoch: u64,
    #[serde(rename = "nonIssuable", default)]
    pub non_issuable: bool,
}

/// The registry file's own top-level shape (spec section 6): `activationAllowed`
/// applies to every template the file contains; each template additionally
/// carries its own `nonIssuable`. Either forbids issuance on its own (spec
/// section 6, "either flag alone is sufficient to force `Disabled`").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct NativeShadowRegistry {
    pub schema: String,
    #[serde(rename = "activationAllowed")]
    pub activation_allowed: bool,
    pub templates: Vec<NativeShadowTemplate>,
}

impl NativeShadowRegistry {
    /// Spec section 6, check 1: a pure function of the registry's own
    /// already-trusted content, re-read from the *current* snapshot on every
    /// call — never cached from a row's bootstrap time.
    pub fn is_statically_issuable(&self, template: &NativeShadowTemplate) -> bool {
        self.activation_allowed && !template.non_issuable
    }

    pub fn four_tuple(&self, template: &NativeShadowTemplate) -> NativeShadowFourTuple {
        NativeShadowFourTuple {
            family_version: template.family_version.clone(),
            template_id: template.template_id.clone(),
            challenge_sha256: template.challenge_sha256.clone(),
            epoch: template.epoch,
        }
    }
}

/// Error loading a registry file from disk.
#[derive(Debug)]
pub(crate) enum NativeShadowRegistryError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsafeProductionPath(String),
}

impl std::fmt::Display for NativeShadowRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "native-shadow registry read failed: {err}"),
            Self::Json(err) => write!(f, "native-shadow registry not valid JSON: {err}"),
            Self::UnsafeProductionPath(reason) => {
                write!(f, "native-shadow production registry path unsafe: {reason}")
            }
        }
    }
}

impl std::error::Error for NativeShadowRegistryError {}

/// A registry parsed from disk, paired with `registryDigest`: the SHA-256 hex
/// digest of the exact raw file bytes as read — a whole-file content digest,
/// no canonicalization or reserialization step (spec section 4), recomputed
/// fresh on every load, never cached across calls.
pub(crate) struct LoadedNativeShadowRegistry {
    pub registry: NativeShadowRegistry,
    pub registry_digest: String,
}

fn load_native_shadow_registry_from_path(
    path: &Path,
) -> Result<LoadedNativeShadowRegistry, NativeShadowRegistryError> {
    let raw = std::fs::read(path).map_err(NativeShadowRegistryError::Io)?;
    let registry_digest = sha256_hex(&raw);
    let registry: NativeShadowRegistry =
        serde_json::from_slice(&raw).map_err(NativeShadowRegistryError::Json)?;
    Ok(LoadedNativeShadowRegistry {
        registry,
        registry_digest,
    })
}

/// Load the only production authority this qualification path is allowed to
/// use. The caller supplies no path: the repository-rooted absolute location
/// is fixed at build time, so changing the process CWD or renaming a test
/// fixture cannot redirect production loading. A symlink at the authority
/// location is refused rather than followed.
pub(crate) fn load_production_native_shadow_registry(
) -> Result<LoadedNativeShadowRegistry, NativeShadowRegistryError> {
    let path = Path::new(PRODUCTION_REGISTRY_PATH);
    let metadata = std::fs::symlink_metadata(path).map_err(NativeShadowRegistryError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(NativeShadowRegistryError::UnsafeProductionPath(
            "authority must be a regular non-symlink file".to_string(),
        ));
    }
    load_native_shadow_registry_from_path(path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// The production registry path `boole-node`'s own configuration must resolve
/// to. RED gate 7 safeguard (a): production configuration is asserted, at
/// startup, to be this path, never a test-only fixture. The actual startup
/// assertion call site is later, route-wiring work (out of this module's
/// scope); `assert_is_canonical_production_registry_path` below is the
/// reusable guard that call site will invoke.
pub(crate) const PRODUCTION_REGISTRY_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/registry-v1.json"
);

/// RED gate 7 safeguard (b): the test suite itself must assert the test-only
/// registry is never the file production configuration resolves to. This is
/// an allowlist of exactly one path, not a blocklist on the file name: a
/// blocklist keyed on the literal substring `"test-only"` lets the exact
/// same test-only fixture bytes through once copied to a path whose name
/// does not contain that substring (e.g. `/tmp/copied-registry.json`) — so
/// any candidate other than the pinned canonical path is rejected here,
/// regardless of what it is named.
pub(crate) fn assert_is_canonical_production_registry_path(candidate: &Path) -> Result<(), String> {
    if candidate == Path::new(PRODUCTION_REGISTRY_PATH) {
        return Ok(());
    }
    Err(format!(
        "native-shadow: refusing to treat {} as production configuration (must be the canonical production path {})",
        candidate.display(),
        PRODUCTION_REGISTRY_PATH
    ))
}

/// In-memory view of permanently exhausted challenges. It is reconstructed
/// exclusively from evidence-backed `TerminalConsumed` events in the state
/// journal; it deliberately has no independent file writer or replay API.
/// That single-authority shape makes durable `Consumed` and the derived
/// outward `challenge_exhausted` view crash-consistent instead of two facts
/// that can drift across files.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowExhaustionLedger {
    exhausted: HashSet<NativeShadowFourTuple>,
}

impl NativeShadowExhaustionLedger {
    pub fn contains(&self, four_tuple: &NativeShadowFourTuple) -> bool {
        self.exhausted.contains(four_tuple)
    }

    fn record_terminal(&mut self, four_tuple: NativeShadowFourTuple) {
        self.exhausted.insert(four_tuple);
    }
}

/// Spec section 6's corrected bootstrap rule, two ordered checks:
/// 1. static issuability gate — either flag forbidding issuance forces
///    `Disabled`;
/// 2. `Active(fresh)`, reached only if the static gate permits issuance.
///
/// A permanent-exhaustion projection can only be derived from a valid
/// terminal event, which necessarily has an existing durable row. It is
/// therefore intentionally absent from this no-row bootstrap API.
pub(crate) fn bootstrap_challenge_state(
    registry: &NativeShadowRegistry,
    template: &NativeShadowTemplate,
) -> ChallengeState {
    if !registry.is_statically_issuable(template) {
        return ChallengeState::Disabled;
    }
    ChallengeState::ActiveFresh
}

/// One operational-state row: the current lifecycle state plus the
/// `registryDigest` this row was bootstrapped under (spec section 4 —
/// `registryDigest` is a field on the row, never a key component).
#[derive(Debug, Clone)]
pub(crate) struct NativeShadowStateRow {
    pub state: ChallengeState,
    pub registry_digest: String,
    /// Whether this row's bootstrap fact is already present in the durable
    /// state journal. Rows reconstructed by recovery are durable; a row made
    /// directly by the in-memory resolver is journaled before its first
    /// transition.
    durable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeShadowEvidenceCommitData {
    candidate_digest: String,
    evidence_digest: String,
    evidence: NativeShadowEvidence,
}

/// Required deterministic fields of `boole.native-shadow.evidence.v1` from
/// the authority spec section 6. Operational telemetry may accompany these
/// fields in the serialized JSON, but none of these bindings may be omitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeShadowEvidence {
    pub schema: String,
    pub submission_schema: String,
    pub submission_digest: String,
    pub family_version: String,
    pub template_id: String,
    pub anchor_digest: String,
    pub challenge_sha256: String,
    pub epoch: u64,
    pub candidate_digest: String,
    pub intake_version: String,
    pub checker_digest: String,
    pub policy_digest: String,
    pub toolchain_digest: String,
    pub verdict: NativeShadowEvidenceVerdict,
    pub reason_code: String,
    pub registry_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeShadowEvidenceVerdict {
    Accepted,
    DeterministicReject,
}

impl NativeShadowEvidence {
    fn validate_bindings(
        &self,
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
    ) -> Result<(), String> {
        if self.schema != "boole.native-shadow.evidence.v1" {
            return Err("evidence schema must be boole.native-shadow.evidence.v1".to_string());
        }
        if self.submission_schema != "boole.native-shadow.submission.v1" {
            return Err("submissionSchema must be boole.native-shadow.submission.v1".to_string());
        }
        if self.family_version != four_tuple.family_version
            || self.template_id != four_tuple.template_id
            || self.challenge_sha256 != four_tuple.challenge_sha256
            || self.epoch != four_tuple.epoch
            || self.candidate_digest != candidate_digest
        {
            return Err("evidence identity or candidate binding mismatch".to_string());
        }
        for (name, digest) in [
            ("submissionDigest", self.submission_digest.as_str()),
            ("templateId", self.template_id.as_str()),
            ("anchorDigest", self.anchor_digest.as_str()),
            ("challengeSha256", self.challenge_sha256.as_str()),
            ("candidateDigest", self.candidate_digest.as_str()),
            ("checkerDigest", self.checker_digest.as_str()),
            ("policyDigest", self.policy_digest.as_str()),
            ("toolchainDigest", self.toolchain_digest.as_str()),
        ] {
            if !is_lower_sha256_hex(digest) {
                return Err(format!(
                    "{name} must be 64 lowercase hexadecimal characters"
                ));
            }
        }
        for (name, value) in [
            ("familyVersion", self.family_version.as_str()),
            ("intakeVersion", self.intake_version.as_str()),
            ("reasonCode", self.reason_code.as_str()),
            ("registryVersion", self.registry_version.as_str()),
        ] {
            if value.is_empty() {
                return Err(format!("{name} must not be empty"));
            }
        }
        Ok(())
    }
}

fn is_lower_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Capability returned only after the exact node-owned evidence bytes were
/// durably appended. Consuming this value is required for the terminal state
/// transition, so `Consumed` cannot be reached through this API without a
/// preceding durable evidence record.
#[derive(Debug)]
pub(crate) struct DurableNativeShadowEvidenceCommit {
    four_tuple: NativeShadowFourTuple,
    registry_digest: String,
    candidate_digest: String,
    evidence_digest: String,
}

/// In-memory operational-state store keyed by the four-tuple alone. Its
/// `resolve` method enforces the row-lookup-before-bootstrap order that
/// closes F4's registry-drift gap (RED gate 3); `begin_execution` and
/// `complete_consumed` drive the durable `Active(fresh)` -> `InFlight` ->
/// `Consumed` state machine (spec section 7). Concurrency (the
/// `native_busy` try-lock, section 8) and containment execution (section 9)
/// are separate, later slices — nothing in this store enforces
/// single-flight execution on its own.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowStateStore {
    rows: HashMap<NativeShadowFourTuple, NativeShadowStateRow>,
    evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData>,
    journal_path: Option<PathBuf>,
}

/// Outcome of resolving a submission's targeted four-tuple against the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolveOutcome {
    /// No existing row for this four-tuple; one was just bootstrapped.
    Bootstrapped(ChallengeState),
    /// An existing row was found and its stored `registryDigest` matches.
    Existing(ChallengeState),
    /// An existing row was found but its stored `registryDigest` does not
    /// match the caller's freshly recomputed one — `PrecheckReject
    /// (registry_drift)` at the route layer (spec section 4). The existing
    /// row is left completely untouched; its state is returned so a route
    /// cannot accidentally hide an `InFlight` row that still requires the
    /// generalized cleanup procedure. No second, parallel row is ever
    /// created for this four-tuple.
    RegistryDrift { state: ChallengeState },
}

/// Route-free submission view derived from the durable row and the replayed
/// terminal projection. It deliberately contains no stored `Exhausted`
/// state: the outward `ChallengeExhausted` result exists only at this
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeShadowAdmissionView {
    ActiveFresh,
    InFlight,
    ChallengeExhausted,
    ChallengeDisabled,
    ChallengeStale,
}

/// Fail-closed admission-resolution failures. A projection mismatch is an
/// internal durability invariant violation, not permission to revive or run
/// the challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeShadowAdmissionError {
    NoSuchRow(NativeShadowFourTuple),
    RegistryDrift {
        state: ChallengeState,
    },
    TerminalProjectionMismatch {
        state: ChallengeState,
        projection_present: bool,
    },
}

impl NativeShadowStateStore {
    fn bind_journal_path(
        &mut self,
        journal_path: &Path,
    ) -> Result<(), NativeShadowTransitionError> {
        if let Some(expected) = &self.journal_path {
            if expected != journal_path {
                return Err(NativeShadowTransitionError::JournalPathMismatch {
                    expected: expected.clone(),
                    actual: journal_path.to_path_buf(),
                });
            }
            return Ok(());
        }
        self.journal_path = Some(journal_path.to_path_buf());
        Ok(())
    }

    /// Exact node-owned verdict evidence recovered for an `InFlight` row
    /// whose terminal write did not complete. The later containment slice
    /// must confirm process-tree cleanup before using this to finish the
    /// terminal transition; retaining it here prevents re-execution or a
    /// second evidence record after restart.
    pub(crate) fn pending_durable_evidence(
        &self,
        four_tuple: &NativeShadowFourTuple,
    ) -> Option<&NativeShadowEvidence> {
        if self.rows.get(four_tuple)?.state != ChallengeState::InFlight {
            return None;
        }
        self.evidence_commits
            .get(four_tuple)
            .map(|commit| &commit.evidence)
    }

    /// Look the four-tuple up **first**; only bootstrap when no row exists
    /// yet. This ordering — row lookup before bootstrap — is exactly what
    /// prevents a live registry-file edit from spawning a second, parallel
    /// row for a four-tuple that already has one (F4's original gap).
    pub fn resolve(
        &mut self,
        four_tuple: &NativeShadowFourTuple,
        registry_digest: &str,
        bootstrap: impl FnOnce() -> ChallengeState,
    ) -> ResolveOutcome {
        if let Some(row) = self.rows.get(four_tuple) {
            return if row.registry_digest == registry_digest {
                ResolveOutcome::Existing(row.state)
            } else {
                ResolveOutcome::RegistryDrift { state: row.state }
            };
        }
        let state = bootstrap();
        self.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state,
                registry_digest: registry_digest.to_string(),
                durable: false,
            },
        );
        ResolveOutcome::Bootstrapped(state)
    }

    /// Resolve an already-resolved row into the submission-facing view
    /// without mutating it. Registry drift is checked
    /// before any terminal projection is interpreted, and any disagreement
    /// between the durable row and that projection fails closed.
    pub fn admission_view(
        &self,
        four_tuple: &NativeShadowFourTuple,
        registry_digest: &str,
        terminal_projection: &NativeShadowExhaustionLedger,
    ) -> Result<NativeShadowAdmissionView, NativeShadowAdmissionError> {
        let row = self
            .rows
            .get(four_tuple)
            .ok_or_else(|| NativeShadowAdmissionError::NoSuchRow(four_tuple.clone()))?;
        if row.registry_digest != registry_digest {
            return Err(NativeShadowAdmissionError::RegistryDrift { state: row.state });
        }

        let projection_present = terminal_projection.contains(four_tuple);
        if projection_present != (row.state == ChallengeState::Consumed) {
            return Err(NativeShadowAdmissionError::TerminalProjectionMismatch {
                state: row.state,
                projection_present,
            });
        }

        match row.state {
            ChallengeState::ActiveFresh => Ok(NativeShadowAdmissionView::ActiveFresh),
            ChallengeState::InFlight => Ok(NativeShadowAdmissionView::InFlight),
            ChallengeState::Consumed => Ok(NativeShadowAdmissionView::ChallengeExhausted),
            ChallengeState::Disabled => Ok(NativeShadowAdmissionView::ChallengeDisabled),
            ChallengeState::Expired => Ok(NativeShadowAdmissionView::ChallengeStale),
        }
    }

    /// Spec sections 5 and 7: `Active(fresh)` -> `InFlight`. The durable
    /// journal event is appended **before** this call returns success, so a
    /// caller can only start invoking a checker after the durable record
    /// already exists on disk — never the reverse order (spec section 7's
    /// "written durably before the checker is invoked").
    pub fn begin_execution(
        &mut self,
        journal_path: impl AsRef<Path>,
        four_tuple: &NativeShadowFourTuple,
    ) -> Result<(), NativeShadowTransitionError> {
        self.bind_journal_path(journal_path.as_ref())?;
        let row = self
            .rows
            .get(four_tuple)
            .ok_or_else(|| NativeShadowTransitionError::NoSuchRow(four_tuple.clone()))?;
        if row.state != ChallengeState::ActiveFresh {
            return Err(NativeShadowTransitionError::InvalidState {
                four_tuple: four_tuple.clone(),
                expected: ChallengeState::ActiveFresh,
                actual: row.state,
            });
        }
        let registry_digest = row.registry_digest.clone();
        let row_is_durable = row.durable;
        if !row_is_durable {
            let bootstrap = NativeShadowJournalEvent::Bootstrap {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                state: ChallengeState::ActiveFresh,
            };
            let line = serde_json::to_string(&bootstrap)
                .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
            append_ndjson_line_durable(journal_path.as_ref(), &line)
                .map_err(NativeShadowTransitionError::Durability)?;
            self.rows
                .get_mut(four_tuple)
                .expect("row checked above")
                .durable = true;
        }
        let event = NativeShadowJournalEvent::InFlight {
            four_tuple: four_tuple.clone(),
            registry_digest,
        };
        let line = serde_json::to_string(&event)
            .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
        append_ndjson_line_durable(journal_path.as_ref(), &line)
            .map_err(NativeShadowTransitionError::Durability)?;
        self.rows
            .get_mut(four_tuple)
            .expect("row checked above")
            .state = ChallengeState::InFlight;
        Ok(())
    }

    /// Persist the exact node-owned evidence bytes before any terminal state
    /// transition. The returned capability is intentionally non-cloneable and
    /// is the only value `complete_consumed` accepts as proof that evidence is
    /// already durable.
    pub fn persist_evidence(
        &mut self,
        journal_path: impl AsRef<Path>,
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        evidence_json: &str,
    ) -> Result<DurableNativeShadowEvidenceCommit, NativeShadowTransitionError> {
        self.bind_journal_path(journal_path.as_ref())?;
        let row = self
            .rows
            .get(four_tuple)
            .ok_or_else(|| NativeShadowTransitionError::NoSuchRow(four_tuple.clone()))?;
        if row.state != ChallengeState::InFlight {
            return Err(NativeShadowTransitionError::InvalidState {
                four_tuple: four_tuple.clone(),
                expected: ChallengeState::InFlight,
                actual: row.state,
            });
        }
        if self.evidence_commits.contains_key(four_tuple) {
            return Err(NativeShadowTransitionError::EvidenceAlreadyExists(
                four_tuple.clone(),
            ));
        }
        let evidence: NativeShadowEvidence = serde_json::from_str(evidence_json)
            .map_err(NativeShadowTransitionError::InvalidEvidence)?;
        if evidence.schema != "boole.native-shadow.evidence.v1" {
            return Err(NativeShadowTransitionError::InvalidEvidenceSchema);
        }
        evidence
            .validate_bindings(four_tuple, candidate_digest)
            .map_err(NativeShadowTransitionError::InvalidEvidenceContract)?;

        let evidence_digest = sha256_hex(evidence_json.as_bytes());
        let registry_digest = row.registry_digest.clone();
        let event = NativeShadowJournalEvent::Evidence {
            four_tuple: four_tuple.clone(),
            registry_digest: registry_digest.clone(),
            candidate_digest: candidate_digest.to_string(),
            evidence_digest: evidence_digest.clone(),
            evidence_json: evidence_json.to_string(),
        };
        append_ndjson_line_durable(
            journal_path.as_ref(),
            &serde_json::to_string(&event)
                .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?,
        )
        .map_err(NativeShadowTransitionError::Durability)?;
        self.evidence_commits.insert(
            four_tuple.clone(),
            NativeShadowEvidenceCommitData {
                candidate_digest: candidate_digest.to_string(),
                evidence_digest: evidence_digest.clone(),
                evidence,
            },
        );
        Ok(DurableNativeShadowEvidenceCommit {
            four_tuple: four_tuple.clone(),
            registry_digest,
            candidate_digest: candidate_digest.to_string(),
            evidence_digest,
        })
    }

    /// Spec sections 5, 6 and 7: `InFlight` -> `Consumed`, with permanent
    /// exhaustion encoded in the **same** `TerminalConsumed` journal line.
    /// Every challenge this module governs is one-shot, so terminal state
    /// and permanent exhaustion must be one crash-atomic logical fact.
    ///
    /// The non-cloneable `evidence` capability can only come from this
    /// store's successful `persist_evidence` call. Consequently, a terminal
    /// append cannot be attempted until complete, typed, node-owned evidence
    /// is already durable and bound to this exact row and candidate.
    pub fn complete_consumed(
        &mut self,
        journal_path: impl AsRef<Path>,
        exhaustion_ledger: &mut NativeShadowExhaustionLedger,
        four_tuple: &NativeShadowFourTuple,
        evidence: DurableNativeShadowEvidenceCommit,
    ) -> Result<(), NativeShadowTransitionError> {
        self.bind_journal_path(journal_path.as_ref())?;
        let row = self
            .rows
            .get(four_tuple)
            .ok_or_else(|| NativeShadowTransitionError::NoSuchRow(four_tuple.clone()))?;
        if row.state != ChallengeState::InFlight {
            return Err(NativeShadowTransitionError::InvalidState {
                four_tuple: four_tuple.clone(),
                expected: ChallengeState::InFlight,
                actual: row.state,
            });
        }
        if evidence.four_tuple != *four_tuple
            || evidence.registry_digest != row.registry_digest
            || !self.evidence_commits.get(four_tuple).is_some_and(|commit| {
                commit.candidate_digest == evidence.candidate_digest
                    && commit.evidence_digest == evidence.evidence_digest
            })
        {
            return Err(NativeShadowTransitionError::EvidenceBindingMismatch(
                four_tuple.clone(),
            ));
        }
        let event = NativeShadowJournalEvent::TerminalConsumed {
            four_tuple: four_tuple.clone(),
            registry_digest: row.registry_digest.clone(),
            candidate_digest: evidence.candidate_digest,
            evidence_digest: evidence.evidence_digest,
            exhausted: true,
        };
        let line = serde_json::to_string(&event)
            .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
        append_ndjson_line_durable(journal_path.as_ref(), &line)
            .map_err(NativeShadowTransitionError::Durability)?;
        self.rows
            .get_mut(four_tuple)
            .expect("row checked above")
            .state = ChallengeState::Consumed;
        exhaustion_ledger.record_terminal(four_tuple.clone());
        Ok(())
    }
}

/// A state-transition was attempted from a row that is not in the required
/// starting state, or against a four-tuple with no existing row at all.
#[derive(Debug)]
pub(crate) enum NativeShadowTransitionError {
    NoSuchRow(NativeShadowFourTuple),
    InvalidState {
        four_tuple: NativeShadowFourTuple,
        expected: ChallengeState,
        actual: ChallengeState,
    },
    Durability(anyhow::Error),
    InvalidEvidence(serde_json::Error),
    InvalidEvidenceSchema,
    InvalidEvidenceContract(String),
    EvidenceAlreadyExists(NativeShadowFourTuple),
    EvidenceBindingMismatch(NativeShadowFourTuple),
    JournalPathMismatch {
        expected: PathBuf,
        actual: PathBuf,
    },
}

impl std::fmt::Display for NativeShadowTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSuchRow(four_tuple) => {
                write!(f, "native-shadow: no existing row for {four_tuple:?}")
            }
            Self::InvalidState {
                four_tuple,
                expected,
                actual,
            } => write!(
                f,
                "native-shadow: {four_tuple:?} is {actual:?}, expected {expected:?}"
            ),
            Self::Durability(err) => write!(f, "native-shadow: durable write failed: {err}"),
            Self::InvalidEvidence(err) => {
                write!(f, "native-shadow: evidence is not valid JSON: {err}")
            }
            Self::InvalidEvidenceSchema => write!(
                f,
                "native-shadow: evidence schema must be boole.native-shadow.evidence.v1"
            ),
            Self::InvalidEvidenceContract(reason) => {
                write!(f, "native-shadow: evidence contract invalid: {reason}")
            }
            Self::EvidenceAlreadyExists(four_tuple) => write!(
                f,
                "native-shadow: durable evidence already exists for {four_tuple:?}"
            ),
            Self::EvidenceBindingMismatch(four_tuple) => write!(
                f,
                "native-shadow: durable evidence binding mismatch for {four_tuple:?}"
            ),
            Self::JournalPathMismatch { expected, actual } => write!(
                f,
                "native-shadow: state store is bound to journal {}, not {}",
                expected.display(),
                actual.display()
            ),
        }
    }
}

impl std::error::Error for NativeShadowTransitionError {}

/// Single durable per-key authority (spec section 7): `Bootstrap` records the
/// original registry binding, `InFlight` records intent before execution,
/// `Evidence` stores the exact node-owned verdict, and `TerminalConsumed`
/// records `Consumed` plus permanent exhaustion together. Recovery derives
/// the in-memory exhaustion view from terminal lines; there is no second
/// independently writable exhaustion file that can drift from this history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeShadowJournalEvent {
    Bootstrap {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        state: ChallengeState,
    },
    InFlight {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
    },
    Evidence {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "candidateDigest")]
        candidate_digest: String,
        #[serde(rename = "evidenceDigest")]
        evidence_digest: String,
        #[serde(rename = "evidenceJson")]
        evidence_json: String,
    },
    TerminalConsumed {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "candidateDigest")]
        candidate_digest: String,
        #[serde(rename = "evidenceDigest")]
        evidence_digest: String,
        exhausted: bool,
    },
}

/// Outcome of replaying the durable state-transition journal alone, before
/// any registry-driven bootstrap of four-tuples the journal never mentions
/// (spec section 7, steps 2-3). `stuck_in_flight` holds every four-tuple
/// whose last journaled event was `InFlight` with no later `Consumed`.
/// Those rows remain present as `InFlight` in `resolved`: retaining the
/// durable marker is what structurally prevents an ordinary lookup from
/// bootstrapping the same challenge back to `Active(fresh)` before the later
/// containment slice has confirmed cleanup.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowJournalReplay {
    pub resolved: HashMap<NativeShadowFourTuple, ChallengeState>,
    pub registry_digests: HashMap<NativeShadowFourTuple, String>,
    evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData>,
    exhausted: HashSet<NativeShadowFourTuple>,
    pub stuck_in_flight: Vec<NativeShadowFourTuple>,
}

pub(crate) fn replay_native_shadow_journal(
    path: impl AsRef<Path>,
) -> anyhow::Result<NativeShadowJournalReplay> {
    let path = path.as_ref();
    let Some(raw) = read_stable_prefix(path)? else {
        return Ok(NativeShadowJournalReplay::default());
    };
    let mut resolved: HashMap<NativeShadowFourTuple, ChallengeState> = HashMap::new();
    let mut registry_digests: HashMap<NativeShadowFourTuple, String> = HashMap::new();
    let mut evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData> =
        HashMap::new();
    let mut exhausted = HashSet::new();
    for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
        let event: NativeShadowJournalEvent = serde_json::from_str(line).map_err(|err| {
            anyhow::anyhow!("nativeShadowJournal: line {} invalid JSON: {}", i + 1, err)
        })?;
        match event {
            NativeShadowJournalEvent::Bootstrap {
                four_tuple,
                registry_digest,
                state,
            } => {
                anyhow::ensure!(
                    !resolved.contains_key(&four_tuple),
                    "nativeShadowJournal: line {} bootstraps an existing row",
                    i + 1
                );
                anyhow::ensure!(
                    matches!(
                        state,
                        ChallengeState::ActiveFresh | ChallengeState::Disabled
                    ),
                    "nativeShadowJournal: line {} has illegal bootstrap state {:?}",
                    i + 1,
                    state
                );
                registry_digests.insert(four_tuple.clone(), registry_digest);
                resolved.insert(four_tuple, state);
            }
            NativeShadowJournalEvent::InFlight {
                four_tuple,
                registry_digest,
            } => {
                anyhow::ensure!(
                    resolved.get(&four_tuple) == Some(&ChallengeState::ActiveFresh),
                    "nativeShadowJournal: line {} transitions a non-Active row to InFlight",
                    i + 1
                );
                anyhow::ensure!(
                    registry_digests.get(&four_tuple) == Some(&registry_digest),
                    "nativeShadowJournal: line {} changes registryDigest for an existing row",
                    i + 1
                );
                resolved.insert(four_tuple, ChallengeState::InFlight);
            }
            NativeShadowJournalEvent::Evidence {
                four_tuple,
                registry_digest,
                candidate_digest,
                evidence_digest,
                evidence_json,
            } => {
                anyhow::ensure!(
                    resolved.get(&four_tuple) == Some(&ChallengeState::InFlight),
                    "nativeShadowJournal: line {} records evidence outside InFlight",
                    i + 1
                );
                anyhow::ensure!(
                    registry_digests.get(&four_tuple) == Some(&registry_digest),
                    "nativeShadowJournal: line {} changes registryDigest for an existing row",
                    i + 1
                );
                anyhow::ensure!(
                    evidence_digest == sha256_hex(evidence_json.as_bytes()),
                    "nativeShadowJournal: line {} evidenceDigest does not bind evidenceJson",
                    i + 1
                );
                let evidence: NativeShadowEvidence =
                    serde_json::from_str(&evidence_json).map_err(|err| {
                        anyhow::anyhow!(
                            "nativeShadowJournal: line {} evidenceJson invalid contract: {}",
                            i + 1,
                            err
                        )
                    })?;
                evidence
                    .validate_bindings(&four_tuple, &candidate_digest)
                    .map_err(|reason| {
                        anyhow::anyhow!(
                            "nativeShadowJournal: line {} evidence contract invalid: {}",
                            i + 1,
                            reason
                        )
                    })?;
                anyhow::ensure!(
                    !evidence_commits.contains_key(&four_tuple),
                    "nativeShadowJournal: line {} duplicates evidence for one challenge",
                    i + 1
                );
                evidence_commits.insert(
                    four_tuple,
                    NativeShadowEvidenceCommitData {
                        candidate_digest,
                        evidence_digest,
                        evidence,
                    },
                );
            }
            NativeShadowJournalEvent::TerminalConsumed {
                four_tuple,
                registry_digest,
                candidate_digest,
                evidence_digest,
                exhausted: terminal_exhausted,
            } => {
                anyhow::ensure!(
                    resolved.get(&four_tuple) == Some(&ChallengeState::InFlight),
                    "nativeShadowJournal: line {} records terminal state outside InFlight",
                    i + 1
                );
                anyhow::ensure!(
                    registry_digests.get(&four_tuple) == Some(&registry_digest),
                    "nativeShadowJournal: line {} changes registryDigest for an existing row",
                    i + 1
                );
                anyhow::ensure!(
                    terminal_exhausted,
                    "nativeShadowJournal: line {} terminal event omitted permanent exhaustion",
                    i + 1
                );
                anyhow::ensure!(
                    evidence_commits.get(&four_tuple).is_some_and(|commit| {
                        commit.candidate_digest == candidate_digest
                            && commit.evidence_digest == evidence_digest
                    }),
                    "nativeShadowJournal: line {} terminal event has no matching durable evidence",
                    i + 1
                );
                exhausted.insert(four_tuple.clone());
                resolved.insert(four_tuple, ChallengeState::Consumed);
            }
        }
    }
    let stuck_in_flight = resolved
        .iter()
        .filter(|(_, state)| **state == ChallengeState::InFlight)
        .map(|(four_tuple, _)| four_tuple.clone())
        .collect();
    Ok(NativeShadowJournalReplay {
        resolved,
        registry_digests,
        evidence_commits,
        exhausted,
        stuck_in_flight,
    })
}

/// Full boot-time recovery (spec section 7, steps 2-4). Step 1's OS-level
/// `flock` and step 5's "begin serving requests" are process/route-wiring
/// concerns outside this module's scope, same as the rest of this module.
///
/// Builds the state store from every journaled row, including unresolved
/// `InFlight` rows, plus a fresh section-6 bootstrap for every
/// registry-declared four-tuple the journal never mentions at all. Keeping a
/// stuck row present as `InFlight` makes it non-servable and non-bootstrapable
/// by construction until a later containment slice resolves it.
pub(crate) struct NativeShadowRecovery {
    pub store: NativeShadowStateStore,
    pub exhaustion_ledger: NativeShadowExhaustionLedger,
    pub stuck_in_flight: Vec<NativeShadowFourTuple>,
}

pub(crate) fn recover_native_shadow_state(
    registry: &NativeShadowRegistry,
    registry_digest: &str,
    journal_path: impl AsRef<Path>,
) -> anyhow::Result<NativeShadowRecovery> {
    let replay = replay_native_shadow_journal(&journal_path)?;
    // Phase 2C makes the evidence-backed terminal journal event the only
    // authority for permanent exhaustion. A legacy standalone exhaustion
    // file may be present on disk, but replaying it here would recreate the
    // split-brain state this correction removes: a challenge could appear
    // spent without any durable evidence or terminal transition.
    let mut exhaustion_ledger = NativeShadowExhaustionLedger::default();
    exhaustion_ledger
        .exhausted
        .extend(replay.exhausted.iter().cloned());
    let stuck: HashSet<NativeShadowFourTuple> = replay.stuck_in_flight.iter().cloned().collect();

    let mut store = NativeShadowStateStore {
        journal_path: Some(journal_path.as_ref().to_path_buf()),
        ..NativeShadowStateStore::default()
    };
    for (four_tuple, state) in &replay.resolved {
        let original_registry_digest = replay
            .registry_digests
            .get(four_tuple)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "nativeShadowJournal: durable row missing its original registryDigest"
                )
            })?;
        if original_registry_digest == registry_digest {
            let template = registry
                .templates
                .iter()
                .find(|template| registry.four_tuple(template) == *four_tuple)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "nativeShadowJournal: current registry digest matches but durable row is absent from registry"
                    )
                })?;
            let statically_issuable = registry.is_statically_issuable(template);
            anyhow::ensure!(
                statically_issuable || *state == ChallengeState::Disabled,
                "nativeShadowJournal: statically disabled registry row recovered as {:?}",
                state
            );
            anyhow::ensure!(
                !statically_issuable || *state != ChallengeState::Disabled,
                "nativeShadowJournal: statically issuable registry row recovered as Disabled"
            );
        }
        store.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state: *state,
                registry_digest: original_registry_digest,
                durable: true,
            },
        );
    }
    store.evidence_commits = replay.evidence_commits.clone();
    for template in &registry.templates {
        let four_tuple = registry.four_tuple(template);
        if stuck.contains(&four_tuple) {
            continue; // fail closed -- never bootstrap over a row still InFlight
        }
        if let Entry::Vacant(entry) = store.rows.entry(four_tuple.clone()) {
            let state = bootstrap_challenge_state(registry, template);
            let event = NativeShadowJournalEvent::Bootstrap {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.to_string(),
                state,
            };
            append_ndjson_line_durable(journal_path.as_ref(), &serde_json::to_string(&event)?)?;
            entry.insert(NativeShadowStateRow {
                state,
                registry_digest: registry_digest.to_string(),
                durable: true,
            });
        }
    }

    Ok(NativeShadowRecovery {
        store,
        exhaustion_ledger,
        stuck_in_flight: replay.stuck_in_flight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRODUCTION_REGISTRY_FIXTURE: &str =
        include_str!("../../../fixtures/native-shadow/registry-v1.json");
    const TEST_ONLY_REGISTRY_FIXTURE: &str =
        include_str!("../../../fixtures/native-shadow/registry-test-only-v1.json");

    fn parse_fixture(raw: &str) -> NativeShadowRegistry {
        serde_json::from_str(raw).expect("fixture must parse as NativeShadowRegistry")
    }

    /// Create one authoritative state-journal path plus a sibling path used
    /// only to emulate obsolete split-ledger files in regression tests.
    fn scratch_journal_and_exhaustion_paths(
        label: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "boole-native-shadow-journal-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        (
            dir.join("journal.ndjson"),
            dir.join("exhaustion-ledger.ndjson"),
        )
    }

    fn persist_test_evidence(
        store: &mut NativeShadowStateStore,
        journal_path: &Path,
        four_tuple: &NativeShadowFourTuple,
        label: &str,
    ) -> DurableNativeShadowEvidenceCommit {
        let candidate_digest = sha256_hex(format!("candidate-{label}").as_bytes());
        let evidence_json = test_evidence_json(four_tuple, &candidate_digest, label);
        store
            .persist_evidence(journal_path, four_tuple, &candidate_digest, &evidence_json)
            .expect("persist test evidence")
    }

    fn test_evidence_json(
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        label: &str,
    ) -> String {
        serde_json::json!({
            "schema": "boole.native-shadow.evidence.v1",
            "submissionSchema": "boole.native-shadow.submission.v1",
            "submissionDigest": sha256_hex(format!("submission-{label}").as_bytes()),
            "familyVersion": four_tuple.family_version,
            "templateId": four_tuple.template_id,
            "anchorDigest": sha256_hex(format!("anchor-{label}").as_bytes()),
            "challengeSha256": four_tuple.challenge_sha256,
            "epoch": four_tuple.epoch,
            "candidateDigest": candidate_digest,
            "intakeVersion": "proof-intake-v1",
            "checkerDigest": sha256_hex(format!("checker-{label}").as_bytes()),
            "policyDigest": sha256_hex(format!("policy-{label}").as_bytes()),
            "toolchainDigest": sha256_hex(format!("toolchain-{label}").as_bytes()),
            "verdict": "accepted",
            "reasonCode": "accepted",
            "registryVersion": "NATIVE-SHADOW-TEST-REGISTRY-V1",
        })
        .to_string()
    }

    // -- registry parsing / registryDigest ---------------------------------

    #[test]
    fn production_fixture_parses_with_expected_static_flags() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        assert!(
            !registry.activation_allowed,
            "tracked production fixture is registry-wide activationAllowed: false"
        );
        assert_eq!(registry.templates.len(), 1);
        assert!(
            registry.templates[0].non_issuable,
            "tracked production fixture's one template is nonIssuable: true"
        );
    }

    #[test]
    fn registry_digest_is_stable_and_content_sensitive() {
        let digest_a = sha256_hex(PRODUCTION_REGISTRY_FIXTURE.as_bytes());
        let digest_b = sha256_hex(PRODUCTION_REGISTRY_FIXTURE.as_bytes());
        assert_eq!(digest_a, digest_b, "same bytes must hash identically");

        let digest_c = sha256_hex(TEST_ONLY_REGISTRY_FIXTURE.as_bytes());
        assert_ne!(
            digest_a, digest_c,
            "different registry file bytes must hash differently"
        );
    }

    #[test]
    fn production_loader_uses_the_pinned_regular_registry_file() {
        let loaded = load_production_native_shadow_registry().expect("load production registry");
        assert_eq!(
            loaded.registry_digest,
            sha256_hex(PRODUCTION_REGISTRY_FIXTURE.as_bytes())
        );
        assert!(!loaded.registry.activation_allowed);
        assert!(loaded.registry.templates[0].non_issuable);
    }

    // -- RED gate 5: production fixture bootstraps Disabled ----------------

    #[test]
    fn production_fixture_bootstraps_disabled_without_terminal_history() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];

        let state = bootstrap_challenge_state(&registry, template);

        assert_eq!(
            state,
            ChallengeState::Disabled,
            "a nonIssuable + activationAllowed:false fixture must never bootstrap \
             Active(fresh), even on a brand-new node with no terminal history"
        );
    }

    // -- RED gate 6 / Phase 2D: no-row bootstrap is projection-free --------

    #[test]
    fn issuable_bootstrap_has_no_terminal_projection_input() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];

        let state = bootstrap_challenge_state(&registry, template);

        assert_eq!(
            state,
            ChallengeState::ActiveFresh,
            "no-row bootstrap only applies current static flags; terminal history \
             necessarily belongs to an existing durable row"
        );
    }

    #[test]
    fn disabled_row_without_terminal_projection_derives_challenge_disabled() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        assert_eq!(
            store.admission_view(&four_tuple, "digest-v1", &terminal_projection),
            Ok(NativeShadowAdmissionView::ChallengeDisabled)
        );
    }

    // -- RED gate 7: test-only registry fixture -----------------------------

    #[test]
    fn test_only_fixture_bootstraps_active_fresh_without_terminal_history() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];

        let state = bootstrap_challenge_state(&registry, template);

        assert_eq!(
            state,
            ChallengeState::ActiveFresh,
            "the test-only fixture's synthetic template is activationAllowed: \
             true and nonIssuable: false, so it must actually reach \
             Active(fresh) — otherwise no automated test could ever exercise \
             the real lifecycle"
        );
    }

    #[test]
    fn production_configuration_path_is_never_the_test_only_fixture() {
        assert!(
            Path::new(PRODUCTION_REGISTRY_PATH).is_absolute(),
            "the production authority path must not be resolved relative to the process working directory"
        );
        assert!(
            assert_is_canonical_production_registry_path(Path::new(PRODUCTION_REGISTRY_PATH))
                .is_ok()
        );
        assert!(assert_is_canonical_production_registry_path(Path::new(
            "fixtures/native-shadow/registry-test-only-v1.json"
        ))
        .is_err());
    }

    // -- fifth-round review finding: the guard must be an allowlist of the --
    // -- one canonical production path, not a blocklist keyed on the ------
    // -- literal substring "test-only" in the file name. A blocklist lets --
    // -- the exact same test-only fixture bytes through once copied to a --
    // -- path whose name does not contain that substring. -------------------

    #[test]
    fn renamed_copy_of_the_test_only_fixture_is_still_rejected() {
        assert!(assert_is_canonical_production_registry_path(Path::new(
            "/tmp/copied-registry.json"
        ))
        .is_err());
    }

    #[test]
    fn any_non_canonical_path_is_rejected_regardless_of_name() {
        assert!(
            assert_is_canonical_production_registry_path(Path::new("/tmp/whatever.json")).is_err()
        );
        assert!(assert_is_canonical_production_registry_path(Path::new(
            "fixtures/native-shadow/registry-v2-staging.json"
        ))
        .is_err());
    }

    // -- RED gate 8: Expired unreachable on the nonIssuable path ------------

    #[test]
    fn bootstrap_never_produces_expired() {
        let production = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let test_only = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);

        for (registry, template) in [
            (&production, &production.templates[0]),
            (&test_only, &test_only.templates[0]),
        ] {
            let state = bootstrap_challenge_state(registry, template);
            assert_ne!(
                state,
                ChallengeState::Expired,
                "no wall-clock/TTL concept applies to the nonIssuable path; \
                 Expired must never be produced by bootstrap"
            );
        }
    }

    // -- RED gate 3: registry drift never spawns a parallel row -------------

    #[test]
    fn registry_drift_on_existing_row_never_bootstraps_a_second_row() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let mut store = NativeShadowStateStore::default();
        let first = store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        assert_eq!(
            first,
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
        );

        // The registry file "changed on disk" — a second submission against
        // the same four-tuple recomputes a different registryDigest.
        let second = store.resolve(&four_tuple, "digest-v2-after-live-edit", || {
            panic!("bootstrap must never run again for a four-tuple that already has a row")
        });
        assert_eq!(
            second,
            ResolveOutcome::RegistryDrift {
                state: ChallengeState::ActiveFresh,
            },
            "a digest mismatch against an existing row is registry_drift, \
             never a second, parallel bootstrap"
        );

        // The original row is untouched: looking it up again with the
        // original digest still finds the same Existing state, not a drift.
        let third = store.resolve(&four_tuple, "digest-v1", || {
            panic!("bootstrap must not run — the original row still exists")
        });
        assert_eq!(third, ResolveOutcome::Existing(ChallengeState::ActiveFresh));
    }

    // -- Phase 2D: route-free derived admission view ----------------------

    #[test]
    fn admission_view_for_an_unknown_four_tuple_fails_closed() {
        let four_tuple = NativeShadowFourTuple {
            family_version: "UNKNOWN/V1".to_string(),
            template_id: "unknown".to_string(),
            challenge_sha256: "unknown".to_string(),
            epoch: 0,
        };
        let store = NativeShadowStateStore::default();
        let terminal_projection = NativeShadowExhaustionLedger::default();

        assert_eq!(
            store.admission_view(&four_tuple, "digest-v1", &terminal_projection),
            Err(NativeShadowAdmissionError::NoSuchRow(four_tuple))
        );
    }

    #[test]
    fn consumed_with_matching_terminal_projection_derives_challenge_exhausted() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("admission-consumed-exhausted");
        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &journal_path,
            &four_tuple,
            "admission-consumed-exhausted",
        );
        store
            .complete_consumed(&journal_path, &mut ledger, &four_tuple, evidence)
            .expect("complete consumed");

        assert_eq!(
            store.admission_view(&four_tuple, "digest-v1", &ledger),
            Ok(NativeShadowAdmissionView::ChallengeExhausted),
            "Consumed plus its matching terminal projection must derive the outward exhausted view"
        );
    }

    #[test]
    fn consumed_without_terminal_projection_fails_closed() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("admission-missing-projection");
        let mut terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &journal_path,
            &four_tuple,
            "admission-missing-projection",
        );
        store
            .complete_consumed(
                &journal_path,
                &mut terminal_projection,
                &four_tuple,
                evidence,
            )
            .expect("complete consumed");

        assert_eq!(
            store.admission_view(
                &four_tuple,
                "digest-v1",
                &NativeShadowExhaustionLedger::default(),
            ),
            Err(NativeShadowAdmissionError::TerminalProjectionMismatch {
                state: ChallengeState::Consumed,
                projection_present: false,
            }),
            "a Consumed row without its matching projection must fail closed, never revive"
        );
    }

    #[test]
    fn terminal_projection_on_a_non_consumed_row_fails_closed() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let mut terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        terminal_projection.record_terminal(four_tuple.clone());

        assert_eq!(
            store.admission_view(&four_tuple, "digest-v1", &terminal_projection),
            Err(NativeShadowAdmissionError::TerminalProjectionMismatch {
                state: ChallengeState::ActiveFresh,
                projection_present: true,
            }),
            "a projection without a durable Consumed row has no admission authority"
        );
    }

    #[test]
    fn registry_drift_precedes_terminal_projection_without_creating_a_second_row() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("admission-terminal-registry-drift");
        let mut terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &journal_path,
            &four_tuple,
            "admission-terminal-registry-drift",
        );
        store
            .complete_consumed(
                &journal_path,
                &mut terminal_projection,
                &four_tuple,
                evidence,
            )
            .expect("complete consumed");

        assert_eq!(
            store.admission_view(
                &four_tuple,
                "digest-v2-after-live-edit",
                &terminal_projection,
            ),
            Err(NativeShadowAdmissionError::RegistryDrift {
                state: ChallengeState::Consumed,
            })
        );
        assert_eq!(
            store.rows.len(),
            1,
            "registry drift must not create a second row"
        );
        assert_eq!(
            store.admission_view(&four_tuple, "digest-v1", &terminal_projection),
            Ok(NativeShadowAdmissionView::ChallengeExhausted),
            "the original terminal row remains intact under its original digest"
        );
    }

    // -- Four-tuple / five-tuple identity -----------------------------------

    #[test]
    fn idempotency_key_distinguishes_candidate_digest_over_same_four_tuple() {
        let four_tuple = NativeShadowFourTuple {
            family_version: "FAM/V1".to_string(),
            template_id: "tmpl".to_string(),
            challenge_sha256: "chal".to_string(),
            epoch: 0,
        };
        let key_a = NativeShadowIdempotencyKey {
            four_tuple: four_tuple.clone(),
            candidate_digest: "answer-a-digest".to_string(),
        };
        let key_b = NativeShadowIdempotencyKey {
            four_tuple: four_tuple.clone(),
            candidate_digest: "answer-b-digest".to_string(),
        };
        let key_a_again = NativeShadowIdempotencyKey {
            four_tuple,
            candidate_digest: "answer-a-digest".to_string(),
        };

        assert_ne!(
            key_a, key_b,
            "two different candidate answers against the same challenge must \
             be distinct idempotency keys, not collide under the four-tuple alone"
        );
        assert_eq!(
            key_a, key_a_again,
            "an exact redelivery of the same candidate answer is the same idempotency key"
        );
    }

    // -- Phase 2: challenge state machine, durable journal, restart recovery

    #[test]
    fn begin_execution_moves_active_fresh_to_in_flight_durably() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("begin-execution");

        let mut store = NativeShadowStateStore::default();
        let outcome = store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        assert_eq!(
            outcome,
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
        );

        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("Active(fresh) -> InFlight must succeed");

        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::InFlight)
        );

        // The transition is durable, not just in-memory: replaying the
        // journal from disk independently must observe it too.
        let replay = replay_native_shadow_journal(&journal_path).expect("replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::InFlight)
        );
    }

    #[test]
    fn begin_execution_refuses_a_row_that_is_not_active_fresh() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("begin-execution-wrong-state");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        let err = store
            .begin_execution(&journal_path, &four_tuple)
            .expect_err("the production fixture's four-tuple bootstraps Disabled, not ActiveFresh");
        assert!(matches!(
            err,
            NativeShadowTransitionError::InvalidState {
                expected: ChallengeState::ActiveFresh,
                actual: ChallengeState::Disabled,
                ..
            }
        ));

        // Refused transitions must not write anything durably.
        assert!(
            replay_native_shadow_journal(&journal_path)
                .expect("replay")
                .resolved
                .is_empty(),
            "a refused transition must leave the journal untouched"
        );
    }

    #[test]
    fn begin_execution_on_an_unknown_four_tuple_is_refused() {
        let four_tuple = NativeShadowFourTuple {
            family_version: "UNKNOWN/V1".to_string(),
            template_id: "unknown".to_string(),
            challenge_sha256: "unknown".to_string(),
            epoch: 0,
        };
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("begin-execution-unknown");
        let mut store = NativeShadowStateStore::default();

        let err = store
            .begin_execution(&journal_path, &four_tuple)
            .expect_err("no row exists for this four-tuple");
        assert!(matches!(err, NativeShadowTransitionError::NoSuchRow(_)));
    }

    #[test]
    fn complete_consumed_moves_in_flight_to_consumed_and_derives_terminal_projection() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("complete-consumed");

        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("Active(fresh) -> InFlight");

        let evidence =
            persist_test_evidence(&mut store, &journal_path, &four_tuple, "complete-consumed");

        store
            .complete_consumed(&journal_path, &mut ledger, &four_tuple, evidence)
            .expect("InFlight -> Consumed must succeed");

        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Consumed)
        );
        assert!(
            ledger.contains(&four_tuple),
            "reaching Consumed must derive permanent exhaustion from the same terminal fact \
             (spec section 6: every challenge this module governs is one-shot)"
        );

        // One terminal journal fact durably reconstructs both logical facts:
        // Consumed state and permanent exhaustion. No second authoritative
        // file write may be required for them to remain consistent.
        let replay = replay_native_shadow_journal(&journal_path).expect("replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::Consumed)
        );
        let recovered =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");
        assert!(recovered.exhaustion_ledger.contains(&four_tuple));
        assert!(
            !exhaustion_path.exists(),
            "the terminal path must not depend on a second exhaustion-file append"
        );
    }

    #[test]
    fn complete_consumed_refuses_a_row_that_is_not_in_flight() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("complete-consumed-wrong-state");

        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        let err = store
            .complete_consumed(
                &journal_path,
                &mut ledger,
                &four_tuple,
                DurableNativeShadowEvidenceCommit {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "digest-v1".to_string(),
                    candidate_digest: "candidate-unused".to_string(),
                    evidence_digest: "evidence-unused".to_string(),
                },
            )
            .expect_err("the row is Active(fresh), not InFlight");
        assert!(matches!(
            err,
            NativeShadowTransitionError::InvalidState {
                expected: ChallengeState::InFlight,
                actual: ChallengeState::ActiveFresh,
                ..
            }
        ));
        assert!(
            !ledger.contains(&four_tuple),
            "a refused transition must not create the terminal projection"
        );
    }

    #[test]
    fn one_state_store_cannot_split_one_execution_across_two_journals() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, other_journal_path) =
            scratch_journal_and_exhaustion_paths("journal-path-binding");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin");

        let err = store
            .persist_evidence(
                &other_journal_path,
                &four_tuple,
                "candidate-split",
                r#"{"schema":"boole.native-shadow.evidence.v1","verdict":"ACCEPT"}"#,
            )
            .expect_err("one lifecycle must never be split across journal files");
        assert!(matches!(
            err,
            NativeShadowTransitionError::JournalPathMismatch { .. }
        ));
        assert!(!other_journal_path.exists());
    }

    #[test]
    fn schema_only_json_cannot_create_a_durable_evidence_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("schema-only-evidence");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin");

        let result = store.persist_evidence(
            &journal_path,
            &four_tuple,
            &sha256_hex(b"candidate-schema-only"),
            r#"{"schema":"boole.native-shadow.evidence.v1"}"#,
        );

        assert!(
            result.is_err(),
            "the schema label alone is not node-owned verdict evidence; all authority-spec bindings are required"
        );
        assert!(!store.evidence_commits.contains_key(&four_tuple));
    }

    // -- restart recovery (spec section 7, steps 2-4) -----------------------

    #[test]
    fn recovery_rebuilds_a_consumed_row_from_the_journal_alone() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-consumed");

        // Simulate a full lifecycle on one "process", then recover as if a
        // brand-new process just started against the same durable files.
        {
            let mut ledger = NativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
            let evidence =
                persist_test_evidence(&mut store, &journal_path, &four_tuple, "recover-consumed");
            store
                .complete_consumed(&journal_path, &mut ledger, &four_tuple, evidence)
                .expect("complete");
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");
        assert!(recovery.stuck_in_flight.is_empty());
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Consumed)
        );
        assert!(recovery.exhaustion_ledger.contains(&four_tuple));
        assert_eq!(
            recovery
                .store
                .admission_view(&four_tuple, "digest-v1", &recovery.exhaustion_ledger,),
            Ok(NativeShadowAdmissionView::ChallengeExhausted),
            "journal replay must reconstruct the projection used by the outward admission view"
        );
    }

    #[test]
    fn legacy_exhaustion_file_alone_cannot_resurrect_terminal_authority() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("legacy-exhaustion-without-terminal");

        // This is the pre-Phase-2C split-brain shape: an exhaustion fact in a
        // second file, but no Bootstrap/InFlight/Evidence/TerminalConsumed
        // chain in the authoritative state journal. It must not be enough to
        // declare the challenge spent.
        append_ndjson_line_durable(
            &exhaustion_path,
            &serde_json::json!({
                "kind": "exhausted",
                "familyVersion": four_tuple.family_version.clone(),
                "templateId": four_tuple.template_id.clone(),
                "challengeSha256": four_tuple.challenge_sha256.clone(),
                "epoch": four_tuple.epoch,
            })
            .to_string(),
        )
        .expect("write legacy exhaustion-only record");

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");

        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row was bootstrapped")),
            ResolveOutcome::Existing(ChallengeState::ActiveFresh),
            "only an evidence-backed terminal journal event may create permanent exhaustion"
        );
        assert!(!recovery.exhaustion_ledger.contains(&four_tuple));
    }

    #[test]
    fn recovery_preserves_the_original_registry_digest_and_reports_drift() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-registry-drift");

        {
            let mut ledger = NativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
            let evidence =
                persist_test_evidence(&mut store, &journal_path, &four_tuple, "registry-drift");
            store
                .complete_consumed(&journal_path, &mut ledger, &four_tuple, evidence)
                .expect("complete");
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v2-after-restart", &journal_path)
                .expect("recover");

        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v2-after-restart", || panic!(
                    "the durable row already exists"
                ),),
            ResolveOutcome::RegistryDrift {
                state: ChallengeState::Consumed,
            },
            "restart recovery must retain the digest captured when the row was first created; \
             replacing it with the current registry digest erases drift detection"
        );
    }

    #[test]
    fn recovery_persists_a_newly_bootstrapped_rows_original_registry_digest() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-bootstrap-registry-drift");

        let first =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("first boot");
        drop(first);

        let mut second =
            recover_native_shadow_state(&registry, "digest-v2-after-restart", &journal_path)
                .expect("second boot");

        assert_eq!(
            second.store.resolve(
                &four_tuple,
                "digest-v2-after-restart",
                || panic!("the bootstrapped row must have been durable"),
            ),
            ResolveOutcome::RegistryDrift {
                state: ChallengeState::ActiveFresh,
            },
            "even a row that never reached InFlight must retain its first registry digest across restart"
        );
    }

    #[test]
    fn recovery_withholds_a_stuck_in_flight_row_instead_of_serving_or_reverting_it() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-stuck-in-flight");

        // Crash right after Active(fresh) -> InFlight, before Consumed.
        {
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");
        assert_eq!(recovery.stuck_in_flight, vec![four_tuple.clone()]);

        // Fail closed: this module has no containment-cleanup capability
        // (spec section 9, a later slice) to confirm the stuck row's
        // process/cgroup was actually torn down. The durable InFlight marker
        // therefore remains present and blocks bootstrap; omitting the row
        // would let an ordinary resolve call recreate it as Active(fresh).
        let outcome = recovery
            .store
            .resolve(&four_tuple, "digest-v1", || ChallengeState::ActiveFresh);
        assert_eq!(
            outcome,
            ResolveOutcome::Existing(ChallengeState::InFlight),
            "a stuck row must remain structurally non-bootstrappable until containment cleanup is confirmed"
        );
    }

    #[test]
    fn recovery_with_evidence_but_no_terminal_stays_in_flight_without_projection() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-evidence-before-terminal");

        {
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
            let _durable_evidence =
                persist_test_evidence(&mut store, &journal_path, &four_tuple, "crash-gap");
            // Simulated crash: no terminal transition is attempted.
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");
        assert_eq!(recovery.stuck_in_flight, vec![four_tuple.clone()]);
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row is durable")),
            ResolveOutcome::Existing(ChallengeState::InFlight)
        );
        assert!(recovery.store.evidence_commits.contains_key(&four_tuple));
        let recovered_evidence = recovery
            .store
            .pending_durable_evidence(&four_tuple)
            .expect("the exact decided verdict evidence must survive restart");
        assert_eq!(
            recovered_evidence.verdict,
            NativeShadowEvidenceVerdict::Accepted
        );
        assert_eq!(recovered_evidence.reason_code, "accepted");
        assert!(!recovery.exhaustion_ledger.contains(&four_tuple));
    }

    #[test]
    fn replay_rejects_a_terminal_record_without_matching_durable_evidence() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("terminal-without-evidence");

        for event in [
            NativeShadowJournalEvent::Bootstrap {
                four_tuple: four_tuple.clone(),
                registry_digest: "digest-v1".to_string(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlight {
                four_tuple: four_tuple.clone(),
                registry_digest: "digest-v1".to_string(),
            },
            NativeShadowJournalEvent::TerminalConsumed {
                four_tuple,
                registry_digest: "digest-v1".to_string(),
                candidate_digest: "candidate-without-evidence".to_string(),
                evidence_digest: "missing".to_string(),
                exhausted: true,
            },
        ] {
            append_ndjson_line_durable(
                &journal_path,
                &serde_json::to_string(&event).expect("serialize event"),
            )
            .expect("append event");
        }

        let err = replay_native_shadow_journal(&journal_path)
            .expect_err("terminal without evidence must fail closed");
        assert!(err
            .to_string()
            .contains("terminal event has no matching durable evidence"));
    }

    #[test]
    fn replay_rejects_bootstrap_exhausted_without_terminal_evidence() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("bootstrap-exhausted-without-evidence");

        append_ndjson_line_durable(
            &journal_path,
            &serde_json::json!({
                "kind": "bootstrap",
                "familyVersion": four_tuple.family_version,
                "templateId": four_tuple.template_id,
                "challengeSha256": four_tuple.challenge_sha256,
                "epoch": four_tuple.epoch,
                "registryDigest": "digest-v1",
                "state": "Exhausted",
            })
            .to_string(),
        )
        .expect("append bootstrap");

        let err = replay_native_shadow_journal(&journal_path)
            .expect_err("stored Exhausted is not a valid ChallengeState variant");
        assert!(err.to_string().contains("unknown variant `Exhausted`"));
    }

    #[test]
    fn recovery_rejects_active_bootstrap_for_same_statically_disabled_registry() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let registry_digest = sha256_hex(PRODUCTION_REGISTRY_FIXTURE.as_bytes());
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("disabled-registry-active-bootstrap");

        append_ndjson_line_durable(
            &journal_path,
            &serde_json::to_string(&NativeShadowJournalEvent::Bootstrap {
                four_tuple,
                registry_digest: registry_digest.clone(),
                state: ChallengeState::ActiveFresh,
            })
            .expect("serialize bootstrap"),
        )
        .expect("append bootstrap");

        let err = recover_native_shadow_state(&registry, &registry_digest, &journal_path)
            .err()
            .expect("a disabled current registry must never recover an Active row");
        assert!(err
            .to_string()
            .contains("statically disabled registry row recovered as ActiveFresh"));
    }

    #[test]
    fn torn_terminal_tail_recovers_as_evidence_backed_in_flight_not_consumed() {
        use std::io::Write as _;

        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("torn-terminal-tail");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin");
        let evidence =
            persist_test_evidence(&mut store, &journal_path, &four_tuple, "torn-terminal");

        let terminal = serde_json::to_string(&NativeShadowJournalEvent::TerminalConsumed {
            four_tuple: four_tuple.clone(),
            registry_digest: evidence.registry_digest,
            candidate_digest: evidence.candidate_digest,
            evidence_digest: evidence.evidence_digest,
            exhausted: true,
        })
        .expect("serialize terminal");
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .expect("open journal");
        journal
            .write_all(&terminal.as_bytes()[..terminal.len() / 2])
            .expect("write torn tail");
        journal.sync_all().expect("sync torn tail");

        let mut recovery = recover_native_shadow_state(&registry, "digest-v1", &journal_path)
            .expect("recover stable prefix");
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row is durable")),
            ResolveOutcome::Existing(ChallengeState::InFlight)
        );
        assert!(recovery.store.evidence_commits.contains_key(&four_tuple));
        assert!(!recovery.exhaustion_ledger.contains(&four_tuple));
    }

    #[cfg(unix)]
    #[test]
    fn evidence_write_failure_leaves_the_row_in_flight_without_a_commit() {
        use std::os::unix::fs::PermissionsExt as _;

        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("evidence-write-failure");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("begin");

        let original_mode = std::fs::metadata(&journal_path)
            .expect("journal metadata")
            .permissions()
            .mode();
        std::fs::set_permissions(
            &journal_path,
            std::fs::Permissions::from_mode(original_mode & !0o222),
        )
        .expect("make journal read-only");
        let candidate_digest = sha256_hex(b"candidate-write-failure");
        let evidence_json = test_evidence_json(&four_tuple, &candidate_digest, "write-failure");
        let result = store.persist_evidence(
            &journal_path,
            &four_tuple,
            &candidate_digest,
            &evidence_json,
        );
        std::fs::set_permissions(
            &journal_path,
            std::fs::Permissions::from_mode(original_mode),
        )
        .expect("restore journal permissions");

        assert!(matches!(
            result,
            Err(NativeShadowTransitionError::Durability(_))
        ));
        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row exists")),
            ResolveOutcome::Existing(ChallengeState::InFlight)
        );
        assert!(!store.evidence_commits.contains_key(&four_tuple));
    }

    #[test]
    fn recovery_bootstraps_every_registry_declared_four_tuple_missing_from_the_journal() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-bootstrap-remainder");

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path).expect("recover");
        assert!(recovery.stuck_in_flight.is_empty());
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Disabled)
        );
    }
}
