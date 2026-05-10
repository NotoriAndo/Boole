use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use boole_miner::{ChainHeadError, HttpChainHeadFetcher};

fn one_shot_get_responder(status: u16, body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    let h = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
        let resp = format!(
            "HTTP/1.1 {status} XX\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(resp.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
        stream.flush().unwrap();
    });
    (url, h)
}

fn valid_head_body() -> &'static [u8] {
    br#"{
        "c": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        "T_ticket": "00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_share": "0000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_block": "00000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_submit": "00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "MinShareScoreMultiplier": 1000000000,
        "M": 32,
        "K_max": 256,
        "L": 4,
        "D_max": 8,
        "provenance": "test"
    }"#
}

#[test]
fn test_fetch_head_parses_chain_head_with_supplied_d_profile() {
    let (url, handle) = one_shot_get_responder(200, valid_head_body().to_vec());
    let f = HttpChainHeadFetcher::with_timeout(
        url,
        Duration::from_secs(5),
        3,
        "v01".to_string(),
        Some(7),
    );
    let head = f.fetch_head().expect("fetch_head");
    assert_eq!(head.d, 3);
    assert_eq!(head.profile, "v01");
    assert_eq!(head.n, Some(7));
    assert_eq!(head.m, 32);
    handle.join().unwrap();
}

#[test]
fn test_fetch_head_min_share_score_uses_t_share_and_multiplier() {
    let (url, handle) = one_shot_get_responder(200, valid_head_body().to_vec());
    let f = HttpChainHeadFetcher::with_timeout(
        url,
        Duration::from_secs(5),
        2,
        "v031".to_string(),
        None,
    );
    let head = f.fetch_head().unwrap();
    // multiplier_nanos = 1e9 means min_share_score == difficulty_weight(T_share).
    let two_to_256: num_bigint::BigUint = num_bigint::BigUint::from(1u8) << 256;
    let expected = &two_to_256 / &head.t_share;
    assert_eq!(head.min_share_score, expected);
    handle.join().unwrap();
}

#[test]
fn test_fetch_head_reports_non_200_status() {
    let (url, handle) = one_shot_get_responder(503, br#"{"error":"down"}"#.to_vec());
    let f = HttpChainHeadFetcher::with_timeout(
        url,
        Duration::from_secs(5),
        1,
        "v01".to_string(),
        None,
    );
    let err = f.fetch_head().unwrap_err();
    assert!(matches!(err, ChainHeadError::Status(503)));
    handle.join().unwrap();
}

#[test]
fn test_fetch_head_rejects_missing_field() {
    // Missing T_ticket.
    let body = br#"{
        "c": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        "T_share": "0000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_block": "00000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_submit": "00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "MinShareScoreMultiplier": 1000000000,
        "M": 32
    }"#
    .to_vec();
    let (url, handle) = one_shot_get_responder(200, body);
    let f = HttpChainHeadFetcher::with_timeout(
        url,
        Duration::from_secs(5),
        1,
        "v01".to_string(),
        None,
    );
    let err = f.fetch_head().unwrap_err();
    assert!(matches!(
        err,
        ChainHeadError::MissingField("T_ticket")
    ));
    handle.join().unwrap();
}

#[test]
fn test_fetch_head_rejects_invalid_c_hex() {
    let body = br#"{
        "c": "not-hex",
        "T_ticket": "01",
        "T_share": "01",
        "T_block": "01",
        "T_submit": "01",
        "MinShareScoreMultiplier": 1000000000,
        "M": 32
    }"#
    .to_vec();
    let (url, handle) = one_shot_get_responder(200, body);
    let f = HttpChainHeadFetcher::with_timeout(
        url,
        Duration::from_secs(5),
        1,
        "v01".to_string(),
        None,
    );
    let err = f.fetch_head().unwrap_err();
    assert!(matches!(err, ChainHeadError::InvalidField { field: "c", .. }));
    handle.join().unwrap();
}
