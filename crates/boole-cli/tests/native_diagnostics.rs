use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn diagnose(report: serde_json::Value) -> (Output, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Vec::new();
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("diagnostic fixture accept: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            assert!(header.len() < 8192);
            let mut byte = [0];
            socket.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        let request = String::from_utf8(header).unwrap();
        let line = request.lines().next().unwrap().to_string();
        // This node is not ready. A diagnostic must not query readiness first.
        let (status, body) = if line == "GET /native/diagnostics HTTP/1.1" {
            ("200 OK", serde_json::to_string(&report).unwrap())
        } else {
            (
                "503 Service Unavailable",
                "{\"error\":\"not_ready\"}".into(),
            )
        };
        write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        vec![line]
    });
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "native",
            "--node",
            &format!("http://{address}"),
            "diagnostics",
        ])
        .env_remove("BOOLE_WALLET_PASSPHRASE")
        .env(
            "BOOLE_WALLET_AGENT_BIN",
            "/nonexistent/diagnostics-must-not-unlock",
        )
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (output, server.join().unwrap())
}

#[test]
fn native_diagnostics_is_one_non_authoritative_read_without_a_ready_node_or_wallet() {
    let report = serde_json::json!({
        "schema": "boole.native.diagnostics.v1", "authority": "local_process_only",
        "ledgerReadiness": "not_checked", "stopping": false,
        "rpc": {"activeRequests": 8, "requestLimit": 8, "activeDiagnostics": 1, "diagnosticLimit": 2},
        "peers": {"enabled": false, "running": false, "peers": []}
    });
    let (output, requests) = diagnose(report.clone());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests, ["GET /native/diagnostics HTTP/1.1"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        report
    );
}

#[test]
fn native_diagnostics_rejects_reports_that_claim_ledger_readiness_or_an_unknown_contract() {
    for (schema, authority, readiness) in [
        ("boole.native.diagnostics.v1", "local_process_only", "ready"),
        (
            "boole.native.diagnostics.v1",
            "canonical_ledger",
            "not_checked",
        ),
        ("unrelated.status.v1", "local_process_only", "not_checked"),
    ] {
        let (output, requests) = diagnose(serde_json::json!({
            "schema": schema, "authority": authority, "ledgerReadiness": readiness
        }));
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("invalid non-authoritative native diagnostic report"));
        assert_eq!(requests, ["GET /native/diagnostics HTTP/1.1"]);
    }
}
