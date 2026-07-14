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
    /// ADR-0015 (c): BLAKE3 root over the sorted, canonical-JSON family
    /// manifest set this network settles bounties against. When pinned, a
    /// named-network node whose local manifest set derives a different
    /// root refuses to boot (SC.2 lands that enforcement); `None` =
    /// unpinned dev network. `operator_signer_pks` is admission
    /// convenience, not consensus — this root is the family authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_manifest_root: Option<String>,
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

/// N5.2 (ADR-0014) — the compiled-in named networks. Booting under one of
/// these names binds the node to the preset: a diverging effective genesis
/// refuses to boot instead of silently forking the network. Unlisted
/// network ids (the `boole-mvp` closed-local default) stay scenario-driven.
///
/// `boole-dev` mirrors the standard runtime-smoke scenario (relaxed: no
/// seed requirement, unpinned checker, static difficulty).
/// `boole-testnet-2` is the shared-testnet declaration — instance-numbered
/// (the ADR-0015 (d) reset window retired `-1`; the ADR-0014
/// "boole-testnet" name is the LINE, and the plain label also predates
/// N5.2 as the P2.10 signing network id, which stays scenario-driven):
/// seed binding required from height 0 (ADR-0014 (d)) and retargeting on;
/// its checker pin is `Some(<hash>)` since SC.9b (ADR-0016 (a) — the
/// ADR-0014 (e) deferral resolved): the value mirrors the canonical
/// `lean/checker` artifact hash pinned in `lean/checker/README.md` and
/// declared by the release channel (`RELEASE-MANIFEST.json` + `SHA256SUMS`);
/// `preset_pin_matches_released_checker_toolchain_manifest` enforces the
/// agreement. `family_manifest_root` stays `None` until SC.2 pins the
/// launch manifest set — before the testnet launch.
pub fn network_genesis_preset(network_id: &str) -> Option<GenesisSpec> {
    let all_zero_anchor = "0".repeat(64);
    let t_max = format!("0x{}", "f".repeat(64));
    let t_eased = format!("0x{}e", "f".repeat(63));
    match network_id {
        "boole-dev" => Some(GenesisSpec {
            network_id: "boole-dev".to_string(),
            params: GenesisParams {
                consensus_rule_version: crate::rules::CONSENSUS_RULE_VERSION,
                t_block: t_eased,
                t_share: t_max,
                k_max: 4,
                retarget: None,
                seed_binding_required: false,
                checker_artifact_hash: None,
                family_manifest_root: None,
            },
            initial_state: GenesisInitialState {
                genesis_c: all_zero_anchor,
            },
        }),
        "boole-testnet-2" => Some(GenesisSpec {
            network_id: "boole-testnet-2".to_string(),
            params: GenesisParams {
                consensus_rule_version: crate::rules::CONSENSUS_RULE_VERSION,
                t_block: t_eased,
                t_share: t_max,
                k_max: 4,
                retarget: Some(DifficultyRetargetPolicy {
                    target_block_ms: 60_000,
                    retarget_every_blocks: 2,
                    max_adjustment_factor: 4,
                }),
                seed_binding_required: true,
                // SC.9b — the canonical checker artifact (sources +
                // toolchain pin), see lean/checker/README.md.
                checker_artifact_hash: Some(
                    "1dd3055acb05142816f2082f0b3ad000c49513c3a2401572ec68703542042be1".to_string(),
                ),
                family_manifest_root: None,
            },
            initial_state: GenesisInitialState {
                genesis_c: all_zero_anchor,
            },
        }),
        _ => None,
    }
}
