use boole_native_shadow_mac4_relay::{
    decode_hello, decode_ready, encode_hello, encode_ready, validate_ready, GuestReady, HostHello,
    FRAME_BYTES, VSOCK_PORT,
};

fn hello() -> HostHello {
    HostHello::new([0x11; 32], [0x22; 32]).expect("valid bound hello")
}

#[test]
fn valid_fresh_handshake_round_trips_and_binds_the_boot_tuple() {
    let hello = hello();
    let encoded = encode_hello(&hello);
    assert_eq!(encoded.len(), FRAME_BYTES);
    let decoded = decode_hello(&encoded).expect("strict hello");
    assert_eq!(decoded, hello);

    let ready = GuestReady::for_hello(&decoded);
    let ready_bytes = encode_ready(&ready);
    let decoded_ready = decode_ready(&ready_bytes).expect("strict ready");
    validate_ready(&hello, &decoded_ready).expect("matching live response");
    assert_eq!(VSOCK_PORT, 4050);
}

#[test]
fn malformed_or_ambiguous_frames_are_rejected() {
    let encoded = encode_hello(&hello());
    let mut cases = Vec::new();

    let mut bad_magic = encoded.to_vec();
    bad_magic[0] ^= 1;
    cases.push(bad_magic);

    let mut bad_kind = encoded.to_vec();
    bad_kind[8] = 0xff;
    cases.push(bad_kind);

    let mut bad_reserved = encoded.to_vec();
    bad_reserved[9] = 1;
    cases.push(bad_reserved);

    let mut bad_protocol = encoded.to_vec();
    bad_protocol[12] ^= 1;
    cases.push(bad_protocol);

    cases.push(encoded[..encoded.len() - 1].to_vec());
    let mut trailing = encoded.to_vec();
    trailing.push(0);
    cases.push(trailing);

    for case in cases {
        assert!(decode_hello(&case).is_err(), "accepted malformed frame");
    }
}

#[test]
fn zero_nonce_and_zero_boot_binding_are_rejected() {
    assert!(HostHello::new([0; 32], [0x22; 32]).is_err());
    assert!(HostHello::new([0x11; 32], [0; 32]).is_err());
}

#[test]
fn response_from_another_attempt_or_image_is_rejected() {
    let expected = hello();
    let ready = GuestReady::for_hello(&expected);

    let other_attempt = HostHello::new([0x33; 32], [0x22; 32]).unwrap();
    assert!(validate_ready(&other_attempt, &ready).is_err());

    let other_image = HostHello::new([0x11; 32], [0x44; 32]).unwrap();
    assert!(validate_ready(&other_image, &ready).is_err());
}

#[test]
fn wrong_response_kind_and_protocol_are_rejected_before_binding() {
    let ready = GuestReady::for_hello(&hello());
    let encoded = encode_ready(&ready);

    let mut wrong_kind = encoded.to_vec();
    wrong_kind[8] = 1;
    assert!(decode_ready(&wrong_kind).is_err());

    let mut wrong_protocol = encoded.to_vec();
    wrong_protocol[12] ^= 1;
    assert!(decode_ready(&wrong_protocol).is_err());
}
