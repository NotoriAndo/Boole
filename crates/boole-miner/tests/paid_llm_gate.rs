//! P2.4 — Paid-LLM gate integration coverage.
//!
//! `boole-miner init` materializes a state file that can later be used
//! to drive a real billing API call. The gate must therefore fire at
//! init time, not just at start time, so a paid backend cannot even be
//! persisted into the on-disk state without the operator's explicit
//! `BOOLE_ALLOW_PAID_LLM` acknowledgement.
//!
//! These tests run in-process (no subprocess spawn) so the gate is
//! exercised on the production code path, not a CLI parser side-effect.

use std::path::PathBuf;

use boole_miner::cli::{run_init, InitArgs, StateArgs, PAID_LLM_ALLOW_ENV};
use boole_testkit::rand_suffix;

fn unique_state_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "boole-miner-paid-llm-{label}-{}-{}.json",
        std::process::id(),
        rand_suffix()
    ))
}

fn anthropic_init_args(path: PathBuf) -> InitArgs {
    InitArgs {
        state_args: StateArgs { state: Some(path) },
        dispatcher_url: "http://127.0.0.1:8080".to_string(),
        llm_backend: "anthropic".to_string(),
        llm_model: Some("claude-opus-4-7".to_string()),
        llm_base_url: None,
        agent_command: None,
        agent_args: None,
        force: false,
    }
}

#[test]
fn run_init_rejects_paid_backend_without_opt_in() {
    // Avoid mutating the global process env: the gate is invoked from
    // run_init, which reads BOOLE_ALLOW_PAID_LLM. Other tests may run
    // in parallel, so we only assert the rejection branch when the env
    // var is unset.
    if std::env::var(PAID_LLM_ALLOW_ENV).is_ok() {
        eprintln!("skipping: {PAID_LLM_ALLOW_ENV} is set in this environment");
        return;
    }

    let path = unique_state_path("reject");
    let args = anthropic_init_args(path.clone());
    let err = run_init(args).expect_err("paid backend must be rejected without opt-in");
    let msg = err.to_string();
    assert!(
        msg.contains(PAID_LLM_ALLOW_ENV),
        "error must name {PAID_LLM_ALLOW_ENV}; got: {msg}"
    );
    assert!(
        !path.exists(),
        "state file must not be written when the gate rejects the init"
    );
}
