//! P2.7 — bounded graceful shutdown on SIGTERM / SIGINT.
//!
//! `boole-node run-local` installs OS signal handlers that fire the same
//! graceful-drain trigger the external-trigger and `--max-requests` paths
//! use. On SIGTERM/SIGINT the server stops accepting connections, finishes
//! in-flight requests, and exits 0 within a bounded window.
//!
//! The other teardown obligations the master plan lists are already
//! satisfied structurally and need no extra step here: NDJSON ledgers fsync
//! per append, the Lean child is reaped via `ChildKillOnDrop` when an
//! interrupted proof future drops, the state-dir flock releases when the
//! state drops on return, and the `BountySidePool` is rebuilt from the
//! durable bounty-event ledger on the next boot (a pure projection, so no
//! side snapshot can diverge from it).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

/// Spawn `run-local` on an ephemeral port and return the child once it has
/// announced the port it bound. Bounded so a boot failure fails the test
/// instead of hanging on the stderr read.
fn spawn_run_local(tag: &str) -> (Child, SocketAddr, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-shutdown-{tag}-{}-{}",
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
        // Drain stderr all the way to EOF. If we stopped reading after the
        // first line, dropping the pipe's read end would SIGPIPE the node on
        // its next `eprintln!` (it prints several boot lines) and terminate
        // it before it can serve a single request.
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

fn http_get_status(addr: SocketAddr, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(addr).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
}

/// Probe `/live` until it serves 200 — this proves boot finished AND the
/// async runtime (where the signal handlers register) is up, closing the
/// race where a signal arrives before its handler is installed.
fn wait_until_live(addr: SocketAddr) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if http_get_status(addr, "/live") == Some(200) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("node /live never returned 200 within 15s");
}

fn send_signal(pid: u32, sig: &str) {
    let status = Command::new("kill")
        .args([sig, &pid.to_string()])
        .status()
        .expect("run kill");
    assert!(status.success(), "kill {sig} {pid} failed");
}

fn assert_bounded_clean_exit(child: &mut Child, signal_name: &str) {
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "{signal_name}: graceful drain must complete within 15s (took {:?})",
                    start.elapsed()
                );
                assert_eq!(
                    status.code(),
                    Some(0),
                    "{signal_name}: graceful shutdown should exit 0; a `None` code means \
                     the process was terminated by the signal's default action instead of \
                     draining. status={status:?}"
                );
                return;
            }
            None => {
                if start.elapsed() > Duration::from_secs(20) {
                    let _ = child.kill();
                    panic!("{signal_name}: node did not exit within 20s of the signal");
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[test]
fn sigterm_triggers_bounded_graceful_shutdown() {
    let (mut child, addr, dir) = spawn_run_local("term");
    wait_until_live(addr);
    send_signal(child.id(), "-TERM");
    assert_bounded_clean_exit(&mut child, "SIGTERM");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sigint_triggers_bounded_graceful_shutdown() {
    let (mut child, addr, dir) = spawn_run_local("int");
    wait_until_live(addr);
    send_signal(child.id(), "-INT");
    assert_bounded_clean_exit(&mut child, "SIGINT");
    let _ = std::fs::remove_dir_all(&dir);
}
