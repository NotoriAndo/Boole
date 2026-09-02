use std::fs::{self, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_cli::installed_product_lifecycle::{
    materialize_verified_host_node_for_test, plan_installed_mac_lifecycle,
    reset_installed_direct_boot_runtime,
};
use sha2::{Digest, Sha256};
use std::process::Command;

struct FixtureDir(std::path::PathBuf);

impl FixtureDir {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "boole-installed-product-lifecycle-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("fixture root");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("private fixture");
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn one_state_root_derives_every_mutable_path_without_vm_arguments() {
    let fixture = FixtureDir::new("paths");
    let state_root = fixture.0.join("state");
    let plan = plan_installed_mac_lifecycle(&state_root).expect("lifecycle plan");

    assert_eq!(plan.state_root(), state_root);
    assert_eq!(
        plan.controller_runtime_root(),
        state_root.join("controller")
    );
    assert_eq!(
        plan.journal_path(),
        state_root.join("journal/replay.ndjson")
    );
    assert_eq!(
        plan.materialized_host_node_path(),
        state_root.join("host/boole-mac-native-shadow-replay-node")
    );
    assert!(plan_installed_mac_lifecycle(std::path::Path::new("relative-state")).is_err());
}

#[test]
fn verified_host_node_is_copied_from_the_open_handle_and_protected() {
    let fixture = FixtureDir::new("materialize");
    let source_path = fixture.0.join("source-node");
    let original = b"#!/bin/sh\nexit 0\n";
    fs::write(&source_path, original).expect("source");
    let source = fs::File::open(&source_path).expect("open retained source");
    let digest = hex::encode(Sha256::digest(original));
    let plan = plan_installed_mac_lifecycle(&fixture.0.join("state")).expect("plan");

    fs::rename(&source_path, fixture.0.join("retained-original")).expect("retain original inode");
    fs::write(&source_path, b"swapped pathname bytes").expect("replace source pathname");
    let target =
        materialize_verified_host_node_for_test(&source, original.len() as u64, &digest, &plan)
            .expect("materialize retained bytes");

    assert_eq!(fs::read(&target).expect("target bytes"), original);
    assert_eq!(
        fs::metadata(&target)
            .expect("target metadata")
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
}

#[test]
fn materialization_fails_closed_on_digest_drift_without_an_executable() {
    let fixture = FixtureDir::new("digest-drift");
    let source_path = fixture.0.join("source-node");
    fs::write(&source_path, b"node bytes").expect("source");
    let source = fs::File::open(&source_path).expect("open source");
    let plan = plan_installed_mac_lifecycle(&fixture.0.join("state")).expect("plan");

    let error = materialize_verified_host_node_for_test(&source, 10, &"00".repeat(32), &plan)
        .expect_err("wrong digest rejected");
    assert!(error.to_string().contains("digest differs"));
    assert!(!plan.materialized_host_node_path().exists());
}

#[test]
fn materialization_rejects_a_permissive_or_symlinked_state_root() {
    let fixture = FixtureDir::new("unsafe-state");
    let source_path = fixture.0.join("source-node");
    let bytes = b"node bytes";
    fs::write(&source_path, bytes).expect("source");
    let source = fs::File::open(&source_path).expect("source handle");
    let digest = hex::encode(Sha256::digest(bytes));

    let permissive = fixture.0.join("permissive");
    fs::create_dir(&permissive).expect("permissive root");
    fs::set_permissions(&permissive, fs::Permissions::from_mode(0o755)).expect("permissive mode");
    let plan = plan_installed_mac_lifecycle(&permissive).expect("plan");
    assert!(
        materialize_verified_host_node_for_test(&source, bytes.len() as u64, &digest, &plan)
            .is_err()
    );

    let private = fixture.0.join("private");
    fs::create_dir(&private).expect("private root");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("private mode");
    let target = fixture.0.join("symlink-state");
    std::os::unix::fs::symlink(&private, &target).expect("state symlink");
    let plan = plan_installed_mac_lifecycle(&target).expect("plan");
    assert!(
        materialize_verified_host_node_for_test(&source, bytes.len() as u64, &digest, &plan)
            .is_err()
    );
}

#[test]
fn product_cli_exposes_foreground_run_and_status_without_vm_path_flags() {
    let help = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["product", "--help"])
        .output()
        .expect("product help");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("run-direct-boot"), "stdout: {stdout}");
    assert!(stdout.contains("status-direct-boot"), "stdout: {stdout}");
    assert!(stdout.contains("rollback-direct-boot"), "stdout: {stdout}");
    assert!(stdout.contains("recover-direct-boot"), "stdout: {stdout}");
    assert!(stdout.contains("reset-direct-boot"), "stdout: {stdout}");

    let run_help = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["product", "run-direct-boot", "--help"])
        .output()
        .expect("run help");
    assert!(run_help.status.success());
    let run_help = String::from_utf8_lossy(&run_help.stdout);
    for forbidden in [
        "--runtime-root",
        "--journal-path",
        "--kernel",
        "--root-disk",
    ] {
        assert!(
            !run_help.contains(forbidden),
            "run help exposed {forbidden}"
        );
    }
}

#[test]
fn product_run_fails_with_a_typed_envelope_before_any_unverified_execution() {
    let fixture = FixtureDir::new("typed-run-refusal");
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "run-direct-boot",
            "--install-root",
            &fixture.0.join("absent-install").display().to_string(),
            "--state-root",
            &fixture.0.join("state").display().to_string(),
            "--product-trust-root-key-id",
            "product-root",
            "--product-trust-root-public-key",
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
            "--guest-trust-root-key-id",
            "guest-root",
            "--guest-trust-root-public-key",
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        ])
        .output()
        .expect("run command");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("\"command\":\"product.run-direct-boot\""),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("\"reason\":\"lifecycle-rejected\""),
        "stderr: {stderr}"
    );
    assert!(!fixture
        .0
        .join("state/host/boole-mac-native-shadow-replay-node")
        .exists());
}

#[test]
fn runtime_reset_removes_only_ephemeral_vm_files_and_preserves_durable_user_state() {
    let fixture = FixtureDir::new("runtime-reset");
    let state_root = fixture.0.join("native-shadow");
    let plan = plan_installed_mac_lifecycle(&state_root).expect("plan");
    for directory in [
        plan.state_root(),
        plan.controller_runtime_root(),
        plan.journal_path().parent().expect("journal parent"),
        plan.materialized_host_node_path()
            .parent()
            .expect("host parent"),
    ] {
        fs::create_dir_all(directory).expect("runtime directory");
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .expect("private runtime directory");
    }
    fs::write(
        plan.controller_runtime_root().join("stale-controller"),
        b"ephemeral controller",
    )
    .expect("controller residue");
    fs::write(
        plan.materialized_host_node_path(),
        b"materialized host node",
    )
    .expect("host runtime");
    fs::write(plan.journal_path(), b"durable exactly-once journal\n").expect("journal");
    let wallet = fixture.0.join("wallet.vault.json");
    fs::write(&wallet, b"encrypted wallet sentinel").expect("wallet sentinel");

    reset_installed_direct_boot_runtime(&state_root).expect("runtime reset");

    assert!(!plan.controller_runtime_root().exists());
    assert!(!plan
        .materialized_host_node_path()
        .parent()
        .unwrap()
        .exists());
    assert_eq!(
        fs::read(plan.journal_path()).expect("journal survives"),
        b"durable exactly-once journal\n"
    );
    assert_eq!(
        fs::read(wallet).expect("wallet survives"),
        b"encrypted wallet sentinel"
    );
}

#[test]
#[allow(unsafe_code)]
fn runtime_reset_refuses_to_delete_a_live_controllers_directory() {
    let fixture = FixtureDir::new("runtime-reset-live");
    let state_root = fixture.0.join("native-shadow");
    let plan = plan_installed_mac_lifecycle(&state_root).expect("plan");
    for directory in [plan.state_root(), plan.controller_runtime_root()] {
        fs::create_dir_all(directory).expect("runtime directory");
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .expect("private runtime directory");
    }
    let lease = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(
            plan.controller_runtime_root()
                .join(".controller-runtime.lock"),
        )
        .expect("controller lease");
    // SAFETY: the test retains the live descriptor until after reset returns.
    assert_eq!(unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX) }, 0);

    let error = reset_installed_direct_boot_runtime(&state_root)
        .expect_err("a live controller must block reset");

    assert!(error.to_string().contains("still active"));
    assert!(plan.controller_runtime_root().is_dir());
}

#[test]
fn product_reset_cli_reports_cleanup_without_touching_the_journal() {
    let fixture = FixtureDir::new("runtime-reset-cli");
    let state_root = fixture.0.join("native-shadow");
    let plan = plan_installed_mac_lifecycle(&state_root).expect("plan");
    for directory in [
        plan.state_root(),
        plan.controller_runtime_root(),
        plan.journal_path().parent().expect("journal parent"),
        plan.materialized_host_node_path()
            .parent()
            .expect("host parent"),
    ] {
        fs::create_dir_all(directory).expect("runtime directory");
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .expect("private runtime directory");
    }
    fs::write(plan.journal_path(), b"durable journal\n").expect("journal");
    fs::write(plan.materialized_host_node_path(), b"runtime node").expect("runtime node");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "product",
            "reset-direct-boot",
            "--state-root",
            &state_root.display().to_string(),
        ])
        .output()
        .expect("reset CLI");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("typed reset response");
    assert_eq!(envelope["command"], "product.reset-direct-boot");
    assert_eq!(envelope["result"]["hostRuntimeRemoved"], true);
    assert_eq!(envelope["result"]["journalPreserved"], true);
    assert_eq!(envelope["result"]["walletStateTouched"], false);
    assert_eq!(
        fs::read(plan.journal_path()).expect("journal"),
        b"durable journal\n"
    );
}
