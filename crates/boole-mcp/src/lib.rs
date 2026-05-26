//! P2.1 closure — in-process trait impls that let `boole-mcp` drive
//! `boole-miner`'s mining loop without an HTTP loopback to
//! `boole-node`.
//!
//! Slice 47 lands the first piece: an `InProcessChainHead` that pins
//! a single `ChainHead` and serves `ChainHeadFetcher::fetch_head`
//! from process memory. Follow-up slices add the matching
//! `InProcessSubmitter` and the full mining round-trip glue.

use boole_miner::{ChainHead, ChainHeadError, ChainHeadFetcher};

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
