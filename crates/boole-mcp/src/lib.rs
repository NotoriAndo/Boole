//! P2.1 closure — in-process trait impls that let `boole-mcp` drive
//! `boole-miner`'s mining loop without an HTTP loopback to
//! `boole-node`.
//!
//! Slice 47 landed `InProcessChainHead` (`ChainHeadFetcher`).
//! Slice 48 lands `InProcessSubmitter` (`Submitter`). The full
//! mining round-trip glue and the `boole.mine` / `boole.status`
//! tools ride on follow-up slices.

use std::sync::Mutex;

use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, ChainHead, ChainHeadError, ChainHeadFetcher,
    SubmitInputs, SubmitResult, Submitter,
};

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

/// `Submitter` impl that returns pinned results and records every
/// `announce_ticket` / `submit` call for later inspection. The
/// captures let boole-mcp's mining tools assert exactly which shares
/// and blocks the miner emitted without standing up an HTTP listener.
pub struct InProcessSubmitter {
    announce_result: AnnounceTicketResult,
    submit_result: SubmitResult,
    captured_announces: Mutex<Vec<CapturedAnnounce>>,
    captured_submits: Mutex<Vec<CapturedSubmit>>,
}

impl InProcessSubmitter {
    pub fn new(announce_result: AnnounceTicketResult, submit_result: SubmitResult) -> Self {
        Self {
            announce_result,
            submit_result,
            captured_announces: Mutex::new(Vec::new()),
            captured_submits: Mutex::new(Vec::new()),
        }
    }

    pub fn captured_announces(&self) -> Vec<CapturedAnnounce> {
        self.captured_announces.lock().unwrap().clone()
    }

    pub fn captured_submits(&self) -> Vec<CapturedSubmit> {
        self.captured_submits.lock().unwrap().clone()
    }
}

impl Submitter for InProcessSubmitter {
    fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        self.captured_announces
            .lock()
            .unwrap()
            .push(CapturedAnnounce {
                c_hex: inputs.c_hex.to_string(),
                pk_hex: inputs.pk_hex.to_string(),
                n_hex: inputs.n_hex.to_string(),
            });
        self.announce_result.clone()
    }

    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        self.captured_submits.lock().unwrap().push(CapturedSubmit {
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
