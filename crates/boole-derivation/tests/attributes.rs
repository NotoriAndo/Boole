//! T2/T3/T5: D2/D5–D8/D17, independent recorded oracle.
use boole_derivation::{derive, inbox, InboxLog, L1Block, ParentHeader};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn rows() -> Vec<Value> {
    let main: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main-budget-v1.json"
    ))
    .unwrap();
    let mut rows = main["heights"].as_array().unwrap().clone();
    rows.extend(
        serde_json::from_str::<Vec<Value>>(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-basefee.json"
        ))
        .unwrap(),
    );
    rows
}

#[test]
fn t3_golden_and_t5_determinism_all_37_heights() {
    let original = include_str!("../../../fixtures/devnet-m1/s1b-recorded-main.json");
    let budget: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main-budget-v1.json"
    ))
    .unwrap();
    assert_eq!(budget["acceptance"]["recorded_abc"], true);
    assert_eq!(budget["acceptance"]["live_abc"], true);
    assert_eq!(
        budget["provenance"]["source_sha256"],
        format!("0x{}", hex::encode(Sha256::digest(original.as_bytes())))
    );
    let baseline: Vec<Value> = serde_json::from_str(original).unwrap();
    for (row, old) in budget["heights"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&baseline)
        .take(23)
    {
        for field in ["attributes", "blockHash", "stateRoot", "receiptsRoot"] {
            assert_eq!(row[field], old[field]);
        }
    }
    let rows = rows();
    assert_eq!(rows.len(), 37);
    for row in rows {
        let l1: L1Block = serde_json::from_value(row["l1"].clone()).unwrap();
        let mut logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone()).unwrap();
        let parent: ParentHeader = serde_json::from_value(row["parent"].clone()).unwrap();
        let a = derive(&l1, &logs, &parent).unwrap();
        assert_eq!(
            serde_json::to_value(&a.attributes).unwrap(),
            row["attributes"],
            "height {}",
            row["height"]
        );
        assert_eq!(
            a.decisions.len(),
            row["decisions"].as_array().unwrap().len()
        );
        for (actual, expected) in a.decisions.iter().zip(row["decisions"].as_array().unwrap()) {
            assert_eq!(actual.rule, expected["rule"]);
            assert_eq!(actual.forward, expected["forward"]);
        }
        let b = derive(&l1, &logs, &parent).unwrap();
        logs.reverse();
        let c = derive(&l1, &logs, &parent).unwrap();
        let bytes = a.canonical_bytes().unwrap();
        assert_eq!(bytes, b.canonical_bytes().unwrap());
        assert_eq!(bytes, c.canonical_bytes().unwrap());
        assert_eq!(
            Sha256::digest(&bytes),
            Sha256::digest(c.canonical_bytes().unwrap())
        );
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("{\"timestamp\":"));
        let fields = [
            "timestamp",
            "prevRandao",
            "suggestedFeeRecipient",
            "withdrawals",
            "parentBeaconBlockRoot",
            "transactions",
            "noTxPool",
            "gasLimit",
            "eip1559Params",
            "minBaseFee",
        ];
        for pair in fields.windows(2) {
            assert!(
                text.find(&format!("\"{}\":", pair[0])).unwrap()
                    < text.find(&format!("\"{}\":", pair[1])).unwrap()
            );
        }
    }
}

fn envelope(item: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 64];
    b[31] = 32;
    b[63] = item.len() as u8;
    b.extend(item);
    b
}

#[test]
fn t2_inbox_kind_and_abi() {
    assert_eq!(
        inbox::user_body(&envelope(&[1, 2, 0xc0])).unwrap(),
        [2, 0xc0]
    );
    for kind in [0, 2, 3, 15, 255] {
        assert_eq!(
            inbox::user_body(&envelope(&[kind])),
            Err("ITEM-KIND-NOT-ACCEPTED")
        );
    }
    assert_eq!(
        inbox::user_body(&envelope(&[])),
        Err("ITEM-KIND-NOT-ACCEPTED")
    );
    let mut bad = envelope(&[1]);
    bad[31] = 31;
    assert_eq!(inbox::user_body(&bad), Err("INBOX-ENVELOPE-INVALID"));
    bad[31] = 32;
    bad[63] = 2;
    assert_eq!(inbox::user_body(&bad), Err("INBOX-ENVELOPE-INVALID"));
    bad[32] = 255;
    assert_eq!(inbox::user_body(&bad), Err("INBOX-ENVELOPE-INVALID"));
    assert_eq!(inbox::user_body(&[0; 63]), Err("INBOX-ENVELOPE-INVALID"));
}

#[test]
fn t2_dropped_items_do_not_affect_other_items() {
    let row = &rows()[0];
    let l1 = serde_json::from_value(row["l1"].clone()).unwrap();
    let parent = serde_json::from_value(row["parent"].clone()).unwrap();
    let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone()).unwrap();
    let baseline = derive(&l1, &logs, &parent).unwrap();
    let mut mixed = logs.clone();
    for (index, data) in [
        vec![0; 63],
        envelope(&[2]),
        envelope(&[1]),
        envelope(&[1, 2, 0xc0]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut log = logs[0].clone();
        log.transaction_index = 10 + index as u64;
        log.log_index = 10 + index as u64;
        log.data = format!("0x{}", hex::encode(data));
        mixed.push(log);
    }
    mixed.reverse();
    let result = derive(&l1, &mixed, &parent).unwrap();
    assert_eq!(result.attributes, baseline.attributes);
    assert_eq!(
        result.decisions[3..]
            .iter()
            .map(|x| x.rule)
            .collect::<Vec<_>>(),
        [
            "INBOX-ENVELOPE-INVALID",
            "ITEM-KIND-NOT-ACCEPTED",
            "USER-TYPE-NOT-ALLOWED",
            "USER-DECODE-OR-SIGNATURE-INVALID"
        ]
    );
}
