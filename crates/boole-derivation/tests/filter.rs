//! T1/T2: D9–D13. Recorded transactions also cover setup and continuations.
use boole_derivation::{config, rlp, user_tx::filter_user, ParentHeader};
use serde_json::Value;

fn rows() -> Vec<Value> {
    [
        include_str!("../../../fixtures/devnet-m1/s1b-recorded-main.json"),
        include_str!("../../../fixtures/devnet-m1/s1b-recorded-basefee.json"),
    ]
    .into_iter()
    .flat_map(|s| serde_json::from_str::<Vec<Value>>(s).unwrap())
    .collect()
}

#[test]
fn t1_all_18_cases_and_continuations() {
    let schedule: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m1/s1b-cases.json")).unwrap();
    let mut cases = 0;
    for (profile, recording) in [
        (
            "main",
            include_str!("../../../fixtures/devnet-m1/s1b-recorded-main.json"),
        ),
        (
            "basefee",
            include_str!("../../../fixtures/devnet-m1/s1b-recorded-basefee.json"),
        ),
    ] {
        let recorded: Vec<Value> = serde_json::from_str(recording).unwrap();
        for (case, row) in schedule[profile].as_array().unwrap().iter().zip(recorded) {
            let is_case = case["role"] == "case";
            if is_case {
                cases += 1;
            }
            let parent: ParentHeader = serde_json::from_value(row["parent"].clone()).unwrap();
            let mut budget = parent.gas_limit.checked_sub(config::L1INFO_GAS).unwrap();
            let raws = case["raws"].as_array().unwrap();
            assert_eq!(raws.len(), row["decisions"].as_array().unwrap().len());
            for (index, (raw, expected)) in raws
                .iter()
                .zip(row["decisions"].as_array().unwrap())
                .enumerate()
            {
                let rule = if is_case {
                    match case["label"].as_str().unwrap() {
                        "1a-bad-rlp" | "3-bad-signature" => "USER-DECODE-OR-SIGNATURE-INVALID",
                        "1b-unknown-type" | "11a-blob" | "11b-7702" | "12-user-mint" => {
                            "USER-TYPE-NOT-ALLOWED"
                        }
                        "7b-above-block-gas" => "USER-GAS-EXCEEDS-BLOCK-BUDGET",
                        "9-sum-gas-limits" if index == 2 => "USER-GAS-EXCEEDS-BLOCK-BUDGET",
                        _ => "FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION",
                    }
                } else {
                    "FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION"
                };
                let bytes = hex::decode(raw.as_str().unwrap().trim_start_matches("0x")).unwrap();
                let actual = filter_user(&bytes, &mut budget);
                assert_eq!(
                    actual.rule, rule,
                    "{} {} item {index}",
                    case["label"], case["role"]
                );
                assert_eq!(
                    actual.forward,
                    rule == "FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION"
                );
                assert_eq!(actual.tx_hash, expected["tx_hash"]);
                if let Some(signer) = expected["signer"].as_str() {
                    assert_eq!(actual.sender.as_deref(), Some(signer));
                }
            }
        }
    }
    assert_eq!(cases, 18);
}

fn fields() -> Vec<Vec<u8>> {
    let raw = hex::decode(
        rows()[0]["decisions"][0]["raw"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    let item = rlp::decode(&raw[1..]).unwrap();
    item.children
        .unwrap()
        .iter()
        .map(|v| raw[1 + v.span.start..1 + v.span.end].to_vec())
        .collect()
}

fn tx(f: &[Vec<u8>]) -> Vec<u8> {
    [vec![2], rlp::list(f)].concat()
}
const INVALID: &str = "USER-DECODE-OR-SIGNATURE-INVALID";

#[test]
fn t2_signature_and_encoding() {
    let original = fields();
    let mut mutations = vec![];
    for (index, value) in [
        (1, rlp::bytes(&[1; 9])),
        (1, rlp::bytes(&[0, 1])),
        (0, rlp::bytes(&[1; 9])),
        (4, rlp::bytes(&[1; 9])),
        (2, rlp::bytes(&[1; 17])),
        (3, rlp::bytes(&[1; 17])),
        (6, rlp::bytes(&[1; 33])),
        (10, rlp::bytes(&[1; 33])),
        (11, rlp::bytes(&[1; 33])),
        (9, rlp::uint(2)),
        (10, rlp::uint(0)),
        (
            10,
            rlp::bytes(
                &hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
                    .unwrap(),
            ),
        ),
        (5, rlp::bytes(&[1; 19])),
        (1, vec![0x81, 1]),
        (1, vec![0xb8, 1, 1]),
        (
            8,
            rlp::list(&[rlp::list(&[
                rlp::bytes(&[1; 20]),
                rlp::list(&[rlp::bytes(&[1; 31])]),
            ])]),
        ),
    ] {
        let mut f = original.clone();
        f[index] = value;
        mutations.push(tx(&f));
    }
    // high-s = n-s, with parity flipped; valid scalar but forbidden malleability.
    let item = rlp::decode(&original[11]).unwrap();
    let n =
        hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
    let mut s = [0u8; 32];
    s[32 - item.bytes.len()..].copy_from_slice(item.bytes);
    let mut high = [0u8; 32];
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let v = i16::from(n[i]) - i16::from(s[i]) - borrow;
        high[i] = v as u8;
        borrow = i16::from(v < 0);
    }
    let mut f = original.clone();
    f[11] = rlp::bytes(&high);
    f[9] = rlp::uint(1);
    mutations.push(tx(&f));
    mutations.push(tx(&original[..11]));
    let mut f = original.clone();
    f.push(rlp::uint(0));
    mutations.push(tx(&f));
    let mut trailing = tx(&original);
    trailing.push(0);
    mutations.push(trailing);
    for raw in mutations {
        assert_eq!(
            filter_user(&raw, &mut 29_000_000).rule,
            INVALID,
            "{}",
            hex::encode(raw)
        );
    }
    assert_eq!(
        filter_user(&[], &mut 29_000_000).rule,
        "USER-TYPE-NOT-ALLOWED"
    );
    for kind in [0, 1, 3, 4, 0x7e, 0xff] {
        assert_eq!(
            filter_user(&[kind, 0xc0], &mut 29_000_000).rule,
            "USER-TYPE-NOT-ALLOWED"
        );
    }
    // r=5 cannot be recovered as a secp256k1 x-coordinate.
    let mut f = original.clone();
    f[10] = rlp::uint(5);
    f[11] = rlp::uint(1);
    assert_eq!(
        filter_user(&tx(&f), &mut 29_000_000).rule,
        "USER-SENDER-UNRECOVERABLE"
    );
    let mut f = original;
    f[4] = rlp::uint(29_000_000);
    assert!(filter_user(&tx(&f), &mut 29_000_000).forward);
    f[8] = rlp::list(&[rlp::list(&[
        rlp::bytes(&[1; 20]),
        rlp::list(&[rlp::bytes(&[1; 32])]),
    ])]);
    assert!(
        filter_user(&tx(&f), &mut 29_000_000).forward,
        "well-shaped nonempty access list"
    );
}

#[test]
fn t2_rlp_bounds_and_depth() {
    for raw in [
        vec![0xb8, 0, 56],
        vec![0xc1, 0x82],
        vec![0x81, 1],
        vec![0xf8, 1, 0x80],
        vec![0xc0, 0],
    ] {
        assert!(rlp::decode(&raw).is_err());
    }
    let mut ok = rlp::bytes(&[]);
    for _ in 0..8 {
        ok = rlp::list(&[ok]);
    }
    assert!(rlp::decode(&ok).is_ok());
    assert!(rlp::decode(&rlp::list(&[ok])).is_err());
}
