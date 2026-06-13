//! N0.1 — characterization (read-only inventory): pins that the LIVE mining
//! loop's canonical bytes come from the structural BPPK placeholder and are
//! NOT bound to any Lean-rendered proof. N0.3 INVERTS this file once the
//! default canonicalizer becomes `LeanBoundCanonicalizer`.
//!
//! Live-path seam inventory (re-verified 2026-06-13):
//! - default wiring: `boole-miner/src/cli.rs:1209` and
//!   `boole-mcp/src/main.rs:604` both inject `StructuralCanonicalizer`
//! - trait field: `mining_loop.rs` `MiningLoopDeps.canonicalizer`
//! - impl: `canonicalizer/structural.rs` (`encode_placeholder_bppk`)
//! - grind hash: `mining_loop.rs` hashes the canon bytes via
//!   `proof_package::bppk_canon_hash`

use std::sync::{Arc, Mutex};
use std::time::Duration;

use boole_core::{parse_biguint_hex, Hex32};
use boole_miner::{
    bppk_canon_hash, encode_placeholder_bppk, run_mining_loop, walk_bppk, AnnounceTicketInputs,
    AnnounceTicketResult, ChainHead, FixedChainHead, GenerateResult, GrinderConfig, MiningLoopDeps,
    MiningLoopOptions, ProverDriver, Strategy, StructuralCanonicalizer, StubTargetEmitter,
    SubmitInputs, SubmitResult, Submitter, Target, TargetEmitArgs, TargetEmitter, Verifier,
    VerifyReason, VerifyResult,
};

fn easy_head() -> ChainHead {
    ChainHead {
        c: Hex32::from_bytes([2u8; 32]),
        t_ticket: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_share: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_block: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        t_submit: parse_biguint_hex(&"f".repeat(64)).unwrap(),
        min_share_score: num_bigint::BigUint::default(),
        m: 1,
        d: 1,
        profile: "v01".to_string(),
        n: None,
    }
}

struct AnsweredDriver;

impl ProverDriver for AnsweredDriver {
    fn name(&self) -> &str {
        "canon-provenance-driver"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, _prompt: &str) -> GenerateResult {
        GenerateResult::Answered {
            answer: "```lean\nby trivial\n```".to_string(),
            elapsed: Duration::ZERO,
            tokens_used: None,
        }
    }
}

struct AlwaysAcceptVerifier;

impl Verifier for AlwaysAcceptVerifier {
    fn verify(
        &self,
        _seed_hex: &str,
        _d: u32,
        _proof_source: &str,
        _n: Option<u32>,
    ) -> VerifyResult {
        VerifyResult {
            accepted: true,
            reason: VerifyReason::Accepted,
            elapsed: Duration::ZERO,
            stderr_tail: String::new(),
            attempt_artifact_path: None,
        }
    }
}

/// Wraps `StubTargetEmitter` and records the exact `Target` the loop used,
/// so the test can recompute the expected placeholder encoding.
struct RecordingTargetEmitter {
    inner: StubTargetEmitter,
    emitted: Arc<Mutex<Vec<Target>>>,
}

impl TargetEmitter for RecordingTargetEmitter {
    fn emit(&self, args: &TargetEmitArgs<'_>) -> anyhow::Result<Target> {
        let target = self.inner.emit(args)?;
        self.emitted.lock().unwrap().push(target.clone());
        Ok(target)
    }
}

/// Captures the canon bytes the live loop actually submits.
struct CanonCapturingSubmitter {
    canon_bytes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Submitter for CanonCapturingSubmitter {
    fn announce_ticket(&self, _inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        AnnounceTicketResult::Observed {
            hash_hex: String::new(),
        }
    }

    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        self.canon_bytes
            .lock()
            .unwrap()
            .push(inputs.canon_bytes.to_vec());
        SubmitResult::Accepted {
            share_hash_hex: String::new(),
        }
    }
}

#[test]
fn live_loop_canon_is_structural_bppk_placeholder_today() {
    let emitted = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::new(Mutex::new(Vec::new()));
    let deps = MiningLoopDeps {
        pk: Hex32::from_bytes([1u8; 32]),
        chain_head: Box::new(FixedChainHead { head: easy_head() }),
        emitter: Box::new(RecordingTargetEmitter {
            inner: StubTargetEmitter::new("render"),
            emitted: emitted.clone(),
        }),
        driver: Box::new(AnsweredDriver),
        verifier: Box::new(AlwaysAcceptVerifier),
        // The SAME default the live `mine start` path injects (cli.rs:1209;
        // boole-mcp main.rs:604).
        canonicalizer: Box::new(StructuralCanonicalizer),
        submit_client: Box::new(CanonCapturingSubmitter {
            canon_bytes: captured.clone(),
        }),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(1),
        deterministic_nonces: true,
        ticket_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        share_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        submit_grind: GrinderConfig {
            max_attempts: Some(1),
            report_every_hashes: 0,
        },
        ..Default::default()
    };

    let outcome = run_mining_loop(deps, opts);
    assert_eq!(outcome.protocol.shares_accepted, 1, "loop must submit once");

    let targets = emitted.lock().unwrap();
    let canons = captured.lock().unwrap();
    assert_eq!(targets.len(), 1, "exactly one target emitted");
    assert_eq!(canons.len(), 1, "exactly one canon submitted");
    let target = &targets[0];
    let canon = &canons[0];

    // (1) Placeholder identity: the bytes the live loop grinds and submits
    // are EXACTLY the structural BPPK placeholder over the intake-normalized
    // proof source ("by trivial") and the emitted target — not a
    // Lean-evidence-bound POFP-v2 package.
    assert_eq!(
        canon,
        &encode_placeholder_bppk("by trivial", target),
        "live canon bytes must equal the structural placeholder encoding"
    );

    // (2) The grind hash formula over those bytes is bppk_canon_hash
    // (the L1 gap: proof_package.rs `bppk_canon_hash`).
    let grind_hash = bppk_canon_hash(canon);
    assert_eq!(
        grind_hash,
        bppk_canon_hash(&encode_placeholder_bppk("by trivial", target))
    );

    // (3) NOT Lean-bound: the package embeds ZERO declarations — no
    // Lean-rendered canonical proof and no checker evidence live inside,
    // only the raw proof source as an opaque string literal.
    let walk = walk_bppk(canon).expect("placeholder package walks");
    assert_eq!(
        walk.decl_count, 0,
        "placeholder must embed no Lean declarations"
    );
}
