//! S19 — `boole mine bounty --node URL --id <id> --prover <hex32>
//! --prover-sk-hex <hex32> [--envelope-path <path>]`. Boots a local node
//! with the mock-verifier fixture and drives the CLI binary against it.
//! P1.6d wires a `boole.signed.v1` envelope around the inner proof payload,
//! so the miner CLI now requires the ed25519 seed via `--prover-sk-hex`.

use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn test_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("mine-bounty-cli-test")
}

const DUMMY_SEED_HEX: &str = "1100000000000000000000000000000000000000000000000000000000000000";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn mock_bounty_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1-mock.json")
        .canonicalize()
        .expect("mock bounty fixture path")
}

struct MockAccept;
impl BountyProofVerifier for MockAccept {
    fn verify(&self, _b: &Bounty, _e: &Value) -> Result<bool, String> {
        Ok(true)
    }
}

fn boot_with_mock(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s19-mine-bounty-cli-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let bounties_path = mock_bounty_fixture_path();
    let mut verifiers: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    verifiers.insert("mock-accept".to_string(), Arc::new(MockAccept));
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: Some("boole-testnet".to_string()),
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

fn cli_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

fn wallet_agent_bin() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target");
    p.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    p.push("boole-wallet-agent");
    p
}

/// Seal `seed_hex` into a fresh vault via `boole-wallet-agent migrate-from-hex`
/// (stdin: passphrase line, then seed-hex line — never argv).
fn seal_vault(agent: &Path, vault: &Path, passphrase: &str, seed_hex: &str) {
    let mut child = Command::new(agent)
        .args(["migrate-from-hex", "--vault", &vault.to_string_lossy()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-wallet-agent migrate-from-hex");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(format!("{passphrase}\n{seed_hex}\n").as_bytes())
        .expect("write passphrase+seed");
    let out = child.wait_with_output().expect("wait agent");
    assert!(
        out.status.success(),
        "migrate-from-hex failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn mine_bounty_with_prover_vault_submits_accepted_envelope() {
    // P1.10 — the prover seed stays sealed in a wallet-agent vault and never
    // enters the boole-cli/boole-miner process. Signing is delegated to the
    // agent subprocess (passphrase via BOOLE_WALLET_PASSPHRASE, never argv);
    // the node must accept the vault-produced signature exactly as it would a
    // seed-produced one (byte-identical — see ADR-0006).
    let agent = wallet_agent_bin();
    assert!(
        agent.exists(),
        "boole-wallet-agent binary missing at {}; build it first \
         (`cargo build -p boole-wallet-agent`). The full gate's workspace \
         build provides it.",
        agent.display()
    );

    let (addr, handle, dir) = boot_with_mock(1);
    let envelope = dir.join("envelope.bin");
    std::fs::write(&envelope, b"{}").expect("write envelope");

    let key = test_key();
    let pk = key.pk_hex();
    let vault = dir.join("prover.vault");
    let passphrase = "vault-pass-p1-10";
    seal_vault(&agent, &vault, passphrase, &key.sk_seed_hex());

    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            cli_url(addr).as_str(),
            "--network",
            "testnet",
            "--id",
            "gamma-1",
            "--prover",
            pk.as_str(),
            "--prover-vault",
            vault.to_str().unwrap(),
            "--envelope-path",
            envelope.to_str().unwrap(),
        ])
        .env("BOOLE_WALLET_PASSPHRASE", passphrase)
        .env("BOOLE_WALLET_AGENT_BIN", agent.to_str().unwrap())
        .output()
        .expect("run cli");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .expect("stdout json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["command"], "mine.bounty");
    assert_eq!(
        env["result"]["accepted"], true,
        "vault-signed proof accepted"
    );
    assert_eq!(env["result"]["bounty"]["status"], "solved");

    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mine_bounty_prover_vault_without_passphrase_is_typed_error() {
    // --prover-vault without BOOLE_WALLET_PASSPHRASE must fail with a typed
    // envelope, never silently fall back to a seed or an empty passphrase.
    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            "http://127.0.0.1:1",
            "--network",
            "testnet",
            "--id",
            "gamma-1",
            "--prover",
            test_key().pk_hex().as_str(),
            "--prover-vault",
            "/nonexistent/prover.vault",
        ])
        .env_remove("BOOLE_WALLET_PASSPHRASE")
        .output()
        .expect("run cli");
    assert!(!out.status.success(), "must fail without a passphrase");
    let env: Value = serde_json::from_str(String::from_utf8_lossy(&out.stderr).trim())
        .expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["reason"], "wallet-passphrase-missing");
}

#[test]
fn mine_bounty_submits_envelope_and_prints_ok_envelope() {
    let (addr, handle, dir) = boot_with_mock(1);

    let envelope = dir.join("envelope.bin");
    std::fs::write(&envelope, b"{}").expect("write envelope");

    let key = test_key();
    let pk = key.pk_hex();
    let sk = key.sk_seed_hex();
    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            cli_url(addr).as_str(),
            "--network",
            "testnet",
            "--id",
            "gamma-1",
            "--prover",
            pk.as_str(),
            "--prover-sk-hex",
            sk.as_str(),
            "--envelope-path",
            envelope.to_str().unwrap(),
        ])
        .output()
        .expect("run cli");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    // P2.5 — `mine bounty` success path is the unified envelope. The
    // proof outcome lives under `result` so the top-level
    // `version`/`command` describe the CLI schema rather than the proof
    // submission shape.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let env: Value = serde_json::from_str(stdout.trim()).expect("stdout json envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "mine.bounty");
    let result = &env["result"];
    assert_eq!(result["accepted"], true);
    assert_eq!(result["duplicate"], false);
    assert_eq!(result["bounty"]["status"], "solved");

    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mine_bounty_rejects_malformed_prover_locally() {
    // `--prover-sk-hex` is required by clap; supply a valid hex32 seed so the
    // arg parser is satisfied and run_bounty's bad_prover check fires first.
    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            "http://127.0.0.1:1",
            "--network",
            "testnet",
            "--id",
            "gamma-1",
            "--prover",
            "not-hex",
            "--prover-sk-hex",
            DUMMY_SEED_HEX,
        ])
        .output()
        .expect("run cli");
    assert!(!out.status.success(), "should fail on malformed prover");
    // P2.5 — `mine bounty` failure path now routes through the unified
    // envelope on stderr with a kebab-case reason token.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let env: Value = serde_json::from_str(stderr.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "mine.bounty");
    assert_eq!(env["error"]["reason"], "bad-prover");
}
