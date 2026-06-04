//! P1.9 — a `boole-node run-local` that disables Lean verification must
//! refuse to boot unless the operator explicitly opted into the insecure
//! posture with `--allow-insecure-verifier`. The refusal fires before any
//! port is bound or ledger opened, so it needs no fixture and cannot hang.

use std::process::Command;

fn boole_node_bin() -> &'static str {
    env!("CARGO_BIN_EXE_boole-node")
}

#[test]
fn run_local_refuses_lean_checker_disabled_without_opt_in() {
    let output = Command::new(boole_node_bin())
        .args(["run-local", "--lean-checker-disabled"])
        .output()
        .expect("spawn boole-node run-local");

    assert_eq!(
        output.status.code(),
        Some(78),
        "expected exit code 78 (EX_CONFIG); stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("insecure_verifier_config"),
        "stderr must carry the typed insecure-verifier envelope; got: {stderr}"
    );
    assert!(
        stderr.contains("--allow-insecure-verifier"),
        "stderr must tell the operator how to opt in; got: {stderr}"
    );
}
