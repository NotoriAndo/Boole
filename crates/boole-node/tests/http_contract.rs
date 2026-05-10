use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::Value;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

const FIXTURE_DIR_REL: &str = "fixtures/protocol/http-contract/v1";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn fixture_path(name: &str) -> PathBuf {
    repo_root().join(FIXTURE_DIR_REL).join(name)
}

fn boot_server(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "boole-http-contract-{}-{}.ndjson",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_file(&tmp);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = mpsc::channel();
    let block_path = tmp.clone();
    let scenario = scenario_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("signal ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    std::thread::sleep(std::time::Duration::from_millis(50));
    (addr, handle, tmp)
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

struct RawResponse {
    status: u16,
    body: Value,
}

fn send_raw(addr: SocketAddr, request: &str) -> RawResponse {
    let mut stream = TcpStream::connect(addr).expect("connect");
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
    let parsed: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("response body not JSON: err={err} raw={body_text}"));
    RawResponse {
        status,
        body: parsed,
    }
}

fn dispatch_fixture_request(addr: SocketAddr, request: &Value) -> RawResponse {
    let method = request["method"].as_str().expect("fixture request.method");
    let path = request["path"].as_str().expect("fixture request.path");
    match method {
        "GET" => {
            let line = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            send_raw(addr, &line)
        }
        "POST" => {
            let body = &request["body"];
            let body_str = serde_json::to_string(body).expect("body json");
            let line = format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body_str.len(),
                body_str
            );
            send_raw(addr, &line)
        }
        other => panic!("fixture has unsupported method: {other}"),
    }
}

fn load_fixture(name: &str) -> Value {
    let path = fixture_path(name);
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read fixture {path:?}: {err}"));
    serde_json::from_str(&raw).expect("fixture json")
}

fn assert_fixture(addr: SocketAddr, fixture_name: &str) {
    let fixture = load_fixture(fixture_name);
    let response = dispatch_fixture_request(addr, &fixture["request"]);

    let expect = &fixture["expect"];
    let expected_status = expect["status"].as_u64().expect("fixture expect.status") as u16;
    assert_eq!(
        response.status, expected_status,
        "fixture {fixture_name}: status mismatch (body={})",
        response.body
    );

    if let Some(equals) =
        expect
            .get("bodyEquals")
            .and_then(|v| if v.is_null() { None } else { Some(v) })
    {
        assert_eq!(
            &response.body, equals,
            "fixture {fixture_name}: bodyEquals mismatch"
        );
    }

    if let Some(contains) = expect.get("bodyContains").and_then(|v| v.as_object()) {
        for (key, value) in contains {
            assert_eq!(
                response.body.get(key),
                Some(value),
                "fixture {fixture_name}: bodyContains[{key}] mismatch (got body={})",
                response.body
            );
        }
    }

    if let Some(types) = expect.get("bodyTypes").and_then(|v| v.as_object()) {
        for (key, type_value) in types {
            let actual = response
                .body
                .get(key)
                .unwrap_or_else(|| panic!("fixture {fixture_name}: bodyTypes[{key}] missing"));
            let type_str = type_value
                .as_str()
                .unwrap_or_else(|| panic!("fixture {fixture_name}: bodyTypes[{key}] not string"));
            match type_str {
                "hex64" => {
                    let actual_str = actual.as_str().unwrap_or_else(|| {
                        panic!("fixture {fixture_name}: bodyTypes[{key}]=hex64 but value not string: {actual}")
                    });
                    assert_eq!(
                        actual_str.len(),
                        64,
                        "fixture {fixture_name}: bodyTypes[{key}]=hex64 length != 64"
                    );
                    assert!(
                        actual_str.bytes().all(|b| b.is_ascii_hexdigit()),
                        "fixture {fixture_name}: bodyTypes[{key}]=hex64 contains non-hex"
                    );
                }
                "bool" => {
                    assert!(
                        actual.is_boolean(),
                        "fixture {fixture_name}: bodyTypes[{key}]=bool but value not bool: {actual}"
                    );
                }
                "uint" => {
                    let n = actual.as_u64().unwrap_or_else(|| {
                        panic!("fixture {fixture_name}: bodyTypes[{key}]=uint but value not u64: {actual}")
                    });
                    let _ = n;
                }
                "string" => {
                    assert!(
                        actual.is_string(),
                        "fixture {fixture_name}: bodyTypes[{key}]=string but value not string: {actual}"
                    );
                }
                "object" => {
                    assert!(
                        actual.is_object(),
                        "fixture {fixture_name}: bodyTypes[{key}]=object but value not object: {actual}"
                    );
                }
                other => panic!("fixture {fixture_name}: unknown bodyType {other}"),
            }
        }
    }
}

fn submit_first_runtime_smoke_step(addr: SocketAddr) {
    let scenario_text = fs::read_to_string(scenario_path()).expect("read scenario fixture");
    let scenario: Value = serde_json::from_str(&scenario_text).expect("scenario json");
    let body = &scenario["steps"][0]["body"];
    let submit_payload = serde_json::json!({"body": body, "canonTag": 0});
    let body_str = serde_json::to_string(&submit_payload).expect("submit body json");
    let line = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    let response = send_raw(addr, &line);
    assert_eq!(response.status, 200, "submit status: {}", response.body);
    assert_eq!(
        response.body["accepted"], true,
        "submit must accept the runtime-smoke fixture step 0 to drive block read tests; got {}",
        response.body
    );
}

#[test]
fn ticket_contract_fixtures_match() {
    let (addr, handle, tmp) = boot_server(4);

    assert_fixture(addr, "ticket-ok.json");
    assert_fixture(addr, "ticket-unexpected-field.json");
    assert_fixture(addr, "ticket-bad-hex.json");
    assert_fixture(addr, "ticket-missing-field.json");

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}

#[test]
fn read_routes_on_empty_chain_match_fixtures() {
    let (addr, handle, tmp) = boot_server(4);

    assert_fixture(addr, "health-ok.json");
    assert_fixture(addr, "block-latest-empty.json");
    assert_fixture(addr, "block-by-height-bad-request.json");
    assert_fixture(addr, "block-by-height-not-found.json");

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}

#[test]
fn block_read_routes_after_first_block_match_fixtures() {
    let (addr, handle, tmp) = boot_server(3);

    submit_first_runtime_smoke_step(addr);
    assert_fixture(addr, "block-latest-ok.json");
    assert_fixture(addr, "block-by-height-ok.json");

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = fs::remove_file(&tmp);
}
