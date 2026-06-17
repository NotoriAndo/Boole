//! P2.1 closure (slice 47) — `InProcessChainHead` trait impl for the
//! `boole-miner` `ChainHeadFetcher` trait.
//!
//! Per master plan §6.5 P2.1 closure criterion 1: boole-mcp must drive
//! a full mining round-trip on a fixture testnet without HTTP loopback
//! to boole-node. The first piece of that is an in-process
//! `ChainHeadFetcher` impl that boole-mcp owns -- the mining loop can
//! then consume a head served by boole-mcp's own state instead of
//! reaching back over HTTP.
//!
//! This slice is intentionally narrow: just the chain-head impl with
//! its own focused gate. `InProcessSubmitter` and the full mining
//! round-trip ride on follow-up slices.

use std::sync::Arc;

use num_bigint::BigUint;

use boole_core::Hex32;
use boole_mcp::InProcessChainHead;
use boole_miner::{ChainHead, ChainHeadFetcher};

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
        difficulty_epoch: 0,
        mode: "static-calibrated".to_string(),
    }
}

#[test]
fn inprocess_chain_head_returns_pinned_head() {
    let head = fixture_head();
    let fetcher = InProcessChainHead::new(head.clone());
    let got = fetcher.fetch_head().expect("in-process fetch_head ok");
    assert_eq!(got.c, head.c);
    assert_eq!(got.t_ticket, head.t_ticket);
    assert_eq!(got.t_share, head.t_share);
    assert_eq!(got.t_block, head.t_block);
    assert_eq!(got.t_submit, head.t_submit);
    assert_eq!(got.min_share_score, head.min_share_score);
    assert_eq!(got.m, head.m);
    assert_eq!(got.d, head.d);
    assert_eq!(got.profile, head.profile);
    assert_eq!(got.n, head.n);
}

#[test]
fn inprocess_chain_head_is_trait_object_safe_via_arc_dyn() {
    // Mining loop will hold the fetcher behind `Arc<dyn ChainHeadFetcher>`,
    // so this test pins the trait-object cast at the public surface.
    let head = fixture_head();
    let fetcher: Arc<dyn ChainHeadFetcher> = Arc::new(InProcessChainHead::new(head.clone()));
    let got = fetcher.fetch_head().expect("trait-object fetch_head ok");
    assert_eq!(got.d, head.d);
    assert_eq!(got.profile, head.profile);
}
