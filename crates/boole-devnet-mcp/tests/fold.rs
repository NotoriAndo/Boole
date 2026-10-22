use anyhow::{bail, Result};
use boole_derivation::{attributes::hex_string, m2::*, InboxLog, L1Block};
use boole_devnet_driver::l1::L1Source;
use boole_devnet_mcp::client::{Client, Poster};
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
}
impl L1Source for Source {
    fn validate_m2_inbox(&mut self) -> Result<()> {
        if self.refuse_runtime {
            bail!("old inbox runtime");
        }
        Ok(())
    }
    fn finalized(&mut self) -> Result<u64> {
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
        None,
        None,
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
    let mut client = Client::new(source, NoPosts, c, [1; 20], dir.0.clone(), None, None).unwrap();
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
        None,
        None
    )
    .is_err());
    assert!(!dir.0.exists());
    let (mut source, _, _) = fixture();
    source.bad_log = true;
    source.finalized = 1;
    let mut client = Client::new(source, NoPosts, c, [1; 20], dir.0.clone(), None, None).unwrap();
    assert!(client.sync().is_err());
    assert_eq!(client.m2_state().height(), 0);
    drop(client);
    let (source, c, _) = fixture();
    let mut client = Client::new(source, NoPosts, c, [1; 20], dir.0.clone(), None, None).unwrap();
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
        None,
        None,
    )
    .unwrap();
    let (source, _, _) = fixture();
    assert!(Client::new(
        source,
        NoPosts,
        c.clone(),
        [1; 20],
        dir.0.clone(),
        None,
        None
    )
    .is_err());
    drop(client);
    let (source, mut c, _) = fixture();
    c.slot_table[1].lane = 2;
    assert!(Client::new(source, NoPosts, c, [1; 20], dir.0.clone(), None, None).is_err());
}

#[derive(Default)]
struct Capture {
    head: u64,
    posts: Vec<u8>,
    receipts: BTreeMap<String, u64>,
    fail_once: bool,
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
        None,
        None,
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
        store.write(&name,&serde_json::json!({"version":1,"config_digest":c.digest(),"prepared":prepared,"cancel_requested":cancel,"local_check":"not_run","sent":sent,"last_error":null})).unwrap();
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
            None,
            None,
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
        None,
        None,
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
        None,
        None,
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
        None,
        None,
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
        None,
        None
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
        let mut client =
            Client::new(source, NoPosts, c, [1; 20], dir.0.clone(), None, None).unwrap();
        assert!(
            client.sync().is_err(),
            "accepted a noncanonical hex prefix in field {field}"
        );
        assert_eq!(client.m2_state().height(), 0);
    }
}
