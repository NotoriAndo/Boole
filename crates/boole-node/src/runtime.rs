use crate::block_store::FileBlockStore;
use boole_core::{
    admit_parsed_submission_typed, block_hash, build_block_selection, calibration_policy,
    parse_submission_body, replay_blocks, share_score, AdmissionDecision, AdmissionParsedDeps,
    BlockBuilderConfig, BuildSelectionResult, CalibrationPolicy, CalibrationReport, CandidateShare,
    Hex32, PersistedBlock, PoolShare, RateLimiter, SharePool,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub policy: CalibrationPolicy,
    pub admission_window_ms: i64,
}

impl RuntimeConfig {
    pub fn from_calibration_report(
        report: CalibrationReport,
        admission_window_ms: i64,
    ) -> Result<Self, String> {
        Ok(Self {
            policy: calibration_policy(&report)?,
            admission_window_ms,
        })
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

    pub fn produce_block_for_current_c(
        &self,
        height: u64,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<PersistedBlock> {
        let prev_c = self
            .current_c
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("current chain head is not set"))?;
        let config = BlockBuilderConfig::from_policy(&self.config.policy)?;
        let selection = self.build_block_selection_for_current_c(accepted_canon_tags)?;
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
        let block = self.produce_block_for_current_c(height, ts, accepted_canon_tags)?;
        FileBlockStore::append(block_path, &block)?;
        let dropped_stale_shares = self.apply_produced_block(&block)?;
        Ok(RuntimeCommittedBlock {
            block,
            dropped_stale_shares,
        })
    }

    pub fn commit_next_block_for_current_c(
        &mut self,
        block_path: impl AsRef<Path>,
        ts: u64,
        accepted_canon_tags: &BTreeSet<u8>,
    ) -> anyhow::Result<RuntimeCommittedBlock> {
        let block_path = block_path.as_ref();
        let height = FileBlockStore::recover(block_path)?.size() as u64;
        self.commit_block_for_current_c(block_path, height, ts, accepted_canon_tags)
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
