//! Accounting fixtures only; these are not network block-size or policy claims.
use std::time::Instant;

use boole_core::native_ledger::{EmissionSchedule, NativeLedger};
use boole_core::{SignedEnvelope, SigningKeyV2};

const NETWORK: &str = "native-pending-fixture";

fn transfer(owner: &SigningKeyV2, to: &str, amount: u128, nonce: u64) -> SignedEnvelope {
    owner
        .sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": to,
                "amount": amount.to_string(), "fee": "1", "nonce": nonce.to_string(),
                "validBefore": "100"
            }),
            Some(NETWORK),
        )
        .unwrap()
}

#[test]
fn a_prepared_reservation_can_be_discarded_or_committed_without_partial_effects() {
    let owner = SigningKeyV2::from_dev_id("prepared-pending-owner");
    let recipient = SigningKeyV2::from_dev_id("prepared-pending-recipient").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(100, 100, 100).unwrap()).unwrap();
    ledger.apply_block(1, &owner.pk_hex(), &[]).unwrap();
    let mut view = ledger.pending_view().unwrap();
    let first = transfer(&owner, &recipient, 40, 0);
    drop(view.prepare(&first).unwrap());
    assert_eq!(view.available_balance(&owner.pk_hex()), 100);
    assert_eq!(view.available_balance(&recipient), 0);
    assert_eq!(view.next_nonce(&owner.pk_hex()), 0);
    assert!(view.prepare(&transfer(&owner, &recipient, 100, 0)).is_err());
    assert!(view.prepare(&transfer(&owner, &recipient, 1, 1)).is_err());
    assert!(view
        .prepare(&transfer(&owner, &recipient, u128::MAX, 0))
        .is_err());
    let mut tampered = first.clone();
    tampered.payload["amount"] = "39".into();
    assert!(view.prepare(&tampered).is_err());
    assert_eq!(view.available_balance(&owner.pk_hex()), 100);
    view.prepare(&first).unwrap().commit();
    assert_eq!(view.available_balance(&owner.pk_hex()), 59);
    assert_eq!(view.available_balance(&recipient), 40);
    assert_eq!(view.next_nonce(&owner.pk_hex()), 1);
    assert!(view.prepare(&first).is_err());
    // Recipient == sender must apply debit before credit and reserve only the fee.
    view.prepare(&transfer(&owner, &owner.pk_hex(), 58, 1))
        .unwrap()
        .commit();
    assert_eq!(view.available_balance(&owner.pk_hex()), 58);
    assert_eq!(view.next_nonce(&owner.pk_hex()), 2);
    assert_eq!(ledger.balance(&owner.pk_hex()), 100);
    assert_eq!(ledger.next_nonce(&owner.pk_hex()), 0);
}

#[test]
fn pending_transfers_preserve_a_large_unrelated_account_set() {
    const ACCOUNTS: u64 = 16_384;
    let owner = SigningKeyV2::from_dev_id("pending-large-state-owner");
    let mut ledger = NativeLedger::new(
        NETWORK,
        EmissionSchedule::new(1_000_000, 1_000_000, 100).unwrap(),
    )
    .unwrap();
    ledger.apply_block(1, &owner.pk_hex(), &[]).unwrap();
    let recipients: Vec<_> = (0..ACCOUNTS).map(|i| format!("{i:064x}")).collect();
    let funding: Vec<_> = recipients
        .iter()
        .enumerate()
        .map(|(i, pk)| transfer(&owner, pk, 1, i as u64))
        .collect();
    ledger.apply_block(2, &owner.pk_hex(), &funding).unwrap();
    drop(funding);
    let before = ledger.clone();
    let pending: Vec<_> = (ACCOUNTS..ACCOUNTS + 512)
        .map(|nonce| transfer(&owner, &owner.pk_hex(), 1, nonce))
        .collect();
    let mut view = ledger.pending_view().unwrap();
    let started = Instant::now();
    for tx in &pending {
        view.push(tx).unwrap();
    }
    eprintln!(
        "512 reservations with {ACCOUNTS} unrelated accounts: {:?}",
        started.elapsed()
    );
    assert_eq!(view.next_nonce(&owner.pk_hex()), ACCOUNTS + 512);
    assert_eq!(
        view.available_balance(&owner.pk_hex()),
        before.spendable_balance(&owner.pk_hex()) - 512
    );
    for pk in recipients {
        assert_eq!(view.available_balance(&pk), 1);
    }
    assert_eq!(ledger, before, "reservations are never canonical debits");
}
