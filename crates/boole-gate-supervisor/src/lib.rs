//! Node-local gate execution. This crate is never linked by the pure fold.
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
pub mod codec;
pub mod rebuild;
pub mod stub;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateInput {
    pub chain_id: u64,
    pub problem_id: Hash,
    pub commit_hash: Hash,
    pub solver: [u8; 20],
    pub env_id: Hash,
    pub export_digest: Hash,
    pub lane: u8,
    pub leaf_kind: u8,
    pub instance_seed: Hash,
    pub fuel_slot: u64,
    pub raw: Vec<u8>,
    pub package: Vec<u8>,
    pub artifacts: Vec<Vec<u8>>,
    pub early: Option<(u8, u16)>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateResult {
    pub outcome: u8,
    pub reason: u16,
    pub fuel: u64,
    pub statement_id: Hash,
    pub ext: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", deny_unknown_fields)]
pub enum GateReply {
    Result { value: GateResult },
    Defer { class: String },
}
pub mod cache;
pub mod process;
pub mod supervisor;

pub mod raw_frame;

/// Build-independent adapter source identity, committed alongside release binaries.
pub fn implementation_digest() -> Hash {
    use sha2::{Digest, Sha256};
    let sources: [(&str, &[u8]); 8] = [
        ("lib.rs", include_bytes!("lib.rs")),
        ("stub.rs", include_bytes!("stub.rs")),
        ("process.rs", include_bytes!("process.rs")),
        ("codec.rs", include_bytes!("codec.rs")),
        ("raw_frame.rs", include_bytes!("raw_frame.rs")),
        ("rebuild.rs", include_bytes!("rebuild.rs")),
        ("supervisor.rs", include_bytes!("supervisor.rs")),
        ("cache.rs", include_bytes!("cache.rs")),
    ];
    let mut h = Sha256::new();
    h.update(b"BOOLE-M3/ADAPTER-SOURCE/v1");
    for (name, bytes) in sources {
        h.update((name.len() as u16).to_be_bytes());
        h.update(name.as_bytes());
        h.update((bytes.len() as u64).to_be_bytes());
        h.update(bytes);
    }
    h.finalize().into()
}
