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
///
/// v2: block_hash preimage v2 (ADR-0014 (a) / N5-pre.1) — the hash
/// commits every replay-consumed block field, not just
/// `prev_c ‖ share_hashes`.
///
/// v3: §SC reset window (ADR-0015 (d) / ADR-0016 (e)) — block_hash
/// preimage v3 (promoted bounty shares + reward committed, declared
/// credit rows removed; replay derives settlement), signed work payload
/// v2 (`boole.signer.work.v2` covers `rewardRecipient`), evidence v2
/// (`signedWork` authorization slot), family manifest `maxHeartbeats`/
/// `maxRecDepth` budget fields, genesis `familyManifestRoot` param.
pub const CONSENSUS_RULE_VERSION: u32 = 3;

/// §SC W1.a (ADR-0014 amendment 2026-07-11) — the min-share-score
/// multiplier is a Tier-2 rule constant, fixed at 1.0 (10⁹ nanos) for
/// every network running rule set v3. It was previously classified as
/// Tier-3 node-local ops config, which left the block's self-declared
/// `minShareScoreMultiplierNanos` bound only to its own arithmetic:
/// replay recomputed `minShareScore` FROM the declared multiplier, so a
/// proposer could move the share-score floor unilaterally and still
/// pass. Replay now rejects any evidence-bearing block whose declared
/// multiplier differs from this constant. Changing the value is a
/// consensus change: bump `CONSENSUS_RULE_VERSION`.
pub const MIN_SHARE_SCORE_MULTIPLIER_NANOS: u64 = 1_000_000_000;
