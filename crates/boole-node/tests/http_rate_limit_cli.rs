//! N0-pre.4 — the per-IP HTTP rate limiter is reachable from the binary.
//!
//! `HttpRateLimiter` has existed and been unit-tested since P1.7, but the
//! production binary hardcoded `http_rate_limit_per_60s: None` (`main.rs`)
//! and `run-local` exposed no flag to turn it on. An operator therefore had
//! no way to enable the per-IP ceiling without editing source — so
//! "per-IP rate limit" was infrastructure-present but operationally
//! unreachable. This test pins the flag end-to-end: spawning the real
//! binary with `--http-rate-limit-per-60s 2` must make the third same-IP
//! `/status` request within the window return 429.
//!
//! The default (flag absent) stays `None`, so every existing route test is
//! unaffected — that is covered by the untouched in-process suite.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_testkit::rand_suffix;

fn boole_node_bin() -> &'static str {
    env!("CARGO_BIN_EXE_boole-node")
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

/// Spawn `run-local` with the rate-limit flag on an ephemeral port; return
/// the child once it announces the bound port. Bounded so a boot failure
/// fails the test instead of hanging on the stderr read.
fn spawn_with_rate_limit(quota: u32) -> (Child, SocketAddr, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-ratelimit-cli-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let mut child = Command::new(boole_node_bin())
        .args([
            "run-local",
            "--addr",
            "127.0.0.1:0",
            "--scenario",
            scenario_path().to_str().expect("scenario utf8"),
            "--lean-checker-disabled",
            "--allow-insecure-verifier",
            "--http-rate-limit-per-60s",
            &quota.to_string(),
            "--block-store",
            dir.join("blocks.ndjson").to_str().expect("block utf8"),
            "--reward-store",
            dir.join("rewards.ndjson").to_str().expect("reward utf8"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-node run-local");

    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Drain stderr to EOF: dropping the read end early would SIGPIPE the
        // node on its next boot `eprintln!` and kill it before it serves.
        let reader = BufReader::new(stderr);
        let mut sent = false;
        for line in reader.lines().map_while(Result::ok) {
            if !sent && line.contains("listening on") {
                let _ = tx.send(line);
                sent = true;
            }
        }
    });
    let line = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("node should announce listening within 20s");
    let after = line
        .rsplit("http://")
        .next()
        .expect("listen line carries http://");
    let addr: SocketAddr = after.trim().parse().expect("parse bound socket addr");
    (child, addr, dir)
}

/// Fresh-connection GET (`Connection: close`) so keep-alive cannot mask a
/// per-request count. Returns the status line code.
fn http_get_status(addr: SocketAddr, path: &str) -> u16 {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Probe `/live` (exempt from the rate cap) until it serves 200, so boot is
/// finished without spending any of the `/status` quota under test.
fn wait_until_live(addr: SocketAddr) {
    for _ in 0..150 {
        if http_get_status(addr, "/live") == 200 {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("node /live never returned 200 within 15s");
}

#[test]
fn run_local_rate_limit_flag_returns_429_over_limit() {
    let (mut child, addr, dir) = spawn_with_rate_limit(2);
    wait_until_live(addr);

    // Quota is 2 per 60s window: the first two /status requests are admitted,
    // the third must be short-circuited with 429. If the flag never reached
    // `LocalNodeConfig.http_rate_limit_per_60s`, the limiter stays off and the
    // third request quietly returns 200 — the failure this test guards.
    let first = http_get_status(addr, "/status");
    let second = http_get_status(addr, "/status");
    let third = http_get_status(addr, "/status");

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(first, 200, "GET /status #1 must be admitted under quota 2");
    assert_eq!(second, 200, "GET /status #2 must be admitted under quota 2");
    assert_eq!(
        third, 429,
        "GET /status #3 must be rejected 429 once the per-IP quota (2 per 60s) \
         set via --http-rate-limit-per-60s is exhausted; a 200 here means the \
         flag never reached the node config"
    );
}
