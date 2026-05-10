use std::time::Instant;

use boole_core::{digest_to_biguint, share_hash, share_score, Hex32};
use num_bigint::BigUint;

use super::{GrindProgress, GrinderConfig, NonceSource};

#[derive(Debug, Clone)]
pub struct GrindShareOutcome {
    pub j: Hex32,
    pub share_hash_bytes: Hex32,
    pub share_score: BigUint,
    pub is_proposer: bool,
    pub hashes_attempted: u64,
    pub elapsed_ms: u128,
}

#[allow(clippy::too_many_arguments)]
pub fn grind_share(
    c: &Hex32,
    pk: &Hex32,
    n: &Hex32,
    canon_hash: &Hex32,
    min_share_score: &BigUint,
    t_block: Option<&BigUint>,
    j_source: &mut dyn NonceSource,
    config: GrinderConfig,
    mut on_progress: Option<&mut dyn FnMut(GrindProgress)>,
) -> Option<GrindShareOutcome> {
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
        j_source.next_nonce(&mut buf);
        let j = Hex32::from_bytes(buf);
        let sh = share_hash(c, pk, n, &j, canon_hash);
        let score = share_score(&sh);
        attempts += 1;

        if &score >= min_share_score {
            let is_proposer = match t_block {
                Some(t) => &digest_to_biguint(&sh) < t,
                None => false,
            };
            return Some(GrindShareOutcome {
                j,
                share_hash_bytes: sh,
                share_score: score,
                is_proposer,
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
