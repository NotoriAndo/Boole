use anyhow::{bail, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, State},
    InboxLog, L1Block, ParentHeader,
};
use boole_devnet_archive::input::InputRow;
use boole_devnet_audit::{audit, ExpectedHeads, Inputs, Node};
use boole_gate_supervisor::rebuild::History;
use serde_json::{json, Value};
use std::collections::BTreeMap;
struct Source {
    rows: Vec<InputRow>,
    changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl History for Source {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        let v = self.row(h)?.verify()?;
        Ok((v.block, v.logs))
    }
}
impl Inputs for Source {
    fn tip(&self) -> u64 {
        self.rows.len() as u64 - 1
    }
    fn row(&mut self, h: u64) -> Result<InputRow> {
        self.rows
            .get(h as usize)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("absent"))
    }
    fn check(&self) -> Result<()> {
        if self.changed.load(std::sync::atomic::Ordering::Relaxed) {
            bail!("source file changed")
        };
        Ok(())
    }
    fn label(&self) -> &'static str {
        "journal"
    }
}
struct View {
    headers: BTreeMap<u64, ParentHeader>,
    txs: BTreeMap<u64, Vec<String>>,
    changed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    mutation_path: Option<std::path::PathBuf>,
}
impl Node for View {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.headers.last_key_value().unwrap().1.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        Ok(self.headers[&h].clone())
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        if let Some(v) = &self.changed {
            v.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(path) = &self.mutation_path {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .append(true)
                .open(path)?
                .write_all(b" ")?;
        }
        Ok(self.txs[&h].clone())
    }
    fn receipt_statuses(&mut self, h: u64) -> Result<Vec<bool>> {
        Ok(vec![true; self.txs[&h].len()])
    }
    fn depositor_balance(&mut self, _: u64) -> Result<u128> {
        Ok(0)
    }
}
fn setup() -> Result<(Config, Source, View, ExpectedHeads)> {
    let v: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let rows: Vec<InputRow> = serde_json::from_value(v["rows"].clone())?;
    let c = Config::devnet();
    let changed = Default::default();
    let source = Source { rows, changed };
    let v: Vec<Value> = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))?;
    let mut parent: ParentHeader = serde_json::from_value(v[0]["parent"].clone())?;
    let mut state = State::new(&c, parent.gas_limit)?;
    let mut headers = BTreeMap::from([(0, parent.clone())]);
    let mut txs = BTreeMap::new();
    for h in 1..=source.tip() {
        let row = source.rows[h as usize].verify()?;
        let (d, next) = boole_derivation::attributes::derive_with_m2(
            &state, &c, &row.block, &row.logs, &parent,
        )?;
        let system = d.attributes.transactions;
        parent.parent_hash = Some(parent.hash.clone());
        parent.number = h;
        parent.hash = hex_string(&[h as u8; 32]);
        parent.timestamp = u64::from_str_radix(&d.attributes.timestamp[2..], 16)?;
        parent.gas_limit = u64::from_str_radix(&d.attributes.gas_limit[2..], 16)?;
        parent.l1_info = Some(system[0].clone());
        parent.m2_root_deposit = Some(system[1].clone());
        headers.insert(h, parent.clone());
        txs.insert(h, system);
        state = next;
    }
    let expected = ExpectedHeads {
        l1_number: source.tip(),
        l1_hash: source.rows.last().unwrap().block_hash.clone(),
        l2: Some((source.tip(), parent.hash)),
    };
    Ok((
        c,
        source,
        View {
            headers,
            txs,
            changed: None,
            mutation_path: None,
        },
        expected,
    ))
}
#[test]
fn t81_changed_prefix_names_first_height() -> Result<()> {
    let (c, mut s, mut n, e) = setup()?;
    n.txs.get_mut(&1).unwrap()[1].replace_range(4..6, "00");
    let r = audit(&mut s, &mut n, &c, None, None, &e);
    assert_eq!(r["first_mismatch"], json!({"height":"1","class":"prefix"}));
    Ok(())
}
#[test]
fn t81_stale_source_refuses_the_independent_head() -> Result<()> {
    let (c, mut s, mut n, mut e) = setup()?;
    e.l1_number += 1;
    e.l1_hash = hex_string(&[88; 32]);
    let r = audit(&mut s, &mut n, &c, None, None, &e);
    assert_eq!(
        r["first_mismatch"],
        json!({"height":e.l1_number.to_string(),"class":"input"})
    );
    Ok(())
}
#[test]
fn t81_mutated_header_is_an_input_mismatch() -> Result<()> {
    let (c, mut s, mut n, e) = setup()?;
    s.rows[1].header_rlp.replace_range(6..8, "ff");
    let r = audit(&mut s, &mut n, &c, None, None, &e);
    assert_eq!(r["first_mismatch"], json!({"height":"1","class":"input"}));
    Ok(())
}
#[test]
fn t81_source_written_during_audit_is_never_reported_ok() -> Result<()> {
    let (c, mut s, mut n, e) = setup()?;
    n.changed = Some(s.changed.clone());
    let r = audit(&mut s, &mut n, &c, None, None, &e);
    assert_eq!(r["first_mismatch"], json!({"height":"1","class":"input"}));
    Ok(())
}
#[test]
fn t81_receipt_golden_and_valid_ancestor_head() -> Result<()> {
    let (c, mut s, mut n, mut e) = setup()?;
    e.l1_number = 1;
    e.l1_hash = s.rows[1].block_hash.clone();
    let r = audit(&mut s, &mut n, &c, None, None, &e);
    let golden: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/audit-receipt-v1.json"
    ))?;
    assert_eq!(r, golden);
    assert!(r["first_mismatch"].is_null());
    assert_eq!(r["schema"], "boole.devnet.audit.v1");
    assert_eq!(r["l1_head"]["number"], "2");
    assert_eq!(r["checks"]["prefix"], "ok");
    assert_eq!(r["labels"]["environment"], "pending");
    assert_eq!(r["k4"]["status"], "pending");
    assert_eq!(r["rewards"]["minted"], "0");
    assert_eq!(r["unsigned"], true);
    Ok(())
}

#[test]
fn t81_l2_receipts_are_bound_to_the_recorded_op_trie_root() -> Result<()> {
    let tape: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m3/miner-client-v1.3.json"
    ))?;
    for row in tape["rows"].as_array().unwrap() {
        let receipts = row["observation"]["receipts"].as_array().unwrap();
        let bytes = receipts
            .iter()
            .map(boole_devnet_audit::rpc::receipt_bytes)
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(
            hex_string(&boole_devnet_archive::input::receipts_root(&bytes)),
            row["observation"]["receipts_root"]
        );
    }
    Ok(())
}

#[test]
fn t81_real_read_only_journal_never_repairs_or_writes_sources() -> Result<()> {
    use boole_devnet_archive::{
        durability::PrivateTempDir,
        journal::{InputJournal, NetworkIdentity, ReadOnlyJournal},
    };
    let (c, s, mut node, expected) = setup()?;
    let root = PrivateTempDir::new("audit-read-only-source")?;
    let network = NetworkIdentity::new(
        &c,
        None,
        node.headers[&0].hash.clone(),
        node.headers[&0].gas_limit,
    )?;
    let mut writer = InputJournal::open(
        root.path(),
        network.clone(),
        || Ok(s.rows[0].clone()),
        false,
    )?;
    for row in &s.rows[1..] {
        writer.append(row)?;
    }
    drop(writer);
    let path = root.path().join("input.ndjson");
    let before = std::fs::read(&path)?;
    let files_before: Vec<_> = std::fs::read_dir(root.path())?
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut source =
        boole_devnet_audit::JournalInputs(ReadOnlyJournal::open(root.path(), &network)?);
    let r = audit(&mut source, &mut node, &c, None, None, &expected);
    assert!(r["first_mismatch"].is_null(), "{r}");
    assert_eq!(std::fs::read(&path)?, before);
    let files_after: Vec<_> = std::fs::read_dir(root.path())?
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        files_before, files_after,
        "audit created a writer lock/cache or repaired a source"
    );
    node.mutation_path = Some(path.clone());
    let r = audit(&mut source, &mut node, &c, None, None, &expected);
    assert_eq!(r["first_mismatch"], json!({"height":"1","class":"input"}));
    let expected_bytes = [before, b" ".to_vec()].concat();
    assert_eq!(std::fs::read(&path)?, expected_bytes);
    assert!(ReadOnlyJournal::open(root.path(), &network).is_err());
    assert_eq!(
        std::fs::read(path)?,
        expected_bytes,
        "torn source was repaired"
    );
    Ok(())
}

#[test]
fn t81_read_only_chain_errors_keep_the_first_bad_journal_height() -> Result<()> {
    use boole_devnet_archive::{
        durability::PrivateTempDir,
        error::Error,
        journal::{InputJournal, NetworkIdentity, ReadOnlyJournal},
    };
    let (c, source, node, _) = setup()?;
    let root = PrivateTempDir::new("audit-chain-height")?;
    let network = NetworkIdentity::new(&c, None, node.headers[&0].hash.clone(), 30_000_000)?;
    let journal = InputJournal::open(
        root.path(),
        network.clone(),
        || Ok(source.rows[0].clone()),
        false,
    )?;
    drop(journal);
    let mut wrong_parent = source.rows[1].clone();
    let raw = boole_derivation::attributes::hex_bytes(&wrong_parent.header_rlp, "header")?;
    let fields = boole_derivation::rlp::decode(&raw)
        .map_err(anyhow::Error::msg)?
        .children
        .unwrap();
    let mut encoded: Vec<Vec<u8>> = fields
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    encoded[0] = boole_derivation::rlp::bytes(&[99; 32]);
    let raw = boole_derivation::rlp::list(&encoded);
    wrong_parent.header_rlp = hex_string(&raw);
    wrong_parent.block_hash = hex_string(&boole_derivation::user_tx::keccak(&raw));
    wrong_parent.verify()?;
    for bad in [source.rows[2].clone(), wrong_parent] {
        let mut bytes = serde_json::to_vec(&source.rows[0])?;
        bytes.push(b'\n');
        bytes.extend(serde_json::to_vec(&bad)?);
        bytes.push(b'\n');
        std::fs::write(root.path().join("input.ndjson"), &bytes)?;
        let error = ReadOnlyJournal::open(root.path(), &network)
            .err()
            .expect("invalid chain accepted");
        assert!(
            matches!(
                error.downcast_ref::<Error>(),
                Some(Error::Integrity {
                    height: 1,
                    class: "input",
                    ..
                })
            ),
            "first bad journal height lost: {error}"
        );
        assert_eq!(std::fs::read(root.path().join("input.ndjson"))?, bytes);
    }
    Ok(())
}

fn wrong_attribute(field: &str) -> Result<()> {
    let (c, mut source, mut node, expected) = setup()?;
    assert!(audit(&mut source, &mut node, &c, None, None, &expected)["first_mismatch"].is_null());
    let header = node.headers.get_mut(&1).unwrap();
    if field == "timestamp" {
        header.timestamp += 1;
    } else {
        header.gas_limit += 1;
    }
    let r = audit(&mut source, &mut node, &c, None, None, &expected);
    assert_eq!(
        r["first_mismatch"],
        json!({"height":"1","class":"prefix"}),
        "unchecked {field}"
    );
    Ok(())
}
#[test]
fn l1_pure_audit_rejects_timestamp_disagreeing_with_derivation() -> Result<()> {
    wrong_attribute("timestamp")
}
#[test]
fn l1_pure_audit_rejects_gas_limit_disagreeing_with_derivation() -> Result<()> {
    wrong_attribute("gasLimit")
}
#[test]
fn l3_m2_receipt_never_claims_that_m3_only_checks_ran() -> Result<()> {
    let (c, mut source, mut node, expected) = setup()?;
    let r = audit(&mut source, &mut node, &c, None, None, &expected);
    assert!(r["first_mismatch"].is_null(), "{r}");
    for class in ["verdict", "admission", "forks"] {
        assert_eq!(r["checks"][class], "not-applicable", "{class}");
    }
    for class in ["input", "fold", "prefix", "ledger"] {
        assert_eq!(r["checks"][class], "ok", "{class}");
    }
    Ok(())
}

#[test]
fn b2_local_resource_stop_is_not_an_audit_mismatch() -> Result<()> {
    struct Limited(View);
    impl Node for Limited {
        fn head(&mut self) -> Result<ParentHeader> {
            self.0.head()
        }
        fn header(&mut self, h: u64) -> Result<ParentHeader> {
            self.0.header(h)
        }
        fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
            self.0.transactions(h)
        }
        fn receipt_statuses(&mut self, _: u64) -> Result<Vec<bool>> {
            Err(boole_devnet_audit::resource::ResourceStop {
                resource: "rpc-bytes",
                limit: 16 * 1024 * 1024,
            }
            .into())
        }
        fn depositor_balance(&mut self, h: u64) -> Result<u128> {
            self.0.depositor_balance(h)
        }
    }
    let (c, mut s, n, e) = setup()?;
    let mut n = Limited(n);
    let receipt = audit(&mut s, &mut n, &c, None, None, &e);
    assert!(
        receipt["first_mismatch"].is_null(),
        "local bound accused the chain: {receipt}"
    );
    assert_eq!(receipt["resource_stop"]["height"], "1");
    assert_eq!(receipt["resource_stop"]["resource"], "rpc-bytes");
    assert_eq!(receipt["checks"]["ledger"], "resource-stop");
    Ok(())
}
