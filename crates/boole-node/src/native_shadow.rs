//! Node-native shadow binding and containment — identity keys, registry
//! binding, and the `nonIssuable` bootstrap rule.
//!
//! Implements
//! `docs/node-native-shadow-binding-containment-implementation-spec-v1.md`
//! sections 4 and 6 only: JSON registry parsing, the four-tuple operational
//! state key with `registryDigest` bound as a field (not a key component,
//! closing that document's F4 registry-drift gap), the five-tuple
//! idempotency key, and the three-ordered-check bootstrap rule that resolves
//! every registry-declared four-tuple to `Disabled`, `Exhausted` or
//! `Active(fresh)`. The durable journal, crash recovery, concurrency and
//! containment-execution slices are separate, later work per that document's
//! sections 7-11 and are not implemented in this module — this module is not
//! wired into `local_node.rs` or any HTTP route, consistent with that
//! document's section 1 non-goals (no route, no `boole-node` server change,
//! until implementation of that route itself is undertaken).

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
/// reached only by the durable state-machine slice, not implemented in this
/// module. `Expired` is declared unreachable on the `nonIssuable` path (RED
/// gate 8): no function in this module ever returns it, by construction —
/// see `bootstrap_challenge_state`'s three-way return.
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

/// In-memory operational-state store keyed by the four-tuple alone. Durable
/// persistence, crash recovery and the `InFlight` state machine are a later
/// slice (spec section 7); this store's job in this slice is only the
/// row-lookup-before-bootstrap order that closes F4's registry-drift gap
/// (RED gate 3).
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
}
