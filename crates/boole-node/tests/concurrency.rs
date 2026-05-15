use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const CONCURRENT_SUBMITS: usize = 16;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn submit_request(addr: SocketAddr, body_str: &str) -> (u16, Value) {
    let request = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("set write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout");
    stream.write_all(request.as_bytes()).expect("write request");
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !bytes.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let raw = String::from_utf8(bytes).expect("utf8 response");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status not parseable from response: {raw}"));
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {raw}"));
    let body: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("response body not JSON: err={err} raw={body_text}"));
    (status, body)
}

#[test]
fn concurrent_submits_serialize_through_admission() {
    let tmp = std::env::temp_dir().join(format!(
        "boole-concurrency-{}-{}.ndjson",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_file(&tmp);

    let scenario_path = repo_root().join("fixtures/protocol/runtime-smoke/v1.json");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let block_path = tmp.clone();
    let server_scenario_path = scenario_path.clone();
    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("signal ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: server_scenario_path,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(CONCURRENT_SUBMITS),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let scenario: Value =
        serde_json::from_str(&fs::read_to_string(&scenario_path).expect("scenario fixture"))
            .expect("scenario json");
    let body = &scenario["steps"][0]["body"];
    let body_str = serde_json::to_string(body).expect("body json");

    let mut workers = Vec::with_capacity(CONCURRENT_SUBMITS);
    for _ in 0..CONCURRENT_SUBMITS {
        let body_str = body_str.clone();
        workers.push(thread::spawn(move || submit_request(addr, &body_str)));
    }

    let mut accepted = 0usize;
    let mut rejected = 0usize;
    for worker in workers {
        let (status, body) = worker.join().expect("worker thread joined");
        assert_eq!(status, 200, "submit must return HTTP 200, got body={body}");
        match body.get("accepted").and_then(Value::as_bool) {
            Some(true) => {
                assert_eq!(body.get("ok"), Some(&Value::Bool(true)));
                accepted += 1;
            }
            Some(false) => {
                assert_eq!(body.get("ok"), Some(&Value::Bool(false)));
                let decision = body
                    .get("decision")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("rejected submit missing decision string: {body}"));
                assert!(
                    !decision.is_empty(),
                    "rejected submit must carry a decision tag, got body={body}"
                );
                rejected += 1;
            }
            other => panic!("submit response missing accepted bool: {other:?} body={body}"),
        }
    }

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    assert_eq!(
        accepted, 1,
        "exactly one of {CONCURRENT_SUBMITS} concurrent /submit calls must win admission \
         (accepted={accepted}, rejected={rejected})"
    );
    assert_eq!(
        accepted + rejected,
        CONCURRENT_SUBMITS,
        "every concurrent submit must produce a coherent admission decision"
    );

    let _ = fs::remove_file(&tmp);
}
