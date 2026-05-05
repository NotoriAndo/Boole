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
                max_requests: Some(3),
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
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !bytes.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let raw = String::from_utf8(bytes).expect("utf8 response");
    assert!(
        raw.starts_with("HTTP/1.1 200"),
        "unexpected response: {raw}"
    );
    let (_, body) = raw.split_once("\r\n\r\n").expect("response body");
    serde_json::from_str(body).expect("response json")
}
