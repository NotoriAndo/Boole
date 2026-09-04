//! ADR-0008 test helper: a minimal binary used ONLY by `boole-lean-runner`'s
//! own kernel-isolation guard tests (see `lib.rs`'s `mod tests`). Each
//! invocation performs one concrete "would this be denied by the sandbox"
//! probe and prints a single machine-parseable `RESULT=...` line, so the
//! guard tests can assert on the precise OS errno the isolation mechanism
//! reports instead of parsing ad-hoc shell output.
//!
//! Never invoked by production code paths; only spawned directly by tests
//! via `env!("CARGO_BIN_EXE_sandbox_probe")`.

// The Linux truncate probes deliberately call the exact syscalls whose
// Landlock access bit is under test. This test-only binary inherits the same
// narrowly documented libc carve-out as boole-lean-runner's production
// sandbox boundary.
#![allow(unsafe_code)]

fn main() {
    let probe = std::env::args().nth(1).unwrap_or_default();
    match probe.as_str() {
        "network-connect" => probe_network_connect(),
        "write" => probe_write(),
        #[cfg(target_os = "linux")]
        "truncate" => probe_truncate(false),
        #[cfg(target_os = "linux")]
        "open-truncate-readonly" => probe_truncate(true),
        #[cfg(target_os = "linux")]
        "ftruncate-readonly" => probe_ftruncate_readonly(),
        #[cfg(unix)]
        "process-spawn" => probe_process_spawn(),
        "spawn-child" => std::process::exit(0),
        "noop" => println!("RESULT=ALLOWED"),
        other => {
            eprintln!("sandbox_probe: unknown probe {other:?}");
            std::process::exit(2);
        }
    }
}

#[cfg(unix)]
fn probe_process_spawn() {
    let thread_ok = std::thread::spawn(|| 7_u8)
        .join()
        .is_ok_and(|value| value == 7);
    let current_exe = std::env::current_exe().expect("resolve sandbox probe executable");
    match std::process::Command::new(current_exe)
        .arg("spawn-child")
        .status()
    {
        Ok(_) => println!("RESULT=PROCESS_ALLOWED thread_ok={thread_ok}"),
        Err(error) => println!(
            "RESULT=PROCESS_DENIED errno={:?} thread_ok={thread_ok}",
            error.raw_os_error()
        ),
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

#[cfg(target_os = "linux")]
fn probe_truncate(via_readonly_open: bool) {
    use std::os::unix::ffi::OsStrExt;

    let target = std::env::args_os()
        .nth(2)
        .expect("truncate probe requires a target path");
    let target = std::ffi::CString::new(std::path::Path::new(&target).as_os_str().as_bytes())
        .expect("truncate probe target contains NUL");
    let rc = if via_readonly_open {
        let fd = unsafe {
            libc::open(
                target.as_ptr(),
                libc::O_RDONLY | libc::O_TRUNC | libc::O_CLOEXEC,
            )
        };
        if fd >= 0 {
            unsafe { libc::close(fd) };
            0
        } else {
            -1
        }
    } else {
        unsafe { libc::truncate(target.as_ptr(), 0) }
    };
    if rc == 0 {
        println!("RESULT=ALLOWED");
    } else {
        let error = std::io::Error::last_os_error();
        println!(
            "RESULT=DENIED errno={:?} display={error}",
            error.raw_os_error()
        );
    }
}

#[cfg(target_os = "linux")]
fn probe_ftruncate_readonly() {
    use std::os::unix::ffi::OsStrExt;

    let target = std::env::args_os()
        .nth(2)
        .expect("ftruncate probe requires a target path");
    let target = std::ffi::CString::new(std::path::Path::new(&target).as_os_str().as_bytes())
        .expect("ftruncate probe target contains NUL");
    // This is the verifier's real child shape: the sandbox is already active
    // before submitted code can open anything, and the parent passes no
    // writable checker-package descriptor through exec. Landlock cannot
    // retroactively restrict a writable descriptor opened before the ruleset,
    // so this probe deliberately does not imply that stronger guarantee.
    let fd = unsafe { libc::open(target.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        println!(
            "RESULT=OPEN_DENIED errno={:?} display={error}",
            error.raw_os_error()
        );
        return;
    }
    let rc = unsafe { libc::ftruncate(fd, 0) };
    let error = (rc != 0).then(std::io::Error::last_os_error);
    unsafe { libc::close(fd) };
    if rc == 0 {
        println!("RESULT=ALLOWED");
    } else {
        let error = error.expect("failed ftruncate records errno");
        println!(
            "RESULT=DENIED errno={:?} display={error}",
            error.raw_os_error()
        );
    }
}
