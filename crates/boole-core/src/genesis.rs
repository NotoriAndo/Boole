//! N5.1 (ADR-0014) — content-addressed genesis.
//!
//! `GenesisSpec` is the single declaration a named network launches from:
//! the network id, every Tier-1 consensus parameter (ADR-0014 (c)), and
//! the initial chain state. Its BLAKE3 hash over the canonical-JSON
//! encoding is the network's identity — equal specs hash equal, and any
//! parameter change diverges (`any_param_change_changes_genesis_hash`),
//! which is what makes post-genesis rule changes hard forks.
//!
//! The chain anchor lives INSIDE the spec (`initial_state.genesis_c`, the
//! `prev_c` block 0 links to) rather than being the spec hash itself: the
//! spec hash is the identity peers/boot compare (N5.2), while the anchor
//! stays a committed field — so adopting the spec did not require another
//! chain reset after the N5-pre.1 preimage-v2 reset.

use serde::{Deserialize, Serialize};

use crate::canonical_json::canonicalize;
use crate::difficulty::DifficultyRetargetPolicy;
use crate::hash::Hex32;

/// Tier-1 consensus parameters (ADR-0014 (c)): the network-varying values
/// replay/block-validation reads. Tier-2 rule constants (seed j-index
/// bound, MTP window, walk depth) stay code constants under
/// `CONSENSUS_RULE_VERSION`; node-local ops knobs (admission thresholds,
/// pool caps, rate limits) stay out entirely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisParams {
    /// The rule set this network launched with (`rules.rs`).
    pub consensus_rule_version: u32,
    /// Initial block target (hex `BigUint`), height-0 difficulty.
    pub t_block: String,
    /// Initial share target (hex `BigUint`).
    pub t_share: String,
    /// Consensus cap on selected shares per block.
    pub k_max: u64,
    /// Difficulty retarget schedule; `None` = static difficulty.
    pub retarget: Option<DifficultyRetargetPolicy>,
    /// ADR-0014 (d): when true, an empty `seedHex` is rejected from
    /// height 0 (no legacy acceptance) — required on the shared testnet.
    pub seed_binding_required: bool,
    /// ADR-0014 (e): when pinned, the network trusts exactly this checker
    /// artifact (toolchain pin included); `None` = unpinned dev network.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checker_artifact_hash: Option<String>,
}

/// The state every participant starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisInitialState {
    /// The chain anchor: block 0's `prev_c` (64-char lowercase hex).
    pub genesis_c: String,
}

/// A named network's complete launch declaration. `hash()` is its
/// content-addressed identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisSpec {
    pub network_id: String,
    pub params: GenesisParams,
    pub initial_state: GenesisInitialState,
}

impl GenesisSpec {
    /// BLAKE3 over `"genesis.v1" ‖ canonical-JSON(self)` — deterministic
    /// across key order and serialization details (the canonical encoding
    /// sorts object keys), sensitive to every committed field.
    pub fn hash(&self) -> Hex32 {
        let value = serde_json::to_value(self).expect("GenesisSpec serializes");
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"genesis.v1");
        hasher.update(&canonicalize(&value));
        Hex32::from_bytes(*hasher.finalize().as_bytes())
    }
}
