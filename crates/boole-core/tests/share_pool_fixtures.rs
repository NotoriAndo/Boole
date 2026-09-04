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

fn make_pool_share(label: &str, pk_suffix: &str, c: &str) -> PoolShare {
    PoolShare {
        label: label.to_string(),
        pk: format!("{:0>64}", pk_suffix),
        n: format!("{:0>64}", format!("n{pk_suffix}")),
        j: format!("{:0>64}", format!("j{pk_suffix}")),
        c: c.to_string(),
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
        MinShareScoreMultiplier: serde_json::Number::from(2),
        K_max: 16,
        ShareCapPerPK_Block: 1,
        SharePoolGlobalCap: None,
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
fn share_pool_enforces_global_cap_before_growing_unbounded() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);

    assert_eq!(
        pool.accept(make_pool_share("first", "01", c)),
        AcceptResult::Ok
    );
    assert_eq!(
        pool.accept(make_pool_share("second", "02", c)),
        AcceptResult::Ok
    );
    assert_eq!(
        pool.accept(make_pool_share("third", "03", c)),
        AcceptResult::Err {
            reason: SharePoolRejectReason::GlobalCapExceeded,
        }
    );
    assert_eq!(pool.size(), 2);
}

#[test]
fn semantic_rejects_release_active_capacity_but_keep_bounded_duplicate_tombstones() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);
    let rejected_one = make_pool_share("rejected-one", "01", c);
    let rejected_two = make_pool_share("rejected-two", "02", c);
    let valid = make_pool_share("valid", "03", c);

    assert_eq!(pool.accept(rejected_one.clone()), AcceptResult::Ok);
    assert!(pool.reserve_for_semantic_check(&rejected_one));
    assert_eq!(
        pool.size(),
        0,
        "an unverified share cannot hold pool capacity"
    );
    assert_eq!(
        pool.accept(rejected_one.clone()),
        AcceptResult::Err {
            reason: SharePoolRejectReason::Duplicate,
        },
        "the capacity-free tombstone must still reject an exact retry"
    );

    let mut same_pk = rejected_one.clone();
    same_pk.label = "same-pk-new-work".to_string();
    same_pk.n = format!("{:0>64}", "n04");
    same_pk.j = format!("{:0>64}", "j04");
    assert_eq!(
        pool.accept(same_pk.clone()),
        AcceptResult::Ok,
        "reservation must decrement the active per-pk count"
    );
    assert!(pool.reserve_for_semantic_check(&same_pk));

    assert_eq!(pool.accept(rejected_two.clone()), AcceptResult::Ok);
    assert!(pool.reserve_for_semantic_check(&rejected_two));
    assert_eq!(pool.size(), 0);
    assert_eq!(
        pool.accept(valid),
        AcceptResult::Ok,
        "unique semantic rejects must not fill the active global pool"
    );
}

#[test]
fn semantic_tombstones_are_bounded_and_evict_oldest_without_touching_active_order() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);
    let first = make_pool_share("first", "01", c);
    let second = make_pool_share("second", "02", c);
    let third = make_pool_share("third", "03", c);

    for share in [&first, &second, &third] {
        assert_eq!(pool.accept(share.clone()), AcceptResult::Ok);
        assert!(pool.reserve_for_semantic_check(share));
    }
    assert_eq!(
        pool.accept(first.clone()),
        AcceptResult::Ok,
        "the third tombstone must evict the oldest when the separate bound is two"
    );
    assert!(pool.restore_after_semantic_check(second.clone()));
    assert_eq!(
        pool.for_chain(c)
            .into_iter()
            .map(|share| share.label.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second"],
        "remove/restore must keep the active insertion order and accounting coherent"
    );
}

#[test]
fn semantic_reservation_restores_once_and_is_cleared_by_a_head_change() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let next_c = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);
    let share = make_pool_share("verified", "01", c);

    assert_eq!(pool.accept(share.clone()), AcceptResult::Ok);
    assert!(pool.reserve_for_semantic_check(&share));
    assert!(pool.restore_after_semantic_check(share.clone()));
    assert_eq!(pool.size(), 1);
    assert!(
        !pool.restore_after_semantic_check(share.clone()),
        "one reservation cannot be restored twice"
    );

    assert!(pool.reserve_for_semantic_check(&share));
    pool.set_current_c(next_c);
    assert!(
        !pool.restore_after_semantic_check(share),
        "a verifier verdict for the old head must not survive a head change"
    );
}

#[test]
fn unavailable_semantic_check_releases_tombstone_without_restoring_candidate() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);
    let unavailable = make_pool_share("unavailable", "01", c);

    assert_eq!(pool.accept(unavailable.clone()), AcceptResult::Ok);
    assert!(pool.reserve_for_semantic_check(&unavailable));
    assert_eq!(pool.size(), 0);
    assert!(pool.release_semantic_reservation(&unavailable));
    assert_eq!(
        pool.size(),
        0,
        "availability cleanup must not make an unverified share eligible"
    );
    assert_eq!(
        pool.accept(unavailable),
        AcceptResult::Ok,
        "RetryableUnavailable must permit the exact submission to retry"
    );
}

#[test]
fn deterministic_semantic_reject_does_not_release_duplicate_tombstone() {
    let c = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut pool = SharePool::new_with_global_cap(4, 2);
    pool.set_current_c(c);
    let rejected = make_pool_share("rejected", "01", c);

    assert_eq!(pool.accept(rejected.clone()), AcceptResult::Ok);
    assert!(pool.reserve_for_semantic_check(&rejected));
    assert_eq!(
        pool.accept(rejected),
        AcceptResult::Err {
            reason: SharePoolRejectReason::Duplicate,
        },
        "a deterministic semantic reject must remain a bounded current-head duplicate"
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
