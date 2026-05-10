use crate::block_store::FileBlockStore;
use crate::reward_store::{verify_ledger_matches_replay, FileRewardLedger};
use boole_core::{
    admit_parsed_submission_typed, block_hash, build_block_selection, calibration_policy,
    compute_block_credits, expected_retarget_difficulty_for_height, parse_submission_body,
    replay_blocks, share_score, AdmissionDecision, AdmissionParsedDeps, BlockBuilderConfig,
    BuildSelectionResult, CalibrationPolicy, CalibrationReport, CandidateShare,
    DifficultyRetargetPolicy, Hex32, PersistedBlock, PersistedRewardEvent, PoolShare, RateLimiter,
    SelectedShareEvidence, SharePool,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
        let replay = replay_blocks(recovered.blocks())?;
        let mut runtime = Self::new(config);
        if let Some(policy) = &runtime.config.difficulty_retarget {
            boole_core::validate_retargeted_difficulty(
                recovered.blocks(),
                &format!("0x{:064x}", runtime.config.policy.thresholds.t_block),
                policy,
            )?;
        }
        if let Some(path) = reward_ledger_path {
            let ledger = if path.exists() {
                let recovered_ledger = FileRewardLedger::recover(&path)?;
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
                // file and the cache cannot drift mid-run. S23b — also fold
                // the block's bounty credit rows into the same event so the
                // re-derived ledger matches what live commit will produce.
                let mut ledger = FileRewardLedger::default();
                for block in recovered.blocks() {
                    let mut credits =
                        compute_block_credits(&block.proposer_pk, &block.selected_share_pks)?;
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
        let config = BlockBuilderConfig::from_policy(&self.config.policy)?;
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

    pub fn produce_block_for_current_c(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<PersistedBlock> {
        let config = BlockBuilderConfig::from_policy(&self.config.policy)?;
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

        Ok(PersistedBlock {
            height,
            prev_c: prev_c.to_string(),
            c,
            proposer_pk: proposer.pk.clone(),
            selected_share_hashes,
            selected_share_pks,
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
            let mut credits = compute_block_credits(&block.proposer_pk, &block.selected_share_pks)?;
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
                n: submission.n_hex,
                j: submission.j_hex,
                c: submission.c_hex,
                share_hash: share_hash.to_hex(),
                score: share_score(share_hash).to_string(),
                canon_tag,
                canon_hash,
                proof_package,
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
