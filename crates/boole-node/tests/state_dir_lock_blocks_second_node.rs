//! P1.1b RED — cross-process integration test for the state-dir lock.
//!
//! L7 contract: two `boole-node run-local --state-dir <DIR>` invocations
//! must NOT coexist on the same directory. The second invocation must
//! reject before binding a port or opening any ledger, with
//! `state-dir-locked` on stderr.
//!
//! This test will stay RED until the P1.1b GREEN slice wires
//! `--state-dir` through `LocalNodeConfig` and `LocalNodeState::from_config`
//! actually calls `acquire_state_dir`.

use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn boole_node_bin() -> &'static str {
    env!("CARGO_BIN_EXE_boole-node")
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn fresh_state_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-node-p1-1b-{}-{}-{}",
        label,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn drain_stderr_async(child: &mut Child) -> Receiver<String> {
    let stderr = child.stderr.take().expect("stderr piped");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn collect_stderr_lines(rx: &Receiver<String>, deadline: Instant) -> Vec<String> {
    let mut lines = Vec::new();
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => lines.push(line),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    lines
}

fn wait_for_port(port: u16, deadline: Instant) -> bool {
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(100),
        )
        .is_ok()
        {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let port = listener.local_addr().expect("local_addr").port();
    drop(listener);
    port
}

fn kill_and_drain(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
    if let Some(mut out) = child.stdout {
        let mut _s = String::new();
        let _ = out.read_to_string(&mut _s);
    }
}

#[test]
fn second_node_at_same_state_dir_is_rejected_with_typed_error() {
    let state_dir = fresh_state_dir("contend");
    let port_a = pick_free_port();
    let port_b = pick_free_port();
    let block_a = state_dir.join("blocks-a.ndjson");
    let block_b = state_dir.join("blocks-b.ndjson");
    let reward_a = state_dir.join("rewards-a.ndjson");
    let reward_b = state_dir.join("rewards-b.ndjson");

    let mut a = Command::new(boole_node_bin())
        .arg("run-local")
        .arg("--addr")
        .arg(format!("127.0.0.1:{port_a}"))
        .arg("--scenario")
        .arg(scenario_path())
        .arg("--block-store")
        .arg(&block_a)
        .arg("--reward-store")
        .arg(&reward_a)
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--max-requests")
        .arg("9999")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first boole-node");
    let a_stderr = drain_stderr_async(&mut a);

    let listening = wait_for_port(port_a, Instant::now() + Duration::from_secs(20));
    assert!(
        listening,
        "first boole-node did not start listening on 127.0.0.1:{port_a}; \
         stderr so far: {:?}",
        collect_stderr_lines(&a_stderr, Instant::now() + Duration::from_millis(50)),
    );

    let mut b = Command::new(boole_node_bin())
        .arg("run-local")
        .arg("--addr")
        .arg(format!("127.0.0.1:{port_b}"))
        .arg("--scenario")
        .arg(scenario_path())
        .arg("--block-store")
        .arg(&block_b)
        .arg("--reward-store")
        .arg(&reward_b)
        .arg("--state-dir")
        .arg(&state_dir)
        .arg("--max-requests")
        .arg("9999")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second boole-node");
    let b_stderr = drain_stderr_async(&mut b);

    let exit_deadline = Instant::now() + Duration::from_secs(15);
    let mut exit_status = None;
    while Instant::now() < exit_deadline {
        match b.try_wait() {
            Ok(Some(status)) => {
                exit_status = Some(status);
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(err) => panic!("second boole-node try_wait failed: {err}"),
        }
    }
    let lines = collect_stderr_lines(&b_stderr, Instant::now() + Duration::from_millis(200));
    let stderr_blob = lines.join("\n");

    // Stop the first node before assertions so a failure does not leak.
    kill_and_drain(a);

    let status = exit_status.unwrap_or_else(|| {
        let _ = b.kill();
        panic!(
            "second boole-node did not exit within 15s while first held \
             the state directory; stderr: {stderr_blob}"
        );
    });
    assert!(
        !status.success(),
        "second boole-node exited 0 while first held the state directory; \
         expected a non-zero typed exit. stderr: {stderr_blob}"
    );
    assert!(
        stderr_blob.contains("state-dir-locked"),
        "second boole-node stderr missing `state-dir-locked` envelope; got: {stderr_blob}"
    );
    // The losing process must not have created a successor block store.
    assert!(
        !block_b.exists(),
        "second boole-node should not have created its block-store while \
         the lock was held; found {} on disk",
        block_b.display(),
    );

    let _ = std::fs::remove_dir_all(&state_dir);
}
