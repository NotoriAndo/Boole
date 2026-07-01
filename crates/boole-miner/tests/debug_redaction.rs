//! P0.8 — secret-bearing types must never leak their secret through the
//! `Debug` trait (logs, panic messages, tracing spans, clap error
//! context all route through `{:?}`).
//!
//! `MinerState` and `LlmConfig` already had hand-written redacting
//! `Debug` impls (boole-miner/src/state.rs). The P0.8 re-audit found two
//! more secret-bearing types that still derived `Debug`:
//!
//!   * `LLMDriverConfig` — `api_key: Option<String>` (paid-backend key)
//!   * `BountyArgs` — `prover_sk_hex: String` (ed25519 signing seed),
//!     transitively printable via the `Debug`-deriving `MineCommand`
//!     enum that wraps it.
//!
//! These tests pin that the `{:?}` rendering of each type contains the
//! literal secret value NOWHERE and shows `<redacted>` instead, so a
//! future refactor that swaps the hand-rolled impl back to a derive
//! fails here before it can ship a credential into an operator's logs.

use std::time::Duration;

use boole_miner::cli::{BountyArgs, BountyNetworkPreset};
use boole_miner::{LLMBackend, LLMDriverConfig};

#[test]
fn llm_driver_config_debug_redacts_api_key() {
    let secret = "sk-LIVE-key-must-never-print-1234567890";
    let cfg = LLMDriverConfig {
        backend: LLMBackend::Anthropic,
        timeout: Duration::from_secs(30),
        claude_binary: None,
        agent_command: None,
        agent_args: vec![],
        api_key: Some(secret.to_string()),
        model: Some("claude-x".to_string()),
        base_url: None,
        max_tokens: None,
        allow_reasoning_as_answer: false,
    };
    let dbg = format!("{cfg:?}");
    assert!(
        !dbg.contains(secret),
        "LLMDriverConfig Debug leaked the api_key: {dbg}"
    );
    assert!(
        dbg.contains("<redacted>"),
        "LLMDriverConfig Debug must show <redacted> for a present api_key: {dbg}"
    );
    // Non-secret fields stay observable for diagnostics.
    assert!(
        dbg.contains("Anthropic"),
        "backend must remain visible: {dbg}"
    );
}

#[test]
fn llm_driver_config_debug_shows_none_when_no_api_key() {
    let cfg = LLMDriverConfig {
        backend: LLMBackend::ClaudeCli,
        timeout: Duration::from_secs(30),
        claude_binary: Some("claude".to_string()),
        agent_command: None,
        agent_args: vec![],
        api_key: None,
        model: None,
        base_url: None,
        max_tokens: None,
        allow_reasoning_as_answer: false,
    };
    let dbg = format!("{cfg:?}");
    // Presence must stay distinguishable: None renders as None, not as
    // <redacted>, so a missing-credential diagnostic is still readable.
    assert!(
        dbg.contains("api_key: None"),
        "absent api_key must render as None, not <redacted>: {dbg}"
    );
    assert!(
        !dbg.contains("<redacted>"),
        "no api_key means nothing to redact: {dbg}"
    );
}

#[test]
fn bounty_args_debug_redacts_prover_sk_hex() {
    let seed = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let args = BountyArgs {
        node: "http://127.0.0.1:8080".to_string(),
        network: BountyNetworkPreset::Testnet,
        id: "gamma-1".to_string(),
        prover: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        prover_sk_hex: Some(seed.to_string()),
        prover_vault: None,
        envelope_path: None,
        timeout_ms: 30_000,
    };
    let dbg = format!("{args:?}");
    assert!(
        !dbg.contains(seed),
        "BountyArgs Debug leaked the ed25519 prover_sk_hex seed: {dbg}"
    );
    assert!(
        dbg.contains("<redacted>"),
        "BountyArgs Debug must show <redacted> for prover_sk_hex: {dbg}"
    );
    // Public-by-design fields stay visible.
    assert!(
        dbg.contains("gamma-1"),
        "bounty id must remain visible: {dbg}"
    );
    assert!(
        dbg.contains("1111111111111111111111111111111111111111111111111111111111111111"),
        "prover public key is public-by-design and must remain visible: {dbg}"
    );
}
