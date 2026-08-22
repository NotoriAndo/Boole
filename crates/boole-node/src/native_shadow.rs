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
//! three-ordered-check bootstrap rule that resolves every registry-declared
//! four-tuple to `Disabled`, `Exhausted` or `Active(fresh)`, the
//! `Active(fresh)` -> `InFlight` -> `Consumed` state machine with its
//! durable NDJSON journal, and boot-time recovery that replays that journal
//! and withholds any row still `InFlight` (fails closed — see
//! `NativeShadowJournalReplay`) pending the later containment slice that can
//! actually confirm its cleanup. This module is not wired into
//! `local_node.rs` or any HTTP route, consistent with that document's
//! section 1 non-goals (no route, no `boole-node` server change, until
//! implementation of that route itself is undertaken); section 7's
//! OS-level `flock` (step 1) and "begin serving requests" (step 5) are
//! process/route-wiring concerns that belong to that later work too.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

/// Operational state key and permanent exhaustion-ledger key (spec section 4,
/// items 1 and 2 — the two now share one identity):
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
/// returns it, by construction — see `bootstrap_challenge_state`'s
/// three-way return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChallengeState {
    ActiveFresh,
    InFlight,
    Consumed,
    Exhausted,
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
}

impl std::fmt::Display for NativeShadowRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "native-shadow registry read failed: {err}"),
            Self::Json(err) => write!(f, "native-shadow registry not valid JSON: {err}"),
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

pub(crate) fn load_native_shadow_registry(
    path: impl AsRef<Path>,
) -> Result<LoadedNativeShadowRegistry, NativeShadowRegistryError> {
    let raw = std::fs::read(path.as_ref()).map_err(NativeShadowRegistryError::Io)?;
    let registry_digest = sha256_hex(&raw);
    let registry: NativeShadowRegistry =
        serde_json::from_slice(&raw).map_err(NativeShadowRegistryError::Json)?;
    Ok(LoadedNativeShadowRegistry {
        registry,
        registry_digest,
    })
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
pub(crate) const PRODUCTION_REGISTRY_PATH: &str = "fixtures/native-shadow/registry-v1.json";

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

/// Permanent, `registryDigest`-independent exhaustion ledger (spec section 4,
/// item 2 and section 6, check 2). Mirrors `FileProofDedupLedger`'s shape: an
/// in-memory `HashSet` mirror rebuilt by replaying an NDJSON file on
/// `recover`, appended to durably one line at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum NativeShadowExhaustionEvent {
    Exhausted {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
    },
}

#[derive(Debug, Default)]
pub(crate) struct FileNativeShadowExhaustionLedger {
    exhausted: HashSet<NativeShadowFourTuple>,
}

impl FileNativeShadowExhaustionLedger {
    /// Build an in-memory ledger by replaying the NDJSON file at `path`.
    /// Returns an empty ledger if the file does not yet exist — this is the
    /// "brand-new node, completely empty exhaustion ledger" case RED gate 5
    /// depends on.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: NativeShadowExhaustionEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!(
                    "nativeShadowExhaustionLedger: line {} invalid JSON: {}",
                    i + 1,
                    err
                )
            })?;
            ledger.apply(event);
        }
        Ok(ledger)
    }

    pub fn contains(&self, four_tuple: &NativeShadowFourTuple) -> bool {
        self.exhausted.contains(four_tuple)
    }

    /// Durably append an exhaustion record. Idempotent: returns `Ok(false)`
    /// with no write when the four-tuple is already recorded, matching
    /// `FileProofDedupLedger::append_credit`'s convention.
    pub fn append_exhausted(
        &mut self,
        path: impl AsRef<Path>,
        four_tuple: &NativeShadowFourTuple,
    ) -> anyhow::Result<bool> {
        if self.contains(four_tuple) {
            return Ok(false);
        }
        let event = NativeShadowExhaustionEvent::Exhausted {
            four_tuple: four_tuple.clone(),
        };
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(&event)?)?;
        self.apply(event);
        Ok(true)
    }

    fn apply(&mut self, event: NativeShadowExhaustionEvent) {
        match event {
            NativeShadowExhaustionEvent::Exhausted { four_tuple } => {
                self.exhausted.insert(four_tuple);
            }
        }
    }
}

/// Spec section 6's corrected bootstrap rule, three ordered checks:
/// 1. static issuability gate (checked first) — either flag forbidding
///    issuance forces `Disabled`, regardless of the exhaustion ledger;
/// 2. the permanent exhaustion ledger, checked only if 1 passes;
/// 3. `Active(fresh)`, reached only if 1 and 2 both pass.
///
/// This exact order is what closes F1 (a `nonIssuable` fixture could
/// otherwise bootstrap `Active(fresh)` on a node with an empty ledger) and
/// what RED gate 6 pins: static issuability always takes precedence over the
/// ledger, never the reverse.
pub(crate) fn bootstrap_challenge_state(
    registry: &NativeShadowRegistry,
    template: &NativeShadowTemplate,
    ledger: &FileNativeShadowExhaustionLedger,
) -> ChallengeState {
    if !registry.is_statically_issuable(template) {
        return ChallengeState::Disabled;
    }
    let four_tuple = registry.four_tuple(template);
    if ledger.contains(&four_tuple) {
        return ChallengeState::Exhausted;
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
    /// row is left completely untouched; no second, parallel row is ever
    /// created for this four-tuple.
    RegistryDrift,
}

impl NativeShadowStateStore {
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
                ResolveOutcome::RegistryDrift
            };
        }
        let state = bootstrap();
        self.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state,
                registry_digest: registry_digest.to_string(),
            },
        );
        ResolveOutcome::Bootstrapped(state)
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
        let event = NativeShadowJournalEvent::InFlight {
            four_tuple: four_tuple.clone(),
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

    /// Spec sections 5, 6 and 7: `InFlight` -> `Consumed`, unconditionally
    /// paired with a durable exhaustion-ledger append for the same
    /// four-tuple — every challenge this module governs is one-shot (spec
    /// section 6's closing paragraph), so reaching `Consumed` always means
    /// this exact four-tuple must never be issuable again, regardless of
    /// what a future registry snapshot might otherwise say.
    ///
    /// The exhaustion-ledger append happens **before** the `Consumed`
    /// journal event, not after: if a crash lands between the two writes,
    /// the exhaustion ledger alone already carries the permanent fact that
    /// matters (this four-tuple must never run again), whereas the reverse
    /// order could leave a `Consumed` journal line on disk with no
    /// permanent record backing it.
    ///
    /// Persisting evidence for the completed execution is the **caller's**
    /// responsibility, done before calling this function (spec section 7's
    /// evidence-before-terminal-transition ordering) — this module has no
    /// evidence store of its own and cannot enforce that ordering on the
    /// caller's behalf.
    pub fn complete_consumed(
        &mut self,
        journal_path: impl AsRef<Path>,
        exhaustion_ledger: &mut FileNativeShadowExhaustionLedger,
        exhaustion_ledger_path: impl AsRef<Path>,
        four_tuple: &NativeShadowFourTuple,
    ) -> Result<(), NativeShadowTransitionError> {
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
        exhaustion_ledger
            .append_exhausted(exhaustion_ledger_path, four_tuple)
            .map_err(NativeShadowTransitionError::Durability)?;
        let event = NativeShadowJournalEvent::Consumed {
            four_tuple: four_tuple.clone(),
        };
        let line = serde_json::to_string(&event)
            .map_err(|err| NativeShadowTransitionError::Durability(anyhow::Error::from(err)))?;
        append_ndjson_line_durable(journal_path.as_ref(), &line)
            .map_err(NativeShadowTransitionError::Durability)?;
        self.rows
            .get_mut(four_tuple)
            .expect("row checked above")
            .state = ChallengeState::Consumed;
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
        }
    }
}

impl std::error::Error for NativeShadowTransitionError {}

/// Durable per-key state-transition journal (spec section 7): `InFlight`
/// records intent durably before a checker execution begins; `Consumed`
/// records the terminal fact after evidence for it already exists. Distinct
/// from the permanent exhaustion ledger above — this journal is
/// per-execution bookkeeping crash recovery uses to find rows left
/// mid-flight; the exhaustion ledger's separate job is blocking revival
/// forever, independent of `registryDigest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum NativeShadowJournalEvent {
    InFlight {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
    },
    Consumed {
        #[serde(flatten)]
        four_tuple: NativeShadowFourTuple,
    },
}

/// Outcome of replaying the durable state-transition journal alone, before
/// any registry-driven bootstrap of four-tuples the journal never mentions
/// (spec section 7, steps 2-3). `stuck_in_flight` holds every four-tuple
/// whose last journaled event was `InFlight` with no later `Consumed` —
/// these are deliberately **excluded** from `resolved` and deliberately
/// **not** auto-resolved to any state, because resolving them for real
/// requires confirming the OS-level containment cleanup (spec section 9)
/// this module does not implement (a later, separate slice). A node
/// discovering a non-empty `stuck_in_flight` set must fail closed for those
/// specific four-tuples — neither silently reverted to `Active(fresh)` nor
/// silently served as `InFlight` — until that later slice exists to resolve
/// them per spec section 7's per-record procedure.
#[derive(Debug, Default)]
pub(crate) struct NativeShadowJournalReplay {
    pub resolved: HashMap<NativeShadowFourTuple, ChallengeState>,
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
    for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
        let event: NativeShadowJournalEvent = serde_json::from_str(line).map_err(|err| {
            anyhow::anyhow!("nativeShadowJournal: line {} invalid JSON: {}", i + 1, err)
        })?;
        match event {
            NativeShadowJournalEvent::InFlight { four_tuple } => {
                resolved.insert(four_tuple, ChallengeState::InFlight);
            }
            NativeShadowJournalEvent::Consumed { four_tuple } => {
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
        stuck_in_flight,
    })
}

/// Full boot-time recovery (spec section 7, steps 2-4). Step 1's OS-level
/// `flock` and step 5's "begin serving requests" are process/route-wiring
/// concerns outside this module's scope, same as the rest of this module.
///
/// Builds the servable state store from: the journal replay's resolved
/// `Consumed` rows (`InFlight` rows are withheld — see
/// `NativeShadowJournalReplay`), plus a fresh section-6 bootstrap for every
/// registry-declared four-tuple the journal never mentions at all. Every
/// four-tuple in `stuck_in_flight` is guaranteed **absent** from `store` —
/// a caller built on top of this module later must treat that set as
/// unavailable, not re-bootstrap it, until a later slice resolves it.
pub(crate) struct NativeShadowRecovery {
    pub store: NativeShadowStateStore,
    pub exhaustion_ledger: FileNativeShadowExhaustionLedger,
    pub stuck_in_flight: Vec<NativeShadowFourTuple>,
}

pub(crate) fn recover_native_shadow_state(
    registry: &NativeShadowRegistry,
    registry_digest: &str,
    journal_path: impl AsRef<Path>,
    exhaustion_ledger_path: impl AsRef<Path>,
) -> anyhow::Result<NativeShadowRecovery> {
    let replay = replay_native_shadow_journal(journal_path)?;
    let exhaustion_ledger = FileNativeShadowExhaustionLedger::recover(exhaustion_ledger_path)?;
    let stuck: HashSet<NativeShadowFourTuple> = replay.stuck_in_flight.iter().cloned().collect();

    let mut store = NativeShadowStateStore::default();
    for (four_tuple, state) in &replay.resolved {
        if stuck.contains(four_tuple) {
            continue; // withheld -- see NativeShadowJournalReplay's doc comment
        }
        store.rows.insert(
            four_tuple.clone(),
            NativeShadowStateRow {
                state: *state,
                registry_digest: registry_digest.to_string(),
            },
        );
    }
    for template in &registry.templates {
        let four_tuple = registry.four_tuple(template);
        if stuck.contains(&four_tuple) {
            continue; // fail closed -- never bootstrap over a row still InFlight
        }
        store.resolve(&four_tuple, registry_digest, || {
            bootstrap_challenge_state(registry, template, &exhaustion_ledger)
        });
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

    fn scratch_ledger_path(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "boole-native-shadow-exhaustion-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.join("exhaustion-ledger.ndjson")
    }

    /// Like `scratch_ledger_path`, but for tests that need both the
    /// state-transition journal and the exhaustion ledger side by side
    /// under one shared scratch directory.
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

    // -- RED gate 5: production fixture bootstraps Disabled ----------------

    #[test]
    fn production_fixture_bootstraps_disabled_on_empty_ledger() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let ledger = FileNativeShadowExhaustionLedger::default();

        let state = bootstrap_challenge_state(&registry, template, &ledger);

        assert_eq!(
            state,
            ChallengeState::Disabled,
            "a nonIssuable + activationAllowed:false fixture must never bootstrap \
             Active(fresh), even on a brand-new node with a completely empty ledger"
        );
    }

    // -- RED gate 6: static-flags-first precedence over the ledger ---------

    #[test]
    fn ledger_recorded_four_tuple_still_issuable_bootstraps_exhausted() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);

        let mut ledger = FileNativeShadowExhaustionLedger::default();
        let path = scratch_ledger_path("gate6-exhausted");
        ledger
            .append_exhausted(&path, &four_tuple)
            .expect("append exhausted");

        let state = bootstrap_challenge_state(&registry, template, &ledger);

        assert_eq!(
            state,
            ChallengeState::Exhausted,
            "a ledger-recorded four-tuple whose current static flags still \
             permit issuance must bootstrap Exhausted, not Active(fresh)"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn ledger_recorded_four_tuple_now_statically_disabled_bootstraps_disabled() {
        // Same four-tuple as the ledger-recorded case above, but this time
        // the *current* registry snapshot forbids issuance — the production
        // fixture's own four-tuple, recorded in the ledger as if it had
        // legitimately run under some past, still-undesigned issuable
        // snapshot. Static issuability must still win.
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);

        let mut ledger = FileNativeShadowExhaustionLedger::default();
        let path = scratch_ledger_path("gate6-disabled-precedence");
        ledger
            .append_exhausted(&path, &four_tuple)
            .expect("append exhausted");

        let state = bootstrap_challenge_state(&registry, template, &ledger);

        assert_eq!(
            state,
            ChallengeState::Disabled,
            "static issuability (check 1) must take precedence over the \
             exhaustion ledger (check 2) — a four-tuple whose current static \
             flags forbid issuance bootstraps Disabled regardless of what the \
             ledger separately records"
        );

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    // -- RED gate 7: test-only registry fixture -----------------------------

    #[test]
    fn test_only_fixture_bootstraps_active_fresh_on_empty_ledger() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let ledger = FileNativeShadowExhaustionLedger::default();

        let state = bootstrap_challenge_state(&registry, template, &ledger);

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
        let empty_ledger = FileNativeShadowExhaustionLedger::default();

        for (registry, template) in [
            (&production, &production.templates[0]),
            (&test_only, &test_only.templates[0]),
        ] {
            let state = bootstrap_challenge_state(registry, template, &empty_ledger);
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
        let ledger = FileNativeShadowExhaustionLedger::default();

        let mut store = NativeShadowStateStore::default();
        let first = store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template, &ledger)
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
            ResolveOutcome::RegistryDrift,
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
        let ledger = FileNativeShadowExhaustionLedger::default();
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("begin-execution");

        let mut store = NativeShadowStateStore::default();
        let outcome = store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template, &ledger)
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
        let ledger = FileNativeShadowExhaustionLedger::default();
        let (journal_path, _exhaustion_path) =
            scratch_journal_and_exhaustion_paths("begin-execution-wrong-state");

        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template, &ledger)
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
    fn complete_consumed_moves_in_flight_to_consumed_and_pairs_the_exhaustion_ledger() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("complete-consumed");

        let mut ledger = FileNativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template, &ledger)
        });
        store
            .begin_execution(&journal_path, &four_tuple)
            .expect("Active(fresh) -> InFlight");

        store
            .complete_consumed(&journal_path, &mut ledger, &exhaustion_path, &four_tuple)
            .expect("InFlight -> Consumed must succeed");

        assert_eq!(
            store.resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Consumed)
        );
        assert!(
            ledger.contains(&four_tuple),
            "reaching Consumed must unconditionally pair with an exhaustion-ledger append \
             (spec section 6: every challenge this module governs is one-shot)"
        );

        // Durable on both fronts, independently re-readable from disk.
        let replay = replay_native_shadow_journal(&journal_path).expect("replay");
        assert_eq!(
            replay.resolved.get(&four_tuple),
            Some(&ChallengeState::Consumed)
        );
        let reloaded_ledger =
            FileNativeShadowExhaustionLedger::recover(&exhaustion_path).expect("recover");
        assert!(reloaded_ledger.contains(&four_tuple));
    }

    #[test]
    fn complete_consumed_refuses_a_row_that_is_not_in_flight() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("complete-consumed-wrong-state");

        let mut ledger = FileNativeShadowExhaustionLedger::default();
        let mut store = NativeShadowStateStore::default();
        store.resolve(&four_tuple, "digest-v1", || {
            bootstrap_challenge_state(&registry, template, &ledger)
        });

        let err = store
            .complete_consumed(&journal_path, &mut ledger, &exhaustion_path, &four_tuple)
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
            "a refused transition must not append to the exhaustion ledger"
        );
    }

    // -- restart recovery (spec section 7, steps 2-4) -----------------------

    #[test]
    fn recovery_rebuilds_a_consumed_row_from_the_journal_alone() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-consumed");

        // Simulate a full lifecycle on one "process", then recover as if a
        // brand-new process just started against the same durable files.
        {
            let mut ledger = FileNativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template, &ledger)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
            store
                .complete_consumed(&journal_path, &mut ledger, &exhaustion_path, &four_tuple)
                .expect("complete");
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path, &exhaustion_path)
                .expect("recover");
        assert!(recovery.stuck_in_flight.is_empty());
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Consumed)
        );
        assert!(recovery.exhaustion_ledger.contains(&four_tuple));
    }

    #[test]
    fn recovery_withholds_a_stuck_in_flight_row_instead_of_serving_or_reverting_it() {
        let registry = parse_fixture(TEST_ONLY_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-stuck-in-flight");

        // Crash right after Active(fresh) -> InFlight, before Consumed.
        {
            let ledger = FileNativeShadowExhaustionLedger::default();
            let mut store = NativeShadowStateStore::default();
            store.resolve(&four_tuple, "digest-v1", || {
                bootstrap_challenge_state(&registry, template, &ledger)
            });
            store
                .begin_execution(&journal_path, &four_tuple)
                .expect("begin");
        }

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path, &exhaustion_path)
                .expect("recover");
        assert_eq!(recovery.stuck_in_flight, vec![four_tuple.clone()]);

        // Fail closed: this module has no containment-cleanup capability
        // (spec section 9, a later slice) to confirm the stuck row's
        // process/cgroup was actually torn down, so it must be neither
        // silently reverted to Active(fresh) nor silently served as
        // InFlight -- the row must be simply absent from the servable store.
        let outcome = recovery
            .store
            .resolve(&four_tuple, "digest-v1", || ChallengeState::ActiveFresh);
        assert!(
            matches!(outcome, ResolveOutcome::Bootstrapped(_)),
            "the stuck row must be genuinely absent from the recovered store, \
             not silently present as Active(fresh) or InFlight — a caller \
             that (incorrectly, for a later slice to fix) re-bootstraps it \
             is not this test's concern, but the recovered store itself \
             must not already contain it"
        );
    }

    #[test]
    fn recovery_bootstraps_every_registry_declared_four_tuple_missing_from_the_journal() {
        let registry = parse_fixture(PRODUCTION_REGISTRY_FIXTURE);
        let template = &registry.templates[0];
        let four_tuple = registry.four_tuple(template);
        let (journal_path, exhaustion_path) =
            scratch_journal_and_exhaustion_paths("recovery-bootstrap-remainder");

        let mut recovery =
            recover_native_shadow_state(&registry, "digest-v1", &journal_path, &exhaustion_path)
                .expect("recover");
        assert!(recovery.stuck_in_flight.is_empty());
        assert_eq!(
            recovery
                .store
                .resolve(&four_tuple, "digest-v1", || panic!("row already exists")),
            ResolveOutcome::Existing(ChallengeState::Disabled)
        );
    }
}
