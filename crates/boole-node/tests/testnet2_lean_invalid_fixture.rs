//! SC.10-iv-c — the committed Lean-INVALID submission fixture the
//! testnet2-lean-invalid-injection smoke drives into a checker-off faulty
//! producer must stay byte-derivable from the consensus sources: it is the
//! honest iv-b lean-bound share with EXACTLY ONE canon byte flipped.
//!
//! The seed still binds to the genesis chain context (so structural
//! admission on the checker-off producer accepts it and self-produces a
//! block carrying it), but the committed `bytes` no longer equal the canon
//! the seed commits to — so every checker-PINNED honest node re-derives the
//! source from the seed, recomputes the canon and deterministically rejects
//! the block at ingest re-verification (`ShareEvidenceVerdict::CanonMismatch`
//! → `BlockReverifyOutcome::DeterministicReject`). That is the "structurally
//! valid but proof-invalid" injection the SC.10 mandatory gate must observe
//! adopted by no honest node.
//!
//! Byte 50 is inside a POFP opaque-digest slot (past the magic/version
//! header), so the flip leaves the package structurally parseable and its
//! score above the floor — the same tamper the SC.10-ii unit tests use
//! (`lean_bound_share(tamper_package: true)`, `bytes[50] ^= 0xff`). Because
//! the fixture is the honest canon minus one byte, ANY drift in the family
//! render / canon / checker pin moves BOTH fixtures and this test fails,
//! printing the regenerated body — the golden-fixture posture applied to the
//! negative injection input.

use std::path::PathBuf;

use boole_core::{
    difficulty_weight, digest_to_biguint, family_v1_lenbound, lean_bound_canon_package,
    lean_bound_verifier_hash, min_share_score, share_hash, share_score, target_seed, Hex32,
};
use serde_json::{json, Value};

const PROFILE: &str = "v1-lenbound";
/// The boole-testnet-2 genesis anchor (all-zero) — the faulty producer's
/// head when the smoke injects, identical to the honest iv-b fixture so both
/// bind to the same chain context.
const C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
/// A DISTINCT admission nonce from the honest iv-b fixture (which uses
/// `aaaa..`). The per-PK admission quota is keyed on the ticket `(pk, c, n)`:
/// when the faulty producer gossips this share to an honest node it observes
/// one ticket, and the honest control share must still be able to bring its
/// own ticket rather than colliding on an already-spent `(pk, c, n)`. A
/// different `n` also re-derives a different seed, so the injected canon is
/// bound to its own seed.
const N: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const NONCE_S: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
/// The single canon byte flipped to make the package no longer the canon of
/// the seed-derived source — inside an opaque-digest slot, matching the
/// SC.10-ii `lean_bound_share` tamper.
const TAMPER_INDEX: usize = 50;

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

/// The honest canon (identical to the iv-b generator) and the tampered
/// package: one byte of that canon flipped.
fn honest_and_tampered_canon() -> (Vec<u8>, Vec<u8>) {
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
    let honest = lean_bound_canon_package(&verifier_hash, &checker_hash, &lean_source);
    let mut tampered = honest.clone();
    tampered[TAMPER_INDEX] ^= 0xff;
    (honest, tampered)
}

fn expected_body() -> Value {
    let c = Hex32::from_hex(C).expect("c");
    let pk = Hex32::from_hex(PK).expect("pk");
    let n = Hex32::from_hex(N).expect("n");
    let seed = target_seed(&c, &pk, &n, 0);
    let (_, tampered) = honest_and_tampered_canon();
    json!({
        "c": C,
        "pk": PK,
        "n": N,
        "j": J,
        "nonceS": NONCE_S,
        "seedHex": seed.to_hex(),
        "bytes": hex::encode(tampered),
    })
}

#[test]
fn testnet2_lean_invalid_share_fixture_matches_generator() {
    let expected = expected_body();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/testnet2-lean-invalid.v1.json");
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
        "the committed Lean-invalid share fixture no longer matches the \
         consensus generator (family render / canon / checker pin moved); \
         replace its `body` with:\n{}",
        serde_json::to_string_pretty(&expected).expect("expected body json")
    );
}

/// The injection is only meaningful if the tampered share is BOTH admissible
/// on the checker-off producer (so a block carrying it is actually assembled
/// and gossiped) AND a canon mismatch the pinned nodes reject. Pin both
/// properties so a future change that made the tamper structurally rejected
/// at admission (the injection never leaving the producer) fails here instead
/// of silently turning the smoke into a no-op.
#[test]
fn tampered_share_is_admissible_yet_canon_mismatched() {
    let (honest, tampered) = honest_and_tampered_canon();
    assert_ne!(
        honest, tampered,
        "the tamper must actually change the canon package"
    );

    // Admissible: the score derived from the tampered package still clears
    // the genesis floor (T_share is max ⇒ min_share_score == 1, and every
    // 256-bit share hash scores >= 1), so the producer admits it.
    let c = Hex32::from_hex(C).expect("c");
    let pk = Hex32::from_hex(PK).expect("pk");
    let n = Hex32::from_hex(N).expect("n");
    let j = Hex32::from_hex(J).expect("j");
    let canon_hash = Hex32::from_bytes(sha256(&tampered));
    let sh = share_hash(&c, &pk, &n, &j, &canon_hash);
    let score = share_score(&sh);
    // The genesis T_share is 0xff..ff (max), so the floor is
    // difficulty_weight(T_share_max) == 2^256 / 2^256 == 1 and every 256-bit
    // share hash scores >= 1 — the tamper cannot fall below the floor.
    let t_share_max = digest_to_biguint(&Hex32::from_bytes([0xffu8; 32]));
    let floor = min_share_score(&t_share_max, boole_core::MIN_SHARE_SCORE_MULTIPLIER_NANOS)
        .expect("min share score");
    assert!(
        score >= floor,
        "tampered share score {score} must clear the genesis floor {floor} \
         so the checker-off producer admits it"
    );
    assert_eq!(
        floor,
        difficulty_weight(&t_share_max).expect("difficulty weight"),
        "genesis T_share floor is difficulty_weight(T_share_max) == 1"
    );
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}
