use boole_core::{
    admit_submission_typed, calibration_policy, AdmissionDecision, AdmissionDeps,
    CalibrationPolicy, CalibrationReport, PoolShare, RateLimiter, SharePool,
};
use serde_json::{Map, Value};

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
}

impl RuntimeAdmissionState {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            rate_limiter: RateLimiter::from_policy(&config.policy, config.admission_window_ms),
            pool: SharePool::from_policy(&config.policy),
            current_c: None,
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
        admit_submission_typed(AdmissionDeps {
            policy: &self.config.policy,
            rate_limiter: &mut self.rate_limiter,
            pool: &mut self.pool,
            now,
            ip,
            body,
        })
    }
}

fn required_string<'a>(body: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be string"))
}
