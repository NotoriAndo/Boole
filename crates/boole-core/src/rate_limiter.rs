use crate::{calibration_policy, CalibrationPolicy, CalibrationReport};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_CURRENT_HEAD_RATE_IDENTITIES: usize = 4096;
const MAX_ACTIVE_RATE_SOURCE_IPS: usize = 4096;

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
    current_c: Option<String>,
    ip: HashMap<String, VecDeque<i64>>,
    pk_count: HashMap<String, i64>,
    pk_tickets: HashMap<String, i64>,
    seen_tickets: HashSet<String>,
    exact_tickets_per_pk_c: HashMap<String, i64>,
    identity_order: VecDeque<String>,
    tracked_identities: HashSet<String>,
}

impl RateLimiter {
    /// Upper bound on the exact-ticket dedup set (D#2). `observe_ticket`
    /// inserts before admission, so without a cap an attacker submitting
    /// distinct `(pk, c, n)` triples grows `seen_tickets` without limit.
    pub const SEEN_TICKETS_CAP: usize = 1_000_000;

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
            current_c: None,
            ip: HashMap::new(),
            pk_count: HashMap::new(),
            pk_tickets: HashMap::new(),
            seen_tickets: HashSet::new(),
            exact_tickets_per_pk_c: HashMap::new(),
            identity_order: VecDeque::new(),
            tracked_identities: HashSet::new(),
        }
    }

    pub fn observe_ticket(&mut self, pk: &str, c: &str, n: Option<&str>) -> bool {
        if self
            .current_c
            .as_deref()
            .is_some_and(|current| current != c)
        {
            return false;
        }
        if let Some(n) = n {
            let ticket_key = format!("{pk}|{c}|{n}");
            if self.seen_tickets.contains(&ticket_key) {
                let pc = key(pk, c);
                if !self.tracked_identities.contains(&pc) && self.track_identity(pc.clone()) {
                    *self.exact_tickets_per_pk_c.entry(pc.clone()).or_insert(0) += 1;
                    *self.pk_tickets.entry(pc).or_insert(0) += 1;
                }
                return false;
            }
            let pc = key(pk, c);
            if !self.track_identity(pc.clone()) {
                return false;
            }
            self.seen_tickets.insert(ticket_key);
            *self.exact_tickets_per_pk_c.entry(pc).or_insert(0) += 1;
            // Bounded-memory guard (D#2): past the cap, drop the exact dedup
            // state instead of growing without limit under a distinct-nonce
            // flood. Trade-off: previously-seen tickets become re-observable
            // after a clear; `has_observed_ticket` then falls back to the
            // per-(pk,c) ticket counters, which keep admission conservative.
            if self.seen_tickets.len() > Self::SEEN_TICKETS_CAP {
                self.seen_tickets.clear();
                self.exact_tickets_per_pk_c.clear();
            }
        } else {
            if !self.track_identity(key(pk, c)) {
                return false;
            }
        }
        let k = key(pk, c);
        *self.pk_tickets.entry(k).or_insert(0) += 1;
        true
    }

    /// Move the limiter to a new canonical head. Per-identity ticket/share
    /// state is meaningful only for one `c`, so retaining it across a head
    /// transition is both stale authority and unbounded memory growth. The
    /// source-IP sliding window deliberately survives to prevent a new block
    /// from resetting network abuse limits.
    pub fn set_current_c(&mut self, c: impl Into<String>) {
        let c = c.into();
        if self.current_c.as_deref() == Some(&c) {
            return;
        }
        self.current_c = Some(c);
        self.clear_identity_state();
    }

    /// Current number of bounded per-`(pk,c)` accounting identities.
    pub fn tracked_identity_len(&self) -> usize {
        self.tracked_identities.len()
    }

    /// Current number of bounded source-IP sliding-window buckets.
    pub fn tracked_source_ip_len(&self) -> usize {
        self.ip.len()
    }

    /// Current size of the exact-ticket dedup set (telemetry + tests).
    pub fn seen_tickets_len(&self) -> usize {
        self.seen_tickets.len()
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
        if !self.ip.contains_key(ip)
            && self
                .ip
                .values()
                .filter(|timestamps| timestamps.iter().any(|ts| *ts >= cutoff))
                .count()
                >= MAX_ACTIVE_RATE_SOURCE_IPS
        {
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
        if !self.ip.contains_key(ip) && self.ip.len() >= MAX_ACTIVE_RATE_SOURCE_IPS {
            for timestamps in self.ip.values_mut() {
                while timestamps.front().is_some_and(|ts| *ts < cutoff) {
                    timestamps.pop_front();
                }
            }
            self.ip.retain(|_, timestamps| !timestamps.is_empty());
        }
        let timestamps = self.ip.entry(ip.to_string()).or_default();
        while timestamps.front().is_some_and(|ts| *ts < cutoff) {
            timestamps.pop_front();
        }
        timestamps.push_back(now);

        let k = key(pk, c);
        let used = self.pk_count.get(&k).copied().unwrap_or(0);
        self.pk_count.insert(k, used + 1);
    }

    /// Undo one known-successful admission charge when a later semantic
    /// verifier cannot reach a verdict. This is not used for deterministic
    /// rejects: invalid work keeps its anti-abuse charge. The exact timestamp
    /// and source are supplied by the reservation owner, so one concurrent
    /// admission cannot release another's quota entry.
    pub fn release_committed(&mut self, now: i64, ip: &str, pk: &str, c: &str) -> bool {
        let k = key(pk, c);
        let Some(position) = self
            .ip
            .get(ip)
            .and_then(|timestamps| timestamps.iter().position(|timestamp| *timestamp == now))
        else {
            return false;
        };
        let has_pk_charge = self.pk_count.get(&k).is_some_and(|used| *used > 0);
        // On the current head, both accounting dimensions must identify the
        // reservation before either is touched. After a head transition,
        // `set_current_c` has intentionally pruned the old per-key dimension;
        // a detached verifier owner must still be able to refund its exact IP
        // timestamp rather than strand the source quota.
        if !has_pk_charge && self.current_c.as_deref().is_none_or(|head| head == c) {
            return false;
        }

        let timestamps = self
            .ip
            .get_mut(ip)
            .expect("timestamp source checked immediately above");
        timestamps.remove(position);
        if timestamps.is_empty() {
            self.ip.remove(ip);
        }

        if has_pk_charge {
            let used = self
                .pk_count
                .get_mut(&k)
                .expect("positive pk charge checked immediately above");
            *used = used.saturating_sub(1);
            if *used == 0 {
                self.pk_count.remove(&k);
            }
        }
        true
    }

    pub fn reset(&mut self) {
        self.current_c = None;
        self.ip.clear();
        self.clear_identity_state();
    }

    fn track_identity(&mut self, identity: String) -> bool {
        if self.tracked_identities.contains(&identity) {
            return true;
        }
        if self.tracked_identities.len() >= MAX_CURRENT_HEAD_RATE_IDENTITIES {
            let Some(position) = self
                .identity_order
                .iter()
                .position(|oldest| self.pk_count.get(oldest).is_none_or(|used| *used == 0))
            else {
                // Every retained identity has a live or deterministic charge.
                // Refuse a new identity rather than evicting an in-flight
                // reservation and making its exact cleanup impossible.
                return false;
            };
            let oldest = self
                .identity_order
                .remove(position)
                .expect("eviction position came from the same deque");
            self.tracked_identities.remove(&oldest);
            self.pk_count.remove(&oldest);
            self.pk_tickets.remove(&oldest);
            self.exact_tickets_per_pk_c.remove(&oldest);
        }
        self.tracked_identities.insert(identity.clone());
        self.identity_order.push_back(identity);
        true
    }

    fn clear_identity_state(&mut self) {
        self.pk_count.clear();
        self.pk_tickets.clear();
        self.seen_tickets.clear();
        self.exact_tickets_per_pk_c.clear();
        self.identity_order.clear();
        self.tracked_identities.clear();
    }
}

fn key(pk: &str, c: &str) -> String {
    format!("{pk}|{c}")
}
