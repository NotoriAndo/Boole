//! T1/T2: D9–D13. Recorded transactions also cover setup and continuations.
use boole_derivation::{rlp, user_tx::filter_user};
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
    let mut cases = 0;
    for row in rows() {
        let gas = u64::from_str_radix(
            row["parent"]["gasLimit"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
            16,
        )
        .unwrap();
        for expected in row["decisions"].as_array().unwrap() {
            let raw =
                hex::decode(expected["raw"].as_str().unwrap().trim_start_matches("0x")).unwrap();
            let actual = filter_user(&raw, gas);
            assert_eq!(actual.rule, expected["rule"], "height {}", row["height"]);
            assert_eq!(actual.forward, expected["forward"]);
            assert_eq!(actual.tx_hash, expected["tx_hash"]);
            if let Some(signer) = expected["signer"].as_str() {
                assert_eq!(actual.sender.as_deref(), Some(signer));
            }
        }
        if row["height"].as_u64().unwrap() % 2 == 0 {
            cases += 1;
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
            filter_user(&raw, 30_000_000).rule,
            INVALID,
            "{}",
            hex::encode(raw)
        );
    }
    assert_eq!(filter_user(&[], 30_000_000).rule, "USER-TYPE-NOT-ALLOWED");
    for kind in [0, 1, 3, 4, 0x7e, 0xff] {
        assert_eq!(
            filter_user(&[kind, 0xc0], 30_000_000).rule,
            "USER-TYPE-NOT-ALLOWED"
        );
    }
    // r=5 cannot be recovered as a secp256k1 x-coordinate.
    let mut f = original.clone();
    f[10] = rlp::uint(5);
    f[11] = rlp::uint(1);
    assert_eq!(
        filter_user(&tx(&f), 30_000_000).rule,
        "USER-SENDER-UNRECOVERABLE"
    );
    let mut f = original;
    f[4] = rlp::uint(30_000_000);
    assert!(filter_user(&tx(&f), 30_000_000).forward);
    f[8] = rlp::list(&[rlp::list(&[
        rlp::bytes(&[1; 20]),
        rlp::list(&[rlp::bytes(&[1; 32])]),
    ])]);
    assert!(
        filter_user(&tx(&f), 30_000_000).forward,
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
