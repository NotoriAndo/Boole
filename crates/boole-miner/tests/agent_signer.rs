//! P1.10 — `AgentSigner` produces the SAME `boole.signed.v1` signature as the
//! in-process `KeySigner` for the same seed, with the seed sealed in a
//! `boole-wallet-agent` vault and never entering the miner process.
//!
//! The vault is created with a KNOWN seed via `boole-wallet-agent
//! migrate-from-hex` (passphrase + seed on stdin, never argv), so the two
//! signers can be compared byte-for-byte.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use boole_core::{verify_signature_with_network, SigningKeyV2};
use boole_miner::{AgentSigner, KeySigner, ProofSigner};
use boole_testkit::rand_suffix;
use serde_json::json;

#[test]
fn wallet_agent_errors_do_not_echo_passphrases_from_child_stderr() {
    use std::os::unix::fs::PermissionsExt;
    let vault = tmp_vault();
    let script = vault.parent().unwrap().join("echo-secret.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nIFS= read -r secret\nprintf '%s' \"$secret\" >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let secret = "disposable-passphrase-must-not-be-in-errors";
    let signer = AgentSigner::new(script.to_str().unwrap(), vault.clone(), secret.to_string());
    let error = signer.pk_hex().expect_err("agent failure");
    assert!(
        !error.contains(secret),
        "child diagnostics must not echo supplied secrets"
    );
    assert!(error.contains("wallet-agent"));
    std::fs::remove_dir_all(vault.parent().unwrap()).unwrap();
}

#[test]
fn wallet_agent_protocol_rejects_noncanonical_and_oversized_stdout() {
    use std::os::unix::fs::PermissionsExt;
    let vault = tmp_vault();
    let script = vault.parent().unwrap().join("bad-output.sh");
    for output in [
        "not-a-public-key".to_string(),
        "a".repeat(8192),
        "A".repeat(64),
        "0".repeat(64),
    ] {
        std::fs::write(
            &script,
            format!("#!/bin/sh\nIFS= read -r secret\nprintf '%s\\n' '{output}'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let signer = AgentSigner::new(
            script.to_str().unwrap(),
            vault.clone(),
            "test-only".to_string(),
        );
        assert!(
            signer.pk_hex().is_err(),
            "an invalid agent response must not become a wallet identity"
        );
    }
    std::fs::remove_dir_all(vault.parent().unwrap()).unwrap();
}

#[test]
fn wallet_agent_excess_output_is_stopped_before_waiting_for_process_exit() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let vault = tmp_vault();
    let script = vault.parent().unwrap().join("flood.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nIFS= read -r secret\nprintf '%s' '{}'\n/bin/sleep 3\n",
            "a".repeat(8192)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let signer = AgentSigner::new(
        script.to_str().unwrap(),
        vault.clone(),
        "test-only".to_string(),
    );
    let start = Instant::now();
    assert!(signer.pk_hex().is_err());
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "output cap must apply while the child is running"
    );
    std::fs::remove_dir_all(vault.parent().unwrap()).unwrap();
}

#[test]
fn wallet_agent_signature_must_verify_for_the_requested_payload_and_network() {
    use std::os::unix::fs::PermissionsExt;
    let vault = tmp_vault();
    let script = vault.parent().unwrap().join("wrong-signature.sh");
    let key = SigningKeyV2::from_dev_id("agent-protocol-signature-test");
    std::fs::write(&script, format!("#!/bin/sh\nIFS= read -r secret\nif [ \"$1\" = pubkey ]; then printf '%s\\n' '{}'; else printf '%s\\n' '{}'; fi\n", key.pk_hex(), "0".repeat(128))).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let signer = AgentSigner::new(
        script.to_str().unwrap(),
        vault.clone(),
        "test-only".to_string(),
    );
    assert!(
        signer
            .sign_payload(&json!({"test": true}), "boole-native-testnet-1")
            .is_err(),
        "an unverified agent signature must not be returned as a signed envelope"
    );
    std::fs::remove_dir_all(vault.parent().unwrap()).unwrap();
}

fn agent_bin() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_boole-miner"))
        .parent()
        .expect("compiled miner has a parent")
        .join("boole-wallet-agent")
}

/// Seal `seed_hex` into a fresh vault via `migrate-from-hex` (stdin:
/// passphrase line, then seed-hex line). Returns once the vault file exists.
fn seal_vault(bin: &Path, vault: &Path, passphrase: &str, seed_hex: &str) {
    let mut child = Command::new(bin)
        .args(["migrate-from-hex", "--vault", &vault.to_string_lossy()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-wallet-agent migrate-from-hex (is the bin built?)");
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

fn tmp_vault() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-10-agent-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("prover.vault")
}

#[test]
fn agent_signer_matches_key_signer_byte_for_byte() {
    let bin = agent_bin();
    if !bin.exists() {
        // Build the bin so the focused run is self-contained.
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "boole-wallet-agent"])
            .status()
            .expect("cargo build boole-wallet-agent");
        assert!(status.success(), "failed to build boole-wallet-agent");
    }

    let key = SigningKeyV2::from_dev_id("p1-10-agent-vs-key");
    let seed_hex = key.sk_seed_hex();
    let pk = key.pk_hex();
    let passphrase = "test-passphrase-p1-10";

    let vault = tmp_vault();
    seal_vault(&bin, &vault, passphrase, &seed_hex);

    let agent = AgentSigner::new(
        bin.to_string_lossy().into_owned(),
        vault.clone(),
        passphrase.to_string(),
    );
    let in_process = KeySigner::new(SigningKeyV2::from_seed_hex(&seed_hex).expect("seed"));

    // pk resolves identically (vault pubkey == seed-derived pk).
    assert_eq!(agent.pk_hex().expect("agent pk"), pk);
    assert_eq!(in_process.pk_hex().expect("key pk"), pk);

    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": "22".repeat(32),
        "prover": pk,
        "validBefore": 1_900_000_000_u64,
        "nonce": "0123456789abcdef0123456789abcdef",
    });

    let agent_env = agent
        .sign_payload(&payload, "boole-testnet")
        .expect("agent sign");
    let key_env = in_process
        .sign_payload(&payload, "boole-testnet")
        .expect("key sign");

    assert_eq!(
        agent_env.signature, key_env.signature,
        "AgentSigner must produce a byte-identical signature to the in-process KeySigner"
    );
    assert_eq!(agent_env.pk, key_env.pk);
    assert_eq!(agent_env.network_id.as_deref(), Some("boole-testnet"));
    assert!(
        verify_signature_with_network(
            &agent_env.pk,
            &agent_env.signature,
            &payload,
            Some("boole-testnet")
        )
        .expect("verify ran"),
        "the vault-produced signature must verify"
    );

    let _ = std::fs::remove_dir_all(vault.parent().unwrap());
}

#[test]
fn agent_signer_wrong_passphrase_is_a_typed_error() {
    let bin = agent_bin();
    if !bin.exists() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "boole-wallet-agent"])
            .status()
            .expect("cargo build");
        assert!(status.success());
    }
    let key = SigningKeyV2::from_dev_id("p1-10-agent-badpass");
    let vault = tmp_vault();
    seal_vault(&bin, &vault, "right-pass", &key.sk_seed_hex());

    let agent = AgentSigner::new(
        bin.to_string_lossy().into_owned(),
        vault.clone(),
        "wrong-pass".to_string(),
    );
    let payload = json!({"schema": "boole.bounty.proof.v1", "bountyId": "x"});
    let err = agent
        .sign_payload(&payload, "boole-testnet")
        .expect_err("a wrong passphrase must fail, not silently sign");
    assert!(
        err.contains("wallet-agent"),
        "error must name the wallet-agent: {err}"
    );

    let _ = std::fs::remove_dir_all(vault.parent().unwrap());
}
