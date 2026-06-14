//! N0.3 — INVERTED from N0.1: the LIVE mining loop, assembled via the
//! production canonicalizer factory `live_canonicalizer`, now grinds and
//! submits Lean-bound canon bytes (POFP-v2 from the family's canonical
//! proof + checker evidence), NOT the structural BPPK placeholder.
//!
//! Live-path seam (N0.3): `boole-miner/src/cli.rs` builds its canonicalizer
//! via `boole_miner::live_canonicalizer(lean_dir, profile)`; with a checker
//! dir it returns `LeanBoundCanonicalizer`. This test drives the loop with
//! the same factory and asserts the submitted canon is lean-bound.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use boole_core::{parse_biguint_hex, Hex32};
use boole_miner::{
    encode_placeholder_bppk, live_canonicalizer, run_mining_loop, AnnounceTicketInputs,
    AnnounceTicketResult, ChainHead, FixedChainHead, GenerateResult, GrinderConfig, MiningLoopDeps,
    MiningLoopOptions, ProverDriver, Strategy, StubTargetEmitter, SubmitInputs, SubmitResult,
    Submitter, Target, TargetEmitArgs, TargetEmitter, Verifier, VerifyReason, VerifyResult,
};

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

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
fn live_loop_canon_is_lean_bound() {
    let lean_dir = canonical_checker_dir();
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
        // The SAME canonicalizer the live `mine start` path injects
        // (cli.rs builds it via this factory). With a checker dir present
        // this is `LeanBoundCanonicalizer`, not the placeholder.
        canonicalizer: live_canonicalizer(Some(lean_dir.as_path()), "v1-lenbound")
            .expect("live canonicalizer builds from the canonical checker dir"),
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

    // (1) No longer the placeholder: the bytes the live loop grinds and
    // submits must NOT equal the structural BPPK placeholder for this
    // (proof source, target) — the L1 gap is closed on the live path.
    assert_ne!(
        canon,
        &encode_placeholder_bppk("by trivial", target),
        "live canon must not be the structural placeholder anymore"
    );

    // (2) Lean-bound identity: the submitted canon equals what the
    // LeanBoundCanonicalizer produces for the same target via the factory,
    // i.e. it is the POFP-v2 package derived from the family canonical
    // proof + checker evidence (not the model's raw answer).
    let expected = live_canonicalizer(Some(lean_dir.as_path()), "v1-lenbound")
        .expect("rebuild factory")
        .canonicalize("by trivial", target)
        .expect("lean-bound canonicalize");
    assert_eq!(
        canon, &expected,
        "live canon must be the Lean-bound POFP-v2 package"
    );

    // (3) It is a POFP-v2 package (magic + version 2), not a BPPK package.
    assert_eq!(&canon[..4], b"POFP", "canon must carry the POFP magic");
    assert_eq!(
        u32::from_le_bytes(canon[4..8].try_into().unwrap()),
        2,
        "canon must be POFP format version 2"
    );
}
