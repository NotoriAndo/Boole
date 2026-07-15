//! SC.10-iv-b — the committed lean-bound submission fixture the
//! testnet2-pinned-boot smoke drives into the pinned node must stay
//! byte-derivable from the consensus sources: seed = `target_seed` over the
//! fixture's chain context, source = the family's canonical render of that
//! seed, and `bytes` = the canon package bound to the `v1-lenbound`
//! verifier hash AND the canonical `lean/checker` artifact hash.
//!
//! The canon embeds the checker artifact hash, so ANY change to the pinned
//! checker moves the expected bytes — this test then fails and prints the
//! regenerated body, making the committed fixture impossible to drift
//! silently (the verdict-corpus golden-fixture posture, applied to the
//! live-Lean smoke input). Regeneration = replace the fixture's `body`
//! with the JSON this test prints on mismatch.

use std::path::PathBuf;

use boole_core::{
    family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, target_seed, Hex32,
};
use serde_json::{json, Value};

const PROFILE: &str = "v1-lenbound";
/// The chain context the smoke submits under: the boole-testnet-2 genesis
/// anchor (all-zero) — the node's head at the moment the smoke submits.
const C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
/// Any nonce clears the fixture policy's trivial T_submit; a constant keeps
/// the fixture fully deterministic.
const NONCE_S: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

fn expected_body() -> Value {
    let c = Hex32::from_hex(C).expect("c");
    let pk = Hex32::from_hex(PK).expect("pk");
    let n = Hex32::from_hex(N).expect("n");
    let seed = target_seed(&c, &pk, &n, 0);
    let seed_hex = seed.to_hex();
    let instance =
        family_v1_lenbound::generate_from_hex(&seed_hex).expect("seed generates an instance");
    let lean_source = family_v1_lenbound::render_canonical_proof(&instance);
    let checker_hash = boole_lean_runner::checker_artifact_hash(&canonical_checker_dir())
        .expect("checker artifact hash");
    let verifier_hash = lean_bound_verifier_hash(PROFILE);
    let canon = lean_bound_canon_package(&verifier_hash, &checker_hash, &lean_source);
    json!({
        "c": C,
        "pk": PK,
        "n": N,
        "j": J,
        "nonceS": NONCE_S,
        "seedHex": seed_hex,
        "bytes": hex::encode(canon),
    })
}

#[test]
fn testnet2_lenbound_share_fixture_matches_generator() {
    let expected = expected_body();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/testnet2-lenbound-share.v1.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "fixture {} unreadable ({err}); regenerate it with body:\n{}",
            path.display(),
            serde_json::to_string_pretty(&expected).expect("expected body json")
        )
    });
    let fixture: Value = serde_json::from_str(&raw).expect("fixture parses");
    assert_eq!(
        fixture["body"],
        expected,
        "the committed lean-bound share fixture no longer matches the \
         consensus generator (family render / canon / checker pin moved); \
         replace its `body` with:\n{}",
        serde_json::to_string_pretty(&expected).expect("expected body json")
    );
}
