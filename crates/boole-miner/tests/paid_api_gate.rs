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

use boole_miner::{
    evaluate_paid_api_policy, LLMBackend, PaidApiPolicyOutcome, EXIT_CODE_POLICY_REFUSED,
    PAID_LLM_ALLOW_ENV,
};

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
        let outcome =
            evaluate_paid_api_policy(LLMBackend::OpenAi, Some(v), false, "mine.start");
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
    for backend in [LLMBackend::Anthropic, LLMBackend::OpenAi, LLMBackend::Google] {
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
