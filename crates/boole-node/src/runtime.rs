use crate::block_store::FileBlockStore;
use boole_core::{
    admit_parsed_submission_typed, block_hash, build_block_selection, calibration_policy,
    expected_retarget_difficulty_for_height, parse_submission_body, replay_blocks, share_score,
    AdmissionDecision, AdmissionParsedDeps, BlockBuilderConfig, BuildSelectionResult,
    CalibrationPolicy, CalibrationReport, CandidateShare, DifficultyRetargetPolicy, Hex32,
    PersistedBlock, PoolShare, RateLimiter, SharePool,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

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
}

impl RuntimeAdmissionState {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            rate_limiter: RateLimiter::from_policy(&config.policy, config.admission_window_ms),
            pool: SharePool::from_policy(&config.policy),
            current_c: None,
            candidates: Vec::new(),
            config,
        }
    }

    pub fn boot_from_store(
        config: RuntimeConfig,
        block_path: impl AsRef<Path>,
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
        runtime.set_current_c(replay.latest_c);
        Ok(runtime)
    }

    pub fn set_current_c(&mut self, c: String) {
        self.current_c = Some(c.clone());
        self.pool.set_current_c(c);
    }

    pub fn current_c(&self) -> Option<&str> {
        self.current_c.as_deref()
    }

    pub fn apply_produced_block(&mut self, block: &PersistedBlock) -> anyhow::Result<usize> {
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
        self.current_c = Some(block.c.clone());
        let dropped = self.pool.prune_to_height(block.c.clone());
        self.candidates.retain(|candidate| candidate.c == block.c);
        Ok(dropped)
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
        self.produce_block_for_current_c_with_config(height, ts, accepted_canon_tags, &config)
    }

    fn produce_block_for_current_c_with_config(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
        config: &BlockBuilderConfig,
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
            min_share_score: config.min_share_score.to_string(),
            kmax_applied: selection.selected.len() as u64,
            difficulty_epoch: config.difficulty_epoch,
            t_block: config.t_block.clone(),
            t_share: config.t_share.clone(),
            difficulty_weight: config.difficulty_weight.clone(),
            dropped_below_min_score: selection.dropped_below_min_score as u64,
            dropped_kernel_reject: selection.dropped_kernel_reject as u64,
            truncated_by_kmax: selection.truncated_by_kmax as u64,
            ts,
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
        let recovered = FileBlockStore::recover(block_path)?;
        if recovered.size() as u64 != height {
            anyhow::bail!(
                "commit height {} does not match recovered store size {}",
                height,
                recovered.size()
            );
        }
        self.commit_with_recovered(
            block_path,
            recovered.blocks(),
            height,
            ts,
            accepted_canon_tags,
        )
    }

    pub fn commit_next_block_for_current_c(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let block_path = block_path.as_ref();
        let recovered = FileBlockStore::recover(block_path)?;
        let height = recovered.size() as u64;
        self.commit_with_recovered(
            block_path,
            recovered.blocks(),
            height,
            ts,
            accepted_canon_tags,
        )
    }

    fn commit_with_recovered(
        &mut self,
        block_path: &Path,
        existing_blocks: &[PersistedBlock],
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let config = self.block_builder_config_for_height(existing_blocks)?;
        let block =
            self.produce_block_for_current_c_with_config(height, ts, accepted_canon_tags, &config)?;
        FileBlockStore::append(block_path, &block)?;
        let dropped_stale_shares = self.apply_produced_block(&block)?;
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
            self.candidates.push(CandidateShare {
                label: "runtime-admission".to_string(),
                pk: submission.pk_hex,
                n: submission.n_hex,
                j: submission.j_hex,
                c: submission.c_hex,
                share_hash: share_hash.to_hex(),
                score: share_score(share_hash).to_string(),
                canon_tag,
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
