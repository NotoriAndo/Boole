use boole_core::{submission_pow_ok, Hex32};
use boole_miner::{
    grind_submission_pow, CounterNonce, GrindProgress, GrinderConfig,
};
use num_bigint::BigUint;
use num_traits::One;

fn two_to_256() -> BigUint {
    BigUint::one() << 256usize
}

#[test]
fn test_grind_submission_pow_easy_threshold_returns_first_nonce() {
    let c = Hex32::from_bytes([0x11; 32]);
    let pk = Hex32::from_bytes([0x22; 32]);
    let canon_hash = Hex32::from_bytes([0x33; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_submission_pow(
        &c,
        &pk,
        &canon_hash,
        &two_to_256(),
        &mut src,
        GrinderConfig {
            max_attempts: Some(1),
            ..Default::default()
        },
        None,
    )
    .expect("submit grind should succeed with easy threshold");

    assert_eq!(outcome.hashes_attempted, 1);
    let (ok, _) = submission_pow_ok(&c, &pk, &outcome.nonce_s, &canon_hash, &two_to_256());
    assert!(ok);
}

#[test]
fn test_grind_submission_pow_unsatisfiable_threshold_returns_none() {
    let c = Hex32::from_bytes([0x11; 32]);
    let pk = Hex32::from_bytes([0x22; 32]);
    let canon_hash = Hex32::from_bytes([0x33; 32]);
    let mut src = CounterNonce::new(0);

    let outcome = grind_submission_pow(
        &c,
        &pk,
        &canon_hash,
        &BigUint::one(),
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
fn test_grind_submission_pow_emits_progress_at_report_every() {
    let c = Hex32::from_bytes([0x11; 32]);
    let pk = Hex32::from_bytes([0x22; 32]);
    let canon_hash = Hex32::from_bytes([0x33; 32]);
    let mut src = CounterNonce::new(0);
    let mut count: u32 = 0;
    let mut cb = |_: GrindProgress| {
        count += 1;
    };
    let _ = grind_submission_pow(
        &c,
        &pk,
        &canon_hash,
        &BigUint::one(),
        &mut src,
        GrinderConfig {
            max_attempts: Some(200),
            report_every_hashes: 50,
        },
        Some(&mut cb),
    );
    assert_eq!(count, 4);
}
