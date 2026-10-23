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
        }
    }
    fn read(&self, h: u64) {
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
        Ok(self.rows[h as usize].clone())
    }
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
fn t84_explicit_engine_tail_recovery_reaches_saved_checkpoint_without_moving_it_backwards(
) -> Result<()> {
    let dir = PrivateTempDir::new("m4-engine-tail")?;
    let path = dir.path().join("checkpoint.json");
    let mut raw = TipOnly::new();
    raw.tip = 3;
    let mut a = Driver::new(raw.clone(), MemoryEngine::new());
    a.set_journal(&path)?;
    for h in 1..=3 {
        a.step(h)?;
    }
    let saved = std::fs::read(&path)?;
    let expected = a.engine.head.clone();
    let mut truncated = a.engine.clone();
    truncated.head = truncated.headers[&1].clone();
    truncated.headers.retain(|h, _| *h <= 1);
    truncated.txs.retain(|h, _| *h <= 1);
    drop(a);
    let mut b = Driver::new(raw.clone(), truncated.clone());
    b.set_journal(&path)?;
    assert!(b.step(2).is_err());
    assert_eq!(std::fs::read(&path)?, saved);
    let mut c = Driver::new(raw, truncated);
    c.set_journal(&path)?;
    c.set_recover_engine_tail(true);
    c.step(2)?;
    assert_eq!(std::fs::read(&path)?, saved);
    c.step(3)?;
    assert_eq!(c.engine.head.hash, expected.hash);
    assert_eq!(std::fs::read(&path)?, saved);
    Ok(())
}

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
            .args(["--exact", "t83_fixture_child", "--ignored", "--nocapture"])
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

#[test]
fn t84_mismatching_tail_is_refused_before_checkpoint_height_import() -> Result<()> {
    for field in ["l2_hash", "m2_root"] {
        let dir = PrivateTempDir::new("m4-bad-tail")?;
        let path = dir.path().join("checkpoint.json");
        let mut raw = TipOnly::new();
        raw.tip = 3;
        let mut a = Driver::new(raw.clone(), MemoryEngine::new());
        a.set_journal(&path)?;
        for h in 1..=3 {
            a.step(h)?;
        }
        let mut engine = a.engine.clone();
        engine.head = engine.headers[&1].clone();
        drop(a);
        let mut saved: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        saved[field] = json!(format!("0x{}", "ff".repeat(32)));
        std::fs::write(&path, serde_json::to_vec(&saved)?)?;
        let before = std::fs::read(&path)?;
        let mut d = Driver::new(raw, engine);
        d.set_journal(&path)?;
        d.set_recover_engine_tail(true);
        d.step(2)?;
        assert_eq!(std::fs::read(&path)?, before);
        let error = d.step(3).unwrap_err();
        assert!(matches!(
            error.kind,
            boole_devnet_driver::driver::StallKind::Integrity { height: 3 }
        ));
        assert_eq!(d.engine.head.number, 2);
        assert_eq!(std::fs::read(&path)?, before);
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
