use std::collections::BTreeMap;

use boole_core::{AcceptResult, CalibrationReport, PoolShare, SharePool, SharePoolRejectReason};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    config: Config,
    current_c: String,
    shares: Vec<ShareFixture>,
    operations: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Config {
    #[serde(rename = "ShareCapPerPK_Block")]
    share_cap_per_pk_block: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct ShareFixture {
    label: String,
    pk: String,
    n: String,
    j: String,
    c: String,
}

fn pool_share(s: &ShareFixture) -> PoolShare {
    PoolShare {
        label: s.label.clone(),
        pk: s.pk.clone(),
        n: s.n.clone(),
        j: s.j.clone(),
        c: s.c.clone(),
    }
}

fn assert_accept_result(got: AcceptResult, expected: &Value) {
    let expected_ok = expected
        .get("ok")
        .and_then(Value::as_bool)
        .expect("ok field");
    assert_eq!(got.ok(), expected_ok);
    if expected_ok {
        assert!(got.reason().is_none());
    } else {
        let reason = expected
            .get("reason")
            .and_then(Value::as_str)
            .expect("reason field");
        assert_eq!(got.reason(), Some(reason));
    }
}

#[test]
fn share_pool_from_calibration_report_uses_policy_cap() {
    let report = CalibrationReport {
        T_submit: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        T_share: "0x2020202020202020202020202020202020202020202020202020202020202020".to_string(),
        T_block: "0x0101010101010101010101010101010101010101010101010101010101010101".to_string(),
        T_ticket: "0x3030303030303030303030303030303030303030303030303030303030303030".to_string(),
        MinShareScoreMultiplier: 2.0,
        K_max: 16,
        ShareCapPerPK_Block: 1,
        L: 4096,
        D_max: 8,
        EMAWindow: 32,
        M: 8,
        perIpRateLimitPer60s: 120,
        provenance: "calibration-final".to_string(),
    };
    let mut pool = SharePool::from_calibration_report(&report).expect("policy pool");
    pool.set_current_c("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let first = PoolShare {
        label: "first".to_string(),
        pk: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        n: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
        j: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
        c: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    };
    let second = PoolShare {
        label: "second".to_string(),
        pk: first.pk.clone(),
        n: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string(),
        j: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        c: first.c.clone(),
    };

    assert_eq!(pool.accept(first), AcceptResult::Ok);
    assert_eq!(
        pool.accept(second),
        AcceptResult::Err {
            reason: SharePoolRejectReason::PkCapExceeded,
        }
    );
}

#[test]
fn share_pool_rejection_uses_typed_reason() {
    let mut pool = SharePool::new(4);
    pool.set_current_c("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let share = PoolShare {
        label: "first".to_string(),
        pk: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        n: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
        j: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
        c: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
    };
    assert_eq!(pool.accept(share.clone()), AcceptResult::Ok);
    assert_eq!(
        pool.accept(share),
        AcceptResult::Err {
            reason: SharePoolRejectReason::Duplicate,
        }
    );
}

#[test]
fn share_pool_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/share-pool/v1.json"
    ))
    .expect("fixture parses");
    let shares = fixture
        .shares
        .iter()
        .map(|s| (s.label.clone(), s.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut pool = SharePool::new(fixture.config.share_cap_per_pk_block);
    pool.set_current_c(fixture.current_c);

    for op in fixture.operations {
        let op_name = op.get("op").and_then(Value::as_str).expect("op");
        match op_name {
            "accept" => {
                let label = op.get("label").and_then(Value::as_str).expect("label");
                let share = shares.get(label).expect("share label");
                let got = pool.accept(pool_share(share));
                assert_accept_result(got, op.get("result").expect("result"));
            }
            "forChain" => {
                let c = op.get("c").and_then(Value::as_str).expect("c");
                let labels = pool
                    .for_chain(c)
                    .into_iter()
                    .map(|s| s.label.clone())
                    .collect::<Vec<_>>();
                let expected_labels = op
                    .get("labels")
                    .and_then(Value::as_array)
                    .expect("labels")
                    .iter()
                    .map(|v| v.as_str().expect("label").to_string())
                    .collect::<Vec<_>>();
                assert_eq!(labels, expected_labels);
                assert_eq!(
                    pool.size(),
                    op.get("size").and_then(Value::as_u64).unwrap() as usize
                );
            }
            "pruneToHeight" => {
                let c = op.get("c").and_then(Value::as_str).expect("c");
                let dropped = pool.prune_to_height(c);
                assert_eq!(
                    dropped,
                    op.get("dropped").and_then(Value::as_u64).unwrap() as usize
                );
                assert_eq!(
                    pool.size(),
                    op.get("size").and_then(Value::as_u64).unwrap() as usize
                );
                let labels = pool
                    .for_chain(c)
                    .into_iter()
                    .map(|s| s.label.clone())
                    .collect::<Vec<_>>();
                let expected_labels = op
                    .get("labels")
                    .and_then(Value::as_array)
                    .expect("labels")
                    .iter()
                    .map(|v| v.as_str().expect("label").to_string())
                    .collect::<Vec<_>>();
                assert_eq!(labels, expected_labels);
            }
            other => panic!("unknown op {other}"),
        }
    }
}
