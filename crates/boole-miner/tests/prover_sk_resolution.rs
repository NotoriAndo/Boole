//! P1.10 — secret-boundary for the prover ed25519 signing seed.
//!
//! `mine bounty` used to REQUIRE `--prover-sk-hex <seed>`, which puts the
//! 32-byte ed25519 seed on the process command line where `ps`, `/proc`,
//! process listings, and shell history can read it. P1.10 adds the
//! `BOOLE_PROVER_SK_HEX` environment variable as an alternative so the seed can
//! be supplied without ever appearing in argv. These tests pin the pure
//! resolution seam `resolve_prover_sk_hex(arg, env)` that the CLI uses, so the
//! "seed can be supplied off the command line" guarantee is verifiable without
//! spawning the binary (and without ever placing a real seed in a test argv).

use boole_miner::cli::resolve_prover_sk_hex;

const SEED_A: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
const SEED_B: &str = "feedface00000000000000000000000000000000000000000000000000000000";

#[test]
fn env_supplies_the_seed_without_argv() {
    // The secret-boundary guarantee: with no `--prover-sk-hex` argv at all, the
    // seed is resolved purely from the environment — it never has to touch the
    // command line.
    let resolved = resolve_prover_sk_hex(None, Some(SEED_A.to_string()))
        .expect("env seed must resolve without argv");
    assert_eq!(resolved, SEED_A);
}

#[test]
fn explicit_argv_flag_still_works_and_wins_for_back_compat() {
    // The flag is retained so existing scripts keep working, and it wins when
    // both are present (an operator who explicitly passed it meant it).
    let resolved =
        resolve_prover_sk_hex(Some(SEED_A), Some(SEED_B.to_string())).expect("argv seed resolves");
    assert_eq!(
        resolved, SEED_A,
        "explicit --prover-sk-hex must win over env"
    );
}

#[test]
fn neither_source_is_a_typed_error_naming_the_env_var() {
    let err = resolve_prover_sk_hex(None, None)
        .expect_err("no seed anywhere must be an error, not a silent empty seed");
    assert!(
        err.contains("BOOLE_PROVER_SK_HEX"),
        "the error must point operators at the env var: {err}"
    );
}

#[test]
fn empty_argv_falls_through_to_env() {
    // An empty `--prover-sk-hex=` must not shadow a real env seed.
    let resolved = resolve_prover_sk_hex(Some(""), Some(SEED_B.to_string()))
        .expect("empty argv falls through to env");
    assert_eq!(resolved, SEED_B);
}

#[test]
fn empty_both_is_an_error() {
    assert!(resolve_prover_sk_hex(Some(""), Some(String::new())).is_err());
    assert!(resolve_prover_sk_hex(Some(""), None).is_err());
}
