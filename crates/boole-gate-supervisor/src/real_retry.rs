//! Policy-v2 observations for real releases only. Never an automatic VOID.
#[derive(Clone, Debug, Default)]
pub struct Retry {
    pub attempts: u32,
    pub next_at: u64,
    pub third_authorized: bool,
    pub last_canary_failed: bool,
    previous_class: Option<String>,
}
impl Retry {
    pub fn d_node(&mut self, now: u64) {
        self.next_at = self.next_at.max(now.saturating_add(60));
    }
    pub fn ready(&self, now: u64) -> bool {
        now >= self.next_at
    }
    pub fn stage(&self) -> &'static str {
        match self.attempts {
            0 => "A1",
            1 => "A2",
            2 if self.third_authorized => "A3",
            _ => "slow",
        }
    }
    pub fn deferred(&mut self, now: u64, class: &str, disagreement: bool) {
        if class == "D-node" {
            self.d_node(now);
            return;
        }
        self.third_authorized |=
            disagreement || self.previous_class.as_deref().is_some_and(|v| v != class);
        self.previous_class = Some(class.into());
        self.attempts += 1;
        self.next_at = now.saturating_add(if self.attempts == 1 {
            60
        } else if self.attempts == 2 && self.third_authorized {
            600
        } else {
            3600
        });
    }
    pub fn canary(&mut self, now: u64, pass: bool) {
        self.last_canary_failed = !pass;
        if !pass {
            self.third_authorized = true;
        }
        self.next_at = if pass && self.attempts == 2 && self.third_authorized {
            // A healthy recheck authorizes the already scheduled A3. It is not
            // a completed third attempt and must not restart the slow ladder.
            self.next_at.max(now)
        } else if pass && self.attempts == 0 {
            now
        } else if pass {
            self.next_at.max(now.saturating_add(3600))
        } else {
            now.saturating_add(if self.attempts <= 2 { 600 } else { 3600 })
        };
    }
}
