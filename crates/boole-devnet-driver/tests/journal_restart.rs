//! T77: journal-only history with a transport that panics below its current tip.
use anyhow::{bail, Result};
use boole_derivation::{attributes::hex_bytes, InboxLog, L1Block, ParentHeader};
use boole_derivation::{attributes::hex_string, rlp, user_tx::keccak};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::{receipts_root, InputRow},
    journal::{InputJournal, NetworkIdentity},
};
use boole_devnet_driver::{
    driver::Driver,
    engine::Engine,
    l1::{JournalSource, L1Source},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn empty_fixture(number: u64, parent: [u8; 32]) -> InputRow {
    let fields = vec![
        rlp::bytes(&parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&[0; 256]),
        rlp::uint(0),
        rlp::uint(number),
        rlp::uint(60_000_000),
        rlp::uint(0),
        rlp::uint(1_790_000_000 + 12 * number),
        rlp::bytes(&[]),
        rlp::bytes(&[1; 32]),
        rlp::bytes(&[0; 8]),
        rlp::uint(1),
        rlp::bytes(&[0; 32]),
        rlp::uint(0),
        rlp::uint(0),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 32]),
    ];
    let raw = rlp::list(&fields);
    InputRow {
        schema_version: 1,
        block_hash: hex_string(&keccak(&raw)),
        header_rlp: hex_string(&raw),
        receipts: Some(vec![]),
        transactions: Some(vec![]),
    }
}

#[derive(Clone)]
struct TipOnly {
    rows: Vec<InputRow>,
    tip: u64,
    strict: bool,
    latest: Option<u64>,
    offline: bool,
    reorg_receipts_at: Option<u64>,
}
impl TipOnly {
    fn new() -> Self {
        let mut rows = vec![empty_fixture(0, [0; 32])];
        for h in 1..=8 {
            let parent = hex_bytes(&rows.last().unwrap().block_hash, "hash")
                .unwrap()
                .try_into()
                .unwrap();
            rows.push(empty_fixture(h, parent));
        }
        Self {
            rows,
            tip: 0,
            strict: false,
            latest: None,
            offline: false,
            reorg_receipts_at: None,
        }
    }
    fn read(&self, h: u64) {
        assert!(!self.offline, "Engine tail replay touched L1 transport");
        assert!(
            !self.strict || h >= self.tip,
            "TIP_ONLY transport read below tip: {h} < {}",
            self.tip
        );
    }
}
impl L1Source for TipOnly {
    fn latest(&mut self) -> Result<u64> {
        Ok(self.latest.unwrap_or(self.tip))
    }
    fn finalized(&mut self) -> Result<u64> {
        assert!(!self.offline, "Engine tail replay refreshed L1");
        Ok(self.tip)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        self.read(h);
        self.rows[h as usize].block()
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        let row = self.rows.iter().find(|r| r.block_hash == hash).unwrap();
        self.read(row.block()?.number);
        row.block()
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        let row = self.rows.iter().find(|r| r.block_hash == hash).unwrap();
        self.read(row.block()?.number);
        Ok(row.verify()?.logs)
    }
    fn input_row(&mut self, h: u64) -> Result<InputRow> {
        self.read(h);
        let mut row = self.rows[h as usize].clone();
        if self.reorg_receipts_at == Some(h) {
            row.receipts = Some(vec!["0xc0".into()]);
        }
        Ok(row)
    }
    fn header_rlp(&mut self, h: u64) -> Result<String> {
        self.read(h);
        Ok(self.rows[h as usize].header_rlp.clone())
    }
}

#[test]
fn live_capture_reorg_between_header_and_receipts_is_retryable_not_sticky() -> Result<()> {
    let d = PrivateTempDir::new("m4-live-reorg")?;
    let mut raw = TipOnly::new();
    raw.latest = Some(2);
    raw.strict = true;
    raw.reorg_receipts_at = Some(1);
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        MemoryEngine::new().head.hash,
        30_000_000,
    )?;
    let mut source = JournalSource::open(raw, d.path(), network, false)?;
    let error = source.finalized().unwrap_err();
    assert!(
        matches!(
            error.downcast_ref::<boole_devnet_archive::error::Error>(),
            Some(boole_devnet_archive::error::Error::Availability { height: 1, .. })
        ),
        "live reorg became sticky: {error:#}"
    );
    assert_eq!(source.journal.borrow().tip(), 0);
    source.inner.reorg_receipts_at = None;
    source.finalized()?;
    source.inner.tip = 1;
    assert_eq!(source.finalized()?, 1);
    assert_eq!(source.journal.borrow().tip(), 1);
    Ok(())
}
#[derive(Clone)]
struct MemoryEngine {
    head: ParentHeader,
    headers: BTreeMap<u64, ParentHeader>,
    txs: BTreeMap<u64, Vec<String>>,
    attributes: Value,
}
impl MemoryEngine {
    fn new() -> Self {
        let rows: Value = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let mut head: ParentHeader = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
        head.timestamp = 1_790_000_000;
        Self {
            headers: BTreeMap::from([(0, head.clone())]),
            head,
            txs: BTreeMap::new(),
            attributes: Value::Null,
        }
    }
}
impl Engine for MemoryEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        Ok(self.headers[&h].clone())
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.txs[&h].clone())
    }
    fn call(&mut self, method: &str, p: Value) -> Result<Value> {
        Ok(match method {
            "engine_forkchoiceUpdatedV3" => {
                if p[1].is_null() {
                    let h = self.head.number + 1;
                    let txs: Vec<String> =
                        serde_json::from_value(self.attributes["transactions"].clone())?;
                    self.head.parent_hash = Some(self.head.hash.clone());
                    self.head.hash = p[0]["headBlockHash"].as_str().unwrap().into();
                    self.head.number = h;
                    self.head.timestamp = u64::from_str_radix(
                        self.attributes["timestamp"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )?;
                    self.head.l1_info = Some(txs[0].clone());
                    self.head.m2_root_deposit = Some(txs[1].clone());
                    self.headers.insert(h, self.head.clone());
                    self.txs.insert(h, txs);
                } else {
                    self.attributes = p[1].clone();
                }
                json!({"payloadStatus":{"status":"VALID"},"payloadId":"0x1"})
            }
            "engine_getPayloadV5" => {
                json!({"executionPayload":{"transactions":self.attributes["transactions"],"blockNumber":format!("0x{:x}",self.head.number+1),"blockHash":format!("0x{:064x}",self.head.number+1),"parentHash":self.head.hash,"timestamp":self.attributes["timestamp"],"gasLimit":self.attributes["gasLimit"]}})
            }
            "engine_newPayloadV4" => {
                self.attributes = p[0].clone();
                json!({"status":"VALID"})
            }
            _ => bail!("unexpected engine method"),
        })
    }
}
#[test]
fn t77_driver_restarts_from_verified_rows_without_any_historical_l1_read() -> Result<()> {
    let dir = PrivateTempDir::new("m4-tip-only")?;
    let engine = MemoryEngine::new();
    let config = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(&config, None, engine.head.hash.clone(), 30_000_000)?;
    let mut raw = TipOnly::new();
    // Simulate already captured node-owned inputs independently of replay.
    let mut journal = InputJournal::open(
        dir.path(),
        network.clone(),
        || Ok(raw.rows[0].clone()),
        false,
    )?;
    journal.append(&raw.rows[1])?;
    journal.append(&raw.rows[2])?;
    drop(journal);
    raw.tip = 2;
    let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    let mut a = Driver::new(source, engine);
    a.step(1)?;
    a.step(2)?;
    let engine = a.engine.clone();
    let mut raw = a.l1.inner.clone();
    drop(a);
    raw.tip = 3;
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    let mut restarted = Driver::new(source, engine);
    restarted.step(3)?;
    assert_eq!(restarted.m2_state().height(), 3);
    assert_eq!(restarted.l1.journal.borrow().tip(), 3);
    Ok(())
}

#[test]
fn t77_finality_jump_uses_previously_captured_unfinalized_cache_only() -> Result<()> {
    let dir = PrivateTempDir::new("m4-epoch-jump")?;
    let engine = MemoryEngine::new();
    let config = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(&config, None, engine.head.hash, 30_000_000)?;
    let mut raw = TipOnly::new();
    raw.latest = Some(8);
    raw.strict = true;
    let mut source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    assert_eq!(source.finalized()?, 0);
    assert_eq!(
        source.journal.borrow().tip(),
        0,
        "unfinalized cache is never fold authority"
    );
    let mut raw = source.inner.clone();
    drop(source);
    raw.tip = 4;
    let mut source = JournalSource::open(raw, dir.path(), network, false)?;
    assert_eq!(source.finalized()?, 4);
    assert_eq!(source.journal.borrow().tip(), 4);
    assert_eq!(source.block(1)?.number, 1);
    Ok(())
}
#[test]
fn t77_missing_expired_input_is_availability_without_partial_journal_publication() -> Result<()> {
    let dir = PrivateTempDir::new("m4-epoch-gap")?;
    let engine = MemoryEngine::new();
    let config = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(&config, None, engine.head.hash, 30_000_000)?;
    let mut raw = TipOnly::new();
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    let mut raw = source.inner.clone();
    drop(source);
    raw.tip = 4;
    let mut source = JournalSource::open(raw, dir.path(), network, false)?;
    let error = source.finalized().unwrap_err();
    assert!(matches!(
        error.downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Availability { .. })
    ));
    assert_eq!(source.journal.borrow().tip(), 0);
    Ok(())
}
#[test]
fn t84_automatic_engine_tail_recovery_is_tip_only_and_never_moves_checkpoint_backwards(
) -> Result<()> {
    let dir = PrivateTempDir::new("m4-engine-tail")?;
    let path = dir.path().join("checkpoint.json");
    let mut raw = TipOnly::new();
    raw.tip = 3;
    let engine = MemoryEngine::new();
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(
        dir.path(),
        network.clone(),
        || Ok(raw.rows[0].clone()),
        false,
    )?;
    for h in 1..=3 {
        journal.append(&raw.rows[h])?;
    }
    drop(journal);
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    let mut a = Driver::new(source, engine);
    a.set_journal(&path)?;
    for h in 1..=3 {
        a.step(h)?;
    }
    let saved = std::fs::read(&path)?;
    let expected = a.engine.head.clone();
    let mut truncated = a.engine.clone();
    let mut raw = a.l1.inner.clone();
    raw.offline = true;
    truncated.head = truncated.headers[&1].clone();
    truncated.headers.retain(|h, _| *h <= 1);
    truncated.txs.retain(|h, _| *h <= 1);
    drop(a);
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    let mut b = Driver::new(source, truncated);
    b.set_journal(&path)?;
    b.step(2)?;
    assert_eq!(std::fs::read(&path)?, saved);
    b.resume()?;
    assert_eq!(b.engine.head.hash, expected.hash);
    assert_eq!(std::fs::read(&path)?, saved);
    Ok(())
}

#[cfg(feature = "qualification-faults")]
mod kill_qualification {
    use super::*;
    struct FileEngine {
        inner: MemoryEngine,
        path: std::path::PathBuf,
    }
    impl FileEngine {
        fn open(path: &std::path::Path) -> Result<Self> {
            Ok(Self {
                inner: if path.exists() {
                    let v: Value = serde_json::from_slice(&std::fs::read(path)?)?;
                    MemoryEngine {
                        head: serde_json::from_value(v["head"].clone())?,
                        headers: serde_json::from_value(v["headers"].clone())?,
                        txs: serde_json::from_value(v["txs"].clone())?,
                        attributes: v["attributes"].clone(),
                    }
                } else {
                    MemoryEngine::new()
                },
                path: path.into(),
            })
        }
        fn save(&self) -> Result<()> {
            let tmp = self.path.with_extension("next");
            std::fs::write(
                &tmp,
                serde_json::to_vec(
                    &json!({"head":self.inner.head,"headers":self.inner.headers,"txs":self.inner.txs,"attributes":self.inner.attributes}),
                )?,
            )?;
            std::fs::File::open(&tmp)?.sync_all()?;
            std::fs::rename(tmp, &self.path)?;
            boole_devnet_archive::durability::fsync_parent_dir(&self.path)
        }
    }
    impl Engine for FileEngine {
        fn head(&mut self) -> Result<ParentHeader> {
            self.inner.head()
        }
        fn header(&mut self, h: u64) -> Result<ParentHeader> {
            self.inner.header(h)
        }
        fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
            self.inner.transactions(h)
        }
        fn call(&mut self, m: &str, p: Value) -> Result<Value> {
            let result = self.inner.call(m, p)?;
            self.save()?;
            Ok(result)
        }
    }
    fn child_run(root: &std::path::Path, repair: bool) -> Result<()> {
        let engine = FileEngine::open(&root.join("engine.json"))?;
        let network = NetworkIdentity::new(
            &boole_derivation::m2::Config::devnet(),
            None,
            engine.inner.headers[&0].hash.clone(),
            30_000_000,
        )?;
        let mut raw = TipOnly::new();
        raw.tip = 4;
        raw.strict = true;
        let source = JournalSource::open(raw, root, network, repair)?;
        source
            .journal
            .borrow()
            .cleanup(&[], Some(&root.join("checkpoint.json")))?;
        let mut driver = Driver::new(source, engine);
        driver.set_journal(&root.join("checkpoint.json"))?;
        driver.set_evidence_repair(&root.join("evidence.jsonl"), repair)?;
        driver.resume()?;
        let first = driver.engine.head()?.number + 1;
        for h in first..=4 {
            driver.step(h)?;
        }
        assert_eq!(driver.m2_state().height(), 4);
        std::fs::write(
            root.join("result.json"),
            serde_json::to_vec(
                &json!({"hash":driver.engine.head()?.hash,"root":hex_string(&driver.m2_state().root)}),
            )?,
        )?;
        Ok(())
    }
    #[test]
    #[ignore = "fixture subprocess only"]
    fn t83_fixture_child() -> Result<()> {
        let Some(root) = std::env::var_os("BOOLE_M4_T83_ROOT") else {
            return Ok(());
        };
        child_run(
            std::path::Path::new(&root),
            std::env::var_os("BOOLE_M4_REPAIR_FIXTURE").is_some(),
        )
    }
    #[test]
    fn t83_real_sigkill_at_journal_build_import_selection_evidence_and_checkpoint_boundaries(
    ) -> Result<()> {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let mut reference = None;
        for point in ["B1", "B2", "B3", "B4", "B5", "B6", "B7", "B9", "B10"] {
            let dir = PrivateTempDir::new("m4-kill")?;
            let root = dir.path().canonicalize()?;
            let engine = MemoryEngine::new();
            let network = NetworkIdentity::new(
                &boole_derivation::m2::Config::devnet(),
                None,
                engine.head.hash,
                30_000_000,
            )?;
            let mut raw = TipOnly::new();
            raw.latest = Some(8);
            let mut source = JournalSource::open(raw, &root, network, false)?;
            // For B9 capture only 0..1 here and make the child publish a new tip-4 object.
            if point == "B9" {
                source.inner.latest = Some(1);
            }
            source.finalized()?;
            drop(source);
            // The B9 run cannot recover a gap: pre-cache 2..3 under another unfinalized live head first.
            if point == "B9" {
                let mut raw = TipOnly::new();
                raw.latest = Some(3);
                let network = NetworkIdentity::new(
                    &boole_derivation::m2::Config::devnet(),
                    None,
                    MemoryEngine::new().head.hash,
                    30_000_000,
                )?;
                let mut source = JournalSource::open(raw, &root, network, false)?;
                source.finalized()?;
                drop(source);
            }
            let marker = root.join("kill-marker.json");
            let log = std::fs::File::create(root.join("child.log"))?;
            let height = if point == "B9" { 4 } else { 2 };
            let role = if point == "B9" {
                "staged-input"
            } else if point == "B10" {
                "evidence"
            } else {
                "driver"
            };
            let mut child = Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "kill_qualification::t83_fixture_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("BOOLE_M4_T83_ROOT", &root)
                .env("BOOLE_M4_KILL_POINT", point)
                .env("BOOLE_M4_KILL_HEIGHT", height.to_string())
                .env("BOOLE_M4_KILL_ROLE", role)
                .env("BOOLE_M4_KILL_MARKER", &marker)
                .stdout(log.try_clone()?)
                .stderr(log)
                .stdin(Stdio::null())
                .spawn()?;
            let end = Instant::now() + Duration::from_secs(10);
            while boole_devnet_archive::fault::stopped_marker(&marker, child.id())?.is_none() {
                if let Some(status) = child.try_wait()? {
                    bail!(
                        "{point} child exited before boundary: {status}: {}",
                        std::fs::read_to_string(root.join("child.log"))?
                    );
                }
                if Instant::now() > end {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("{point} boundary timeout");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let hit: Value = serde_json::from_slice(&std::fs::read(marker)?)?;
            assert_eq!(hit["pid"], child.id());
            assert_eq!(hit["point"], point);
            child.kill()?;
            let status = child.wait()?;
            assert!(!status.success());
            if point == "B10" {
                let before = std::fs::read(root.join("evidence.jsonl"))?;
                let error = child_run(&root, false).unwrap_err();
                assert!(error
                    .downcast_ref::<boole_devnet_archive::error::Error>()
                    .is_some());
                assert_eq!(std::fs::read(root.join("evidence.jsonl"))?, before);
                child_run(&root, true)?;
            } else {
                child_run(&root, false)?;
            }
            let result = std::fs::read(root.join("result.json"))?;
            if let Some(expected) = &reference {
                assert_eq!(&result, expected, "{point} diverged");
            } else {
                reference = Some(result);
            }
            if point == "B4" {
                let log = std::fs::read_to_string(root.join("evidence.jsonl"))?;
                assert!(log.lines().any(|s| serde_json::from_str::<Value>(s)
                    .is_ok_and(|v| v["height"] == 2 && v["outcome"] == "RECOVERED")));
            }
        }
        Ok(())
    }
}

fn store_files(root: &std::path::Path) -> Result<BTreeMap<std::path::PathBuf, Vec<u8>>> {
    fn visit(
        root: &std::path::Path,
        dir: &std::path::Path,
        files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let p = entry?.path();
            if p.is_dir() {
                visit(root, &p, files)?;
            } else {
                files.insert(p.strip_prefix(root)?.to_owned(), std::fs::read(p)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

#[test]
fn t84_mismatching_tail_is_sticky_and_preserves_every_store_file() -> Result<()> {
    for field in ["l2_hash", "m2_root"] {
        let dir = PrivateTempDir::new("m4-bad-tail")?;
        let path = dir.path().join("checkpoint.json");
        let evidence = dir.path().join("evidence.jsonl");
        let mut raw = TipOnly::new();
        raw.tip = 3;
        let engine = MemoryEngine::new();
        let network = NetworkIdentity::new(
            &boole_derivation::m2::Config::devnet(),
            None,
            engine.head.hash.clone(),
            30_000_000,
        )?;
        let mut journal = InputJournal::open(
            dir.path(),
            network.clone(),
            || Ok(raw.rows[0].clone()),
            false,
        )?;
        for h in 1..=3 {
            journal.append(&raw.rows[h])?;
        }
        drop(journal);
        raw.strict = true;
        let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
        let mut a = Driver::new(source, engine);
        a.set_journal(&path)?;
        a.set_evidence(&evidence)?;
        for h in 1..=3 {
            a.step(h)?;
        }
        let mut engine = a.engine.clone();
        engine.head = engine.headers[&1].clone();
        let raw = a.l1.inner.clone();
        drop(a);
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        saved[field] = json!(format!("0x{}", "ff".repeat(32)));
        std::fs::write(&path, serde_json::to_vec(&saved)?)?;
        let before = store_files(dir.path())?;
        let source = JournalSource::open(raw, dir.path(), network, false)?;
        let mut d = Driver::new(source, engine);
        d.set_journal(&path)?;
        d.set_evidence(&evidence)?;
        d.step(2)?;
        assert_eq!(
            std::fs::read(&path)?,
            before[&std::path::PathBuf::from("checkpoint.json")]
        );
        let error = d.step(3).unwrap_err();
        assert!(matches!(
            error.kind,
            boole_devnet_driver::driver::StallKind::Integrity { height: 3 }
        ));
        assert_eq!(
            d.engine.head.number, 2,
            "mismatching checkpoint payload was not imported"
        );
        assert!(
            store_files(dir.path())? == before,
            "mismatch changed a store file"
        );
        let again = d.step(3).unwrap_err();
        assert_eq!(again.kind, error.kind);
        assert_eq!(again.reason, error.reason);
        assert!(
            store_files(dir.path())? == before,
            "sticky stop changed a store file"
        );
    }
    Ok(())
}

#[test]
fn t83_recovered_follow_evidence_never_claims_computed_verification() -> Result<()> {
    use boole_gate_supervisor::{
        codec::Codec,
        process::{Envelope, Worker},
        stub::Descriptor,
        supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
    };
    use sha2::{Digest, Sha256};
    let dir = PrivateTempDir::new("m4-follow-recovery")?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let gate = target.join("debug/boole-gate-supervisor");
    let pin: [u8; 32] = Sha256::digest(std::fs::read(&gate)?).into();
    let descriptor = Descriptor::pinned(pin);
    let mut config = boole_derivation::m2::Config::devnet();
    config.m3 = Some(boole_derivation::m3::M3Config::devnet(
        config.slot_table.len(),
    ));
    config.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor.digest();
    let forks = boole_derivation::m3::ForkTable::genesis(&config)?;
    let artifacts = dir.path().join("artifacts");
    install_devnet_artifacts(&config, &artifacts)?;
    let make_supervisor = |cache: &std::path::Path| -> Result<_> {
        Supervisor::new(
            &config,
            &forks,
            Mode::Follow {
                from: 1,
                through: 4,
            },
            vec![Worker {
                descriptor: descriptor.clone(),
                binary: gate.clone(),
                binary_sha256: pin,
            }],
            Some(Codec::portable_stub()),
            cache,
            cache,
            &artifacts,
            Host::local()?,
            Envelope::default(),
        )
    };
    let root = dir.path().join("state");
    let engine = MemoryEngine::new();
    let identity =
        NetworkIdentity::new(&config, Some(&forks), engine.head.hash.clone(), 30_000_000)?;
    let mut raw = TipOnly::new();
    let mut journal =
        InputJournal::open(&root, identity.clone(), || Ok(raw.rows[0].clone()), false)?;
    for h in 1..=3 {
        journal.append(&raw.rows[h])?;
    }
    drop(journal);
    raw.tip = 3;
    raw.strict = true;
    let source = JournalSource::open(raw.clone(), &root, identity.clone(), false)?;
    let mut d = Driver::with_m3(
        source,
        engine,
        config.clone(),
        30_000_000,
        make_supervisor(&dir.path().join("first-cache"))?,
        forks.clone(),
    )?;
    let checkpoint = root.join("checkpoint.json");
    let evidence = root.join("evidence.jsonl");
    d.set_journal(&checkpoint)?;
    d.set_evidence(&evidence)?;
    d.step(1)?;
    let saved_cp = std::fs::read(&checkpoint)?;
    let saved_evidence = std::fs::read(&evidence)?;
    d.step(2)?;
    let engine = d.engine.clone();
    drop(d);
    // Exact B4 filesystem state: selected block2, only complete height1 evidence/checkpoint.
    std::fs::write(&checkpoint, saved_cp)?;
    std::fs::write(&evidence, saved_evidence)?;
    let source = JournalSource::open(raw, &root, identity, false)?;
    let supervisor = make_supervisor(&dir.path().join("second-cache"))?;
    let mut d = Driver::with_m3(source, engine, config, 30_000_000, supervisor, forks)?;
    d.set_journal(&checkpoint)?;
    d.set_evidence(&evidence)?;
    d.resume()?;
    let rows: Vec<Value> = std::fs::read_to_string(&evidence)?
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let recovered = rows
        .iter()
        .find(|v| v["height"] == 2 && v["outcome"] == "RECOVERED")
        .unwrap();
    assert_eq!(
        recovered["verification"], "checkpoint-trusted",
        "a followed verdict was never computed locally"
    );
    Ok(())
}

#[test]
fn t84_operator_checkpoint_move_refuses_immutable_evidence_without_changing_files() -> Result<()> {
    let dir = PrivateTempDir::new("m4-operator-tail")?;
    let path = dir.path().join("checkpoint.json");
    let evidence = dir.path().join("evidence.jsonl");
    let engine = MemoryEngine::new();
    let mut raw = TipOnly::new();
    raw.tip = 3;
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(
        dir.path(),
        network.clone(),
        || Ok(raw.rows[0].clone()),
        false,
    )?;
    for h in 1..=3 {
        journal.append(&raw.rows[h])?;
    }
    drop(journal);
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    let mut a = Driver::new(source, engine);
    a.set_journal(&path)?;
    a.set_evidence(&evidence)?;
    for h in 1..=3 {
        a.step(h)?;
    }
    let mut engine = a.engine.clone();
    engine.head = engine.headers[&1].clone();
    let raw = a.l1.inner.clone();
    drop(a);
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    let mut d = Driver::new(source, engine);
    d.set_journal(&path)?;
    d.set_evidence(&evidence)?;
    d.set_recover_engine_tail(true);
    let before = store_files(dir.path())?;
    let error = d.step(2).unwrap_err();
    assert!(matches!(
        error.kind,
        boole_devnet_driver::driver::StallKind::Integrity { .. }
    ));
    assert!(error.reason.contains("operator-checkpoint-move"));
    assert!(store_files(dir.path())? == before);
    assert_eq!(d.engine.head.number, 1);
    Ok(())
}

#[test]
fn t84_missing_own_tail_pauses_without_rpc_or_checkpoint_movement() -> Result<()> {
    let dir = PrivateTempDir::new("m4-missing-tail")?;
    let path = dir.path().join("checkpoint.json");
    let engine = MemoryEngine::new();
    let mut raw = TipOnly::new();
    raw.tip = 3;
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(
        dir.path(),
        network.clone(),
        || Ok(raw.rows[0].clone()),
        false,
    )?;
    for h in 1..=2 {
        journal.append(&raw.rows[h])?;
    }
    drop(journal);
    let mut a = Driver::new(raw.clone(), engine);
    a.set_journal(&path)?;
    for h in 1..=3 {
        a.step(h)?;
    }
    let mut engine = a.engine.clone();
    engine.head = engine.headers[&1].clone();
    drop(a);
    raw.offline = true;
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    let mut d = Driver::new(source, engine);
    d.set_journal(&path)?;
    let before = store_files(dir.path())?;
    let error = d.step(2).unwrap_err();
    assert!(matches!(
        error.kind,
        boole_devnet_driver::driver::StallKind::Availability { height: 3 }
    ));
    assert!(error.reason.contains("own journal cannot cover"));
    assert!(store_files(dir.path())? == before);
    assert_eq!(d.engine.head.number, 1);
    assert!(
        d.last_failure().is_none(),
        "journal availability must remain resumable"
    );
    Ok(())
}

#[test]
fn checkpoint_replace_uses_an_attempt_nonce_and_preserves_unrecognized_old_temps() -> Result<()> {
    let d = PrivateTempDir::new("m4-checkpoint-nonce")?;
    let p = d.path().join("checkpoint.json");
    let old = p.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&old, b"unrecognized previous-attempt bytes")?;
    let mut raw = TipOnly::new();
    raw.tip = 1;
    let mut driver = Driver::new(raw, MemoryEngine::new());
    driver.set_journal(&p)?;
    driver.step(1)?;
    assert_eq!(std::fs::read(old)?, b"unrecognized previous-attempt bytes");
    let cp: Value = serde_json::from_slice(&std::fs::read(p)?)?;
    assert_eq!(cp["height"], 1);
    Ok(())
}

#[test]
fn finalized_capture_verification_failure_remains_a_sticky_integrity_stop() -> Result<()> {
    let d = PrivateTempDir::new("m4-finalized-integrity")?;
    let raw = TipOnly::new();
    let engine = MemoryEngine::new();
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let mut source = JournalSource::open(raw, d.path(), network, false)?;
    source.inner.tip = 1;
    source.inner.reorg_receipts_at = Some(1);
    let mut driver = Driver::new(source, engine);
    let error = driver.step(1).unwrap_err();
    assert!(matches!(
        error.kind,
        boole_devnet_driver::driver::StallKind::Integrity { height: 1 }
    ));
    assert_eq!(driver.l1.journal.borrow().tip(), 0);
    driver.l1.inner.reorg_receipts_at = None;
    assert_eq!(driver.step(1).unwrap_err().reason, error.reason);
    assert_eq!(driver.engine.head.number, 0);
    Ok(())
}

#[test]
fn t78_fresh_driver_joins_verified_archive_when_own_l1_refuses_history() -> Result<()> {
    use boole_devnet_archive::{
        archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore},
        client::ArchiveClient,
        http::ArchiveServer,
    };
    use std::sync::{Arc, Mutex};
    let root = PrivateTempDir::new("m4-archive-join")?;
    let config = boole_derivation::m2::Config::devnet();
    let raw = TipOnly::new();
    let rows = raw.rows.clone();
    let engine = MemoryEngine::new();
    let network = NetworkIdentity::new(&config, None, engine.head.hash.clone(), 30_000_000)?;
    let profile = ArchiveProfile {
        identity: ArchiveIdentity {
            network: network.clone(),
            l1_anchor_number: 0,
            l1_anchor_hash: rows[0].block_hash.clone(),
        },
        bind: "127.0.0.1:0".parse()?,
        prune_before: None,
    };
    let x = Arc::new(Mutex::new(ArchiveStore::open(
        &root.path().join("x"),
        profile,
        false,
    )?));
    let source = JournalSource::open(raw, &root.path().join("a"), network.clone(), false)?;
    let mut a = Driver::with_config(source, engine, config.clone(), 30_000_000)?;
    a.resume()?;
    let mut sink =
        boole_devnet_driver::driver::archive::ArchiveSink::new(&mut a, Arc::clone(&x), None)?;
    sink.synchronize(&mut a)?;
    let mut expected = vec![];
    for h in 1..=8 {
        a.l1.inner.tip = h;
        a.step(h)?;
        let head = a.engine.head()?;
        expected.push((
            head.hash.clone(),
            a.m2_state().root,
            a.engine.transactions(h)?,
        ));
        sink.synchronize(&mut a)?;
    }
    // Reopening a sink replays only this driver's verified journal/Engine and
    // safely fills any missing public index suffix, without re-applying a fold.
    drop(sink);
    let mut sink =
        boole_devnet_driver::driver::archive::ArchiveSink::new(&mut a, Arc::clone(&x), None)?;
    sink.synchronize(&mut a)?;
    let server = ArchiveServer::start(x, "127.0.0.1:0".parse()?)?;
    let mut own = TipOnly::new();
    own.tip = 8;
    own.strict = true;
    let dir = root.path().join("c");
    let source = JournalSource::open_archive(
        own,
        &dir,
        network.clone(),
        false,
        ArchiveClient::http(vec![server.url()])?,
    )?;
    assert_eq!(source.inputs_label(1), "archive-verified");
    assert_eq!(
        std::fs::read_dir(dir.join("archive-download"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().len() == 64)
            .count(),
        0,
        "appended header cache was retained"
    );
    let mut c = Driver::with_config(source, MemoryEngine::new(), config.clone(), 30_000_000)?;
    c.set_journal(&dir.join("checkpoint.json"))?;
    c.set_evidence(&dir.join("evidence.jsonl"))?;
    c.resume()?;
    for h in 1..=8 {
        c.step(h)?;
        let (hash, root, txs) = &expected[h as usize - 1];
        assert_eq!(&c.engine.head()?.hash, hash);
        assert_eq!(&c.m2_state().root, root);
        assert_eq!(&c.engine.transactions(h)?, txs);
    }
    let engine = c.engine.clone();
    drop(c);
    // B1 crash: the journal append is durable, its disposable header copy remains.
    let cache = boole_devnet_archive::objects::Objects::open(&dir.join("archive-download"))?;
    let raw = boole_derivation::attributes::hex_bytes(&rows[3].header_rlp, "header")?;
    cache.put(&rows[3].block_hash, &raw, |b| {
        boole_devnet_archive::archive::verify_header(&rows[3].block_hash, b)
    })?;
    std::fs::write(
        dir.join("archive-download").join(&rows[3].block_hash[2..]),
        b"corrupt already-journaled cache copy",
    )?;
    drop(cache);
    let source = JournalSource::open_archive(
        TipOnly {
            offline: false,
            tip: 8,
            strict: true,
            ..TipOnly::new()
        },
        &dir,
        network,
        false,
        ArchiveClient::http(vec![server.url()])?,
    )?;
    let mut restarted = Driver::with_config(source, engine, config.clone(), 30_000_000)?;
    restarted.set_journal(&dir.join("checkpoint.json"))?;
    restarted.resume()?;
    assert_eq!(restarted.m2_state().root, expected.last().unwrap().1);
    assert_eq!(restarted.l1.inputs_label(1), "journal");
    // Export the public GET tree, then join from static files with no store
    // or HTTP fallback behind this Fetch. Every hash/root/prefix must match.
    let mirror = root.path().join("static-mirror");
    let mut transport = boole_devnet_archive::client::HttpFetch;
    use boole_devnet_archive::client::Fetch;
    for route in std::iter::once("info".to_owned())
        .chain(rows.iter().flat_map(|row| {
            let mut routes = vec![format!("l1/header/{}", &row.block_hash[2..])];
            if boole_devnet_archive::input::header_needs_receipts(
                &boole_derivation::attributes::hex_bytes(&row.header_rlp, "mirror").unwrap(),
            )
            .unwrap()
            {
                routes.push(format!("l1/receipts/{}", &row.block_hash[2..]));
            }
            routes
        }))
        .chain((0..=8).map(|h| format!("index/{h}?limit=1")))
    {
        let bytes = transport.get(
            &server.url(),
            &route,
            boole_devnet_archive::archive::OBJ_MAX,
        )?;
        let dest = mirror.join(route.split('?').next().unwrap());
        std::fs::create_dir_all(dest.parent().unwrap())?;
        std::fs::write(dest, bytes)?;
    }
    let mut own = TipOnly::new();
    own.tip = 8;
    own.strict = true;
    let source = JournalSource::open_archive(
        own,
        &root.path().join("static-c"),
        NetworkIdentity::new(&config, None, MemoryEngine::new().head.hash, 30_000_000)?,
        false,
        ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(StaticMirror(mirror)),
        )?,
    )?;
    let mut mirrored =
        Driver::with_config(source, MemoryEngine::new(), config.clone(), 30_000_000)?;
    for h in 1..=8 {
        mirrored.step(h)?;
        let (hash, root, txs) = &expected[h as usize - 1];
        assert_eq!(&mirrored.engine.head()?.hash, hash);
        assert_eq!(&mirrored.m2_state().root, root);
        assert_eq!(&mirrored.engine.transactions(h)?, txs);
    }
    let prefix = root.path().join("prefix");
    let mut journal = InputJournal::open(
        &prefix,
        NetworkIdentity::new(&config, None, MemoryEngine::new().head.hash, 30_000_000)?,
        || Ok(rows[0].clone()),
        false,
    )?;
    journal.append(&rows[1])?;
    journal.append(&rows[2])?;
    drop(journal);
    let mut own = TipOnly::new();
    own.tip = 8;
    own.strict = true;
    let mut source = JournalSource::open_archive(
        own,
        &prefix,
        NetworkIdentity::new(&config, None, MemoryEngine::new().head.hash, 30_000_000)?,
        false,
        ArchiveClient::http(vec![server.url()])?,
    )?;
    assert_eq!(
        source.finalized()?,
        8,
        "partial archive join did not resume"
    );
    assert_eq!(
        std::fs::read_dir(dir.join("archive-download"))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().len() == 64)
            .count(),
        0,
        "restart retained a durable row cache copy"
    );
    assert_eq!(source.inputs_label(1), "journal");
    assert_eq!(source.inputs_label(3), "archive-verified");
    Ok(())
}

struct StaticMirror(std::path::PathBuf);
impl boole_devnet_archive::client::Fetch for StaticMirror {
    fn get(&mut self, _: &str, route: &str, _: usize) -> Result<Vec<u8>> {
        Ok(std::fs::read(
            self.0.join(route.split('?').next().unwrap()),
        )?)
    }
}

#[test]
fn t78_recovered_selected_blocks_keep_own_journal_input_provenance() -> Result<()> {
    let dir = PrivateTempDir::new("m4-recovered-inputs")?;
    let engine = MemoryEngine::new();
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let source = JournalSource::open(TipOnly::new(), dir.path(), network.clone(), false)?;
    let mut a = Driver::new(source, engine);
    for h in 1..=2 {
        a.l1.inner.tip = h;
        a.step(h)?;
    }
    let root = a.m2_state().root;
    let engine = a.engine.clone();
    drop(a);
    let mut raw = TipOnly::new();
    raw.tip = 2;
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    assert_eq!(source.inputs_label(1), "journal");
    let mut recovered = Driver::new(source, engine);
    let evidence = dir.path().join("evidence.jsonl");
    recovered.set_evidence(&evidence)?;
    recovered.resume()?;
    assert_eq!(recovered.m2_state().root, root);
    let rows: Vec<Value> = std::fs::read_to_string(evidence)?
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(row["outcome"], "RECOVERED");
        assert_eq!(row["verification"], "computed");
        assert_eq!(
            row["inputs"], "journal",
            "recovered evidence omitted input provenance"
        );
    }
    Ok(())
}

#[test]
fn fix_c2_stalling_archive_cannot_hold_a_derived_step() -> Result<()> {
    use boole_devnet_archive::client::{ArchiveClient, Fetch};
    struct Stalled;
    impl Fetch for Stalled {
        fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
            std::thread::sleep(std::time::Duration::from_millis(200));
            anyhow::bail!("unresponsive archive")
        }
    }
    let dir = PrivateTempDir::new("m4-tripwire-budget")?;
    let engine = MemoryEngine::new();
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    drop(JournalSource::open(
        TipOnly::new(),
        dir.path(),
        network.clone(),
        false,
    )?);
    let mut source = JournalSource::open_archive(
        TipOnly::new(),
        dir.path(),
        network,
        false,
        ArchiveClient::new(vec!["http://127.0.0.1:1".into()], Box::new(Stalled))?,
    )?;
    source.inner.tip = 1;
    let mut d = Driver::new(source, engine);
    let start = std::time::Instant::now();
    d.step(1)?;
    let elapsed = start.elapsed();
    assert_eq!(d.m2_state().height(), 1);
    assert!(
        elapsed < std::time::Duration::from_millis(150),
        "archive held derivation for {elapsed:?}"
    );
    Ok(())
}
#[test]
fn fix_c5_deterministic_sink_mismatch_is_sticky_integrity() -> Result<()> {
    use boole_devnet_archive::archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore};
    use boole_devnet_driver::driver::archive::ArchiveSink;
    use std::sync::{Arc, Mutex};
    let dir = PrivateTempDir::new("m4-sink-integrity")?;
    let engine = MemoryEngine::new();
    let raw = TipOnly::new();
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        engine.head.hash.clone(),
        30_000_000,
    )?;
    let store = Arc::new(Mutex::new(ArchiveStore::open(
        &dir.path().join("archive"),
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: network.clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: raw.rows[0].block_hash.clone(),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?));
    let source = JournalSource::open(raw, &dir.path().join("node"), network, false)?;
    let mut d = Driver::new(source, engine);
    d.resume()?;
    let mut sink = ArchiveSink::new(&mut d, store, None)?;
    sink.synchronize(&mut d)?;
    d.l1.inner.tip = 1;
    d.step(1)?;
    let parent = d.engine.headers[&1].parent_hash.clone();
    d.engine.headers.get_mut(&1).unwrap().parent_hash = Some(hex_string(&[255; 32]));
    let err = sink.synchronize(&mut d).unwrap_err();
    assert!(
        matches!(
            err.downcast_ref::<boole_devnet_archive::error::Error>(),
            Some(boole_devnet_archive::error::Error::Integrity {
                class: "archive-replay",
                ..
            })
        ),
        "deterministic mismatch treated as {err:#}"
    );
    d.engine.headers.get_mut(&1).unwrap().parent_hash = parent;
    assert!(
        sink.synchronize(&mut d).is_err(),
        "integrity stop resumed without restart"
    );
    Ok(())
}

#[test]
fn fix2_m1_restart_sink_republishes_unseen_corruption_without_sticky_stop() -> Result<()> {
    use boole_devnet_archive::archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore};
    use boole_devnet_driver::driver::archive::ArchiveSink;
    use std::sync::{Arc, Mutex};
    let dir = PrivateTempDir::new("archive-restart-quarantine")?;
    let raw = TipOnly::new();
    let genesis = raw.rows[0].clone();
    let engine = MemoryEngine::new();
    let config = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(&config, None, engine.head.hash.clone(), 30_000_000)?;
    let public = dir.path().join("public");
    let store = Arc::new(Mutex::new(ArchiveStore::open(
        &public,
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: network.clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: genesis.block_hash.clone(),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?));
    let source = JournalSource::open(raw, &dir.path().join("node"), network, false)?;
    let mut driver = Driver::new(source, engine);
    driver.resume()?;
    let mut sink = ArchiveSink::new(&mut driver, store.clone(), None)?;
    sink.synchronize(&mut driver)?;
    drop(sink);
    std::fs::write(
        public.join("header").join(&genesis.block_hash[2..]),
        b"unseen corrupt header",
    )?;
    let mut sink = ArchiveSink::new(&mut driver, store.clone(), None)?;
    sink.synchronize(&mut driver)?;
    assert_eq!(
        store.lock().unwrap().header(&genesis.block_hash)?.unwrap(),
        hex_bytes(&genesis.header_rlp, "header")?
    );
    let saved =
        std::fs::read_dir(public.join("header/corrupt"))?.collect::<std::io::Result<Vec<_>>>()?;
    assert_eq!(saved.len(), 1);
    assert_eq!(
        std::fs::read(saved[0].path().join("object"))?,
        b"unseen corrupt header"
    );
    driver.l1.inner.tip = 1;
    driver.step(1)?;
    sink.synchronize(&mut driver)?;
    assert_eq!(store.lock().unwrap().index(0, 1024)?.len(), 2);
    Ok(())
}
