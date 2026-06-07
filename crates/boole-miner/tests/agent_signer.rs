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

fn agent_bin() -> PathBuf {
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
