//! T77: miner folds its authenticated journal while its transport panics below tip.
use anyhow::{bail, Result};
use boole_derivation::{InboxLog, L1Block};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::InputRow,
    journal::{InputJournal, NetworkIdentity},
};
use boole_devnet_driver::l1::{JournalSource, L1Source};
use boole_devnet_mcp::client::{Client, ClientOptions, Poster};
#[derive(Clone)]
struct Tip {
    rows: Vec<InputRow>,
}
impl L1Source for Tip {
    fn finalized(&mut self) -> Result<u64> {
        Ok(2)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        assert!(h >= 2, "historical RPC read {h}");
        self.rows[h as usize].block()
    }
    fn inbox_logs(&mut self, _: &str) -> Result<Vec<InboxLog>> {
        panic!("historical log RPC")
    }
    fn input_row(&mut self, h: u64) -> Result<InputRow> {
        assert!(h >= 2, "historical input RPC read {h}");
        Ok(self.rows[h as usize].clone())
    }
    fn block_by_hash(&mut self, _: &str) -> Result<L1Block> {
        panic!("historical hash RPC")
    }
}
struct NoPost;
impl Poster for NoPost {
    fn head(&mut self) -> Result<u64> {
        Ok(2)
    }
    fn post(&mut self, _: u8, _: &[u8]) -> Result<String> {
        bail!("unexpected post")
    }
    fn included(&mut self, _: &str) -> Result<Option<u64>> {
        bail!("unexpected receipt RPC")
    }
}
#[test]
fn t77_miner_restarts_from_own_journal_without_a_historical_rpc() -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let rows: Vec<InputRow> = serde_json::from_value(value["rows"].clone())?;
    let d = PrivateTempDir::new("m4-miner-journal")?;
    let c = boole_derivation::m2::Config::devnet();
    let identity = NetworkIdentity::new(
        &c,
        None,
        boole_derivation::attributes::hex_string(&[1; 32]),
        30_000_000,
    )?;
    let mut journal =
        InputJournal::open(d.path(), identity.clone(), || Ok(rows[0].clone()), false)?;
    for row in rows.iter().skip(1) {
        journal.append(row)?;
    }
    drop(journal);
    let raw = Tip { rows };
    let source = JournalSource::open(raw.clone(), d.path(), identity.clone(), false)?;
    let mut client = Client::new(
        source,
        NoPost,
        c.clone(),
        [7; 20],
        d.path().to_owned(),
        ClientOptions::new(30_000_000),
    )?;
    client.sync()?;
    let root = client.m2_state().root;
    assert_eq!(client.m2_state().height(), 2);
    drop(client);
    let source = JournalSource::open(raw, d.path(), identity, false)?;
    let mut client = Client::new(
        source,
        NoPost,
        c,
        [7; 20],
        d.path().to_owned(),
        ClientOptions::new(30_000_000),
    )?;
    client.sync()?;
    assert_eq!(client.m2_state().height(), 2);
    assert_eq!(client.m2_state().root, root);
    Ok(())
}

#[test]
fn t78_fresh_miner_fills_own_verified_journal_then_restarts_tip_only() -> Result<()> {
    use boole_devnet_archive::{
        archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore, Selection},
        client::ArchiveClient,
        http::ArchiveServer,
    };
    use std::sync::{Arc, Mutex};
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let rows: Vec<InputRow> = serde_json::from_value(value["rows"].clone())?;
    let d = PrivateTempDir::new("m4-miner-archive")?;
    let c = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(
        &c,
        None,
        boole_derivation::attributes::hex_string(&[1; 32]),
        30_000_000,
    )?;
    let x = std::sync::Arc::new(Mutex::new(ArchiveStore::open(
        &d.path().join("x"),
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: network.clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: rows[0].block_hash.clone(),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?));
    for (h, row) in rows.iter().enumerate() {
        x.lock().unwrap().ingest(
            row,
            2,
            &Selection {
                l2_number: h as u64,
                l2_hash: boole_derivation::attributes::hex_string(&[1; 32]),
                root: boole_derivation::attributes::hex_string(&[2; 32]),
                system: vec![],
                inputs: "journal".into(),
                verdicts: "computed".into(),
            },
        )?;
    }
    let server = ArchiveServer::start(Arc::clone(&x), "127.0.0.1:0".parse()?)?;
    let path = d.path().join("miner");
    let raw = Tip { rows };
    let source = JournalSource::open_archive(
        raw.clone(),
        &path,
        network.clone(),
        false,
        ArchiveClient::http(vec![server.url()])?,
    )?;
    let mut client = Client::new(
        source,
        NoPost,
        c.clone(),
        [7; 20],
        path.clone(),
        ClientOptions::new(30_000_000),
    )?;
    client.sync()?;
    let root = client.m2_state().root;
    assert_eq!(client.status()["inputs"], "archive-verified");
    assert_eq!(client.status()["input_ranges"][0]["from"], 1);
    drop(client);
    let source = JournalSource::open_archive(
        raw,
        &path,
        network,
        false,
        ArchiveClient::http(vec![server.url()])?,
    )?;
    let mut client = Client::new(
        source,
        NoPost,
        c,
        [7; 20],
        path,
        ClientOptions::new(30_000_000),
    )?;
    client.sync()?;
    assert_eq!(client.status()["inputs"], "journal");
    assert_eq!(client.m2_state().root, root);
    Ok(())
}
