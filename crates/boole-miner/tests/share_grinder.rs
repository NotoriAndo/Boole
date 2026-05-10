use boole_core::{digest_to_biguint, share_hash, share_score, Hex32};
use boole_miner::{grind_share, CounterNonce, GrindProgress, GrinderConfig};
use num_bigint::BigUint;
use num_traits::One;

fn two_to_256() -> BigUint {
    BigUint::one() << 256usize
}

#[test]
fn test_grind_share_returns_first_j_meeting_floor_with_easy_threshold() {
    let c = Hex32::from_bytes([0xab; 32]);
    let pk = Hex32::from_bytes([0xcd; 32]);
    let n = Hex32::from_bytes([0x01; 32]);
    let canon_hash = Hex32::from_bytes([0u8; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_share(
        &c,
        &pk,
        &n,
        &canon_hash,
        &BigUint::one(),
        None,
        &mut src,
        GrinderConfig {
            max_attempts: Some(1),
            ..Default::default()
        },
        None,
    )
    .expect("share grind should succeed with floor=1 on first attempt");

    assert_eq!(outcome.hashes_attempted, 1);
    assert!(outcome.share_score >= BigUint::one());
    assert!(!outcome.is_proposer);

    let expected = share_hash(&c, &pk, &n, &outcome.j, &canon_hash);
    assert_eq!(outcome.share_hash_bytes, expected);
    assert_eq!(outcome.share_score, share_score(&expected));
}

#[test]
fn test_grind_share_returns_none_when_floor_unreachable_in_budget() {
    let c = Hex32::from_bytes([0xab; 32]);
    let pk = Hex32::from_bytes([0xcd; 32]);
    let n = Hex32::from_bytes([0x01; 32]);
    let canon_hash = Hex32::from_bytes([0u8; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_share(
        &c,
        &pk,
        &n,
        &canon_hash,
        &two_to_256(),
        None,
        &mut src,
        GrinderConfig {
            max_attempts: Some(500),
            ..Default::default()
        },
        None,
    );
    assert!(outcome.is_none());
}

#[test]
fn test_grind_share_flags_proposer_when_share_hash_below_t_block() {
    let c = Hex32::from_bytes([0xab; 32]);
    let pk = Hex32::from_bytes([0xcd; 32]);
    let n = Hex32::from_bytes([0x01; 32]);
    let canon_hash = Hex32::from_bytes([0u8; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_share(
        &c,
        &pk,
        &n,
        &canon_hash,
        &BigUint::one(),
        Some(&two_to_256()),
        &mut src,
        GrinderConfig {
            max_attempts: Some(1),
            ..Default::default()
        },
        None,
    )
    .expect("grind succeeds with easy threshold");

    assert!(outcome.is_proposer);
}

#[test]
fn test_grind_share_does_not_flag_proposer_when_t_block_too_tight() {
    let c = Hex32::from_bytes([0xab; 32]);
    let pk = Hex32::from_bytes([0xcd; 32]);
    let n = Hex32::from_bytes([0x01; 32]);
    let canon_hash = Hex32::from_bytes([0u8; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_share(
        &c,
        &pk,
        &n,
        &canon_hash,
        &BigUint::one(),
        Some(&BigUint::one()),
        &mut src,
        GrinderConfig {
            max_attempts: Some(1),
            ..Default::default()
        },
        None,
    )
    .expect("grind succeeds with easy floor");

    assert!(!outcome.is_proposer);
    assert!(digest_to_biguint(&outcome.share_hash_bytes) >= BigUint::one());
}

#[test]
fn test_grind_share_emits_progress_at_report_every_boundary() {
    let c = Hex32::from_bytes([0xab; 32]);
    let pk = Hex32::from_bytes([0xcd; 32]);
    let n = Hex32::from_bytes([0x01; 32]);
    let canon_hash = Hex32::from_bytes([0u8; 32]);
    let mut src = CounterNonce::new(0);
    let mut count: u32 = 0;
    let mut cb = |_: GrindProgress| {
        count += 1;
    };
    let _ = grind_share(
        &c,
        &pk,
        &n,
        &canon_hash,
        &two_to_256(),
        None,
        &mut src,
        GrinderConfig {
            max_attempts: Some(200),
            report_every_hashes: 50,
        },
        Some(&mut cb),
    );
    assert_eq!(count, 4);
}
