//! D1: devnet conventions, not production allocations.
pub const L2_CHAIN_ID: u64 = 720926;
pub const L1_CHAIN_ID: u64 = 31337;
pub const INBOX: &str = "0xb001000000000000000000000000000000000001";
pub const DEPOSITOR: &str = "0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001";
pub const L1_BLOCK: &str = "0x4200000000000000000000000000000000000015";
pub const POST_SELECTOR: &str = "0xbaf9b369";
pub const L1INFO_GAS: u64 = 1_000_000;

pub fn posted_topic() -> String {
    format!(
        "0x{}",
        hex::encode(crate::user_tx::keccak(b"Posted(bytes)"))
    )
}
