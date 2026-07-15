//! SC.10-ii-d-2 — share-level Lean re-verify at gossip admission, the gate
//! `ingress_admit_share` runs before a peer-announced base-lane share may
//! stay in the candidate pool on a named (checker-pinned) network.
//!
//! ADR-0016 (c-2): a producer does NOT re-run the checker over its own
//! assembled block — its Lean gate is admission. That is only sound if every
//! share in the pool passed the SAME single verifier entry ingest and reorg
//! re-verification use. `reverify_share_evidence` is the share-level fold of
//! that entry (`verify_lean_bound_share_evidence`), the exact per-share
//! mapping `reverify_block_selected_shares` folds over:
//!   * the share accepts / is not Lean-bound / is skipped ⇒ `Verified`;
//!   * a deterministic failure (source re-derive, canon mismatch, Lean
//!     `DeterministicReject`) ⇒ `DeterministicReject`;
//!   * a containment / availability failure ⇒ `RetryableUnavailable` —
//!     never a fail-open admit (ADR-0016 (a-3)).
//!
//! These focused tests pin the host-independent verdicts, mirroring
//! `ingest_block_reverify.rs`: the empty-seed Verified case and the
//! canon-mismatch deterministic reject need no Lean process, and the
//! retryable case is forced by pointing the checker at a non-existent
//! directory. The Lean-`Accepted`/`DeterministicReject` verdicts over a live
//! checker — and the end-to-end "a producer refuses a block assembled from
//! an unverified share" invariant — are exercised by the SC.10-iv Lean-invalid
//! injection smoke where lake is available.

use std::path::PathBuf;

use boole_core::{
    family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, AdmissionDecision,
    CalibrationReport, SelectedShareEvidence, BASE_LANE_MAX_HEARTBEATS, BASE_LANE_MAX_REC_DEPTH,
};
use boole_node::{
    reverify_share_evidence, BlockReverifyOutcome, RuntimeAdmissionState, RuntimeConfig,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

const PROFILE: &str = "v1-lenbound";
const SEED_HEX: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn checker_hash() -> String {
    boole_lean_runner::checker_artifact_hash(&canonical_checker_dir())
        .expect("checker artifact hash")
}

/// One base-lane Lean-bound share whose `proofPackage` is the canon a live
/// miner would have ground for `seed_hex` against the canonical checker.
/// `tamper_package` flips a canon byte so canon recompute mismatches; an
/// empty `seed_hex` yields a non-Lean-bound placeholder share.
fn lean_bound_share(tamper_package: bool, seed_hex: &str) -> SelectedShareEvidence {
    let checker_hash = checker_hash();
    let verifier_hash = lean_bound_verifier_hash(PROFILE);
    let (canon, proof_package) = match family_v1_lenbound::generate_from_hex(seed_hex) {
        Ok(instance) => {
            let lean_source = family_v1_lenbound::render_canonical_proof(&instance);
            let canon = lean_bound_canon_package(&verifier_hash, &checker_hash, &lean_source);
            let mut pkg = hex::encode(&canon);
            if tamper_package {
                let mut bytes = canon.clone();
                bytes[50] ^= 0xff;
                pkg = hex::encode(bytes);
            }
            (canon, pkg)
        }
        Err(_) => (Vec::new(), "00".repeat(64)),
    };
    let canon_hash = if canon.is_empty() {
        "00".repeat(32)
    } else {
        hex::encode(Sha256::digest(&canon))
    };
    SelectedShareEvidence {
        pk: PK.to_string(),
        n: N.to_string(),
        j: J.to_string(),
        c: PREV_C.to_string(),
        canon_hash,
        proof_package,
        seed_hex: seed_hex.to_string(),
        signed_work: None,
    }
}

#[test]
fn reverify_share_verified_when_not_lean_bound() {
    // An empty-seed share is not Lean-bound: the entry never launches Lean
    // and admission must keep admitting it (pre-N0.4b legacy posture — the
    // same share ingest folds to `Verified` inside a block).
    let share = lean_bound_share(false, "");
    let outcome = reverify_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::Verified),
        "a not-Lean-bound share must fold to Verified, got {outcome:?}"
    );
}

#[test]
fn reverify_share_deterministic_reject_on_canon_mismatch() {
    // A tampered-canon share is a pure file-hash binding failure — the
    // deterministic reject admission must convert into a typed refusal
    // BEFORE the share can sit in the pool a self-produced block draws
    // from. No Lean process is needed to reach it.
    let share = lean_bound_share(true, SEED_HEX);
    let outcome = reverify_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::DeterministicReject { .. }),
        "a tampered-canon share must fold to DeterministicReject, got {outcome:?}"
    );
}

#[test]
fn reverify_share_retryable_when_checker_dir_missing() {
    // A well-formed canon share that WOULD run Lean, but the checker
    // directory does not exist: the runner launch fails, which is an
    // availability failure — the share must NOT be admitted (that would be
    // a fail-open accept into the producer's own pool) and must NOT be a
    // deterministic reject either (ADR-0016 (a-3)).
    let share = lean_bound_share(false, SEED_HEX);
    let missing_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker-does-not-exist-sc10iid2");
    let outcome = reverify_share_evidence(
        PREV_C,
        &share,
        missing_dir.as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::RetryableUnavailable { .. }),
        "a missing checker dir must fold to RetryableUnavailable, got {outcome:?}"
    );
}

#[test]
fn retract_candidate_removes_admitted_share_from_candidates_but_keeps_pool_slot() {
    // The retraction the gossip-ingress Lean gate runs on a refusal: the
    // share must leave the candidate set a self-produced block draws on
    // (`candidate_shares_for_current_c`), while its SharePool (pk, n, j, c)
    // slot deliberately stays — the same pool-outlives-rejection posture as
    // the `duplicate_proof` peek — which also blocks an identical
    // re-announce until the pool prunes at the next commit.
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let report: CalibrationReport =
        serde_json::from_value(fixture["cfg"].clone()).expect("cfg parses");
    let constants = &fixture["constants"];
    let valid_patch = fixture["operations"]
        .as_array()
        .expect("operations array")
        .iter()
        .find(|op| op["name"] == "valid_after_bad_not_rate_limited")
        .expect("valid op")["bodyPatch"]
        .as_object()
        .cloned()
        .unwrap_or_default();

    let mut body = Map::new();
    for (key, field) in [
        ("c", "c"),
        ("pk", "pk"),
        ("n", "n"),
        ("j", "j"),
        ("nonceS", "nonceS"),
        ("bytes", "validBytesHex"),
    ] {
        body.insert(
            key.to_string(),
            Value::String(constants[field].as_str().expect(field).to_string()),
        );
    }
    for (key, value) in &valid_patch {
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }

    let mut runtime = RuntimeAdmissionState::new(
        RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config boots"),
    );
    runtime.set_current_c(constants["c"].as_str().expect("c").to_string());
    runtime
        .observe_ticket_from_body(&body)
        .expect("observe ticket");
    let decision = runtime.admit_body(1_800_000_000_000, "198.51.100.1", &body);
    let AdmissionDecision::Accepted { share_hash } = decision else {
        panic!("fixture share must be admitted: {decision:?}");
    };
    assert_eq!(runtime.candidate_shares_for_current_c().len(), 1);
    assert_eq!(runtime.pool_size(), 1);

    runtime.retract_candidate(&share_hash.to_hex());

    assert_eq!(
        runtime.candidate_shares_for_current_c().len(),
        0,
        "a retracted share must no longer be assemblable into a self-produced block"
    );
    assert_eq!(
        runtime.pool_size(),
        1,
        "the SharePool slot deliberately outlives the retraction"
    );
}
