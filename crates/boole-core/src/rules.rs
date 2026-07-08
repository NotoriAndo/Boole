//! Consensus rule-set identifier (ADR-0014 (b) / ADR-0009 amendment
//! 2026-07-08).
//!
//! `PROTOCOL_VERSION` (boole-p2p) versions the wire-frame schema only; this
//! constant identifies the block-validity rule set the binary enforces, so
//! two nodes can refuse to peer before silently forking on the same shares.
//! It lives in boole-core because it is a consensus identifier, not a wire
//! detail: the p2p layer carries it, the rules define it.

/// The consensus rule set this binary enforces. Any change to a block
/// validity rule (admission-independent: replay, fork-choice, dedup,
/// hash preimage, seed binding, ...) bumps this by one. Carried in the
/// p2p `Hello`; a mismatched peer is disconnected with a typed drop.
/// Also committed into `GenesisSpec.params` at N5.1 (the rule set a
/// network launched with).
pub const CONSENSUS_RULE_VERSION: u32 = 1;
