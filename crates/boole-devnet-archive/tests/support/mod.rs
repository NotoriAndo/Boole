use anyhow::Result;
// One inbox-addressed signed transaction with an authenticated status-0 receipt,
// no logs, and a zero bloom. The original three-transaction fixture is retained.
pub fn reverted_inbox_row(
    mut row: boole_devnet_archive::input::InputRow,
) -> Result<boole_devnet_archive::input::InputRow> {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        rlp,
        user_tx::keccak,
    };
    use boole_devnet_archive::input::receipts_root;
    let tx = hex_bytes(&row.transactions.as_ref().unwrap()[1], "inbox transaction")?;
    let receipt = hex_bytes(&row.receipts.as_ref().unwrap()[2], "reverted receipt")?;
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let decoded = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let fields = decoded.children.unwrap();
    let mut fields: Vec<_> = fields
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    fields[4] = rlp::bytes(&receipts_root(std::slice::from_ref(&tx)));
    fields[5] = rlp::bytes(&receipts_root(std::slice::from_ref(&receipt)));
    fields[6] = rlp::bytes(&[0; 256]);
    let raw = rlp::list(&fields);
    row.header_rlp = hex_string(&raw);
    row.block_hash = hex_string(&keccak(&raw));
    row.transactions = Some(vec![hex_string(&tx)]);
    row.receipts = Some(vec![hex_string(&receipt)]);
    assert!(!boole_devnet_archive::input::header_needs_receipts(&raw)?);
    assert_eq!(
        row.verify()?.canonical_receipts,
        vec![(hex_string(&keccak(&tx)), false)]
    );
    assert!(row.verify()?.logs.is_empty());
    Ok(row)
}
