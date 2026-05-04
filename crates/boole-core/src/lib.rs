//! Core protocol types and deterministic state transition logic for Boole.
//!
//! This crate is intentionally small at first. It will grow by matching
//! TypeScript-generated golden fixtures from `/Users/seoyong/projects/pof`.

pub mod block;
pub mod hash;
pub mod replay;

pub use block::PersistedBlock;
pub use hash::{block_hash, h_protocol, Hex32};
pub use replay::{
    compute_block_credits, replay_blocks, PersistedCredit, PersistedRewardEvent, ReplayResult,
};

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
