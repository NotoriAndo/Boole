//! Seed↔prev-block binding at admission (pre-mortem audit 2026-07-05) —
//! the protocol narrative says the chain poses the problem ("deterministic
//! examiner"), but before this slice a submission's `seedHex` was persisted
//! verbatim: a miner could solve any problem of its choosing and still
//! admit. Admission now requires a CLAIMED seed to be one the protocol
//! posed: `seedHex == target_seed(c, pk, n, j_index)` for some
//! `j_index < TARGET_SEED_J_INDEX_BOUND`. An empty/absent `seedHex` stays
//! admissible (pre-N0.4b legacy posture; mandatory seeds are N3.3 scope).

use boole_core::{
    admit_submission_json, admit_submission_typed, calibration_policy, target_seed,
    AdmissionDecision, AdmissionDeps, AdmissionStatus, CalibrationReport, Hex32, RateLimiter,
    SharePool, TARGET_SEED_J_INDEX_BOUND,
};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
    valid_bytes_hex: String,
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
        .expect("fixture parses")
}

fn derived_seed(j_index: u32) -> String {
    let fixture = load_fixture();
    target_seed(
        &Hex32::from_hex(&fixture.constants.c).expect("c hex32"),
        &Hex32::from_hex(&fixture.constants.pk).expect("pk hex32"),
        &Hex32::from_hex(&fixture.constants.n).expect("n hex32"),
        j_index,
    )
    .to_hex()
}

fn admit_with_seed(seed_hex: Option<String>) -> AdmissionDecision {
    let fixture = load_fixture();
    let policy = calibration_policy(&fixture.cfg).expect("policy parses");
    let mut rate_limiter = RateLimiter::new(fixture.cfg.clone(), 60_000);
    let mut pool = SharePool::new(fixture.cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());
    assert!(rate_limiter.observe_ticket(&fixture.constants.pk, &fixture.constants.c, None));

    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(fixture.constants.c.clone()));
    body.insert(
        "pk".to_string(),
        Value::String(fixture.constants.pk.clone()),
    );
    body.insert("n".to_string(), Value::String(fixture.constants.n.clone()));
    body.insert("j".to_string(), Value::String(fixture.constants.j.clone()));
    body.insert(
        "nonceS".to_string(),
        Value::String(fixture.constants.nonce_s.clone()),
    );
    body.insert(
        "bytes".to_string(),
        Value::String(fixture.constants.valid_bytes_hex.clone()),
    );
    if let Some(seed) = seed_hex {
        body.insert("seedHex".to_string(), Value::String(seed));
    }

    admit_submission_typed(AdmissionDeps {
        policy: &policy,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_000,
        ip: &fixture.constants.ip,
        body: &body,
    })
}

#[test]
fn admission_accepts_body_without_seed_hex() {
    let decision = admit_with_seed(None);
    assert!(
        matches!(decision, AdmissionDecision::Accepted { .. }),
        "absent seedHex is the legacy posture and must stay admissible; got {decision:?}"
    );
}

#[test]
fn admission_accepts_chain_derived_seed_hex() {
    let decision = admit_with_seed(Some(derived_seed(0)));
    assert!(
        matches!(decision, AdmissionDecision::Accepted { .. }),
        "seedHex derived as target_seed(c, pk, n, 0) must admit; got {decision:?}"
    );
}

#[test]
fn admission_accepts_seed_at_last_in_bound_j_index() {
    let decision = admit_with_seed(Some(derived_seed(TARGET_SEED_J_INDEX_BOUND - 1)));
    assert!(
        matches!(decision, AdmissionDecision::Accepted { .. }),
        "j_index == bound-1 is the last posed target and must admit; got {decision:?}"
    );
}

#[test]
fn admission_rejects_seed_hex_not_derived_from_context() {
    let decision = admit_with_seed(Some("66".repeat(32)));
    let AdmissionDecision::Rejected { status, .. } = &decision else {
        panic!("an arbitrary seedHex must be rejected — the miner chose its own problem");
    };
    assert_eq!(*status, AdmissionStatus::UnprocessableEntity);

    let json = admit_submission_json(&decision);
    assert_eq!(json["error"], "seed_binding");
    assert_eq!(json["reason"], "seed_not_derived_from_context");
    assert_eq!(json["rejection"]["stage"], "seed_binding");
}

#[test]
fn admission_rejects_seed_derived_beyond_j_index_bound() {
    let decision = admit_with_seed(Some(derived_seed(TARGET_SEED_J_INDEX_BOUND)));
    assert!(
        matches!(decision, AdmissionDecision::Rejected { .. }),
        "a seed derived at j_index == bound was never posed and must reject; got {decision:?}"
    );
}
