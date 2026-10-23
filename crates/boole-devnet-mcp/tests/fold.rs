use anyhow::{bail, Result};
use boole_derivation::{attributes::hex_string, m2::*, InboxLog, L1Block};
use boole_devnet_driver::l1::L1Source;
use boole_devnet_mcp::client::{Client, ClientOptions, Poster};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
        let root = base.join("mcp-unit-tests");
        std::fs::create_dir_all(&root).unwrap();
        Self(root.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Clone)]
struct Source {
    rows: BTreeMap<u64, (L1Block, Vec<InboxLog>)>,
    finalized: u64,
    reads: Vec<u64>,
    bad_anchor: bool,
    bad_log: bool,
    refuse_runtime: bool,
    bad_prefix: u8,
    transient_reads: u8,
    finality_calls: u8,
}
impl L1Source for Source {
    fn validate_m2_inbox(&mut self) -> Result<()> {
        if self.refuse_runtime {
            bail!("old inbox runtime");
        }
        Ok(())
    }
    fn finalized(&mut self) -> Result<u64> {
        self.finality_calls += 1;
        if self.transient_reads > 0 {
            self.transient_reads -= 1;
            return Err(boole_devnet_driver::engine::TransientRpcError.into());
        }
        Ok(self.finalized)
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        self.reads.push(n);
        assert!(n <= self.finalized);
        let mut b = self.rows[&n].0.clone();
        if self.bad_prefix == 1 && n == 1 {
            b.parent_hash = b.parent_hash.replacen("0x", "0X", 1);
        }
        if self.bad_anchor {
            b.hash = hex_string(&[255; 32]);
        }
        Ok(b)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        let mut logs = self
            .rows
            .values()
            .find(|(b, _)| b.hash == hash)
            .unwrap()
            .1
            .clone();
        if !logs.is_empty() {
            match self.bad_prefix {
                2 => logs[0].address = logs[0].address.replacen("0x", "0X", 1),
                3 => logs[0].topics[0] = logs[0].topics[0].replacen("0x", "0X", 1),
                4 => logs[0].block_hash = logs[0].block_hash.replacen("0x", "0X", 1),
                _ => {}
            }
        }
        if self.bad_log && !logs.is_empty() {
            logs[0].removed = true;
        }
        Ok(logs)
    }
}
struct NoPosts;
impl Poster for NoPosts {
    fn head(&mut self) -> Result<u64> {
        Ok(1000)
    }
    fn post(&mut self, _: u8, _: &[u8]) -> Result<String> {
        panic!("read tools posted a transaction")
    }
    fn included(&mut self, _: &str) -> Result<Option<u64>> {
        panic!("read tools queried a transaction")
    }
}
fn fixture() -> (Source, Config, Vec<Value>) {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m2/rounds-v1.1.json")).unwrap();
    let rows = fixture["heights"].as_array().unwrap().clone();
    let mut map: BTreeMap<u64, (L1Block, Vec<InboxLog>)> = rows
        .iter()
        .map(|r| {
            let b: L1Block = serde_json::from_value(r["l1"].clone()).unwrap();
            (
                b.number,
                (b, serde_json::from_value(r["logs"].clone()).unwrap()),
            )
        })
        .collect();
    let mut genesis = map[&1].0.clone();
    genesis.number = 0;
    genesis.hash = genesis.parent_hash.clone();
    map.insert(0, (genesis, vec![]));
    (
        Source {
            rows: map,
            finalized: 0,
            reads: vec![],
            bad_anchor: false,
            bad_log: false,
            refuse_runtime: false,
            bad_prefix: 0,
            transient_reads: 0,
            finality_calls: 0,
        },
        serde_json::from_value(fixture["config"].clone()).unwrap(),
        rows,
    )
}
#[test]
fn finalized_client_matches_all_recorded_driver_roots_and_reads_problem() {
    let (source, c, rows) = fixture();
    let dir = Dir::new();
    let mut client = Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    assert_eq!(client.status()["fold_height"], 0);
    for row in &rows {
        let h = row["height"].as_u64().unwrap();
        client.source.finalized = h;
        client.sync().unwrap();
        assert_eq!(
            hex_string(&client.m2_state().root),
            row["result"]["m2_root"],
            "height {h}"
        );
        if h == 5 {
            let status = client.status();
            let p = client.m2_state().rounds[&(0, 1)].problem.unwrap();
            assert_eq!(status["slots"][0]["lane"], 2);
            assert_eq!(
                status["slots"][0]["last_commit_height"],
                5 + c.slot_table[0].solve_deadline
            );
            let package = client.problem(p).unwrap();
            assert_eq!(package["membership"], "accepted_by_finalized_fold");
            assert!(package["decoded_len"].as_u64().unwrap() > 0);
            assert_eq!(
                client
                    .call("boole.devnet_problem", &serde_json::json!({"slot":0}))
                    .unwrap(),
                package
            );
        }
    }
    assert_eq!(
        client.status()["slot_table_digest"],
        hex_string(&c.slot_table_digest())
    );
    assert!(client.source.reads.iter().all(|h| *h <= 140));
}
#[test]
fn changed_finalized_anchor_is_sticky_and_never_adopts_a_new_root() {
    let (source, c, _) = fixture();
    let dir = Dir::new();
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.source.finalized = 3;
    client.sync().unwrap();
    let root = client.m2_state().root;
    client.source.bad_anchor = true;
    assert!(client
        .sync()
        .unwrap_err()
        .to_string()
        .contains("anchor changed"));
    client.source.bad_anchor = false;
    assert!(client.sync().is_err());
    assert_eq!(client.m2_state().root, root);
    assert_eq!(client.m2_state().height(), 3);
}
#[test]
fn runtime_bad_logs_and_backwards_finality_refuse_progress() {
    let (mut source, c, _) = fixture();
    source.refuse_runtime = true;
    let dir = Dir::new();
    assert!(Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000)
    )
    .is_err());
    assert!(!dir.0.exists());
    let (mut source, _, _) = fixture();
    source.bad_log = true;
    source.finalized = 1;
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    assert!(client.sync().is_err());
    assert_eq!(client.m2_state().height(), 0);
    drop(client);
    let (source, c, _) = fixture();
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.source.finalized = 3;
    client.sync().unwrap();
    client.source.finalized = 2;
    assert!(client.sync().is_err());
    assert_eq!(client.m2_state().height(), 3);
}
#[test]
fn store_is_exclusive_and_bound_to_the_exact_configuration() {
    let (source, c, _) = fixture();
    let dir = Dir::new();
    let client = Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    let (source, _, _) = fixture();
    assert!(Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000)
    )
    .is_err());
    drop(client);
    let (source, mut c, _) = fixture();
    c.slot_table[1].lane = 2;
    assert!(Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000)
    )
    .is_err());
}

#[derive(Default)]
struct Capture {
    head: u64,
    posts: Vec<u8>,
    receipts: BTreeMap<String, u64>,
    fail_once: bool,
    reverted: Option<String>,
}
impl Poster for Capture {
    fn head(&mut self) -> Result<u64> {
        Ok(self.head)
    }
    fn post(&mut self, kind: u8, _: &[u8]) -> Result<String> {
        self.posts.push(kind);
        if self.fail_once {
            self.fail_once = false;
            bail!("private-send-error-canary");
        }
        Ok(hex_string(&[self.posts.len() as u8; 32]))
    }
    fn included(&mut self, tx: &str) -> Result<Option<u64>> {
        if self.reverted.as_deref() == Some(tx) {
            return Err(boole_devnet_mcp::client::RevertedReceipt.into());
        }
        Ok(self.receipts.get(tx).copied())
    }
}
fn persisted_candidates(
    dir: &Dir,
    count: u8,
    cancel: bool,
    sent_head: Option<u64>,
) -> (Source, Config) {
    use boole_devnet_mcp::{
        store::Store,
        wire::{calldata, Material, Prepared},
    };
    let (mut source, c, _) = fixture();
    source.finalized = 5;
    let mut bootstrap = Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    bootstrap.sync().unwrap();
    let p = bootstrap.m2_state().rounds[&(0, 1)].problem.unwrap();
    let mut source = bootstrap.source.clone();
    drop(bootstrap);
    let store = Store::open(dir.0.clone()).unwrap();
    for i in 1..=count {
        let prepared = Prepared::new(
            &c,
            Material {
                problem: p,
                payee: [6; 20],
                salt: [i; 32],
                cancel_secret: [i + 10; 32],
                raw_len: 3,
                export_digest: [1; 32],
                compressed: b"abc".to_vec(),
                packages: vec![],
                format: 2,
            },
        )
        .unwrap();
        let block = source.rows.get_mut(&6).unwrap();
        block.1.push(InboxLog {
            address: boole_derivation::config::INBOX.into(),
            topics: vec![boole_derivation::config::posted_topic()],
            block_hash: block.0.hash.clone(),
            removed: false,
            transaction_index: 100 + u64::from(i),
            log_index: 100 + u64::from(i),
            data: hex_string(&calldata(2, &prepared.commit_body([1; 20]))[4..]),
        });
        let sent = sent_head
            .map(|h| serde_json::json!({"commit":{"head":h,"tx":null}}))
            .unwrap_or(serde_json::json!({}));
        let name = format!("submission-{}.json", hex_string(&prepared.commit));
        store.write(&name,&serde_json::json!({"schema_version":2,"config_digest":c.digest(),"prepared":prepared,"cancel_requested":cancel,"local_check":"not_run","sent":sent,"last_error":null})).unwrap();
    }
    drop(store);
    source.finalized = 6;
    (source, c)
}
#[test]
fn cancel_does_not_post_past_the_window_when_finality_lags() {
    for (head, expected) in [(69, vec![7]), (70, vec![])] {
        let dir = Dir::new();
        let (source, c) = persisted_candidates(&dir, 1, true, None);
        let mut client = Client::new(
            source,
            Capture {
                head,
                ..Capture::default()
            },
            c,
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        client.tick().unwrap();
        assert_eq!(
            client.poster.posts,
            expected,
            "next inclusion at {}",
            head + 1
        );
    }
}
#[test]
fn restart_keeps_the_posting_budget_of_the_current_head() {
    let dir = Dir::new();
    let (source, c) = persisted_candidates(&dir, 6, false, Some(14));
    let mut client = Client::new(
        source,
        Capture {
            head: 14,
            ..Capture::default()
        },
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.tick().unwrap();
    assert!(
        client.poster.posts.is_empty(),
        "restart forgot six durable posting attempts at this head"
    );
    client.poster.head = 15;
    client.tick().unwrap();
    assert_eq!(client.poster.posts, vec![3; 6]);
}

#[test]
fn open_waits_for_the_minimum_and_missing_unfinalized_parts_are_reposted() {
    let dir = Dir::new();
    let (source, c) = persisted_candidates(&dir, 1, false, None);
    let mut client = Client::new(
        source,
        Capture {
            head: 13,
            ..Capture::default()
        },
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.tick().unwrap();
    assert!(client.poster.posts.is_empty());
    client.poster.head = 14;
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3]);
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3]);
    client.poster.receipts.insert(hex_string(&[1; 32]), 15);
    client.poster.head = 15;
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3, 4]);
    client.poster.receipts.insert(hex_string(&[2; 32]), 16);
    client.poster.head = 16;
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3, 4]);
    client.poster.receipts.remove(&hex_string(&[2; 32]));
    client.poster.head = 17;
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3, 4, 4]);
    assert_eq!(
        client.m2_state().height(),
        6,
        "unfinalized receipts must not advance the fold"
    );
}

#[test]
fn unknown_send_outcome_reconciles_from_the_fold_without_leaking_the_error() {
    let dir = Dir::new();
    let (mut source, c) = persisted_candidates(&dir, 1, false, None);
    source.finalized = 5;
    let mut client = Client::new(
        source,
        Capture {
            head: 5,
            fail_once: true,
            ..Capture::default()
        },
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [2]);
    let store_name = std::fs::read_dir(&dir.0)
        .unwrap()
        .filter_map(Result::ok)
        .find(|p| p.file_name().to_string_lossy().starts_with("submission-"))
        .unwrap()
        .path();
    let saved: Value = serde_json::from_slice(&std::fs::read(store_name).unwrap()).unwrap();
    let id: Hash = serde_json::from_value(saved["prepared"]["commit"].clone()).unwrap();
    let view = client.submission(id).unwrap();
    assert!(view["last_error"]
        .as_str()
        .unwrap()
        .contains("outcome unknown"));
    assert!(!view.to_string().contains("private-send-error-canary"));
    client.source.finalized = 6;
    client.poster.head = 13;
    client.tick().unwrap();
    assert_eq!(client.submission(id).unwrap()["status"], "COMMITTED");
    assert_eq!(client.poster.posts, [2]);
}

#[test]
fn restart_refuses_corrupt_unrevealed_material() {
    let dir = Dir::new();
    let (source, c) = persisted_candidates(&dir, 1, false, None);
    let store = boole_devnet_mcp::store::Store::open(dir.0.clone()).unwrap();
    let name = store.submissions().unwrap().pop().unwrap();
    let mut saved: Value = store.read(&name).unwrap();
    saved["prepared"]["chunks"][0][0] = serde_json::json!(99);
    store.write(&name, &saved).unwrap();
    drop(store);
    assert!(Client::new(
        source,
        Capture::default(),
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000)
    )
    .is_err());
}

#[test]
fn hex_encoding_rejections_match_the_driver_input_contract() {
    for field in 1..=4 {
        let (mut source, c, _) = fixture();
        source.finalized = 1;
        source.bad_prefix = field;
        let dir = Dir::new();
        let mut client = Client::new(
            source,
            NoPosts,
            c,
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        assert!(
            client.sync().is_err(),
            "accepted a noncanonical hex prefix in field {field}"
        );
        assert_eq!(client.m2_state().height(), 0);
    }
}

fn saved_submission(dir: &Dir) -> (String, Value, boole_devnet_mcp::wire::Prepared) {
    let store = boole_devnet_mcp::store::Store::open(dir.0.clone()).unwrap();
    let name = store.submissions().unwrap().pop().unwrap();
    let saved: Value = store.read(&name).unwrap();
    let prepared = serde_json::from_value(saved["prepared"].clone()).unwrap();
    (name, saved, prepared)
}
fn add_item(source: &mut Source, height: u64, kind: u8, body: &[u8]) {
    let row = source.rows.get_mut(&height).unwrap();
    row.1.push(InboxLog {
        address: boole_derivation::config::INBOX.into(),
        topics: vec![boole_derivation::config::posted_topic()],
        block_hash: row.0.hash.clone(),
        removed: false,
        transaction_index: 200,
        log_index: 200,
        data: hex_string(&boole_devnet_mcp::wire::calldata(kind, body)[4..]),
    });
}
#[test]
fn final_submission_states_survive_recorded_pruning_and_restart() {
    for cancelled in [false, true] {
        let dir = Dir::new();
        let (mut source, c) = persisted_candidates(&dir, 1, cancelled, None);
        let (_, _, prepared) = saved_submission(&dir);
        if cancelled {
            add_item(&mut source, 7, 7, &prepared.cancel());
        }
        source.finalized = 140;
        let mut client = Client::new(
            source,
            NoPosts,
            c.clone(),
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        client.sync().unwrap();
        assert!(!client
            .m2_state()
            .candidates
            .contains_key(&(prepared.problem, prepared.commit)));
        let expected = if cancelled { "CANCELLED" } else { "EXPIRED" };
        let view = client.submission(prepared.commit).unwrap();
        assert_eq!(view["status"], expected);
        assert!(view["prune_height"].as_u64().unwrap() < 140);
        let source = client.source.clone();
        drop(client);
        let mut restarted = Client::new(
            source,
            NoPosts,
            c,
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        restarted.sync().unwrap();
        assert_eq!(
            restarted.submission(prepared.commit).unwrap()["status"],
            expected
        );
    }
}
#[test]
fn a_commit_that_misses_its_deadline_becomes_final() {
    let dir = Dir::new();
    let (mut source, c) = persisted_candidates(&dir, 1, false, Some(5));
    let (_, _, prepared) = saved_submission(&dir);
    source
        .rows
        .get_mut(&6)
        .unwrap()
        .1
        .retain(|l| l.transaction_index < 100);
    source.finalized = 140;
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.sync().unwrap();
    assert_eq!(
        client.submission(prepared.commit).unwrap()["status"],
        "COMMIT_NOT_INCLUDED"
    );
}
#[test]
fn transient_rpc_reads_retry_before_stalling_and_status_is_reachable() {
    for (failures, recovers) in [(1, true), (3, false)] {
        let (mut source, c, _) = fixture();
        source.finalized = 3;
        source.transient_reads = failures;
        let dir = Dir::new();
        let mut client = Client::new(
            source,
            NoPosts,
            c,
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        let view = client
            .call("boole.devnet_status", &serde_json::json!({}))
            .unwrap();
        assert_eq!(view["stalled"].is_null(), recovers);
        assert_eq!(view["fold_height"], if recovers { 3 } else { 0 });
        assert_eq!(client.source.finality_calls, if recovers { 2 } else { 3 });
        if !recovers {
            assert!(client
                .call("boole.devnet_problem", &serde_json::json!({"slot":0}))
                .is_err());
        }
    }
}
#[test]
fn malformed_l1_headers_stall_before_folding_like_the_driver() {
    for (field, bad) in [
        ("mixHash", "0x00".to_owned()),
        ("mixHash", format!("0X{}", "11".repeat(32))),
        ("parentBeaconBlockRoot", "0x00".into()),
        ("parentBeaconBlockRoot", format!("0X{}", "11".repeat(32))),
        ("baseFeePerGas", "0x".into()),
        ("baseFeePerGas", "0xzz".into()),
        ("baseFeePerGas", format!("0x{}", "1".repeat(65))),
    ] {
        let (mut source, c, _) = fixture();
        source.finalized = 1;
        let mut value = serde_json::to_value(&source.rows[&1].0).unwrap();
        value[field] = serde_json::json!(bad);
        source.rows.get_mut(&1).unwrap().0 = serde_json::from_value(value).unwrap();
        let dir = Dir::new();
        let mut client = Client::new(
            source,
            NoPosts,
            c,
            [1; 20],
            dir.0.clone(),
            ClientOptions::new(30_000_000),
        )
        .unwrap();
        assert!(client.sync().is_err(), "accepted malformed {field}");
        assert_eq!(client.m2_state().height(), 0);
        assert!(!client
            .call("boole.devnet_status", &serde_json::json!({}))
            .unwrap()["stalled"]
            .is_null());
    }
}
#[test]
fn reverted_open_is_durable_and_does_not_block_other_submissions() {
    let dir = Dir::new();
    let (source, c) = persisted_candidates(&dir, 2, false, None);
    let mut client = Client::new(
        source,
        Capture {
            head: 14,
            ..Capture::default()
        },
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [3, 3]);
    client.poster.reverted = Some(hex_string(&[1; 32]));
    client.poster.head = 15;
    client.tick().unwrap();
    assert_eq!(
        client.poster.posts,
        [3, 3],
        "respect the receipt reconciliation gap"
    );
    let source = client.source.clone();
    drop(client);
    let store = boole_devnet_mcp::store::Store::open(dir.0.clone()).unwrap();
    let states: Vec<Value> = store
        .submissions()
        .unwrap()
        .iter()
        .map(|n| store.read(n).unwrap())
        .collect();
    let failed = states
        .iter()
        .find(|s| s["sent"]["open"]["tx"] == hex_string(&[1; 32]))
        .unwrap();
    assert_eq!(failed["sent"]["open"]["failure"], "REVERTED");
    let failed_id: Hash = serde_json::from_value(failed["prepared"]["commit"].clone()).unwrap();
    drop(store);
    let mut restarted = Client::new(
        source,
        Capture {
            head: 16,
            reverted: Some(hex_string(&[1; 32])),
            ..Capture::default()
        },
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    assert_eq!(
        restarted.submission(failed_id).unwrap()["transactions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["stage"] == "open")
            .unwrap()["failure"],
        "REVERTED"
    );
    restarted.tick().unwrap();
    assert_eq!(
        restarted.poster.posts,
        [3, 3],
        "both submissions resume within their M2 windows"
    );
}

#[test]
fn startup_and_store_identity_use_the_observed_l2_gas_limit() {
    let (source, c, _) = fixture();
    assert!(c.validate(1_000_000).is_err());
    let dir = Dir::new();
    assert!(Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(1_000_000)
    )
    .is_err());
    assert!(!dir.0.exists());
    let (source, _, _) = fixture();
    let client = Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(40_000_000),
    )
    .unwrap();
    drop(client);
    let identity: Value =
        serde_json::from_slice(&std::fs::read(dir.0.join("identity.json")).unwrap()).unwrap();
    assert_eq!(identity["l2_gas_limit"], 40_000_000);
    let (source, _, _) = fixture();
    assert!(Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000)
    )
    .is_err());
}

#[test]
fn a_closed_problem_without_a_post_has_a_final_status() {
    let dir = Dir::new();
    let (mut source, c) = persisted_candidates(&dir, 1, false, None);
    let (_, _, prepared) = saved_submission(&dir);
    source
        .rows
        .get_mut(&6)
        .unwrap()
        .1
        .retain(|l| l.transaction_index < 100);
    source.finalized = 140;
    let mut client = Client::new(
        source,
        NoPosts,
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.sync().unwrap();
    assert_eq!(
        client.submission(prepared.commit).unwrap()["status"],
        "PROBLEM_CLOSED"
    );
}
#[test]
fn an_open_candidate_with_finalized_parts_can_cancel_instead_of_posting_parts() {
    use boole_devnet_mcp::{
        store::Store,
        wire::{Material, Prepared},
    };
    let dir = Dir::new();
    let (mut source, c) = persisted_candidates(&dir, 1, false, None);
    let (name, mut saved, old) = saved_submission(&dir);
    let prepared = Prepared::new(
        &c,
        Material {
            problem: old.problem,
            payee: [6; 20],
            salt: [7; 32],
            cancel_secret: [8; 32],
            raw_len: (2 * PART_SIZE + 3) as u64,
            export_digest: [9; 32],
            compressed: vec![10; 2 * PART_SIZE + 3],
            packages: vec![],
            format: 2,
        },
    )
    .unwrap();
    source
        .rows
        .get_mut(&6)
        .unwrap()
        .1
        .retain(|l| l.transaction_index < 100);
    add_item(&mut source, 6, 2, &prepared.commit_body([1; 20]));
    add_item(&mut source, 15, 3, &prepared.open([1; 20]));
    add_item(&mut source, 16, 4, &prepared.part([1; 20], 0).unwrap());
    let store = Store::open(dir.0.clone()).unwrap();
    std::fs::remove_file(dir.0.join(name)).unwrap();
    saved["prepared"] = serde_json::to_value(&prepared).unwrap();
    store
        .write(
            &format!("submission-{}.json", hex_string(&prepared.commit)),
            &saved,
        )
        .unwrap();
    drop(store);
    source.finalized = 16;
    let mut client = Client::new(
        source,
        Capture {
            head: 16,
            ..Capture::default()
        },
        c,
        [1; 20],
        dir.0.clone(),
        ClientOptions::new(30_000_000),
    )
    .unwrap();
    client.sync().unwrap();
    let view = client.submission(prepared.commit).unwrap();
    assert_eq!(view["status"], "OPEN");
    assert_eq!(view["finalized_parts"], 1);
    assert_eq!(view["parts"], 3);
    client.cancel(prepared.commit).unwrap();
    client.tick().unwrap();
    assert_eq!(client.poster.posts, [7]);
    add_item(&mut client.source, 17, 7, &prepared.cancel());
    client.source.finalized = 17;
    client.sync().unwrap();
    assert_eq!(
        client.submission(prepared.commit).unwrap()["status"],
        "CANCELLED"
    );
}

struct RootNode(Option<Hash>);
impl boole_devnet_mcp::client::L2Source for RootNode {
    fn root(&mut self, _height: u64, _origin: &str) -> Result<Option<Hash>> {
        Ok(self.0)
    }
}
#[test]
fn m2root_client_reports_match_and_hard_stalls_on_mismatch() {
    let (mut source, c, rows) = fixture();
    source.finalized = 3;
    let expected =
        boole_devnet_mcp::client::fixed(rows[2]["result"]["m2_root"].as_str().unwrap()).unwrap();
    for root in [Some(expected), Some([255; 32]), None] {
        let dir = Dir::new();
        let mut options = ClientOptions::new(30_000_000);
        options.l2 = Some(Box::new(RootNode(root)));
        let mut client = Client::new(
            source.clone(),
            NoPosts,
            c.clone(),
            [1; 20],
            dir.0.clone(),
            options,
        )
        .unwrap();
        let result = client.sync();
        assert_eq!(result.is_ok(), root.is_none() || root == Some(expected));
        let view = client
            .call("boole.devnet_status", &serde_json::json!({}))
            .unwrap();
        assert_eq!(
            view["l2_m2_root_match"],
            root.map(|r| r == expected)
                .map(serde_json::Value::Bool)
                .unwrap_or(serde_json::Value::Null)
        );
        if root == Some([255; 32]) {
            assert!(client.tick().is_err());
            assert!(!view["stalled"].is_null());
        }
    }
}
