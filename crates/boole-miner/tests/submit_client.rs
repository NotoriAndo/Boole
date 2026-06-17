use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, SubmitClient, SubmitInputs, SubmitResult,
};

type CapturedRequest = Arc<Mutex<Option<(String, Vec<u8>)>>>;

struct CannedServer {
    url: String,
    last_request: CapturedRequest,
    handle: Option<JoinHandle<()>>,
}

impl CannedServer {
    fn new(status: u16, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}");
        let last = Arc::new(Mutex::new(None));
        let last_clone = Arc::clone(&last);
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header_text = std::str::from_utf8(&buf[..pos]).unwrap();
                    let cl = header_text
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    let total = pos + 4 + cl;
                    while buf.len() < total {
                        let n = stream.read(&mut chunk).unwrap();
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let headers = std::str::from_utf8(&buf[..pos]).unwrap().to_string();
                    let body = buf[pos + 4..].to_vec();
                    *last_clone.lock().unwrap() = Some((headers, body));
                    break;
                }
            }
            let resp = format!(
                "HTTP/1.1 {status} XX\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
        });
        Self {
            url,
            last_request: last,
            handle: Some(handle),
        }
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn captured(&self) -> Option<(String, Vec<u8>)> {
        self.last_request.lock().unwrap().clone()
    }

    fn shutdown(&mut self) {
        if let Some(h) = self.handle.take() {
            h.join().unwrap();
        }
    }
}

impl Drop for CannedServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[test]
fn test_announce_ticket_observed_returns_hash_hex() {
    let srv = CannedServer::new(200, br#"{"hashHex":"deadbeef"}"#.to_vec());
    let client = SubmitClient::new(srv.url());
    let res = client.announce_ticket(AnnounceTicketInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
    });
    assert_eq!(
        res,
        AnnounceTicketResult::Observed {
            hash_hex: "deadbeef".to_string()
        }
    );
    let (headers, body) = srv.captured().expect("server saw request");
    assert!(headers.starts_with("POST /ticket HTTP/1.1\r\n"));
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["c"], "00".repeat(32));
    assert_eq!(json["pk"], "11".repeat(32));
    assert_eq!(json["n"], "22".repeat(32));
}

#[test]
fn test_announce_ticket_replay_on_422_with_replay_reason() {
    let srv = CannedServer::new(422, br#"{"reason":"replay"}"#.to_vec());
    let client = SubmitClient::new(srv.url());
    let res = client.announce_ticket(AnnounceTicketInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
    });
    assert_eq!(res, AnnounceTicketResult::Replay);
}

#[test]
fn test_announce_ticket_rejected_on_400() {
    let srv = CannedServer::new(400, br#"{"error":"shape","reason":"bad_n"}"#.to_vec());
    let client = SubmitClient::new(srv.url());
    let res = client.announce_ticket(AnnounceTicketInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
    });
    assert_eq!(
        res,
        AnnounceTicketResult::Rejected {
            status: 400,
            error: "shape".to_string(),
            reason: Some("bad_n".to_string()),
        }
    );
}

#[test]
fn test_submit_accepted_on_200_returns_share_hash() {
    let srv = CannedServer::new(
        200,
        br#"{"accepted":true,"shareHash":"cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebabe"}"#.to_vec(),
    );
    let client = SubmitClient::new(srv.url());
    let res = client.submit(SubmitInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
        j_hex: "33".repeat(32).as_str(),
        nonce_s_hex: "44".repeat(32).as_str(),
        canon_bytes: b"POFP-bytes",
        seed_hex: "",
    });
    match res {
        SubmitResult::Accepted { share_hash_hex } => {
            assert_eq!(share_hash_hex.len(), 64);
        }
        other => panic!("expected Accepted, got {other:?}"),
    }
    let (_headers, body) = srv.captured().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["bytes"], hex::encode(b"POFP-bytes"));
    assert_eq!(json["nonceS"], "44".repeat(32));
}

#[test]
fn test_submit_rejected_on_200_with_accepted_false_remaps_to_422() {
    let srv = CannedServer::new(
        200,
        br#"{"accepted":false,"error":"validator","decision":"bad_proof"}"#.to_vec(),
    );
    let client = SubmitClient::new(srv.url());
    let res = client.submit(SubmitInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
        j_hex: "33".repeat(32).as_str(),
        nonce_s_hex: "44".repeat(32).as_str(),
        canon_bytes: b"x",
        seed_hex: "",
    });
    assert_eq!(
        res,
        SubmitResult::Rejected {
            status: 422,
            error: "validator".to_string(),
            reason: Some("bad_proof".to_string()),
            field: None,
            detail: None,
        }
    );
}

#[test]
fn test_submit_rate_limited_on_429() {
    let srv = CannedServer::new(429, br#"{"reason":"pk_quota"}"#.to_vec());
    let client = SubmitClient::new(srv.url());
    let res = client.submit(SubmitInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
        j_hex: "33".repeat(32).as_str(),
        nonce_s_hex: "44".repeat(32).as_str(),
        canon_bytes: b"x",
        seed_hex: "",
    });
    assert_eq!(
        res,
        SubmitResult::RateLimited {
            reason: "pk_quota".to_string()
        }
    );
}

#[test]
fn test_submit_rejected_on_400_carries_field_and_detail() {
    let srv = CannedServer::new(
        400,
        br#"{"error":"shape","reason":"bad_hex","field":"c","detail":"odd length"}"#.to_vec(),
    );
    let client = SubmitClient::new(srv.url());
    let res = client.submit(SubmitInputs {
        c_hex: "00".repeat(32).as_str(),
        pk_hex: "11".repeat(32).as_str(),
        n_hex: "22".repeat(32).as_str(),
        j_hex: "33".repeat(32).as_str(),
        nonce_s_hex: "44".repeat(32).as_str(),
        canon_bytes: b"x",
        seed_hex: "",
    });
    assert_eq!(
        res,
        SubmitResult::Rejected {
            status: 400,
            error: "shape".to_string(),
            reason: Some("bad_hex".to_string()),
            field: Some("c".to_string()),
            detail: Some("odd length".to_string()),
        }
    );
}
