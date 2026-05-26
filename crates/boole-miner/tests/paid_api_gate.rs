//! P2.4 (slice 43) — pure paid-API policy evaluation.
//!
//! Splits the (env, TTY) decision out of `run_start` so the gate is
//! testable without spawning a subprocess or mutating the global
//! process environment. The caller is responsible for printing the
//! envelope and exiting with the carried code; this function returns a
//! typed outcome describing what should happen next.
//!
//! Slice 43 matrix (non-TTY + env paths):
//!   - paid backend + no opt-in + no TTY → `Err(PaidApiPolicyError)`
//!     carrying `exit_code = 3` and the unified `paid-api-not-opted-in`
//!     envelope (the typed refusal that drives the binary's exit).
//!   - paid backend + opt-in env (`1`/`true`/`TRUE`/`True`, trimmed) +
//!     no TTY → `Ok(AllowedByEnv)`.
//!   - non-paid backend + any (env, TTY) combination → `Ok(NotPaid)`.
//!   - paid backend + no opt-in + TTY → `Ok(RequiresInteractiveConfirm)`.
//!     The caller must prompt the operator; slice 44 wires the prompt.
//!
//! Wiring of `run_start` + actual exit-code propagation through the
//! binary boundary lives in a follow-up slice. Slice 43 is the pure
//! function + envelope shape that the wiring slice will consume.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use boole_miner::cli::{
    run_start_with_paid_policy_hooks, run_start_with_paid_policy_inputs,
    run_start_with_paid_policy_inputs_and_prompt, PaidApiConfirmDecision, PaidPolicyHooks,
    StartArgs, StateArgs,
};
use boole_miner::{
    evaluate_paid_api_policy, generate_miner_state, save_state, DispatcherConfig, LLMBackend,
    LlmConfig, MinerStateConfig, PaidApiPolicyError, PaidApiPolicyOutcome,
    EXIT_CODE_POLICY_REFUSED, PAID_LLM_ALLOW_ENV,
};
use boole_testkit::rand_suffix;

#[test]
fn paid_backend_no_optin_no_tty_returns_typed_refusal_with_exit_code_3() {
    let err = evaluate_paid_api_policy(LLMBackend::Anthropic, None, false, "mine.start")
        .expect_err("non-TTY + no opt-in must refuse");
    assert_eq!(err.exit_code, EXIT_CODE_POLICY_REFUSED);
    assert_eq!(EXIT_CODE_POLICY_REFUSED, 3, "P2.4 spec exit code is 3");
    let env = &err.envelope;
    assert_eq!(env["ok"], serde_json::Value::Bool(false));
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "mine.start");
    assert_eq!(env["error"]["reason"], "paid-api-not-opted-in");
    assert_eq!(env["error"]["backend"], "anthropic");
    assert_eq!(env["error"]["allowEnv"], PAID_LLM_ALLOW_ENV);
}

#[test]
fn paid_backend_with_truthy_optin_passes_in_non_tty() {
    for v in ["1", "true", "TRUE", "True", " 1 ", "true\n"] {
        let outcome = evaluate_paid_api_policy(LLMBackend::Anthropic, Some(v), false, "mine.start")
            .unwrap_or_else(|e| panic!("env {v:?} should opt in: {e:?}"));
        assert!(
            matches!(outcome, PaidApiPolicyOutcome::AllowedByEnv),
            "value {v:?} should produce AllowedByEnv, got {outcome:?}"
        );
    }
}

#[test]
fn paid_backend_with_falsy_or_empty_optin_refuses_in_non_tty() {
    for v in ["0", "false", "no", "", "yes", "True\nfalse"] {
        let outcome = evaluate_paid_api_policy(LLMBackend::OpenAi, Some(v), false, "mine.start");
        assert!(
            outcome.is_err(),
            "value {v:?} must not opt in (got {outcome:?})"
        );
    }
}

#[test]
fn non_paid_backend_passes_silently_in_every_combination() {
    let non_paid = [
        LLMBackend::Mock,
        LLMBackend::ClaudeCli,
        LLMBackend::OpenAiCompat,
        LLMBackend::AgentCli,
    ];
    let envs: [Option<&str>; 3] = [None, Some("1"), Some("0")];
    for backend in non_paid {
        for tty in [false, true] {
            for env in envs {
                let outcome = evaluate_paid_api_policy(backend, env, tty, "mine.start")
                    .expect("non-paid backend must always pass");
                assert!(
                    matches!(outcome, PaidApiPolicyOutcome::NotPaid),
                    "{backend:?} tty={tty} env={env:?} → {outcome:?}"
                );
            }
        }
    }
}

#[test]
fn paid_backend_no_optin_with_tty_defers_to_interactive_confirm() {
    for backend in [
        LLMBackend::Anthropic,
        LLMBackend::OpenAi,
        LLMBackend::Google,
    ] {
        let outcome = evaluate_paid_api_policy(backend, None, true, "mine.start")
            .expect("TTY path defers refusal to interactive confirm");
        assert!(
            matches!(outcome, PaidApiPolicyOutcome::RequiresInteractiveConfirm),
            "{backend:?} TTY + no opt-in → {outcome:?}"
        );
    }
}

#[test]
fn envelope_command_field_is_caller_specified() {
    let err = evaluate_paid_api_policy(LLMBackend::OpenAi, None, false, "mine.init")
        .expect_err("non-TTY must refuse for init too");
    assert_eq!(err.envelope["command"], "mine.init");
    assert_eq!(err.envelope["error"]["backend"], "openai");
}

#[test]
fn envelope_lists_all_paid_backends_in_extras_hint() {
    let err = evaluate_paid_api_policy(LLMBackend::Google, None, false, "mine.start")
        .expect_err("refused");
    // The envelope must include a one-shot hint naming the env var the
    // operator must export so an automation pipeline can surface it
    // without grepping a human-readable message.
    assert_eq!(err.envelope["error"]["allowEnv"], PAID_LLM_ALLOW_ENV);
    assert_eq!(err.envelope["error"]["backend"], "google");
}

// ---------- slice 44: wiring into run_start + binary exit code ----------

fn write_paid_backend_state(backend: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-miner-run-start-{backend}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).unwrap();
    let state_path = dir.join("state.json");
    let state = generate_miner_state(
        MinerStateConfig {
            dispatcher: DispatcherConfig {
                url: "http://127.0.0.1:8080".to_string(),
            },
            llm: LlmConfig {
                backend: backend.to_string(),
                api_key: None,
                model: Some("model-x".to_string()),
                base_url: None,
                agent_command: None,
                agent_args: None,
            },
        },
        "2026-01-01T00:00:00Z",
    );
    save_state(&state, &state_path).expect("save state");
    (dir, state_path)
}

fn start_args(state_path: PathBuf) -> StartArgs {
    StartArgs {
        state_args: StateArgs {
            state: Some(state_path),
        },
        profile: "v1-lenbound".to_string(),
        difficulty: 1,
        n: None,
        max_shares: None,
        max_cycles: None,
        head_timeout_ms: 10_000,
        mock_llm_response: None,
        #[cfg(feature = "dev-tools")]
        mock_verify_accept: false,
        fixed_target_seed_hex: None,
        fixed_target_render: None,
        deterministic_nonces: false,
        grind_max_attempts: None,
        lean_dir: None,
    }
}

#[test]
fn run_start_with_paid_backend_no_optin_no_tty_returns_typed_refusal() {
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);

    // Pass allow_env=None, is_tty=false explicitly — no process env mutation
    // so the test is safe under cargo's parallel runner.
    let err = run_start_with_paid_policy_inputs(args, None, false)
        .expect_err("paid backend + no opt-in + no TTY must refuse");
    let refusal: &PaidApiPolicyError = err
        .downcast_ref::<PaidApiPolicyError>()
        .unwrap_or_else(|| panic!("err must downcast to PaidApiPolicyError; got: {err:?}"));
    assert_eq!(refusal.exit_code, EXIT_CODE_POLICY_REFUSED);
    assert_eq!(refusal.envelope["command"], "mine.start");
    assert_eq!(refusal.envelope["error"]["reason"], "paid-api-not-opted-in");
    assert_eq!(refusal.envelope["error"]["backend"], "anthropic");
    assert_eq!(refusal.envelope["error"]["allowEnv"], PAID_LLM_ALLOW_ENV);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_start_with_paid_backend_tty_no_optin_also_refuses_in_slice_44() {
    // Slice 44 wires the pure function into run_start but does NOT yet
    // implement the interactive y/N prompt (slice 45). Until then, the
    // TTY path must refuse with the same typed envelope as the non-TTY
    // path so an operator sees a clean failure rather than a hang.
    let (dir, state_path) = write_paid_backend_state("openai");
    let args = start_args(state_path);

    let err = run_start_with_paid_policy_inputs(args, None, true)
        .expect_err("TTY + no opt-in must still refuse until slice 45 wires prompt");
    let refusal: &PaidApiPolicyError = err
        .downcast_ref::<PaidApiPolicyError>()
        .unwrap_or_else(|| panic!("err must downcast to PaidApiPolicyError; got: {err:?}"));
    assert_eq!(refusal.exit_code, EXIT_CODE_POLICY_REFUSED);
    assert_eq!(refusal.envelope["error"]["backend"], "openai");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_start_non_paid_backend_proceeds_past_policy_check() {
    // A mock backend has no gate. The call still fails (because the test
    // dispatcher URL is not running), but the failure must NOT downcast
    // to PaidApiPolicyError — i.e. the policy check let it through.
    let (dir, state_path) = write_paid_backend_state("mock");
    let args = start_args(state_path);

    let err = run_start_with_paid_policy_inputs(args, None, false)
        .expect_err("mock backend will fail past the gate (no live dispatcher)");
    assert!(
        err.downcast_ref::<PaidApiPolicyError>().is_none(),
        "non-paid backend must NOT be refused by the paid-API gate; err: {err:?}"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ---------- slice 45: TTY interactive y/N prompt ----------

#[test]
fn tty_no_optin_with_yes_prompt_proceeds_past_paid_gate_exactly_once() {
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_prompt = Arc::clone(&calls);
    let prompt = Box::new(move || {
        calls_in_prompt.fetch_add(1, Ordering::SeqCst);
        Ok(PaidApiConfirmDecision::Proceed)
    });

    let err = run_start_with_paid_policy_inputs_and_prompt(args, None, true, Some(prompt))
        .expect_err("anthropic backend still fails past the gate (no live dispatcher)");
    assert!(
        err.downcast_ref::<PaidApiPolicyError>().is_none(),
        "prompt=Proceed must let the gate pass; err: {err:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "prompt must be invoked exactly once on TTY+no-opt-in"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tty_no_optin_with_refuse_prompt_returns_typed_refusal_exactly_once() {
    let (dir, state_path) = write_paid_backend_state("google");
    let args = start_args(state_path);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_prompt = Arc::clone(&calls);
    let prompt = Box::new(move || {
        calls_in_prompt.fetch_add(1, Ordering::SeqCst);
        Ok(PaidApiConfirmDecision::Refuse)
    });

    let err = run_start_with_paid_policy_inputs_and_prompt(args, None, true, Some(prompt))
        .expect_err("Refuse decision must produce the typed refusal");
    let refusal = err
        .downcast_ref::<PaidApiPolicyError>()
        .unwrap_or_else(|| panic!("err must downcast to PaidApiPolicyError; got: {err:?}"));
    assert_eq!(refusal.exit_code, EXIT_CODE_POLICY_REFUSED);
    assert_eq!(refusal.envelope["error"]["backend"], "google");
    assert_eq!(refusal.envelope["error"]["reason"], "paid-api-not-opted-in");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "prompt invoked once");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tty_no_optin_with_prompt_io_error_returns_typed_refusal() {
    let (dir, state_path) = write_paid_backend_state("openai");
    let args = start_args(state_path);
    let prompt = Box::new(|| -> std::io::Result<PaidApiConfirmDecision> {
        Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "stdin closed",
        ))
    });

    let err = run_start_with_paid_policy_inputs_and_prompt(args, None, true, Some(prompt))
        .expect_err("prompt I/O error must produce the typed refusal");
    let refusal = err
        .downcast_ref::<PaidApiPolicyError>()
        .unwrap_or_else(|| panic!("err must downcast to PaidApiPolicyError; got: {err:?}"));
    assert_eq!(refusal.exit_code, EXIT_CODE_POLICY_REFUSED);
    assert_eq!(refusal.envelope["error"]["backend"], "openai");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn non_tty_path_does_not_invoke_prompt_at_all() {
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_prompt = Arc::clone(&calls);
    let prompt = Box::new(move || {
        calls_in_prompt.fetch_add(1, Ordering::SeqCst);
        Ok(PaidApiConfirmDecision::Proceed)
    });

    let err = run_start_with_paid_policy_inputs_and_prompt(args, None, false, Some(prompt))
        .expect_err("non-TTY + no opt-in must refuse without consulting prompt");
    assert!(err.downcast_ref::<PaidApiPolicyError>().is_some());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "non-TTY path must NOT invoke the prompt (silent refusal for automation)"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn opt_in_env_does_not_invoke_prompt_even_on_tty() {
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_prompt = Arc::clone(&calls);
    let prompt = Box::new(move || {
        calls_in_prompt.fetch_add(1, Ordering::SeqCst);
        Ok(PaidApiConfirmDecision::Refuse)
    });

    let err = run_start_with_paid_policy_inputs_and_prompt(args, Some("1"), true, Some(prompt))
        .expect_err("post-gate failure expected (no live dispatcher)");
    assert!(
        err.downcast_ref::<PaidApiPolicyError>().is_none(),
        "opt-in env must bypass prompt and the policy gate; err: {err:?}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "env opt-in short-circuits the prompt path"
    );

    let _ = fs::remove_dir_all(&dir);
}

// ---------- slice 46: boole_paid_api_optin_total counter ----------

#[test]
fn env_optin_with_paid_backend_increments_injected_counter_exactly_once() {
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);
    let counter = Arc::new(AtomicU64::new(0));
    let hooks = PaidPolicyHooks {
        prompt: None,
        optin_counter: Some(Arc::clone(&counter)),
    };

    // Anthropic + BOOLE_ALLOW_PAID_LLM=1 → AllowedByEnv. The post-gate
    // run will fail (no live dispatcher) but the counter must have been
    // incremented before that failure surfaces.
    let err = run_start_with_paid_policy_hooks(args, Some("1"), false, hooks)
        .expect_err("post-gate failure expected (no live dispatcher)");
    assert!(
        err.downcast_ref::<PaidApiPolicyError>().is_none(),
        "env opt-in must pass the gate; err: {err:?}"
    );
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "env opt-in on a paid backend must increment the counter exactly once"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn non_paid_backend_does_not_increment_counter_even_with_env_set() {
    let (dir, state_path) = write_paid_backend_state("mock");
    let args = start_args(state_path);
    let counter = Arc::new(AtomicU64::new(0));
    let hooks = PaidPolicyHooks {
        prompt: None,
        optin_counter: Some(Arc::clone(&counter)),
    };

    let _ = run_start_with_paid_policy_hooks(args, Some("1"), false, hooks);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "non-paid backend takes NotPaid path; counter must not move"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn refused_run_does_not_increment_counter() {
    let (dir, state_path) = write_paid_backend_state("openai");
    let args = start_args(state_path);
    let counter = Arc::new(AtomicU64::new(0));
    let hooks = PaidPolicyHooks {
        prompt: None,
        optin_counter: Some(Arc::clone(&counter)),
    };

    // Paid backend + no env + no TTY → typed refusal.
    let err = run_start_with_paid_policy_hooks(args, None, false, hooks)
        .expect_err("non-TTY no opt-in must refuse");
    assert!(err.downcast_ref::<PaidApiPolicyError>().is_some());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "refused run must NOT increment the opt-in counter"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tty_prompt_proceed_does_not_increment_env_optin_counter() {
    // The counter is specifically scoped to the env opt-in path (per
    // §6.5 P2.4 criterion: "BOOLE_ALLOW_PAID_LLM=1 set → proceeds, with
    // a structured boole_paid_api_optin_total counter increment"). An
    // operator who consented via the TTY prompt did NOT export the env
    // var, so the counter must stay still.
    let (dir, state_path) = write_paid_backend_state("anthropic");
    let args = start_args(state_path);
    let counter = Arc::new(AtomicU64::new(0));
    let prompt = Box::new(|| Ok(PaidApiConfirmDecision::Proceed));
    let hooks = PaidPolicyHooks {
        prompt: Some(prompt),
        optin_counter: Some(Arc::clone(&counter)),
    };

    let _ = run_start_with_paid_policy_hooks(args, None, true, hooks);
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "TTY-prompt Proceed path must NOT increment the env opt-in counter"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn repeated_env_optin_increments_counter_per_call() {
    let counter = Arc::new(AtomicU64::new(0));
    for _ in 0..3 {
        let (dir, state_path) = write_paid_backend_state("google");
        let args = start_args(state_path);
        let hooks = PaidPolicyHooks {
            prompt: None,
            optin_counter: Some(Arc::clone(&counter)),
        };
        let _ = run_start_with_paid_policy_hooks(args, Some("1"), false, hooks);
        let _ = fs::remove_dir_all(&dir);
    }
    assert_eq!(
        counter.load(Ordering::SeqCst),
        3,
        "each AllowedByEnv decision increments the counter exactly once"
    );
}

#[test]
fn boole_miner_start_paid_backend_no_optin_exits_with_code_3_and_envelope_on_stderr() {
    // Subprocess test: confirms the binary boundary translates the typed
    // PaidApiPolicyError into the documented exit code (3) and emits the
    // unified envelope to stderr. This is the operator-facing contract.
    let (dir, state_path) = write_paid_backend_state("google");

    // Build a clean env: drop the opt-in so the gate fires. Keep HOME
    // pointing at the per-test dir so the migration-check sidecar does
    // not race with other tests.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_boole-miner"));
    cmd.arg("start")
        .arg("--state")
        .arg(&state_path)
        .env_remove(PAID_LLM_ALLOW_ENV)
        .env("HOME", &dir);
    let output = cmd.output().expect("spawn boole-miner");

    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE_POLICY_REFUSED),
        "stderr:\n{}\nstdout:\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let envelope_line = stderr
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{') && l.contains("paid-api-not-opted-in"))
        .unwrap_or_else(|| {
            panic!(
                "expected JSON envelope line containing paid-api-not-opted-in; stderr:\n{stderr}"
            )
        });
    let parsed: serde_json::Value =
        serde_json::from_str(envelope_line.trim()).expect("envelope is valid JSON");
    assert_eq!(parsed["ok"], serde_json::Value::Bool(false));
    assert_eq!(parsed["version"], "v1");
    assert_eq!(parsed["command"], "mine.start");
    assert_eq!(parsed["error"]["reason"], "paid-api-not-opted-in");
    assert_eq!(parsed["error"]["backend"], "google");
    assert_eq!(parsed["error"]["allowEnv"], PAID_LLM_ALLOW_ENV);

    let _ = fs::remove_dir_all(&dir);
}
