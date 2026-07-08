//! N3.1 (ADR-0009) — the minimal P2P transport: a `Transport` trait over
//! line-framed JSON/TCP, with the wire contract's caps pinned from the
//! first codec RED (ADR-0009 (c): no "add limits later" drift). Scope is
//! transport + codec only — gossip/sync (N3.2+), encryption, and peer
//! authentication beyond the static allowlist are explicit non-goals.

use std::thread;

use boole_p2p::{
    Frame, FrameError, HeadSummary, TcpTransport, Transport, GET_BLOCKS_RANGE_CAP, MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
};

fn hello(network_id: &str) -> Frame {
    Frame::Hello {
        protocol_version: PROTOCOL_VERSION,
        consensus_rule_version: 1,
        network_id: network_id.to_string(),
        genesis_hash: "00".repeat(32),
        head: HeadSummary {
            height: 7,
            c: "ab".repeat(32),
        },
    }
}

#[test]
fn two_in_process_peers_complete_hello_handshake() {
    let transport = TcpTransport::new();
    let mut listener = transport.bind("127.0.0.1:0").expect("bind ephemeral");
    let addr = transport.local_addr(&listener).expect("listener addr");

    let server = thread::spawn(move || {
        let server_transport = TcpTransport::new();
        let (mut conn, _peer) = server_transport.accept(&mut listener).expect("accept");
        let inbound = server_transport.recv_frame(&mut conn).expect("recv hello");
        assert_eq!(inbound, hello("boole-closed-testnet"));
        server_transport
            .send_frame(&mut conn, &hello("boole-closed-testnet"))
            .expect("reply hello");
    });

    let mut conn = transport.connect(&addr).expect("connect");
    transport
        .send_frame(&mut conn, &hello("boole-closed-testnet"))
        .expect("send hello");
    let reply = transport.recv_frame(&mut conn).expect("recv reply");

    let Frame::Hello {
        protocol_version,
        consensus_rule_version,
        network_id,
        genesis_hash,
        head,
    } = reply
    else {
        panic!("expected Hello reply");
    };
    assert_eq!(protocol_version, PROTOCOL_VERSION);
    assert_eq!(consensus_rule_version, 1);
    assert_eq!(network_id, "boole-closed-testnet");
    assert_eq!(genesis_hash, "00".repeat(32));
    assert_eq!(head.height, 7);
    assert_eq!(head.c, "ab".repeat(32));

    server.join().expect("server thread");
}

#[test]
fn all_five_frame_variants_roundtrip_the_codec() {
    let transport = TcpTransport::new();
    let mut listener = transport.bind("127.0.0.1:0").expect("bind");
    let addr = transport.local_addr(&listener).expect("addr");

    let frames = vec![
        hello("net"),
        Frame::ShareAnnounce {
            submission: serde_json::json!({"c": "11", "pk": "22", "seedHex": "33"}),
        },
        Frame::BlockAnnounce {
            height: 3,
            c: "cd".repeat(32),
        },
        Frame::GetBlocks { from: 0, to: 255 },
        Frame::Blocks {
            blocks: vec![
                serde_json::json!({"height": 0}),
                serde_json::json!({"height": 1}),
            ],
        },
    ];

    let expected = frames.clone();
    let server = thread::spawn(move || {
        let t = TcpTransport::new();
        let (mut conn, _peer) = t.accept(&mut listener).expect("accept");
        for frame in &expected {
            let inbound = t.recv_frame(&mut conn).expect("recv");
            assert_eq!(&inbound, frame);
        }
    });

    let mut conn = transport.connect(&addr).expect("connect");
    for frame in &frames {
        transport.send_frame(&mut conn, frame).expect("send");
    }
    drop(conn);
    server.join().expect("server thread");
}

#[test]
fn wire_contract_caps_are_pinned() {
    // ADR-0009 (c): both caps are part of the wire contract, pinned here so
    // a silent constant edit fails a test, not a peer in production.
    assert_eq!(MAX_FRAME_BYTES, 16 * 1024 * 1024);
    assert_eq!(GET_BLOCKS_RANGE_CAP, 256);
}

#[test]
fn oversize_inbound_line_is_rejected_without_unbounded_buffering() {
    use std::io::Write;
    use std::net::TcpStream;

    let transport = TcpTransport::new();
    let mut listener = transport.bind("127.0.0.1:0").expect("bind");
    let addr = transport.local_addr(&listener).expect("addr");

    // A raw writer (not the codec) streams more than MAX_FRAME_BYTES without
    // ever sending a newline; the reader must fail with FrameTooLarge as
    // soon as the cap is crossed instead of buffering the line forever.
    let writer = thread::spawn(move || {
        let mut raw = TcpStream::connect(addr).expect("raw connect");
        let chunk = vec![b'a'; 64 * 1024];
        let mut sent = 0usize;
        while sent <= MAX_FRAME_BYTES + chunk.len() {
            if raw.write_all(&chunk).is_err() {
                break; // reader dropped the connection at the cap — expected
            }
            sent += chunk.len();
        }
    });

    let (mut conn, _peer) = transport.accept(&mut listener).expect("accept");
    let err = transport
        .recv_frame(&mut conn)
        .expect_err("oversize line must be rejected");
    assert!(
        matches!(err, FrameError::FrameTooLarge { .. }),
        "expected FrameTooLarge, got {err:?}"
    );
    drop(conn);
    writer.join().expect("writer thread");
}

#[test]
fn oversize_outbound_frame_is_rejected_at_send() {
    let transport = TcpTransport::new();
    let listener = transport.bind("127.0.0.1:0").expect("bind");
    let addr = transport.local_addr(&listener).expect("addr");
    let mut conn = transport.connect(&addr).expect("connect");

    let oversize = Frame::Blocks {
        blocks: vec![serde_json::Value::String("b".repeat(MAX_FRAME_BYTES + 1))],
    };
    let err = transport
        .send_frame(&mut conn, &oversize)
        .expect_err("an over-cap outbound frame must be rejected before hitting the wire");
    assert!(
        matches!(err, FrameError::FrameTooLarge { .. }),
        "expected FrameTooLarge, got {err:?}"
    );
}

#[test]
fn malformed_json_line_is_a_typed_decode_error() {
    use std::io::Write;
    use std::net::TcpStream;

    let transport = TcpTransport::new();
    let mut listener = transport.bind("127.0.0.1:0").expect("bind");
    let addr = transport.local_addr(&listener).expect("addr");

    let writer = thread::spawn(move || {
        let mut raw = TcpStream::connect(addr).expect("raw connect");
        raw.write_all(b"this is not a frame\n").expect("write");
    });

    let (mut conn, _peer) = transport.accept(&mut listener).expect("accept");
    let err = transport
        .recv_frame(&mut conn)
        .expect_err("malformed line must be rejected");
    assert!(
        matches!(err, FrameError::Malformed { .. }),
        "expected Malformed, got {err:?}"
    );
    writer.join().expect("writer thread");
}

#[test]
fn get_blocks_range_over_cap_is_rejected_by_validate() {
    // Inclusive range: 256 blocks is the cap (ADR-0009 (c)); 257 rejects.
    let at_cap = Frame::GetBlocks {
        from: 100,
        to: 100 + GET_BLOCKS_RANGE_CAP - 1,
    };
    at_cap.validate().expect("a 256-block request is allowed");

    let over_cap = Frame::GetBlocks {
        from: 100,
        to: 100 + GET_BLOCKS_RANGE_CAP,
    };
    let err = over_cap
        .validate()
        .expect_err("a 257-block request must be rejected");
    assert!(
        matches!(err, FrameError::RangeTooWide { .. }),
        "expected RangeTooWide, got {err:?}"
    );

    let inverted = Frame::GetBlocks { from: 5, to: 4 };
    assert!(
        inverted.validate().is_err(),
        "an inverted range must be rejected"
    );
}

#[test]
fn recv_frame_validates_inbound_get_blocks_range() {
    // The codec itself enforces the range cap on ingress — a peer cannot
    // hand us an over-cap request that our handler then has to remember to
    // check (ADR-0009 (e): malformed/cap violation → typed error).
    let transport = TcpTransport::new();
    let mut listener = transport.bind("127.0.0.1:0").expect("bind");
    let addr = transport.local_addr(&listener).expect("addr");

    let writer = thread::spawn(move || {
        use std::io::Write;
        use std::net::TcpStream;
        let mut raw = TcpStream::connect(addr).expect("raw connect");
        // Hand-encoded over-cap GetBlocks, bypassing send-side validation.
        raw.write_all(br#"{"type":"getBlocks","from":0,"to":1000}"#)
            .expect("write");
        raw.write_all(b"\n").expect("newline");
    });

    let (mut conn, _peer) = transport.accept(&mut listener).expect("accept");
    let err = transport
        .recv_frame(&mut conn)
        .expect_err("inbound over-cap GetBlocks must be rejected");
    assert!(
        matches!(err, FrameError::RangeTooWide { .. }),
        "expected RangeTooWide, got {err:?}"
    );
    writer.join().expect("writer thread");
}
