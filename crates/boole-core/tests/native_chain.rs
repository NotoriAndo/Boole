use boole_core::native_chain::{NativeBlock, NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::signed_envelope::SigningKeyV2;

fn mine(
    chain: &NativeChain,
    key: &SigningKeyV2,
    reward: &str,
    ts: u64,
    transfers: &[NativeTransfer],
) -> NativeBlock {
    let block = chain
        .template(&key.pk_hex(), reward, ts, transfers)
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .expect("bounded PoW");
    let signature = key
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    block.authorize(&signature).unwrap()
}

#[test]
fn a_mined_owner_authorized_block_replays_independently_under_the_pinned_genesis() {
    let producer = SigningKeyV2::from_dev_id("native-chain-producer");
    let reward = SigningKeyV2::from_dev_id("native-chain-cold-reward").pk_hex();
    let mut first = NativeChain::new().unwrap();
    let template = first
        .template(&producer.pk_hex(), &reward, 60_000, &[])
        .unwrap();
    let mined = template.mine(0, 2_000_000).unwrap().expect("bounded PoW");
    let authorization = producer
        .sign_for_network(
            &mined.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let block = mined.authorize(&authorization).unwrap();

    first.append(block.clone()).unwrap();
    let second = NativeChain::replay(&[block]).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.ledger().issued(), 50_000 * 100_000_000);
    assert_eq!(first.ledger().balance(&reward), first.ledger().issued());
    assert_eq!(first.ledger().balance(&producer.pk_hex()), 0);
    assert_eq!(first.ledger().spendable_balance(&reward), 0);
}

#[test]
fn actual_mined_reward_can_be_transferred_and_replayed_without_creating_extra_supply() {
    let alice = SigningKeyV2::from_dev_id("block-transfer-alice");
    let bob = SigningKeyV2::from_dev_id("block-transfer-bob").pk_hex();
    let miner = SigningKeyV2::from_dev_id("block-transfer-miner");
    let mut chain = NativeChain::new().unwrap();
    for height in 1..=10 {
        let reward = if height == 1 {
            alice.pk_hex()
        } else {
            miner.pk_hex()
        };
        let block = mine(&chain, &miner, &reward, height * 60_000, &[]);
        chain.append(block).unwrap();
    }
    let send = alice
        .sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob,
                "amount": "12300000000", "fee": "1000", "nonce": "0", "validBefore": "11"
            }),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let send = NativeTransfer::try_from(&send).unwrap();
    let block = mine(&chain, &miner, &miner.pk_hex(), 660_000, &[send]);
    let before = chain.clone();
    let mut tampered = block.clone();
    tampered.transfers[0].payload.amount = "12400000000".to_string();
    assert!(chain.append(tampered).is_err());
    assert_eq!(chain, before);
    chain.append(block).unwrap();

    assert_eq!(chain.ledger().balance(&bob), 123 * 100_000_000);
    assert_eq!(chain.ledger().next_nonce(&alice.pk_hex()), 1);
    assert_eq!(chain.ledger().issued(), 11 * 50_000 * 100_000_000);
    assert_eq!(
        chain.ledger().balance(&alice.pk_hex())
            + chain.ledger().balance(&bob)
            + chain.ledger().balance(&miner.pk_hex()),
        chain.ledger().issued()
    );
    assert_eq!(NativeChain::replay(chain.blocks()).unwrap(), chain);
}

#[test]
fn difficulty_retargets_from_committed_timestamps_and_rejects_self_declared_easier_work() {
    let miner = SigningKeyV2::from_dev_id("native-retarget-miner");
    let mut chain = NativeChain::new().unwrap();
    for ts in [60_000, 60_001] {
        chain
            .append(mine(&chain, &miner, &miner.pk_hex(), ts, &[]))
            .unwrap();
    }
    let template = chain
        .template(&miner.pk_hex(), &miner.pk_hex(), 120_000, &[])
        .unwrap();
    assert!(template.header.target.as_str() < native_testnet().initial_target());
    let mut easier = template.clone();
    easier.header.target = native_testnet().initial_target().to_string();
    let block = easier.mine(0, 2_000_000).unwrap().unwrap();
    let signature = miner
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let before = chain.clone();
    assert!(chain.append(block.authorize(&signature).unwrap()).is_err());
    assert_eq!(chain, before);
    chain
        .append(mine(&chain, &miner, &miner.pk_hex(), 120_000, &[]))
        .unwrap();
    assert_eq!(chain, NativeChain::replay(chain.blocks()).unwrap());
}

#[test]
fn timestamp_must_exceed_median_history_even_with_valid_pow_and_producer_signature() {
    let miner = SigningKeyV2::from_dev_id("native-mtp-miner");
    let mut chain = NativeChain::new().unwrap();
    chain
        .append(mine(&chain, &miner, &miner.pk_hex(), 60_000, &[]))
        .unwrap();
    let template = chain.template(&miner.pk_hex(), &miner.pk_hex(), 60_000, &[]);
    assert!(
        template.is_err(),
        "honest templates cannot reuse median time"
    );
    let mut template = chain
        .template(&miner.pk_hex(), &miner.pk_hex(), 120_000, &[])
        .unwrap();
    template.header.timestamp_ms = 60_000;
    let block = template.mine(0, 2_000_000).unwrap().unwrap();
    let auth = miner
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let before = chain.clone();
    assert!(chain.append(block.authorize(&auth).unwrap()).is_err());
    assert_eq!(chain, before);
}

#[test]
fn fork_choice_uses_verified_work_not_length_and_has_a_deterministic_tie_break() {
    let miner = SigningKeyV2::from_dev_id("native-fork-choice-miner");
    let mut harder = NativeChain::new().unwrap();
    for ts in [60_000, 60_001, 120_000] {
        harder
            .append(mine(&harder, &miner, &miner.pk_hex(), ts, &[]))
            .unwrap();
    }
    let mut longer = NativeChain::new().unwrap();
    for ts in [60_000, 120_000, 180_000, 240_000] {
        longer
            .append(mine(&longer, &miner, &miner.pk_hex(), ts, &[]))
            .unwrap();
    }
    assert!(harder.blocks().len() < longer.blocks().len());
    assert!(harder.outranks(&longer));
    assert!(!longer.outranks(&harder));
    let a = NativeChain::replay(&longer.blocks()[..1]).unwrap();
    let other = SigningKeyV2::from_dev_id("native-fork-choice-other");
    let empty = NativeChain::new().unwrap();
    let b = NativeChain::replay(&[mine(&empty, &other, &other.pk_hex(), 60_000, &[])]).unwrap();
    assert_eq!(a.outranks(&b), a.head_hash() < b.head_hash());
    assert_ne!(a.outranks(&b), b.outranks(&a));
    assert!(!a.outranks(&a));
}

#[test]
fn a_recent_verified_prefix_can_be_forked_with_identical_accounting_and_difficulty() {
    let alice = SigningKeyV2::from_dev_id("native-undo-alice");
    let bob = SigningKeyV2::from_dev_id("native-undo-bob");
    let mut chain = NativeChain::new().unwrap();
    for height in 1..=12 {
        let transfers = if height == 11 {
            vec![NativeTransfer::try_from(&alice.sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob.pk_hex(),
                    "amount": "10000", "fee": "1000", "nonce": "0", "validBefore": "20"
                }), Some(native_testnet().network_id()),
            ).unwrap()).unwrap()]
        } else {
            vec![]
        };
        let block = mine(&chain, &alice, &alice.pk_hex(), height * 60_000, &transfers);
        chain.append(block).unwrap();
    }
    let original = chain.clone();
    for height in 0..=12 {
        let mut fork = chain.fork_at(height).unwrap();
        let replayed = NativeChain::replay(&chain.blocks()[..height as usize]).unwrap();
        assert_eq!(fork, replayed, "prefix at {height}");
        assert_eq!(fork.next_target().unwrap(), replayed.next_target().unwrap());
        assert_eq!(
            fork.minimum_timestamp_ms().unwrap(),
            replayed.minimum_timestamp_ms().unwrap()
        );
        for block in &chain.blocks()[height as usize..] {
            fork.append(block.clone()).unwrap();
        }
        assert_eq!(fork, original, "reappend from {height}");
    }
    assert!(chain.fork_at(13).is_err());
    assert_eq!(chain, original, "forking never mutates the source");
}

#[test]
fn recent_forks_restore_zero_entries_dependent_transfers_fees_and_maturing_rewards() {
    let alice = SigningKeyV2::from_dev_id("undo-dependent-alice");
    let bob = SigningKeyV2::from_dev_id("undo-dependent-bob");
    let carol = SigningKeyV2::from_dev_id("undo-dependent-carol");
    let miner = SigningKeyV2::from_dev_id("undo-dependent-miner");
    let mut chain = NativeChain::new().unwrap();
    for height in 1..=10 {
        let reward = if height == 1 {
            alice.pk_hex()
        } else {
            miner.pk_hex()
        };
        chain
            .append(mine(&chain, &miner, &reward, height * 60_000, &[]))
            .unwrap();
    }
    let transfer = |from: &SigningKeyV2, to: &SigningKeyV2, amount: &str, nonce: &str| {
        NativeTransfer::try_from(
            &from
                .sign_for_network(
                    &serde_json::json!({
                        "schema": "boole.transfer.v1", "from": from.pk_hex(), "to": to.pk_hex(),
                        "amount": amount, "fee": "1000", "nonce": nonce, "validBefore": "30"
                    }),
                    Some(native_testnet().network_id()),
                )
                .unwrap(),
        )
        .unwrap()
    };
    let before = chain.clone();
    let transfers = [
        transfer(&alice, &bob, "10000", "0"),
        transfer(&bob, &carol, "9000", "0"),
        transfer(&alice, &alice, "123", "1"),
        transfer(&carol, &carol, "10", "0"),
    ];
    chain
        .append(mine(&chain, &miner, &miner.pk_hex(), 660_000, &transfers))
        .unwrap();
    assert_eq!(chain.ledger().balance(&bob.pk_hex()), 0);
    assert_eq!(chain.ledger().balance_entry_count(), 4);
    assert_eq!(chain.ledger().next_nonce(&alice.pk_hex()), 2);
    assert_eq!(chain.fork_at(10).unwrap(), before);
    let transfers = [transfer(&miner, &miner, "1", "0")];
    chain
        .append(mine(&chain, &miner, &miner.pk_hex(), 720_000, &transfers))
        .unwrap();
    let original = chain.clone();
    for height in [0, 1, 9, 10, 11, 12] {
        let fork = chain.fork_at(height).unwrap();
        assert_eq!(
            fork,
            NativeChain::replay(&chain.blocks()[..height as usize]).unwrap()
        );
    }
    let prepared = chain
        .prepare(mine(&chain, &miner, &miner.pk_hex(), 780_000, &[]))
        .unwrap();
    chain
        .append(mine(&chain, &miner, &alice.pk_hex(), 780_000, &[]))
        .unwrap();
    assert!(
        chain.commit(prepared).is_err(),
        "stale preparation must not add undo state"
    );
    let mut bad = mine(&chain, &miner, &miner.pk_hex(), 840_000, &[]);
    bad.producer_signature = "00".repeat(64);
    assert!(chain.append(bad).is_err());
    assert_eq!(chain.fork_at(12).unwrap(), original);
    assert_eq!(chain.fork_at(10).unwrap(), before);
}

#[test]
fn recent_fork_retention_is_bounded_and_missing_undo_never_fabricates_a_prefix() {
    let miner = SigningKeyV2::from_dev_id("native-undo-retention");
    let mut chain = NativeChain::new().unwrap();
    for height in 1..=257 {
        chain
            .append(mine(&chain, &miner, &miner.pk_hex(), height * 60_000, &[]))
            .unwrap();
    }
    assert!(chain.fork_at(0).is_err());
    let fork = chain.fork_at(1).unwrap();
    let replayed = NativeChain::replay(&chain.blocks()[..1]).unwrap();
    assert_eq!(fork, replayed);
    assert!(
        fork.fork_at(0).is_err(),
        "pruned undo is not recreated from claimed state"
    );
    assert_eq!(replayed.fork_at(0).unwrap(), NativeChain::new().unwrap());
    assert_eq!(
        chain.fork_at(256).unwrap(),
        NativeChain::replay(&chain.blocks()[..256]).unwrap()
    );
}
