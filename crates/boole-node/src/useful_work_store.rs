//! BF.4 — durable useful-work event log + chain-rederived spent mirror.
//!
//! Append-only NDJSON log of registry/assignment/receipt/reward events
//! with an in-memory spent mirror (`task_id -> RewardRecord`). Reuses the
//! `FileProofDedupLedger` durability conventions: stable-prefix recovery
//! (a torn trailing line from a crash mid-write is truncated on disk),
//! durable appends (flush + fsync + parent-dir fsync on create), and an
//! atomic temp-file + rename swap for the reorg rebuild.
//!
//! ADR-0012 stance, same as the proof-dedup ledger: this store is an
//! ADMISSION EARLY-REJECT CACHE, never the consensus source of truth.
//! The "one reward per task" rule is ultimately enforced by block
//! commitment replay (BF.7); deleting this file weakens nothing but
//! early-reject latency. The reorg path therefore REPLACES the file from
//! the canonical chain's rewards — the mirror is a pure function of the
//! chain, so abandoned-fork rewards must not linger as spent.
//!
//! BF.4 rollback contract: nothing in the runtime opens this store while
//! `UsefulBaseMode` is `Disabled` (wiring lands with BF.6a/BF.7), so the
//! BF.0 "disabled boot creates no useful-base files" pin stays intact.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::durability::{
    append_ndjson_line_durable, read_stable_prefix, write_ndjson_lines_atomic,
};

/// Canonical on-disk file name for future state-dir wiring (BF.6a/BF.7).
pub const USEFUL_WORK_STORE_FILE: &str = "useful-work.ndjson";

/// One serialized line in the useful-work NDJSON log. Tagged so future
/// event kinds can be added without breaking older logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UsefulWorkEvent {
    #[serde(rename_all = "camelCase")]
    TaskRegistered { task_id: String },
    #[serde(rename_all = "camelCase")]
    TaskAssigned {
        task_id: String,
        ticket_id: String,
        epoch: u64,
    },
    #[serde(rename_all = "camelCase")]
    ReceiptRecorded {
        task_id: String,
        submission_id: String,
        artifact_root: String,
        verdict: String,
    },
    #[serde(rename_all = "camelCase")]
    RewardSettled {
        task_id: String,
        submission_id: String,
        reward_pk: String,
        epoch: u64,
    },
}

/// The spent-mirror value: who was paid for a task, exactly once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RewardRecord {
    pub task_id: String,
    pub submission_id: String,
    pub reward_pk: String,
    pub epoch: u64,
}

#[derive(Debug, Default)]
pub struct FileUsefulWorkStore {
    events_len: usize,
    rewarded: BTreeMap<String, RewardRecord>,
}

impl FileUsefulWorkStore {
    /// Build the in-memory mirror by replaying the NDJSON log at `path`.
    /// Returns an empty store if the file does not exist. A torn trailing
    /// line is truncated on disk (stable-prefix recovery); a duplicate
    /// reward INSIDE the stable prefix is a hard error — that log was
    /// written by a buggy or tampered process and recovery refuses to
    /// guess which payout is real.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut store = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: UsefulWorkEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("usefulWorkStore: line {} invalid JSON: {}", i + 1, err)
            })?;
            if let UsefulWorkEvent::RewardSettled { task_id, .. } = &event {
                if store.rewarded.contains_key(task_id) {
                    anyhow::bail!(
                        "usefulWorkStore: line {}: duplicate reward for task {}",
                        i + 1,
                        task_id
                    );
                }
            }
            store.apply(event);
        }
        Ok(store)
    }

    /// Reorg path — atomically REPLACE the log with the canonical chain's
    /// reward records and return the fresh mirror. Abandoned-fork rewards
    /// are dropped (the task is claimable again on the new chain); a crash
    /// mid-rebuild leaves either the old or the new file, never a splice.
    pub fn rebuild_from_rewards(
        path: impl AsRef<Path>,
        rewards: &[RewardRecord],
    ) -> anyhow::Result<Self> {
        let mut store = Self::default();
        let mut lines = Vec::with_capacity(rewards.len());
        for record in rewards {
            if store.rewarded.contains_key(&record.task_id) {
                anyhow::bail!(
                    "usefulWorkStore: canonical rewards contain a duplicate for task {}",
                    record.task_id
                );
            }
            let event = UsefulWorkEvent::RewardSettled {
                task_id: record.task_id.clone(),
                submission_id: record.submission_id.clone(),
                reward_pk: record.reward_pk.clone(),
                epoch: record.epoch,
            };
            lines.push(serde_json::to_string(&event)?);
            store.apply(event);
        }
        write_ndjson_lines_atomic(path.as_ref(), &lines)?;
        Ok(store)
    }

    /// Persist and apply one event. For `RewardSettled`, returns
    /// `Ok(false)` WITHOUT touching the log when the task already has a
    /// reward — a duplicate payout (including a reward-pk swap on the
    /// same task) must never reach disk; the caller rejects it as
    /// `duplicate_reward`. Every other event returns `Ok(true)` after a
    /// durable append.
    pub fn append_event(
        &mut self,
        path: impl AsRef<Path>,
        event: UsefulWorkEvent,
    ) -> anyhow::Result<bool> {
        if let UsefulWorkEvent::RewardSettled { task_id, .. } = &event {
            if self.rewarded.contains_key(task_id) {
                return Ok(false);
            }
        }
        let line = serde_json::to_string(&event)?;
        append_ndjson_line_durable(path.as_ref(), &line)?;
        self.apply(event);
        Ok(true)
    }

    pub fn is_task_rewarded(&self, task_id: &str) -> bool {
        self.rewarded.contains_key(task_id)
    }

    pub fn reward_record(&self, task_id: &str) -> Option<&RewardRecord> {
        self.rewarded.get(task_id)
    }

    pub fn events_len(&self) -> usize {
        self.events_len
    }

    fn apply(&mut self, event: UsefulWorkEvent) {
        if let UsefulWorkEvent::RewardSettled {
            task_id,
            submission_id,
            reward_pk,
            epoch,
        } = &event
        {
            self.rewarded.insert(
                task_id.clone(),
                RewardRecord {
                    task_id: task_id.clone(),
                    submission_id: submission_id.clone(),
                    reward_pk: reward_pk.clone(),
                    epoch: *epoch,
                },
            );
        }
        self.events_len += 1;
    }
}
