//! D12: cumulative budget uses gas limits, including client-invalid candidates.
use boole_derivation::{derive, rlp, Derived, InboxLog, InputError, L1Block, ParentHeader};
use serde_json::Value;
const DROP: &str = "USER-GAS-EXCEEDS-BLOCK-BUDGET";
fn block(gas_limits: &[u64], parent_limit: u64) -> Result<Derived, InputError> {
    block_version(gas_limits, parent_limit, false)
}
fn block_version(
    gas_limits: &[u64],
    parent_limit: u64,
    current: bool,
) -> Result<Derived, InputError> {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))
    .unwrap();
    let row = &rows[0];
    let l1: L1Block = serde_json::from_value(row["l1"].clone()).unwrap();
    let mut parent: ParentHeader = serde_json::from_value(row["parent"].clone()).unwrap();
    parent.gas_limit = parent_limit;
    let raw = hex::decode(
        row["decisions"][0]["raw"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    let decoded = rlp::decode(&raw[1..]).unwrap();
    let fields: Vec<_> = decoded
        .children
        .unwrap()
        .iter()
        .map(|f| raw[1 + f.span.start..1 + f.span.end].to_vec())
        .collect();
    let template: InboxLog = serde_json::from_value(row["logs"][0].clone()).unwrap();
    let logs: Vec<_> = gas_limits
        .iter()
        .enumerate()
        .map(|(i, gas)| {
            let mut fields = fields.clone();
            fields[4] = rlp::uint(*gas);
            let item = [vec![1, 2], rlp::list(&fields)].concat();
            let mut envelope = vec![0; 64];
            envelope[31] = 32;
            envelope[56..64].copy_from_slice(&(item.len() as u64).to_be_bytes());
            envelope.extend(item);
            let mut log = template.clone();
            log.transaction_index = i as u64;
            log.log_index = i as u64;
            log.data = format!("0x{}", hex::encode(envelope));
            log
        })
        .collect();
    if current {
        let c = boole_derivation::m2::Config::devnet();
        let state = boole_derivation::m2::State::new(&c, parent_limit)?;
        boole_derivation::derive_with_m2(&state, &c, &l1, &logs, &parent).map(|x| x.0)
    } else {
        derive(&l1, &logs, &parent)
    }
}
#[test]
fn budget_exact_limit_forwards() {
    assert!(block(&[29_000_000], 30_000_000).unwrap().decisions[0].forward);
}
#[test]
fn budget_plus_one_drops() {
    let d = block(&[29_000_001], 30_000_000).unwrap();
    assert_eq!(d.decisions[0].rule, DROP);
    assert!(!d.decisions[0].forward);
}
#[test]
fn dropped_large_candidate_does_not_consume_budget() {
    let d = block(&[30_000_000, 21_000, 28_979_000], 30_000_000).unwrap();
    assert_eq!(d.decisions[0].rule, DROP);
    assert!(d.decisions[1].forward);
    assert!(d.decisions[2].forward);
}
#[test]
fn cumulative_three_twelve_million_candidates() {
    let d = block(&[12_000_000; 3], 30_000_000).unwrap();
    assert_eq!(
        d.decisions.iter().map(|x| x.forward).collect::<Vec<_>>(),
        [true, true, false]
    );
    assert_eq!(d.decisions[2].rule, DROP);
    assert_eq!(d.attributes.transactions.len(), 3);
}
#[test]
fn system_reservation_underflow_is_an_input_error() {
    assert_eq!(block(&[], 999_999), Err(InputError::SystemGasBudget));
    assert!(block(&[], 1_000_000).is_ok());
}

#[test]
fn current_user_budget_reserves_m2root_at_the_exact_f6_boundary() {
    let exact = block_version(&[28_000_000], 30_000_000, true).unwrap();
    assert!(exact.decisions[0].forward);
    assert_eq!(exact.attributes.transactions.len(), 3);
    let over = block_version(&[28_000_001, 21_000, 27_979_000], 30_000_000, true).unwrap();
    assert_eq!(over.decisions[0].rule, DROP);
    assert!(over.decisions[1].forward);
    assert!(over.decisions[2].forward);
    assert_eq!(over.attributes.transactions.len(), 4);
}
