//! Structural fixtures only: these numbers are not a network monetary preset.
use boole_core::native_ledger::{EmissionSchedule, NativeLedger, NativeLedgerError};
use boole_core::signed_envelope::{SignedEnvelope, SigningKeyV2};

const NETWORK: &str = "native-ledger-fixture";

fn transfer(key: &SigningKeyV2, to: &str, amount: u128, fee: u128, nonce: u64) -> SignedEnvelope {
    key.sign_for_network(
        &serde_json::json!({
            "schema": "boole.transfer.v1", "from": key.pk_hex(), "to": to,
            "amount": amount.to_string(), "fee": fee.to_string(),
            "nonce": nonce.to_string(), "validBefore": "100"
        }),
        Some(NETWORK),
    )
    .unwrap()
}

#[test]
fn base_issuance_goes_to_the_block_reward_recipient_without_share_credits() {
    let miner = SigningKeyV2::from_dev_id("native-ledger-miner").pk_hex();
    let recipient = SigningKeyV2::from_dev_id("native-ledger-cold-reward").pk_hex();
    let schedule = EmissionSchedule::new(100, 32, 2).unwrap();
    let mut ledger = NativeLedger::new("native-ledger-fixture", schedule).unwrap();

    ledger.apply_block(1, &recipient, &[]).unwrap();

    assert_eq!(ledger.balance(&recipient), 32);
    assert_eq!(ledger.balance(&miner), 0);
    assert_eq!(ledger.issued(), 32);
    assert_eq!(ledger.height(), 1);
}

#[test]
fn signed_transfer_spends_mined_balance_and_pays_only_the_fee_to_the_producer() {
    let alice = SigningKeyV2::from_dev_id("native-ledger-alice");
    let bob = SigningKeyV2::from_dev_id("native-ledger-bob").pk_hex();
    let producer = SigningKeyV2::from_dev_id("native-ledger-producer").pk_hex();
    let schedule = EmissionSchedule::new(1_000, 100, 10).unwrap();
    let mut ledger = NativeLedger::new("native-ledger-fixture", schedule).unwrap();
    ledger.apply_block(1, &alice.pk_hex(), &[]).unwrap();
    let transfer = alice
        .sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1",
                "from": alice.pk_hex(), "to": bob,
                "amount": "40", "fee": "3", "nonce": "0", "validBefore": "2"
            }),
            Some("native-ledger-fixture"),
        )
        .unwrap();

    ledger.apply_block(2, &producer, &[transfer]).unwrap();

    assert_eq!(ledger.balance(&alice.pk_hex()), 57);
    assert_eq!(ledger.balance(&bob), 40);
    assert_eq!(ledger.balance(&producer), 103);
    assert_eq!(ledger.issued(), 200);
}

#[test]
fn one_invalid_transfer_rolls_back_the_entire_block_including_fees_and_nonces() {
    let alice = SigningKeyV2::from_dev_id("atomic-alice");
    let bob = SigningKeyV2::from_dev_id("atomic-bob").pk_hex();
    let producer = SigningKeyV2::from_dev_id("atomic-producer").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(1_000, 100, 10).unwrap()).unwrap();
    ledger.apply_block(1, &alice.pk_hex(), &[]).unwrap();
    let before = ledger.clone();
    let first = transfer(&alice, &bob, 40, 3, 0);
    let second = transfer(&alice, &bob, 99, 1, 1);

    assert!(ledger
        .apply_block(2, &producer, &[first.clone(), second])
        .is_err());
    assert_eq!(
        ledger, before,
        "a rejected block must leave no partial transfer or reward"
    );
    ledger.apply_block(2, &producer, &[first]).unwrap();
    assert_eq!(ledger.next_nonce(&alice.pk_hex()), 1);
}

#[test]
fn weak_ed25519_identity_cannot_authorize_spending() {
    // The identity point has no secret owner. Ordinary (non-strict) Ed25519
    // verification can accept this identity-R, zero-S signature on any message.
    let weak_pk = format!("01{}", "00".repeat(31));
    let recipient = SigningKeyV2::from_dev_id("weak-key-recipient").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(1_000, 100, 10).unwrap()).unwrap();
    ledger.apply_block(1, &weak_pk, &[]).unwrap();
    let before = ledger.clone();
    let forged = SignedEnvelope {
        schema: "boole.signed.v1",
        pk: weak_pk.clone(),
        signature: format!("{weak_pk}{}", "00".repeat(32)),
        network_id: Some(NETWORK.to_string()),
        payload: serde_json::json!({
            "schema": "boole.transfer.v1", "from": weak_pk, "to": recipient,
            "amount": "40", "fee": "1", "nonce": "0", "validBefore": "100"
        }),
    };
    assert!(ledger.apply_block(2, &recipient, &[forged]).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn issuance_halves_on_the_height_boundary_stops_at_the_cap_and_never_wraps() {
    let miner = SigningKeyV2::from_dev_id("schedule-miner").pk_hex();
    let mut ledger = NativeLedger::new(NETWORK, EmissionSchedule::new(60, 32, 2).unwrap()).unwrap();
    // ADR-0011 is INITIAL_REWARD >> (height / HALVING_EPOCH), with no
    // genesis payout: height 1 emits 32, height 2 emits 16, then cap clips 16 to 12.
    for (height, issued) in [(1, 32), (2, 48), (3, 60), (4, 60)] {
        ledger.apply_block(height, &miner, &[]).unwrap();
        assert_eq!(ledger.issued(), issued);
        assert_eq!(ledger.balance(&miner), issued);
    }
    let mut floor =
        NativeLedger::new(NETWORK, EmissionSchedule::new(u128::MAX, 8, 1).unwrap()).unwrap();
    for height in 1..=130 {
        floor.apply_block(height, &miner, &[]).unwrap();
    }
    assert_eq!(
        floor.issued(),
        7,
        "epochs beyond the integer width must emit zero"
    );
    assert_eq!(floor.balance(&miner), 7);
    assert_eq!(
        EmissionSchedule::new(10, 1, 0),
        Err(NativeLedgerError::InvalidSchedule)
    );
}

#[test]
fn only_an_unmodified_network_bound_owner_transfer_can_debit_the_account() {
    let owner = SigningKeyV2::from_dev_id("auth-owner");
    let session = SigningKeyV2::from_dev_id("auth-work-session");
    let recipient = SigningKeyV2::from_dev_id("auth-recipient").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(1_000, 100, 10).unwrap()).unwrap();
    ledger.apply_block(1, &owner.pk_hex(), &[]).unwrap();
    let valid = transfer(&owner, &recipient, 10, 1, 0);
    let before = ledger.clone();
    let mut cases = Vec::new();
    cases.push(owner.sign(&valid.payload).unwrap()); // unbound legacy signature
    cases.push(
        owner
            .sign_for_network(&valid.payload, Some("another-network"))
            .unwrap(),
    );
    cases.push(
        session
            .sign_for_network(&valid.payload, Some(NETWORK))
            .unwrap(),
    );
    let mut rewritten = cases[1].clone();
    rewritten.network_id = Some(NETWORK.to_string());
    cases.push(rewritten);
    for (field, value) in [
        ("amount", "11"),
        ("fee", "2"),
        ("nonce", "1"),
        ("validBefore", "99"),
        ("to", owner.pk_hex().as_str()),
    ] {
        let mut tampered = valid.clone();
        tampered.payload[field] = value.into();
        cases.push(tampered);
    }
    let mut work_payload = valid.payload.clone();
    work_payload["schema"] = "boole.signer.work.v2".into();
    cases.push(
        owner
            .sign_for_network(&work_payload, Some(NETWORK))
            .unwrap(),
    );
    let mut wrong_envelope = valid.clone();
    wrong_envelope.schema = "boole.signed.unknown";
    cases.push(wrong_envelope);
    for (index, bad) in cases.into_iter().enumerate() {
        assert!(
            ledger.apply_block(2, &recipient, &[bad]).is_err(),
            "case {index}"
        );
        assert_eq!(ledger, before, "case {index} changed accounting");
    }
    ledger.apply_block(2, &recipient, &[valid]).unwrap();
    assert_eq!(ledger.balance(&owner.pk_hex()), 89);
}

#[test]
fn nonce_expiry_zero_amount_and_amount_plus_fee_fail_closed_without_consuming_state() {
    let owner = SigningKeyV2::from_dev_id("state-owner");
    let recipient = SigningKeyV2::from_dev_id("state-recipient").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(1_000, 100, 10).unwrap()).unwrap();
    ledger.apply_block(1, &owner.pk_hex(), &[]).unwrap();
    let before = ledger.clone();
    let mut expired_payload = transfer(&owner, &recipient, 1, 1, 0).payload;
    expired_payload["validBefore"] = "1".into();
    let expired = owner
        .sign_for_network(&expired_payload, Some(NETWORK))
        .unwrap();
    for (bad, error) in [
        (
            transfer(&owner, &recipient, 100, 1, 0),
            NativeLedgerError::InsufficientBalance,
        ),
        (
            transfer(&owner, &recipient, 0, 1, 0),
            NativeLedgerError::ZeroAmount,
        ),
        (
            transfer(&owner, &recipient, u128::MAX, 1, 0),
            NativeLedgerError::Overflow,
        ),
        (
            transfer(&owner, &recipient, 1, 1, 1),
            NativeLedgerError::UnexpectedNonce {
                expected: 0,
                actual: 1,
            },
        ),
        (expired, NativeLedgerError::Expired),
    ] {
        assert_eq!(ledger.apply_block(2, &recipient, &[bad]), Err(error));
        assert_eq!(ledger, before);
    }
    let valid = transfer(&owner, &recipient, 50, 1, 0);
    ledger
        .apply_block(2, &recipient, std::slice::from_ref(&valid))
        .unwrap();
    let committed = ledger.clone();
    assert_eq!(
        ledger.apply_block(3, &recipient, &[valid]),
        Err(NativeLedgerError::UnexpectedNonce {
            expected: 1,
            actual: 0
        })
    );
    assert_eq!(ledger, committed, "redelivery is not a second debit");
}

#[test]
fn full_width_amounts_and_overlapping_accounts_preserve_every_unit() {
    let alice = SigningKeyV2::from_dev_id("max-alice");
    let bob = SigningKeyV2::from_dev_id("max-bob");
    let schedule = EmissionSchedule::new(u128::MAX, u128::MAX, 10).unwrap();
    let mut ledger = NativeLedger::new(NETWORK, schedule).unwrap();
    ledger.apply_block(1, &alice.pk_hex(), &[]).unwrap();
    let self_payment = transfer(&alice, &alice.pk_hex(), u128::MAX - 1, 1, 0);
    ledger
        .apply_block(2, &alice.pk_hex(), &[self_payment])
        .unwrap();
    assert_eq!(ledger.balance(&alice.pk_hex()), u128::MAX);

    let payment = transfer(&alice, &bob.pk_hex(), u128::MAX - 1, 1, 1);
    ledger.apply_block(3, &bob.pk_hex(), &[payment]).unwrap();
    assert_eq!(ledger.balance(&alice.pk_hex()), 0);
    assert_eq!(ledger.balance(&bob.pk_hex()), u128::MAX);
    assert_eq!(ledger.next_nonce(&alice.pk_hex()), 2);
    assert_eq!(ledger.issued(), u128::MAX);

    let payment = transfer(&bob, &alice.pk_hex(), u128::MAX - 1, 1, 0);
    ledger.apply_block(4, &bob.pk_hex(), &[payment]).unwrap();
    assert_eq!(ledger.balance(&alice.pk_hex()), u128::MAX - 1);
    assert_eq!(ledger.balance(&bob.pk_hex()), 1);
    assert_eq!(ledger.issued(), u128::MAX);
}

#[test]
fn noncanonical_or_ambiguous_signed_payloads_are_rejected() {
    let owner = SigningKeyV2::from_dev_id("wire-owner");
    let recipient = SigningKeyV2::from_dev_id("wire-recipient").pk_hex();
    let mut ledger =
        NativeLedger::new(NETWORK, EmissionSchedule::new(1_000, 100, 10).unwrap()).unwrap();
    ledger.apply_block(1, &owner.pk_hex(), &[]).unwrap();
    let before = ledger.clone();
    let valid = transfer(&owner, &recipient, 1, 1, 0).payload;
    for (field, value) in [
        ("amount", serde_json::json!(1)),
        ("amount", serde_json::json!("01")),
        ("amount", serde_json::json!("+1")),
        ("amount", serde_json::json!("-1")),
        ("amount", serde_json::json!("1.0")),
        ("amount", serde_json::json!("1e1")),
        (
            "amount",
            serde_json::json!("340282366920938463463374607431768211456"),
        ),
        ("fee", serde_json::json!(" 1")),
        ("nonce", serde_json::json!("00")),
        ("nonce", serde_json::json!("18446744073709551616")),
        ("validBefore", serde_json::json!(100)),
        ("extra", serde_json::json!("ignored?")),
    ] {
        let mut payload = valid.clone();
        payload[field] = value;
        let signed = owner.sign_for_network(&payload, Some(NETWORK)).unwrap();
        assert_eq!(
            ledger.apply_block(2, &recipient, &[signed]),
            Err(NativeLedgerError::InvalidTransfer),
            "{field}"
        );
        assert_eq!(ledger, before);
    }
}

#[test]
fn independent_accounting_replay_rebuilds_balances_and_nonces_from_empty_genesis() {
    let alice = SigningKeyV2::from_dev_id("replay-alice");
    let bob = SigningKeyV2::from_dev_id("replay-bob");
    let producer = SigningKeyV2::from_dev_id("replay-producer").pk_hex();
    let schedule = EmissionSchedule::new(1_000, 100, 10).unwrap();
    let blocks = [
        (alice.pk_hex(), vec![]),
        (
            producer.clone(),
            vec![
                transfer(&alice, &bob.pk_hex(), 40, 3, 0),
                transfer(&bob, &alice.pk_hex(), 10, 1, 0),
            ],
        ),
        (producer, vec![transfer(&alice, &bob.pk_hex(), 20, 2, 1)]),
    ];
    let mut live = NativeLedger::new(NETWORK, schedule).unwrap();
    let mut prefix = None;
    for (index, (recipient, transfers)) in blocks.iter().enumerate() {
        live.apply_block(index as u64 + 1, recipient, transfers)
            .unwrap();
        if index == 0 {
            prefix = Some(live.clone());
        }
    }
    let mut replay = NativeLedger::new(NETWORK, schedule).unwrap();
    for (index, (recipient, transfers)) in blocks.iter().enumerate() {
        replay
            .apply_block(index as u64 + 1, recipient, transfers)
            .unwrap();
    }
    assert_eq!(live, replay);
    assert_eq!(replay.balance(&alice.pk_hex()), 45);
    assert_eq!(replay.balance(&bob.pk_hex()), 49);
    assert_eq!(replay.balance(&blocks[1].0), 206);
    assert_eq!(replay.next_nonce(&alice.pk_hex()), 2);
    assert_eq!(replay.next_nonce(&bob.pk_hex()), 1);
    assert_eq!(replay.issued(), 300);

    // Accounting-only branch replacement, not a node fork-choice/reorg test.
    let mut alternate = prefix.unwrap();
    alternate.apply_block(2, &bob.pk_hex(), &[]).unwrap();
    assert_eq!(alternate.balance(&alice.pk_hex()), 100);
    assert_eq!(alternate.next_nonce(&alice.pk_hex()), 0);
    assert_eq!(alternate.balance(&bob.pk_hex()), 100);
}

#[test]
fn height_checks_and_end_of_block_issuance_prevent_unearned_spending() {
    let miner = SigningKeyV2::from_dev_id("height-miner");
    let recipient = SigningKeyV2::from_dev_id("height-recipient").pk_hex();
    let schedule = EmissionSchedule::new(1_000, 100, 10).unwrap();
    let mut ledger = NativeLedger::new(NETWORK, schedule).unwrap();
    let before = ledger.clone();
    assert_eq!(
        ledger.apply_block(2, &miner.pk_hex(), &[]),
        Err(NativeLedgerError::UnexpectedHeight {
            expected: 1,
            actual: 2
        })
    );
    assert_eq!(
        ledger.apply_block(1, "not-a-public-key", &[]),
        Err(NativeLedgerError::InvalidPublicKey)
    );
    assert_eq!(
        ledger.apply_block(1, &miner.pk_hex(), &[transfer(&miner, &recipient, 1, 1, 0)]),
        Err(NativeLedgerError::InsufficientBalance)
    );
    assert_eq!(ledger, before);
    ledger.apply_block(1, &miner.pk_hex(), &[]).unwrap();
    let committed = ledger.clone();
    assert_eq!(
        ledger.apply_block(1, &miner.pk_hex(), &[]),
        Err(NativeLedgerError::UnexpectedHeight {
            expected: 2,
            actual: 1
        })
    );
    assert_eq!(ledger, committed);
    for network in ["", "network\nother", "network with spaces"] {
        assert_eq!(
            NativeLedger::new(network, schedule),
            Err(NativeLedgerError::InvalidNetwork)
        );
    }
}
