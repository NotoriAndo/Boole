use boole_native_shadow_mac4_relay::{
    decode_hello, decode_proxy_hello, decode_proxy_ready, decode_ready, encode_hello,
    encode_proxy_hello, encode_proxy_ready, encode_ready, relay_launcher_exchange,
    serve_proxy_connection, validate_proxy_ready, validate_ready, GuestProxyReady, GuestReady,
    HostHello, HostProxyHello, ProxyPhase, FRAME_BYTES, PROXY_FRAME_BYTES, PROXY_VSOCK_PORT,
    VSOCK_PORT,
};
use std::io::{self, Cursor, Read, Write};

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
fn authenticated_proxy_ready_binds_phase_boot_and_root_launcher_peer() {
    let hello = HostProxyHello::new([0x33; 32], [0x44; 32], ProxyPhase::Qualification)
        .expect("valid proxy hello");
    let encoded = encode_proxy_hello(&hello);
    assert_eq!(encoded.len(), PROXY_FRAME_BYTES);
    let decoded = decode_proxy_hello(&encoded).expect("strict proxy hello");
    assert_eq!(decoded, hello);

    let ready =
        GuestProxyReady::for_hello(&decoded, 4242, 0, 0).expect("root launcher peer is required");
    let ready_bytes = encode_proxy_ready(&ready);
    let decoded_ready = decode_proxy_ready(&ready_bytes).expect("strict proxy ready");
    validate_proxy_ready(&hello, &decoded_ready).expect("matching authenticated proxy");
    assert_eq!(PROXY_VSOCK_PORT, 4051);
}

fn framed(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

#[test]
fn execution_proxy_forwards_the_exact_four_frame_sequence_and_eof() {
    let hello = framed(b"host execution hello");
    let request = framed(b"host execution request");
    let ready = framed(b"launcher execution ready");
    let report = framed(b"launcher execution report");
    let mut host_input = Cursor::new([hello.clone(), request.clone()].concat());
    let mut host_output = Vec::new();
    let mut launcher_input = Cursor::new([ready.clone(), report.clone()].concat());
    let mut launcher_output = Vec::new();

    relay_launcher_exchange(
        ProxyPhase::Execution,
        &mut host_input,
        &mut host_output,
        &mut launcher_input,
        &mut launcher_output,
    )
    .expect("one exact bounded execution exchange");

    assert_eq!(launcher_output, [hello, request].concat());
    assert_eq!(host_output, [ready, report].concat());
}

struct ScriptedIo {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl ScriptedIo {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input: Cursor::new(input),
            output: Vec::new(),
        }
    }
}

impl Read for ScriptedIo {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for ScriptedIo {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn one_authenticated_proxy_connection_reports_the_kernel_peer_before_forwarding() {
    let proxy_hello = HostProxyHello::new([0x55; 32], [0x66; 32], ProxyPhase::Qualification)
        .expect("proxy hello");
    let qualification_hello = framed(b"qualification hello");
    let qualification_ready = framed(b"qualification ready");
    let mut host = ScriptedIo::new(
        [
            encode_proxy_hello(&proxy_hello).to_vec(),
            qualification_hello.clone(),
        ]
        .concat(),
    );
    let mut launcher = ScriptedIo::new(qualification_ready.clone());

    serve_proxy_connection(&mut host, &mut launcher, 4242, 0, 0)
        .expect("authenticated fixed-socket proxy");

    let expected_ready = GuestProxyReady::for_hello(&proxy_hello, 4242, 0, 0).unwrap();
    assert_eq!(
        host.output,
        [
            encode_proxy_ready(&expected_ready).to_vec(),
            qualification_ready
        ]
        .concat()
    );
    assert_eq!(launcher.output, qualification_hello);
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
