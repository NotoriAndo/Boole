use crate::block_store::FileBlockStore;
use crate::bounty_event_store::FileBountyEventLedger;
use crate::durability::write_ndjson_lines_atomic;
use crate::reward_store::{verify_ledger_matches_replay, FileRewardLedger};
use boole_core::{
    admit_parsed_submission_typed, block_hash, build_block_selection, calibration_policy,
    choose_canonical_head, compute_block_reward_credits, derive_bounty_settlement,
    difficulty_weight, expected_retarget_difficulty_for_height, head_block_hash,
    parse_submission_body, replay_blocks_allow_legacy_evidence_less,
    replay_blocks_with_genesis_and_registry,
    replay_blocks_with_retarget_allow_legacy_evidence_less, share_score, AdmissionDecision,
    AdmissionParsedDeps, BlockBuilderConfig, BuildSelectionResult, CalibrationPolicy,
    CalibrationReport, CandidateShare, DifficultyEvidence, DifficultyRetargetPolicy,
    FamilyManifestRegistry, LegacyEvidenceOptIn, PersistedBlock, PersistedRewardEvent, PoolShare,
    RateLimiter, SelectedShareEvidence, SharePool,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// P1.3b — derive the canonical reward event for a block. The block store is
/// the source of truth for the reward ledger, so this single helper is used
/// by the live commit path, ingest, reorg rebuild, AND the boot heal — the
/// event can never differ between them. It folds the base proposer/share
/// credits and the bounty credits derived from the block's committed
/// promoted shares (ADR-0015 (a): settlement is re-derived against the
/// family `registry` via `derive_bounty_settlement`, never read from a
/// declared field) into one event.
fn derive_reward_event(
    block: &PersistedBlock,
    registry: &FamilyManifestRegistry,
) -> anyhow::Result<PersistedRewardEvent> {
    let mut credits = compute_block_reward_credits(block)?;
    for bounty_credit in
        derive_bounty_settlement(&block.promoted_bounty_shares, registry, block.height)?
    {
        credits.push(boole_core::PersistedCredit {
            pk: bounty_credit.prover.clone(),
            amount: bounty_credit.amount.clone(),
        });
    }
    Ok(PersistedRewardEvent {
        height: block.height,
        c: block.c.clone(),
        credits,
    })
}

/// P1.3b — re-derive the bounty-event ledger rows for a block, byte-identical
/// to what `submit_json` writes (`crates/boole-node/src/local_node.rs`), so a
/// bounty-event ledger that trails the block store after a crash mid-commit
/// can be healed from the block store. Returns `(credit_events,
/// share_promoted_events)`. Credit rows are DERIVED from the committed
/// `promoted_bounty_shares` via `derive_bounty_settlement` (ADR-0015 (a)),
/// so this is a pure function of (persisted block, family registry).
///
/// N4 — also reused by the reorg rebuild (`rebuild_bounty_ledger_rows` in
/// `local_node.rs`) to re-project the block-driven ledger rows onto a
/// newly-adopted chain, hence `pub(crate)`.
pub(crate) fn derive_bounty_events(
    block: &PersistedBlock,
    registry: &FamilyManifestRegistry,
) -> anyhow::Result<(Vec<serde_json::Value>, Vec<serde_json::Value>)> {
    let credits = derive_bounty_settlement(&block.promoted_bounty_shares, registry, block.height)?
        .iter()
        .map(|c| {
            serde_json::json!({
                "schemaVersion": 1,
                "kind": "credit",
                "height": block.height,
                "c": block.c,
                "familyId": c.family_id,
                "bountyId": c.bounty_id,
                "prover": c.prover,
                "amount": c.amount,
            })
        })
        .collect();
    let shares = block
        .promoted_bounty_shares
        .iter()
        .map(|s| {
            serde_json::json!({
                "schemaVersion": 1,
                "kind": "share_promoted",
                "height": block.height,
                "familyId": s.family_id,
                "bountyId": s.bounty_id,
                "proofHash": s.proof_hash,
                "prover": s.prover,
            })
        })
        .collect();
    Ok((credits, shares))
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub policy: CalibrationPolicy,
    pub admission_window_ms: i64,
    pub difficulty_retarget: Option<DifficultyRetargetPolicy>,
}

impl RuntimeConfig {
    /// N5.1 (ADR-0014) — the GenesisSpec this node's consensus surface
    /// declares: Tier-1 params from the calibration policy + retarget
    /// schedule, identity from the caller.
    ///
    /// SC.10-iv-0 — `seed_binding_required`, `checker_artifact_hash` and
    /// `family_manifest_root` are network identity DECLARATIONS no
    /// calibration scenario can express: when `network_id` names a compiled
    /// preset they are adopted from it, so a threshold-matching node can
    /// actually boot the checker-pinned network (previously they were
    /// hardcoded `false`/`None`/`None`, making the boot genesis gate
    /// unpassable for `boole-testnet-2` under ANY scenario). The gate keeps
    /// its Tier-1 meaning: a scenario whose t_block/t_share/k_max/retarget
    /// diverge from the preset still refuses to boot under that name.
    /// Off-preset (closed-local / fixture) networks keep the pre-N5.2
    /// defaults: seed binding optional, no checker pin, no manifest root.
    pub fn genesis_spec(&self, network_id: &str, genesis_c: &str) -> boole_core::GenesisSpec {
        let (seed_binding_required, checker_artifact_hash, family_manifest_root) =
            match boole_core::network_genesis_preset(network_id) {
                Some(preset) => (
                    preset.params.seed_binding_required,
                    preset.params.checker_artifact_hash,
                    preset.params.family_manifest_root,
                ),
                None => (false, None, None),
            };
        boole_core::GenesisSpec {
            network_id: network_id.to_string(),
            params: boole_core::GenesisParams {
                consensus_rule_version: boole_core::CONSENSUS_RULE_VERSION,
                t_block: format!("0x{:064x}", self.policy.thresholds.t_block),
                t_share: format!("0x{:064x}", self.policy.thresholds.t_share),
                k_max: self.policy.k_max as u64,
                retarget: self.difficulty_retarget.clone(),
                seed_binding_required,
                checker_artifact_hash,
                family_manifest_root,
            },
            initial_state: boole_core::GenesisInitialState {
                genesis_c: genesis_c.to_string(),
            },
        }
    }

    pub fn from_calibration_report(
        report: CalibrationReport,
        admission_window_ms: i64,
    ) -> Result<Self, String> {
        Ok(Self {
            policy: calibration_policy(&report)?,
            admission_window_ms,
            difficulty_retarget: None,
        })
    }

    pub fn with_difficulty_retarget(
        mut self,
        policy: DifficultyRetargetPolicy,
    ) -> Result<Self, String> {
        policy.validate().map_err(|err| err.to_string())?;
        self.difficulty_retarget = Some(policy);
        Ok(self)
    }
}

/// N2.2 — hard upper bound on how far a block `ts` may sit ahead of the
/// node's clock ("future drift"): generous enough to absorb real clock
/// skew between operators, tight enough that a self-reported `ts` cannot
/// pre-stage a large forward drift for a later median-time-past window.
/// SC.5 moved it here so self-produce, extend-by-one ingest, AND the
/// reorg candidate path share one boundary.
pub(crate) const BLOCK_TS_MAX_FUTURE_DRIFT_MS: u64 = 2 * 60 * 60 * 1000;

/// Rejects a block `ts` that lies more than `BLOCK_TS_MAX_FUTURE_DRIFT_MS`
/// ahead of `now_ms`. `now_ms` is threaded in explicitly so this stays a
/// pure, directly unit-testable function.
pub(crate) fn check_block_ts_future_drift(ts_ms: u64, now_ms: u64) -> anyhow::Result<()> {
    let max_allowed_ms = now_ms.saturating_add(BLOCK_TS_MAX_FUTURE_DRIFT_MS);
    if ts_ms > max_allowed_ms {
        anyhow::bail!(
            "block ts {} exceeds the future-drift bound: now={} maxAllowedMs={} (driftBoundMs={})",
            ts_ms,
            now_ms,
            max_allowed_ms,
            BLOCK_TS_MAX_FUTURE_DRIFT_MS
        );
    }
    Ok(())
}

pub struct RuntimeCommittedBlock {
    pub block: PersistedBlock,
    pub dropped_stale_shares: usize,
}

/// N4.3 — outcome of evaluating a competing chain against the current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorgOutcome {
    /// The candidate chain won fork-choice; the block store and reward ledger
    /// were rewritten to it and all in-memory state re-derived from genesis.
    /// Carries the adopted head block's height.
    Reorged { new_head_height: u64 },
    /// The current chain is at least as good (heavier, or an exact tie the
    /// current tip already holds); nothing on disk or in memory changed.
    KeptCurrent,
}

pub struct RuntimeAdmissionState {
    pub config: RuntimeConfig,
    rate_limiter: RateLimiter,
    pool: SharePool,
    current_c: Option<String>,
    candidates: Vec<CandidateShare>,
    /// In-memory mirror of the on-disk block store. Populated at boot via
    /// `FileBlockStore::recover` and incrementally updated on every commit.
    /// Hot paths (commit, status, head) read from this instead of re-reading
    /// the file each request, eliminating the previous O(N²) commit cost.
    block_cache: Vec<PersistedBlock>,
    /// Path to the on-disk reward ledger. When `Some`, every commit appends
    /// a `PersistedRewardEvent` to this file before in-memory state is
    /// updated. When `None`, reward bookkeeping is disabled (legacy tests).
    reward_ledger_path: Option<PathBuf>,
    /// In-memory mirror of the reward ledger. Populated at boot from the
    /// file (or re-derived from blocks if the file is absent) and updated
    /// in lockstep with `reward_ledger_path` appends on every commit.
    reward_ledger: Option<FileRewardLedger>,
    /// §SC reset window (ADR-0015 (a)) — the family manifest set bounty
    /// settlement derives against on every consensus path this runtime
    /// touches (boot replay/heal, commit, ingest, reorg). Empty when the
    /// node runs without family manifests: chains carrying promoted
    /// bounty shares then reject.
    family_registry: FamilyManifestRegistry,
    /// SC.5 — the genesis spec this runtime booted under (present iff
    /// booted via `boot_from_store_with_genesis`, i.e. every served
    /// node). When present, the self-produce commit strict-replays
    /// cache+block under it BEFORE anything reaches disk, so the node
    /// can never persist a chain its own reboot would refuse.
    boot_genesis: Option<boole_core::GenesisSpec>,
}

impl RuntimeAdmissionState {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            rate_limiter: RateLimiter::from_policy(&config.policy, config.admission_window_ms),
            pool: SharePool::from_policy(&config.policy),
            current_c: None,
            candidates: Vec::new(),
            block_cache: Vec::new(),
            reward_ledger_path: None,
            reward_ledger: None,
            family_registry: FamilyManifestRegistry::new(),
            boot_genesis: None,
            config,
        }
    }

    /// §SC reset window — install the family manifest set consensus-path
    /// bounty settlement derives against. Callers that boot via
    /// `boot_from_store_with_bounty_ledger_and_registry` never need this;
    /// it exists for the `new()`-then-wire construction local tests use.
    pub fn set_family_registry(&mut self, registry: FamilyManifestRegistry) {
        self.family_registry = registry;
    }

    pub fn family_registry(&self) -> &FamilyManifestRegistry {
        &self.family_registry
    }

    pub fn boot_from_store(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
        reward_ledger_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        Self::boot_from_store_with_bounty_ledger(config, block_path, reward_ledger_path, None)
    }

    /// S23d — boot variant that cross-checks the bounty event ledger
    /// against replay-derived per-family credit totals. Used by
    /// local_node so a corrupted bounty ledger fails boot rather than
    /// silently letting reward and audit logs drift apart.
    pub fn boot_from_store_with_bounty_ledger(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
        reward_ledger_path: Option<PathBuf>,
        bounty_event_ledger_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        Self::boot_from_store_with_bounty_ledger_and_registry(
            config,
            block_path,
            reward_ledger_path,
            bounty_event_ledger_path,
            FamilyManifestRegistry::new(),
        )
    }

    /// §SC reset window (ADR-0015 (a)) — boot with the family manifest set
    /// bounty settlement derives against. A chain carrying promoted bounty
    /// shares can only boot through here (the registry-less variants
    /// reject it as naming an unknown family).
    pub fn boot_from_store_with_bounty_ledger_and_registry(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
        reward_ledger_path: Option<PathBuf>,
        bounty_event_ledger_path: Option<PathBuf>,
        family_registry: FamilyManifestRegistry,
    ) -> anyhow::Result<Self> {
        Self::boot_from_store_inner(
            config,
            block_path,
            reward_ledger_path,
            bounty_event_ledger_path,
            family_registry,
            None,
        )
    }

    /// SC.5 (GAP-08) — genesis-aware boot: the node's OWN block store is
    /// replayed under the SAME strict contract live ingest/reorg use
    /// (`replay_blocks_with_genesis_and_registry` — anchor, difficulty,
    /// k_max, seed policy, evidence all enforced). The node boot path
    /// (`local_node`) always routes here, so the legacy evidence-less
    /// opt-in below is structurally unreachable for a served network; it
    /// survives only for pre-genesis fixture/test callers.
    pub fn boot_from_store_with_genesis(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
        reward_ledger_path: Option<PathBuf>,
        bounty_event_ledger_path: Option<PathBuf>,
        family_registry: FamilyManifestRegistry,
        genesis: &boole_core::GenesisSpec,
    ) -> anyhow::Result<Self> {
        Self::boot_from_store_inner(
            config,
            block_path,
            reward_ledger_path,
            bounty_event_ledger_path,
            family_registry,
            Some(genesis),
        )
    }

    fn boot_from_store_inner(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
        reward_ledger_path: Option<PathBuf>,
        bounty_event_ledger_path: Option<PathBuf>,
        family_registry: FamilyManifestRegistry,
        genesis: Option<&boole_core::GenesisSpec>,
    ) -> anyhow::Result<Self> {
        let recovered = FileBlockStore::recover(block_path)?;
        let mut runtime = Self::new(config);
        runtime.family_registry = family_registry;
        runtime.boot_genesis = genesis.cloned();
        // N1.3 (G2) — retarget-aware boot replay: when a retarget policy is
        // configured, fold its difficulty validation into replay (rejects a
        // forged epoch-boundary t_block) instead of a separate call.
        //
        // SC.5 — with a GenesisSpec the boot replay is the strict
        // genesis-aware contract (one verdict for one chain, boot or
        // live). The legacy evidence-less opt-in remains ONLY for
        // pre-genesis fixture/test callers that boot without a spec; the
        // served node path always passes one. (The pre-SC.5 comment
        // claiming "there is no p2p ingest path in this codebase yet"
        // was stale — N3.3/N4.3 landed ingest and reorg.)
        let replay = if let Some(spec) = genesis {
            replay_blocks_with_genesis_and_registry(
                recovered.blocks(),
                spec,
                &runtime.family_registry,
            )?
        } else {
            let opt_in = LegacyEvidenceOptIn::for_legacy_replay_only();
            match &runtime.config.difficulty_retarget {
                Some(policy) => replay_blocks_with_retarget_allow_legacy_evidence_less(
                    recovered.blocks(),
                    &format!("0x{:064x}", runtime.config.policy.thresholds.t_block),
                    policy,
                    opt_in,
                    &runtime.family_registry,
                )?,
                None => replay_blocks_allow_legacy_evidence_less(
                    recovered.blocks(),
                    opt_in,
                    &runtime.family_registry,
                )?,
            }
        };
        // P1.3b — bounty-event ledger crash-mid-commit heal. The bounty-event
        // ledger is the LAST store written per block (block → reward →
        // bounty-event `credit` rows → bounty-event `share_promoted` rows →
        // receipt), so a crash after the reward append but before the
        // bounty-event appends leaves it short of the last block's rows, and a
        // deleted ledger (the documented upgrade-recovery path) leaves it short
        // of EVERY block's rows. `verify_ledger_matches_replay` below would then
        // refuse to boot a `--bounty-events` node.
        //
        // The ledger INTERLEAVES route-driven events (`create` / `status_change`
        // / `proof`, written by the announce/status/proof handlers at arbitrary
        // times) with BLOCK-driven `credit` + `share_promoted` rows (written at
        // block commit in `submit_json`, credit-rows-then-share-rows per block,
        // in block order). ONLY the block-driven rows are re-derivable from the
        // block store (`derive_bounty_events`); the route-driven rows are not and
        // are left untouched. Filtering the route-driven rows out, the surviving
        // block-driven rows are a strict PREFIX of the expected
        // `credit`/`share_promoted` sequence (same block order; `recover` also
        // truncates any torn trailing line first), so re-append the missing
        // suffix. Healing the `share_promoted` rows — not just `credit` — is what
        // stops `rebuild_bounty_side_pool` re-promoting an already-committed
        // share. A genuine tamper keeps the block-driven count equal, so nothing
        // is appended and the verify still bails. (A DELETED ledger loses the
        // route-driven audit rows permanently — only the block-derivable
        // credit/share rows are restored.)
        if let Some(bounty_path) = bounty_event_ledger_path.as_deref() {
            let mut expected: Vec<serde_json::Value> = Vec::new();
            for block in recovered.blocks() {
                let (credits, shares) = derive_bounty_events(block, &runtime.family_registry)?;
                expected.extend(credits);
                expected.extend(shares);
            }
            if !expected.is_empty() {
                let present = if bounty_path.exists() {
                    FileBountyEventLedger::recover(bounty_path)?
                } else {
                    Vec::new()
                };
                // Count ONLY the block-driven rows already on disk: the ledger
                // also holds route-driven `create`/`status_change`/`proof` rows
                // that `expected` does not, so a raw `present.len()` would be the
                // wrong basis for the prefix comparison (and would wrongly skip
                // the heal whenever any route event exists).
                let present_block_rows = present
                    .iter()
                    .filter(|e| {
                        matches!(
                            e.get("kind").and_then(serde_json::Value::as_str),
                            Some("credit") | Some("share_promoted")
                        )
                    })
                    .count();
                if present_block_rows < expected.len() {
                    let missing = expected.len() - present_block_rows;
                    for ev in expected.into_iter().skip(present_block_rows) {
                        FileBountyEventLedger::append(bounty_path, &ev)?;
                    }
                    eprintln!(
                        "boole-node: bounty-event ledger healed from block store: \
                         re-derived {} trailing credit/share_promoted event(s) up to \
                         height {} (crash-mid-commit recovery)",
                        missing,
                        recovered.blocks().last().map(|b| b.height).unwrap_or(0),
                    );
                }
            }
        }
        if let Some(path) = reward_ledger_path {
            let ledger = if path.exists() {
                let mut recovered_ledger = FileRewardLedger::recover(&path)?;
                // P1.3b — crash-mid-commit heal. A crash between
                // `FileBlockStore::append` and `FileRewardLedger::append`
                // leaves the reward ledger trailing the block store by one (or
                // more) events. The block store is the source of truth — each
                // block fully determines its reward event — so re-derive and
                // append the missing trailing events instead of refusing to
                // boot, then re-verify. A GENUINE balance tamper (a wrong
                // amount in an EXISTING event, which does not change the event
                // count) is NOT healed: the count already matches the block
                // store, so no event is re-derived and the verify below bails.
                let blocks = recovered.blocks();
                if recovered_ledger.size() < blocks.len() {
                    let from = recovered_ledger.size();
                    for block in &blocks[from..] {
                        let event = derive_reward_event(block, &runtime.family_registry)?;
                        FileRewardLedger::append(&path, &event)?;
                        recovered_ledger.apply(event)?;
                    }
                    eprintln!(
                        "boole-node: reward ledger healed from block store: re-derived {} \
                         trailing event(s) up to height {} (crash-mid-commit recovery)",
                        blocks.len() - from,
                        blocks.last().map(|b| b.height).unwrap_or(0),
                    );
                }
                // P1.3b — by this point BOTH the reward ledger (heal above) and
                // the bounty-event ledger (heal before the reward block) have
                // been brought into agreement with the block store, so this
                // verify confirms convergence and still bails on a GENUINE
                // tamper (an existing event whose value is wrong but whose count
                // matches — neither heal fires for that case).
                verify_ledger_matches_replay(
                    &recovered_ledger,
                    &replay.balances,
                    bounty_event_ledger_path.as_deref(),
                    &replay.bounty_credit_by_family,
                )?;
                recovered_ledger
            } else {
                // Re-derive from blocks: write one event per block to the file
                // and rebuild the in-memory state from the same source so the
                // file and the cache cannot drift mid-run.
                let mut ledger = FileRewardLedger::default();
                for block in recovered.blocks() {
                    let event = derive_reward_event(block, &runtime.family_registry)?;
                    FileRewardLedger::append(&path, &event)?;
                    ledger.apply(event)?;
                }
                ledger
            };
            runtime.reward_ledger_path = Some(path);
            runtime.reward_ledger = Some(ledger);
        }
        runtime.set_current_c(replay.latest_c);
        runtime.block_cache = recovered.blocks().to_vec();
        Ok(runtime)
    }

    /// Look up a balance by pk. Returns 0 if no ledger is configured or the
    /// pk has never been credited. Lookup is the wire contract for the
    /// `/account/{pk}/balance` route — never throws.
    pub fn balance_for(&self, pk: &str) -> u128 {
        self.reward_ledger
            .as_ref()
            .map(|ledger| ledger.balance_of(pk))
            .unwrap_or(0)
    }

    /// Latest event's `(height, c)`. `None` if no ledger is configured or
    /// the ledger has no events yet (genesis-only state).
    pub fn ledger_head(&self) -> Option<(u64, String)> {
        self.reward_ledger
            .as_ref()
            .and_then(|ledger| ledger.last_event())
            .map(|event| (event.height, event.c.clone()))
    }

    pub fn cached_blocks(&self) -> &[PersistedBlock] {
        &self.block_cache
    }

    pub fn cached_block_count(&self) -> usize {
        self.block_cache.len()
    }

    pub fn set_current_c(&mut self, c: String) {
        self.current_c = Some(c.clone());
        self.pool.set_current_c(c);
    }

    pub fn current_c(&self) -> Option<&str> {
        self.current_c.as_deref()
    }

    /// Read-only pre-check that `block` can be applied on top of the current
    /// runtime head. Returns Err if linkage or shape is wrong; never mutates
    /// state. Pair with `apply_block_unchecked` after the disk append succeeds
    /// so the cache and the file always agree even if the runtime is killed
    /// between the two steps.
    pub fn check_block_applicable(&self, block: &PersistedBlock) -> anyhow::Result<()> {
        let current_c = self
            .current_c
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("current chain head is not set"))?;
        if block.prev_c != current_c {
            anyhow::bail!(
                "block prevC {} does not match runtime head {}",
                block.prev_c,
                current_c
            );
        }
        block.validate_shape()?;
        Ok(())
    }

    /// Apply state mutations for a block that has already been validated by
    /// `check_block_applicable`. Infallible: callers must validate first.
    pub fn apply_block_unchecked(&mut self, block: &PersistedBlock) -> usize {
        self.current_c = Some(block.c.clone());
        let dropped = self.pool.prune_to_height(block.c.clone());
        self.candidates.retain(|candidate| candidate.c == block.c);
        dropped
    }

    pub fn apply_produced_block(&mut self, block: &PersistedBlock) -> anyhow::Result<usize> {
        self.check_block_applicable(block)?;
        Ok(self.apply_block_unchecked(block))
    }

    /// N3.3 — persist and apply a peer-ingested block with the same
    /// {check, append, reward-append, apply_unchecked, cache} ordering as
    /// the self-produce commit (`commit_using_cache`), so a crash between
    /// the write steps leaves the on-disk store and the in-memory state in
    /// agreement (and the P1.3b boot heal covers the same windows).
    ///
    /// The caller MUST already have validated the block through the strict
    /// replay entry points (`replay_blocks` / `replay_blocks_with_retarget`)
    /// over the extended chain — this function only re-asserts the cheap
    /// head-extension check before writing. The reward event is re-derived
    /// from the block exactly like the boot heal (`derive_reward_event`),
    /// so `verify_ledger_matches_replay` stays green on the next boot.
    pub fn ingest_external_block(
        &mut self,
        block_path: impl AsRef<Path>,
        block: &PersistedBlock,
    ) -> anyhow::Result<usize> {
        self.check_block_applicable(block)?;
        FileBlockStore::append(block_path.as_ref(), block)?;
        if let (Some(ledger_path), Some(ledger)) = (
            self.reward_ledger_path.as_ref(),
            self.reward_ledger.as_mut(),
        ) {
            let event = derive_reward_event(block, &self.family_registry)?;
            FileRewardLedger::append(ledger_path, &event)?;
            ledger.apply(event)?;
        }
        let dropped = self.apply_block_unchecked(block);
        self.block_cache.push(block.clone());
        Ok(dropped)
    }

    /// N4.3 — adopt `candidate` in place of the current chain when it wins
    /// fork-choice, re-deriving all state deterministically from genesis.
    ///
    /// The candidate is strict-replayed first (a competing chain never takes
    /// the legacy evidence-less boot path — it uses the same
    /// `replay_blocks`/`replay_blocks_with_retarget` entry points a p2p ingest
    /// would), so a tampered or evidence-less chain returns `Err` and leaves
    /// the current chain untouched. The keep/reorg decision reuses N4.2's
    /// [`choose_canonical_head`] + [`head_block_hash`] so the reorg trigger can
    /// never drift from the standalone selection rule: keep the current chain
    /// unless the candidate is strictly heavier (or wins the lowest-head-hash
    /// tie-break).
    ///
    /// On adoption the block store and reward ledger files are each rewritten
    /// atomically (write sibling temp + `rename`) and the in-memory
    /// cache/head/ledger/share-pool are rebuilt from the candidate, so a fresh
    /// boot over the rewritten files reconstructs byte-identical state.
    ///
    /// Non-goals (this slice): incremental rollback (it re-derives the whole
    /// chain rather than diffing to the common ancestor) and bounty-event
    /// ledger rewind. The sync-path trigger that calls this on a divergent,
    /// heavier peer chain is `local_node::ingest_candidate_chain` (N4).
    pub fn reorg_to_heavier_chain(
        &mut self,
        block_path: impl AsRef<Path>,
        candidate: &[PersistedBlock],
        genesis: &boole_core::GenesisSpec,
    ) -> anyhow::Result<ReorgOutcome> {
        let candidate_head = candidate
            .last()
            .ok_or_else(|| anyhow::anyhow!("reorg candidate chain is empty"))?;

        // SC.5 (2nd review item 9) — the candidate path applies the same
        // ts future-drift guard direct ingest applies to its tip.
        // Replay's median-time-past check below is RELATIVE, so an
        // all-future suffix would otherwise sail through and poison the
        // retarget inputs once adopted. Tip-only on purpose: historical
        // blocks are exempt (their ts sits in the past by construction —
        // rejecting them would brick honest catch-up), and a chain whose
        // interior runs ahead of a present-time tip trips the monotonic
        // median check instead.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        check_block_ts_future_drift(candidate_head.ts, now_ms)?;

        // 1. Strict replay from genesis (N5.1: the GenesisSpec is the
        //    consensus source — anchor, difficulty, k_max, seed policy).
        //    Peer/competing chains use the strict path (no legacy
        //    evidence-less opt-in), so an evidence-less or tampered
        //    candidate is rejected before it can displace the current
        //    chain.
        let replay =
            replay_blocks_with_genesis_and_registry(candidate, genesis, &self.family_registry)?;

        // 2. Fork-choice. Reuse choose_canonical_head + head_block_hash so this
        //    decision is identical to the standalone selection rule (N4.2). An
        //    empty current chain loses to any valid candidate.
        let candidate_head_hash = head_block_hash(candidate_head)?;
        if let Some(current_head) = self.block_cache.last() {
            if head_block_hash(current_head)? == candidate_head_hash {
                return Ok(ReorgOutcome::KeptCurrent); // already on this exact tip
            }
            let winner = choose_canonical_head(&[self.block_cache.clone(), candidate.to_vec()])?;
            if winner != candidate_head_hash {
                return Ok(ReorgOutcome::KeptCurrent); // current chain is at least as good
            }
        }

        // 3. Rewrite the block store atomically to the candidate chain.
        let block_lines = candidate
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?;
        write_ndjson_lines_atomic(block_path.as_ref(), &block_lines)?;

        // 4. Rebuild the reward ledger atomically from the candidate — one
        //    event per block, identical to the boot re-derive path, so the
        //    next boot's `verify_ledger_matches_replay` stays green.
        if let Some(ledger_path) = self.reward_ledger_path.clone() {
            let mut ledger = FileRewardLedger::default();
            let mut event_lines = Vec::with_capacity(candidate.len());
            for block in candidate {
                let event = derive_reward_event(block, &self.family_registry)?;
                event_lines.push(serde_json::to_string(&event)?);
                ledger.apply(event)?;
            }
            write_ndjson_lines_atomic(&ledger_path, &event_lines)?;
            self.reward_ledger = Some(ledger);
        }

        // 5. Rebuild in-memory chain/head/pool from the candidate. Use the
        //    replay-derived head `c` (not the stored one) as the authoritative
        //    tip, matching how boot sets the head.
        self.block_cache = candidate.to_vec();
        self.set_current_c(replay.latest_c);
        self.pool.prune_to_height(candidate_head.c.clone());
        self.candidates
            .retain(|candidate| candidate.c == candidate_head.c);

        Ok(ReorgOutcome::Reorged {
            new_head_height: candidate_head.height,
        })
    }

    pub fn pool_size(&self) -> usize {
        self.pool.size()
    }

    pub fn shares_for_current_c(&self) -> Vec<&PoolShare> {
        self.current_c
            .as_deref()
            .map(|c| self.pool.for_chain(c))
            .unwrap_or_default()
    }

    pub fn candidate_shares_for_current_c(&self) -> Vec<CandidateShare> {
        self.current_c
            .as_deref()
            .map(|c| {
                self.candidates
                    .iter()
                    .filter(|candidate| candidate.c == c)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn build_block_selection_for_current_c(
        &self,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<BuildSelectionResult> {
        let current_c = self
            .current_c
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("current chain head is not set"))?;
        // N1.2 (G3) — honor difficulty retarget: route through the
        // height-aware config instead of static from_policy so selection
        // uses the same t_block the commit/replay/`/head` paths do.
        let config = self.block_builder_config_for_height(self.cached_blocks())?;
        build_block_selection(
            current_c,
            &self.candidate_shares_for_current_c(),
            &config,
            accepted_canon_tags,
            &self.credited_canon_hashes(),
            &[],
        )
    }

    /// N4-pre.1 (ADR-0012 (d)) — every canon_hash already credited on this
    /// chain, re-derived from the cached block evidence. Fed into
    /// `build_block_selection` so an honest proposer never builds a block
    /// the consensus dedup rule would reject on replay/ingest.
    fn credited_canon_hashes(&self) -> BTreeSet<String> {
        self.block_cache
            .iter()
            .flat_map(|block| {
                block
                    .selected_share_evidence
                    .iter()
                    .map(|evidence| evidence.canon_hash.clone())
            })
            .collect()
    }

    fn block_builder_config_for_height(
        &self,
        existing_blocks: &[PersistedBlock],
    ) -> anyhow::Result<BlockBuilderConfig> {
        let Some(policy) = &self.config.difficulty_retarget else {
            return BlockBuilderConfig::from_policy(&self.config.policy);
        };
        let evidence = expected_retarget_difficulty_for_height(
            existing_blocks,
            &format!("0x{:064x}", self.config.policy.thresholds.t_block),
            policy,
        )?;
        BlockBuilderConfig::from_policy_with_t_block(
            &self.config.policy,
            evidence.t_block,
            evidence.difficulty_epoch,
        )
    }

    /// N1.1 (G1/G2) — the height-effective difficulty `/head` must report.
    /// With retarget enabled this runs the same `expected_retarget_difficulty_for_height`
    /// path the commit uses (`block_builder_config_for_height`), so a miner
    /// reads the runtime-effective `T_block` + epoch/mode, not the static
    /// calibrated report. With retarget disabled it reports the static
    /// calibrated thresholds under `static-calibrated`/epoch 0.
    pub fn effective_difficulty_for_head(&self) -> anyhow::Result<DifficultyEvidence> {
        let static_t_block = format!("0x{:064x}", self.config.policy.thresholds.t_block);
        match &self.config.difficulty_retarget {
            Some(policy) => expected_retarget_difficulty_for_height(
                self.cached_blocks(),
                &static_t_block,
                policy,
            ),
            None => Ok(DifficultyEvidence {
                mode: "static-calibrated".to_string(),
                retarget: "not-enabled".to_string(),
                difficulty_epoch: 0,
                t_block: static_t_block,
                t_share: format!("0x{:064x}", self.config.policy.thresholds.t_share),
                difficulty_weight: difficulty_weight(&self.config.policy.thresholds.t_block)?
                    .to_string(),
            }),
        }
    }

    pub fn produce_block_for_current_c(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<PersistedBlock> {
        // N1.2 (G3) — honor difficulty retarget (see build_block_selection).
        let config = self.block_builder_config_for_height(self.cached_blocks())?;
        self.produce_block_for_current_c_with_config(height, ts, accepted_canon_tags, &config, &[])
    }

    fn produce_block_for_current_c_with_config(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        config: &BlockBuilderConfig,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
    ) -> anyhow::Result<PersistedBlock> {
        let prev_c = self
            .current_c
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("current chain head is not set"))?;
        let selection = build_block_selection(
            prev_c,
            &self.candidate_shares_for_current_c(),
            config,
            accepted_canon_tags,
            &self.credited_canon_hashes(),
            promoted_bounty_shares,
        )?;
        let BuildSelectionResult::Ok(selection) = selection else {
            anyhow::bail!("block selection did not produce a single proposer");
        };
        let selected_share_hashes = selection
            .selected
            .iter()
            .map(|share| share.share_hash.clone())
            .collect::<Vec<_>>();
        let selected_share_pks = selection
            .selected
            .iter()
            .map(|share| share.pk.clone())
            .collect::<Vec<_>>();
        let selected_share_reward_pks = if selection
            .selected
            .iter()
            .any(|share| !share.reward_pk.is_empty())
        {
            selection
                .selected
                .iter()
                .map(|share| {
                    if share.reward_pk.is_empty() {
                        share.pk.clone()
                    } else {
                        share.reward_pk.clone()
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let selected_share_evidence = selection
            .selected
            .iter()
            .map(|share| SelectedShareEvidence {
                pk: share.pk.clone(),
                n: share.n.clone(),
                j: share.j.clone(),
                c: share.c.clone(),
                canon_hash: share.canon_hash.clone(),
                proof_package: share.proof_package.clone(),
                seed_hex: share.seed_hex.clone(),
                // Evidence v2 slot — SC.1 wires the submit-path signed
                // work envelope through CandidateShare into here.
                signed_work: None,
            })
            .collect::<Vec<_>>();
        let proposer = selection
            .selected
            .get(selection.proposer_index)
            .ok_or_else(|| anyhow::anyhow!("proposer index out of range"))?;
        let proposer_reward_pk = proposer.reward_pk.clone();

        // Preimage v3 (ADR-0015 (a)): the hash commits the assembled block's
        // replay-consumed fields — including the promoted bounty share rows
        // settlement derives from — so build first with an empty `c`, then
        // derive it.
        let mut block = PersistedBlock {
            height,
            prev_c: prev_c.to_string(),
            c: String::new(),
            proposer_pk: proposer.pk.clone(),
            selected_share_hashes,
            selected_share_pks,
            selected_share_reward_pks,
            proposer_reward_pk,
            selected_share_evidence,
            min_share_score: config.min_share_score.to_string(),
            min_share_score_multiplier_nanos: config.min_share_score_multiplier_nanos,
            kmax_applied: selection.selected.len() as u64,
            difficulty_epoch: config.difficulty_epoch,
            t_block: config.t_block.clone(),
            t_share: config.t_share.clone(),
            difficulty_weight: config.difficulty_weight.clone(),
            dropped_below_min_score: selection.dropped_below_min_score as u64,
            dropped_kernel_reject: selection.dropped_kernel_reject as u64,
            truncated_by_kmax: selection.truncated_by_kmax as u64,
            ts,
            // Preimage v3 — the committed settlement inputs; credit rows
            // are derived from these, never persisted.
            promoted_bounty_shares: selection.promoted_bounty_shares.clone(),
        };
        block.c = block_hash(&block).to_hex();
        Ok(block)
    }

    pub fn commit_block_for_current_c(
        &mut self,
        block_path: impl AsRef<Path>,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let block_path = block_path.as_ref();
        let cached_height = self.block_cache.len() as u64;
        if cached_height != height {
            anyhow::bail!(
                "commit height {} does not match cached store size {}",
                height,
                cached_height
            );
        }
        self.commit_using_cache(block_path, height, ts, accepted_canon_tags, &[])
    }

    pub fn commit_next_block_for_current_c(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        self.commit_next_block_for_current_c_with_promoted(block_path, ts, accepted_canon_tags, &[])
    }

    /// S23c — promotion-aware commit. The caller (local_node.rs)
    /// computes `select_promoted_bounty_selection(...)` against its
    /// bounty side-pool / family registry / operator pks just before
    /// the commit and threads the promoted SHARES through here. The
    /// bounty credits are DERIVED from those committed rows
    /// (`derive_bounty_settlement` inside `derive_reward_event`) and
    /// merged into the same `PersistedRewardEvent` as the base-lane
    /// proposer credits, so the reward ledger and replay share one
    /// settlement policy (ADR-0015 (a)).
    pub fn commit_next_block_for_current_c_with_promoted(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let block_path = block_path.as_ref();
        let height = self.block_cache.len() as u64;
        self.commit_using_cache(
            block_path,
            height,
            ts,
            accepted_canon_tags,
            promoted_bounty_shares,
        )
    }

    fn commit_using_cache(
        &mut self,
        block_path: &Path,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let config = self.block_builder_config_for_height(&self.block_cache)?;
        let block = self.produce_block_for_current_c_with_config(
            height,
            ts,
            accepted_canon_tags,
            &config,
            promoted_bounty_shares,
        )?;
        // Validate first so any rejection cannot leave a block on disk that
        // the runtime never applied. The pair {check, append, apply_unchecked}
        // is the only ordering where a crash between the two write steps
        // still leaves the on-disk store and the in-memory state in agreement.
        self.check_block_applicable(&block)?;
        // SC.5 (SC.7 위임) — a genesis-booted runtime strict-replays the
        // WHOLE chain-to-be (cache + candidate) before the append: prior
        // to this, the commit path checked only linkage+shape, so a
        // node whose local config diverged from its genesis could write
        // a chain to disk that its own reboot (and every peer) rejects.
        if let Some(genesis) = &self.boot_genesis {
            let mut candidate = self.block_cache.clone();
            candidate.push(block.clone());
            replay_blocks_with_genesis_and_registry(&candidate, genesis, &self.family_registry)
                .map_err(|err| {
                    anyhow::anyhow!(
                        "self-produced block at height {} fails the strict genesis replay                          this node itself enforces — refusing to commit it: {err:#}",
                        block.height
                    )
                })?;
        }
        FileBlockStore::append(block_path, &block)?;
        if let (Some(ledger_path), Some(ledger)) = (
            self.reward_ledger_path.as_ref(),
            self.reward_ledger.as_mut(),
        ) {
            // ADR-0015 (a) — one derivation for live commit, ingest,
            // reorg, and boot heal: `derive_reward_event` folds base-lane
            // credits and settlement-derived bounty credits.
            let event = derive_reward_event(&block, &self.family_registry)?;
            FileRewardLedger::append(ledger_path, &event)?;
            ledger.apply(event)?;
        }
        let dropped_stale_shares = self.apply_block_unchecked(&block);
        self.block_cache.push(block.clone());
        Ok(RuntimeCommittedBlock {
            block,
            dropped_stale_shares,
        })
    }

    pub fn observe_ticket_from_body(&mut self, body: &Map<String, Value>) -> Result<bool, String> {
        let pk = required_string(body, "pk")?;
        let c = required_string(body, "c")?;
        let n = body.get("n").and_then(Value::as_str);
        Ok(self.rate_limiter.observe_ticket(pk, c, n))
    }

    pub fn admit_body(
        &mut self,
        now: i64,
        ip: &str,
        body: &Map<String, Value>,
    ) -> AdmissionDecision {
        self.admit_body_with_canon_tag(now, ip, body, 0)
    }

    pub fn admit_body_with_canon_tag(
        &mut self,
        now: i64,
        ip: &str,
        body: &Map<String, Value>,
        canon_tag: u8,
    ) -> AdmissionDecision {
        self.admit_body_with_canon_tag_and_reward_pk(now, ip, body, canon_tag, None)
    }

    pub fn admit_body_with_canon_tag_and_reward_pk(
        &mut self,
        now: i64,
        ip: &str,
        body: &Map<String, Value>,
        canon_tag: u8,
        reward_pk: Option<&str>,
    ) -> AdmissionDecision {
        let submission = match parse_submission_body(body) {
            Ok(submission) => submission,
            Err(decision) => return decision,
        };
        let decision = admit_parsed_submission_typed(AdmissionParsedDeps {
            policy: &self.config.policy,
            rate_limiter: &mut self.rate_limiter,
            pool: &mut self.pool,
            now,
            ip,
            submission: &submission,
        });
        if let AdmissionDecision::Accepted { share_hash } = &decision {
            // Defence-in-depth: SharePool already enforces global_share_cap, but
            // the candidates Vec is a separate collection. If anything ever
            // makes it past the pool while the cap is full (e.g. policy bug,
            // future code path), do not let this Vec grow unbounded.
            if self.candidates.len() >= self.config.policy.global_share_cap {
                return decision;
            }
            let canon_hash = hex::encode(Sha256::digest(&submission.package_bytes));
            let proof_package = hex::encode(&submission.package_bytes);
            self.candidates.push(CandidateShare {
                label: "runtime-admission".to_string(),
                pk: submission.pk_hex,
                reward_pk: reward_pk.unwrap_or("").to_string(),
                n: submission.n_hex,
                j: submission.j_hex,
                c: submission.c_hex,
                share_hash: share_hash.to_hex(),
                score: share_score(share_hash).to_string(),
                canon_tag,
                canon_hash,
                proof_package,
                seed_hex: submission.seed_hex,
            });
        }
        decision
    }

    /// SC.10-ii-d-2 — drop an already-admitted share from the candidate set a
    /// self-produced block draws on (`candidate_shares_for_current_c`). The
    /// gossip-ingress Lean gate calls this when the pinned checker refuses
    /// (or cannot reach a verdict on) a share structural admission accepted:
    /// ADR-0016 (c-2) makes admission the producer's Lean gate, so a share
    /// that did not clear it must never be assemblable into this node's own
    /// block. The SharePool entry deliberately stays — like the
    /// `duplicate_proof` peek in `ingress_admit_share`, the pool's
    /// (pk, n, j, c) slot outlives the rejection, which also blocks an
    /// identical re-announce until the pool prunes at the next commit.
    pub fn retract_candidate(&mut self, share_hash: &str) {
        self.candidates
            .retain(|candidate| candidate.share_hash != share_hash);
    }
}

fn required_string<'a>(body: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be string"))
}
