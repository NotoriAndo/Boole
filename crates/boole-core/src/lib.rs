//! Core protocol types and deterministic state transition logic for Boole.
//!
//! This crate is intentionally small at first. It will grow by matching
//! TypeScript-generated golden fixtures from `/Users/seoyong/projects/pof`.

pub mod block;
pub mod block_builder;
pub mod hash;
pub mod replay;
pub mod share_pool;

pub use block::PersistedBlock;
pub use block_builder::{
    build_block_selection, BlockBuilderConfig, BuildSelectionResult, BuiltBlockSelection,
    CandidateShare,
};
pub use hash::{
    block_hash, difficulty_weight, digest_to_biguint, h_protocol, min_share_score,
    parse_biguint_hex, share_hash, share_score, submission_pow_hash, submission_pow_ok, ticket,
    Hex32, TicketResult,
};
pub use replay::{
    compute_block_credits, replay_blocks, PersistedCredit, PersistedRewardEvent, ReplayResult,
};
pub use share_pool::{AcceptResult, PoolShare, SharePool};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex32_roundtrip_accepts_lowercase_32_bytes() {
        let input = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let parsed = Hex32::from_hex(input).expect("valid hex32");
        assert_eq!(parsed.to_hex(), input);
    }
}
