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
    fn post_durable(
        &mut self,
        kind: u8,
        body: &[u8],
        _persist: &mut dyn FnMut(&boole_devnet_mcp::posting::SignedPosting) -> Result<()>,
    ) -> Result<String> {
        // Read-only test adapter: retain its panic/error on any attempted post.
        self.post(kind, body)
    }

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

#[derive(Clone)]
struct M3View {
    headers: std::collections::BTreeMap<u64, boole_derivation::ParentHeader>,
    blocks: std::collections::BTreeMap<u64, boole_devnet_mcp::client::CanonicalBlock>,
    deny_old: bool,
}
impl boole_devnet_mcp::client::L2Source for M3View {
    fn header(&mut self, h: u64) -> Result<Option<boole_derivation::ParentHeader>> {
        assert!(!self.deny_old || h >= 2, "historical L2 header {h}");
        Ok(self.headers.get(&h).cloned())
    }

    fn root(&mut self, h: u64, origin: &str) -> Result<Option<[u8; 32]>> {
        Ok(Some(boole_derivation::system::read_m2_root(
            self.headers[&h].m2_root_deposit.as_deref().unwrap(),
            h,
            origin,
        )?))
    }
    fn block(&mut self, h: u64) -> Result<Option<boole_devnet_mcp::client::CanonicalBlock>> {
        assert!(!self.deny_old || h >= 2, "historical L2 read {h}");
        Ok(self.blocks.get(&h).cloned())
    }
}
#[test]
fn t88_m3_miner_pins_before_state_and_restarts_from_bindings() -> Result<()> {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        m2::{Config, State},
        m3::{ForkTable, M3Config},
        ParentHeader,
    };
    use boole_devnet_mcp::client::CanonicalBlock;
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    let rows: Vec<InputRow> = serde_json::from_value(v["rows"].clone())?;
    let d = PrivateTempDir::new("m4c2-miner")?;
    let mut c = Config::devnet();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    let forks = ForkTable::genesis(&c)?;
    let old: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m1/s1b-recorded-main.json"
    ))?;
    let mut parent: ParentHeader = serde_json::from_value(old[0]["parent"].clone())?;
    let id = NetworkIdentity::new(&c, Some(&forks), parent.hash.clone(), 30_000_000)?;
    let mut journal = InputJournal::open(d.path(), id.clone(), || Ok(rows[0].clone()), false)?;
    for row in rows.iter().skip(1) {
        journal.append(row)?;
    }
    drop(journal);
    let fixed = |s: &str| -> [u8; 32] { hex_bytes(s, "hash").unwrap().try_into().unwrap() };
    let mut view = M3View {
        headers: [(0, parent.clone())].into(),
        blocks: [(
            0,
            CanonicalBlock {
                number: 0,
                hash: fixed(&parent.hash),
                parent_hash: [0; 32],
                transactions: vec![],
            },
        )]
        .into(),
        deny_old: false,
    };
    let mut state = State::new(&c, 30_000_000)?;
    for h in 1..=2 {
        let input = rows[h as usize].verify()?;
        let (derived, next) = boole_derivation::attributes::derive_with_m3(
            &state,
            &c,
            &input.block,
            &input.logs,
            &parent,
            &[],
            &forks,
        )?;
        parent.parent_hash = Some(parent.hash.clone());
        parent.hash = hex_string(&[h as u8; 32]);
        parent.number = h;
        parent.timestamp = u64::from_str_radix(&derived.attributes.timestamp[2..], 16)?;
        parent.l1_info = derived.attributes.transactions.first().cloned();
        parent.m2_root_deposit = derived.attributes.transactions.get(1).cloned();
        view.headers.insert(h, parent.clone());
        view.blocks.insert(
            h,
            CanonicalBlock {
                number: h,
                hash: fixed(&parent.hash),
                parent_hash: fixed(parent.parent_hash.as_ref().unwrap()),
                transactions: derived.attributes.transactions,
            },
        );
        state = next;
    }
    for restart in [false, true] {
        let source = JournalSource::open(Tip { rows: rows.clone() }, d.path(), id.clone(), false)?;
        let mut options = ClientOptions::new(30_000_000);
        options.forks = Some(forks.clone());
        let mut node = view.clone();
        node.deny_old = restart;
        options.l2 = Some(Box::new(node));
        let mut client = Client::new(
            source,
            NoPost,
            c.clone(),
            [7; 20],
            d.path().to_owned(),
            options,
        )?;
        client.sync()?;
        assert_eq!(client.m2_state().root, state.root);
        let pins =
            boole_devnet_archive::pins::ReadOnlyPins::open(d.path(), &id, &rows[0].block_hash)?
                .expect("miner state published without default durable pins");
        assert_eq!(pins.binding(2)?.root, state.root);
        assert!(d.path().join("retention-tip.json").exists());
    }
    Ok(())
}
