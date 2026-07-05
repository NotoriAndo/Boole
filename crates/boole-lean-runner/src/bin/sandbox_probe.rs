//! ADR-0008 test helper: a minimal binary used ONLY by `boole-lean-runner`'s
//! own kernel-isolation guard tests (see `lib.rs`'s `mod tests`). Each
//! invocation performs one concrete "would this be denied by the sandbox"
//! probe and prints a single machine-parseable `RESULT=...` line, so the
//! guard tests can assert on the precise OS errno the isolation mechanism
//! reports instead of parsing ad-hoc shell output.
//!
//! Never invoked by production code paths; only spawned directly by tests
//! via `env!("CARGO_BIN_EXE_sandbox_probe")`.

fn main() {
    let probe = std::env::args().nth(1).unwrap_or_default();
    match probe.as_str() {
        "network-connect" => probe_network_connect(),
        "write" => probe_write(),
        "noop" => println!("RESULT=ALLOWED"),
        other => {
            eprintln!("sandbox_probe: unknown probe {other:?}");
            std::process::exit(2);
        }
    }
}

/// Loopback connect to a port nothing listens on. Under an isolation
/// mechanism that denies network egress this must fail with EPERM/EACCES
/// (the sandbox intercepting the syscall itself); unsandboxed it fails with
/// ECONNREFUSED instead — a different errno, which is exactly the
/// baseline-vs-sandbox distinction the guard tests check for.
fn probe_network_connect() {
    match std::net::TcpStream::connect("127.0.0.1:1") {
        Ok(_) => println!("RESULT=ALLOWED"),
        Err(e) => println!("RESULT=DENIED errno={:?} display={e}", e.raw_os_error()),
    }
}

/// Writes to the path given as the second argument.
fn probe_write() {
    let target = std::env::args()
        .nth(2)
        .expect("probe write requires a target path");
    match std::fs::write(&target, b"sandbox_probe write") {
        Ok(_) => {
            println!("RESULT=ALLOWED");
            let _ = std::fs::remove_file(&target);
        }
        Err(e) => println!("RESULT=DENIED errno={:?} display={e}", e.raw_os_error()),
    }
}
