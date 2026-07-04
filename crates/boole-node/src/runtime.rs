use crate::block_store::FileBlockStore;
use crate::bounty_event_store::FileBountyEventLedger;
use crate::reward_store::{verify_ledger_matches_replay, FileRewardLedger};
use boole_core::{
    admit_parsed_submission_typed, block_hash, build_block_selection, calibration_policy,
    compute_block_reward_credits, difficulty_weight, expected_retarget_difficulty_for_height,
    parse_submission_body, replay_blocks_allow_legacy_evidence_less,
    replay_blocks_with_retarget_allow_legacy_evidence_less, share_score, AdmissionDecision,
    AdmissionParsedDeps, BlockBuilderConfig, BuildSelectionResult, CalibrationPolicy,
    CalibrationReport, CandidateShare, DifficultyEvidence, DifficultyRetargetPolicy, Hex32,
    LegacyEvidenceOptIn, PersistedBlock, PersistedRewardEvent, PoolShare, RateLimiter,
    SelectedShareEvidence, SharePool,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// P1.3b — derive the canonical reward event for a block. The block store is
/// the source of truth for the reward ledger, so this single helper is used
/// both to re-derive an absent ledger and to heal a ledger that trails the
/// block store after a crash mid-commit. It folds the base proposer/share
/// credits and the block's promoted bounty credits into one event, matching
/// exactly what the live commit path writes.
fn derive_reward_event(block: &PersistedBlock) -> anyhow::Result<PersistedRewardEvent> {
    let mut credits = compute_block_reward_credits(block)?;
    for bounty_credit in &block.promoted_bounty_credits {
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
/// share_promoted_events)`. The block carries BOTH `promoted_bounty_credits`
/// and `promoted_bounty_shares` (the latter with the `proofHash` that is
/// otherwise lost when the in-memory selection is dropped), so this is a pure
/// function of the persisted block.
fn derive_bounty_events(
    block: &PersistedBlock,
) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let credits = block
        .promoted_bounty_credits
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
    (credits, shares)
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub policy: CalibrationPolicy,
    pub admission_window_ms: i64,
    pub difficulty_retarget: Option<DifficultyRetargetPolicy>,
}

impl RuntimeConfig {
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

pub struct RuntimeCommittedBlock {
    pub block: PersistedBlock,
    pub dropped_stale_shares: usize,
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
            config,
        }
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
        let recovered = FileBlockStore::recover(block_path)?;
        let mut runtime = Self::new(config);
        // N1.3 (G2) — retarget-aware boot replay: when a retarget policy is
        // configured, fold its difficulty validation into replay (rejects a
        // forged epoch-boundary t_block) instead of a separate call.
        //
        // N3-pre.1 — this replays the node's OWN local block store (never a
        // peer-supplied chain: there is no p2p ingest path in this codebase
        // yet), so it opts into the legacy evidence-less path. That keeps
        // boot compatible with pre-evidence local chains/fixtures while a
        // future p2p ingest replay path (not this function) stays on the
        // strict `replay_blocks`/`replay_blocks_with_retarget` entry points,
        // which have no parameter that could accept this opt-in.
        let opt_in = LegacyEvidenceOptIn::for_legacy_replay_only();
        let replay = match &runtime.config.difficulty_retarget {
            Some(policy) => replay_blocks_with_retarget_allow_legacy_evidence_less(
                recovered.blocks(),
                &format!("0x{:064x}", runtime.config.policy.thresholds.t_block),
                policy,
                opt_in,
            )?,
            None => replay_blocks_allow_legacy_evidence_less(recovered.blocks(), opt_in)?,
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
                let (credits, shares) = derive_bounty_events(block);
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
                        let event = derive_reward_event(block)?;
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
                    let event = derive_reward_event(block)?;
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
            &[],
            &[],
        )
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
        self.produce_block_for_current_c_with_config(
            height,
            ts,
            accepted_canon_tags,
            &config,
            &[],
            &[],
        )
    }

    fn produce_block_for_current_c_with_config(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        config: &BlockBuilderConfig,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
        promoted_bounty_credits: &[boole_core::PromotedBountyCredit],
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
            promoted_bounty_shares,
            promoted_bounty_credits,
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
            })
            .collect::<Vec<_>>();
        let share_hashes = selected_share_hashes
            .iter()
            .map(|hash| Hex32::from_hex(hash))
            .collect::<Result<Vec<_>, _>>()?;
        let prev = Hex32::from_hex(prev_c)?;
        let c = block_hash(&prev, &share_hashes).to_hex();
        let proposer = selection
            .selected
            .get(selection.proposer_index)
            .ok_or_else(|| anyhow::anyhow!("proposer index out of range"))?;
        let proposer_reward_pk = proposer.reward_pk.clone();

        Ok(PersistedBlock {
            height,
            prev_c: prev_c.to_string(),
            c,
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
            promoted_bounty_credits: selection.promoted_bounty_credits.clone(),
            // P1.3b — persist the promoted shares on the block so the
            // bounty-event ledger's `share_promoted` rows are re-derivable
            // from the block store after a crash mid-commit. Not hashed.
            promoted_bounty_shares: selection.promoted_bounty_shares.clone(),
        })
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
        self.commit_using_cache(block_path, height, ts, accepted_canon_tags, &[], &[])
    }

    pub fn commit_next_block_for_current_c(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        self.commit_next_block_for_current_c_with_promoted(
            block_path,
            ts,
            accepted_canon_tags,
            &[],
            &[],
        )
    }

    /// S23c — promotion-aware commit. The caller (local_node.rs)
    /// computes `select_promoted_bounty_selection(...)` against its
    /// bounty side-pool / family registry / operator pks just before
    /// the commit and threads the result through here. The bounty
    /// credits are merged into the same `PersistedRewardEvent` as the
    /// base-lane proposer credits so the reward ledger is the single
    /// source of truth, AND folded into the persisted block's
    /// `promoted_bounty_credits` so replay can recompute them.
    pub fn commit_next_block_for_current_c_with_promoted(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
        promoted_bounty_credits: &[boole_core::PromotedBountyCredit],
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let block_path = block_path.as_ref();
        let height = self.block_cache.len() as u64;
        self.commit_using_cache(
            block_path,
            height,
            ts,
            accepted_canon_tags,
            promoted_bounty_shares,
            promoted_bounty_credits,
        )
    }

    fn commit_using_cache(
        &mut self,
        block_path: &Path,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        promoted_bounty_shares: &[boole_core::PromotedBountyShare],
        promoted_bounty_credits: &[boole_core::PromotedBountyCredit],
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let config = self.block_builder_config_for_height(&self.block_cache)?;
        let block = self.produce_block_for_current_c_with_config(
            height,
            ts,
            accepted_canon_tags,
            &config,
            promoted_bounty_shares,
            promoted_bounty_credits,
        )?;
        // Validate first so any rejection cannot leave a block on disk that
        // the runtime never applied. The pair {check, append, apply_unchecked}
        // is the only ordering where a crash between the two write steps
        // still leaves the on-disk store and the in-memory state in agreement.
        self.check_block_applicable(&block)?;
        FileBlockStore::append(block_path, &block)?;
        if let (Some(ledger_path), Some(ledger)) = (
            self.reward_ledger_path.as_ref(),
            self.reward_ledger.as_mut(),
        ) {
            let mut credits = compute_block_reward_credits(&block)?;
            // S23c — fold bounty credits into the same reward event so
            // `verify_ledger_matches_replay` sees one unified balance map.
            // Empty `promoted_bounty_credits` means base-only blocks
            // produce byte-identical events to pre-S23.
            for bounty_credit in &block.promoted_bounty_credits {
                credits.push(boole_core::PersistedCredit {
                    pk: bounty_credit.prover.clone(),
                    amount: bounty_credit.amount.clone(),
                });
            }
            let event = PersistedRewardEvent {
                height: block.height,
                c: block.c.clone(),
                credits,
            };
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
}

fn required_string<'a>(body: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be string"))
}
