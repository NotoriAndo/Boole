use boole_derivation::{
    attributes::derive_with_m2,
    m2::{Config, State},
    L1Block, ParentHeader,
};
use serde_json::Value;

fn inputs() -> (L1Block, ParentHeader) {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    (
        serde_json::from_value(rows[0]["l1"].clone()).unwrap(),
        serde_json::from_value(rows[0]["parent"].clone()).unwrap(),
    )
}
#[test]
fn every_current_block_commits_its_post_fold_root_before_users() {
    let (l1, parent) = inputs();
    let c = Config::devnet();
    let state = State::new(&c, parent.gas_limit).unwrap();
    let (derived, next) = derive_with_m2(&state, &c, &l1, &[], &parent).unwrap();
    assert_eq!(
        derived.attributes.transactions.len(),
        2,
        "empty L1 blocks still need M2ROOT"
    );
    let raw =
        boole_derivation::attributes::hex_bytes(&derived.attributes.transactions[1], "deposit")
            .unwrap();
    assert!(raw.windows(32).any(|w| w == next.root));
}
#[test]
fn s4_includes_both_system_reservations() {
    let c = Config::devnet();
    let boundary = 2_000_000 + c.q_max * c.verdict_gas + 16_777_216;
    assert!(c.validate(boundary).is_ok());
    assert_eq!(c.validate(boundary - 1), Err(4));
}

#[test]
fn strict_m2root_decoder_binds_every_field_and_rejects_noncanonical_forms() {
    use boole_derivation::{
        attributes::hex_bytes,
        rlp,
        system::{m2_root_deposit, read_m2_root},
        InputError,
    };
    let (l1, _) = inputs();
    let root = [19; 32];
    let tx = m2_root_deposit(&l1, root).unwrap();
    assert_eq!(read_m2_root(&tx, l1.number, &l1.hash).unwrap(), root);
    let raw = hex_bytes(&tx, "root").unwrap();
    let f = rlp::decode(&raw[1..]).unwrap().children.unwrap();
    let fields: Vec<_> = f
        .iter()
        .map(|i| raw[1 + i.span.start..1 + i.span.end].to_vec())
        .collect();
    for index in 0..8 {
        let mut bad = fields.clone();
        let mut value = f[index].bytes.to_vec();
        if value.is_empty() {
            value.push(0);
        } else {
            value[0] ^= 1;
        }
        bad[index] = rlp::bytes(&value);
        let invalid =
            boole_derivation::attributes::hex_string(&[vec![0x7e], rlp::list(&bad)].concat());
        assert_eq!(
            read_m2_root(&invalid, l1.number, &l1.hash),
            Err(InputError::M2Root { height: l1.number }),
            "field {index}"
        );
    }
    for invalid in [
        tx.clone() + "00",
        tx.replacen("0x7e", "0x02", 1),
        tx[..tx.len() - 2].to_owned(),
    ] {
        assert!(read_m2_root(&invalid, l1.number, &l1.hash).is_err());
    }
    assert!(read_m2_root(&tx, l1.number + 1, &l1.hash).is_err());
    assert!(read_m2_root(&tx, l1.number, &format!("0x{}", "ee".repeat(32))).is_err());
}

#[test]
fn m2root_exact_bytes_match_the_independent_vector() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/root-system-v1.2.json"
    ))
    .unwrap();
    let (mut l1, _) = inputs();
    l1.hash = v["origin"].as_str().unwrap().into();
    l1.number = v["number"].as_u64().unwrap();
    let root = boole_derivation::attributes::hex_bytes(v["root"].as_str().unwrap(), "root")
        .unwrap()
        .try_into()
        .unwrap();
    assert_eq!(
        boole_derivation::system::m2_root_deposit(&l1, root).unwrap(),
        v["deposit"]
    );
}
#[test]
fn current_genesis_adds_only_the_stop_sink_and_deposit_gas_is_fatal() {
    let old: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m1/l2-genesis.json")).unwrap();
    let mut current: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/l2-genesis-v1.2.json"
    ))
    .unwrap();
    let sink = current["alloc"]
        .as_object_mut()
        .unwrap()
        .remove(boole_derivation::config::M2_ROOT_SINK)
        .unwrap();
    assert_eq!(sink, serde_json::json!({"balance":"0x0","code":"0x00"}));
    assert_eq!(current, old);
    let (l1, mut parent) = inputs();
    let c = Config::devnet();
    let state = State::new(&c, 30_000_000).unwrap();
    parent.gas_limit = 1_999_999;
    assert_eq!(
        derive_with_m2(&state, &c, &l1, &[], &parent).unwrap_err(),
        boole_derivation::InputError::SystemGasBudget
    );
}
