use boole_core::{
    admit_parsed_submission_typed, build_block_selection, calibration_policy,
    parse_submission_body, share_score, AdmissionDecision, AdmissionParsedDeps, BlockBuilderConfig,
    BuildSelectionResult, CalibrationPolicy, CalibrationReport, CandidateShare, PoolShare,
    RateLimiter, SharePool,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

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

    pub fn set_current_c(&mut self, c: String) {
        self.current_c = Some(c.clone());
        self.pool.set_current_c(c);
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
