#![cfg(unix)]
use boole_devnet_mcp::{
    codec::{digest, private_file},
    hook::Precheck,
    store::Store,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    time::{Duration, Instant},
};
fn dir() -> PathBuf {
    let root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let root = root.join("mcp-hook-tests");
    fs::create_dir_all(&root).unwrap();
    let mut nonce = [0; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce).unwrap();
    root.join(boole_derivation::attributes::hex_string(&nonce))
}
#[test]
fn private_files_permissions_symlinks_and_pinned_hook_outcomes() {
    let path = dir();
    let store = Store::open(path.clone()).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    store
        .write("test.json", &serde_json::json!({"secret":"local-only"}))
        .unwrap();
    assert_eq!(
        fs::metadata(path.join("test.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let script = path.join("precheck");
    for (code, expected) in [
        (0, None),
        (10, Some("deterministic reject")),
        (2, Some("tool error")),
    ] {
        let bytes = format!("#!/bin/sh\necho private-hook-canary\nexit {code}\n").into_bytes();
        fs::write(&script, &bytes).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let hook = Precheck {
            executable: script.clone(),
            sha256: digest(&bytes),
            gate_rule_digest: None,
            timeout: Duration::from_secs(2),
        };
        let result = hook.run(&path, b"export", b"problem", [1; 32], 2);
        if let Some(expected) = expected {
            let e = result.unwrap_err().to_string();
            assert!(e.contains(expected));
            assert!(!e.contains("private-hook-canary"));
        } else {
            result.unwrap();
        }
        if code == 0 {
            let alias = path.join("installed-tool-alias");
            std::os::unix::fs::symlink(&script, &alias).unwrap();
            Precheck {
                executable: alias,
                sha256: digest(&bytes),
                gate_rule_digest: None,
                timeout: Duration::from_secs(2),
            }
            .run(&path, b"export", b"problem", [1; 32], 2)
            .unwrap();
        }
        let bad = Precheck {
            executable: script.clone(),
            sha256: [0; 32],
            gate_rule_digest: None,
            timeout: Duration::from_secs(2),
        };
        assert!(bad
            .run(&path, b"a", b"b", [0; 32], 1)
            .unwrap_err()
            .to_string()
            .contains("digest mismatch"));
    }
    fs::set_permissions(path.join("test.json"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.read::<serde_json::Value>("test.json").is_err());
    std::os::unix::fs::symlink(&script, path.join("linked.json")).unwrap();
    assert!(store.read::<serde_json::Value>("linked.json").is_err());
    drop(store);
    fs::remove_dir_all(path).unwrap();
}
#[test]
fn hook_timeout_kills_and_reaps_its_process() {
    let path = dir();
    let store = Store::open(path.clone()).unwrap();
    let script = path.join("precheck");
    let bytes = b"#!/bin/sh\nexec sleep 5\n";
    private_file(&script, bytes).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let hook = Precheck {
        executable: script,
        sha256: digest(bytes),
        gate_rule_digest: None,
        timeout: Duration::from_millis(30),
    };
    let start = Instant::now();
    assert!(hook
        .run(&path, b"a", b"b", [0; 32], 1)
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(3));
    drop(store);
    fs::remove_dir_all(path).unwrap();
}
