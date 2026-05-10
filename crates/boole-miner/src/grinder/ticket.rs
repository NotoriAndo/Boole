use std::time::Instant;

use boole_core::{ticket, Hex32};
use num_bigint::BigUint;

use super::{GrindProgress, GrinderConfig, NonceSource};

#[derive(Debug, Clone)]
pub struct GrindTicketOutcome {
    pub nonce: Hex32,
    pub hash_bytes: Hex32,
    pub hash_int: BigUint,
    pub hashes_attempted: u64,
    pub elapsed_ms: u128,
}

pub fn grind_ticket(
    c: &Hex32,
    pk: &Hex32,
    t_ticket: &BigUint,
    nonce_source: &mut dyn NonceSource,
    config: GrinderConfig,
    mut on_progress: Option<&mut dyn FnMut(GrindProgress)>,
) -> Option<GrindTicketOutcome> {
    let start = Instant::now();
    let mut buf = [0u8; 32];
    let mut attempts: u64 = 0;
    let mut last_report_at = start;
    let mut last_report_attempts: u64 = 0;

    loop {
        if let Some(max) = config.max_attempts {
            if attempts >= max {
                return None;
            }
        }
        nonce_source.next_nonce(&mut buf);
        let nonce = Hex32::from_bytes(buf);
        let result = ticket(c, pk, &nonce, t_ticket);
        attempts += 1;

        if result.valid {
            return Some(GrindTicketOutcome {
                nonce,
                hash_bytes: result.hash_bytes,
                hash_int: result.hash_int,
                hashes_attempted: attempts,
                elapsed_ms: start.elapsed().as_millis(),
            });
        }

        if config.report_every_hashes > 0 && attempts.is_multiple_of(config.report_every_hashes) {
            if let Some(cb) = on_progress.as_deref_mut() {
                let now = Instant::now();
                let dt_ms = now.duration_since(last_report_at).as_millis().max(1);
                let dh = attempts - last_report_attempts;
                cb(GrindProgress {
                    hashes_attempted: attempts,
                    hashes_per_sec: (dh as f64 * 1000.0) / dt_ms as f64,
                    elapsed_ms: now.duration_since(start).as_millis(),
                });
                last_report_at = now;
                last_report_attempts = attempts;
            }
        }
    }
}
