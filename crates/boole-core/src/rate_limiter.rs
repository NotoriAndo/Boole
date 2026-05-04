use crate::{calibration_policy, CalibrationPolicy, CalibrationReport};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    Allowed,
    Rejected { reason: RateLimitRejectReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitRejectReason {
    IpQuota,
    PkQuota,
}

impl RateLimitRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IpQuota => "ip_quota",
            Self::PkQuota => "pk_quota",
        }
    }
}

pub fn rate_limit_result_json(result: &RateLimitResult) -> Value {
    match result {
        RateLimitResult::Allowed => json!({ "allowed": true }),
        RateLimitResult::Rejected { reason } => {
            json!({ "allowed": false, "reason": reason.as_str() })
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    m: i64,
    per_ip_rate_limit_per_60s: usize,
    window_ms: i64,
    ip: HashMap<String, VecDeque<i64>>,
    pk_count: HashMap<String, i64>,
    pk_tickets: HashMap<String, i64>,
    seen_tickets: HashSet<String>,
    exact_tickets_per_pk_c: HashMap<String, i64>,
}

impl RateLimiter {
    pub fn new(cfg: CalibrationReport, window_ms: i64) -> Self {
        Self::from_calibration_report(&cfg, window_ms).expect("calibration report is valid")
    }

    pub fn from_calibration_report(
        cfg: &CalibrationReport,
        window_ms: i64,
    ) -> Result<Self, String> {
        let policy = calibration_policy(cfg)?;
        Ok(Self::from_policy(&policy, window_ms))
    }

    pub fn from_policy(policy: &CalibrationPolicy, window_ms: i64) -> Self {
        Self {
            m: policy.m,
            per_ip_rate_limit_per_60s: policy.per_ip_rate_limit_per_60s,
            window_ms,
            ip: HashMap::new(),
            pk_count: HashMap::new(),
            pk_tickets: HashMap::new(),
            seen_tickets: HashSet::new(),
            exact_tickets_per_pk_c: HashMap::new(),
        }
    }

    pub fn observe_ticket(&mut self, pk: &str, c: &str, n: Option<&str>) -> bool {
        if let Some(n) = n {
            let ticket_key = format!("{pk}|{c}|{n}");
            if self.seen_tickets.contains(&ticket_key) {
                return false;
            }
            self.seen_tickets.insert(ticket_key);
            let pc = key(pk, c);
            *self.exact_tickets_per_pk_c.entry(pc).or_insert(0) += 1;
        }
        let k = key(pk, c);
        *self.pk_tickets.entry(k).or_insert(0) += 1;
        true
    }

    pub fn has_observed_ticket(&self, pk: &str, c: &str, n: &str) -> bool {
        let pc = key(pk, c);
        if self.exact_tickets_per_pk_c.get(&pc).copied().unwrap_or(0) == 0 {
            return self.pk_tickets.get(&pc).copied().unwrap_or(0) > 0;
        }
        self.seen_tickets.contains(&format!("{pk}|{c}|{n}"))
    }

    pub fn check(&mut self, now: i64, ip: &str, pk: &str, c: &str) -> RateLimitResult {
        let result = self.peek(now, ip, pk, c);
        if matches!(result, RateLimitResult::Allowed) {
            self.commit(now, ip, pk, c);
        }
        result
    }

    pub fn check_json(&mut self, now: i64, ip: &str, pk: &str, c: &str) -> Value {
        rate_limit_result_json(&self.check(now, ip, pk, c))
    }

    pub fn peek(&self, now: i64, ip: &str, pk: &str, c: &str) -> RateLimitResult {
        let cutoff = now - self.window_ms;
        let ip_count = self
            .ip
            .get(ip)
            .map(|timestamps| timestamps.iter().filter(|ts| **ts >= cutoff).count())
            .unwrap_or(0);
        if ip_count >= self.per_ip_rate_limit_per_60s {
            return RateLimitResult::Rejected {
                reason: RateLimitRejectReason::IpQuota,
            };
        }

        let k = key(pk, c);
        let tickets = self.pk_tickets.get(&k).copied().unwrap_or(0);
        let ceiling = tickets * self.m;
        let used = self.pk_count.get(&k).copied().unwrap_or(0);
        if used >= ceiling {
            return RateLimitResult::Rejected {
                reason: RateLimitRejectReason::PkQuota,
            };
        }

        RateLimitResult::Allowed
    }

    pub fn commit(&mut self, now: i64, ip: &str, pk: &str, c: &str) {
        let cutoff = now - self.window_ms;
        let timestamps = self.ip.entry(ip.to_string()).or_default();
        while timestamps.front().is_some_and(|ts| *ts < cutoff) {
            timestamps.pop_front();
        }
        timestamps.push_back(now);

        let k = key(pk, c);
        let used = self.pk_count.get(&k).copied().unwrap_or(0);
        self.pk_count.insert(k, used + 1);
    }

    pub fn reset(&mut self) {
        self.ip.clear();
        self.pk_count.clear();
        self.pk_tickets.clear();
        self.seen_tickets.clear();
        self.exact_tickets_per_pk_c.clear();
    }
}

fn key(pk: &str, c: &str) -> String {
    format!("{pk}|{c}")
}
