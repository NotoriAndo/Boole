//! P2.1 closure (slice 50) — pin the `slice 49` bundle's structural
//! compatibility with `boole_miner::run_mining_loop`.
//!
//! With `MiningLoopOptions { max_cycles: Some(0), .. }` the loop body
//! never executes (the `while summary.protocol.cycles_run < 0` guard
//! short-circuits), so this test does zero driver / verifier / Lean
//! work but still catches upstream `MiningLoopDeps` shape drift the
//! moment `run_mining_loop` rejects the bundle's field set.
//!
//! Per master plan §6.5 P2.1: with slices 47+48+49+50 the composition
//! surface is contract-pinned end-to-end. The actual `boole.mine` /
//! `boole.status` MCP tool wiring that drives a real round-trip (with
//! `MockAccept` verifier + `MockLlmResponse` driver) rides on the
//! follow-up slice that exposes those tools on boole-mcp's HTTP API.

use num_bigint::BigUint;

use boole_core::Hex32;
use boole_mcp::{build_in_process_mining_deps, InProcessMiningInputs};
use boole_miner::{
    run_mining_loop, AnnounceTicketResult, ChainHead, MiningLoopOptions, MockDriver, MockResponse,
    RejectingVerifier, StructuralCanonicalizer, StubTargetEmitter, SubmitResult, VerifyReason,
};

fn fixture_inputs() -> InProcessMiningInputs {
    InProcessMiningInputs {
        pk: Hex32::from_bytes([1u8; 32]),
        head: ChainHead {
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
            difficulty_epoch: 0,
            mode: "static-calibrated".to_string(),
        },
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
fn run_mining_loop_accepts_in_process_bundle_with_zero_cycles() {
    let bundle = build_in_process_mining_deps(fixture_inputs());
    let capture = bundle.capture.clone();

    let opts = MiningLoopOptions {
        max_cycles: Some(0),
        ..Default::default()
    };

    let summary = run_mining_loop(bundle.deps, opts);

    // Zero cycles means: no head fetch attempted, no submitter calls.
    assert_eq!(summary.protocol.cycles_run, 0);
    assert_eq!(summary.protocol.tickets_found, 0);
    assert_eq!(summary.protocol.shares_accepted, 0);
    assert_eq!(summary.protocol.network_errors, 0);

    assert!(capture.captured_announces().is_empty());
    assert!(capture.captured_submits().is_empty());
}
