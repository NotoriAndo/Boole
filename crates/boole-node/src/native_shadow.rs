//! Node-native shadow binding and containment — identity keys, registry
//! binding, the `nonIssuable` bootstrap rule, the challenge state machine,
//! its durable journal, and restart recovery.
//!
//! Implements
//! `docs/node-native-shadow-binding-containment-implementation-spec-v1.md`
//! sections 4, 6, 7 and section 8's route-free concurrency primitive (the
//! containment execution layer, section 9, remains a later slice): JSON registry
//! parsing, the four-tuple operational state key with `registryDigest`
//! bound as a field (not a key component, closing that document's F4
//! registry-drift gap), the five-tuple idempotency key, the
//! two-check bootstrap rule that resolves every previously unseen
//! registry-declared four-tuple to `Disabled` or `Active(fresh)`, the
//! `Active(fresh)` -> `InFlight` -> `Consumed` state machine with its
//! durable NDJSON journal, the route-free admission view that derives
//! `challenge_exhausted` from `Consumed` plus its terminal projection, and
//! boot-time recovery that replays that journal through one lifetime-held,
//! non-blocking-flocked file descriptor and retains any row still `InFlight`
//! as non-bootstrapable (fails closed —
//! see `NativeShadowJournalReplay`) pending the later containment slice that
//! can actually confirm its cleanup. This module is not wired into
//! `local_node.rs` or any HTTP route, consistent with that document's
//! section 1 non-goals (no route, no `boole-node` server change, until
//! implementation of that route itself is undertaken). Section 7's
//! same-descriptor lock/replay/append foundation and the route-free,
//! non-blocking RAII execution permit intended for one node-wide AppState
//! instance are implemented here. New lifecycle writes also bind the
//! node-owned `executionPolicyDigest` through v2 journal/evidence records;
//! legacy v1 records remain replay-only. AppState/route
//! ownership, containment-backed cleanup and "begin serving requests" remain
//! later process/route-wiring work.

use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durability::{
    append_ndjson_line_durable_on_file, fsync_parent_dir, read_stable_prefix_on_file,
};
use crate::state_dir::flock_exclusive_nonblocking;

#[cfg(test)]
use crate::durability::append_ndjson_line_durable;

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

/// SHA-256 identity of the node-owned execution/containment policy. This is
/// deliberately distinct from evidence `policyDigest`, which continues to
/// identify the checker policy frozen by the problem family.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NativeShadowExecutionPolicyDigest(String);

impl NativeShadowExecutionPolicyDigest {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for NativeShadowExecutionPolicyDigest {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !is_lower_sha256_hex(value) {
            return Err(
                "executionPolicyDigest must be 64 lowercase hexadecimal characters".to_string(),
            );
        }
        Ok(Self(value.to_string()))
    }
}

impl Serialize for NativeShadowExecutionPolicyDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NativeShadowExecutionPolicyDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::try_from(value.as_str()).map_err(serde::de::Error::custom)
    }
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

/// Route-free single-slot primitive for the node-wide execution rule (spec
/// section 8). A future AppState owns exactly one shared
/// `Arc<NativeShadowExecutionGate>`, and every route invocation must acquire
/// its RAII permit from it before any workspace, containment or durable state
/// change. A failed acquisition returns [`NativeShadowBusy`] without waiting
/// or queueing; the future route maps that error to
/// `RetryableUnavailable(native_busy)`.
#[derive(Debug)]
pub(crate) struct NativeShadowExecutionGate {
    busy: AtomicBool,
}

/// Immediate refusal from [`NativeShadowExecutionGate::try_acquire`]. The
/// route maps this to the outward `native_busy` reason code in a later slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeShadowBusy;

impl NativeShadowBusy {
    pub(crate) const fn reason_code(self) -> &'static str {
        "native_busy"
    }
}

impl std::fmt::Display for NativeShadowBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason_code())
    }
}

impl std::error::Error for NativeShadowBusy {}

/// Unique ownership token for the future node-wide native execution slot. It is
/// intentionally neither `Clone` nor `Copy`; normal return, error return and
/// panic unwind all release the slot through `Drop`.
#[derive(Debug)]
#[must_use = "dropping the native-shadow execution permit releases its shared slot"]
pub(crate) struct NativeShadowExecutionPermit {
    gate: Arc<NativeShadowExecutionGate>,
}

impl NativeShadowExecutionGate {
    pub(crate) const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
        }
    }

    pub(crate) fn try_acquire(
        self: &Arc<Self>,
    ) -> Result<NativeShadowExecutionPermit, NativeShadowBusy> {
        self.busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| NativeShadowBusy)?;
        Ok(NativeShadowExecutionPermit {
            gate: Arc::clone(self),
        })
    }
}

impl Drop for NativeShadowExecutionPermit {
    fn drop(&mut self) {
        self.gate.busy.store(false, Ordering::Release);
    }
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
    /// Node-owned execution/containment policy selected before this row can
    /// enter `InFlight`. Legacy journal rows have no such binding and remain
    /// readable, but cannot begin a new execution through the v2 path.
    pub execution_policy_digest: Option<NativeShadowExecutionPolicyDigest>,
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
    execution_policy_digest: Option<NativeShadowExecutionPolicyDigest>,
    evidence: NativeShadowEvidence,
}

/// Required deterministic evidence fields. Legacy journal replay accepts
/// `boole.native-shadow.evidence.v1` without an execution-policy binding;
/// every new write uses v2 and requires `executionPolicyDigest`. The existing
/// `policyDigest` field continues to mean checker policy, not containment.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy_digest: Option<NativeShadowExecutionPolicyDigest>,
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
        execution_policy_digest: Option<&NativeShadowExecutionPolicyDigest>,
    ) -> Result<(), String> {
        let expected_schema = if execution_policy_digest.is_some() {
            "boole.native-shadow.evidence.v2"
        } else {
            "boole.native-shadow.evidence.v1"
        };
        if self.schema != expected_schema {
            return Err(format!("evidence schema must be {expected_schema}"));
        }
        if self.execution_policy_digest.as_ref() != execution_policy_digest {
            return Err("evidence executionPolicyDigest binding mismatch".to_string());
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
    execution_policy_digest: NativeShadowExecutionPolicyDigest,
    candidate_digest: String,
    evidence_digest: String,
}

static NEXT_NATIVE_SHADOW_JOURNAL_AUTHORITY_ID: AtomicU64 = AtomicU64::new(1);

/// The sole writable authority for one native-shadow journal during a node
/// process lifetime. The lock, replay, torn-tail repair, appends and fsyncs
/// all use `file`; callers cannot clone or extract that descriptor.
pub(crate) struct NativeShadowJournalAuthority {
    file: File,
    diagnostic_path: PathBuf,
    device: u64,
    inode: u64,
    authority_id: u64,
    poisoned: bool,
    #[cfg(test)]
    fail_next_append: bool,
}

impl std::fmt::Debug for NativeShadowJournalAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeShadowJournalAuthority")
            .field("diagnostic_path", &self.diagnostic_path)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) enum NativeShadowJournalAuthorityError {
    Io(PathBuf, std::io::Error),
    Locked(PathBuf),
    UnsafePath(PathBuf),
    PathIdentityChanged(PathBuf),
    Poisoned(PathBuf),
    Unsupported,
}

impl std::fmt::Display for NativeShadowJournalAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, err) => write!(
                f,
                "native-shadow journal {} I/O failed: {err}",
                path.display()
            ),
            Self::Locked(path) => write!(
                f,
                "native-shadow journal {} is already locked",
                path.display()
            ),
            Self::UnsafePath(path) => write!(
                f,
                "native-shadow journal {} must be a regular non-symlink file",
                path.display()
            ),
            Self::PathIdentityChanged(path) => write!(
                f,
                "native-shadow journal {} changed identity while locked",
                path.display()
            ),
            Self::Poisoned(path) => write!(
                f,
                "native-shadow journal {} authority is fail-closed",
                path.display()
            ),
            Self::Unsupported => write!(f, "native-shadow journal authority requires a unix host"),
        }
    }
}

impl std::error::Error for NativeShadowJournalAuthorityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(_, err) => Some(err),
            _ => None,
        }
    }
}

impl NativeShadowJournalAuthority {
    #[cfg(unix)]
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, NativeShadowJournalAuthorityError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| NativeShadowJournalAuthorityError::Io(parent.to_path_buf(), err))?;
        }

        let created = match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                    return Err(NativeShadowJournalAuthorityError::UnsafePath(
                        path.to_path_buf(),
                    ));
                }
                false
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                return Err(NativeShadowJournalAuthorityError::Io(
                    path.to_path_buf(),
                    err,
                ));
            }
        };

        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)
            .map_err(|err| NativeShadowJournalAuthorityError::Io(path.to_path_buf(), err))?;
        let metadata = file
            .metadata()
            .map_err(|err| NativeShadowJournalAuthorityError::Io(path.to_path_buf(), err))?;
        if !metadata.file_type().is_file() {
            return Err(NativeShadowJournalAuthorityError::UnsafePath(
                path.to_path_buf(),
            ));
        }
        let path_metadata = std::fs::symlink_metadata(path)
            .map_err(|err| NativeShadowJournalAuthorityError::Io(path.to_path_buf(), err))?;
        if path_metadata.file_type().is_symlink()
            || !path_metadata.file_type().is_file()
            || path_metadata.dev() != metadata.dev()
            || path_metadata.ino() != metadata.ino()
        {
            return Err(NativeShadowJournalAuthorityError::PathIdentityChanged(
                path.to_path_buf(),
            ));
        }
        flock_exclusive_nonblocking(&file).map_err(|err| {
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                NativeShadowJournalAuthorityError::Locked(path.to_path_buf())
            } else {
                NativeShadowJournalAuthorityError::Io(path.to_path_buf(), err)
            }
        })?;
        if created {
            file.sync_all()
                .map_err(|err| NativeShadowJournalAuthorityError::Io(path.to_path_buf(), err))?;
            fsync_parent_dir(path).map_err(|err| {
                NativeShadowJournalAuthorityError::Io(
                    path.to_path_buf(),
                    std::io::Error::other(err.to_string()),
                )
            })?;
        }

        Ok(Self {
            file,
            diagnostic_path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
            authority_id: NEXT_NATIVE_SHADOW_JOURNAL_AUTHORITY_ID.fetch_add(1, Ordering::Relaxed),
            poisoned: false,
            #[cfg(test)]
            fail_next_append: false,
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn open(_path: impl AsRef<Path>) -> Result<Self, NativeShadowJournalAuthorityError> {
        Err(NativeShadowJournalAuthorityError::Unsupported)
    }

    fn authority_id(&self) -> u64 {
        self.authority_id
    }

    fn ensure_path_identity(&mut self) -> Result<(), NativeShadowJournalAuthorityError> {
        if self.poisoned {
            return Err(NativeShadowJournalAuthorityError::Poisoned(
                self.diagnostic_path.clone(),
            ));
        }

        #[cfg(unix)]
        {
            let matches = std::fs::symlink_metadata(&self.diagnostic_path)
                .ok()
                .is_some_and(|metadata| {
                    !metadata.file_type().is_symlink()
                        && metadata.file_type().is_file()
                        && metadata.dev() == self.device
                        && metadata.ino() == self.inode
                });
            if !matches {
                self.poisoned = true;
                return Err(NativeShadowJournalAuthorityError::PathIdentityChanged(
                    self.diagnostic_path.clone(),
                ));
            }
            Ok(())
        }

        #[cfg(not(unix))]
        {
            self.poisoned = true;
            Err(NativeShadowJournalAuthorityError::Unsupported)
        }
    }

    fn append_line(&mut self, line: &str) -> Result<(), NativeShadowJournalAuthorityError> {
        self.ensure_path_identity()?;
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_append) {
            self.poisoned = true;
            return Err(NativeShadowJournalAuthorityError::Io(
                self.diagnostic_path.clone(),
                std::io::Error::other("injected durable append failure"),
            ));
        }
        if let Err(err) = append_ndjson_line_durable_on_file(&mut self.file, line) {
            self.poisoned = true;
            return Err(NativeShadowJournalAuthorityError::Io(
                self.diagnostic_path.clone(),
                std::io::Error::other(err.to_string()),
            ));
        }
        self.ensure_path_identity()
    }

    #[cfg(test)]
    fn fail_next_append_for_test(&mut self) {
        self.fail_next_append = true;
    }

    fn read_stable_prefix(&mut self) -> Result<String, NativeShadowJournalAuthorityError> {
        self.ensure_path_identity()?;
        let raw = match read_stable_prefix_on_file(&mut self.file) {
            Ok(raw) => raw,
            Err(err) => {
                self.poisoned = true;
                return Err(NativeShadowJournalAuthorityError::Io(
                    self.diagnostic_path.clone(),
                    std::io::Error::other(err.to_string()),
                ));
            }
        };
        self.ensure_path_identity()?;
        Ok(raw)
    }
}

/// In-memory operational-state store keyed by the four-tuple alone. Its
/// `resolve` method enforces the row-lookup-before-bootstrap order that
/// closes F4's registry-drift gap (RED gate 3); `begin_execution` and
/// `complete_consumed` drive the durable `Active(fresh)` -> `InFlight` ->
/// `Consumed` state machine (spec section 7). Section 8's `native_busy`
/// permit is a separate primitive above, and
/// containment execution (section 9) is a later slice — nothing in this
/// store acquires the permit or enforces single-flight execution on its own.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowStateStore {
    rows: HashMap<NativeShadowFourTuple, NativeShadowStateRow>,
    evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData>,
    journal_authority_id: Option<u64>,
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
    /// The four-tuple and registry still match, but the node's current
    /// execution/containment policy differs from the immutable row binding.
    /// The original row remains authoritative and is never replaced.
    ExecutionPolicyDrift { state: ChallengeState },
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
    ExecutionPolicyDrift {
        state: ChallengeState,
    },
    TerminalProjectionMismatch {
        state: ChallengeState,
        projection_present: bool,
    },
}

impl NativeShadowStateStore {
    fn bind_journal_authority(
        &mut self,
        authority: &NativeShadowJournalAuthority,
    ) -> Result<(), NativeShadowTransitionError> {
        if let Some(expected) = self.journal_authority_id {
            if expected != authority.authority_id() {
                return Err(NativeShadowTransitionError::JournalAuthorityMismatch);
            }
            return Ok(());
        }
        self.journal_authority_id = Some(authority.authority_id());
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
    #[cfg(test)]
    fn resolve(
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
                execution_policy_digest: None,
                durable: false,
            },
        );
        ResolveOutcome::Bootstrapped(state)
    }

    /// Production-safe resolver for a newly selected execution policy. New
    /// rows bind that digest immediately; existing rows return a distinct
    /// drift outcome rather than being rewritten or bootstrapped in parallel.
    pub fn resolve_with_execution_policy(
        &mut self,
        four_tuple: &NativeShadowFourTuple,
        registry_digest: &str,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
        bootstrap: impl FnOnce() -> ChallengeState,
    ) -> ResolveOutcome {
        if let Some(row) = self.rows.get(four_tuple) {
            if row.registry_digest != registry_digest {
                return ResolveOutcome::RegistryDrift { state: row.state };
            }
            return if row.execution_policy_digest.as_ref() == Some(execution_policy_digest) {
                ResolveOutcome::Existing(row.state)
            } else {
                ResolveOutcome::ExecutionPolicyDrift { state: row.state }
            };
        }
        let state = bootstrap();
        self.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state,
                registry_digest: registry_digest.to_string(),
                execution_policy_digest: Some(execution_policy_digest.clone()),
                durable: false,
            },
        );
        ResolveOutcome::Bootstrapped(state)
    }

    /// Resolve an already-resolved row into the submission-facing view
    /// without mutating it. Registry drift is checked
    /// before any terminal projection is interpreted, and any disagreement
    /// between the durable row and that projection fails closed.
    fn admission_view_core(
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

    /// Production admission projection. Both the registry and the node-owned
    /// execution policy must match the immutable row before any lifecycle or
    /// terminal projection is interpreted.
    pub fn admission_view_with_execution_policy(
        &self,
        four_tuple: &NativeShadowFourTuple,
        registry_digest: &str,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
        terminal_projection: &NativeShadowExhaustionLedger,
    ) -> Result<NativeShadowAdmissionView, NativeShadowAdmissionError> {
        let row = self
            .rows
            .get(four_tuple)
            .ok_or_else(|| NativeShadowAdmissionError::NoSuchRow(four_tuple.clone()))?;
        if row.registry_digest != registry_digest {
            return Err(NativeShadowAdmissionError::RegistryDrift { state: row.state });
        }
        if row.execution_policy_digest.as_ref() != Some(execution_policy_digest) {
            return Err(NativeShadowAdmissionError::ExecutionPolicyDrift { state: row.state });
        }
        self.admission_view_core(four_tuple, registry_digest, terminal_projection)
    }

    /// Legacy test helper for the pre-v2 admission assertions. It is absent
    /// from production builds so future route code cannot bypass execution
    /// policy binding accidentally.
    #[cfg(test)]
    fn admission_view(
        &self,
        four_tuple: &NativeShadowFourTuple,
        registry_digest: &str,
        terminal_projection: &NativeShadowExhaustionLedger,
    ) -> Result<NativeShadowAdmissionView, NativeShadowAdmissionError> {
        self.admission_view_core(four_tuple, registry_digest, terminal_projection)
    }

    /// Spec sections 5 and 7: `Active(fresh)` -> `InFlight`. The durable
    /// journal event is appended **before** this call returns success, so a
    /// caller can only start invoking a checker after the durable record
    /// already exists on disk — never the reverse order (spec section 7's
    /// "written durably before the checker is invoked").
    pub fn begin_execution(
        &mut self,
        authority: &mut NativeShadowJournalAuthority,
        four_tuple: &NativeShadowFourTuple,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    ) -> Result<(), NativeShadowTransitionError> {
        self.bind_journal_authority(authority)?;
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
        let row_execution_policy_digest = row.execution_policy_digest.clone();
        let row_is_durable = row.durable;
        if row_is_durable && row_execution_policy_digest.as_ref() != Some(execution_policy_digest) {
            return Err(NativeShadowTransitionError::ExecutionPolicyDrift(
                four_tuple.clone(),
            ));
        }
        if row_execution_policy_digest
            .as_ref()
            .is_some_and(|bound| bound != execution_policy_digest)
        {
            return Err(NativeShadowTransitionError::ExecutionPolicyDrift(
                four_tuple.clone(),
            ));
        }
        if !row_is_durable {
            let bootstrap = NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: execution_policy_digest.clone(),
                state: ChallengeState::ActiveFresh,
            };
            let line = serde_json::to_string(&bootstrap)
                .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
            authority
                .append_line(&line)
                .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
            let row = self.rows.get_mut(four_tuple).expect("row checked above");
            row.durable = true;
            row.execution_policy_digest = Some(execution_policy_digest.clone());
        }
        let event = NativeShadowJournalEvent::InFlightV2 {
            four_tuple: four_tuple.clone(),
            registry_digest,
            execution_policy_digest: execution_policy_digest.clone(),
        };
        let line = serde_json::to_string(&event)
            .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
        authority
            .append_line(&line)
            .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
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
        authority: &mut NativeShadowJournalAuthority,
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        evidence_json: &str,
    ) -> Result<DurableNativeShadowEvidenceCommit, NativeShadowTransitionError> {
        self.bind_journal_authority(authority)?;
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
        let execution_policy_digest = row
            .execution_policy_digest
            .clone()
            .ok_or_else(|| NativeShadowTransitionError::ExecutionPolicyDrift(four_tuple.clone()))?;
        let evidence: NativeShadowEvidence = serde_json::from_str(evidence_json)
            .map_err(NativeShadowTransitionError::InvalidEvidence)?;
        if evidence.schema != "boole.native-shadow.evidence.v2" {
            return Err(NativeShadowTransitionError::InvalidEvidenceSchema);
        }
        evidence
            .validate_bindings(four_tuple, candidate_digest, Some(&execution_policy_digest))
            .map_err(NativeShadowTransitionError::InvalidEvidenceContract)?;

        let evidence_digest = sha256_hex(evidence_json.as_bytes());
        let registry_digest = row.registry_digest.clone();
        let event = NativeShadowJournalEvent::EvidenceV2 {
            four_tuple: four_tuple.clone(),
            registry_digest: registry_digest.clone(),
            execution_policy_digest: execution_policy_digest.clone(),
            candidate_digest: candidate_digest.to_string(),
            evidence_digest: evidence_digest.clone(),
            evidence_json: evidence_json.to_string(),
        };
        authority
            .append_line(
                &serde_json::to_string(&event).map_err(|err| {
                    NativeShadowTransitionError::Durability(anyhow::Error::from(err))
                })?,
            )
            .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
        self.evidence_commits.insert(
            four_tuple.clone(),
            NativeShadowEvidenceCommitData {
                candidate_digest: candidate_digest.to_string(),
                evidence_digest: evidence_digest.clone(),
                execution_policy_digest: Some(execution_policy_digest.clone()),
                evidence,
            },
        );
        Ok(DurableNativeShadowEvidenceCommit {
            four_tuple: four_tuple.clone(),
            registry_digest,
            execution_policy_digest,
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
        authority: &mut NativeShadowJournalAuthority,
        exhaustion_ledger: &mut NativeShadowExhaustionLedger,
        four_tuple: &NativeShadowFourTuple,
        evidence: DurableNativeShadowEvidenceCommit,
    ) -> Result<(), NativeShadowTransitionError> {
        self.bind_journal_authority(authority)?;
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
            || row.execution_policy_digest.as_ref() != Some(&evidence.execution_policy_digest)
            || !self.evidence_commits.get(four_tuple).is_some_and(|commit| {
                commit.candidate_digest == evidence.candidate_digest
                    && commit.evidence_digest == evidence.evidence_digest
                    && commit.execution_policy_digest.as_ref()
                        == Some(&evidence.execution_policy_digest)
            })
        {
            return Err(NativeShadowTransitionError::EvidenceBindingMismatch(
                four_tuple.clone(),
            ));
        }
        let event = NativeShadowJournalEvent::TerminalConsumedV2 {
            four_tuple: four_tuple.clone(),
            registry_digest: row.registry_digest.clone(),
            execution_policy_digest: evidence.execution_policy_digest,
            candidate_digest: evidence.candidate_digest,
            evidence_digest: evidence.evidence_digest,
            exhausted: true,
        };
        let line = serde_json::to_string(&event)
            .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
        authority
            .append_line(&line)
            .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
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
    ExecutionPolicyDrift(NativeShadowFourTuple),
    JournalAuthorityMismatch,
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
                "native-shadow: new evidence schema must be boole.native-shadow.evidence.v2"
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
            Self::ExecutionPolicyDrift(four_tuple) => write!(
                f,
                "native-shadow: execution policy drift for {four_tuple:?}"
            ),
            Self::JournalAuthorityMismatch => write!(
                f,
                "native-shadow: state store is bound to a different journal authority"
            ),
        }
    }
}

impl std::error::Error for NativeShadowTransitionError {}

/// Single durable per-key authority (spec section 7). Un-suffixed variants
/// are read-only legacy v1 records. Every new lifecycle writes the v2
/// variants, which carry one immutable execution-policy binding from
/// bootstrap through terminal consumption. Recovery derives the in-memory
/// exhaustion view from terminal lines; there is no second independently
/// writable exhaustion file that can drift from this history.
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
    BootstrapV2 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
        state: ChallengeState,
    },
    InFlight {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
    },
    InFlightV2 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
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
    EvidenceV2 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
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
    TerminalConsumedV2 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
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
    pub execution_policy_digests:
        HashMap<NativeShadowFourTuple, Option<NativeShadowExecutionPolicyDigest>>,
    evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData>,
    exhausted: HashSet<NativeShadowFourTuple>,
    pub stuck_in_flight: Vec<NativeShadowFourTuple>,
}

pub(crate) fn replay_native_shadow_journal(
    authority: &mut NativeShadowJournalAuthority,
) -> anyhow::Result<NativeShadowJournalReplay> {
    let raw = authority.read_stable_prefix()?;
    let mut resolved: HashMap<NativeShadowFourTuple, ChallengeState> = HashMap::new();
    let mut registry_digests: HashMap<NativeShadowFourTuple, String> = HashMap::new();
    let mut execution_policy_digests: HashMap<
        NativeShadowFourTuple,
        Option<NativeShadowExecutionPolicyDigest>,
    > = HashMap::new();
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
                execution_policy_digests.insert(four_tuple.clone(), None);
                resolved.insert(four_tuple, state);
            }
            NativeShadowJournalEvent::BootstrapV2 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
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
                execution_policy_digests.insert(four_tuple.clone(), Some(execution_policy_digest));
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
                anyhow::ensure!(
                    execution_policy_digests.get(&four_tuple) == Some(&None),
                    "nativeShadowJournal: line {} mixes legacy and v2 lifecycle events",
                    i + 1
                );
                resolved.insert(four_tuple, ChallengeState::InFlight);
            }
            NativeShadowJournalEvent::InFlightV2 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
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
                anyhow::ensure!(
                    execution_policy_digests.get(&four_tuple)
                        == Some(&Some(execution_policy_digest)),
                    "nativeShadowJournal: line {} changes executionPolicyDigest or mixes lifecycle versions",
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
                    execution_policy_digests.get(&four_tuple) == Some(&None),
                    "nativeShadowJournal: line {} mixes legacy and v2 lifecycle events",
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
                    .validate_bindings(&four_tuple, &candidate_digest, None)
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
                        execution_policy_digest: None,
                        evidence,
                    },
                );
            }
            NativeShadowJournalEvent::EvidenceV2 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
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
                    execution_policy_digests.get(&four_tuple)
                        == Some(&Some(execution_policy_digest.clone())),
                    "nativeShadowJournal: line {} changes executionPolicyDigest or mixes lifecycle versions",
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
                    .validate_bindings(
                        &four_tuple,
                        &candidate_digest,
                        Some(&execution_policy_digest),
                    )
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
                        execution_policy_digest: Some(execution_policy_digest),
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
                    execution_policy_digests.get(&four_tuple) == Some(&None),
                    "nativeShadowJournal: line {} mixes legacy and v2 lifecycle events",
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
            NativeShadowJournalEvent::TerminalConsumedV2 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
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
                    execution_policy_digests.get(&four_tuple)
                        == Some(&Some(execution_policy_digest.clone())),
                    "nativeShadowJournal: line {} changes executionPolicyDigest or mixes lifecycle versions",
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
                            && commit.execution_policy_digest.as_ref()
                                == Some(&execution_policy_digest)
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
        execution_policy_digests,
        evidence_commits,
        exhausted,
        stuck_in_flight,
    })
}

/// Full boot-time recovery (spec section 7, steps 1-4). The authority passed
/// here already owns step 1's lifetime `flock`; step 5's "begin serving
/// requests" remains route-wiring work outside this module.
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
    execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    authority: &mut NativeShadowJournalAuthority,
) -> anyhow::Result<NativeShadowRecovery> {
    let replay = replay_native_shadow_journal(authority)?;
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
        journal_authority_id: Some(authority.authority_id()),
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
        let original_execution_policy_digest = replay
            .execution_policy_digests
            .get(four_tuple)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("nativeShadowJournal: durable row missing its lifecycle version")
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
                execution_policy_digest: original_execution_policy_digest,
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
            let event = NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.to_string(),
                execution_policy_digest: execution_policy_digest.clone(),
                state,
            };
            authority.append_line(&serde_json::to_string(&event)?)?;
            entry.insert(NativeShadowStateRow {
                state,
                registry_digest: registry_digest.to_string(),
                execution_policy_digest: Some(execution_policy_digest.clone()),
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

    fn test_execution_policy_digest() -> NativeShadowExecutionPolicyDigest {
        NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"native-shadow-test-execution-policy-v1").as_str(),
        )
        .expect("test execution policy digest")
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
        authority: &mut NativeShadowJournalAuthority,
        four_tuple: &NativeShadowFourTuple,
        label: &str,
    ) -> DurableNativeShadowEvidenceCommit {
        let candidate_digest = sha256_hex(format!("candidate-{label}").as_bytes());
        let evidence_json = test_evidence_json(four_tuple, &candidate_digest, label);
        store
            .persist_evidence(authority, four_tuple, &candidate_digest, &evidence_json)
            .expect("persist test evidence")
    }

    fn test_evidence_json(
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        label: &str,
    ) -> String {
        test_evidence_v2_json(
            four_tuple,
            candidate_digest,
            label,
            &test_execution_policy_digest(),
        )
    }

    fn test_evidence_v2_json(
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        label: &str,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    ) -> String {
        serde_json::json!({
            "schema": "boole.native-shadow.evidence.v2",
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
            "policyDigest": sha256_hex(format!("checker-policy-{label}").as_bytes()),
            "executionPolicyDigest": execution_policy_digest.as_str(),
            "toolchainDigest": sha256_hex(format!("toolchain-{label}").as_bytes()),
            "verdict": "accepted",
            "reasonCode": "accepted",
            "registryVersion": "NATIVE-SHADOW-TEST-REGISTRY-V1",
        })
        .to_string()
    }

    fn test_legacy_evidence_json(
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        label: &str,
    ) -> String {
        serde_json::json!({
            "schema": "boole.native-shadow.evidence.v1",
            "submissionSchema": "boole.native-shadow.submission.v1",
            "submissionDigest": sha256_hex(format!("legacy-submission-{label}").as_bytes()),
            "familyVersion": four_tuple.family_version,
            "templateId": four_tuple.template_id,
            "anchorDigest": sha256_hex(format!("legacy-anchor-{label}").as_bytes()),
            "challengeSha256": four_tuple.challenge_sha256,
            "epoch": four_tuple.epoch,
            "candidateDigest": candidate_digest,
            "intakeVersion": "proof-intake-v1",
            "checkerDigest": sha256_hex(format!("legacy-checker-{label}").as_bytes()),
            "policyDigest": sha256_hex(format!("legacy-checker-policy-{label}").as_bytes()),
            "toolchainDigest": sha256_hex(format!("legacy-toolchain-{label}").as_bytes()),
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &mut authority,
            &four_tuple,
            "admission-consumed-exhausted",
        );
        store
            .complete_consumed(&mut authority, &mut ledger, &four_tuple, evidence)
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &mut authority,
            &four_tuple,
            "admission-missing-projection",
        );
        store
            .complete_consumed(
                &mut authority,
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut terminal_projection = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin execution");
        let evidence = persist_test_evidence(
            &mut store,
            &mut authority,
            &four_tuple,
            "admission-terminal-registry-drift",
        );
        store
            .complete_consumed(
                &mut authority,
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut store = NativeShadowStateStore::default();
        let outcome = store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        assert_eq!(
            outcome,
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
        );

        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("Active(fresh) -> InFlight must succeed");

        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::InFlight)
        );

        // The transition is durable, not just in-memory: replaying the
        // journal from disk independently must observe it too.
        let replay = replay_native_shadow_journal(&mut authority).expect("replay");
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        let err = store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
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
            replay_native_shadow_journal(&mut authority)
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();

        let err = store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("Active(fresh) -> InFlight");

        let evidence =
            persist_test_evidence(&mut store, &mut authority, &four_tuple, "complete-consumed");

        store
            .complete_consumed(&mut authority, &mut ledger, &four_tuple, evidence)
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
        let replay = replay_native_shadow_journal(&mut authority).expect("replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::Consumed)
        );
        let recovered = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut ledger = NativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        let err = store
            .complete_consumed(
                &mut authority,
                &mut ledger,
                &four_tuple,
                DurableNativeShadowEvidenceCommit {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "digest-v1".to_string(),
                    execution_policy_digest: test_execution_policy_digest(),
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut other_authority = NativeShadowJournalAuthority::open(&other_journal_path)
            .expect("other journal authority");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin");

        let err = store
            .persist_evidence(
                &mut other_authority,
                &four_tuple,
                "candidate-split",
                r#"{"schema":"boole.native-shadow.evidence.v1","verdict":"ACCEPT"}"#,
            )
            .expect_err("one lifecycle must never be split across journal files");
        assert!(matches!(
            err,
            NativeShadowTransitionError::JournalAuthorityMismatch
        ));
        assert_eq!(
            std::fs::metadata(&other_journal_path)
                .expect("other authority created its journal")
                .len(),
            0,
            "the mismatched authority must receive no record"
        );
    }

    #[test]
    fn schema_only_json_cannot_create_a_durable_evidence_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("schema-only-evidence");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin");

        let result = store.persist_evidence(
            &mut authority,
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        // Simulate a full lifecycle on one "process", then recover as if a
        // brand-new process just started against the same durable files.
        {
            let mut ledger = NativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
                .expect("begin");
            let evidence =
                persist_test_evidence(&mut store, &mut authority, &four_tuple, "recover-consumed");
            store
                .complete_consumed(&mut authority, &mut ledger, &four_tuple, evidence)
                .expect("complete");
        }

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

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

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");

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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        {
            let mut ledger = NativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
                .expect("begin");
            let evidence =
                persist_test_evidence(&mut store, &mut authority, &four_tuple, "registry-drift");
            store
                .complete_consumed(&mut authority, &mut ledger, &four_tuple, evidence)
                .expect("complete");
        }

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v2-after-restart",
            &test_execution_policy_digest(),
            &mut authority,
        )
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let first = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("first boot");
        drop(first);

        let mut second = recover_native_shadow_state(
            &registry,
            "digest-v2-after-restart",
            &test_execution_policy_digest(),
            &mut authority,
        )
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        // Crash right after Active(fresh) -> InFlight, before Consumed.
        {
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
                .expect("begin");
        }

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        {
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
                .expect("begin");
            let _durable_evidence =
                persist_test_evidence(&mut store, &mut authority, &four_tuple, "crash-gap");
            // Simulated crash: no terminal transition is attempted.
        }

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

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
            authority
                .append_line(&serde_json::to_string(&event).expect("serialize event"))
                .expect("append event");
        }

        let err = replay_native_shadow_journal(&mut authority)
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        authority
            .append_line(
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

        let err = replay_native_shadow_journal(&mut authority)
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        authority
            .append_line(
                &serde_json::to_string(&NativeShadowJournalEvent::Bootstrap {
                    four_tuple,
                    registry_digest: registry_digest.clone(),
                    state: ChallengeState::ActiveFresh,
                })
                .expect("serialize bootstrap"),
            )
            .expect("append bootstrap");

        let err = recover_native_shadow_state(
            &registry,
            &registry_digest,
            &test_execution_policy_digest(),
            &mut authority,
        )
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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin");
        let evidence =
            persist_test_evidence(&mut store, &mut authority, &four_tuple, "torn-terminal");

        let terminal = serde_json::to_string(&NativeShadowJournalEvent::TerminalConsumed {
            four_tuple: four_tuple.clone(),
            registry_digest: evidence.registry_digest,
            candidate_digest: evidence.candidate_digest,
            evidence_digest: evidence.evidence_digest,
            exhausted: true,
        })
        .expect("serialize terminal");
        drop(store);
        drop(authority);
        let mut journal = std::fs::OpenOptions::new()
            .append(true)
            .open(&journal_path)
            .expect("open journal");
        journal
            .write_all(&terminal.as_bytes()[..terminal.len() / 2])
            .expect("write torn tail");
        journal.sync_all().expect("sync torn tail");
        drop(journal);

        let mut authority = NativeShadowJournalAuthority::open(&journal_path)
            .expect("reopened authority after simulated crash");

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
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
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("evidence-write-failure");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin");

        authority.fail_next_append_for_test();
        let candidate_digest = sha256_hex(b"candidate-write-failure");
        let evidence_json = test_evidence_json(&four_tuple, &candidate_digest, "write-failure");
        let result = store.persist_evidence(
            &mut authority,
            &four_tuple,
            &candidate_digest,
            &evidence_json,
        );

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
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");

        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("recover");
        assert!(recovery.stuck_in_flight.is_empty());
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Disabled)
        );
    }

    // -- Phase 3A.1: one lifetime-held, flocked journal descriptor --------

    #[cfg(unix)]
    #[test]
    fn second_journal_authority_is_refused_until_the_first_is_dropped() {
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("authority-exclusive-lock");

        let first = NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
        let second = NativeShadowJournalAuthority::open(&journal_path)
            .expect_err("a second writer must fail immediately");
        assert!(matches!(
            second,
            NativeShadowJournalAuthorityError::Locked(_)
        ));

        drop(first);
        NativeShadowJournalAuthority::open(&journal_path)
            .expect("dropping the first authority releases the flock");
    }

    #[cfg(unix)]
    #[test]
    fn journal_authority_rejects_a_symlink_or_non_regular_final_component() {
        use std::os::unix::fs::symlink;

        let (journal_path, other_path) =
            scratch_journal_and_exhaustion_paths("authority-final-component");
        std::fs::write(&other_path, b"target\n").expect("target");
        symlink(&other_path, &journal_path).expect("symlink");
        assert!(matches!(
            NativeShadowJournalAuthority::open(&journal_path),
            Err(NativeShadowJournalAuthorityError::UnsafePath(_))
        ));

        std::fs::remove_file(&journal_path).expect("remove symlink");
        std::fs::create_dir(&journal_path).expect("directory at final component");
        assert!(matches!(
            NativeShadowJournalAuthority::open(&journal_path),
            Err(NativeShadowJournalAuthorityError::UnsafePath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn path_replacement_cannot_redirect_an_authoritative_append() {
        let (journal_path, displaced_path) =
            scratch_journal_and_exhaustion_paths("authority-path-replacement");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        authority.append_line("first").expect("first append");

        std::fs::rename(&journal_path, &displaced_path).expect("move locked inode");
        std::fs::write(&journal_path, b"replacement\n").expect("replacement inode");

        assert!(matches!(
            authority.append_line("must-not-land"),
            Err(NativeShadowJournalAuthorityError::PathIdentityChanged(_))
        ));
        assert!(matches!(
            authority.append_line("still-must-not-land"),
            Err(NativeShadowJournalAuthorityError::Poisoned(_))
        ));
        assert_eq!(
            std::fs::read_to_string(&displaced_path).expect("locked inode"),
            "first\n"
        );
        assert_eq!(
            std::fs::read_to_string(&journal_path).expect("replacement inode"),
            "replacement\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_replacement_cannot_redirect_authoritative_replay() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let (journal_path, displaced_path) =
            scratch_journal_and_exhaustion_paths("authority-replay-path-replacement");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        authority
            .append_line(
                &serde_json::to_string(&NativeShadowJournalEvent::Bootstrap {
                    four_tuple,
                    registry_digest: "digest-v1".to_string(),
                    state: ChallengeState::ActiveFresh,
                })
                .expect("serialize bootstrap"),
            )
            .expect("seed authoritative journal");
        let authoritative_before = std::fs::read(&journal_path).expect("authoritative bytes");

        std::fs::rename(&journal_path, &displaced_path).expect("move locked inode");
        std::fs::write(&journal_path, b"replacement-torn-tail").expect("replacement inode");
        let replacement_before = std::fs::read(&journal_path).expect("replacement bytes");

        let err = replay_native_shadow_journal(&mut authority)
            .expect_err("replay must reject a replaced authority path");
        assert!(err.to_string().contains("changed identity while locked"));
        assert_eq!(
            std::fs::read(&displaced_path).expect("locked inode after replay refusal"),
            authoritative_before
        );
        assert_eq!(
            std::fs::read(&journal_path).expect("replacement inode after replay refusal"),
            replacement_before
        );

        let second = replay_native_shadow_journal(&mut authority)
            .expect_err("a replaced authority remains fail-closed");
        assert!(second.to_string().contains("authority is fail-closed"));
    }

    #[cfg(unix)]
    #[test]
    fn one_state_store_rejects_a_different_live_authority() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, other_path) =
            scratch_journal_and_exhaustion_paths("store-authority-binding");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
        let mut other_authority =
            NativeShadowJournalAuthority::open(&other_path).expect("other authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("bind first authority");
        let first_before = std::fs::read(&journal_path).expect("first journal");
        let other_before = std::fs::read(&other_path).expect("other journal");

        let candidate_digest = sha256_hex(b"authority-mismatch");
        let evidence_json =
            test_evidence_json(&four_tuple, &candidate_digest, "authority-mismatch");
        assert!(matches!(
            store.persist_evidence(
                &mut other_authority,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            ),
            Err(NativeShadowTransitionError::JournalAuthorityMismatch)
        ));
        assert_eq!(
            std::fs::read(&journal_path).expect("first journal after refusal"),
            first_before
        );
        assert_eq!(
            std::fs::read(&other_path).expect("other journal after refusal"),
            other_before
        );
    }

    #[cfg(unix)]
    #[test]
    fn reopening_the_same_path_cannot_continue_an_old_state_store() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("authority-reopen-binding");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("bind first authority");
        let before = std::fs::read(&journal_path).expect("journal before reopen");
        drop(authority);

        let mut reopened =
            NativeShadowJournalAuthority::open(&journal_path).expect("reopened authority");
        let candidate_digest = sha256_hex(b"reopened-authority");
        let evidence_json =
            test_evidence_json(&four_tuple, &candidate_digest, "reopened-authority");
        assert!(matches!(
            store.persist_evidence(
                &mut reopened,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            ),
            Err(NativeShadowTransitionError::JournalAuthorityMismatch)
        ));
        assert_eq!(
            std::fs::read(&journal_path).expect("journal after refusal"),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn one_locked_descriptor_replays_truncates_and_appends_the_full_lifecycle() {
        use std::io::Write as _;

        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("authority-full-lifecycle");
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&journal_path)
            .expect("seed torn journal")
            .write_all(b"torn-without-newline")
            .expect("write torn tail");

        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery = recover_native_shadow_state(
            &registry,
            "digest-v1",
            &test_execution_policy_digest(),
            &mut authority,
        )
        .expect("replay and bootstrap through held descriptor");
        let after_recovery =
            std::fs::read_to_string(&journal_path).expect("journal after recovery");
        assert!(!after_recovery.contains("torn-without-newline"));
        assert_eq!(after_recovery.lines().count(), 1);
        assert!(after_recovery.contains(r#""kind":"bootstrap_v2""#));
        assert!(matches!(
            NativeShadowJournalAuthority::open(&journal_path),
            Err(NativeShadowJournalAuthorityError::Locked(_))
        ));

        recovery
            .store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin");
        let candidate_digest = sha256_hex(b"authority-full-lifecycle");
        let evidence_json =
            test_evidence_json(&four_tuple, &candidate_digest, "authority-full-lifecycle");
        let evidence = recovery
            .store
            .persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            )
            .expect("evidence");
        recovery
            .store
            .complete_consumed(
                &mut authority,
                &mut recovery.exhaustion_ledger,
                &four_tuple,
                evidence,
            )
            .expect("terminal");

        let replay = replay_native_shadow_journal(&mut authority).expect("same-fd replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::Consumed)
        );
        assert!(replay.exhausted.contains(&four_tuple));
        assert!(matches!(
            NativeShadowJournalAuthority::open(&journal_path),
            Err(NativeShadowJournalAuthorityError::Locked(_))
        ));
    }

    // -- Phase 3A.2: route-free non-blocking single-slot primitive ---------

    #[test]
    fn native_execution_gate_rejects_every_concurrent_arrival_immediately() {
        let gate = std::sync::Arc::new(NativeShadowExecutionGate::new());
        let _first = gate.try_acquire().expect("first execution owns the slot");

        assert_eq!(NativeShadowBusy.reason_code(), "native_busy");
        assert_eq!(NativeShadowBusy.to_string(), "native_busy");
        assert!(matches!(gate.try_acquire(), Err(NativeShadowBusy)));
        assert!(matches!(gate.try_acquire(), Err(NativeShadowBusy)));
    }

    #[test]
    fn native_execution_permit_releases_on_normal_error_and_panic_paths() {
        let gate = std::sync::Arc::new(NativeShadowExecutionGate::new());

        drop(gate.try_acquire().expect("normal-path permit"));
        drop(gate.try_acquire().expect("normal drop releases the slot"));

        fn fail_after_acquire(
            gate: &std::sync::Arc<NativeShadowExecutionGate>,
        ) -> Result<(), &'static str> {
            let _permit = gate.try_acquire().map_err(|_| "busy")?;
            Err("checker failed")
        }
        assert_eq!(fail_after_acquire(&gate), Err("checker failed"));
        drop(gate.try_acquire().expect("error return releases the slot"));

        let panic_gate = std::sync::Arc::clone(&gate);
        let unwind = std::panic::catch_unwind(move || {
            let _permit = panic_gate.try_acquire().expect("panic-path permit");
            panic!("simulated checker panic");
        });
        assert!(unwind.is_err());
        drop(gate.try_acquire().expect("panic unwind releases the slot"));
    }

    #[test]
    fn native_execution_gate_has_exactly_one_winner_under_thread_contention() {
        const CONTENDERS: usize = 12;
        let gate = std::sync::Arc::new(NativeShadowExecutionGate::new());
        let start = std::sync::Arc::new(std::sync::Barrier::new(CONTENDERS + 1));
        let release =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
        let mut threads = Vec::new();

        for _ in 0..CONTENDERS {
            let gate = std::sync::Arc::clone(&gate);
            let start = std::sync::Arc::clone(&start);
            let release = std::sync::Arc::clone(&release);
            let attempt_tx = attempt_tx.clone();
            threads.push(std::thread::spawn(move || {
                start.wait();
                match gate.try_acquire() {
                    Ok(permit) => {
                        attempt_tx.send(true).expect("report winner");
                        let (lock, wake) = &*release;
                        let mut released = lock.lock().expect("release mutex");
                        while !*released {
                            released = wake.wait(released).expect("release wait");
                        }
                        drop(permit);
                    }
                    Err(NativeShadowBusy) => {
                        attempt_tx.send(false).expect("report busy");
                    }
                }
            }));
        }
        drop(attempt_tx);
        start.wait();

        let attempts: Vec<bool> = (0..CONTENDERS)
            .map(|_| attempt_rx.recv().expect("one report per contender"))
            .collect();
        assert_eq!(attempts.into_iter().filter(|won| *won).count(), 1);

        let (lock, wake) = &*release;
        *lock.lock().expect("release mutex") = true;
        wake.notify_all();
        for thread in threads {
            thread.join().expect("contender thread");
        }
        drop(gate.try_acquire().expect("winner drop releases the slot"));
    }

    #[test]
    fn route_free_busy_ordering_fixture_keeps_state_and_journal_untouched() {
        let gate = std::sync::Arc::new(NativeShadowExecutionGate::new());
        let _running = gate.try_acquire().expect("existing native execution");
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("native-busy-no-mutation");
        let _authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let journal_before = std::fs::read(&journal_path).expect("journal before busy arrival");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template)
        });

        assert!(matches!(gate.try_acquire(), Err(NativeShadowBusy)));
        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row exists")),
            ResolveOutcome::Existing(ChallengeState::ActiveFresh)
        );
        assert_eq!(
            std::fs::read(&journal_path).expect("journal after busy arrival"),
            journal_before
        );
    }

    #[test]
    fn native_execution_permit_is_raii_and_worker_handoff_safe() {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<NativeShadowExecutionGate>();
        assert_send::<NativeShadowExecutionPermit>();
        assert!(std::mem::needs_drop::<NativeShadowExecutionPermit>());
    }

    // -- Phase 3B.0: execution-policy identity is distinct from checker policy

    #[test]
    fn execution_policy_digest_is_a_validated_lower_sha256_identity() {
        let digest = sha256_hex(b"node-owned-containment-policy-v1");
        let parsed = NativeShadowExecutionPolicyDigest::try_from(digest.as_str())
            .expect("lowercase SHA-256 digest");
        assert_eq!(parsed.as_str(), digest);

        for invalid in ["short", &"A".repeat(64), &"g".repeat(64)] {
            assert!(
                NativeShadowExecutionPolicyDigest::try_from(invalid).is_err(),
                "{invalid:?} is not a lowercase SHA-256 digest"
            );
        }
    }

    #[test]
    fn execution_policy_drift_preserves_the_original_state_row() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let first = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"execution-policy-first").as_str(),
        )
        .expect("first policy digest");
        let changed = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"execution-policy-changed").as_str(),
        )
        .expect("changed policy digest");
        let mut store = NativeShadowStateStore::default();

        assert_eq!(
            store.resolve_with_execution_policy(&four_tuple, "registry-digest", &first, || {
                ChallengeState::ActiveFresh
            },),
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
        );
        assert_eq!(
            store.resolve_with_execution_policy(
                &four_tuple,
                "registry-digest",
                &changed,
                || panic!("policy drift must not bootstrap a replacement row"),
            ),
            ResolveOutcome::ExecutionPolicyDrift {
                state: ChallengeState::ActiveFresh
            }
        );
        assert_eq!(
            store
                .rows
                .get(&four_tuple)
                .and_then(|row| row.execution_policy_digest.as_ref()),
            Some(&first),
            "policy drift must leave the original row binding untouched"
        );
    }

    #[test]
    fn begin_execution_durably_binds_execution_policy_before_state_change() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let policy = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"execution-policy-v1").as_str(),
        )
        .expect("execution policy digest");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("execution-policy-in-flight-v2");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve_with_execution_policy(&four_tuple, "registry-digest", &policy, || {
            ChallengeState::ActiveFresh
        });

        store
            .begin_execution(&mut authority, &four_tuple, &policy)
            .expect("v2 execution begins durably");

        let journal = std::fs::read_to_string(&journal_path).expect("journal");
        assert!(journal.contains(r#""kind":"bootstrap_v2""#));
        assert!(journal.contains(r#""kind":"in_flight_v2""#));
        assert!(journal.contains(&format!(r#""executionPolicyDigest":"{}""#, policy.as_str())));
        assert_eq!(
            store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::InFlight)
        );
    }

    #[test]
    fn persist_evidence_writes_v2_with_the_rows_execution_policy_binding() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let policy = test_execution_policy_digest();
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("execution-policy-evidence-v2");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve_with_execution_policy(&four_tuple, "registry-digest", &policy, || {
            ChallengeState::ActiveFresh
        });
        store
            .begin_execution(&mut authority, &four_tuple, &policy)
            .expect("begin execution");
        let candidate_digest = sha256_hex(b"v2-candidate");
        let evidence_json = test_evidence_v2_json(&four_tuple, &candidate_digest, "v2", &policy);

        store
            .persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            )
            .expect("v2 evidence must persist");

        let journal = std::fs::read_to_string(&journal_path).expect("journal");
        assert!(journal.contains(r#""kind":"evidence_v2""#));
        assert!(journal.contains(&format!(r#""executionPolicyDigest":"{}""#, policy.as_str())));
    }

    #[test]
    fn recovery_bootstraps_new_rows_with_the_current_execution_policy() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let changed = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"changed-after-recovery").as_str(),
        )
        .expect("changed policy digest");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("recovery-execution-policy-v2");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");

        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");

        assert_eq!(
            recovery.store.resolve_with_execution_policy(
                &four_tuple,
                "registry-digest",
                &changed,
                || panic!("durable row exists"),
            ),
            ResolveOutcome::ExecutionPolicyDrift {
                state: ChallengeState::ActiveFresh
            }
        );
        assert!(std::fs::read_to_string(&journal_path)
            .expect("journal")
            .contains(r#""kind":"bootstrap_v2""#));
    }

    #[test]
    fn admission_reports_execution_policy_drift_without_mutating_the_row() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let changed = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"admission-policy-drift").as_str(),
        )
        .expect("changed policy digest");
        let mut store = NativeShadowStateStore::default();
        store.resolve_with_execution_policy(&four_tuple, "registry-digest", &policy, || {
            ChallengeState::ActiveFresh
        });

        assert_eq!(
            store.admission_view_with_execution_policy(
                &four_tuple,
                "registry-digest",
                &changed,
                &NativeShadowExhaustionLedger::default(),
            ),
            Err(NativeShadowAdmissionError::ExecutionPolicyDrift {
                state: ChallengeState::ActiveFresh
            })
        );
        assert_eq!(
            store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
    }

    #[test]
    fn replay_preserves_a_complete_legacy_v1_lifecycle_read_only() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let candidate_digest = sha256_hex(b"legacy-candidate");
        let evidence_json = test_legacy_evidence_json(&four_tuple, &candidate_digest, "complete");
        let legacy_evidence: NativeShadowEvidence =
            serde_json::from_str(&evidence_json).expect("legacy evidence");
        assert!(
            serde_json::to_value(&legacy_evidence)
                .expect("serialize legacy evidence")
                .get("executionPolicyDigest")
                .is_none(),
            "v1 compatibility must not synthesize a null v2-only field"
        );
        let evidence_digest = sha256_hex(evidence_json.as_bytes());
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("legacy-v1-readable");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        for event in [
            NativeShadowJournalEvent::Bootstrap {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlight {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
            },
            NativeShadowJournalEvent::Evidence {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                candidate_digest: candidate_digest.clone(),
                evidence_digest: evidence_digest.clone(),
                evidence_json,
            },
            NativeShadowJournalEvent::TerminalConsumed {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                candidate_digest,
                evidence_digest,
                exhausted: true,
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("serialize legacy event"))
                .expect("append legacy event");
        }

        let replay = replay_native_shadow_journal(&mut authority).expect("legacy replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::Consumed)
        );
        assert_eq!(
            replay.execution_policy_digests.get(&four_tuple),
            Some(&None),
            "legacy rows remain explicitly unbound and cannot start a v2 execution"
        );
    }

    #[test]
    fn replay_rejects_legacy_v2_mixing_and_execution_policy_mismatch() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let first = test_execution_policy_digest();
        let changed =
            NativeShadowExecutionPolicyDigest::try_from(sha256_hex(b"mixed-policy").as_str())
                .expect("changed policy");

        for (label, second) in [
            (
                "legacy-after-v2",
                NativeShadowJournalEvent::InFlight {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                },
            ),
            (
                "mismatched-v2",
                NativeShadowJournalEvent::InFlightV2 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: changed,
                },
            ),
        ] {
            let (journal_path, _other_path) = scratch_journal_and_exhaustion_paths(label);
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("authority");
            let bootstrap = NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: first.clone(),
                state: ChallengeState::ActiveFresh,
            };
            authority
                .append_line(&serde_json::to_string(&bootstrap).expect("bootstrap"))
                .expect("append bootstrap");
            authority
                .append_line(&serde_json::to_string(&second).expect("second"))
                .expect("append second");

            let err = replay_native_shadow_journal(&mut authority)
                .expect_err("mixed or mismatched v2 lifecycle must fail closed");
            assert!(
                err.to_string().contains("mixes legacy and v2")
                    || err.to_string().contains("changes executionPolicyDigest"),
                "unexpected replay failure: {err}"
            );
        }
    }

    #[test]
    fn replay_rejects_v2_evidence_with_missing_or_mismatched_execution_policy() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let changed = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"evidence-policy-mismatch").as_str(),
        )
        .expect("changed policy");
        let candidate_digest = sha256_hex(b"replay-v2-candidate");
        let mut mismatched = serde_json::from_str::<serde_json::Value>(&test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "mismatch",
            &changed,
        ))
        .expect("evidence value");
        let mut missing = mismatched.clone();
        missing
            .as_object_mut()
            .expect("evidence object")
            .remove("executionPolicyDigest");

        for (label, evidence_json) in [
            ("v2-evidence-missing-policy", missing.to_string()),
            (
                "v2-evidence-mismatched-policy",
                mismatched.take().to_string(),
            ),
        ] {
            let (journal_path, _other_path) = scratch_journal_and_exhaustion_paths(label);
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("authority");
            for event in [
                NativeShadowJournalEvent::BootstrapV2 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: policy.clone(),
                    state: ChallengeState::ActiveFresh,
                },
                NativeShadowJournalEvent::InFlightV2 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: policy.clone(),
                },
                NativeShadowJournalEvent::EvidenceV2 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: policy.clone(),
                    candidate_digest: candidate_digest.clone(),
                    evidence_digest: sha256_hex(evidence_json.as_bytes()),
                    evidence_json: evidence_json.clone(),
                },
            ] {
                authority
                    .append_line(&serde_json::to_string(&event).expect("event"))
                    .expect("append event");
            }

            let err = replay_native_shadow_journal(&mut authority)
                .expect_err("v2 evidence policy omission/mismatch must fail closed");
            assert!(
                err.to_string()
                    .contains("evidence executionPolicyDigest binding mismatch"),
                "unexpected replay failure: {err}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn begin_execution_durability_failure_does_not_change_lifecycle_state() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("execution-policy-begin-write-failure");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve_with_execution_policy(&four_tuple, "registry-digest", &policy, || {
            ChallengeState::ActiveFresh
        });
        authority.fail_next_append_for_test();

        assert!(matches!(
            store.begin_execution(&mut authority, &four_tuple, &policy),
            Err(NativeShadowTransitionError::Durability(_))
        ));
        assert_eq!(
            store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh),
            "state cannot enter InFlight until its v2 journal event is durable"
        );
        assert_eq!(std::fs::metadata(&journal_path).expect("journal").len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn durable_v2_bootstrap_survives_a_later_in_flight_append_failure() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("execution-policy-in-flight-write-failure");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");

        // Recovery durably bootstraps every registry row before returning it.
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("durable v2 bootstrap");
        let bootstrap_bytes = std::fs::read(&journal_path).expect("bootstrap bytes");
        authority.fail_next_append_for_test();

        assert!(matches!(
            recovery
                .store
                .begin_execution(&mut authority, &four_tuple, &policy),
            Err(NativeShadowTransitionError::Durability(_))
        ));
        let row = recovery.store.rows.get(&four_tuple).expect("row");
        assert_eq!(row.state, ChallengeState::ActiveFresh);
        assert_eq!(row.execution_policy_digest.as_ref(), Some(&policy));
        assert_eq!(
            std::fs::read(&journal_path).expect("journal after refusal"),
            bootstrap_bytes,
            "a failed InFlightV2 append must preserve the durable BootstrapV2 prefix exactly"
        );
    }

    #[test]
    fn replayed_legacy_rows_cannot_be_upgraded_by_a_new_execution_write() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();

        // A legacy ActiveFresh row may be inspected, but a new execution must
        // not silently graft a v2 policy binding onto its durable lifecycle.
        let (active_path, _other_path) =
            scratch_journal_and_exhaustion_paths("legacy-active-new-execution-refused");
        let mut active_authority =
            NativeShadowJournalAuthority::open(&active_path).expect("active authority");
        active_authority
            .append_line(
                &serde_json::to_string(&NativeShadowJournalEvent::Bootstrap {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    state: ChallengeState::ActiveFresh,
                })
                .expect("legacy bootstrap"),
            )
            .expect("append legacy bootstrap");
        let active_before = std::fs::read(&active_path).expect("active legacy bytes");
        let mut active_recovery = recover_native_shadow_state(
            &registry,
            "registry-digest",
            &policy,
            &mut active_authority,
        )
        .expect("recover legacy active row");
        assert!(matches!(
            active_recovery
                .store
                .begin_execution(&mut active_authority, &four_tuple, &policy),
            Err(NativeShadowTransitionError::ExecutionPolicyDrift(_))
        ));
        assert_eq!(
            std::fs::read(&active_path).expect("active journal after refusal"),
            active_before
        );

        // The same rule applies after a legacy InFlight marker: v2 evidence
        // cannot be appended to complete a lifecycle started under v1.
        let (flight_path, _other_path) =
            scratch_journal_and_exhaustion_paths("legacy-flight-v2-evidence-refused");
        let mut flight_authority =
            NativeShadowJournalAuthority::open(&flight_path).expect("flight authority");
        for event in [
            NativeShadowJournalEvent::Bootstrap {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlight {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
            },
        ] {
            flight_authority
                .append_line(&serde_json::to_string(&event).expect("legacy event"))
                .expect("append legacy event");
        }
        let flight_before = std::fs::read(&flight_path).expect("flight legacy bytes");
        let mut flight_recovery = recover_native_shadow_state(
            &registry,
            "registry-digest",
            &policy,
            &mut flight_authority,
        )
        .expect("recover legacy in-flight row");
        let candidate_digest = sha256_hex(b"legacy-flight-candidate");
        let evidence_json =
            test_evidence_v2_json(&four_tuple, &candidate_digest, "legacy-flight", &policy);
        assert!(matches!(
            flight_recovery.store.persist_evidence(
                &mut flight_authority,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            ),
            Err(NativeShadowTransitionError::ExecutionPolicyDrift(_))
        ));
        assert_eq!(
            std::fs::read(&flight_path).expect("flight journal after refusal"),
            flight_before
        );
    }

    #[test]
    fn terminal_capability_cannot_cross_an_execution_policy_binding() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let wrong_policy = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"wrong-terminal-policy").as_str(),
        )
        .expect("wrong policy");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("terminal-policy-binding");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut store = NativeShadowStateStore::default();
        let mut ledger = NativeShadowExhaustionLedger::default();
        store.resolve_with_execution_policy(&four_tuple, "registry-digest", &policy, || {
            ChallengeState::ActiveFresh
        });
        store
            .begin_execution(&mut authority, &four_tuple, &policy)
            .expect("begin");
        let mut evidence =
            persist_test_evidence(&mut store, &mut authority, &four_tuple, "terminal-binding");
        evidence.execution_policy_digest = wrong_policy;

        assert!(matches!(
            store.complete_consumed(&mut authority, &mut ledger, &four_tuple, evidence,),
            Err(NativeShadowTransitionError::EvidenceBindingMismatch(_))
        ));
        assert_eq!(
            store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::InFlight)
        );
        assert!(!ledger.contains(&four_tuple));
    }
}
