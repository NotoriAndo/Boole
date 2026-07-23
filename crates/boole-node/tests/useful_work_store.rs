//! BF.4 — durable useful-work event log + chain-rederived spent mirror.
//!
//! The store is an ADMISSION EARLY-REJECT CACHE, never the source of
//! truth (same ADR-0012 stance as `FileProofDedupLedger`): the consensus
//! rule "one reward per task" will be enforced by block commitment
//! replay (BF.7); this file only buys early-reject latency. Recovery
//! reuses the stable-prefix pattern (torn trailing line truncated), and
//! the reorg path atomically REPLACES the file from the canonical
//! chain's rewards, dropping abandoned-fork entries.

use boole_node::{FileUsefulWorkStore, RewardRecord, UsefulWorkEvent};
use boole_testkit::rand_suffix;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn digest(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn tmp_store_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-bf4-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("useful-work.ndjson")
}

fn reward(task: u8, pk: u8) -> UsefulWorkEvent {
    UsefulWorkEvent::RewardSettled {
        task_id: digest(task),
        submission_id: digest(task.wrapping_add(0x10)),
        reward_pk: digest(pk),
        epoch: 7,
    }
}

#[test]
fn append_and_recover_round_trips() {
    let path = tmp_store_path();
    let mut store = FileUsefulWorkStore::recover(&path).expect("fresh store");
    assert_eq!(store.events_len(), 0);

    store
        .append_event(
            &path,
            UsefulWorkEvent::TaskRegistered {
                task_id: digest(0x01),
            },
        )
        .expect("append registered");
    store
        .append_event(
            &path,
            UsefulWorkEvent::TaskAssigned {
                task_id: digest(0x01),
                ticket_id: digest(0x02),
                epoch: 7,
            },
        )
        .expect("append assigned");
    store
        .append_event(
            &path,
            UsefulWorkEvent::ReceiptRecorded {
                task_id: digest(0x01),
                submission_id: digest(0x11),
                artifact_root: digest(0x21),
                verdict: "accepted".to_string(),
            },
        )
        .expect("append receipt");
    assert!(store
        .append_event(&path, reward(0x01, 0x31))
        .expect("append reward"));
    assert!(store.is_task_rewarded(&digest(0x01)));

    let recovered = FileUsefulWorkStore::recover(&path).expect("recover");
    assert_eq!(recovered.events_len(), 4);
    assert!(recovered.is_task_rewarded(&digest(0x01)));
    assert_eq!(
        recovered.reward_record(&digest(0x01)),
        store.reward_record(&digest(0x01)),
        "restart must rebuild the identical spent mirror"
    );
}

#[test]
fn torn_trailing_line_is_truncated_and_appendable() {
    let path = tmp_store_path();
    let mut store = FileUsefulWorkStore::recover(&path).expect("fresh store");
    store
        .append_event(
            &path,
            UsefulWorkEvent::TaskRegistered {
                task_id: digest(0x01),
            },
        )
        .expect("append");
    store
        .append_event(&path, reward(0x01, 0x31))
        .expect("append reward");

    // Simulate a crash mid-write: a torn half-record with no newline.
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open for tear");
    file.write_all(b"{\"kind\":\"rewardSettled\",\"taskId\":\"dead")
        .expect("torn bytes");
    drop(file);

    let mut recovered = FileUsefulWorkStore::recover(&path).expect("recover over torn tail");
    assert_eq!(
        recovered.events_len(),
        2,
        "the torn line is not an event; the stable prefix survives"
    );
    assert!(recovered.is_task_rewarded(&digest(0x01)));

    // The tail was truncated on disk: the next append starts at a clean
    // record boundary and a fresh recovery sees exactly 3 events.
    recovered
        .append_event(
            &path,
            UsefulWorkEvent::TaskRegistered {
                task_id: digest(0x02),
            },
        )
        .expect("append after truncation");
    let again = FileUsefulWorkStore::recover(&path).expect("recover again");
    assert_eq!(again.events_len(), 3);
}

#[test]
fn duplicate_reward_is_rejected_even_after_restart_and_pk_swap() {
    let path = tmp_store_path();
    let mut store = FileUsefulWorkStore::recover(&path).expect("fresh store");
    assert!(store
        .append_event(&path, reward(0x01, 0x31))
        .expect("first reward"));

    // Same task, same pk: duplicate payout.
    assert!(!store
        .append_event(&path, reward(0x01, 0x31))
        .expect("checked append"));
    // Same task, swapped reward pk: still the same task — one reward max.
    assert!(!store
        .append_event(&path, reward(0x01, 0x32))
        .expect("checked append"));

    // A restart must not forget the payout.
    let mut recovered = FileUsefulWorkStore::recover(&path).expect("recover");
    assert!(!recovered
        .append_event(&path, reward(0x01, 0x33))
        .expect("checked append"));
    assert_eq!(
        recovered
            .reward_record(&digest(0x01))
            .expect("record")
            .reward_pk,
        digest(0x31),
        "the original payee stays; the swap never lands"
    );
    let final_state = FileUsefulWorkStore::recover(&path).expect("final recover");
    assert_eq!(
        final_state.events_len(),
        1,
        "rejected duplicates must not be appended to the log"
    );
}

#[test]
fn reorg_rebuild_drops_abandoned_rewards() {
    let path = tmp_store_path();
    let mut store = FileUsefulWorkStore::recover(&path).expect("fresh store");
    store
        .append_event(&path, reward(0x0a, 0x31))
        .expect("reward a");
    store
        .append_event(&path, reward(0x0b, 0x32))
        .expect("reward b");
    assert!(store.is_task_rewarded(&digest(0x0a)));

    // The canonical chain after the reorg only contains task 0x0b's
    // reward: the local mirror is a pure function of the chain, so task
    // 0x0a must NOT stay spent (it is claimable again on the new chain).
    let canonical = vec![RewardRecord {
        task_id: digest(0x0b),
        submission_id: digest(0x1b),
        reward_pk: digest(0x32),
        epoch: 7,
    }];
    let rebuilt =
        FileUsefulWorkStore::rebuild_from_rewards(&path, &canonical).expect("reorg rebuild");
    assert!(!rebuilt.is_task_rewarded(&digest(0x0a)));
    assert!(rebuilt.is_task_rewarded(&digest(0x0b)));

    // The swap was atomic and durable: a fresh recovery agrees.
    let recovered = FileUsefulWorkStore::recover(&path).expect("recover after reorg");
    assert!(!recovered.is_task_rewarded(&digest(0x0a)));
    assert!(recovered.is_task_rewarded(&digest(0x0b)));
    assert_eq!(recovered.events_len(), 1);
}

#[test]
fn incremental_mirror_matches_replay_derived_mirror() {
    // Gate: the mirror built by live appends equals the mirror rebuilt
    // from the same canonical reward list (replay parity).
    let live_path = tmp_store_path();
    let mut live = FileUsefulWorkStore::recover(&live_path).expect("fresh store");
    let records = vec![
        RewardRecord {
            task_id: digest(0x0a),
            submission_id: digest(0x1a),
            reward_pk: digest(0x31),
            epoch: 7,
        },
        RewardRecord {
            task_id: digest(0x0b),
            submission_id: digest(0x1b),
            reward_pk: digest(0x32),
            epoch: 8,
        },
    ];
    for record in &records {
        live.append_event(
            &live_path,
            UsefulWorkEvent::RewardSettled {
                task_id: record.task_id.clone(),
                submission_id: record.submission_id.clone(),
                reward_pk: record.reward_pk.clone(),
                epoch: record.epoch,
            },
        )
        .expect("live append");
    }

    let replay_path = tmp_store_path();
    let replayed =
        FileUsefulWorkStore::rebuild_from_rewards(&replay_path, &records).expect("rebuild");
    for record in &records {
        assert_eq!(
            live.reward_record(&record.task_id),
            replayed.reward_record(&record.task_id),
            "live and replay-derived mirrors must agree"
        );
    }
}

#[test]
fn duplicate_reward_inside_the_log_file_is_a_recovery_error() {
    // A log that already contains two rewards for one task was written by
    // a buggy or tampered process: recovery refuses to guess.
    let path = tmp_store_path();
    let line = serde_json::to_string(&reward(0x01, 0x31)).expect("serialize");
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open");
    writeln!(file, "{line}").expect("write");
    writeln!(file, "{line}").expect("write dup");
    drop(file);
    let err = FileUsefulWorkStore::recover(&path).unwrap_err();
    assert!(
        err.to_string().contains("duplicate reward"),
        "unexpected error: {err}"
    );
}
