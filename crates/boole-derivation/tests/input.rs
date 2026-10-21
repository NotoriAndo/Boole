//! T4: D4/D5/D7; input failures stall rather than becoming item drops.
use boole_derivation::{check_l1_link, derive, InboxLog, InputError, L1Block, ParentHeader};
use serde_json::Value;
fn input(index: usize) -> (L1Block, Vec<InboxLog>, ParentHeader) {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let row = &rows[index];
    (
        serde_json::from_value(row["l1"].clone()).unwrap(),
        serde_json::from_value(row["logs"].clone()).unwrap(),
        serde_json::from_value(row["parent"].clone()).unwrap(),
    )
}
#[test]
fn t4_height() {
    let (mut l, logs, p) = input(0);
    l.number += 1;
    assert_eq!(derive(&l, &logs, &p), Err(InputError::Height));
}
#[test]
fn t4_log_consistency_and_duplicates() {
    let (l, logs, p) = input(0);
    for field in 0..5 {
        let mut bad = logs.clone();
        match field {
            0 => bad[0].address = p.miner.clone(),
            1 => bad[0].topics[0] = l.hash.clone(),
            2 => bad[0].block_hash = l.parent_hash.clone(),
            3 => bad[0].removed = true,
            _ => bad[0].topics.clear(),
        };
        assert_eq!(derive(&l, &bad, &p), Err(InputError::Log), "field {field}");
    }
    let mut dup = logs.clone();
    dup.push(logs[0].clone());
    assert_eq!(derive(&l, &dup, &p), Err(InputError::DuplicateLog));
}
#[test]
fn t4_extra_and_overflow() {
    let (l, logs, p) = input(0);
    for value in [
        "0x",
        "0x0100",
        "0x0200000032000000060000000000000000",
        "nohex",
    ] {
        let mut p = p.clone();
        p.extra_data = value.into();
        assert_eq!(derive(&l, &logs, &p), Err(InputError::ExtraData));
    }
    let mut p = p;
    p.timestamp = u64::MAX;
    assert_eq!(derive(&l, &logs, &p), Err(InputError::TimestampOverflow));
}
#[test]
fn t4_l1_link() {
    let (mut l, logs, p) = input(1);
    assert!(check_l1_link(&l, &p).is_ok());
    l.parent_hash = l.hash.clone();
    assert_eq!(check_l1_link(&l, &p), Err(InputError::L1Link));
    assert_eq!(derive(&l, &logs, &p), Err(InputError::L1Link));
    let (l, logs, mut p) = input(1);
    p.l1_info = None;
    assert_eq!(derive(&l, &logs, &p), Err(InputError::L1Link));
    p.l1_info = Some("0x7ec0".into());
    assert_eq!(derive(&l, &logs, &p), Err(InputError::L1Link));
}
#[test]
fn t4_timestamp_clamp() {
    let (mut l, logs, p) = input(0);
    for timestamp in [p.timestamp - 1, p.timestamp] {
        l.timestamp = timestamp;
        assert_eq!(
            derive(&l, &logs, &p).unwrap().attributes.timestamp,
            format!("0x{:x}", p.timestamp + 1)
        );
    }
}
