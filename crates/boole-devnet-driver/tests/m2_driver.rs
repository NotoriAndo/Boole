//! T15–T16: four Engine calls, transactional state and hash-anchored restart.
use anyhow::{bail, Result};
use boole_derivation::{
    attributes::derive_m1,
    m2::{Config, State},
    InboxLog, L1Block, ParentHeader,
};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::Engine,
    l1::L1Source,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Source {
    rows: BTreeMap<u64, (L1Block, Vec<InboxLog>)>,
    hash_reads: Vec<String>,
    number_reads: Vec<u64>,
    fail_hash: bool,
}
impl Source {
    fn new() -> Self {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        Self {
            rows: rows
                .into_iter()
                .map(|r| {
                    let l: L1Block = serde_json::from_value(r["l1"].clone()).unwrap();
                    let mut logs: Vec<InboxLog> =
                        serde_json::from_value(r["logs"].clone()).unwrap();
                    let mut data = vec![0; 64];
                    data[31] = 32;
                    data[63] = 1;
                    data.push(2);
                    logs.push(InboxLog {
                        address: boole_derivation::config::INBOX.into(),
                        topics: vec![boole_derivation::config::posted_topic()],
                        block_hash: l.hash.clone(),
                        removed: false,
                        transaction_index: 99,
                        log_index: 99,
                        data: boole_derivation::attributes::hex_string(&data),
                    });
                    (l.number, (l, logs))
                })
                .collect(),
            hash_reads: vec![],
            number_reads: vec![],
            fail_hash: false,
        }
    }
}
impl L1Source for Source {
    fn finalized(&mut self) -> Result<u64> {
        Ok(35)
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        self.number_reads.push(n);
        Ok(self.rows[&n].0.clone())
    }
    fn block_by_hash(&mut self, h: &str) -> Result<L1Block> {
        self.hash_reads.push(h.into());
        if self.fail_hash {
            bail!("injected anchored RPC failure");
        }
        Ok(self
            .rows
            .values()
            .find(|r| r.0.hash == h)
            .unwrap()
            .0
            .clone())
    }
    fn inbox_logs(&mut self, h: &str) -> Result<Vec<InboxLog>> {
        Ok(self
            .rows
            .values()
            .find(|r| r.0.hash == h)
            .unwrap()
            .1
            .clone())
    }
}
#[derive(Clone)]
struct Client {
    head: ParentHeader,
    history: BTreeMap<u64, ParentHeader>,
    attrs: Value,
    calls: Vec<(String, Value)>,
    fail: bool,
}
impl Client {
    fn new() -> Self {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let head: ParentHeader = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
        Self {
            history: [(0, head.clone())].into(),
            head,
            attrs: Value::Null,
            calls: vec![],
            fail: false,
        }
    }
}
impl Engine for Client {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        Ok(self.history[&h].clone())
    }
    fn call(&mut self, m: &str, p: Value) -> Result<Value> {
        self.calls.push((m.into(), p.clone()));
        if self.fail {
            bail!("injected engine failure");
        }
        Ok(match m {
            "engine_forkchoiceUpdatedV3" => {
                if p[1].is_null() {
                    self.head.parent_hash = Some(self.head.hash.clone());
                    self.head.number += 1;
                    self.head.hash = p[0]["headBlockHash"].as_str().unwrap().into();
                    self.head.timestamp = u64::from_str_radix(
                        self.attrs["timestamp"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )
                    .unwrap();
                    self.head.l1_info =
                        Some(self.attrs["transactions"][0].as_str().unwrap().into());
                    self.history.insert(self.head.number, self.head.clone());
                } else {
                    self.attrs = p[1].clone();
                }
                json!({"payloadStatus":{"status":"VALID"},"payloadId":"0x1"})
            }
            "engine_getPayloadV5" => {
                json!({"executionPayload":{"transactions":self.attrs["transactions"],"blockNumber":format!("0x{:x}",self.head.number+1),"blockHash":format!("0x{:064x}",self.head.number+1),"parentHash":self.head.hash,"timestamp":self.attrs["timestamp"],"gasLimit":self.attrs["gasLimit"]}})
            }
            "engine_newPayloadV4" => json!({"status":"VALID"}),
            _ => panic!("unexpected Engine method"),
        })
    }
}
#[test]
fn t15_m2_adds_no_engine_call_or_payload() {
    let mut l = Source::new();
    let mut client = Client::new();
    let parent = client.head().unwrap();
    let expected = derive_m1(&l.block(1).unwrap(), &l.rows[&1].1, &parent).unwrap();
    let mut d = Driver::new(l, client);
    let StepOutcome::Derived(actual) = d.step(1).unwrap() else {
        panic!("wait")
    };
    assert_eq!(actual.attributes, expected.attributes);
    assert_eq!(d.engine.calls.len(), 4);
    assert_eq!(d.m2_state().height(), 1);
    assert_ne!(d.m2_state().root, [0; 32]);
    assert!(actual.decisions.last().unwrap().m2.is_some());
}
#[test]
fn t16_restart_uses_l1_info_hashes_and_matches_continuous() {
    let mut a = Driver::new(Source::new(), Client::new());
    for h in 1..=18 {
        a.step(h).unwrap();
    }
    let client = a.engine.clone();
    let mut b = Driver::new(Source::new(), client);
    let root = a.m2_state().root;
    assert_ne!(
        root,
        State::new(&Config::devnet(), 30_000_000).unwrap().root
    );
    a.step(19).unwrap();
    b.step(19).unwrap();
    assert_eq!(a.m2_state().root, b.m2_state().root);
    assert_eq!(b.l1.hash_reads.len(), 18);
    assert!(b.l1.number_reads.iter().all(|h| *h == 19));
    assert_eq!(b.engine.calls.len(), a.engine.calls.len());
    for h in 20..=35 {
        a.step(h).unwrap();
        b.step(h).unwrap();
        assert_eq!(a.m2_state().root, b.m2_state().root);
    }
}
#[test]
fn t16_stall_preserves_previous_state_and_rpc_failure_is_sticky() {
    let mut a = Driver::new(Source::new(), Client::new());
    a.step(1).unwrap();
    let root = a.m2_state().root;
    a.engine.fail = true;
    assert!(a.step(2).is_err());
    assert_eq!(a.m2_state().height(), 1);
    assert_eq!(a.m2_state().root, root);
    let mut a = Driver::new(Source::new(), Client::new());
    a.step(1).unwrap();
    let mut source = Source::new();
    source.fail_hash = true;
    let mut b = Driver::new(source, a.engine);
    assert!(b
        .step(2)
        .unwrap_err()
        .reason
        .contains("anchored RPC failure"));
    assert_eq!(b.m2_state().height(), 0);
    let reads = b.l1.hash_reads.len();
    assert!(b.step(2).is_err());
    assert_eq!(reads, b.l1.hash_reads.len());
}
#[test]
fn t16_checkpoint_roundtrip_and_tamper_stalls() {
    let temporary = TestDir::new("checkpoint");
    let dir = &temporary.0;
    let path = dir.join("checkpoint.json");
    let mut a = Driver::new(Source::new(), Client::new());
    a.set_journal(&path).unwrap();
    a.step(1).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["height"], 1);
    let mut b = Driver::new(Source::new(), a.engine.clone());
    b.set_journal(&path).unwrap();
    b.step(2).unwrap();
    a.step(2).unwrap();
    assert_eq!(a.m2_state().root, b.m2_state().root);
    let mut bad: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    bad["m2_root"] = json!("0x00");
    std::fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
    let mut c = Driver::new(Source::new(), b.engine);
    c.set_journal(&path).unwrap();
    assert!(c.step(3).unwrap_err().reason.contains("checkpoint"));
    assert_eq!(c.m2_state().height(), 0);
}

#[test]
fn t16_invalid_startup_and_actual_parent_s4_stall_before_engine() {
    let mut c = Config::devnet();
    c.slot_table.clear();
    assert!(Driver::with_config(Source::new(), Client::new(), c, 30_000_000).is_err());
    assert!(
        Driver::with_config(Source::new(), Client::new(), Config::devnet(), 25_000_000).is_err()
    );
    let mut client = Client::new();
    client.head.gas_limit = 25_000_000;
    let mut driver = Driver::new(Source::new(), client);
    assert!(driver.step(1).unwrap_err().reason.contains("S4"));
    assert_eq!(driver.engine.calls.len(), 0);
    assert_eq!(driver.m2_state().height(), 0);
}
#[test]
fn t16_repeated_height_stalls_without_state_change() {
    let mut d = Driver::new(Source::new(), Client::new());
    d.step(1).unwrap();
    let root = d.m2_state().root;
    assert!(d.step(1).is_err());
    assert_eq!(d.m2_state().root, root);
    assert_eq!(d.engine.calls.len(), 4);
}
#[test]
fn t16_checkpoint_collision_preserves_existing_file_and_state() {
    let temporary = TestDir::new("collision");
    let dir = &temporary.0;
    let path = dir.join("checkpoint.json");
    let collision = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&collision, b"preserve existing temporary file").unwrap();
    let mut d = Driver::new(Source::new(), Client::new());
    d.set_journal(&path).unwrap();
    assert!(d
        .step(1)
        .unwrap_err()
        .reason
        .contains("persist M2 checkpoint"));
    assert_eq!(d.m2_state().height(), 0);
    assert_eq!(
        std::fs::read(&collision).unwrap(),
        b"preserve existing temporary file"
    );
    assert!(!path.exists());
}
#[test]
fn t16_changed_cached_parent_hash_stalls_before_engine() {
    let mut d = Driver::new(Source::new(), Client::new());
    d.step(1).unwrap();
    let root = d.m2_state().root;
    d.engine.head.hash = format!("0x{}", "de".repeat(32));
    assert!(d.step(2).unwrap_err().reason.contains("parent changed"));
    assert_eq!(d.m2_state().root, root);
    assert_eq!(d.m2_state().height(), 1);
    assert_eq!(d.engine.calls.len(), 4);
}
#[test]
fn t16_verified_restart_parent_survives_next_height_stall() {
    let mut a = Driver::new(Source::new(), Client::new());
    a.step(1).unwrap();
    let parent_root = a.m2_state().root;
    let mut client = a.engine;
    client.fail = true;
    let mut restarted = Driver::new(Source::new(), client);
    assert!(restarted.step(2).is_err());
    assert_eq!(restarted.m2_state().height(), 1);
    assert_eq!(restarted.m2_state().root, parent_root);
}

fn evidence_rows(path: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[test]
fn review_checkpoint_failure_appends_stall_with_previous_root() {
    let dir = TestDir::new("evidence-collision");
    let path = dir.0.join("checkpoint.json");
    let evidence = dir.0.join("evidence.jsonl");
    let collision = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&collision, b"owned elsewhere").unwrap();
    let mut d = Driver::new(Source::new(), Client::new());
    let previous = d.m2_state().root;
    d.set_journal(&path).unwrap();
    d.set_evidence(&evidence).unwrap();
    assert!(d.step(1).is_err());
    let rows = evidence_rows(&evidence);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["outcome"], "DERIVED");
    assert_eq!(rows[1]["outcome"], "STALL");
    assert_eq!(
        rows[1]["m2_root"],
        boole_derivation::attributes::hex_string(&previous)
    );
}
#[test]
fn review_fold_stall_evidence_keeps_previous_root() {
    let dir = TestDir::new("fold-stall");
    let path = dir.0.join("evidence.jsonl");
    let mut d = Driver::new(Source::new(), Client::new());
    d.set_evidence(&path).unwrap();
    d.step(1).unwrap();
    let previous = d.m2_state().root;
    d.engine.fail = true;
    assert!(d.step(2).is_err());
    let rows = evidence_rows(&path);
    assert_eq!(rows[1]["outcome"], "STALL");
    assert_eq!(
        rows[1]["m2_root"],
        boole_derivation::attributes::hex_string(&previous)
    );
}
#[test]
fn review_refold_rejects_broken_l2_parent_chain() {
    let mut a = Driver::new(Source::new(), Client::new());
    for h in 1..=3 {
        a.step(h).unwrap();
    }
    let mut value = serde_json::to_value(&a.engine.history[&2]).unwrap();
    value["parentHash"] = json!(format!("0x{}", "ff".repeat(32)));
    a.engine
        .history
        .insert(2, serde_json::from_value(value).unwrap());
    let calls = a.engine.calls.len();
    let mut b = Driver::new(Source::new(), a.engine);
    assert!(b.step(4).unwrap_err().reason.contains("parentHash"));
    assert_eq!(b.engine.calls.len(), calls);
    assert_eq!(b.m2_state().height(), 0);
}
struct TestDir(std::path::PathBuf);
impl TestDir {
    fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
            })
            .join("m2-driver-tests");
        std::fs::create_dir_all(&root).unwrap();
        loop {
            let path = root.join(format!(
                "{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        }
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn review_cli_startup_reports_the_actual_slot_table_digest() {
    let dir = TestDir::new("startup-slot-digest");
    for c in [Config::devnet(), Config::lean_testnet(99)] {
        let config = dir.0.join("config.json");
        std::fs::write(&config, serde_json::to_vec(&c).unwrap()).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_boole-devnet-driver"))
            .args([
                "--l1-rpc",
                "http://127.0.0.1:0",
                "--engine-rpc",
                "http://127.0.0.1:0",
                "--node-rpc",
                "http://127.0.0.1:0",
                "--until",
                "0",
                "--jwt-secret",
            ])
            .arg(dir.0.join("unused-jwt"))
            .arg("--m2-config")
            .arg(&config)
            .output()
            .unwrap();
        // The banner must precede the RPC attempt, even when no node is up.
        assert!(!output.status.success());
        let stderr = String::from_utf8(output.stderr).unwrap();
        let expected = format!(
            "slot_table_digest={}",
            boole_derivation::attributes::hex_string(&c.slot_table_digest())
        );
        assert_eq!(stderr.lines().next(), Some(expected.as_str()));
    }
}

#[test]
fn review_restart_from_open_reveal_and_round_matches_full_state() {
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/devnet-m2/commit-reveal-v1.1.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let config: Config = serde_json::from_value(fixture["config"].clone()).unwrap();
    let rows = fixture["heights"].as_array().unwrap();
    let mut source = Source::new();
    source.rows = rows
        .iter()
        .map(|r| {
            let l: L1Block = serde_json::from_value(r["l1"].clone()).unwrap();
            (
                l.number,
                (l, serde_json::from_value(r["logs"].clone()).unwrap()),
            )
        })
        .collect();
    let mut client = Client::new();
    client.head = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
    client.history = [(0, client.head.clone())].into();
    let mut a = Driver::with_config(source.clone(), client, config.clone(), 30_000_000).unwrap();
    for h in 1..=17 {
        a.step(h).unwrap();
    }
    assert!(!a.m2_state().rounds.is_empty());
    assert!(a
        .m2_state()
        .candidates
        .values()
        .any(|c| c.status() == boole_derivation::m2::Status::Open));
    assert!(a.m2_state().candidates.len() >= 3);
    let mut b = Driver::with_config(source, a.engine.clone(), config, 30_000_000).unwrap();
    for h in 18..=35 {
        a.step(h).unwrap();
        b.step(h).unwrap();
        assert_eq!(format!("{:?}", a.m2_state()), format!("{:?}", b.m2_state()));
    }
    assert_eq!(b.l1.hash_reads.len(), 17);
    assert!(b.l1.number_reads.iter().all(|h| *h >= 18));
}
#[test]
fn review_ahead_checkpoint_stalls_without_changing_it() {
    let dir = TestDir::new("ahead");
    let path = dir.0.join("checkpoint.json");
    let mut a = Driver::new(Source::new(), Client::new());
    a.set_journal(&path).unwrap();
    a.step(1).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let mut b = Driver::new(Source::new(), Client::new());
    b.set_journal(&path).unwrap();
    assert!(b.step(1).unwrap_err().reason.contains("height mismatch"));
    assert_eq!(saved, std::fs::read(path).unwrap());
    assert!(b.engine.calls.is_empty());
}

#[test]
fn review_temp_directory_honors_target_and_cleans_on_unwind() {
    let dir = TestDir::new("target-check");
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        assert!(dir.0.starts_with(target));
    }
    let path = dir.0.clone();
    let failure = std::panic::catch_unwind(move || {
        let _guard = dir;
        panic!("injected test failure");
    });
    assert!(failure.is_err());
    assert!(!path.exists());
}

#[test]
fn v11_slot_digests_are_on_wait_derived_and_stall_evidence() {
    let dir = TestDir::new("slot-digests");
    let path = dir.0.join("evidence.jsonl");
    let mut d = Driver::new(Source::new(), Client::new());
    d.set_evidence(&path).unwrap();
    assert!(matches!(
        d.step(36).unwrap(),
        StepOutcome::WaitingForFinality
    ));
    d.step(1).unwrap();
    d.engine.fail = true;
    assert!(d.step(2).is_err());
    let rows = evidence_rows(&path);
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(
            row["slot_table_digest"],
            boole_derivation::attributes::hex_string(&Config::devnet().slot_table_digest())
        );
        assert_eq!(
            row["m2_config_digest"],
            boole_derivation::attributes::hex_string(&Config::devnet().digest())
        );
    }
}
