use boole_core::{ticket, Hex32};
use boole_miner::{
    grind_ticket, CounterNonce, GrindProgress, GrinderConfig, NonceSource,
};
use num_bigint::BigUint;
use num_traits::One;

fn two_to_256() -> BigUint {
    BigUint::one() << 256usize
}

fn fill_hex32(byte: u8) -> Hex32 {
    Hex32::from_bytes([byte; 32])
}

#[test]
fn test_grind_ticket_returns_first_valid_nonce_with_easy_threshold() {
    let c = fill_hex32(0xab);
    let pk = fill_hex32(0xcd);
    let mut src = CounterNonce::new(0);
    let outcome = grind_ticket(
        &c,
        &pk,
        &two_to_256(),
        &mut src,
        GrinderConfig {
            max_attempts: Some(1),
            ..Default::default()
        },
        None,
    )
    .expect("grind ticket succeeds with easy threshold");
    assert_eq!(outcome.hashes_attempted, 1);
    let re = ticket(&c, &pk, &outcome.nonce, &two_to_256());
    assert!(re.valid, "returned nonce must satisfy threshold on re-check");
}

#[test]
fn test_grind_ticket_returns_none_when_threshold_unsatisfiable() {
    let c = fill_hex32(0xab);
    let pk = fill_hex32(0xcd);
    let mut src = CounterNonce::new(0);
    let outcome = grind_ticket(
        &c,
        &pk,
        &BigUint::one(),
        &mut src,
        GrinderConfig {
            max_attempts: Some(1000),
            ..Default::default()
        },
        None,
    );
    assert!(outcome.is_none());
}

#[test]
fn test_grind_ticket_emits_progress_at_report_every_boundary() {
    let c = fill_hex32(0xab);
    let pk = fill_hex32(0xcd);
    let mut src = CounterNonce::new(0);
    let mut count: u32 = 0;
    let mut cb = |_: GrindProgress| {
        count += 1;
    };
    let _ = grind_ticket(
        &c,
        &pk,
        &BigUint::one(),
        &mut src,
        GrinderConfig {
            max_attempts: Some(250),
            report_every_hashes: 100,
        },
        Some(&mut cb),
    );
    assert_eq!(count, 2, "progress should fire at attempts=100 and 200");
}

#[test]
fn test_counter_nonce_is_deterministic_across_instances() {
    let mut a = CounterNonce::new(42);
    let mut b = CounterNonce::new(42);
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    for _ in 0..5 {
        a.next_nonce(&mut buf_a);
        b.next_nonce(&mut buf_b);
        assert_eq!(buf_a, buf_b);
    }
}

#[test]
fn test_grind_ticket_at_half_threshold_finds_valid_within_budget() {
    let c = fill_hex32(0xab);
    let pk = fill_hex32(0xcd);
    let t_half = two_to_256() >> 1;
    let mut src = CounterNonce::new(0);
    let outcome = grind_ticket(
        &c,
        &pk,
        &t_half,
        &mut src,
        GrinderConfig {
            max_attempts: Some(100),
            ..Default::default()
        },
        None,
    )
    .expect("grind ticket should find a valid nonce within 100 attempts at half threshold");
    assert!(outcome.hash_int < t_half);
}
