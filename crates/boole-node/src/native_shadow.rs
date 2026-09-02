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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use boole_core::hash::{h_protocol, Hex32};
use boole_core::useful_product::{ReceiptRejectReason, ReceiptVerdict, VerificationReceipt};
use boole_core::useful_work_bf6::NativeTaskIdentity;
use boole_native_shadow_protocol::{
    verify_authority_bundle, VerifiedClosedLocalReplayAuthorization,
    VerifiedClosedLocalReplayGrant, INSTALLED_REGISTRY_PATH, TRACKED_EXECUTION_POLICY_BYTES,
    TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
};

use crate::durability::{
    append_ndjson_line_durable_on_file, fsync_parent_dir, read_stable_prefix_on_file,
};
use crate::state_dir::flock_exclusive_nonblocking;

const NATIVE_BF3_ARTIFACT_DOMAIN: &[u8] = b"boole.native-shadow.bf3-artifact.v1";
pub(crate) const PRODUCTION_NATIVE_SHADOW_JOURNAL_PATH: &str =
    "/var/lib/boole/native-shadow/node-state/replay-v1.ndjson";

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

/// Opaque node capability allowing only the frozen closed-local replay
/// overlay to create one active row despite the production registry remaining
/// permanently non-issuable. Production construction is possible only from
/// the protocol grant's exact authorization.
#[derive(Debug)]
pub(crate) struct VerifiedNativeShadowReplayBootstrap {
    four_tuple: NativeShadowFourTuple,
    registry_version: String,
    registry_digest: String,
    execution_policy_digest: NativeShadowExecutionPolicyDigest,
}

impl VerifiedNativeShadowReplayBootstrap {
    pub(crate) fn from_authorization(
        authorization: &VerifiedClosedLocalReplayAuthorization,
    ) -> Result<Self, String> {
        Ok(Self {
            four_tuple: NativeShadowFourTuple {
                family_version: authorization.family_version().to_string(),
                template_id: authorization.template_id().to_string(),
                challenge_sha256: authorization.challenge_sha256().to_string(),
                epoch: authorization.epoch(),
            },
            registry_version: authorization.registry_version().to_string(),
            registry_digest: authorization.registry_digest_hex().to_string(),
            execution_policy_digest: NativeShadowExecutionPolicyDigest::try_from(
                authorization.execution_policy_digest_hex(),
            )?,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        four_tuple: NativeShadowFourTuple,
        registry_version: &str,
        registry_digest: String,
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
    ) -> Self {
        Self {
            four_tuple,
            registry_version: registry_version.to_string(),
            registry_digest,
            execution_policy_digest,
        }
    }

    pub(crate) fn four_tuple(&self) -> &NativeShadowFourTuple {
        &self.four_tuple
    }

    pub(crate) fn registry_version(&self) -> &str {
        &self.registry_version
    }

    pub(crate) fn registry_digest(&self) -> &str {
        &self.registry_digest
    }

    pub(crate) fn execution_policy_digest(&self) -> &NativeShadowExecutionPolicyDigest {
        &self.execution_policy_digest
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

/// Lifecycle projection of one registry template. Production bytes first pass
/// the shared full strict authority schema and are then explicitly reduced to
/// these state-machine fields. Unit-only lifecycle fixtures keep using this
/// smaller model so they cannot be mistaken for installed authority bytes.
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
    Authority(String),
    UnsafeProductionPath(String),
}

impl std::fmt::Display for NativeShadowRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "native-shadow registry read failed: {err}"),
            Self::Authority(err) => {
                write!(f, "native-shadow registry authority invalid: {err}")
            }
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

fn load_native_shadow_registry_from_bytes(
    raw: &[u8],
) -> Result<LoadedNativeShadowRegistry, NativeShadowRegistryError> {
    let verified = verify_authority_bundle(
        raw,
        TRACKED_EXECUTION_POLICY_BYTES,
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    )
    .map_err(|error| NativeShadowRegistryError::Authority(error.to_string()))?;
    let registry = NativeShadowRegistry {
        schema: verified.registry().schema().to_string(),
        activation_allowed: verified.registry().activation_allowed(),
        templates: verified
            .registry()
            .templates()
            .iter()
            .map(|template| NativeShadowTemplate {
                family_version: template.family_version().to_string(),
                template_id: template.template_id().to_string(),
                challenge_sha256: template.challenge_sha256().to_string(),
                epoch: template.epoch(),
                non_issuable: template.non_issuable(),
            })
            .collect(),
    };
    Ok(LoadedNativeShadowRegistry {
        registry,
        registry_digest: verified.registry_digest().to_string(),
    })
}

/// Load the only production authority this qualification path is allowed to
/// use. The caller supplies no path: the installed root-owned absolute
/// location is fixed at build time, so changing the process CWD or renaming a
/// repository fixture cannot redirect production loading. The final component
/// is opened once with `O_NOFOLLOW`; metadata and bytes come from that same FD.
fn validate_production_authority_metadata(
    is_regular_file: bool,
    link_count: u64,
    owner_uid: u32,
    owner_gid: u32,
    permission_bits: u32,
    byte_len: u64,
) -> Result<(), NativeShadowRegistryError> {
    if is_regular_file
        && link_count == 1
        && owner_uid == 0
        && owner_gid == 0
        && permission_bits == 0o444
        && byte_len == TRACKED_REGISTRY_BYTES.len() as u64
    {
        return Ok(());
    }

    Err(NativeShadowRegistryError::UnsafeProductionPath(
        "authority must be a root:root 0444 regular one-link non-symlink file with the tracked byte length"
            .to_string(),
    ))
}

pub(crate) fn load_production_native_shadow_registry(
) -> Result<LoadedNativeShadowRegistry, NativeShadowRegistryError> {
    let path = Path::new(PRODUCTION_REGISTRY_PATH);
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(NativeShadowRegistryError::Io)?;
    let metadata = file.metadata().map_err(NativeShadowRegistryError::Io)?;
    validate_production_authority_metadata(
        metadata.file_type().is_file(),
        metadata.nlink(),
        metadata.uid(),
        metadata.gid(),
        metadata.mode() & 0o7777,
        metadata.len(),
    )?;
    let mut raw = Vec::new();
    file.read_to_end(&mut raw)
        .map_err(NativeShadowRegistryError::Io)?;
    load_native_shadow_registry_from_bytes(&raw)
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
pub(crate) const PRODUCTION_REGISTRY_PATH: &str = INSTALLED_REGISTRY_PATH;

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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NativeShadowReceiptMapError {
    #[error("native-shadow BF3 mapping requires matching durable v2 evidence")]
    DurableEvidenceBindingMismatch,
    #[error("native-shadow BF3 mapping received an invalid digest in {0}")]
    InvalidDigest(&'static str),
    #[error("native-shadow BF3 mapping received an invalid verdict/reason pair")]
    InvalidVerdictReason,
}

fn bf3_root(domain: &[u8], fields: &[&[u8]]) -> Hex32 {
    let mut canonical = Vec::new();
    for field in fields {
        // Match the common BF task/product framing: u64 little-endian byte
        // length followed by the exact typed field bytes.
        canonical.extend_from_slice(&(field.len() as u64).to_le_bytes());
        canonical.extend_from_slice(field);
    }
    h_protocol(domain, &[&canonical])
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
        if !valid_evidence_verdict_reason(self.verdict, &self.reason_code) {
            return Err("evidence verdict/reason pair is invalid".to_string());
        }
        Ok(())
    }
}

fn valid_evidence_verdict_reason(verdict: NativeShadowEvidenceVerdict, reason_code: &str) -> bool {
    match verdict {
        NativeShadowEvidenceVerdict::Accepted => reason_code == "accepted",
        NativeShadowEvidenceVerdict::DeterministicReject => matches!(
            reason_code,
            "compile_or_hidden_test_failed"
                | "forbidden_construct"
                | "malformed_patch_region"
                | "outside_patch_modified"
                | "patch_line_limit_exceeded"
                | "patch_size_exceeded"
                | "submission_unreadable"
                | "checker_rejected"
                | "submission_resource_ceiling_breach"
                | "checker_reported_reason_unconfirmed"
        ),
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

/// Frozen closed-local matrix attempt classes. The three checker rows and
/// the one proof-intake-only row share one durable four-attempt budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeShadowGrantAttemptKindV1 {
    PreIntake,
    Checker,
}

/// Borrowed node-owned fields for one durable matrix-attempt reservation.
/// Reservation is deliberately separate from grant authorization so a crash
/// cannot reopen the in-memory one-shot budget.
pub(crate) struct NativeShadowGrantAttemptFieldsV1<'a> {
    pub(crate) four_tuple: &'a NativeShadowFourTuple,
    pub(crate) registry_digest: &'a str,
    pub(crate) execution_policy_digest: &'a NativeShadowExecutionPolicyDigest,
    pub(crate) operation_id_hex: &'a str,
    pub(crate) candidate_digest: &'a str,
    pub(crate) submission_digest: &'a str,
    pub(crate) kind: NativeShadowGrantAttemptKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeShadowGrantAttemptRecordV1 {
    four_tuple: NativeShadowFourTuple,
    registry_digest: String,
    execution_policy_digest: NativeShadowExecutionPolicyDigest,
    operation_id_hex: String,
    candidate_digest: String,
    submission_digest: String,
    kind: NativeShadowGrantAttemptKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeShadowGrantAttemptScopeV1 {
    family_version: String,
    template_id: String,
    challenge_sha256: String,
    registry_digest: String,
    execution_policy_digest: NativeShadowExecutionPolicyDigest,
}

#[derive(Debug)]
#[must_use]
pub(crate) struct DurableNativeShadowGrantAttemptV1 {
    record: NativeShadowGrantAttemptRecordV1,
    journal_authority_id: u64,
}

impl DurableNativeShadowGrantAttemptV1 {
    pub(crate) fn four_tuple(&self) -> &NativeShadowFourTuple {
        &self.record.four_tuple
    }

    pub(crate) fn operation_id_hex(&self) -> &str {
        &self.record.operation_id_hex
    }

    pub(crate) fn candidate_digest(&self) -> &str {
        &self.record.candidate_digest
    }

    pub(crate) fn submission_digest(&self) -> &str {
        &self.record.submission_digest
    }

    pub(crate) fn kind(&self) -> NativeShadowGrantAttemptKindV1 {
        self.record.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeShadowGrantAttemptErrorV1 {
    InvalidDigest(&'static str),
    ScopeDrift,
    CaseAlreadyReserved,
    OperationAlreadyReserved,
    TotalBudgetExceeded,
    CheckerBudgetExceeded,
    Durability(String),
}

impl std::fmt::Display for NativeShadowGrantAttemptErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDigest(field) => write!(f, "invalid grant-attempt digest: {field}"),
            Self::ScopeDrift => write!(f, "grant-attempt scope drifted"),
            Self::CaseAlreadyReserved => write!(f, "grant case was already reserved"),
            Self::OperationAlreadyReserved => write!(f, "grant operation was already reserved"),
            Self::TotalBudgetExceeded => write!(f, "grant four-attempt budget exhausted"),
            Self::CheckerBudgetExceeded => write!(f, "grant three-checker budget exhausted"),
            Self::Durability(reason) => write!(f, "grant-attempt durable write failed: {reason}"),
        }
    }
}

impl std::error::Error for NativeShadowGrantAttemptErrorV1 {}

/// Replayable projection of the fixed 4/3/1 matrix budget. A row is inserted
/// only after its journal line is fsynced, and replay applies the same caps.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowGrantAttemptLedgerV1 {
    scope: Option<NativeShadowGrantAttemptScopeV1>,
    records: HashMap<NativeShadowFourTuple, NativeShadowGrantAttemptRecordV1>,
    operation_ids: HashSet<String>,
    checker_attempts: usize,
}

impl NativeShadowGrantAttemptLedgerV1 {
    pub(crate) fn total_attempts(&self) -> usize {
        self.records.len()
    }

    pub(crate) fn checker_attempts(&self) -> usize {
        self.checker_attempts
    }

    pub(crate) fn matches_flight(
        &self,
        four_tuple: &NativeShadowFourTuple,
        operation_id_hex: &str,
        candidate_digest: &str,
    ) -> bool {
        self.records.get(four_tuple).is_some_and(|record| {
            record.kind == NativeShadowGrantAttemptKindV1::Checker
                && record.operation_id_hex == operation_id_hex
                && record.candidate_digest == candidate_digest
        })
    }

    fn matches_evidence(
        &self,
        four_tuple: &NativeShadowFourTuple,
        candidate_digest: &str,
        submission_digest: &str,
    ) -> bool {
        self.records.get(four_tuple).is_some_and(|record| {
            record.kind == NativeShadowGrantAttemptKindV1::Checker
                && record.candidate_digest == candidate_digest
                && record.submission_digest == submission_digest
        })
    }

    pub(crate) fn validate_against_closed_local_grant(
        &self,
        grant: &VerifiedClosedLocalReplayGrant,
        expected_execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    ) -> anyhow::Result<()> {
        for record in self.records.values() {
            anyhow::ensure!(
                record.registry_digest == grant.registry_digest_hex(),
                "durable grant attempt registryDigest differs from the exact replay grant"
            );
            anyhow::ensure!(
                &record.execution_policy_digest == expected_execution_policy_digest,
                "durable grant attempt executionPolicyDigest differs from the exact replay grant"
            );
            anyhow::ensure!(
                grant.matches_durable_attempt(
                    boole_native_shadow_protocol::DurableClosedLocalReplayAttemptFields {
                        family_version: &record.four_tuple.family_version,
                        template_id: &record.four_tuple.template_id,
                        challenge_sha256: &record.four_tuple.challenge_sha256,
                        epoch: record.four_tuple.epoch,
                        operation_id_hex: &record.operation_id_hex,
                        candidate_digest_hex: &record.candidate_digest,
                        submission_digest_hex: &record.submission_digest,
                        pre_intake_only: record.kind == NativeShadowGrantAttemptKindV1::PreIntake,
                    },
                ),
                "durable grant attempt is not one of the four exact replay cases"
            );
        }
        Ok(())
    }

    pub(crate) fn reserve(
        &mut self,
        authority: &mut NativeShadowJournalAuthority,
        fields: NativeShadowGrantAttemptFieldsV1<'_>,
    ) -> Result<DurableNativeShadowGrantAttemptV1, NativeShadowGrantAttemptErrorV1> {
        let record = NativeShadowGrantAttemptRecordV1::try_from(fields)?;
        self.validate_new_record(&record)?;
        let event = NativeShadowJournalEvent::GrantAttemptReservedV1 {
            four_tuple: record.four_tuple.clone(),
            registry_digest: record.registry_digest.clone(),
            execution_policy_digest: record.execution_policy_digest.clone(),
            operation_id_hex: record.operation_id_hex.clone(),
            candidate_digest: record.candidate_digest.clone(),
            submission_digest: record.submission_digest.clone(),
            attempt_kind: record.kind,
        };
        let line = serde_json::to_string(&event)
            .map_err(|error| NativeShadowGrantAttemptErrorV1::Durability(error.to_string()))?;
        authority
            .append_line(&line)
            .map_err(|error| NativeShadowGrantAttemptErrorV1::Durability(error.to_string()))?;
        self.insert_validated(record.clone());
        Ok(DurableNativeShadowGrantAttemptV1 {
            record,
            journal_authority_id: authority.authority_id(),
        })
    }

    fn replay_record(
        &mut self,
        record: NativeShadowGrantAttemptRecordV1,
    ) -> Result<(), NativeShadowGrantAttemptErrorV1> {
        self.validate_new_record(&record)?;
        self.insert_validated(record);
        Ok(())
    }

    fn validate_new_record(
        &self,
        record: &NativeShadowGrantAttemptRecordV1,
    ) -> Result<(), NativeShadowGrantAttemptErrorV1> {
        if self.records.contains_key(&record.four_tuple) {
            return Err(NativeShadowGrantAttemptErrorV1::CaseAlreadyReserved);
        }
        if self.operation_ids.contains(&record.operation_id_hex) {
            return Err(NativeShadowGrantAttemptErrorV1::OperationAlreadyReserved);
        }
        let scope = NativeShadowGrantAttemptScopeV1::from(record);
        if self
            .scope
            .as_ref()
            .is_some_and(|expected| expected != &scope)
        {
            return Err(NativeShadowGrantAttemptErrorV1::ScopeDrift);
        }
        if self.records.len() >= 4 {
            return Err(NativeShadowGrantAttemptErrorV1::TotalBudgetExceeded);
        }
        if record.kind == NativeShadowGrantAttemptKindV1::Checker && self.checker_attempts >= 3 {
            return Err(NativeShadowGrantAttemptErrorV1::CheckerBudgetExceeded);
        }
        Ok(())
    }

    fn insert_validated(&mut self, record: NativeShadowGrantAttemptRecordV1) {
        self.scope
            .get_or_insert_with(|| NativeShadowGrantAttemptScopeV1::from(&record));
        if record.kind == NativeShadowGrantAttemptKindV1::Checker {
            self.checker_attempts += 1;
        }
        self.operation_ids.insert(record.operation_id_hex.clone());
        self.records.insert(record.four_tuple.clone(), record);
    }
}

impl TryFrom<NativeShadowGrantAttemptFieldsV1<'_>> for NativeShadowGrantAttemptRecordV1 {
    type Error = NativeShadowGrantAttemptErrorV1;

    fn try_from(fields: NativeShadowGrantAttemptFieldsV1<'_>) -> Result<Self, Self::Error> {
        for (field, value) in [
            ("registryDigest", fields.registry_digest),
            ("operationIdHex", fields.operation_id_hex),
            ("candidateDigest", fields.candidate_digest),
            ("submissionDigest", fields.submission_digest),
        ] {
            if !is_lower_sha256_hex(value) {
                return Err(NativeShadowGrantAttemptErrorV1::InvalidDigest(field));
            }
        }
        Ok(Self {
            four_tuple: fields.four_tuple.clone(),
            registry_digest: fields.registry_digest.to_string(),
            execution_policy_digest: fields.execution_policy_digest.clone(),
            operation_id_hex: fields.operation_id_hex.to_string(),
            candidate_digest: fields.candidate_digest.to_string(),
            submission_digest: fields.submission_digest.to_string(),
            kind: fields.kind,
        })
    }
}

impl From<&NativeShadowGrantAttemptRecordV1> for NativeShadowGrantAttemptScopeV1 {
    fn from(record: &NativeShadowGrantAttemptRecordV1) -> Self {
        Self {
            family_version: record.four_tuple.family_version.clone(),
            template_id: record.four_tuple.template_id.clone(),
            challenge_sha256: record.four_tuple.challenge_sha256.clone(),
            registry_digest: record.registry_digest.clone(),
            execution_policy_digest: record.execution_policy_digest.clone(),
        }
    }
}

/// Immutable durable binding for one v3 checker attempt. The operation ID is
/// globally one-shot for the lifetime of the journal, while the candidate
/// digest binds the exact submitted answer to this challenge execution.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeShadowFlightBindingV3 {
    registry_digest: String,
    execution_policy_digest: NativeShadowExecutionPolicyDigest,
    operation_id_hex: String,
    candidate_digest: String,
}

/// Closed node-owned retryable outcome vocabulary. Checker/deterministic
/// reason strings cannot be passed to the rollback API or deserialized from
/// a forged journal line as retryable infrastructure outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeShadowRetryableReasonV3 {
    NativeBusy,
    ContainmentWallClockKill,
    ContainmentKilled,
    ContainmentEnvironmentUnavailable,
    CheckerInternalError,
}

impl NativeShadowRetryableReasonV3 {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NativeBusy => "native_busy",
            Self::ContainmentWallClockKill => "containment_wall_clock_kill",
            Self::ContainmentKilled => "containment_killed",
            Self::ContainmentEnvironmentUnavailable => "containment_environment_unavailable",
            Self::CheckerInternalError => "checker_internal_error",
        }
    }
}

/// Opaque proof that startup recovery removed the exact V3 operation's
/// containment leaf. There is intentionally no production constructor in
/// this journal-only slice; the trusted containment integration must add the
/// constructor at the point where it verifies cleanup. Consequently restart
/// replay cannot be rolled back merely by knowing journal strings.
#[must_use]
pub(crate) struct VerifiedNativeShadowCleanupV3 {
    operation_id_hex: String,
}

impl VerifiedNativeShadowCleanupV3 {
    #[cfg(test)]
    fn for_test(operation_id_hex: &str) -> Self {
        Self {
            operation_id_hex: operation_id_hex.to_string(),
        }
    }
}

/// Non-cloneable proof that an `InFlightV3` record is durable. Later v3
/// transitions consume this capability rather than accepting caller-supplied
/// operation/candidate bindings again.
#[derive(Debug)]
#[must_use]
pub(crate) struct DurableNativeShadowInFlightV3 {
    four_tuple: NativeShadowFourTuple,
    binding: NativeShadowFlightBindingV3,
}

impl DurableNativeShadowInFlightV3 {
    pub(crate) fn operation_id_hex(&self) -> &str {
        &self.binding.operation_id_hex
    }

    pub(crate) fn candidate_digest(&self) -> &str {
        &self.binding.candidate_digest
    }
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
                "native-shadow journal {} or its state directory has unsafe type, owner, mode, or link count",
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
    /// Open the sole installed replay journal. The deployment package, not
    /// this process, creates the fixed private state directory. Refusing to
    /// create or repair it keeps a missing/unsafe installation fail-closed.
    #[cfg(target_os = "linux")]
    pub(crate) fn open_production(
        expected_node_uid: u32,
        expected_node_gid: u32,
    ) -> Result<Self, NativeShadowJournalAuthorityError> {
        let path = Path::new(PRODUCTION_NATIVE_SHADOW_JOURNAL_PATH);
        Self::open_prepared_production_dir(
            path.parent()
                .expect("fixed production journal has a parent"),
            path.file_name()
                .expect("fixed production journal has a file name"),
            expected_node_uid,
            expected_node_gid,
        )
    }

    /// Open a caller-provisioned private journal location through the same
    /// descriptor-relative production checks as the fixed Linux service.
    /// This is the user-scoped curl-install entrypoint on macOS; it never
    /// creates, repairs or relaxes the supplied state directory.
    #[cfg(unix)]
    pub(crate) fn open_prepared_production(
        path: impl AsRef<Path>,
        expected_uid: u32,
        expected_gid: u32,
    ) -> Result<Self, NativeShadowJournalAuthorityError> {
        let path = path.as_ref();
        let directory = path
            .parent()
            .ok_or_else(|| NativeShadowJournalAuthorityError::UnsafePath(path.to_path_buf()))?;
        let file_name = path
            .file_name()
            .ok_or_else(|| NativeShadowJournalAuthorityError::UnsafePath(path.to_path_buf()))?;
        Self::open_prepared_production_dir(directory, file_name, expected_uid, expected_gid)
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn open_production(
        _expected_node_uid: u32,
        _expected_node_gid: u32,
    ) -> Result<Self, NativeShadowJournalAuthorityError> {
        Err(NativeShadowJournalAuthorityError::Unsupported)
    }

    /// Descriptor-relative production opener shared with Unix tests. The
    /// caller supplies an already provisioned, private state directory; no
    /// arbitrary path is created or repaired here.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn open_prepared_production_dir(
        directory: &Path,
        file_name: &std::ffi::OsStr,
        expected_uid: u32,
        expected_gid: u32,
    ) -> Result<Self, NativeShadowJournalAuthorityError> {
        if file_name.as_bytes().is_empty()
            || file_name.as_bytes().contains(&b'/')
            || file_name == std::ffi::OsStr::new(".")
            || file_name == std::ffi::OsStr::new("..")
        {
            return Err(NativeShadowJournalAuthorityError::UnsafePath(
                directory.join(file_name),
            ));
        }
        let diagnostic_path = directory.join(file_name);
        let directory_file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory)
            .map_err(|err| {
                if matches!(err.raw_os_error(), Some(code) if code == libc::ELOOP || code == libc::ENOTDIR) {
                    NativeShadowJournalAuthorityError::UnsafePath(directory.to_path_buf())
                } else {
                    NativeShadowJournalAuthorityError::Io(directory.to_path_buf(), err)
                }
            })?;
        let directory_metadata = directory_file
            .metadata()
            .map_err(|err| NativeShadowJournalAuthorityError::Io(directory.to_path_buf(), err))?;
        if !directory_metadata.file_type().is_dir()
            || directory_metadata.uid() != expected_uid
            || directory_metadata.gid() != expected_gid
            || directory_metadata.mode() & 0o7777 != 0o700
        {
            return Err(NativeShadowJournalAuthorityError::UnsafePath(
                directory.to_path_buf(),
            ));
        }

        let name = std::ffi::CString::new(file_name.as_bytes())
            .map_err(|_| NativeShadowJournalAuthorityError::UnsafePath(diagnostic_path.clone()))?;
        let base_flags =
            libc::O_RDWR | libc::O_APPEND | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        let (raw_fd, created) = {
            // SAFETY: `directory_file` owns a live directory descriptor and
            // `name` is a NUL-terminated single path component. `openat`
            // neither retains either pointer nor aliases Rust memory.
            let existing =
                unsafe { libc::openat(directory_file.as_raw_fd(), name.as_ptr(), base_flags, 0) };
            if existing >= 0 {
                (existing, false)
            } else {
                let existing_error = std::io::Error::last_os_error();
                if existing_error.kind() != std::io::ErrorKind::NotFound {
                    return Err(if existing_error.raw_os_error() == Some(libc::ELOOP) {
                        NativeShadowJournalAuthorityError::UnsafePath(diagnostic_path.clone())
                    } else {
                        NativeShadowJournalAuthorityError::Io(
                            diagnostic_path.clone(),
                            existing_error,
                        )
                    });
                }
                // SAFETY: same descriptor/name reasoning as above. O_EXCL
                // ensures a concurrent creator cannot redirect this open.
                let created_fd = unsafe {
                    libc::openat(
                        directory_file.as_raw_fd(),
                        name.as_ptr(),
                        base_flags | libc::O_CREAT | libc::O_EXCL,
                        0o600,
                    )
                };
                if created_fd < 0 {
                    return Err(NativeShadowJournalAuthorityError::Io(
                        diagnostic_path.clone(),
                        std::io::Error::last_os_error(),
                    ));
                }
                (created_fd, true)
            }
        };
        // SAFETY: `raw_fd` is a newly owned descriptor returned by a
        // successful `openat` call and is transferred exactly once.
        let file = unsafe { File::from_raw_fd(raw_fd) };
        let metadata = file
            .metadata()
            .map_err(|err| NativeShadowJournalAuthorityError::Io(diagnostic_path.clone(), err))?;
        if !metadata.file_type().is_file()
            || metadata.uid() != expected_uid
            || metadata.gid() != expected_gid
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(NativeShadowJournalAuthorityError::UnsafePath(
                diagnostic_path,
            ));
        }
        flock_exclusive_nonblocking(&file).map_err(|err| {
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                NativeShadowJournalAuthorityError::Locked(diagnostic_path.clone())
            } else {
                NativeShadowJournalAuthorityError::Io(diagnostic_path.clone(), err)
            }
        })?;
        if created {
            file.sync_all().map_err(|err| {
                NativeShadowJournalAuthorityError::Io(diagnostic_path.clone(), err)
            })?;
            directory_file.sync_all().map_err(|err| {
                NativeShadowJournalAuthorityError::Io(directory.to_path_buf(), err)
            })?;
        }

        Ok(Self {
            file,
            diagnostic_path,
            device: metadata.dev(),
            inode: metadata.ino(),
            authority_id: NEXT_NATIVE_SHADOW_JOURNAL_AUTHORITY_ID.fetch_add(1, Ordering::Relaxed),
            poisoned: false,
            #[cfg(test)]
            fail_next_append: false,
        })
    }

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
/// closes F4's registry-drift gap (RED gate 3). The legacy-compatible
/// `begin_execution` path remains readable, while `begin_execution_v3`
/// durably binds operation/candidate identity and `retryable_rollback_v3`
/// is the only live retry path back to `Active(fresh)`. `complete_consumed`
/// drives the evidence-backed terminal transition. Section 8's `native_busy`
/// permit is a separate primitive above, and
/// containment execution (section 9) is a later slice — nothing in this
/// store acquires the permit or enforces single-flight execution on its own.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowStateStore {
    rows: HashMap<NativeShadowFourTuple, NativeShadowStateRow>,
    evidence_commits: HashMap<NativeShadowFourTuple, NativeShadowEvidenceCommitData>,
    flight_bindings_v3: HashMap<NativeShadowFourTuple, NativeShadowFlightBindingV3>,
    seen_operation_ids_v3: HashSet<String>,
    v3_lifecycle_rows: HashSet<NativeShadowFourTuple>,
    journal_authority_id: Option<u64>,
}

/// Current request bindings required to reproduce one already-terminal BF.3
/// receipt without re-running the checker.
pub(crate) struct NativeShadowTerminalReceiptBinding<'a> {
    pub registry_version: &'a str,
    pub registry_digest: &'a str,
    pub execution_policy_digest: &'a NativeShadowExecutionPolicyDigest,
    pub candidate_digest: &'a str,
    pub submission_digest: &'a str,
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

    /// Exact node-owned evidence for a terminal redelivery. This is read-only
    /// and exists only after the evidence-backed `Consumed` transition.
    pub(crate) fn terminal_durable_evidence(
        &self,
        four_tuple: &NativeShadowFourTuple,
    ) -> Option<&NativeShadowEvidence> {
        if self.rows.get(four_tuple)?.state != ChallengeState::Consumed {
            return None;
        }
        self.evidence_commits
            .get(four_tuple)
            .map(|commit| &commit.evidence)
    }

    /// Digest of the exact evidence bytes originally fsynced. Redelivery must
    /// use this value instead of serializing the typed evidence again, because
    /// semantically equivalent JSON formatting is not byte identity.
    pub(crate) fn terminal_durable_evidence_digest(
        &self,
        four_tuple: &NativeShadowFourTuple,
    ) -> Option<&str> {
        if self.rows.get(four_tuple)?.state != ChallengeState::Consumed {
            return None;
        }
        self.evidence_commits
            .get(four_tuple)
            .map(|commit| commit.evidence_digest.as_str())
    }

    /// Map only an already-durable, node-owned v2 checker verdict into the
    /// common BF.3 receipt contract. The non-cloneable capability proves that
    /// the exact evidence bytes were appended for this in-flight row; raw
    /// submission JSON and miner-created receipt-shaped data have no entry
    /// point into this adapter.
    pub(crate) fn map_durable_v2_to_bf3_receipt(
        &self,
        four_tuple: &NativeShadowFourTuple,
        durable: &DurableNativeShadowEvidenceCommit,
    ) -> Result<VerificationReceipt, NativeShadowReceiptMapError> {
        let Some(row) = self.rows.get(four_tuple) else {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        };
        let Some(commit) = self.evidence_commits.get(four_tuple) else {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        };
        if row.state != ChallengeState::InFlight
            || durable.four_tuple != *four_tuple
            || durable.registry_digest != row.registry_digest
            || row.execution_policy_digest.as_ref() != Some(&durable.execution_policy_digest)
            || commit.candidate_digest != durable.candidate_digest
            || commit.evidence_digest != durable.evidence_digest
            || commit.execution_policy_digest.as_ref() != Some(&durable.execution_policy_digest)
            || commit.evidence.schema != "boole.native-shadow.evidence.v2"
        {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        }
        let evidence = &commit.evidence;
        evidence
            .validate_bindings(
                four_tuple,
                &durable.candidate_digest,
                Some(&durable.execution_policy_digest),
            )
            .map_err(|_| NativeShadowReceiptMapError::DurableEvidenceBindingMismatch)?;

        self.map_internal_v2_evidence_to_bf3_receipt(row, commit, &durable.execution_policy_digest)
    }

    /// Re-project only already-terminal journal evidence. Every current
    /// request binding is checked against the exact stored evidence; no JSON
    /// reserialization is used to decide identity or create a new verdict.
    pub(crate) fn map_terminal_v2_to_bf3_receipt(
        &self,
        four_tuple: &NativeShadowFourTuple,
        binding: NativeShadowTerminalReceiptBinding<'_>,
        exhaustion: &NativeShadowExhaustionLedger,
    ) -> Result<VerificationReceipt, NativeShadowReceiptMapError> {
        let Some(row) = self.rows.get(four_tuple) else {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        };
        let Some(commit) = self.evidence_commits.get(four_tuple) else {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        };
        if row.state != ChallengeState::Consumed
            || !exhaustion.contains(four_tuple)
            || row.registry_digest != binding.registry_digest
            || row.execution_policy_digest.as_ref() != Some(binding.execution_policy_digest)
            || commit.candidate_digest != binding.candidate_digest
            || commit.execution_policy_digest.as_ref() != Some(binding.execution_policy_digest)
            || commit.evidence.schema != "boole.native-shadow.evidence.v2"
            || commit.evidence.registry_version != binding.registry_version
            || commit.evidence.submission_digest != binding.submission_digest
        {
            return Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch);
        }
        commit
            .evidence
            .validate_bindings(
                four_tuple,
                binding.candidate_digest,
                Some(binding.execution_policy_digest),
            )
            .map_err(|_| NativeShadowReceiptMapError::DurableEvidenceBindingMismatch)?;

        self.map_internal_v2_evidence_to_bf3_receipt(row, commit, binding.execution_policy_digest)
    }

    fn map_internal_v2_evidence_to_bf3_receipt(
        &self,
        row: &NativeShadowStateRow,
        commit: &NativeShadowEvidenceCommitData,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    ) -> Result<VerificationReceipt, NativeShadowReceiptMapError> {
        let evidence = &commit.evidence;

        let parse_digest = |value: &str, field| {
            Hex32::from_hex(value).map_err(|_| NativeShadowReceiptMapError::InvalidDigest(field))
        };
        let submission_id = parse_digest(&evidence.submission_digest, "submissionDigest")?;
        let template_id = parse_digest(&evidence.template_id, "templateId")?;
        let anchor_digest = parse_digest(&evidence.anchor_digest, "anchorDigest")?;
        let challenge_digest = parse_digest(&evidence.challenge_sha256, "challengeSha256")?;
        let candidate_digest = parse_digest(&evidence.candidate_digest, "candidateDigest")?;
        let checker_hash = parse_digest(&evidence.checker_digest, "checkerDigest")?;
        let checker_policy_digest = parse_digest(&evidence.policy_digest, "policyDigest")?;
        let execution_policy_digest =
            parse_digest(execution_policy_digest.as_str(), "executionPolicyDigest")?;
        let toolchain_digest = parse_digest(&evidence.toolchain_digest, "toolchainDigest")?;
        let registry_digest = parse_digest(&row.registry_digest, "registryDigest")?;

        let verdict = match (evidence.verdict, evidence.reason_code.as_str()) {
            (NativeShadowEvidenceVerdict::Accepted, "accepted") => ReceiptVerdict::Accepted,
            (NativeShadowEvidenceVerdict::DeterministicReject, "compile_or_hidden_test_failed") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::CompileOrHiddenTestFailed)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "forbidden_construct") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::ForbiddenConstruct)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "malformed_patch_region") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::MalformedPatchRegion)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "outside_patch_modified") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::OutsidePatchModified)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "patch_line_limit_exceeded") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::PatchLineLimitExceeded)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "patch_size_exceeded") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::PatchSizeExceeded)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "submission_unreadable") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::SubmissionUnreadable)
            }
            (NativeShadowEvidenceVerdict::DeterministicReject, "checker_rejected") => {
                ReceiptVerdict::Rejected(ReceiptRejectReason::CompileOrHiddenTestFailed)
            }
            (
                NativeShadowEvidenceVerdict::DeterministicReject,
                "submission_resource_ceiling_breach",
            ) => ReceiptVerdict::Rejected(ReceiptRejectReason::CompileOrHiddenTestFailed),
            (
                NativeShadowEvidenceVerdict::DeterministicReject,
                "checker_reported_reason_unconfirmed",
            ) => ReceiptVerdict::Rejected(ReceiptRejectReason::CompileOrHiddenTestFailed),
            _ => return Err(NativeShadowReceiptMapError::InvalidVerdictReason),
        };
        let epoch = evidence.epoch.to_be_bytes();
        let task_id = NativeTaskIdentity::try_new(
            evidence.family_version.clone(),
            template_id,
            anchor_digest,
        )
        .map_err(|_| NativeShadowReceiptMapError::DurableEvidenceBindingMismatch)?
        .task_id()
        .as_hex32();
        let verdict_label = match verdict {
            ReceiptVerdict::Accepted => "accepted",
            ReceiptVerdict::Rejected(reason) => reason.label(),
        };
        let artifact_root = bf3_root(
            NATIVE_BF3_ARTIFACT_DOMAIN,
            &[
                evidence.schema.as_bytes(),
                evidence.submission_schema.as_bytes(),
                submission_id.as_bytes(),
                evidence.family_version.as_bytes(),
                template_id.as_bytes(),
                anchor_digest.as_bytes(),
                challenge_digest.as_bytes(),
                &epoch,
                candidate_digest.as_bytes(),
                evidence.intake_version.as_bytes(),
                checker_hash.as_bytes(),
                checker_policy_digest.as_bytes(),
                execution_policy_digest.as_bytes(),
                toolchain_digest.as_bytes(),
                verdict_label.as_bytes(),
                evidence.reason_code.as_bytes(),
                evidence.registry_version.as_bytes(),
                registry_digest.as_bytes(),
            ],
        );

        Ok(VerificationReceipt {
            task_id,
            submission_id,
            artifact_root,
            checker_hash,
            verdict,
        })
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

    /// Bootstrap only the exact row carried by the one-shot replay grant.
    /// This is the sole exception to the disabled production registry and it
    /// cannot synthesize an alternate registry or policy binding.
    pub(crate) fn resolve_verified_closed_local_replay(
        &mut self,
        bootstrap: &VerifiedNativeShadowReplayBootstrap,
    ) -> ResolveOutcome {
        self.resolve_with_execution_policy(
            bootstrap.four_tuple(),
            bootstrap.registry_digest(),
            bootstrap.execution_policy_digest(),
            || ChallengeState::ActiveFresh,
        )
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
    #[cfg(test)]
    fn begin_execution(
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
        if self.v3_lifecycle_rows.contains(four_tuple) {
            return Err(NativeShadowTransitionError::V3Required(four_tuple.clone()));
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

    /// Start a checker execution under the v3 contract. Unlike the legacy
    /// compatibility API, this transition cannot be called without the exact
    /// operation ID and candidate digest that the launcher request will use.
    /// Both values are journaled before the in-memory row becomes InFlight.
    pub(crate) fn begin_reserved_closed_local_replay_execution_v3(
        &mut self,
        authority: &mut NativeShadowJournalAuthority,
        reservation: DurableNativeShadowGrantAttemptV1,
    ) -> Result<DurableNativeShadowInFlightV3, NativeShadowTransitionError> {
        if reservation.journal_authority_id != authority.authority_id() {
            return Err(NativeShadowTransitionError::AttemptBindingMismatch(
                reservation.record.four_tuple,
            ));
        }
        let record = reservation.record;
        let row = self
            .rows
            .get(&record.four_tuple)
            .ok_or_else(|| NativeShadowTransitionError::NoSuchRow(record.four_tuple.clone()))?;
        if record.kind != NativeShadowGrantAttemptKindV1::Checker
            || row.registry_digest != record.registry_digest
            || row.execution_policy_digest.as_ref() != Some(&record.execution_policy_digest)
        {
            return Err(NativeShadowTransitionError::AttemptBindingMismatch(
                record.four_tuple,
            ));
        }
        self.begin_execution_v3(
            authority,
            &record.four_tuple,
            &record.execution_policy_digest,
            &record.operation_id_hex,
            &record.candidate_digest,
        )
    }

    pub fn begin_execution_v3(
        &mut self,
        authority: &mut NativeShadowJournalAuthority,
        four_tuple: &NativeShadowFourTuple,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
        operation_id_hex: &str,
        candidate_digest: &str,
    ) -> Result<DurableNativeShadowInFlightV3, NativeShadowTransitionError> {
        self.bind_journal_authority(authority)?;
        if !is_lower_sha256_hex(operation_id_hex) {
            return Err(NativeShadowTransitionError::InvalidOperationId);
        }
        if !is_lower_sha256_hex(candidate_digest) {
            return Err(NativeShadowTransitionError::InvalidCandidateDigest);
        }
        if self.seen_operation_ids_v3.contains(operation_id_hex) {
            return Err(NativeShadowTransitionError::OperationIdReused(
                operation_id_hex.to_string(),
            ));
        }
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
        if row_execution_policy_digest
            .as_ref()
            .is_some_and(|bound| bound != execution_policy_digest)
            || (row_is_durable
                && row_execution_policy_digest.as_ref() != Some(execution_policy_digest))
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
            authority
                .append_line(&serde_json::to_string(&bootstrap).map_err(|err| {
                    NativeShadowTransitionError::Durability(anyhow::Error::from(err))
                })?)
                .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
            let row = self.rows.get_mut(four_tuple).expect("row checked above");
            row.durable = true;
            row.execution_policy_digest = Some(execution_policy_digest.clone());
        }

        let binding = NativeShadowFlightBindingV3 {
            registry_digest: registry_digest.clone(),
            execution_policy_digest: execution_policy_digest.clone(),
            operation_id_hex: operation_id_hex.to_string(),
            candidate_digest: candidate_digest.to_string(),
        };
        let event = NativeShadowJournalEvent::InFlightV3 {
            four_tuple: four_tuple.clone(),
            registry_digest,
            execution_policy_digest: execution_policy_digest.clone(),
            operation_id_hex: operation_id_hex.to_string(),
            candidate_digest: candidate_digest.to_string(),
        };
        authority
            .append_line(
                &serde_json::to_string(&event).map_err(|err| {
                    NativeShadowTransitionError::Durability(anyhow::Error::from(err))
                })?,
            )
            .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
        self.rows
            .get_mut(four_tuple)
            .expect("row checked above")
            .state = ChallengeState::InFlight;
        self.flight_bindings_v3
            .insert(four_tuple.clone(), binding.clone());
        self.seen_operation_ids_v3
            .insert(operation_id_hex.to_string());
        self.v3_lifecycle_rows.insert(four_tuple.clone());
        Ok(DurableNativeShadowInFlightV3 {
            four_tuple: four_tuple.clone(),
            binding,
        })
    }

    /// Recreate the non-cloneable flight capability after restart, but only
    /// by consuming an opaque proof that the same operation's containment
    /// tree was cleaned up. Legacy V1/V2 flights have no V3 binding, and a
    /// flight with durable terminal evidence is never resumable as retryable.
    pub fn resume_in_flight_v3(
        &self,
        cleanup: VerifiedNativeShadowCleanupV3,
        four_tuple: &NativeShadowFourTuple,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
        operation_id_hex: &str,
        candidate_digest: &str,
    ) -> Result<DurableNativeShadowInFlightV3, NativeShadowTransitionError> {
        if cleanup.operation_id_hex != operation_id_hex {
            return Err(NativeShadowTransitionError::CleanupBindingMismatch);
        }
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
            return Err(NativeShadowTransitionError::RetryableAfterEvidence(
                four_tuple.clone(),
            ));
        }
        let binding = self.flight_bindings_v3.get(four_tuple).ok_or_else(|| {
            NativeShadowTransitionError::FlightBindingMismatch(four_tuple.clone())
        })?;
        if binding.execution_policy_digest != *execution_policy_digest
            || binding.operation_id_hex != operation_id_hex
            || binding.candidate_digest != candidate_digest
            || row.registry_digest != binding.registry_digest
            || row.execution_policy_digest.as_ref() != Some(execution_policy_digest)
        {
            return Err(NativeShadowTransitionError::FlightBindingMismatch(
                four_tuple.clone(),
            ));
        }
        Ok(DurableNativeShadowInFlightV3 {
            four_tuple: four_tuple.clone(),
            binding: binding.clone(),
        })
    }

    /// Recreate the non-cloneable durable-evidence capability after a crash
    /// that happened after `EvidenceV2` was fsynced but before the matching
    /// terminal event. This never executes the checker again. It is available
    /// only after the trusted launcher proved that the exact recovered
    /// operation's containment tree was cleaned up.
    pub(crate) fn recover_pending_evidence_v3(
        &self,
        cleanup: VerifiedNativeShadowCleanupV3,
        four_tuple: &NativeShadowFourTuple,
        execution_policy_digest: &NativeShadowExecutionPolicyDigest,
        operation_id_hex: &str,
        candidate_digest: &str,
    ) -> Result<DurableNativeShadowEvidenceCommit, NativeShadowTransitionError> {
        if cleanup.operation_id_hex != operation_id_hex {
            return Err(NativeShadowTransitionError::CleanupBindingMismatch);
        }
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
        let binding = self.flight_bindings_v3.get(four_tuple).ok_or_else(|| {
            NativeShadowTransitionError::FlightBindingMismatch(four_tuple.clone())
        })?;
        if binding.execution_policy_digest != *execution_policy_digest
            || binding.operation_id_hex != operation_id_hex
            || binding.candidate_digest != candidate_digest
            || row.registry_digest != binding.registry_digest
            || row.execution_policy_digest.as_ref() != Some(execution_policy_digest)
        {
            return Err(NativeShadowTransitionError::FlightBindingMismatch(
                four_tuple.clone(),
            ));
        }
        let commit = self.evidence_commits.get(four_tuple).ok_or_else(|| {
            NativeShadowTransitionError::EvidenceBindingMismatch(four_tuple.clone())
        })?;
        if commit.candidate_digest != candidate_digest
            || commit.execution_policy_digest.as_ref() != Some(execution_policy_digest)
            || commit.evidence.schema != "boole.native-shadow.evidence.v2"
        {
            return Err(NativeShadowTransitionError::EvidenceBindingMismatch(
                four_tuple.clone(),
            ));
        }
        commit
            .evidence
            .validate_bindings(four_tuple, candidate_digest, Some(execution_policy_digest))
            .map_err(|_| {
                NativeShadowTransitionError::EvidenceBindingMismatch(four_tuple.clone())
            })?;
        Ok(DurableNativeShadowEvidenceCommit {
            four_tuple: four_tuple.clone(),
            registry_digest: row.registry_digest.clone(),
            execution_policy_digest: execution_policy_digest.clone(),
            candidate_digest: candidate_digest.to_string(),
            evidence_digest: commit.evidence_digest.clone(),
        })
    }

    /// Return a v3 attempt to `ActiveFresh` after a retryable infrastructure
    /// outcome. The exact original binding is repeated in the rollback line,
    /// and that line is fsynced before memory is changed. The operation ID is
    /// deliberately retained in the global seen set, so retrying requires a
    /// fresh operation ID even for the same answer and challenge.
    pub fn retryable_rollback_v3(
        &mut self,
        authority: &mut NativeShadowJournalAuthority,
        flight: &DurableNativeShadowInFlightV3,
        reason: NativeShadowRetryableReasonV3,
    ) -> Result<(), NativeShadowTransitionError> {
        self.bind_journal_authority(authority)?;
        let four_tuple = &flight.four_tuple;
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
            return Err(NativeShadowTransitionError::RetryableAfterEvidence(
                four_tuple.clone(),
            ));
        }
        if row.registry_digest != flight.binding.registry_digest
            || row.execution_policy_digest.as_ref() != Some(&flight.binding.execution_policy_digest)
            || self.flight_bindings_v3.get(four_tuple) != Some(&flight.binding)
        {
            return Err(NativeShadowTransitionError::FlightBindingMismatch(
                four_tuple.clone(),
            ));
        }
        let event = NativeShadowJournalEvent::RetryableRollbackV3 {
            four_tuple: four_tuple.clone(),
            registry_digest: flight.binding.registry_digest.clone(),
            execution_policy_digest: flight.binding.execution_policy_digest.clone(),
            operation_id_hex: flight.binding.operation_id_hex.clone(),
            candidate_digest: flight.binding.candidate_digest.clone(),
            reason,
        };
        authority
            .append_line(
                &serde_json::to_string(&event).map_err(|err| {
                    NativeShadowTransitionError::Durability(anyhow::Error::from(err))
                })?,
            )
            .map_err(|err| NativeShadowTransitionError::Durability(err.into()))?;
        self.rows
            .get_mut(four_tuple)
            .expect("row checked above")
            .state = ChallengeState::ActiveFresh;
        self.flight_bindings_v3.remove(four_tuple);
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
        if self
            .flight_bindings_v3
            .get(four_tuple)
            .is_some_and(|binding| {
                binding.registry_digest != row.registry_digest
                    || binding.execution_policy_digest != execution_policy_digest
                    || binding.candidate_digest != candidate_digest
            })
        {
            return Err(NativeShadowTransitionError::EvidenceBindingMismatch(
                four_tuple.clone(),
            ));
        }
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
        self.flight_bindings_v3.remove(four_tuple);
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
    InvalidOperationId,
    InvalidCandidateDigest,
    OperationIdReused(String),
    FlightBindingMismatch(NativeShadowFourTuple),
    RetryableAfterEvidence(NativeShadowFourTuple),
    CleanupBindingMismatch,
    AttemptBindingMismatch(NativeShadowFourTuple),
    V3Required(NativeShadowFourTuple),
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
            Self::InvalidOperationId => write!(
                f,
                "native-shadow: operationIdHex must be 64 lowercase hexadecimal characters"
            ),
            Self::InvalidCandidateDigest => write!(
                f,
                "native-shadow: candidateDigest must be 64 lowercase hexadecimal characters"
            ),
            Self::OperationIdReused(operation_id) => write!(
                f,
                "native-shadow: operationIdHex was already used: {operation_id}"
            ),
            Self::FlightBindingMismatch(four_tuple) => write!(
                f,
                "native-shadow: durable v3 flight binding mismatch for {four_tuple:?}"
            ),
            Self::RetryableAfterEvidence(four_tuple) => write!(
                f,
                "native-shadow: retryable rollback is forbidden after evidence for {four_tuple:?}"
            ),
            Self::CleanupBindingMismatch => write!(
                f,
                "native-shadow: verified cleanup does not match the recovered operation"
            ),
            Self::AttemptBindingMismatch(four_tuple) => write!(
                f,
                "native-shadow: durable grant attempt does not match state row {four_tuple:?}"
            ),
            Self::V3Required(four_tuple) => write!(
                f,
                "native-shadow: v3 lifecycle row cannot downgrade to v2 for {four_tuple:?}"
            ),
        }
    }
}

impl std::error::Error for NativeShadowTransitionError {}

/// Single durable per-key authority (spec section 7). Un-suffixed variants
/// are read-only legacy v1 records. V2 records retain compatibility with the
/// earlier execution-policy-bound lifecycle. New checker attempts use V3 to
/// bind operation/candidate identity and to record retryable rollback without
/// erasing global operation-ID history. Recovery derives the in-memory
/// exhaustion view from terminal lines; there is no second independently
/// writable exhaustion file that can drift from this history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum NativeShadowJournalEvent {
    GrantAttemptReservedV1 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
        #[serde(rename = "operationIdHex")]
        operation_id_hex: String,
        #[serde(rename = "candidateDigest")]
        candidate_digest: String,
        #[serde(rename = "submissionDigest")]
        submission_digest: String,
        #[serde(rename = "attemptKind")]
        attempt_kind: NativeShadowGrantAttemptKindV1,
    },
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
    InFlightV3 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
        #[serde(rename = "operationIdHex")]
        operation_id_hex: String,
        #[serde(rename = "candidateDigest")]
        candidate_digest: String,
    },
    RetryableRollbackV3 {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
        #[serde(rename = "registryDigest")]
        registry_digest: String,
        #[serde(rename = "executionPolicyDigest")]
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
        #[serde(rename = "operationIdHex")]
        operation_id_hex: String,
        #[serde(rename = "candidateDigest")]
        candidate_digest: String,
        #[serde(rename = "reasonCode")]
        reason: NativeShadowRetryableReasonV3,
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
    flight_bindings_v3: HashMap<NativeShadowFourTuple, NativeShadowFlightBindingV3>,
    /// Every v3 flight ever observed, including rows later rolled back or
    /// terminalized. The live map above is intentionally pruned by terminal
    /// events, but closed-local startup still has to prove that each historic
    /// execution had a preceding durable checker-attempt reservation.
    v3_flight_history: Vec<(NativeShadowFourTuple, NativeShadowFlightBindingV3)>,
    seen_operation_ids_v3: HashSet<String>,
    v3_lifecycle_rows: HashSet<NativeShadowFourTuple>,
    exhausted: HashSet<NativeShadowFourTuple>,
    pub(crate) attempts: NativeShadowGrantAttemptLedgerV1,
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
    let mut flight_bindings_v3 = HashMap::new();
    let mut v3_flight_history = Vec::new();
    let mut seen_operation_ids_v3 = HashSet::new();
    let mut v3_lifecycle_rows = HashSet::new();
    let mut exhausted = HashSet::new();
    let mut attempts = NativeShadowGrantAttemptLedgerV1::default();
    for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
        let event: NativeShadowJournalEvent = serde_json::from_str(line).map_err(|err| {
            anyhow::anyhow!("nativeShadowJournal: line {} invalid JSON: {}", i + 1, err)
        })?;
        match event {
            NativeShadowJournalEvent::GrantAttemptReservedV1 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
                operation_id_hex,
                candidate_digest,
                submission_digest,
                attempt_kind,
            } => {
                let record = NativeShadowGrantAttemptRecordV1 {
                    four_tuple,
                    registry_digest,
                    execution_policy_digest,
                    operation_id_hex,
                    candidate_digest,
                    submission_digest,
                    kind: attempt_kind,
                };
                attempts.replay_record(record).map_err(|error| {
                    anyhow::anyhow!(
                        "nativeShadowJournal: line {} invalid grant attempt: {}",
                        i + 1,
                        error
                    )
                })?;
            }
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
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
                operation_id_hex,
                candidate_digest,
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
                        == Some(&Some(execution_policy_digest.clone())),
                    "nativeShadowJournal: line {} changes executionPolicyDigest or mixes lifecycle versions",
                    i + 1
                );
                anyhow::ensure!(
                    is_lower_sha256_hex(&operation_id_hex),
                    "nativeShadowJournal: line {} has invalid operationIdHex",
                    i + 1
                );
                anyhow::ensure!(
                    is_lower_sha256_hex(&candidate_digest),
                    "nativeShadowJournal: line {} has invalid candidateDigest",
                    i + 1
                );
                anyhow::ensure!(
                    seen_operation_ids_v3.insert(operation_id_hex.clone()),
                    "nativeShadowJournal: line {} reuses operationIdHex",
                    i + 1
                );
                let binding = NativeShadowFlightBindingV3 {
                    registry_digest,
                    execution_policy_digest,
                    operation_id_hex,
                    candidate_digest,
                };
                v3_flight_history.push((four_tuple.clone(), binding.clone()));
                flight_bindings_v3.insert(four_tuple.clone(), binding);
                v3_lifecycle_rows.insert(four_tuple.clone());
                resolved.insert(four_tuple, ChallengeState::InFlight);
            }
            NativeShadowJournalEvent::RetryableRollbackV3 {
                four_tuple,
                registry_digest,
                execution_policy_digest,
                operation_id_hex,
                candidate_digest,
                reason: _,
            } => {
                anyhow::ensure!(
                    resolved.get(&four_tuple) == Some(&ChallengeState::InFlight),
                    "nativeShadowJournal: line {} rolls back a non-InFlight row",
                    i + 1
                );
                let expected = NativeShadowFlightBindingV3 {
                    registry_digest,
                    execution_policy_digest,
                    operation_id_hex,
                    candidate_digest,
                };
                anyhow::ensure!(
                    flight_bindings_v3.get(&four_tuple) == Some(&expected),
                    "nativeShadowJournal: line {} retryable rollback binding mismatch",
                    i + 1
                );
                anyhow::ensure!(
                    !evidence_commits.contains_key(&four_tuple),
                    "nativeShadowJournal: line {} rolls back after durable evidence",
                    i + 1
                );
                flight_bindings_v3.remove(&four_tuple);
                resolved.insert(four_tuple, ChallengeState::ActiveFresh);
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
                if let Some(binding) = flight_bindings_v3.get(&four_tuple) {
                    anyhow::ensure!(
                        binding.registry_digest == registry_digest
                            && binding.execution_policy_digest == execution_policy_digest
                            && binding.candidate_digest == candidate_digest,
                        "nativeShadowJournal: line {} evidence does not match its InFlightV3 binding",
                        i + 1
                    );
                }
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
                flight_bindings_v3.remove(&four_tuple);
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
                flight_bindings_v3.remove(&four_tuple);
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
        flight_bindings_v3,
        v3_flight_history,
        seen_operation_ids_v3,
        v3_lifecycle_rows,
        exhausted,
        attempts,
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
    pub(crate) attempts: NativeShadowGrantAttemptLedgerV1,
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
    store.flight_bindings_v3 = replay.flight_bindings_v3.clone();
    store.seen_operation_ids_v3 = replay.seen_operation_ids_v3.clone();
    store.v3_lifecycle_rows = replay.v3_lifecycle_rows.clone();
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
        attempts: replay.attempts,
        stuck_in_flight: replay.stuck_in_flight,
    })
}

/// Recover only the frozen closed-local replay overlay from its dedicated
/// journal. Unlike ordinary registry recovery, an empty journal stays empty:
/// the disabled production registry can never create an active row. The row
/// first appears only through an opaque grant authorization.
pub(crate) fn recover_verified_closed_local_replay_state(
    expected_registry_version: &str,
    expected_registry_digest: &str,
    expected_execution_policy_digest: &NativeShadowExecutionPolicyDigest,
    authority: &mut NativeShadowJournalAuthority,
) -> anyhow::Result<NativeShadowRecovery> {
    let replay = replay_native_shadow_journal(authority)?;
    for (four_tuple, binding) in &replay.v3_flight_history {
        anyhow::ensure!(
            replay.attempts.matches_flight(
                four_tuple,
                &binding.operation_id_hex,
                &binding.candidate_digest,
            ),
            "closed-local replay InFlightV3 has no matching durable checker attempt"
        );
    }
    for (four_tuple, commit) in &replay.evidence_commits {
        anyhow::ensure!(
            commit.evidence.registry_version == expected_registry_version,
            "closed-local replay evidence registryVersion drift"
        );
        anyhow::ensure!(
            replay.attempts.matches_evidence(
                four_tuple,
                &commit.candidate_digest,
                &commit.evidence.submission_digest,
            ),
            "closed-local replay evidence has no matching durable checker attempt"
        );
    }
    let mut exhaustion_ledger = NativeShadowExhaustionLedger::default();
    exhaustion_ledger
        .exhausted
        .extend(replay.exhausted.iter().cloned());

    let mut store = NativeShadowStateStore {
        journal_authority_id: Some(authority.authority_id()),
        ..NativeShadowStateStore::default()
    };
    for (four_tuple, state) in &replay.resolved {
        let registry_digest = replay
            .registry_digests
            .get(four_tuple)
            .ok_or_else(|| anyhow::anyhow!("closed-local replay row missing registryDigest"))?;
        let execution_policy_digest =
            replay
                .execution_policy_digests
                .get(four_tuple)
                .ok_or_else(|| {
                    anyhow::anyhow!("closed-local replay row missing executionPolicyDigest")
                })?;
        anyhow::ensure!(
            registry_digest == expected_registry_digest,
            "closed-local replay journal registryDigest drift"
        );
        anyhow::ensure!(
            execution_policy_digest.as_ref() == Some(expected_execution_policy_digest),
            "closed-local replay journal executionPolicyDigest drift"
        );
        anyhow::ensure!(
            replay.v3_lifecycle_rows.contains(four_tuple),
            "closed-local replay journal contains a legacy lifecycle row"
        );
        store.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state: *state,
                registry_digest: registry_digest.clone(),
                execution_policy_digest: execution_policy_digest.clone(),
                durable: true,
            },
        );
    }
    store.evidence_commits = replay.evidence_commits;
    store.flight_bindings_v3 = replay.flight_bindings_v3;
    store.seen_operation_ids_v3 = replay.seen_operation_ids_v3;
    store.v3_lifecycle_rows = replay.v3_lifecycle_rows;

    Ok(NativeShadowRecovery {
        store,
        exhaustion_ledger,
        attempts: replay.attempts,
        stuck_in_flight: replay.stuck_in_flight,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "linux-arm64-authority"))]
    const PRODUCTION_REGISTRY_FIXTURE: &str =
        include_str!("../../../fixtures/native-shadow/registry-v1.json");
    #[cfg(feature = "linux-arm64-authority")]
    const PRODUCTION_REGISTRY_FIXTURE: &str =
        include_str!("../../../fixtures/native-shadow/registry-arm64-v1.json");
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

    fn map_test_bf3_receipt(
        label: &str,
        four_tuple: NativeShadowFourTuple,
        registry_digest: &str,
        execution_policy_digest: NativeShadowExecutionPolicyDigest,
        evidence: serde_json::Value,
    ) -> Result<VerificationReceipt, NativeShadowReceiptMapError> {
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths(&format!("bf3-map-{label}"));
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, registry_digest, || ChallengeState::ActiveFresh);
        store
            .begin_execution(&mut authority, &four_tuple, &execution_policy_digest)
            .expect("begin execution");
        let candidate_digest = evidence["candidateDigest"]
            .as_str()
            .expect("candidate digest")
            .to_string();
        let durable = store
            .persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &evidence.to_string(),
            )
            .expect("persist evidence");
        store.map_durable_v2_to_bf3_receipt(&four_tuple, &durable)
    }

    #[test]
    fn durable_v2_accept_maps_to_the_common_bf3_receipt() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let registry_digest = sha256_hex(TEST_ONLY_REGISTRY_FIXTURE.as_bytes());
        let (journal_path, _exhaustion_path) = scratch_journal_and_exhaustion_paths("bf3-accepted");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, &registry_digest, || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin execution");
        let durable =
            persist_test_evidence(&mut store, &mut authority, &four_tuple, "bf3-accepted");

        let receipt = store
            .map_durable_v2_to_bf3_receipt(&four_tuple, &durable)
            .expect("durable node-owned evidence maps");
        let evidence = store
            .pending_durable_evidence(&four_tuple)
            .expect("pending durable evidence");

        assert!(receipt.accepted());
        assert_eq!(receipt.submission_id.to_hex(), evidence.submission_digest);
        assert_eq!(receipt.checker_hash.to_hex(), evidence.checker_digest);
        assert_eq!(
            [receipt.task_id.to_hex(), receipt.artifact_root.to_hex()],
            [
                "77b0edae129635b6a5af8ff728d492073e9bde5aebf834c9976eac448d9e7ba6".to_string(),
                "3ecccd5a5eee1d75bdc30eb774b56a0332a247b96e063c789d5072acf9f25632".to_string(),
            ],
            "native BF3 canonical roots are protocol fixtures"
        );
        assert_eq!(
            receipt,
            store
                .map_durable_v2_to_bf3_receipt(&four_tuple, &durable)
                .expect("same durable evidence maps byte-identically")
        );
    }

    #[test]
    fn terminal_redelivery_reuses_the_exact_durable_evidence_and_receipt() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let registry_digest = sha256_hex(b"bf3-terminal-redelivery-registry");
        let policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"bf3-terminal-redelivery-candidate");
        let (journal_path, _) = scratch_journal_and_exhaustion_paths("bf3-terminal-redelivery");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        let mut exhaustion = NativeShadowExhaustionLedger::default();
        store.resolve_with_execution_policy(&four_tuple, &registry_digest, &policy, || {
            ChallengeState::ActiveFresh
        });
        store
            .begin_execution(&mut authority, &four_tuple, &policy)
            .expect("begin execution");
        let evidence_json = test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "terminal-redelivery",
            &policy,
        );
        let evidence: NativeShadowEvidence =
            serde_json::from_str(&evidence_json).expect("typed evidence");
        let durable = store
            .persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &evidence_json,
            )
            .expect("persist evidence");
        let first_receipt = store
            .map_durable_v2_to_bf3_receipt(&four_tuple, &durable)
            .expect("first receipt");
        store
            .complete_consumed(&mut authority, &mut exhaustion, &four_tuple, durable)
            .expect("terminal transition");

        assert_eq!(
            store.terminal_durable_evidence(&four_tuple),
            Some(&evidence)
        );
        assert_eq!(
            store.terminal_durable_evidence_digest(&four_tuple),
            Some(sha256_hex(evidence_json.as_bytes()).as_str())
        );
        let redelivered = store
            .map_terminal_v2_to_bf3_receipt(
                &four_tuple,
                NativeShadowTerminalReceiptBinding {
                    registry_version: &evidence.registry_version,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    candidate_digest: &candidate_digest,
                    submission_digest: &evidence.submission_digest,
                },
                &exhaustion,
            )
            .expect("terminal redelivery");
        assert_eq!(redelivered, first_receipt);
    }

    #[test]
    fn bf3_task_id_is_stable_while_instance_bindings_stay_in_the_artifact_root() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let base_tuple = registry.four_tuple(&registry.templates[0]);
        let base_registry_digest = sha256_hex(b"bf3-stable-task-registry");
        let policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"bf3-stable-task-candidate");
        let base_evidence: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &base_tuple,
            &candidate_digest,
            "bf3-stable-task",
            &policy,
        ))
        .expect("base evidence");
        let base = map_test_bf3_receipt(
            "stable-task-base",
            base_tuple.clone(),
            &base_registry_digest,
            policy.clone(),
            base_evidence.clone(),
        )
        .expect("base receipt");
        let expected_stable = boole_core::useful_work_bf6::NativeTaskIdentity::try_new(
            base_evidence["familyVersion"].as_str().unwrap(),
            Hex32::from_hex(base_evidence["templateId"].as_str().unwrap()).unwrap(),
            Hex32::from_hex(base_evidence["anchorDigest"].as_str().unwrap()).unwrap(),
        )
        .unwrap()
        .task_id()
        .as_hex32();
        assert_eq!(base.task_id, expected_stable);

        for (label, field) in [
            ("family", "familyVersion"),
            ("template", "templateId"),
            ("anchor", "anchorDigest"),
        ] {
            let mut tuple = base_tuple.clone();
            let mut evidence = base_evidence.clone();
            match field {
                "familyVersion" => {
                    tuple.family_version = "TEST-ONLY/OTHER-FAMILY-V1".to_string();
                    evidence[field] = serde_json::json!(tuple.family_version);
                }
                "templateId" => {
                    tuple.template_id = sha256_hex(b"bf3-other-template");
                    evidence[field] = serde_json::json!(tuple.template_id);
                }
                "anchorDigest" => {
                    evidence[field] = serde_json::json!(sha256_hex(b"bf3-other-anchor"));
                }
                _ => unreachable!(),
            }
            let changed = map_test_bf3_receipt(
                &format!("stable-task-material-{label}"),
                tuple,
                &base_registry_digest,
                policy.clone(),
                evidence,
            )
            .expect("changed task material maps");
            assert_ne!(changed.task_id, base.task_id, "{field}");
        }

        let mut challenge_tuple = base_tuple.clone();
        challenge_tuple.challenge_sha256 = sha256_hex(b"bf3-other-challenge");
        let mut challenge_evidence = base_evidence.clone();
        challenge_evidence["challengeSha256"] = serde_json::json!(challenge_tuple.challenge_sha256);
        let challenge = map_test_bf3_receipt(
            "stable-task-challenge",
            challenge_tuple,
            &base_registry_digest,
            policy.clone(),
            challenge_evidence,
        )
        .expect("challenge-bound instance maps");

        let mut epoch_tuple = base_tuple.clone();
        epoch_tuple.epoch += 1;
        let mut epoch_evidence = base_evidence.clone();
        epoch_evidence["epoch"] = serde_json::json!(epoch_tuple.epoch);
        let epoch = map_test_bf3_receipt(
            "stable-task-epoch",
            epoch_tuple,
            &base_registry_digest,
            policy.clone(),
            epoch_evidence,
        )
        .expect("epoch-bound instance maps");

        let registry_changed = map_test_bf3_receipt(
            "stable-task-registry",
            base_tuple,
            &sha256_hex(b"bf3-other-registry"),
            policy,
            base_evidence,
        )
        .expect("registry-bound instance maps");

        for receipt in [challenge, epoch, registry_changed] {
            assert_eq!(receipt.task_id, base.task_id);
            assert_ne!(receipt.artifact_root, base.artifact_root);
        }
    }

    #[test]
    fn durable_v2_semantic_rejects_preserve_exact_evidence_but_use_existing_bf3_reason_set() {
        let cases = [
            (
                "compile_or_hidden_test_failed",
                "compile-or-hidden-test-failed",
            ),
            ("forbidden_construct", "forbidden-construct"),
            ("malformed_patch_region", "malformed-patch-region"),
            ("outside_patch_modified", "outside-patch-modified"),
            ("patch_line_limit_exceeded", "patch-line-limit-exceeded"),
            ("patch_size_exceeded", "patch-size-exceeded"),
            ("submission_unreadable", "submission-unreadable"),
            ("checker_rejected", "compile-or-hidden-test-failed"),
            (
                "submission_resource_ceiling_breach",
                "compile-or-hidden-test-failed",
            ),
            (
                "checker_reported_reason_unconfirmed",
                "compile-or-hidden-test-failed",
            ),
        ];
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let registry_digest = sha256_hex(TEST_ONLY_REGISTRY_FIXTURE.as_bytes());

        for (checker_reason, receipt_reason) in cases {
            let (journal_path, _exhaustion_path) =
                scratch_journal_and_exhaustion_paths(checker_reason);
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, &registry_digest, || {
                bootstrap_challenge_state(&registry, template)
            });
            store
                .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
                .expect("begin execution");
            let candidate_digest = sha256_hex(format!("candidate-{checker_reason}").as_bytes());
            let mut evidence: serde_json::Value = serde_json::from_str(&test_evidence_json(
                &four_tuple,
                &candidate_digest,
                checker_reason,
            ))
            .expect("evidence JSON");
            evidence["verdict"] = serde_json::json!("deterministic_reject");
            evidence["reasonCode"] = serde_json::json!(checker_reason);
            let durable = store
                .persist_evidence(
                    &mut authority,
                    &four_tuple,
                    &candidate_digest,
                    &evidence.to_string(),
                )
                .expect("persist deterministic reject evidence");

            let receipt = store
                .map_durable_v2_to_bf3_receipt(&four_tuple, &durable)
                .expect("semantic reject maps to BF3");
            assert!(!receipt.accepted(), "{checker_reason}");
            assert_eq!(receipt.reject_label(), Some(receipt_reason));
        }
    }

    #[test]
    fn bf3_artifact_root_commits_every_deterministic_native_binding() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let base_tuple = registry.four_tuple(&registry.templates[0]);
        let base_registry_digest = sha256_hex(b"bf3-registry-base");
        let base_policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"bf3-candidate-base");
        let base_evidence: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &base_tuple,
            &candidate_digest,
            "bf3-root-base",
            &base_policy,
        ))
        .expect("base evidence");
        let base = map_test_bf3_receipt(
            "base",
            base_tuple.clone(),
            &base_registry_digest,
            base_policy.clone(),
            base_evidence.clone(),
        )
        .expect("base receipt");

        for (index, field) in [
            "submissionDigest",
            "anchorDigest",
            "candidateDigest",
            "intakeVersion",
            "checkerDigest",
            "policyDigest",
            "toolchainDigest",
            "registryVersion",
        ]
        .into_iter()
        .enumerate()
        {
            let mut evidence = base_evidence.clone();
            evidence[field] = if matches!(field, "intakeVersion" | "registryVersion") {
                serde_json::json!(format!("changed-{field}"))
            } else {
                serde_json::json!(sha256_hex(format!("changed-{field}").as_bytes()))
            };
            let mapped = map_test_bf3_receipt(
                &format!("field-{index}"),
                base_tuple.clone(),
                &base_registry_digest,
                base_policy.clone(),
                evidence,
            )
            .expect("changed binding maps");
            assert_ne!(base.artifact_root, mapped.artifact_root, "{field}");
        }

        for (index, field) in ["familyVersion", "templateId", "challengeSha256", "epoch"]
            .into_iter()
            .enumerate()
        {
            let mut four_tuple = base_tuple.clone();
            let mut evidence = base_evidence.clone();
            match field {
                "familyVersion" => {
                    four_tuple.family_version = "TEST-ONLY/CHANGED-V2".to_string();
                    evidence[field] = serde_json::json!(four_tuple.family_version);
                }
                "templateId" => {
                    four_tuple.template_id = sha256_hex(b"changed-template");
                    evidence[field] = serde_json::json!(four_tuple.template_id);
                }
                "challengeSha256" => {
                    four_tuple.challenge_sha256 = sha256_hex(b"changed-challenge");
                    evidence[field] = serde_json::json!(four_tuple.challenge_sha256);
                }
                "epoch" => {
                    four_tuple.epoch += 1;
                    evidence[field] = serde_json::json!(four_tuple.epoch);
                }
                _ => unreachable!(),
            }
            let mapped = map_test_bf3_receipt(
                &format!("identity-{index}"),
                four_tuple,
                &base_registry_digest,
                base_policy.clone(),
                evidence,
            )
            .expect("changed task binding maps");
            assert_ne!(base.artifact_root, mapped.artifact_root, "{field}");
        }

        let changed_policy = NativeShadowExecutionPolicyDigest::try_from(
            sha256_hex(b"changed-execution-policy").as_str(),
        )
        .expect("changed policy digest");
        let mut policy_evidence = base_evidence.clone();
        policy_evidence["executionPolicyDigest"] = serde_json::json!(changed_policy.as_str());
        let mapped = map_test_bf3_receipt(
            "execution-policy",
            base_tuple.clone(),
            &base_registry_digest,
            changed_policy,
            policy_evidence,
        )
        .expect("changed execution policy maps");
        assert_ne!(base.artifact_root, mapped.artifact_root);

        let mapped = map_test_bf3_receipt(
            "registry-digest",
            base_tuple.clone(),
            &sha256_hex(b"changed-registry-digest"),
            base_policy.clone(),
            base_evidence.clone(),
        )
        .expect("changed registry digest maps");
        assert_ne!(base.artifact_root, mapped.artifact_root);

        let mut reject_evidence = base_evidence;
        reject_evidence["verdict"] = serde_json::json!("deterministic_reject");
        reject_evidence["reasonCode"] = serde_json::json!("forbidden_construct");
        let mapped = map_test_bf3_receipt(
            "verdict-reason",
            base_tuple,
            &base_registry_digest,
            base_policy,
            reject_evidence,
        )
        .expect("changed verdict maps");
        assert_ne!(base.artifact_root, mapped.artifact_root);
    }

    #[test]
    fn bf3_mapping_rejects_nonsemantic_or_mismatched_evidence() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let registry_digest = sha256_hex(b"bf3-invalid-registry");
        let policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"bf3-invalid-candidate");
        let base: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "bf3-invalid",
            &policy,
        ))
        .expect("base evidence");

        for (label, verdict, reason) in [
            ("accepted-reject-reason", "accepted", "forbidden_construct"),
            ("reject-accepted-reason", "deterministic_reject", "accepted"),
            (
                "unknown-reject-reason",
                "deterministic_reject",
                "not_a_checker_reason",
            ),
        ] {
            let mut evidence = base.clone();
            evidence["verdict"] = serde_json::json!(verdict);
            evidence["reasonCode"] = serde_json::json!(reason);
            let (journal_path, _exhaustion_path) =
                scratch_journal_and_exhaustion_paths(&format!("bf3-invalid-{label}"));
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, &registry_digest, || {
                ChallengeState::ActiveFresh
            });
            store
                .begin_execution(&mut authority, &four_tuple, &policy)
                .expect("begin execution");
            assert!(matches!(
                store.persist_evidence(
                    &mut authority,
                    &four_tuple,
                    &candidate_digest,
                    &evidence.to_string(),
                ),
                Err(NativeShadowTransitionError::InvalidEvidenceContract(_))
            ));
        }

        assert_eq!(
            map_test_bf3_receipt(
                "bad-registry-digest",
                four_tuple.clone(),
                "not-a-sha256",
                policy.clone(),
                base.clone(),
            ),
            Err(NativeShadowReceiptMapError::InvalidDigest("registryDigest"))
        );

        let mut retryable = base;
        retryable["verdict"] = serde_json::json!("retryable_unavailable");
        retryable["reasonCode"] = serde_json::json!("resource_wall_limit");
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("bf3-retryable-no-receipt");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, &registry_digest, || {
            ChallengeState::ActiveFresh
        });
        store
            .begin_execution(&mut authority, &four_tuple, &policy)
            .expect("begin execution");
        assert!(matches!(
            store.persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &retryable.to_string(),
            ),
            Err(NativeShadowTransitionError::InvalidEvidence(_))
        ));

        for (label, field, value, expected_schema_error) in [
            (
                "legacy-evidence",
                "schema",
                "boole.native-shadow.evidence.v1",
                true,
            ),
            (
                "wrong-submission-schema",
                "submissionSchema",
                "boole.native-shadow.submission.v0",
                false,
            ),
        ] {
            let (journal_path, _exhaustion_path) = scratch_journal_and_exhaustion_paths(label);
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, &registry_digest, || {
                ChallengeState::ActiveFresh
            });
            store
                .begin_execution(&mut authority, &four_tuple, &policy)
                .expect("begin execution");
            let mut evidence: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
                &four_tuple,
                &candidate_digest,
                label,
                &policy,
            ))
            .expect("evidence");
            evidence[field] = serde_json::json!(value);
            let result = store.persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &evidence.to_string(),
            );
            if expected_schema_error {
                assert!(matches!(
                    result,
                    Err(NativeShadowTransitionError::InvalidEvidenceSchema)
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(NativeShadowTransitionError::InvalidEvidenceContract(_))
                ));
            }
        }
    }

    #[test]
    fn bf3_mapping_rejects_a_forged_durable_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let registry_digest = sha256_hex(b"bf3-capability-registry");
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("bf3-capability");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("journal authority");
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, &registry_digest, || {
            bootstrap_challenge_state(&registry, template)
        });
        store
            .begin_execution(&mut authority, &four_tuple, &test_execution_policy_digest())
            .expect("begin execution");
        let mut durable =
            persist_test_evidence(&mut store, &mut authority, &four_tuple, "bf3-capability");
        durable.evidence_digest = sha256_hex(b"forged-evidence-digest");

        assert_eq!(
            store.map_durable_v2_to_bf3_receipt(&four_tuple, &durable),
            Err(NativeShadowReceiptMapError::DurableEvidenceBindingMismatch)
        );
    }

    #[test]
    fn invalid_verdict_reason_pair_is_rejected_before_evidence_is_journaled() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"invalid-verdict-reason-candidate");
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("invalid-verdict-reason-persist");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");
        let _flight = recovery
            .store
            .begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &sha256_hex(b"invalid-verdict-reason-operation"),
                &candidate_digest,
            )
            .expect("begin");
        let before = std::fs::read(&journal_path).expect("journal before invalid evidence");
        let mut evidence: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "invalid-verdict-reason",
            &policy,
        ))
        .expect("typed evidence");
        evidence["reasonCode"] = serde_json::json!("forbidden_construct");

        assert!(matches!(
            recovery.store.persist_evidence(
                &mut authority,
                &four_tuple,
                &candidate_digest,
                &serde_json::to_string(&evidence).expect("evidence JSON"),
            ),
            Err(NativeShadowTransitionError::InvalidEvidenceContract(_))
        ));
        assert_eq!(
            std::fs::read(&journal_path).expect("journal after invalid evidence"),
            before
        );
    }

    #[test]
    fn bf3_mapping_excludes_operational_ids_and_resource_telemetry() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let registry_digest = sha256_hex(b"bf3-telemetry-registry");
        let policy = test_execution_policy_digest();
        let candidate_digest = sha256_hex(b"bf3-telemetry-candidate");
        let base: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "bf3-telemetry",
            &policy,
        ))
        .expect("base evidence");
        let plain = map_test_bf3_receipt(
            "telemetry-plain",
            four_tuple.clone(),
            &registry_digest,
            policy.clone(),
            base.clone(),
        )
        .expect("plain evidence maps");

        let mut operational = base;
        operational["operationId"] = serde_json::json!(sha256_hex(b"operation-id"));
        operational["resourceTelemetry"] = serde_json::json!({
            "wallMillis": 91,
            "peakMemoryBytes": 123456,
            "processes": 7
        });
        let with_telemetry = map_test_bf3_receipt(
            "telemetry-extra",
            four_tuple,
            &registry_digest,
            policy,
            operational,
        )
        .expect("operational metadata maps");

        assert_eq!(
            plain, with_telemetry,
            "operation identifiers and resource telemetry are not deterministic verdict inputs"
        );
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
    fn tracked_production_bytes_use_the_full_strict_authority_model() {
        let loaded = load_native_shadow_registry_from_bytes(PRODUCTION_REGISTRY_FIXTURE.as_bytes())
            .expect("strictly parse tracked registry bytes");
        assert_eq!(
            loaded.registry_digest,
            sha256_hex(PRODUCTION_REGISTRY_FIXTURE.as_bytes())
        );
        assert!(!loaded.registry.activation_allowed);
        assert!(loaded.registry.templates[0].non_issuable);
    }

    #[test]
    fn production_registry_bytes_reject_unknown_missing_and_duplicate_fields() {
        let unknown = PRODUCTION_REGISTRY_FIXTURE.replacen('{', "{\"unknown\":1,", 1);
        assert!(load_native_shadow_registry_from_bytes(unknown.as_bytes()).is_err());

        let missing = PRODUCTION_REGISTRY_FIXTURE.replacen("\"nonIssuable\": true", "", 1);
        assert_ne!(missing, PRODUCTION_REGISTRY_FIXTURE);
        assert!(load_native_shadow_registry_from_bytes(missing.as_bytes()).is_err());

        let duplicate = PRODUCTION_REGISTRY_FIXTURE.replacen(
            "  \"activationAllowed\": false,",
            "  \"activationAllowed\": false,\n  \"activationAllowed\": false,",
            1,
        );
        assert!(load_native_shadow_registry_from_bytes(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn production_authority_metadata_must_match_every_frozen_file_property() {
        let expected_len = TRACKED_REGISTRY_BYTES.len() as u64;
        assert!(
            validate_production_authority_metadata(true, 1, 0, 0, 0o444, expected_len,).is_ok()
        );

        for invalid in [
            (false, 1, 0, 0, 0o444, expected_len),
            (true, 2, 0, 0, 0o444, expected_len),
            (true, 1, 501, 0, 0o444, expected_len),
            (true, 1, 0, 20, 0o444, expected_len),
            (true, 1, 0, 0, 0o644, expected_len),
            (true, 1, 0, 0, 0o444, expected_len + 1),
        ] {
            assert!(
                validate_production_authority_metadata(
                    invalid.0, invalid.1, invalid.2, invalid.3, invalid.4, invalid.5,
                )
                .is_err(),
                "unsafe metadata tuple must fail closed: {invalid:?}"
            );
        }
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
        assert_eq!(
            PRODUCTION_REGISTRY_PATH, "/usr/share/boole/native-shadow/registry-v1.json",
            "production must open the installed root-owned authority, never a repository fixture"
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
    fn prepared_production_journal_directory_is_private_and_file_is_one_link() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let (journal_path, other_path) =
            scratch_journal_and_exhaustion_paths("production-authority-safe-dir");
        let directory = journal_path.parent().expect("journal parent");
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
            .expect("private state directory");
        let directory_metadata = std::fs::metadata(directory).expect("state directory metadata");
        let uid = directory_metadata.uid();
        let gid = directory_metadata.gid();

        let authority =
            NativeShadowJournalAuthority::open_prepared_production(&journal_path, uid, gid)
                .expect("prepared production journal opens");
        let metadata = std::fs::metadata(&journal_path).expect("journal metadata");
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.uid(), uid);
        assert_eq!(metadata.gid(), gid);
        assert_eq!(metadata.nlink(), 1);
        drop(authority);

        std::fs::hard_link(&journal_path, &other_path).expect("second hard link");
        assert!(matches!(
            NativeShadowJournalAuthority::open_prepared_production_dir(
                directory,
                journal_path.file_name().expect("journal name"),
                uid,
                gid,
            ),
            Err(NativeShadowJournalAuthorityError::UnsafePath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn prepared_production_journal_rejects_permissive_or_symlinked_directory() {
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

        let (journal_path, other_path) =
            scratch_journal_and_exhaustion_paths("production-authority-unsafe-dir");
        let directory = journal_path.parent().expect("journal parent");
        let directory_metadata = std::fs::metadata(directory).expect("state directory metadata");
        let uid = directory_metadata.uid();
        let gid = directory_metadata.gid();

        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o750))
            .expect("permissive state directory");
        assert!(matches!(
            NativeShadowJournalAuthority::open_prepared_production_dir(
                directory,
                journal_path.file_name().expect("journal name"),
                uid,
                gid,
            ),
            Err(NativeShadowJournalAuthorityError::UnsafePath(_))
        ));

        let real_directory = other_path.with_extension("real-dir");
        let linked_directory = other_path.with_extension("linked-dir");
        std::fs::create_dir(&real_directory).expect("real directory");
        std::fs::set_permissions(&real_directory, std::fs::Permissions::from_mode(0o700))
            .expect("private real directory");
        symlink(&real_directory, &linked_directory).expect("directory symlink");
        assert!(matches!(
            NativeShadowJournalAuthority::open_prepared_production_dir(
                &linked_directory,
                std::ffi::OsStr::new("replay-v1.ndjson"),
                uid,
                gid,
            ),
            Err(NativeShadowJournalAuthorityError::UnsafePath(_))
                | Err(NativeShadowJournalAuthorityError::Io(_, _))
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

    #[test]
    fn v3_begin_durably_binds_operation_and_candidate_before_in_flight() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-operation-one");
        let candidate_digest = sha256_hex(b"v3-candidate-one");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-operation-binding");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");

        let flight = recovery
            .store
            .begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            )
            .expect("durable v3 in-flight binding");

        assert_eq!(flight.operation_id_hex(), operation_id);
        assert_eq!(flight.candidate_digest(), candidate_digest);
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::InFlight)
        );
        let raw = std::fs::read_to_string(&journal_path).expect("journal");
        let last: serde_json::Value =
            serde_json::from_str(raw.lines().last().expect("v3 in-flight journal line"))
                .expect("v3 journal JSON");
        assert_eq!(last["kind"], "in_flight_v3");
        assert_eq!(last["operationIdHex"], operation_id);
        assert_eq!(last["candidateDigest"], candidate_digest);
        assert_eq!(last["executionPolicyDigest"], policy.as_str());
    }

    #[test]
    fn v3_retryable_rollback_is_durable_before_reactivating_the_challenge() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-retryable-operation");
        let candidate_digest = sha256_hex(b"v3-retryable-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-retryable-rollback");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");
        let flight = recovery
            .store
            .begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            )
            .expect("begin v3");

        recovery
            .store
            .retryable_rollback_v3(
                &mut authority,
                &flight,
                NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
            )
            .expect("durable retryable rollback");

        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
        assert!(!recovery.store.flight_bindings_v3.contains_key(&four_tuple));
        assert!(recovery.store.seen_operation_ids_v3.contains(&operation_id));
        let raw = std::fs::read_to_string(&journal_path).expect("journal");
        let last: serde_json::Value =
            serde_json::from_str(raw.lines().last().expect("rollback journal line"))
                .expect("rollback JSON");
        assert_eq!(last["kind"], "retryable_rollback_v3");
        assert_eq!(last["operationIdHex"], operation_id);
        assert_eq!(last["candidateDigest"], candidate_digest);
        assert_eq!(
            last["reasonCode"],
            NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable.as_str()
        );
    }

    #[cfg(unix)]
    #[test]
    fn v3_retryable_rollback_append_failure_leaves_memory_and_journal_in_flight() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-rollback-write-failure-operation");
        let candidate_digest = sha256_hex(b"v3-rollback-write-failure-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-rollback-write-failure");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");
        let flight = recovery
            .store
            .begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            )
            .expect("begin v3");
        let before = std::fs::read(&journal_path).expect("journal before failure");
        authority.fail_next_append_for_test();

        assert!(matches!(
            recovery.store.retryable_rollback_v3(
                &mut authority,
                &flight,
                NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
            ),
            Err(NativeShadowTransitionError::Durability(_))
        ));
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::InFlight)
        );
        assert_eq!(
            recovery
                .store
                .flight_bindings_v3
                .get(&four_tuple)
                .map(|binding| binding.operation_id_hex.as_str()),
            Some(operation_id.as_str())
        );
        assert!(recovery.store.seen_operation_ids_v3.contains(&operation_id));
        assert_eq!(flight.operation_id_hex(), operation_id);
        assert_eq!(flight.candidate_digest(), candidate_digest);
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn v3_operation_id_remains_globally_spent_after_rollback_and_restart() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-one-shot-operation");
        let first_candidate = sha256_hex(b"v3-first-candidate");
        let second_candidate = sha256_hex(b"v3-second-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-operation-restart-reuse");

        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &first_candidate,
                )
                .expect("first begin");
            recovery
                .store
                .retryable_rollback_v3(
                    &mut authority,
                    &flight,
                    NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
                )
                .expect("rollback");
        }

        let before = std::fs::read(&journal_path).expect("journal before reuse");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
        assert!(matches!(
            recovery.store.begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &operation_id,
                &second_candidate,
            ),
            Err(NativeShadowTransitionError::OperationIdReused(id)) if id == operation_id
        ));
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn v3_operation_id_is_global_across_distinct_four_tuples() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let first = registry.four_tuple(&registry.templates[0]);
        let mut second = first.clone();
        second.epoch = second.epoch.checked_add(1).expect("test epoch");
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-cross-tuple-operation");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-operation-cross-tuple");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut store = NativeShadowStateStore::default();
        for four_tuple in [&first, &second] {
            assert_eq!(
                store.resolve_with_execution_policy(four_tuple, "registry-digest", &policy, || {
                    ChallengeState::ActiveFresh
                },),
                ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
            );
        }
        let _flight = store
            .begin_execution_v3(
                &mut authority,
                &first,
                &policy,
                &operation_id,
                &sha256_hex(b"v3-cross-tuple-first-candidate"),
            )
            .expect("first tuple begins");
        let before = std::fs::read(&journal_path).expect("journal before collision");

        assert!(matches!(
            store.begin_execution_v3(
                &mut authority,
                &second,
                &policy,
                &operation_id,
                &sha256_hex(b"v3-cross-tuple-second-candidate"),
            ),
            Err(NativeShadowTransitionError::OperationIdReused(id)) if id == operation_id
        ));
        assert_eq!(
            store.rows.get(&second).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn torn_v3_rollback_tail_recovers_the_durable_in_flight_binding() {
        use std::io::Write as _;

        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-torn-rollback-operation");
        let candidate_digest = sha256_hex(b"v3-torn-rollback-candidate");
        let (journal_path, _other_path) = scratch_journal_and_exhaustion_paths("v3-torn-rollback");

        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("recovery");
            let _flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &candidate_digest,
                )
                .expect("begin v3");
            let mut raw = std::fs::OpenOptions::new()
                .append(true)
                .open(&journal_path)
                .expect("append torn tail");
            raw.write_all(b"{\"kind\":\"retryable_rollback_v3\"")
                .expect("write torn tail");
            raw.sync_all().expect("sync torn tail");
        }

        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("reopen authority");
        let replay = replay_native_shadow_journal(&mut authority).expect("stable replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::InFlight)
        );
        assert_eq!(replay.stuck_in_flight, vec![four_tuple.clone()]);
        assert_eq!(
            replay
                .flight_bindings_v3
                .get(&four_tuple)
                .map(|binding| binding.operation_id_hex.as_str()),
            Some(operation_id.as_str())
        );
        assert!(replay.seen_operation_ids_v3.contains(&operation_id));
        assert!(!std::fs::read_to_string(&journal_path)
            .expect("repaired journal")
            .contains("retryable_rollback_v3"));
    }

    #[test]
    fn replay_refuses_to_upgrade_a_legacy_v2_flight_with_a_v3_rollback() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v2-flight-v3-rollback-refused");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
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
            NativeShadowJournalEvent::RetryableRollbackV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: policy,
                operation_id_hex: sha256_hex(b"invented-operation"),
                candidate_digest: sha256_hex(b"invented-candidate"),
                reason: NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("event"))
                .expect("append event");
        }

        let err = replay_native_shadow_journal(&mut authority)
            .expect_err("a legacy v2 flight has no v3 binding to roll back");
        assert!(
            err.to_string()
                .contains("retryable rollback binding mismatch"),
            "unexpected replay failure: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn v3_begin_append_failure_does_not_reserve_operation_or_change_state() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-begin-write-failure-operation");
        let candidate_digest = sha256_hex(b"v3-begin-write-failure-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-begin-write-failure");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("durable bootstrap");
        let before = std::fs::read(&journal_path).expect("bootstrap bytes");
        authority.fail_next_append_for_test();

        assert!(matches!(
            recovery.store.begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            ),
            Err(NativeShadowTransitionError::Durability(_))
        ));
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
        assert!(!recovery.store.flight_bindings_v3.contains_key(&four_tuple));
        assert!(!recovery.store.seen_operation_ids_v3.contains(&operation_id));
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn replay_rejects_operation_id_collision_even_after_retryable_rollback() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-replay-collision-operation");
        let first_candidate = sha256_hex(b"v3-replay-collision-first");
        let second_candidate = sha256_hex(b"v3-replay-collision-second");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-replay-operation-collision");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        for event in [
            NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: policy.clone(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id.clone(),
                candidate_digest: first_candidate.clone(),
            },
            NativeShadowJournalEvent::RetryableRollbackV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id.clone(),
                candidate_digest: first_candidate,
                reason: NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
            },
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: "registry-digest".to_string(),
                execution_policy_digest: policy,
                operation_id_hex: operation_id,
                candidate_digest: second_candidate,
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("event"))
                .expect("append event");
        }

        let err = replay_native_shadow_journal(&mut authority)
            .expect_err("operation IDs are globally one-shot across rollbacks");
        assert!(
            err.to_string().contains("reuses operationIdHex"),
            "unexpected replay failure: {err}"
        );
    }

    #[test]
    fn v3_flight_refuses_evidence_for_a_different_candidate() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let bound_candidate = sha256_hex(b"v3-bound-candidate");
        let other_candidate = sha256_hex(b"v3-other-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-candidate-evidence-binding");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("recovery");
        let _flight = recovery
            .store
            .begin_execution_v3(
                &mut authority,
                &four_tuple,
                &policy,
                &sha256_hex(b"v3-candidate-binding-operation"),
                &bound_candidate,
            )
            .expect("begin v3");
        let before = std::fs::read(&journal_path).expect("journal before evidence");
        let evidence_json =
            test_evidence_v2_json(&four_tuple, &other_candidate, "v3-other", &policy);

        assert!(matches!(
            recovery.store.persist_evidence(
                &mut authority,
                &four_tuple,
                &other_candidate,
                &evidence_json,
            ),
            Err(NativeShadowTransitionError::EvidenceBindingMismatch(key)) if key == four_tuple
        ));
        assert!(!recovery.store.evidence_commits.contains_key(&four_tuple));
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn restarted_v3_flight_can_resume_its_exact_binding_and_roll_back() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-resume-operation");
        let candidate_digest = sha256_hex(b"v3-resume-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-resume-after-restart");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let _flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &candidate_digest,
                )
                .expect("begin v3");
        }

        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");
        let resumed = recovery
            .store
            .resume_in_flight_v3(
                VerifiedNativeShadowCleanupV3::for_test(&operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            )
            .expect("resume exact durable binding");
        recovery
            .store
            .retryable_rollback_v3(
                &mut authority,
                &resumed,
                NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
            )
            .expect("recovery rollback");
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
    }

    #[test]
    fn restarted_v3_flight_rejects_cleanup_for_a_different_operation_without_mutation() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-cleanup-bound-operation");
        let wrong_operation_id = sha256_hex(b"v3-cleanup-wrong-operation");
        let candidate_digest = sha256_hex(b"v3-cleanup-bound-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-cleanup-binding-mismatch");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let _flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &candidate_digest,
                )
                .expect("begin v3");
        }

        let before = std::fs::read(&journal_path).expect("journal before resume");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");

        assert!(matches!(
            recovery.store.resume_in_flight_v3(
                VerifiedNativeShadowCleanupV3::for_test(&wrong_operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            ),
            Err(NativeShadowTransitionError::CleanupBindingMismatch)
        ));
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::InFlight)
        );
        assert_eq!(
            std::fs::read(&journal_path).expect("journal after resume"),
            before
        );
    }

    #[test]
    fn recovered_legacy_v1_flight_cannot_resume_with_v3_cleanup_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"legacy-v1-cleanup-operation");
        let candidate_digest = sha256_hex(b"legacy-v1-cleanup-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("legacy-v1-v3-resume-refused");
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
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("legacy event"))
                .expect("append legacy event");
        }
        drop(authority);

        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("legacy recovery");

        assert!(matches!(
            recovery.store.resume_in_flight_v3(
                VerifiedNativeShadowCleanupV3::for_test(&operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            ),
            Err(NativeShadowTransitionError::FlightBindingMismatch(key)) if key == four_tuple
        ));
    }

    #[test]
    fn recovered_legacy_v2_flight_cannot_resume_with_v3_cleanup_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"legacy-v2-cleanup-operation");
        let candidate_digest = sha256_hex(b"legacy-v2-cleanup-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("legacy-v2-v3-resume-refused");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            recovery
                .store
                .begin_execution(&mut authority, &four_tuple, &policy)
                .expect("legacy v2 begin");
        }
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");

        assert!(matches!(
            recovery.store.resume_in_flight_v3(
                VerifiedNativeShadowCleanupV3::for_test(&operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            ),
            Err(NativeShadowTransitionError::FlightBindingMismatch(key)) if key == four_tuple
        ));
    }

    #[test]
    fn recovered_v3_flight_with_terminal_evidence_cannot_roll_back() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-evidence-cleanup-operation");
        let candidate_digest = sha256_hex(b"v3-evidence-cleanup-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-evidence-resume-refused");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let _flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &candidate_digest,
                )
                .expect("begin v3");
            let evidence_json =
                test_evidence_v2_json(&four_tuple, &candidate_digest, "v3-evidence", &policy);
            let _evidence = recovery
                .store
                .persist_evidence(
                    &mut authority,
                    &four_tuple,
                    &candidate_digest,
                    &evidence_json,
                )
                .expect("persist terminal evidence");
        }
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");

        assert!(matches!(
            recovery.store.resume_in_flight_v3(
                VerifiedNativeShadowCleanupV3::for_test(&operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            ),
            Err(NativeShadowTransitionError::RetryableAfterEvidence(key)) if key == four_tuple
        ));
    }

    #[test]
    fn recovered_v3_flight_with_terminal_evidence_finishes_without_reexecuting_checker() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let operation_id = sha256_hex(b"v3-recovered-evidence-operation");
        let candidate_digest = sha256_hex(b"v3-recovered-evidence-candidate");
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-recovered-evidence-terminalize");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let _flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &operation_id,
                    &candidate_digest,
                )
                .expect("begin v3");
            let evidence_json = test_evidence_v2_json(
                &four_tuple,
                &candidate_digest,
                "v3-recovered-evidence",
                &policy,
            );
            let _durable = recovery
                .store
                .persist_evidence(
                    &mut authority,
                    &four_tuple,
                    &candidate_digest,
                    &evidence_json,
                )
                .expect("persist evidence before simulated crash");
        }

        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");
        let durable = recovery
            .store
            .recover_pending_evidence_v3(
                VerifiedNativeShadowCleanupV3::for_test(&operation_id),
                &four_tuple,
                &policy,
                &operation_id,
                &candidate_digest,
            )
            .expect("recover exact durable evidence capability");
        recovery
            .store
            .complete_consumed(
                &mut authority,
                &mut recovery.exhaustion_ledger,
                &four_tuple,
                durable,
            )
            .expect("finish the already-decided terminal transition");

        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::Consumed)
        );
        assert!(recovery.exhaustion_ledger.contains(&four_tuple));
    }

    #[test]
    fn closed_local_replay_attempt_budget_is_durable_and_each_case_is_one_shot() {
        let (journal_path, _) = scratch_journal_and_exhaustion_paths("closed-local-attempt-budget");
        let policy = test_execution_policy_digest();
        let registry_digest = sha256_hex(b"closed-local-attempt-registry");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut attempts = NativeShadowGrantAttemptLedgerV1::default();

        for epoch in 0..3_u64 {
            let four_tuple = NativeShadowFourTuple {
                family_version: "family-v1".to_string(),
                template_id: sha256_hex(b"closed-local-attempt-template"),
                challenge_sha256: sha256_hex(b"closed-local-attempt-challenge"),
                epoch,
            };
            let _reservation = attempts
                .reserve(
                    &mut authority,
                    NativeShadowGrantAttemptFieldsV1 {
                        four_tuple: &four_tuple,
                        registry_digest: &registry_digest,
                        execution_policy_digest: &policy,
                        operation_id_hex: &sha256_hex(format!("operation-{epoch}").as_bytes()),
                        candidate_digest: &sha256_hex(format!("candidate-{epoch}").as_bytes()),
                        submission_digest: &sha256_hex(format!("submission-{epoch}").as_bytes()),
                        kind: NativeShadowGrantAttemptKindV1::Checker,
                    },
                )
                .expect("three checker rows fit the frozen budget");
        }
        let empty = NativeShadowFourTuple {
            family_version: "family-v1".to_string(),
            template_id: sha256_hex(b"closed-local-attempt-template"),
            challenge_sha256: sha256_hex(b"closed-local-attempt-challenge"),
            epoch: 3,
        };
        let _empty_reservation = attempts
            .reserve(
                &mut authority,
                NativeShadowGrantAttemptFieldsV1 {
                    four_tuple: &empty,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    operation_id_hex: &sha256_hex(b"operation-3"),
                    candidate_digest: &sha256_hex(b"candidate-3"),
                    submission_digest: &sha256_hex(b"submission-3"),
                    kind: NativeShadowGrantAttemptKindV1::PreIntake,
                },
            )
            .expect("the pre-intake row is the fourth and final matrix attempt");
        assert_eq!(attempts.total_attempts(), 4);
        assert_eq!(attempts.checker_attempts(), 3);
        assert!(matches!(
            attempts.reserve(
                &mut authority,
                NativeShadowGrantAttemptFieldsV1 {
                    four_tuple: &empty,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    operation_id_hex: &sha256_hex(b"different-operation"),
                    candidate_digest: &sha256_hex(b"different-candidate"),
                    submission_digest: &sha256_hex(b"different-submission"),
                    kind: NativeShadowGrantAttemptKindV1::PreIntake,
                },
            ),
            Err(NativeShadowGrantAttemptErrorV1::CaseAlreadyReserved)
        ));

        drop(authority);
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("reopen");
        let replay = replay_native_shadow_journal(&mut authority).expect("replay attempts");
        assert_eq!(replay.attempts.total_attempts(), 4);
        assert_eq!(replay.attempts.checker_attempts(), 3);
    }

    #[test]
    fn fourth_checker_attempt_is_refused_before_any_journal_append() {
        let (journal_path, _) = scratch_journal_and_exhaustion_paths("closed-local-checker-budget");
        let policy = test_execution_policy_digest();
        let registry_digest = sha256_hex(b"closed-local-checker-registry");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut attempts = NativeShadowGrantAttemptLedgerV1::default();

        for epoch in 0..4_u64 {
            let four_tuple = NativeShadowFourTuple {
                family_version: "family-v1".to_string(),
                template_id: sha256_hex(b"closed-local-checker-template"),
                challenge_sha256: sha256_hex(b"closed-local-checker-challenge"),
                epoch,
            };
            let result = attempts.reserve(
                &mut authority,
                NativeShadowGrantAttemptFieldsV1 {
                    four_tuple: &four_tuple,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    operation_id_hex: &sha256_hex(format!("checker-operation-{epoch}").as_bytes()),
                    candidate_digest: &sha256_hex(format!("checker-candidate-{epoch}").as_bytes()),
                    submission_digest: &sha256_hex(
                        format!("checker-submission-{epoch}").as_bytes(),
                    ),
                    kind: NativeShadowGrantAttemptKindV1::Checker,
                },
            );
            if epoch < 3 {
                let _reservation = result.expect("first three checker rows fit");
            } else {
                assert!(matches!(
                    result,
                    Err(NativeShadowGrantAttemptErrorV1::CheckerBudgetExceeded)
                ));
            }
        }
        assert_eq!(attempts.total_attempts(), 3);
        assert_eq!(attempts.checker_attempts(), 3);
        let lines = std::fs::read_to_string(&journal_path)
            .expect("journal")
            .lines()
            .count();
        assert_eq!(lines, 3, "refused attempt must not append a journal line");
    }

    #[test]
    fn checker_execution_can_begin_only_from_a_matching_durable_attempt_capability() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let registry_digest = sha256_hex(b"reserved-execution-registry");
        let operation_id = sha256_hex(b"reserved-execution-operation");
        let candidate_digest = sha256_hex(b"reserved-execution-candidate");
        let submission_digest = sha256_hex(b"reserved-execution-submission");
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("reserved-execution-capability");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        let mut recovery =
            recover_native_shadow_state(&registry, &registry_digest, &policy, &mut authority)
                .expect("recovery");
        let reservation = recovery
            .attempts
            .reserve(
                &mut authority,
                NativeShadowGrantAttemptFieldsV1 {
                    four_tuple: &four_tuple,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    operation_id_hex: &operation_id,
                    candidate_digest: &candidate_digest,
                    submission_digest: &submission_digest,
                    kind: NativeShadowGrantAttemptKindV1::Checker,
                },
            )
            .expect("durable checker reservation");

        let flight = recovery
            .store
            .begin_reserved_closed_local_replay_execution_v3(&mut authority, reservation)
            .expect("matching durable attempt starts one flight");
        assert_eq!(flight.operation_id_hex(), operation_id);
        assert_eq!(flight.candidate_digest(), candidate_digest);
    }

    #[test]
    fn checker_attempt_capability_cannot_cross_journal_authorities() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let registry_digest = sha256_hex(b"cross-journal-reserved-execution-registry");
        let operation_id = sha256_hex(b"cross-journal-reserved-execution-operation");
        let candidate_digest = sha256_hex(b"cross-journal-reserved-execution-candidate");
        let submission_digest = sha256_hex(b"cross-journal-reserved-execution-submission");
        let (first_journal_path, second_journal_path) =
            scratch_journal_and_exhaustion_paths("cross-journal-reserved-execution");
        let mut first_authority =
            NativeShadowJournalAuthority::open(&first_journal_path).expect("first authority");
        let mut first_attempts = NativeShadowGrantAttemptLedgerV1::default();
        let reservation = first_attempts
            .reserve(
                &mut first_authority,
                NativeShadowGrantAttemptFieldsV1 {
                    four_tuple: &four_tuple,
                    registry_digest: &registry_digest,
                    execution_policy_digest: &policy,
                    operation_id_hex: &operation_id,
                    candidate_digest: &candidate_digest,
                    submission_digest: &submission_digest,
                    kind: NativeShadowGrantAttemptKindV1::Checker,
                },
            )
            .expect("reservation is durable in the first journal");

        let mut second_authority =
            NativeShadowJournalAuthority::open(&second_journal_path).expect("second authority");
        let mut second_recovery = recover_native_shadow_state(
            &registry,
            &registry_digest,
            &policy,
            &mut second_authority,
        )
        .expect("second journal recovery");

        assert!(matches!(
            second_recovery
                .store
                .begin_reserved_closed_local_replay_execution_v3(
                    &mut second_authority,
                    reservation,
                ),
            Err(NativeShadowTransitionError::AttemptBindingMismatch(key)) if key == four_tuple
        ));
        assert_eq!(
            second_recovery
                .store
                .rows
                .get(&four_tuple)
                .map(|row| row.state),
            Some(ChallengeState::ActiveFresh),
            "a capability from another journal must not mutate this journal's row"
        );
    }

    #[test]
    fn closed_local_replay_bootstrap_can_activate_only_its_opaque_overlay_row() {
        let four_tuple = NativeShadowFourTuple {
            family_version: "TUPLE-STRUCT-PROJECT/TEST".to_string(),
            template_id: sha256_hex(b"closed-local-replay-bootstrap-template"),
            challenge_sha256: sha256_hex(b"closed-local-replay-bootstrap-challenge"),
            epoch: 0,
        };
        let policy = test_execution_policy_digest();
        let bootstrap = VerifiedNativeShadowReplayBootstrap::for_test(
            four_tuple.clone(),
            "CLOSED-LOCAL-REPLAY-OVERLAY-V1",
            sha256_hex(b"closed-local-replay-bootstrap-registry"),
            policy,
        );
        let mut store = NativeShadowStateStore::default();

        assert_eq!(
            store.resolve_verified_closed_local_replay(&bootstrap),
            ResolveOutcome::Bootstrapped(ChallengeState::ActiveFresh)
        );
        assert_eq!(
            store.resolve_verified_closed_local_replay(&bootstrap),
            ResolveOutcome::Existing(ChallengeState::ActiveFresh)
        );
        assert_eq!(store.rows.len(), 1);
        assert!(store.rows.contains_key(&four_tuple));
    }

    #[test]
    fn closed_local_replay_recovery_does_not_bootstrap_the_disabled_registry() {
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("closed-local-replay-empty-recovery");
        let registry_digest = sha256_hex(b"closed-local-replay-recovery-registry");
        let policy = test_execution_policy_digest();
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");

        let recovery = recover_verified_closed_local_replay_state(
            "CLOSED-LOCAL-REPLAY-OVERLAY-V1",
            &registry_digest,
            &policy,
            &mut authority,
        )
        .expect("empty replay journal recovers without inventing a row");

        assert!(recovery.store.rows.is_empty());
        assert_eq!(recovery.attempts.total_attempts(), 0);
        assert!(recovery.stuck_in_flight.is_empty());
    }

    #[test]
    fn closed_local_replay_recovery_rejects_evidence_for_another_registry_version() {
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("closed-local-wrong-registry-version");
        let registry_digest = sha256_hex(b"closed-local-registry-version-registry");
        let policy = test_execution_policy_digest();
        let four_tuple = NativeShadowFourTuple {
            family_version: "TUPLE-STRUCT-PROJECT/TEST".to_string(),
            template_id: sha256_hex(b"closed-local-registry-version-template"),
            challenge_sha256: sha256_hex(b"closed-local-registry-version-challenge"),
            epoch: 1,
        };
        let operation_id = sha256_hex(b"closed-local-registry-version-operation");
        let candidate_digest = sha256_hex(b"closed-local-registry-version-candidate");
        let mut evidence: serde_json::Value = serde_json::from_str(&test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "closed-local-registry-version",
            &policy,
        ))
        .expect("evidence");
        evidence["registryVersion"] = serde_json::json!("WRONG-OVERLAY-VERSION");
        let submission_digest = evidence["submissionDigest"]
            .as_str()
            .expect("submission digest")
            .to_string();
        let evidence_json = serde_json::to_string(&evidence).expect("evidence JSON");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        for event in [
            NativeShadowJournalEvent::GrantAttemptReservedV1 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id.clone(),
                candidate_digest: candidate_digest.clone(),
                submission_digest,
                attempt_kind: NativeShadowGrantAttemptKindV1::Checker,
            },
            NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id,
                candidate_digest: candidate_digest.clone(),
            },
            NativeShadowJournalEvent::EvidenceV2 {
                four_tuple,
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                candidate_digest,
                evidence_digest: sha256_hex(evidence_json.as_bytes()),
                evidence_json,
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("event"))
                .expect("append");
        }

        let error = match recover_verified_closed_local_replay_state(
            "EXPECTED-OVERLAY-VERSION",
            &registry_digest,
            &policy,
            &mut authority,
        ) {
            Ok(_) => panic!("wrong evidence registryVersion must fail startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("registryVersion"));
    }

    #[test]
    fn closed_local_replay_recovery_rejects_in_flight_without_prior_durable_attempt() {
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("closed-local-missing-attempt");
        let registry_digest = sha256_hex(b"closed-local-missing-attempt-registry");
        let policy = test_execution_policy_digest();
        let four_tuple = NativeShadowFourTuple {
            family_version: "TUPLE-STRUCT-PROJECT/TEST".to_string(),
            template_id: sha256_hex(b"closed-local-missing-attempt-template"),
            challenge_sha256: sha256_hex(b"closed-local-missing-attempt-challenge"),
            epoch: 2,
        };
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        for event in [
            NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple,
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: sha256_hex(b"closed-local-missing-attempt-operation"),
                candidate_digest: sha256_hex(b"closed-local-missing-attempt-candidate"),
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("event"))
                .expect("append");
        }

        let error = match recover_verified_closed_local_replay_state(
            "EXPECTED-OVERLAY-VERSION",
            &registry_digest,
            &policy,
            &mut authority,
        ) {
            Ok(_) => panic!("InFlight without a prior durable attempt must fail startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("durable checker attempt"));
    }

    #[test]
    fn closed_local_replay_recovery_rejects_evidence_with_another_attempt_submission() {
        let (journal_path, _) =
            scratch_journal_and_exhaustion_paths("closed-local-evidence-attempt-drift");
        let registry_digest = sha256_hex(b"closed-local-evidence-attempt-registry");
        let policy = test_execution_policy_digest();
        let four_tuple = NativeShadowFourTuple {
            family_version: "TUPLE-STRUCT-PROJECT/TEST".to_string(),
            template_id: sha256_hex(b"closed-local-evidence-attempt-template"),
            challenge_sha256: sha256_hex(b"closed-local-evidence-attempt-challenge"),
            epoch: 2,
        };
        let operation_id = sha256_hex(b"closed-local-evidence-attempt-operation");
        let candidate_digest = sha256_hex(b"closed-local-evidence-attempt-candidate");
        let evidence_json = test_evidence_v2_json(
            &four_tuple,
            &candidate_digest,
            "closed-local-evidence-attempt",
            &policy,
        );
        let evidence: NativeShadowEvidence =
            serde_json::from_str(&evidence_json).expect("evidence");
        let mut authority = NativeShadowJournalAuthority::open(&journal_path).expect("authority");
        for event in [
            NativeShadowJournalEvent::GrantAttemptReservedV1 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id.clone(),
                candidate_digest: candidate_digest.clone(),
                submission_digest: sha256_hex(b"another-submission"),
                attempt_kind: NativeShadowGrantAttemptKindV1::Checker,
            },
            NativeShadowJournalEvent::BootstrapV2 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                state: ChallengeState::ActiveFresh,
            },
            NativeShadowJournalEvent::InFlightV3 {
                four_tuple: four_tuple.clone(),
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                operation_id_hex: operation_id,
                candidate_digest: candidate_digest.clone(),
            },
            NativeShadowJournalEvent::EvidenceV2 {
                four_tuple,
                registry_digest: registry_digest.clone(),
                execution_policy_digest: policy.clone(),
                candidate_digest,
                evidence_digest: sha256_hex(evidence_json.as_bytes()),
                evidence_json,
            },
        ] {
            authority
                .append_line(&serde_json::to_string(&event).expect("event"))
                .expect("append");
        }

        let error = match recover_verified_closed_local_replay_state(
            &evidence.registry_version,
            &registry_digest,
            &policy,
            &mut authority,
        ) {
            Ok(_) => panic!("evidence for another durable submission must fail startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("durable checker attempt"));
    }

    #[test]
    fn v3_rollback_cannot_downgrade_the_next_attempt_to_v2() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        let (journal_path, _other_path) =
            scratch_journal_and_exhaustion_paths("v3-rollback-v2-downgrade");
        {
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("first authority");
            let mut recovery =
                recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                    .expect("first recovery");
            let flight = recovery
                .store
                .begin_execution_v3(
                    &mut authority,
                    &four_tuple,
                    &policy,
                    &sha256_hex(b"v3-downgrade-operation"),
                    &sha256_hex(b"v3-downgrade-candidate"),
                )
                .expect("begin v3");
            recovery
                .store
                .retryable_rollback_v3(
                    &mut authority,
                    &flight,
                    NativeShadowRetryableReasonV3::ContainmentEnvironmentUnavailable,
                )
                .expect("rollback v3");
        }
        let before = std::fs::read(&journal_path).expect("journal before downgrade");
        let mut authority =
            NativeShadowJournalAuthority::open(&journal_path).expect("second authority");
        let mut recovery =
            recover_native_shadow_state(&registry, "registry-digest", &policy, &mut authority)
                .expect("second recovery");

        assert!(matches!(
            recovery
                .store
                .begin_execution(&mut authority, &four_tuple, &policy),
            Err(NativeShadowTransitionError::V3Required(key)) if key == four_tuple
        ));
        assert_eq!(
            recovery.store.rows.get(&four_tuple).map(|row| row.state),
            Some(ChallengeState::ActiveFresh)
        );
        assert_eq!(std::fs::read(&journal_path).expect("journal"), before);
    }

    #[test]
    fn replay_rejects_deterministic_reason_disguised_as_retryable_rollback() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let four_tuple = registry.four_tuple(&registry.templates[0]);
        let policy = test_execution_policy_digest();
        for reason in ["accepted", "checker_rejected", "arbitrary_retryable"] {
            let operation_id = sha256_hex(format!("v3-forged-{reason}-operation").as_bytes());
            let candidate_digest = sha256_hex(format!("v3-forged-{reason}-candidate").as_bytes());
            let (journal_path, _other_path) =
                scratch_journal_and_exhaustion_paths(&format!("v3-forged-{reason}"));
            let mut authority =
                NativeShadowJournalAuthority::open(&journal_path).expect("authority");
            for event in [
                NativeShadowJournalEvent::BootstrapV2 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: policy.clone(),
                    state: ChallengeState::ActiveFresh,
                },
                NativeShadowJournalEvent::InFlightV3 {
                    four_tuple: four_tuple.clone(),
                    registry_digest: "registry-digest".to_string(),
                    execution_policy_digest: policy.clone(),
                    operation_id_hex: operation_id.clone(),
                    candidate_digest: candidate_digest.clone(),
                },
            ] {
                authority
                    .append_line(&serde_json::to_string(&event).expect("event"))
                    .expect("append event");
            }
            let forged = serde_json::json!({
                "kind": "retryable_rollback_v3",
                "familyVersion": four_tuple.family_version,
                "templateId": four_tuple.template_id,
                "challengeSha256": four_tuple.challenge_sha256,
                "epoch": four_tuple.epoch,
                "registryDigest": "registry-digest",
                "executionPolicyDigest": policy.as_str(),
                "operationIdHex": operation_id,
                "candidateDigest": candidate_digest,
                "reasonCode": reason,
            });
            authority
                .append_line(&forged.to_string())
                .expect("append forged rollback");

            let err = replay_native_shadow_journal(&mut authority)
                .expect_err("non-retryable reason cannot be journaled as retryable");
            assert!(
                err.to_string().contains("unknown variant"),
                "unexpected replay failure for {reason}: {err}"
            );
        }
    }
}
