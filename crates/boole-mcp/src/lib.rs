//! P2.1 closure — in-process trait impls that let `boole-mcp` drive
//! `boole-miner`'s mining loop without an HTTP loopback to
//! `boole-node`.
//!
//! Pieces:
//!   * **slice 47** — `InProcessChainHead` (`ChainHeadFetcher`).
//!   * **slice 48** — `InProcessSubmitter` (`Submitter`) + capture
//!     buffers so a test or the future `boole.mine` tool can read
//!     back what shares/blocks the miner emitted.
//!   * **slice 49** — `build_in_process_mining_deps` factory that
//!     bundles both impls + caller-injected heavy collaborators
//!     (driver, verifier, emitter, canonicalizer) into a
//!     `MiningLoopDeps` ready for `run_mining_loop`, and hands back a
//!     clonable `CaptureLog` so the caller can inspect submitter
//!     captures after the submitter has moved behind the
//!     `Box<dyn Submitter>` trait object owned by `MiningLoopDeps`.
//!
//! Actual mining-loop invocation + the `boole.mine` / `boole.status`
//! MCP tool wiring ride on follow-up slices.

use std::sync::{Arc, Mutex};

use boole_core::Hex32;
use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, Canonicalizer, ChainHead, ChainHeadError,
    ChainHeadFetcher, MiningLoopDeps, ProverDriver, SubmitInputs, SubmitResult, Submitter,
    TargetEmitter, Verifier,
};

/// P2.1 slice 55 — canonical runtime-smoke scenario fixture embedded at
/// build time. Keeps the binary self-sufficient: a user running
/// `boole-mcp serve` does not need anything from `fixtures/` on their
/// host. Closes P2.1 closure criterion 3. Future slices source
/// `default_in_process_inputs` thresholds from this byte slice instead
/// of hardcoded BigUint constants.
pub const RUNTIME_SMOKE_FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/protocol/runtime-smoke/v1.json");

/// `ChainHeadFetcher` impl that returns a single pinned `ChainHead`.
/// Suitable for boole-mcp's mining tools when the head is sourced from
/// boole-mcp's own state instead of an external boole-node `GET /head`
/// HTTP call.
pub struct InProcessChainHead {
    head: ChainHead,
}

impl InProcessChainHead {
    pub fn new(head: ChainHead) -> Self {
        Self { head }
    }
}

impl ChainHeadFetcher for InProcessChainHead {
    fn fetch_head(&self) -> Result<ChainHead, ChainHeadError> {
        Ok(self.head.clone())
    }
}

/// One captured `announce_ticket` call. Owned strings because the
/// `AnnounceTicketInputs` lifetime ends as soon as the call returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAnnounce {
    pub c_hex: String,
    pub pk_hex: String,
    pub n_hex: String,
}

/// One captured `submit` call. `canon_bytes` is cloned to an owned
/// `Vec<u8>` for the same lifetime reason as `CapturedAnnounce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSubmit {
    pub c_hex: String,
    pub pk_hex: String,
    pub n_hex: String,
    pub j_hex: String,
    pub nonce_s_hex: String,
    pub canon_bytes: Vec<u8>,
}

/// Shared, clonable view onto an `InProcessSubmitter`'s capture
/// buffers. Cheap to clone (Arc internally) so a caller can hand the
/// submitter into `Box<dyn Submitter>` while retaining a separate
/// handle to read announce / submit captures.
#[derive(Debug, Clone, Default)]
pub struct CaptureLog {
    inner: Arc<CaptureLogInner>,
}

#[derive(Debug, Default)]
struct CaptureLogInner {
    announces: Mutex<Vec<CapturedAnnounce>>,
    submits: Mutex<Vec<CapturedSubmit>>,
}

impl CaptureLog {
    pub fn captured_announces(&self) -> Vec<CapturedAnnounce> {
        self.inner.announces.lock().unwrap().clone()
    }

    pub fn captured_submits(&self) -> Vec<CapturedSubmit> {
        self.inner.submits.lock().unwrap().clone()
    }

    fn record_announce(&self, a: CapturedAnnounce) {
        self.inner.announces.lock().unwrap().push(a);
    }

    fn record_submit(&self, s: CapturedSubmit) {
        self.inner.submits.lock().unwrap().push(s);
    }
}

/// `Submitter` impl that returns pinned results and records every
/// `announce_ticket` / `submit` call for later inspection. Captures
/// live in a clonable `CaptureLog` so the caller keeps a read handle
/// even after the submitter moves behind `Box<dyn Submitter>`.
pub struct InProcessSubmitter {
    announce_result: AnnounceTicketResult,
    submit_result: SubmitResult,
    capture: CaptureLog,
}

impl InProcessSubmitter {
    pub fn new(announce_result: AnnounceTicketResult, submit_result: SubmitResult) -> Self {
        Self {
            announce_result,
            submit_result,
            capture: CaptureLog::default(),
        }
    }

    /// Hand out a clonable handle onto the capture log. The submitter
    /// keeps its own handle; both halves see every recorded call.
    pub fn capture_log(&self) -> CaptureLog {
        self.capture.clone()
    }

    pub fn captured_announces(&self) -> Vec<CapturedAnnounce> {
        self.capture.captured_announces()
    }

    pub fn captured_submits(&self) -> Vec<CapturedSubmit> {
        self.capture.captured_submits()
    }
}

impl Submitter for InProcessSubmitter {
    fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        self.capture.record_announce(CapturedAnnounce {
            c_hex: inputs.c_hex.to_string(),
            pk_hex: inputs.pk_hex.to_string(),
            n_hex: inputs.n_hex.to_string(),
        });
        self.announce_result.clone()
    }

    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        self.capture.record_submit(CapturedSubmit {
            c_hex: inputs.c_hex.to_string(),
            pk_hex: inputs.pk_hex.to_string(),
            n_hex: inputs.n_hex.to_string(),
            j_hex: inputs.j_hex.to_string(),
            nonce_s_hex: inputs.nonce_s_hex.to_string(),
            canon_bytes: inputs.canon_bytes.to_vec(),
        });
        self.submit_result.clone()
    }
}

/// Inputs that fully describe an in-process mining-deps composition.
/// `prompt_builder`, `log`, and `sleeper` are intentionally omitted —
/// they stay `None` on the produced `MiningLoopDeps` and the caller
/// (the future `boole.mine` tool, slice 50+) decides whether to wire
/// them.
pub struct InProcessMiningInputs {
    pub pk: Hex32,
    pub head: ChainHead,
    pub announce_result: AnnounceTicketResult,
    pub submit_result: SubmitResult,
    pub emitter: Box<dyn TargetEmitter>,
    pub driver: Box<dyn ProverDriver>,
    pub verifier: Box<dyn Verifier>,
    pub canonicalizer: Box<dyn Canonicalizer>,
}

/// Bundle of `MiningLoopDeps` ready for `run_mining_loop` and a
/// clonable `CaptureLog` the caller retains for inspection.
pub struct InProcessMiningBundle {
    pub deps: MiningLoopDeps,
    pub capture: CaptureLog,
}

/// Compose `InProcessChainHead` + `InProcessSubmitter` + the
/// caller-injected heavy collaborators into a single `MiningLoopDeps`
/// the future `boole.mine` tool can hand straight to
/// `boole_miner::run_mining_loop`.
pub fn build_in_process_mining_deps(inputs: InProcessMiningInputs) -> InProcessMiningBundle {
    let submitter = InProcessSubmitter::new(inputs.announce_result, inputs.submit_result);
    let capture = submitter.capture_log();
    let deps = MiningLoopDeps {
        pk: inputs.pk,
        chain_head: Box::new(InProcessChainHead::new(inputs.head)),
        emitter: inputs.emitter,
        driver: inputs.driver,
        verifier: inputs.verifier,
        canonicalizer: inputs.canonicalizer,
        submit_client: Box::new(submitter),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    InProcessMiningBundle { deps, capture }
}
