//! P2.1 closure (slice 49) — `build_in_process_mining_deps` factory
//! that composes `InProcessChainHead` + `InProcessSubmitter` + the
//! caller-injected heavy collaborators (driver, verifier, emitter,
//! canonicalizer) into a ready-to-run `boole_miner::MiningLoopDeps`,
//! and hands back a clonable `CaptureLog` so the caller can inspect
//! announce/submit captures after the submitter moves behind the
//! `Box<dyn Submitter>` trait object owned by `MiningLoopDeps`.
//!
//! Per master plan §6.5 P2.1 closure criterion 1: slice 47 + 48 land
//! the trait impls; this slice lands the single composition point so
//! the follow-up `boole.mine` tool wiring has one obvious factory to
//! call. Actual mining-loop invocation rides on slice 50.

use num_bigint::BigUint;

use boole_core::Hex32;
use boole_mcp::{
    build_in_process_mining_deps, CapturedAnnounce, CapturedSubmit, InProcessMiningInputs,
};
use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, ChainHead, MockDriver, MockResponse,
    RejectingVerifier, StructuralCanonicalizer, StubTargetEmitter, SubmitInputs, SubmitResult,
    VerifyReason,
};

fn fixture_head() -> ChainHead {
    ChainHead {
        c: Hex32::from_bytes([0u8; 32]),
        t_ticket: BigUint::from(1u32) << 240,
        t_share: BigUint::from(1u32) << 232,
        t_block: BigUint::from(1u32) << 224,
        t_submit: BigUint::from(1u32) << 248,
        min_share_score: BigUint::from(1u32),
        m: 7,
        d: 11,
        profile: "v1-lenbound".to_string(),
        n: Some(3),
    }
}

fn fixture_inputs() -> InProcessMiningInputs {
    InProcessMiningInputs {
        pk: Hex32::from_bytes([1u8; 32]),
        head: fixture_head(),
        announce_result: AnnounceTicketResult::Observed {
            hash_hex: "0xticket".to_string(),
        },
        submit_result: SubmitResult::Accepted {
            share_hash_hex: "0xshare".to_string(),
        },
        emitter: Box::new(StubTargetEmitter::new("stub")),
        driver: Box::new(MockDriver::new(vec![MockResponse::Text(
            "fun xs => xs".to_string(),
        )])),
        verifier: Box::new(RejectingVerifier::new(VerifyReason::ElaborateFailed)),
        canonicalizer: Box::new(StructuralCanonicalizer),
    }
}

#[test]
fn build_in_process_mining_deps_returns_deps_serving_pinned_head() {
    let inputs = fixture_inputs();
    let expected_head = inputs.head.clone();

    let bundle = build_in_process_mining_deps(inputs);

    let got = bundle.deps.chain_head.fetch_head().expect("fetch_head ok");
    assert_eq!(got.c, expected_head.c);
    assert_eq!(got.t_ticket, expected_head.t_ticket);
    assert_eq!(got.profile, expected_head.profile);
    assert_eq!(got.n, expected_head.n);
    // pk + emitter + driver + verifier + canonicalizer fields are
    // moved into the deps verbatim — non-None presence pins them.
    assert_eq!(bundle.deps.pk, Hex32::from_bytes([1u8; 32]));
}

#[test]
fn build_in_process_mining_deps_capture_log_records_submitter_calls() {
    let bundle = build_in_process_mining_deps(fixture_inputs());

    bundle
        .deps
        .submit_client
        .announce_ticket(AnnounceTicketInputs {
            c_hex: "cA",
            pk_hex: "pkA",
            n_hex: "nA",
        });
    bundle.deps.submit_client.submit(SubmitInputs {
        c_hex: "cB",
        pk_hex: "pkB",
        n_hex: "nB",
        j_hex: "jB",
        nonce_s_hex: "nonceB",
        canon_bytes: b"canonB",
    });

    let announces = bundle.capture.captured_announces();
    assert_eq!(announces.len(), 1);
    assert_eq!(
        announces[0],
        CapturedAnnounce {
            c_hex: "cA".to_string(),
            pk_hex: "pkA".to_string(),
            n_hex: "nA".to_string(),
        }
    );

    let submits = bundle.capture.captured_submits();
    assert_eq!(submits.len(), 1);
    assert_eq!(
        submits[0],
        CapturedSubmit {
            c_hex: "cB".to_string(),
            pk_hex: "pkB".to_string(),
            n_hex: "nB".to_string(),
            j_hex: "jB".to_string(),
            nonce_s_hex: "nonceB".to_string(),
            canon_bytes: b"canonB".to_vec(),
        }
    );
}

#[test]
fn build_in_process_mining_deps_capture_log_is_clonable_independent_handle() {
    // The mining loop owns the submitter via `Box<dyn Submitter>`, so
    // the caller must be able to inspect captures via a separately
    // owned `CaptureLog` clone (Arc-backed internally).
    let bundle = build_in_process_mining_deps(fixture_inputs());
    let extra = bundle.capture.clone();

    bundle
        .deps
        .submit_client
        .announce_ticket(AnnounceTicketInputs {
            c_hex: "cX",
            pk_hex: "pkX",
            n_hex: "nX",
        });

    assert_eq!(bundle.capture.captured_announces().len(), 1);
    assert_eq!(extra.captured_announces().len(), 1);
}

#[test]
fn build_in_process_mining_deps_leaves_optional_collaborators_unset() {
    // Slice 49 covers required collaborators only. `prompt_builder`,
    // `log`, and `sleeper` stay `None` so the caller (the future
    // `boole.mine` tool, slice 50+) chooses whether to wire them.
    let bundle = build_in_process_mining_deps(fixture_inputs());
    assert!(bundle.deps.prompt_builder.is_none());
    assert!(bundle.deps.log.is_none());
    assert!(bundle.deps.sleeper.is_none());
}
