use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use boole_mcp::{NATIVE_VERIFIER_RESPONSE_MAX_BYTES, NATIVE_VERIFIER_TIMEOUT_SECS};

const _: () = assert!(NATIVE_VERIFIER_TIMEOUT_SECS > 115);

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    body: Value,
}

#[derive(Debug, Clone)]
struct RawResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    chunked: bool,
}

impl RawResponse {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string().into_bytes(),
            chunked: false,
        }
    }
}

type NativeHandler = dyn Fn(&CapturedRequest, usize) -> Option<RawResponse> + Send + Sync + 'static;

struct NativeUpstream {
    addr: SocketAddr,
    shutdown: Arc<Mutex<bool>>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    connections: Arc<AtomicUsize>,
}

impl NativeUpstream {
    fn start(response: Value) -> Self {
        Self::start_with(move |_, _| Some(RawResponse::json(200, response.clone())))
    }

    fn start_with(
        handler: impl Fn(&CapturedRequest, usize) -> Option<RawResponse> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind native upstream");
        let addr = listener.local_addr().expect("native address");
        let shutdown = Arc::new(Mutex::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let handler: Arc<NativeHandler> = Arc::new(handler);
        let shutdown_thread = Arc::clone(&shutdown);
        let requests_thread = Arc::clone(&requests);
        let connections_thread = Arc::clone(&connections);
        thread::spawn(move || {
            for stream in listener.incoming() {
                if *shutdown_thread.lock().expect("shutdown mutex") {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let sequence = connections_thread.fetch_add(1, Ordering::SeqCst) + 1;
                let handler = Arc::clone(&handler);
                let requests = Arc::clone(&requests_thread);
                thread::spawn(move || {
                    handle_native_connection(stream, sequence, handler, requests)
                });
            }
        });
        Self {
            addr,
            shutdown,
            requests,
            connections,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().expect("requests mutex").clone()
    }

    fn connection_count(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl Drop for NativeUpstream {
    fn drop(&mut self) {
        *self.shutdown.lock().expect("shutdown mutex") = true;
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(100));
    }
}

fn handle_native_connection(
    mut stream: TcpStream,
    sequence: usize,
    handler: Arc<NativeHandler>,
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("native read timeout");
    let mut reader = BufReader::new(stream.try_clone().expect("clone native stream"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            return;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().expect("content length");
        }
    }
    let mut body = vec![0_u8; content_length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let mut words = request_line.split_whitespace();
    let method = words.next().unwrap_or("").to_string();
    let path = words.next().unwrap_or("").to_string();
    let body: Value = serde_json::from_slice(&body).expect("native request JSON");
    let request = CapturedRequest { method, path, body };
    requests
        .lock()
        .expect("requests mutex")
        .push(request.clone());

    let Some(response) = handler(&request, sequence) else {
        return;
    };
    let mut head = format!("HTTP/1.1 {} Test\r\n", response.status);
    for (name, value) in response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if response.chunked {
        head.push_str("Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
    } else {
        head.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            response.body.len()
        ));
    }
    let _ = stream.write_all(head.as_bytes());
    if response.chunked {
        let _ = write!(stream, "{:X}\r\n", response.body.len());
        let _ = stream.write_all(&response.body);
        let _ = stream.write_all(b"\r\n0\r\n\r\n");
    } else {
        let _ = stream.write_all(&response.body);
    }
    let _ = stream.flush();
}

fn spawn_serve(native_url: &str) -> (ChildGuard, SocketAddr) {
    spawn_serve_config("http://127.0.0.1:9", native_url, None)
}

fn spawn_serve_with_proxy(native_url: &str, proxy_url: Option<&str>) -> (ChildGuard, SocketAddr) {
    spawn_serve_config("http://127.0.0.1:9", native_url, proxy_url)
}

fn spawn_serve_config(
    node_url: &str,
    native_url: &str,
    proxy_url: Option<&str>,
) -> (ChildGuard, SocketAddr) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_boole-mcp"));
    command.args([
        "serve",
        "--node-url",
        node_url,
        "--native-shadow-url",
        native_url,
        "--listen",
        "127.0.0.1:0",
    ]);
    if let Some(proxy_url) = proxy_url {
        command
            .env("HTTP_PROXY", proxy_url)
            .env("HTTPS_PROXY", proxy_url)
            .env("ALL_PROXY", proxy_url)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-mcp serve");
    let stderr = child.stderr.take().expect("stderr");
    let mut first_line = String::new();
    BufReader::new(stderr)
        .read_line(&mut first_line)
        .expect("read listener line");
    let addr: SocketAddr = first_line
        .trim()
        .strip_prefix("boole-mcp listening on http://")
        .unwrap_or_else(|| panic!("unexpected startup: {first_line:?}"))
        .parse()
        .expect("MCP listener address");
    wait_for_tcp(addr);
    (ChildGuard(child), addr)
}

fn assert_native_serve_startup_rejected(listen: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "serve",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            "http://127.0.0.1:8082",
            "--listen",
            listen,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-mcp serve with rejected listener");
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll rejected serve") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native-enabled MCP unexpectedly served on {listen}");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .expect("rejected serve stderr")
        .read_to_string(&mut stderr)
        .expect("read rejected serve stderr");
    assert!(!status.success(), "native-enabled {listen} must fail");
    assert!(
        stderr.contains("--listen") && stderr.contains("numeric loopback"),
        "listen={listen} stderr={stderr}"
    );
}

fn wait_for_tcp(addr: SocketAddr) {
    let start = Instant::now();
    while TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "MCP did not start"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

fn invoke(addr: SocketAddr, args: Value) -> (u16, Value) {
    let body = json!({"tool": "boole.verify_native", "args": args}).to_string();
    invoke_raw(addr, &body)
}

fn invoke_raw(addr: SocketAddr, body: &str) -> (u16, Value) {
    let request = format!(
        "POST /mcp/invoke HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(3)).expect("connect MCP");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("MCP read timeout");
    stream
        .write_all(request.as_bytes())
        .expect("write MCP request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read MCP response");
    let raw = String::from_utf8(raw).expect("MCP response UTF-8");
    let status = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|word| word.parse().ok())
        .expect("HTTP status");
    let body = raw.split_once("\r\n\r\n").expect("HTTP body").1;
    (
        status,
        serde_json::from_str(body).expect("MCP response JSON"),
    )
}

fn write_mcp_frame(writer: &mut impl Write, value: &Value) {
    let body = value.to_string();
    write_mcp_frame_raw(writer, &body);
}

fn write_mcp_frame_raw(writer: &mut impl Write, body: &str) {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len()).expect("write MCP header");
    writer
        .write_all(body.as_bytes())
        .expect("write MCP frame body");
    writer.flush().expect("flush MCP frame");
}

fn read_mcp_frame(reader: &mut impl BufRead) -> Value {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read MCP header");
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>().expect("MCP length"));
        }
    }
    let mut body = vec![0; content_length.expect("Content-Length")];
    reader.read_exact(&mut body).expect("read MCP frame body");
    serde_json::from_slice(&body).expect("MCP frame JSON")
}

fn submission(raw_answer: &str) -> Value {
    json!({
        "schema": "boole.native-shadow.submission.v1",
        "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
        "templateId": "a".repeat(64),
        "challengeSha256": "b".repeat(64),
        "epoch": 7,
        "rawAnswer": raw_answer,
    })
}

fn submission_with_duplicate_field(field: &str, first_value: &str) -> String {
    let submission = submission("answer").to_string();
    let needle = format!("\"{field}\":");
    let replacement = format!("\"{field}\":{first_value},\"{field}\":");
    let duplicated = submission.replacen(&needle, &replacement, 1);
    assert_ne!(duplicated, submission, "fixture field must exist: {field}");
    duplicated
}

fn accepted_response() -> Value {
    json!({
        "schema": "boole.native-shadow.adjudication.v1",
        "outcome": "accepted",
        "reasonCode": "accepted",
        "redelivered": false,
        "evidenceDigest": "c".repeat(64),
        "receipt": {
            "taskId": "1".repeat(64),
            "submissionId": "2".repeat(64),
            "artifactRoot": "3".repeat(64),
            "checkerHash": "4".repeat(64),
            "verdict": "accepted",
            "rejectReason": null
        }
    })
}

fn deterministic_reject_response() -> Value {
    json!({
        "schema": "boole.native-shadow.adjudication.v1",
        "outcome": "deterministic_reject",
        "reasonCode": "compile_or_hidden_test_failed",
        "redelivered": false,
        "evidenceDigest": "d".repeat(64),
        "receipt": {
            "taskId": "5".repeat(64),
            "submissionId": "6".repeat(64),
            "artifactRoot": "7".repeat(64),
            "checkerHash": "8".repeat(64),
            "verdict": "rejected",
            "rejectReason": "compile-or-hidden-test-failed"
        }
    })
}

#[test]
fn http_and_stdio_forward_exact_six_fields_and_preserve_the_adjudication_body() {
    let expected_response = accepted_response();
    let expected_reject = deterministic_reject_response();
    let expected_client_error = json!({
        "error": "native-intake-rejected",
        "reasonCode": "challenge_unknown"
    });
    let accepted_for_upstream = expected_response.clone();
    let reject_for_upstream = expected_reject.clone();
    let client_error_for_upstream = expected_client_error.clone();
    let upstream = NativeUpstream::start_with(move |_, sequence| {
        Some(match sequence {
            1 | 2 => RawResponse::json(200, accepted_for_upstream.clone()),
            3 => RawResponse::json(200, reject_for_upstream.clone()),
            4 => RawResponse::json(422, client_error_for_upstream.clone()),
            _ => panic!("unexpected native request sequence {sequence}"),
        })
    });
    let (_mcp, mcp_addr) = spawn_serve(&upstream.url());
    let expected_request = submission("```rust\nfn answer() {}\n```");

    let (status, response) = invoke(mcp_addr, expected_request.clone());

    assert_eq!(status, 200, "response={response}");
    assert_eq!(response, expected_response);
    let requests = upstream.requests();
    assert_eq!(requests.len(), 1, "requests={requests:?}");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/native-shadow/submissions");
    assert_eq!(requests[0].body, expected_request);

    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdio stdout"));
    let _guard = ChildGuard(child);
    write_mcp_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {
                "name": "boole.verify_native",
                "arguments": submission("stdio-answer"),
            }
        }),
    );

    let response = read_mcp_frame(&mut stdout);

    assert_eq!(response["id"], 11);
    assert_eq!(response["result"]["isError"], false);
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("native response text");
    let adjudication: Value = serde_json::from_str(text).expect("native adjudication JSON");
    assert_eq!(adjudication, expected_response);
    let requests = upstream.requests();
    assert_eq!(requests.len(), 2, "requests={requests:?}");
    assert_eq!(requests[1].method, "POST");
    assert_eq!(requests[1].path, "/native-shadow/submissions");
    assert_eq!(requests[1].body, submission("stdio-answer"));

    write_mcp_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "tools/call",
            "params": {
                "name": "boole.verify_native",
                "arguments": submission("stdio-deterministic-reject"),
            }
        }),
    );
    let reject = read_mcp_frame(&mut stdout);
    assert_eq!(reject["result"]["isError"], false);
    let reject_text = reject["result"]["content"][0]["text"]
        .as_str()
        .expect("deterministic reject text");
    assert_eq!(
        serde_json::from_str::<Value>(reject_text).expect("deterministic reject JSON"),
        expected_reject,
        "a native 200 deterministic verdict is a successful MCP call, not a transport error"
    );

    write_mcp_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "tools/call",
            "params": {
                "name": "boole.verify_native",
                "arguments": submission("stdio-native-4xx"),
            }
        }),
    );
    let client_error = read_mcp_frame(&mut stdout);
    assert_eq!(client_error["result"]["isError"], true);
    let client_error_text = client_error["result"]["content"][0]["text"]
        .as_str()
        .expect("native 4xx text");
    assert_eq!(
        serde_json::from_str::<Value>(client_error_text).expect("native 4xx JSON"),
        expected_client_error,
        "stdio must preserve the native error body without inventing a status field"
    );

    let requests = upstream.requests();
    assert_eq!(requests.len(), 4, "requests={requests:?}");
    assert!(
        requests
            .iter()
            .all(|request| request.method == "POST"
                && request.path == "/native-shadow/submissions"),
        "all stdio verdict classes must use the one native route: {requests:?}"
    );
}

#[test]
fn native_enabled_http_serve_requires_a_numeric_loopback_listener() {
    for listen in ["0.0.0.0:0", "[::]:0", "192.0.2.1:0", "localhost:0"] {
        assert_native_serve_startup_rejected(listen);
    }

    // The restriction belongs to the native bridge. Preserve the historical
    // legacy-only serve contract, including hostname resolution, when no
    // native upstream is configured.
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "serve",
            "--node-url",
            "http://127.0.0.1:9",
            "--listen",
            "localhost:0",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn legacy-only boole-mcp serve");
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = child.try_wait().expect("poll legacy-only serve") {
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .expect("legacy stderr")
                .read_to_string(&mut stderr)
                .expect("read legacy stderr");
            assert!(
                !stderr.contains("--listen must") && !stderr.contains("numeric loopback"),
                "legacy-only serve was incorrectly subjected to the native listener policy: \
                 status={status} stderr={stderr}"
            );
            break;
        }
        if Instant::now() >= deadline {
            // A normally bound server stays alive. The local sandbox may instead
            // refuse hostname-based bind; either result proves the native-only
            // validation did not reject this historical legacy input.
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_transport_is_distinct_loopback_proxyless_redirectless_and_bounded() {
    {
        let output = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
            .args([
                "stdio",
                "--node-url",
                "http://127.0.0.1:8080",
                "--native-shadow-url",
                "http://[::1]:8082",
            ])
            .stdin(Stdio::null())
            .output()
            .expect("run numeric IPv6 loopback native URL");
        assert!(
            output.status.success(),
            "numeric IPv6 loopback must be accepted: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    {
        let native_error = json!({
            "error": "native-intake-rejected",
            "reasonCode": "epoch_invalid"
        });
        let expected_error = native_error.clone();
        let upstream = NativeUpstream::start_with(move |request, sequence| {
            assert_eq!(sequence, 1, "negative epoch must be forwarded exactly once");
            assert_eq!(request.body["epoch"], json!(-1));
            Some(RawResponse::json(400, expected_error.clone()))
        });
        let (_mcp, mcp_addr) = spawn_serve(&upstream.url());
        let mut negative_epoch = submission("answer");
        negative_epoch["epoch"] = json!(-1);

        let (status, response) = invoke(mcp_addr, negative_epoch.clone());

        assert_eq!(status, 400, "response={response}");
        assert_eq!(response, native_error);
        let requests = upstream.requests();
        assert_eq!(requests.len(), 1, "requests={requests:?}");
        assert_eq!(requests[0].body, negative_epoch);
    }

    {
        let upstream = NativeUpstream::start(accepted_response());
        let (_mcp, mcp_addr) = spawn_serve(&upstream.url());
        let mut cases = Vec::new();
        let mut unknown = submission("answer");
        unknown["unexpected"] = json!(true);
        cases.push(unknown);
        let mut missing = submission("answer");
        missing
            .as_object_mut()
            .expect("submission object")
            .remove("rawAnswer");
        cases.push(missing);
        let mut wrong_string_type = submission("answer");
        wrong_string_type["templateId"] = json!(7);
        cases.push(wrong_string_type);
        let mut fractional_epoch = submission("answer");
        fractional_epoch["epoch"] = json!(1.5);
        cases.push(fractional_epoch);

        for args in cases {
            let (status, response) = invoke(mcp_addr, args);
            assert_eq!(status, 400, "response={response}");
            assert_eq!(response["error"], "invalid-native-submission-arguments");
        }
        assert!(
            upstream.requests().is_empty(),
            "invalid arguments must be rejected before the native network boundary"
        );
    }

    for rejected in [
        "http://localhost:8082",
        "http://0.0.0.0:8082",
        "http://192.0.2.1:8082",
        "https://127.0.0.1:8082",
        "http://127.0.0.1:8082/extra",
        "http://user@127.0.0.1:8082",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
            .args([
                "serve",
                "--node-url",
                "http://127.0.0.1:9",
                "--native-shadow-url",
                rejected,
                "--listen",
                "127.0.0.1:0",
            ])
            .output()
            .expect("run rejected native URL");
        assert!(
            !output.status.success(),
            "{rejected} must be rejected before serving"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--native-shadow-url"),
            "rejected={rejected} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    {
        let output = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
            .args([
                "stdio",
                "--node-url",
                "http://127.0.0.1:8080",
                "--native-shadow-url",
                "http://127.0.0.1:8080",
            ])
            .stdin(Stdio::null())
            .output()
            .expect("run aliased origins");
        assert!(!output.status.success(), "legacy/native alias must fail");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("distinct"),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    {
        let redirect_target = NativeUpstream::start(json!({"must": "not be reached"}));
        let location = format!("{}/legacy-or-remote-route", redirect_target.url());
        let redirect = NativeUpstream::start_with(move |_, _| {
            let mut response = RawResponse::json(307, json!({"error": "redirect-refused"}));
            response.headers.push(("Location".into(), location.clone()));
            Some(response)
        });
        let (_mcp, mcp_addr) = spawn_serve(&redirect.url());
        let (status, response) = invoke(mcp_addr, submission("answer"));
        assert_eq!(status, 307, "response={response}");
        assert_eq!(response, json!({"error": "redirect-refused"}));
        assert_eq!(redirect.requests().len(), 1);
        assert!(
            redirect_target.requests().is_empty(),
            "native transport must not follow redirects"
        );
    }

    {
        let expected = accepted_response();
        let upstream = NativeUpstream::start(expected.clone());
        let proxy_trap = NativeUpstream::start(json!({"error": "proxy-used"}));
        let (_mcp, mcp_addr) =
            spawn_serve_with_proxy(&upstream.url(), Some(proxy_trap.url().as_str()));
        let (status, response) = invoke(mcp_addr, submission("answer"));
        assert_eq!(status, 200, "response={response}");
        assert_eq!(response, expected);
        assert_eq!(upstream.requests().len(), 1);
        assert!(
            proxy_trap.requests().is_empty(),
            "native verifier traffic must never use an ambient proxy"
        );
    }

    for chunked in [false, true] {
        let upstream = NativeUpstream::start_with(move |_, _| {
            let mut response = RawResponse::json(
                200,
                json!({"padding": "x".repeat(NATIVE_VERIFIER_RESPONSE_MAX_BYTES)}),
            );
            response.chunked = chunked;
            Some(response)
        });
        let (_mcp, mcp_addr) = spawn_serve(&upstream.url());
        let (status, response) = invoke(mcp_addr, submission("answer"));
        assert_eq!(status, 502, "chunked={chunked} response={response}");
        assert_eq!(response["error"], "native-upstream-outcome-unknown");
        assert_eq!(response["detail"], "native-response-too-large");
        assert_eq!(response["retry"], "resubmit-exact-six-fields");
        assert_eq!(response["maxBytes"], NATIVE_VERIFIER_RESPONSE_MAX_BYTES);
    }

    {
        let upstream = NativeUpstream::start_with(|_, _| {
            Some(RawResponse {
                status: 200,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: b"not-json".to_vec(),
                chunked: false,
            })
        });
        let (_mcp, mcp_addr) = spawn_serve(&upstream.url());
        let (status, response) = invoke(mcp_addr, submission("answer"));
        assert_eq!(status, 502, "response={response}");
        assert_eq!(response["error"], "native-upstream-outcome-unknown");
        assert_eq!(response["detail"], "native-response-invalid-json");
        assert_eq!(response["retry"], "resubmit-exact-six-fields");
        assert!(response.get("outcome").is_none(), "response={response}");
        assert!(response.get("receipt").is_none(), "response={response}");
    }
}

#[test]
fn duplicate_native_argument_keys_are_rejected_before_http_or_stdio_upstream() {
    let upstream = NativeUpstream::start(accepted_response());
    let (_mcp, mcp_addr) = spawn_serve(&upstream.url());

    for arguments in [
        submission_with_duplicate_field("epoch", "8"),
        submission_with_duplicate_field("rawAnswer", "\"first-answer\""),
    ] {
        let body = format!(r#"{{"tool":"boole.verify_native","args":{arguments}}}"#);
        let (status, response) = invoke_raw(mcp_addr, &body);
        assert_eq!(status, 400, "response={response}");
        assert_eq!(response["error"], "invalid-native-submission-arguments");
    }
    assert!(
        upstream.requests().is_empty(),
        "duplicate HTTP arguments must not cross the native network boundary"
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdio stdout"));
    let _guard = ChildGuard(child);

    for (id, arguments) in [
        (21, submission_with_duplicate_field("epoch", "8")),
        (
            22,
            submission_with_duplicate_field("rawAnswer", "\"first-answer\""),
        ),
    ] {
        let frame = format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"boole.verify_native","arguments":{arguments}}}}}"#
        );
        write_mcp_frame_raw(&mut stdin, &frame);
        let response = read_mcp_frame(&mut stdout);
        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["isError"], true);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("duplicate error text");
        let error: Value = serde_json::from_str(text).expect("duplicate error JSON");
        assert_eq!(error["error"], "invalid-native-submission-arguments");
    }
    assert!(
        upstream.requests().is_empty(),
        "duplicate stdio arguments must not cross the native network boundary"
    );
}

#[test]
fn stdio_native_notification_is_ignored_without_upstream_or_response() {
    let upstream = NativeUpstream::start(accepted_response());
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = child.stdout.take().expect("stdio stdout");
    let _guard = ChildGuard(child);

    write_mcp_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "boole.verify_native",
                "arguments": submission("notification-must-not-run"),
            }
        }),
    );
    drop(stdin);

    let mut response = Vec::new();
    stdout
        .read_to_end(&mut response)
        .expect("read notification output through EOF");
    assert!(
        response.is_empty(),
        "a JSON-RPC notification must not receive a response: {}",
        String::from_utf8_lossy(&response)
    );
    assert!(
        upstream.requests().is_empty(),
        "an id-less native tools/call must not consume a native challenge"
    );
}

#[test]
fn stdio_native_call_requires_jsonrpc_v2_and_string_or_number_id() {
    let upstream = NativeUpstream::start(accepted_response());
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdio stdout"));
    let _guard = ChildGuard(child);

    let params = json!({
        "name": "boole.verify_native",
        "arguments": submission("invalid-envelope-must-not-run"),
    });
    for request in [
        json!({"id": 31, "method": "tools/call", "params": params}),
        json!({"jsonrpc": "1.0", "id": 32, "method": "tools/call", "params": params}),
        json!({"jsonrpc": "2.0", "id": null, "method": "tools/call", "params": params}),
        json!({"jsonrpc": "2.0", "id": true, "method": "tools/call", "params": params}),
        json!({"jsonrpc": "2.0", "id": [], "method": "tools/call", "params": params}),
        json!({"jsonrpc": "2.0", "id": {"invalid": "id"}, "method": "tools/call", "params": params}),
    ] {
        write_mcp_frame(&mut stdin, &request);
        let response = read_mcp_frame(&mut stdout);
        assert_eq!(response["error"]["code"], -32600, "request={request}");
        assert!(
            upstream.requests().is_empty(),
            "invalid JSON-RPC envelope crossed the native network boundary: {request}"
        );
    }
    assert!(
        upstream.requests().is_empty(),
        "invalid JSON-RPC envelopes must not cross the native network boundary"
    );

    write_mcp_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": "native-string-id",
            "method": "tools/call",
            "params": {
                "name": "boole.verify_native",
                "arguments": submission("valid-string-id"),
            }
        }),
    );
    let response = read_mcp_frame(&mut stdout);
    assert_eq!(response["id"], "native-string-id");
    assert_eq!(response["result"]["isError"], false);
    assert_eq!(upstream.requests().len(), 1);
}

#[test]
fn stdio_native_call_rejects_duplicate_id_before_upstream() {
    let upstream = NativeUpstream::start(accepted_response());
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdio stdout"));
    let _guard = ChildGuard(child);
    let arguments = submission("duplicate-id-must-not-run");
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":41,"id":42,"method":"tools/call","params":{{"name":"boole.verify_native","arguments":{arguments}}}}}"#
    );

    write_mcp_frame_raw(&mut stdin, &request);
    let response = read_mcp_frame(&mut stdout);

    assert_eq!(response["id"], Value::Null, "response={response}");
    assert_eq!(response["error"]["code"], -32600, "response={response}");
    assert!(
        upstream.requests().is_empty(),
        "duplicate request ids must not consume a native challenge"
    );
}

#[test]
fn stdio_native_call_rejects_duplicate_jsonrpc_before_upstream() {
    let upstream = NativeUpstream::start(accepted_response());
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-mcp"))
        .args([
            "stdio",
            "--node-url",
            "http://127.0.0.1:9",
            "--native-shadow-url",
            upstream.url().as_str(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let mut stdin = child.stdin.take().expect("stdio stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdio stdout"));
    let _guard = ChildGuard(child);
    let arguments = submission("duplicate-jsonrpc-must-not-run");
    let request = format!(
        r#"{{"jsonrpc":"1.0","jsonrpc":"2.0","id":43,"method":"tools/call","params":{{"name":"boole.verify_native","arguments":{arguments}}}}}"#
    );

    write_mcp_frame_raw(&mut stdin, &request);
    let response = read_mcp_frame(&mut stdout);

    assert_eq!(response["id"], Value::Null, "response={response}");
    assert_eq!(response["error"]["code"], -32600, "response={response}");
    assert!(
        upstream.requests().is_empty(),
        "duplicate JSON-RPC versions must not consume a native challenge"
    );
}

#[test]
fn manual_redelivery_transport_survives_mcp_restart_without_automatic_retry() {
    let accepted_committed = Arc::new(AtomicBool::new(false));
    let accepted_fixture_evaluations = Arc::new(AtomicUsize::new(0));
    let rejected_fixture_evaluations = Arc::new(AtomicUsize::new(0));
    let committed_for_handler = Arc::clone(&accepted_committed);
    let accepted_evaluations_for_handler = Arc::clone(&accepted_fixture_evaluations);
    let rejected_evaluations_for_handler = Arc::clone(&rejected_fixture_evaluations);
    let native = NativeUpstream::start_with(move |request, _| {
        let raw_answer = request.body["rawAnswer"]
            .as_str()
            .expect("strict MCP shape already checked");
        if raw_answer == "accepted-answer" {
            if !committed_for_handler.swap(true, Ordering::SeqCst) {
                accepted_evaluations_for_handler.fetch_add(1, Ordering::SeqCst);
                // Model the real native service committing its durable terminal
                // result before the caller loses the connection.
                return None;
            }
            let mut response = accepted_response();
            response["redelivered"] = json!(true);
            return Some(RawResponse::json(200, response));
        }
        rejected_evaluations_for_handler.fetch_add(1, Ordering::SeqCst);
        Some(RawResponse::json(200, deterministic_reject_response()))
    });
    let legacy_node_trap = NativeUpstream::start(json!({"error": "legacy-node-used"}));
    let accepted_submission = submission("accepted-answer");

    {
        let (_first_mcp, mcp_addr) =
            spawn_serve_config(&legacy_node_trap.url(), &native.url(), None);
        let (status, response) = invoke(mcp_addr, accepted_submission.clone());
        assert_eq!(status, 502, "response={response}");
        assert_eq!(response["error"], "native-upstream-outcome-unknown");
        assert_eq!(response["retry"], "resubmit-exact-six-fields");
        assert!(response.get("outcome").is_none(), "response={response}");
        assert!(response.get("receipt").is_none(), "response={response}");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(native.connection_count(), 1, "automatic retry is forbidden");
        assert_eq!(native.requests().len(), 1, "automatic retry is forbidden");
    }

    {
        let (_restarted_mcp, mcp_addr) =
            spawn_serve_config(&legacy_node_trap.url(), &native.url(), None);
        let (status, redelivered) = invoke(mcp_addr, accepted_submission.clone());
        assert_eq!(status, 200, "redelivered={redelivered}");
        assert_eq!(redelivered["outcome"], "accepted");
        assert_eq!(redelivered["redelivered"], true);
        assert_eq!(redelivered["receipt"], accepted_response()["receipt"]);
        assert_eq!(
            redelivered["evidenceDigest"],
            accepted_response()["evidenceDigest"]
        );

        let (status, rejected) = invoke(mcp_addr, submission("tampered-answer"));
        assert_eq!(status, 200, "rejected={rejected}");
        assert_eq!(rejected, deterministic_reject_response());
    }

    assert_eq!(
        accepted_fixture_evaluations.load(Ordering::SeqCst),
        1,
        "the MCP transport must recover the modeled terminal result, not retry it"
    );
    assert_eq!(rejected_fixture_evaluations.load(Ordering::SeqCst), 1);
    assert_eq!(native.requests().len(), 3);
    assert!(
        native
            .requests()
            .iter()
            .all(|request| request.path == "/native-shadow/submissions"),
        "native verification must use only the dedicated submission route"
    );
    assert!(
        legacy_node_trap.requests().is_empty(),
        "boole.verify_native must never touch legacy /receipts or the node URL"
    );
}
