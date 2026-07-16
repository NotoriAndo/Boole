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

use std::path::Path;

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
}
