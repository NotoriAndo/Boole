//! End-to-end tests for the raw-TCP HttpClient. We spin up a tiny
//! single-shot HTTP server in a background thread and assert the request
//! line / headers / body the client emits, plus how it parses the canned
//! response.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use boole_miner::HttpClient;

fn read_request(mut stream: TcpStream) -> (String, Vec<u8>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut headers_end = None;
    loop {
        let n = stream.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            headers_end = Some(pos);
            // For requests with Content-Length, read the rest.
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
            break;
        }
    }
    let split = headers_end.expect("headers end");
    let headers = String::from_utf8(buf[..split].to_vec()).unwrap();
    let body = buf[split + 4..].to_vec();
    (headers, body)
}

fn one_shot_server<F>(handler: F) -> (String, thread::JoinHandle<()>)
where
    F: FnOnce(String, Vec<u8>) -> (u16, Vec<u8>) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{}", addr);
    let h = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (headers, body) = read_request(stream.try_clone().unwrap());
        let (status, resp_body) = handler(headers, body);
        let resp = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            resp_body.len()
        );
        stream.write_all(resp.as_bytes()).unwrap();
        stream.write_all(&resp_body).unwrap();
        stream.flush().unwrap();
    });
    (url, h)
}

#[test]
fn test_post_json_sends_correct_request_and_parses_200() {
    let (url, handle) = one_shot_server(|headers, body| {
        assert!(headers.starts_with("POST /api/echo HTTP/1.1\r\n"));
        assert!(headers.contains("Content-Type: application/json"));
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["x"], 42);
        (200, br#"{"ok":true}"#.to_vec())
    });
    let client = HttpClient::new(url, Duration::from_secs(5));
    let res = client
        .post_json("/api/echo", &serde_json::json!({"x": 42}))
        .unwrap();
    assert_eq!(res.status, 200);
    assert_eq!(res.body, br#"{"ok":true}"#);
    handle.join().unwrap();
}

#[test]
fn test_get_sends_accept_header_and_parses_404() {
    let (url, handle) = one_shot_server(|headers, body| {
        assert!(headers.starts_with("GET /head HTTP/1.1\r\n"));
        assert!(headers.contains("Accept: application/json"));
        assert!(body.is_empty());
        (404, br#"{"error":"missing"}"#.to_vec())
    });
    let client = HttpClient::new(url, Duration::from_secs(5));
    let res = client.get("/head").unwrap();
    assert_eq!(res.status, 404);
    assert_eq!(res.body, br#"{"error":"missing"}"#);
    handle.join().unwrap();
}

#[test]
fn test_base_url_with_path_prefix_is_prepended() {
    let (url, handle) = one_shot_server(|headers, _body| {
        assert!(headers.starts_with("POST /v1/submit HTTP/1.1\r\n"));
        (200, br#"{}"#.to_vec())
    });
    let client = HttpClient::new(format!("{url}/v1/"), Duration::from_secs(5));
    let res = client.post_json("/submit", &serde_json::json!({})).unwrap();
    assert_eq!(res.status, 200);
    handle.join().unwrap();
}
