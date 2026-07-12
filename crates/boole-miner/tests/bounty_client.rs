use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use boole_core::{
    canonical_payload_hash_hex, verify_signature, verify_signature_with_network, SigningKeyV2,
};
use boole_miner::{BountyClient, BountyProofInputs, BountyProofResult, KeySigner};

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
}

impl Drop for CannedServer {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn test_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("bounty-client-test")
}

#[test]
fn test_submit_proof_ok_on_200_returns_accepted_duplicate_bounty() {
    let key = test_key();
    let srv = CannedServer::new(
        200,
        br#"{"accepted":true,"duplicate":false,"bounty":{"id":"abc","status":"open"}}"#.to_vec(),
    );
    let client = BountyClient::new(srv.url());
    let res = client.submit_proof(BountyProofInputs {
        bounty_id: "abc",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({"proof": "x"}),
        network_id: "boole-testnet",
    });
    match res {
        BountyProofResult::Ok {
            accepted,
            duplicate,
            proof_hash,
            envelope_hash,
            bounty,
        } => {
            assert!(accepted);
            assert!(!duplicate);
            // Canned 200 body carries neither identity field — both stay None.
            assert_eq!(proof_hash, None);
            assert_eq!(envelope_hash, None);
            assert_eq!(bounty["id"], "abc");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
    let (headers, body) = srv.captured().unwrap();
    assert!(headers.starts_with("POST /bounties/abc/proof HTTP/1.1\r\n"));
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    // boole.signed.v1 outer envelope.
    assert_eq!(json["schema"], "boole.signed.v1");
    assert_eq!(json["pk"], key.pk_hex());
    let signature = json["signature"].as_str().expect("signature str");
    let payload = &json["payload"];
    // Outer signature must verify against the inner payload.
    assert!(verify_signature(&key.pk_hex(), signature, payload).is_ok());
    // Inner payload shape.
    assert_eq!(payload["schema"], "boole.bounty.proof.v1");
    assert_eq!(payload["bountyId"], "abc");
    assert_eq!(payload["prover"], key.pk_hex());
    // §SC W1.b — proofHash is derived from the envelope's canonical JSON,
    // the same computation the node re-runs before accepting the proof.
    let expected = canonical_payload_hash_hex(&serde_json::json!({"proof": "x"}));
    assert_eq!(payload["proofHash"], expected);
    assert_eq!(payload["envelope"]["proof"], "x");
}

#[test]
fn test_submit_proof_not_found_on_404() {
    let srv = CannedServer::new(404, br#"{"error":"missing"}"#.to_vec());
    let client = BountyClient::new(srv.url());
    let res = client.submit_proof(BountyProofInputs {
        bounty_id: "missing-id",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({}),
        network_id: "boole-testnet",
    });
    assert_eq!(
        res,
        BountyProofResult::NotFound {
            id: "missing-id".to_string()
        }
    );
}

#[test]
fn test_submit_proof_terminal_on_409() {
    let srv = CannedServer::new(409, br#"{"status":"solved"}"#.to_vec());
    let client = BountyClient::new(srv.url());
    let res = client.submit_proof(BountyProofInputs {
        bounty_id: "abc",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({}),
        network_id: "boole-testnet",
    });
    assert_eq!(
        res,
        BountyProofResult::Terminal {
            status: "solved".to_string()
        }
    );
}

#[test]
fn test_submit_proof_no_verifier_on_501() {
    let srv = CannedServer::new(501, br#"{"kind":"lean-checker"}"#.to_vec());
    let client = BountyClient::new(srv.url());
    let res = client.submit_proof(BountyProofInputs {
        bounty_id: "abc",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({}),
        network_id: "boole-testnet",
    });
    assert_eq!(
        res,
        BountyProofResult::NoVerifier {
            verifier_kind: "lean-checker".to_string()
        }
    );
}

#[test]
fn test_submit_proof_bad_request_on_400() {
    let srv = CannedServer::new(
        400,
        br#"{"error":"shape","detail":"prover not hex32"}"#.to_vec(),
    );
    let client = BountyClient::new(srv.url());
    let res = client.submit_proof(BountyProofInputs {
        bounty_id: "abc",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({}),
        network_id: "boole-testnet",
    });
    assert_eq!(
        res,
        BountyProofResult::BadRequest {
            error: "shape".to_string(),
            detail: Some("prover not hex32".to_string()),
        }
    );
}

#[test]
fn test_submit_proof_url_percent_encodes_bounty_id() {
    let srv = CannedServer::new(200, br#"{"accepted":true}"#.to_vec());
    let client = BountyClient::new(srv.url());
    let _ = client.submit_proof(BountyProofInputs {
        bounty_id: "weird id/with:slash",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({}),
        network_id: "boole-testnet",
    });
    let (headers, _body) = srv.captured().unwrap();
    assert!(
        headers.contains("POST /bounties/weird%20id%2Fwith%3Aslash/proof HTTP/1.1\r\n"),
        "actual: {headers}"
    );
}

#[test]
fn test_submit_proof_stamps_wire_network_id_and_uses_network_bound_digest() {
    let key = test_key();
    let srv = CannedServer::new(200, br#"{"accepted":true}"#.to_vec());
    let client = BountyClient::new(srv.url());
    let _ = client.submit_proof(BountyProofInputs {
        bounty_id: "abc",
        signer: &KeySigner::new(test_key()),
        envelope: serde_json::json!({"proof": "x"}),
        network_id: "boole-testnet",
    });
    let (_headers, body) = srv.captured().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["network_id"], "boole-testnet",
        "wire body must include the boole-testnet network_id"
    );
    let signature = json["signature"].as_str().expect("signature str");
    let payload = &json["payload"];
    assert!(
        verify_signature_with_network(&key.pk_hex(), signature, payload, Some("boole-testnet"))
            .is_ok(),
        "signature must verify against boole-testnet-bound digest"
    );
    assert!(
        !verify_signature(&key.pk_hex(), signature, payload).unwrap(),
        "legacy digest must NOT verify a network-bound signature"
    );
}
