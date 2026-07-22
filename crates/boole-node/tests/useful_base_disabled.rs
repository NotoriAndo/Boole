//! BF.0 — disabled useful-work scaffold parity pin.
//!
//! With no useful-base configuration the scaffold must be completely
//! absent: the mode defaults to `Disabled`, an explicit `TestnetScaffold`
//! opt-in cannot leak into the consensus genesis surface, and a disabled
//! boot leaves zero useful-base files on disk. The existing v3 golden
//! pins (`block_hash_fixtures`, `genesis_replay`, `runtime_boot_parity`)
//! stay the byte-identity net; this file pins the scaffold-off contract.

use boole_node::{serve_local_node, LocalNodeConfig, RuntimeConfig, UsefulBaseMode};
use boole_testkit::rand_suffix;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

fn runtime_config() -> RuntimeConfig {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/runtime-smoke/v1.json"
    ))
    .expect("fixture parses");
    let report = serde_json::from_value(fixture["cfg"].clone()).expect("calibration report");
    RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config")
}

#[test]
fn runtime_config_defaults_useful_base_mode_to_disabled() {
    let config = runtime_config();
    assert_eq!(config.useful_base_mode, UsefulBaseMode::Disabled);
}

#[test]
fn testnet_scaffold_opt_in_does_not_leak_into_genesis_spec() {
    let disabled = runtime_config();
    let scaffold = runtime_config().with_useful_base_mode(UsefulBaseMode::TestnetScaffold);
    assert_eq!(scaffold.useful_base_mode, UsefulBaseMode::TestnetScaffold);
    assert_eq!(
        disabled.genesis_spec("closed-local", "c0"),
        scaffold.genesis_spec("closed-local", "c0"),
        "useful-base mode must not appear anywhere in the consensus genesis surface"
    );
}

#[test]
fn disabled_boot_creates_no_useful_base_files() {
    let work_dir = std::env::temp_dir().join(format!(
        "boole-bf0-disabled-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let state_dir = work_dir.join("state");
    fs::create_dir_all(&work_dir).expect("work dir");

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");
    let scenario_path = repo_root.join("fixtures/protocol/runtime-smoke/v1.json");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");
    let (tx, rx) = mpsc::channel();
    let block_path = work_dir.join("blocks.ndjson");
    let server_state_dir = state_dir.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("signal ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(1),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: Some(server_state_dir),
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(std::time::Duration::from_millis(50));

    let status = request_json(addr, "GET /status HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert_eq!(status["ok"], true);
    handle
        .join()
        .expect("server thread joins")
        .expect("server exits cleanly");

    let state_entries: BTreeSet<String> = fs::read_dir(&state_dir)
        .expect("state dir exists")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let expected: BTreeSet<String> = ["state.lock", "state.manifest.json"]
        .into_iter()
        .map(String::from)
        .collect();
    assert_eq!(
        state_entries, expected,
        "a disabled boot must create exactly the pre-BF state files and nothing else"
    );

    for dir in [&work_dir, &state_dir] {
        for entry in fs::read_dir(dir).expect("dir exists") {
            let name = entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase();
            assert!(
                !name.contains("useful"),
                "no useful-base path may exist when the mode is disabled: {name}"
            );
        }
    }

    let _ = fs::remove_dir_all(&work_dir);
}

fn request_json(addr: std::net::SocketAddr, raw: &str) -> Value {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(raw.as_bytes()).expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("http body present");
    serde_json::from_str(body).expect("json body")
}
