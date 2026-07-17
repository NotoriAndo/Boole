//! SC.10-iii — the node-local **verified-prefix checkpoint** store
//! (ADR-0016 (c)/(c-1)).
//!
//! The checkpoint records the highest block height through which THIS node
//! has itself Lean-re-verified the base-lane chain. Boot and restart skip
//! Lean re-verification below it (structural replay always runs in full);
//! initial sync verifies as it applies and advances it. It is node-local
//! performance state, never consensus data — a fresh node with no checkpoint
//! verifies from genesis and derives the identical head (invariant 3).
//!
//! ADR-0016 (c-1) pins its identity: a checkpoint written under one
//! toolchain/budget must never let a node skip re-verification across a
//! change. The record therefore carries, and boot MUST validate, all of:
//!   1. `genesis_spec_hash` — which chain identity it verified (this already
//!      transitively commits the checker pin and family-manifest root, both
//!      `GenesisParams` fields, but the checker hash is ALSO bound explicitly
//!      below as defense-in-depth per (c-1));
//!   2. `block_hash` at `height` — which prefix (a reorg below the checkpoint
//!      height changes this and invalidates it — validated against the store
//!      in SC.10-iii-c/-d, not here);
//!   3. `checker_artifact_hash` — which pinned checker produced the verdicts;
//!   4. the committed base-lane budget (`max_heartbeats`, `max_rec_depth`) it
//!      verified under.
//!
//! Any identity mismatch at boot ⇒ the checkpoint is discarded and Lean
//! re-verification resumes from genesis (safe: structural replay is
//! unaffected). Persistence uses the same atomic-file semantics as the block
//! store and reward ledger (temp → fsync → rename → dir-fsync), so a crash
//! during a checkpoint replacement leaves either the whole previous record or
//! the whole new one — never a torn mix that could mark an unverified prefix
//! as verified. A partial/corrupt file reads as ABSENT, which is safe.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::durability::{read_stable_prefix, write_ndjson_lines_atomic};

/// The persisted verified-prefix checkpoint record (ADR-0016 (c-1)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPrefixCheckpoint {
    /// Chain identity this node Lean-verified against (`GenesisSpec.hash()`).
    pub genesis_spec_hash: String,
    /// Highest height (block count) Lean-re-verified through, inclusive.
    pub height: u64,
    /// Block hash at `height` — the specific prefix this checkpoint trusts.
    pub block_hash: String,
    /// The pinned checker whose verdicts produced this checkpoint.
    pub checker_artifact_hash: String,
    /// Committed base-lane step budget the verdicts ran under.
    pub max_heartbeats: u64,
    /// Committed base-lane recursion-depth budget the verdicts ran under.
    pub max_rec_depth: u64,
}

/// The runtime identity a boot resolves and matches a persisted checkpoint
/// against. `block_hash` is validated separately against the on-disk chain
/// (the store knows its own block at `height`), so it is not part of this
/// identity tuple.
#[derive(Debug, Clone, Copy)]
pub struct CheckpointIdentity<'a> {
    pub genesis_spec_hash: &'a str,
    pub checker_artifact_hash: &'a str,
    pub max_heartbeats: u64,
    pub max_rec_depth: u64,
}

impl VerifiedPrefixCheckpoint {
    /// True iff this record was verified under the given runtime identity.
    /// A single differing field ⇒ the checkpoint must be discarded at boot
    /// (ADR-0016 (c-1)): skipping re-verification across a
    /// genesis/checker/budget change is exactly what the binding forbids.
    pub fn identity_matches(&self, identity: &CheckpointIdentity<'_>) -> bool {
        self.genesis_spec_hash == identity.genesis_spec_hash
            && self.checker_artifact_hash == identity.checker_artifact_hash
            && self.max_heartbeats == identity.max_heartbeats
            && self.max_rec_depth == identity.max_rec_depth
    }
}

/// The checkpoint file path for a node whose block store is at `block_path`.
/// A sibling of the block store (`<stem>.checkpoint.json`), so the checkpoint
/// lives beside the exact chain it certifies and is scoped per store path
/// (matching how the reward ledger is a sibling path).
pub fn checkpoint_path_for(block_path: &Path) -> PathBuf {
    block_path.with_extension("checkpoint.json")
}

/// Build the checkpoint a verified commit at `height` / `block_hash` should
/// persist, or `None` when the node is not checker-pinned.
///
/// `checker_artifact_hash` is `Some` only on a checker-pinned named network —
/// the only lane where the node produces Lean verdicts. A closed-local /
/// no-checker node ran no checker, so it has no verified prefix to record and
/// this returns `None` (the caller writes nothing, keeping pre-SC.10
/// behaviour). ADR-0016 (c): the checkpoint records what THIS node itself
/// Lean-re-verified.
pub fn build_verified_checkpoint(
    genesis_spec_hash: &str,
    checker_artifact_hash: Option<&str>,
    max_heartbeats: u64,
    max_rec_depth: u64,
    height: u64,
    block_hash: &str,
) -> Option<VerifiedPrefixCheckpoint> {
    Some(VerifiedPrefixCheckpoint {
        genesis_spec_hash: genesis_spec_hash.to_string(),
        height,
        block_hash: block_hash.to_string(),
        checker_artifact_hash: checker_artifact_hash?.to_string(),
        max_heartbeats,
        max_rec_depth,
    })
}

/// Atomically persist `checkpoint` to `path` (whole-or-nothing replace).
///
/// The caller MUST have durably written the block/chain the checkpoint
/// refers to FIRST (ADR-0016 (c-1) update order: `Lean success → chain
/// durable write → checkpoint durable write`). This function only guarantees
/// the checkpoint write itself is atomic.
pub fn write_checkpoint(path: &Path, checkpoint: &VerifiedPrefixCheckpoint) -> anyhow::Result<()> {
    let line = serde_json::to_string(checkpoint)?;
    write_ndjson_lines_atomic(path, &[line])
}

/// Read the checkpoint at `path`, returning `None` when there is no usable
/// record: the file is absent, empty, torn (crash mid-write), or does not
/// parse. Every not-`Some` outcome is safe — boot falls back to verifying
/// from genesis. Identity validation against the runtime is the caller's job
/// (`identity_matches` + the on-disk block hash at `height`).
pub fn read_checkpoint(path: &Path) -> anyhow::Result<Option<VerifiedPrefixCheckpoint>> {
    let Some(raw) = read_stable_prefix(path)? else {
        return Ok(None);
    };
    let line = raw.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Ok(None);
    }
    // A corrupt/partial record is treated as absent (never an error): the
    // node simply re-verifies from genesis, exactly like a fresh node.
    Ok(serde_json::from_str(line).ok())
}

/// SC.10-iii-c — whether a persisted checkpoint survives a boot.
///
/// It survives iff its identity matches this boot's (genesis / checker /
/// budget) AND — when the on-disk chain already contains the block at the
/// checkpoint height — that block's hash matches the checkpoint's. When the
/// chain is SHORTER than the checkpoint height (a re-bootstrap with a wiped
/// store), the block-hash check is deferred to sync (SC.10-iii-c-2), so
/// `block_hash_at_height` is `None` and the checkpoint survives on identity
/// alone. Any mismatch ⇒ the checkpoint must be discarded, so it can never
/// let a later re-verification be skipped across a genesis/checker/budget
/// change or onto a different prefix (ADR-0016 (c-1)).
pub fn checkpoint_survives_boot(
    checkpoint: &VerifiedPrefixCheckpoint,
    identity: &CheckpointIdentity<'_>,
    block_hash_at_height: Option<&str>,
) -> bool {
    checkpoint.identity_matches(identity)
        && block_hash_at_height.is_none_or(|hash| hash == checkpoint.block_hash)
}

/// SC.10-iii-c — read the checkpoint at `path` and DELETE it if it does not
/// survive this boot (identity/prefix mismatch, or the node is no longer
/// checker-pinned so `identity` is `None`). Returns the surviving checkpoint,
/// or `None` when it was absent or discarded.
///
/// Discarding a stale checkpoint is always safe: the node then re-verifies
/// from genesis exactly like a fresh node. A missing file is a no-op.
pub fn validate_or_discard_checkpoint_at_boot(
    path: &Path,
    identity: Option<&CheckpointIdentity<'_>>,
    block_hash_at_height: Option<&str>,
) -> anyhow::Result<Option<VerifiedPrefixCheckpoint>> {
    let Some(checkpoint) = read_checkpoint(path)? else {
        return Ok(None);
    };
    let survives =
        identity.is_some_and(|id| checkpoint_survives_boot(&checkpoint, id, block_hash_at_height));
    if survives {
        return Ok(Some(checkpoint));
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(None)
}

/// SC.10-iii-c-2 — what a checker-pinned node should do about the pinned-checker
/// re-verify for a block it is about to ingest, given its verified-prefix
/// checkpoint. This is the Bitcoin `assumevalid` shape: below a checkpoint the
/// operator has already verified, the (expensive) Lean re-verify is skipped;
/// everything at or above it is fully re-verified. Structural replay ALWAYS
/// runs regardless — only the Lean step is affected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointSkipDecision {
    /// The block lies strictly within the trusted verified prefix (its post-
    /// commit height is below the checkpoint height, or it IS the checkpoint
    /// block and its hash matches). Skip the Lean re-verify and adopt.
    SkipReverify,
    /// No checkpoint covers this block (none present, or the block is at or
    /// above the checkpoint height and not the checkpoint block). Run the
    /// normal Lean re-verify.
    RunReverify,
    /// The block completes the checkpoint height but its hash does NOT match
    /// the checkpoint's prefix: the re-synced chain diverges from what this
    /// node verified. Discard the checkpoint, then run the normal Lean
    /// re-verify (nothing above a divergence may be skipped).
    DivergedDiscardThenReverify,
}

/// Decide the re-verify action for `block` (identified by its post-commit
/// height `block_height + 1` and head hash `block_c`) under `checkpoint`.
///
/// `block_height` is the block's own height field; `new_height = block_height
/// + 1` is the chain length after committing it, which is what a checkpoint
/// height counts. The checkpoint block is the one whose commit brings the
/// chain to exactly `checkpoint.height`, and its head hash is
/// `checkpoint.block_hash`.
pub fn checkpoint_skip_decision(
    checkpoint: Option<&VerifiedPrefixCheckpoint>,
    block_height: u64,
    block_c: &str,
) -> CheckpointSkipDecision {
    let Some(checkpoint) = checkpoint else {
        return CheckpointSkipDecision::RunReverify;
    };
    let new_height = block_height + 1;
    if new_height < checkpoint.height {
        // Strictly below the checkpoint block: structural linkage (enforced by
        // replay) ties this block to the checkpoint block whose hash is
        // verified below, so it is within the trusted prefix.
        CheckpointSkipDecision::SkipReverify
    } else if new_height == checkpoint.height {
        if block_c == checkpoint.block_hash {
            CheckpointSkipDecision::SkipReverify
        } else {
            CheckpointSkipDecision::DivergedDiscardThenReverify
        }
    } else {
        CheckpointSkipDecision::RunReverify
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VerifiedPrefixCheckpoint {
        VerifiedPrefixCheckpoint {
            genesis_spec_hash: "genesis-abc".to_string(),
            height: 7,
            block_hash: "block-hash-at-7".to_string(),
            checker_artifact_hash: "checker-1dd3055a".to_string(),
            max_heartbeats: 400_000,
            max_rec_depth: 512,
        }
    }

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-checkpoint-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn checkpoint_round_trips_through_atomic_store() {
        let dir = scratch_dir("roundtrip");
        let path = dir.join("checkpoint.json");
        let cp = sample();
        write_checkpoint(&path, &cp).expect("write");
        let read = read_checkpoint(&path).expect("read").expect("present");
        assert_eq!(read, cp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_checkpoint_returns_none_for_missing_file() {
        let dir = scratch_dir("missing");
        let path = dir.join("nope.json");
        assert!(read_checkpoint(&path).expect("read").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_or_partial_checkpoint_reads_as_absent() {
        // A torn write (crash mid-`write`) or garbage bytes must never parse
        // as a valid checkpoint — that is exactly the "never mark an
        // unverified prefix as verified" guarantee at the store level.
        let dir = scratch_dir("corrupt");
        let path = dir.join("checkpoint.json");
        std::fs::write(&path, b"{\"genesis_spec_hash\":\"gen").expect("torn write");
        assert!(
            read_checkpoint(&path).expect("read").is_none(),
            "a torn/partial checkpoint must read as absent"
        );
        std::fs::write(&path, b"not json at all\n").expect("garbage write");
        assert!(
            read_checkpoint(&path).expect("read").is_none(),
            "non-JSON garbage must read as absent"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_replace_yields_only_the_new_record_never_a_mix() {
        // checkpoint_advances_only_after_verified_block_is_durable (store
        // level): replacing an older checkpoint with a newer one is an atomic
        // swap — a reader sees either the whole old record or the whole new
        // one, never a spliced mix of the two heights.
        let dir = scratch_dir("replace");
        let path = dir.join("checkpoint.json");
        let mut old = sample();
        old.height = 3;
        old.block_hash = "block-hash-at-3".to_string();
        write_checkpoint(&path, &old).expect("write old");

        let mut new = sample();
        new.height = 9;
        new.block_hash = "block-hash-at-9".to_string();
        write_checkpoint(&path, &new).expect("write new");

        let read = read_checkpoint(&path).expect("read").expect("present");
        assert_eq!(read, new, "read must be exactly the new record");
        assert_eq!(read.height, 9);
        assert_eq!(read.block_hash, "block-hash-at-9");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_path_is_a_sibling_of_the_block_store() {
        let p = checkpoint_path_for(std::path::Path::new("/data/node1-blocks.ndjson"));
        assert_eq!(
            p,
            std::path::PathBuf::from("/data/node1-blocks.checkpoint.json")
        );
        // No extension on the store path still yields a sibling checkpoint.
        let p2 = checkpoint_path_for(std::path::Path::new("/data/blocks"));
        assert_eq!(p2, std::path::PathBuf::from("/data/blocks.checkpoint.json"));
    }

    #[test]
    fn build_verified_checkpoint_is_none_without_a_checker_pin() {
        // A closed-local / no-checker node produced no Lean verdicts, so there
        // is no verified prefix to record.
        assert!(build_verified_checkpoint("gen", None, 400_000, 512, 5, "head-hash").is_none());
    }

    #[test]
    fn build_verified_checkpoint_carries_the_head_and_identity() {
        let cp = build_verified_checkpoint(
            "genesis-abc",
            Some("checker-1dd3055a"),
            400_000,
            512,
            9,
            "block-hash-at-9",
        )
        .expect("checker-pinned node records a checkpoint");
        assert_eq!(
            cp,
            VerifiedPrefixCheckpoint {
                genesis_spec_hash: "genesis-abc".to_string(),
                height: 9,
                block_hash: "block-hash-at-9".to_string(),
                checker_artifact_hash: "checker-1dd3055a".to_string(),
                max_heartbeats: 400_000,
                max_rec_depth: 512,
            }
        );
    }

    #[test]
    fn identity_matches_accepts_the_exact_tuple() {
        let cp = sample();
        let id = CheckpointIdentity {
            genesis_spec_hash: "genesis-abc",
            checker_artifact_hash: "checker-1dd3055a",
            max_heartbeats: 400_000,
            max_rec_depth: 512,
        };
        assert!(cp.identity_matches(&id));
    }

    #[test]
    fn identity_matches_rejects_any_single_field_change() {
        let cp = sample();
        let base = CheckpointIdentity {
            genesis_spec_hash: "genesis-abc",
            checker_artifact_hash: "checker-1dd3055a",
            max_heartbeats: 400_000,
            max_rec_depth: 512,
        };
        assert!(!cp.identity_matches(&CheckpointIdentity {
            genesis_spec_hash: "genesis-DIFFERENT",
            ..base
        }));
        assert!(!cp.identity_matches(&CheckpointIdentity {
            checker_artifact_hash: "checker-DIFFERENT",
            ..base
        }));
        assert!(!cp.identity_matches(&CheckpointIdentity {
            max_heartbeats: 1,
            ..base
        }));
        assert!(!cp.identity_matches(&CheckpointIdentity {
            max_rec_depth: 1,
            ..base
        }));
    }

    fn matching_identity() -> CheckpointIdentity<'static> {
        CheckpointIdentity {
            genesis_spec_hash: "genesis-abc",
            checker_artifact_hash: "checker-1dd3055a",
            max_heartbeats: 400_000,
            max_rec_depth: 512,
        }
    }

    #[test]
    fn checkpoint_survives_boot_accepts_matching_identity_and_prefix() {
        let cp = sample();
        assert!(checkpoint_survives_boot(
            &cp,
            &matching_identity(),
            Some("block-hash-at-7")
        ));
    }

    #[test]
    fn checkpoint_survives_boot_defers_prefix_check_when_chain_is_shorter() {
        // Re-bootstrap (wiped store): the chain has no block at the checkpoint
        // height yet, so the block-hash check is deferred to sync — identity
        // alone keeps the checkpoint.
        let cp = sample();
        assert!(checkpoint_survives_boot(&cp, &matching_identity(), None));
    }

    #[test]
    fn checkpoint_survives_boot_rejects_a_prefix_mismatch() {
        let cp = sample();
        assert!(!checkpoint_survives_boot(
            &cp,
            &matching_identity(),
            Some("a-different-block-hash")
        ));
    }

    #[test]
    fn checkpoint_survives_boot_rejects_an_identity_mismatch() {
        let cp = sample();
        let stale = CheckpointIdentity {
            genesis_spec_hash: "genesis-DIFFERENT",
            ..matching_identity()
        };
        // Even with a matching prefix hash, a stale identity fails.
        assert!(!checkpoint_survives_boot(
            &cp,
            &stale,
            Some("block-hash-at-7")
        ));
    }

    #[test]
    fn validate_or_discard_keeps_a_matching_checkpoint() {
        let dir = scratch_dir("keep");
        let path = dir.join("checkpoint.json");
        let cp = sample();
        write_checkpoint(&path, &cp).expect("write");
        let kept = validate_or_discard_checkpoint_at_boot(
            &path,
            Some(&matching_identity()),
            Some("block-hash-at-7"),
        )
        .expect("validate")
        .expect("kept");
        assert_eq!(kept, cp);
        assert!(path.exists(), "a surviving checkpoint stays on disk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_or_discard_deletes_a_stale_or_diverged_checkpoint() {
        for (tag, identity, block_hash) in [
            (
                "genesis",
                Some(CheckpointIdentity {
                    genesis_spec_hash: "genesis-DIFFERENT",
                    ..matching_identity()
                }),
                Some("block-hash-at-7"),
            ),
            (
                "budget",
                Some(CheckpointIdentity {
                    max_heartbeats: 1,
                    ..matching_identity()
                }),
                Some("block-hash-at-7"),
            ),
            ("prefix", Some(matching_identity()), Some("other-hash")),
            // No checker pinned now ⇒ any checkpoint is stale.
            ("no-checker", None, Some("block-hash-at-7")),
        ] {
            let dir = scratch_dir(&format!("discard-{tag}"));
            let path = dir.join("checkpoint.json");
            write_checkpoint(&path, &sample()).expect("write");
            let out = validate_or_discard_checkpoint_at_boot(&path, identity.as_ref(), block_hash)
                .expect("validate");
            assert!(out.is_none(), "{tag}: stale checkpoint must be discarded");
            assert!(
                !path.exists(),
                "{tag}: discarded checkpoint must be deleted"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn validate_or_discard_is_a_noop_when_absent() {
        let dir = scratch_dir("absent");
        let path = dir.join("nope.json");
        let out = validate_or_discard_checkpoint_at_boot(&path, Some(&matching_identity()), None)
            .expect("validate");
        assert!(out.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skip_decision_runs_reverify_when_no_checkpoint() {
        assert_eq!(
            checkpoint_skip_decision(None, 0, "any"),
            CheckpointSkipDecision::RunReverify
        );
    }

    #[test]
    fn skip_decision_skips_below_the_checkpoint_height() {
        // sample() has height 7: a block whose post-commit height (2+1=3) is
        // below 7 is within the trusted prefix.
        let cp = sample();
        assert_eq!(
            checkpoint_skip_decision(Some(&cp), 2, "irrelevant-below-hash"),
            CheckpointSkipDecision::SkipReverify
        );
    }

    #[test]
    fn skip_decision_skips_the_checkpoint_block_when_its_hash_matches() {
        // block_height 6 -> new_height 7 == checkpoint height; matching hash.
        let cp = sample();
        assert_eq!(
            checkpoint_skip_decision(Some(&cp), 6, "block-hash-at-7"),
            CheckpointSkipDecision::SkipReverify
        );
    }

    #[test]
    fn skip_decision_diverges_when_the_checkpoint_block_hash_differs() {
        // block_height 6 -> new_height 7 == checkpoint height; WRONG hash: the
        // re-synced chain reached a different prefix tip.
        let cp = sample();
        assert_eq!(
            checkpoint_skip_decision(Some(&cp), 6, "a-different-tip-hash"),
            CheckpointSkipDecision::DivergedDiscardThenReverify
        );
    }

    #[test]
    fn skip_decision_runs_reverify_above_the_checkpoint() {
        // block_height 7 -> new_height 8 > checkpoint height 7.
        let cp = sample();
        assert_eq!(
            checkpoint_skip_decision(Some(&cp), 7, "block-hash-at-7"),
            CheckpointSkipDecision::RunReverify
        );
    }
}
