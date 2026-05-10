use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::Value;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

#[test]
fn local_node_serves_status_and_accepts_submit_into_replayable_block() {
    let tmp = std::env::temp_dir().join(format!("boole-local-node-{}.ndjson", std::process::id()));
    let _ = fs::remove_file(&tmp);

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let scenario_path = repo_root.join("fixtures/protocol/runtime-smoke/v1.json");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = mpsc::channel();
    let block_path = tmp.clone();
    let server_scenario_path = scenario_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("signal ready");
        let result = serve_local_node(
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
                max_requests: Some(4),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        );
        if let Err(err) = &result {
            eprintln!("local node server exited with error: {err:?}");
        }
        result
    });
    rx.recv().expect("server ready");
    std::thread::sleep(std::time::Duration::from_millis(50));

    let status = request_json(addr, "GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(status["ok"], true);
    assert_eq!(status["height"], 0);

    let scenario: Value =
        serde_json::from_str(&fs::read_to_string(&scenario_path).expect("scenario fixture"))
            .expect("scenario json");
    let body = &scenario["steps"][0]["body"];
    let submit = request_json_with_body(addr, "/submit", body);
    assert_eq!(submit["accepted"], true);
    assert_eq!(submit["block"]["height"], 0);
    assert_eq!(submit["replayMatchesRuntime"], true);

    let head = request_json(addr, "GET /head HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(head["height"], 1);
    assert_eq!(head["c"], submit["block"]["c"]);
    assert_eq!(head["T_share"], scenario["cfg"]["T_share"]);

    // pof TicketBody contract: {c, pk, n} only. Submit-shaped bodies that include j/nonceS/etc
    // are rejected with HTTP 400 unexpected_field at the /ticket boundary.
    let ticket_body = serde_json::json!({
        "c": body["c"],
        "pk": body["pk"],
        "n": body["n"],
    });
    let ticket = request_json_with_body(addr, "/ticket", &ticket_body);
    assert_eq!(ticket["ok"], true);
    assert_eq!(ticket["hashHex"].as_str().expect("ticket hash").len(), 64);

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}

#[test]
fn local_node_submit_uses_tcp_peer_ip_not_spoofed_body_ip_for_rate_limit() {
    let tmp = std::env::temp_dir().join(format!(
        "boole-local-node-peer-ip-boundary-{}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&tmp);

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let scenario_path = repo_root.join("fixtures/protocol/runtime-smoke/v1.json");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = mpsc::channel();
    let block_path = tmp.clone();
    let server_scenario_path = scenario_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("signal ready");
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
                max_requests: Some(2),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    std::thread::sleep(std::time::Duration::from_millis(50));

    let scenario: Value =
        serde_json::from_str(&fs::read_to_string(&scenario_path).expect("scenario fixture"))
            .expect("scenario json");

    let first = request_json_with_body(addr, "/submit", &scenario["steps"][0]);
    assert_eq!(first["accepted"], true);

    let mut second = scenario["steps"][1].clone();
    second["body"]["c"] = first["block"]["c"].clone();
    second["ts"] = serde_json::json!(1800000001123u64);
    second["ip"] = serde_json::json!("198.51.100.250");
    let rejected = request_json_with_body(addr, "/submit", &second);
    assert_eq!(rejected["accepted"], false);
    assert!(
        rejected["decision"]
            .as_str()
            .expect("debug decision")
            .contains("IpQuota"),
        "second submit from same TCP peer must hit peer-IP quota, not spoofed body IP: {rejected}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}

#[test]
fn local_node_rejects_oversized_http_body_before_json_parsing() {
    let tmp = std::env::temp_dir().join(format!(
        "boole-local-node-oversized-{}.ndjson",
        std::process::id()
    ));
    let _ = fs::remove_file(&tmp);

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let scenario_path = repo_root.join("fixtures/protocol/runtime-smoke/v1.json");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = mpsc::channel();
    let block_path = tmp.clone();
    let server_scenario_path = scenario_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("signal ready");
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
                max_requests: Some(1),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    std::thread::sleep(std::time::Duration::from_millis(50));

    let oversized_len = 1_048_577;
    let request = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {oversized_len}\r\n\r\n"
    );
    let raw = request_raw(addr, &request);
    assert!(
        raw.starts_with("HTTP/1.1 413"),
        "oversized body should be rejected with 413 before parsing: {raw}"
    );
    let (_, body) = raw.split_once("\r\n\r\n").expect("response body");
    let parsed: Value = serde_json::from_str(body).expect("response json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "body_too_large");
    assert_eq!(parsed["limitBytes"], 1_048_576);
    assert_eq!(parsed["actualBytes"], oversized_len);

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}

fn request_json(addr: std::net::SocketAddr, request: &str) -> Value {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write request");
    read_json_response(stream)
}

fn request_json_with_body(addr: std::net::SocketAddr, path: &str, body: &Value) -> Value {
    let body = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(), body
    );
    request_json(addr, &request)
}

fn read_json_response(mut stream: TcpStream) -> Value {
    let raw = read_raw_response(&mut stream);
    assert!(
        raw.starts_with("HTTP/1.1 200"),
        "unexpected response: {raw}"
    );
    let (_, body) = raw.split_once("\r\n\r\n").expect("response body");
    serde_json::from_str(body).expect("response json")
}

fn request_raw(addr: std::net::SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write request");
    read_raw_response(&mut stream)
}

fn read_raw_response(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !bytes.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    String::from_utf8(bytes).expect("utf8 response")
}
