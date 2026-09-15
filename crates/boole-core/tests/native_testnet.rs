use boole_core::native_network::native_testnet;
use boole_core::signed_envelope::{SignedEnvelope, SigningKeyV2};

fn transfer(key: &SigningKeyV2, to: &str, amount: u128, fee: u128, nonce: u64) -> SignedEnvelope {
    key.sign_for_network(
        &serde_json::json!({
            "schema": "boole.transfer.v1", "from": key.pk_hex(), "to": to,
            "amount": amount.to_string(), "fee": fee.to_string(),
            "nonce": nonce.to_string(), "validBefore": "1000"
        }),
        Some(native_testnet().network_id()),
    )
    .unwrap()
}

#[test]
fn approved_testnet_issues_fifty_thousand_coins_toward_a_billion_coin_cap() {
    let network = native_testnet();
    let miner = SigningKeyV2::from_dev_id("native-testnet-policy-miner");
    let mut ledger = network.ledger().unwrap();

    ledger.apply_block(1, &miner.pk_hex(), &[]).unwrap();

    assert_eq!(ledger.issued(), 50_000 * 100_000_000);
    assert_eq!(ledger.balance(&miner.pk_hex()), ledger.issued());
    assert_eq!(network.total_supply(), 1_000_000_000 * 100_000_000);
    assert_eq!(network.network_id(), "boole-native-testnet-1");
    assert_eq!(network.decimals(), 8);
}

#[test]
fn testnet_reward_is_spendable_at_creation_height_plus_ten_not_before() {
    let alice = SigningKeyV2::from_dev_id("maturity-alice");
    let bob = SigningKeyV2::from_dev_id("maturity-bob").pk_hex();
    let producer = SigningKeyV2::from_dev_id("maturity-producer").pk_hex();
    let mut ledger = native_testnet().ledger().unwrap();
    ledger.apply_block(1, &alice.pk_hex(), &[]).unwrap();
    let reward = ledger.issued();
    let send = transfer(&alice, &bob, reward - 1000, 1000, 0);
    for height in 2..=10 {
        let before = ledger.clone();
        assert!(ledger
            .apply_block(height, &producer, std::slice::from_ref(&send))
            .is_err());
        assert_eq!(ledger, before, "immature spends must not change any state");
        ledger.apply_block(height, &producer, &[]).unwrap();
    }
    ledger.apply_block(11, &producer, &[send]).unwrap();
    assert_eq!(ledger.balance(&alice.pk_hex()), 0);
    assert_eq!(ledger.balance(&bob), reward - 1000);
    assert_eq!(ledger.balance(&producer), 10 * reward + 1000);
    assert_eq!(ledger.issued(), 11 * reward);
}

#[test]
fn testnet_rejects_below_minimum_fees_without_losing_maturing_rewards() {
    let alice = SigningKeyV2::from_dev_id("fee-alice");
    let bob = SigningKeyV2::from_dev_id("fee-bob").pk_hex();
    let producer = SigningKeyV2::from_dev_id("fee-producer").pk_hex();
    let mut ledger = native_testnet().ledger().unwrap();
    ledger.apply_block(1, &alice.pk_hex(), &[]).unwrap();
    for height in 2..=10 {
        ledger.apply_block(height, &producer, &[]).unwrap();
    }
    let before = ledger.clone();
    for fee in [0, 999] {
        let send = transfer(&alice, &bob, 10, fee, 0);
        assert!(ledger.apply_block(11, &producer, &[send]).is_err());
        assert_eq!(ledger, before);
    }
    ledger
        .apply_block(11, &producer, &[transfer(&alice, &bob, 10, 1000, 0)])
        .unwrap();
    assert_eq!(ledger.balance(&bob), 10);
    assert_eq!(ledger.locked_balance(&alice.pk_hex()), 0);
    assert_eq!(ledger.next_nonce(&alice.pk_hex()), 1);
}

#[test]
fn approved_schedule_halves_at_ten_thousand_and_never_exceeds_the_billion_coin_ceiling() {
    let network = native_testnet();
    let miner = SigningKeyV2::from_dev_id("complete-testnet-emission").pk_hex();
    let mut ledger = network.ledger().unwrap();
    for height in 1..=430_010 {
        let before = ledger.issued();
        ledger.apply_block(height, &miner, &[]).unwrap();
        assert!(ledger.issued() <= network.total_supply());
        match height {
            9_999 => assert_eq!(ledger.issued() - before, 50_000 * 100_000_000),
            10_000 => assert_eq!(ledger.issued() - before, 25_000 * 100_000_000),
            430_000..=430_010 => assert_eq!(ledger.issued(), before),
            _ => {}
        }
    }
    // Height zero has no payout; integer halvings floor in the smallest unit.
    // The billion-coin value is a ceiling, not a promise to mint every unit.
    assert_eq!(ledger.issued(), 99_994_999_999_860_000);
    assert_eq!(ledger.balance(&miner), ledger.issued());
    assert_eq!(ledger.spendable_balance(&miner), ledger.issued());
    let json = serde_json::to_value(&network).unwrap();
    assert_eq!(json["totalSupply"], "100000000000000000");
    assert_eq!(json["initialReward"], "5000000000000");
}
