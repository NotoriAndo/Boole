use anyhow::Result;
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::InputRow,
    journal::{InputJournal, NetworkIdentity},
};
#[test]
fn pinned_anvil_input_proofs_reverify_without_a_node() -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let rows: Vec<InputRow> = serde_json::from_value(value["rows"].clone())?;
    let dir = PrivateTempDir::new("m4-native-input-replay")?;
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        boole_derivation::attributes::hex_string(&[1; 32]),
        30_000_000,
    )?;
    let mut journal =
        InputJournal::open(dir.path(), network.clone(), || Ok(rows[0].clone()), false)?;
    for row in rows.iter().skip(1) {
        journal.append(row)?;
    }
    drop(journal);
    let journal = InputJournal::open(dir.path(), network, || unreachable!(), false)?;
    for h in 0..3 {
        assert_eq!(journal.get(h)?.block.hash, rows[h as usize].block_hash);
    }
    let verified = journal.get(1)?;
    assert_eq!(verified.logs[0].log_index, 1);
    for (hash, success) in verified.canonical_receipts {
        assert_eq!(journal.transaction_receipt(&hash)?, Some((1, success)));
    }
    Ok(())
}
