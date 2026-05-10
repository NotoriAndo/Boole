use boole_miner::{CounterNonce, NonceSource, OsRngNonce};

#[test]
fn test_counter_nonce_writes_big_endian_into_full_buffer() {
    let mut src = CounterNonce::new(0x0102_0304_u128);
    let mut buf = [0u8; 32];
    src.next_nonce(&mut buf);
    let mut expected = [0u8; 32];
    expected[28] = 0x01;
    expected[29] = 0x02;
    expected[30] = 0x03;
    expected[31] = 0x04;
    assert_eq!(buf, expected);
}

#[test]
fn test_counter_nonce_increments_after_each_call() {
    let mut src = CounterNonce::new(0xff);
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    src.next_nonce(&mut a);
    src.next_nonce(&mut b);
    assert_eq!(a[31], 0xff);
    assert_eq!(b[31], 0x00);
    assert_eq!(b[30], 0x01);
}

#[test]
fn test_os_rng_nonce_fills_buffer_and_is_nonzero_with_overwhelming_probability() {
    let mut src = OsRngNonce;
    let mut buf = [0u8; 32];
    src.next_nonce(&mut buf);
    assert!(
        buf.iter().any(|&b| b != 0),
        "OsRng should fill 32 bytes with high entropy"
    );
}
