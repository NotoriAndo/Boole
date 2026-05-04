use boole_core::{admit_submission, AdmissionDeps, CalibrationReport, RateLimiter, SharePool};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
    #[serde(default)]
    observe_ticket: bool,
    expect: Value,
}

#[test]
fn admission_pipeline_matches_improved_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");

    let mut rate_limiter = RateLimiter::new(fixture.cfg.clone(), 60_000);
    let mut pool = SharePool::new(fixture.cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());

    for (idx, op) in fixture.operations.iter().enumerate() {
        let body = body_for(&fixture.constants, &op.body_patch);
        if op.observe_ticket {
            assert!(rate_limiter.observe_ticket(
                body.get("pk").and_then(Value::as_str).expect("pk"),
                body.get("c").and_then(Value::as_str).expect("c"),
                body.get("n").and_then(Value::as_str),
            ));
        }
        let got = admit_submission(AdmissionDeps {
            cfg: &fixture.cfg,
            rate_limiter: &mut rate_limiter,
            pool: &mut pool,
            now: 1_800_000_000_000 + idx as i64,
            ip: &fixture.constants.ip,
            body: &body,
        });
        assert_eq!(got, op.expect, "{}", op.name);
    }
}

fn body_for(constants: &Constants, patch: &Map<String, Value>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(constants.c.clone()));
    body.insert("pk".to_string(), Value::String(constants.pk.clone()));
    body.insert("n".to_string(), Value::String(constants.n.clone()));
    body.insert("j".to_string(), Value::String(constants.j.clone()));
    body.insert(
        "nonceS".to_string(),
        Value::String(constants.nonce_s.clone()),
    );
    body.insert(
        "bytes".to_string(),
        Value::String(constants.valid_bytes_hex.clone()),
    );
    for (key, value) in patch {
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
    body
}
