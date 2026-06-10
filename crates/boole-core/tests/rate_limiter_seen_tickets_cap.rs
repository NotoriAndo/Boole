//! D#2 — `seen_tickets` / `exact_tickets_per_pk_c` must stay bounded under a
//! distinct-nonce flood. `observe_ticket` inserts before admission, so an
//! attacker free to fabricate `(pk, c, n)` triples must not be able to grow
//! the dedup set without limit.

use boole_core::{calibration_policy, CalibrationReport, RateLimiter};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    cfg: CalibrationReport,
    window_ms: i64,
}

#[test]
fn seen_tickets_set_is_bounded_under_distinct_nonce_flood() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let policy = calibration_policy(&fixture.cfg).expect("policy parses");
    let mut limiter = RateLimiter::from_policy(&policy, fixture.window_ms);

    // Exact-nonce dedup still works below the cap.
    assert!(limiter.observe_ticket("pk-flood", "c", Some("n-dup")));
    assert!(!limiter.observe_ticket("pk-flood", "c", Some("n-dup")));

    for i in 0..RateLimiter::SEEN_TICKETS_CAP {
        limiter.observe_ticket("pk-flood", "c", Some(&format!("n-{i}")));
    }
    assert!(
        limiter.seen_tickets_len() <= RateLimiter::SEEN_TICKETS_CAP,
        "seen_tickets must stay bounded under a distinct-nonce flood, got {}",
        limiter.seen_tickets_len()
    );
}
