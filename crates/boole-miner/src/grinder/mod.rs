use rand_core::{OsRng, RngCore};

pub mod share;
pub mod submit_pow;
pub mod ticket;

pub use share::{grind_share, GrindShareOutcome};
pub use submit_pow::{grind_submission_pow, GrindSubmitOutcome};
pub use ticket::{grind_ticket, GrindTicketOutcome};

pub trait NonceSource {
    fn next_nonce(&mut self, out: &mut [u8]);
}

pub struct CounterNonce {
    next: u128,
}

impl CounterNonce {
    pub fn new(start: u128) -> Self {
        Self { next: start }
    }
}

impl NonceSource for CounterNonce {
    fn next_nonce(&mut self, out: &mut [u8]) {
        out.fill(0);
        let mut v = self.next;
        for i in (0..out.len()).rev() {
            out[i] = (v & 0xff) as u8;
            v >>= 8;
            if v == 0 {
                break;
            }
        }
        self.next = self.next.wrapping_add(1);
    }
}

pub struct OsRngNonce;

impl NonceSource for OsRngNonce {
    fn next_nonce(&mut self, out: &mut [u8]) {
        OsRng.fill_bytes(out);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GrindProgress {
    pub hashes_attempted: u64,
    pub hashes_per_sec: f64,
    pub elapsed_ms: u128,
}

#[derive(Clone, Copy, Debug)]
pub struct GrinderConfig {
    pub max_attempts: Option<u64>,
    pub report_every_hashes: u64,
}

impl Default for GrinderConfig {
    fn default() -> Self {
        Self {
            max_attempts: None,
            report_every_hashes: 100_000,
        }
    }
}
