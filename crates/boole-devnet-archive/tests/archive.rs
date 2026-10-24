use anyhow::Result;
use boole_devnet_archive::{
    archive::{ArchiveIdentity, ArchiveProfile},
    journal::NetworkIdentity,
};
fn profile() -> ArchiveProfile {
    let c = boole_derivation::m2::Config::devnet();
    ArchiveProfile {
        identity: ArchiveIdentity {
            network: NetworkIdentity::new(&c, None, format!("0x{}", "01".repeat(32)), 30_000_000)
                .unwrap(),
            l1_anchor_number: 0,
            l1_anchor_hash: format!("0x{}", "02".repeat(32)),
        },
        bind: "127.0.0.1:0".parse().unwrap(),
        prune_before: None,
    }
}
#[test]
fn t70_archive_profile_refuses_public_bind_and_any_input_prune() -> Result<()> {
    let mut p = profile();
    p.prune_before = Some(0);
    assert!(p.validate().unwrap_err().to_string().contains("S20"));
    p.prune_before = None;
    p.bind = "0.0.0.0:9090".parse()?;
    assert!(p.validate().unwrap_err().to_string().contains("S21"));
    p.bind = "[::1]:0".parse()?;
    p.validate()?;
    Ok(())
}
fn rows() -> Vec<boole_devnet_archive::input::InputRow> {
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))
    .unwrap();
    serde_json::from_value(v["rows"].clone()).unwrap()
}
fn selected(h: u64) -> boole_devnet_archive::archive::Selection {
    boole_devnet_archive::archive::Selection {
        l2_number: h,
        l2_hash: format!("0x{}", "03".repeat(32)),
        root: format!("0x{}", "04".repeat(32)),
        system: vec![],
        inputs: "journal".into(),
        verdicts: "computed".into(),
    }
}
#[test]
fn t75_ingest_refuses_an_unfinalized_height() -> Result<()> {
    let d = boole_devnet_archive::durability::PrivateTempDir::new("m4-archive-ingest")?;
    let r = rows();
    let mut p = profile();
    p.identity.l1_anchor_hash = r[0].block_hash.clone();
    let mut x = boole_devnet_archive::archive::ArchiveStore::open(d.path(), p, false)?;
    x.ingest(&r[0], 0, &selected(0))?;
    assert!(x
        .ingest(&r[1], 0, &selected(1))
        .unwrap_err()
        .to_string()
        .contains("unfinalized"));
    Ok(())
}
#[test]
fn t70_archive_manifest_index_identity_lock_and_reopen() -> Result<()> {
    use boole_devnet_archive::{archive::ArchiveStore, durability::PrivateTempDir};
    let d = PrivateTempDir::new("m4-archive-identity")?;
    let r = rows();
    let mut p = profile();
    p.identity.l1_anchor_hash = r[0].block_hash.clone();
    let mut x = ArchiveStore::open(d.path(), p.clone(), false)?;
    assert!(
        ArchiveStore::open(d.path(), p.clone(), false).is_err(),
        "second writer accepted"
    );
    x.ingest(&r[0], 0, &selected(0))?;
    drop(x);
    let mut wrong = p.clone();
    wrong.identity.network.l2_chain_id += 1;
    assert!(ArchiveStore::open(d.path(), wrong, false).is_err());
    let mut x = ArchiveStore::open(d.path(), p.clone(), false)?;
    x.ingest(&r[1], 1, &selected(1))?;
    let before = x.info();
    drop(x);
    let x = ArchiveStore::open(d.path(), p.clone(), false)?;
    assert_eq!(x.info(), before);
    assert_eq!(x.index(0, 1024)?.len(), 2);
    drop(x);
    std::fs::remove_file(d.path().join("index.ndjson"))?;
    assert!(ArchiveStore::open(d.path(), p, false)
        .err()
        .unwrap()
        .to_string()
        .contains("S19"));
    Ok(())
}
#[test]
fn t74_api_refuses_write_methods_and_oversized_ranges() -> Result<()> {
    use boole_devnet_archive::{
        archive::ArchiveStore, durability::PrivateTempDir, http::ArchiveServer,
    };
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::{Arc, Mutex},
    };
    let d = PrivateTempDir::new("m4-api")?;
    let x = Arc::new(Mutex::new(ArchiveStore::open(d.path(), profile(), false)?));
    let server = ArchiveServer::start(x, "127.0.0.1:0".parse()?)?;
    let request = |method: &str, path: &str| -> Result<Vec<u8>> {
        let mut s = TcpStream::connect(server.addr())?;
        write!(s,"{method} /boole/archive/v1/{path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut r = vec![];
        s.read_to_end(&mut r)?;
        Ok(r)
    };
    assert!(request("GET", "info")?.starts_with(b"HTTP/1.1 200"));
    assert!(
        request("POST", "info")?.starts_with(b"HTTP/1.1 405"),
        "API accepted a write method"
    );
    assert!(
        request("GET", "index/0?limit=1025")?.starts_with(b"HTTP/1.1 400"),
        "range exceeded SEGMENT"
    );
    let missing = request("GET", &format!("l1/header/{}", "00".repeat(32)))?;
    assert!(missing.starts_with(b"HTTP/1.1 404"));
    assert!(!missing.ends_with(b"\r\n\r\n"));
    assert!(request("GET", "segment/0")?.starts_with(b"HTTP/1.1 404"));
    Ok(())
}
struct BytesFetch(Vec<u8>);
impl boole_devnet_archive::client::Fetch for BytesFetch {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        Ok(self.0.clone())
    }
}
#[test]
fn t71_archive_header_mutation_is_discarded_as_availability() -> Result<()> {
    let r = rows();
    let mut raw = boole_derivation::attributes::hex_bytes(&r[1].header_rlp, "fixture")?;
    *raw.last_mut().unwrap() ^= 1;
    let mut client = boole_devnet_archive::client::ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(BytesFetch(raw)),
    )?;
    let e = client
        .header(&r[1].block_hash, 1)
        .expect_err("mutated archive header was adopted");
    assert!(matches!(
        e.downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Availability { .. })
    ));
    assert_eq!(client.discarded, 3);
    Ok(())
}
struct RowFetch {
    row: boole_devnet_archive::input::InputRow,
}
impl boole_devnet_archive::client::Fetch for RowFetch {
    fn get(&mut self, source: &str, route: &str, _: usize) -> Result<Vec<u8>> {
        if route.starts_with("l1/header/") {
            return Ok(boole_derivation::attributes::hex_bytes(
                &self.row.header_rlp,
                "fixture",
            )?);
        }
        if route.starts_with("l1/txs/") {
            return Ok(serde_json::to_vec(self.row.transactions.as_ref().unwrap())?);
        }
        anyhow::ensure!(
            route.starts_with("l1/receipts/"),
            "unexpected fixture route"
        );
        let mut receipts = self.row.receipts.clone().unwrap();
        if source.ends_with(":1") {
            receipts.remove(0);
        }
        Ok(serde_json::to_vec(&receipts)?)
    }
}
#[test]
fn t72_bad_receipts_switch_source_before_they_can_reach_a_fold() -> Result<()> {
    let r = rows();
    let row = r
        .iter()
        .find(|r| !r.verify().unwrap().logs.is_empty())
        .unwrap()
        .clone();
    let h = row.block()?.number;
    let expected = serde_json::to_value(row.verify()?.logs)?;
    let mut client = boole_devnet_archive::client::ArchiveClient::new(
        vec!["http://127.0.0.1:1".into(), "http://127.0.0.1:2".into()],
        Box::new(RowFetch { row: row.clone() }),
    )?;
    let chosen = client.row(&row.block_hash, h)?;
    assert_eq!(serde_json::to_value(chosen.verify()?.logs)?, expected);
    assert_eq!(client.failures[0], 1);
    assert_eq!(client.failures[1], 0);
    Ok(())
}

#[test]
fn t74_get_cannot_smuggle_a_body_or_exceed_request_cap() -> Result<()> {
    use boole_devnet_archive::{
        archive::{ArchiveStore, REQ_MAX},
        durability::PrivateTempDir,
        http::ArchiveServer,
    };
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::{Arc, Mutex},
    };
    let d = PrivateTempDir::new("m4-api-body")?;
    let server = ArchiveServer::start(
        Arc::new(Mutex::new(ArchiveStore::open(d.path(), profile(), false)?)),
        "127.0.0.1:0".parse()?,
    )?;
    for headers in [
        format!("Content-Length: {}\r\n", REQ_MAX + 1),
        "Transfer-Encoding: chunked\r\n".into(),
        "Content-Length: 0\r\nContent-Length: 0\r\n".into(),
        "X-Pad: ".to_owned() + &"a".repeat(REQ_MAX) + "\r\n",
    ] {
        let mut stream = TcpStream::connect(server.addr())?;
        let request =
            format!("GET /boole/archive/v1/info HTTP/1.1\r\nHost: localhost\r\n{headers}\r\n");
        let _ = stream.write_all(request.as_bytes());
        let mut body = vec![];
        let _ = stream.read_to_end(&mut body);
        assert!(
            body.starts_with(b"HTTP/1.1 4"),
            "request framing was accepted: {}",
            String::from_utf8_lossy(&body)
        );
    }
    Ok(())
}
#[test]
fn t70_all_manifest_identity_fields_are_bound_before_serving() -> Result<()> {
    use boole_devnet_archive::{
        archive::{ArchiveIdentity, ArchiveStore},
        durability::PrivateTempDir,
    };
    let d = PrivateTempDir::new("m4-all-identities")?;
    let p = profile();
    drop(ArchiveStore::open(d.path(), p.clone(), false)?);
    for field in [
        "l1_chain_id",
        "l2_chain_id",
        "l2_gas_limit",
        "config_digest",
        "fork_genesis",
        "l2_genesis_hash",
    ] {
        let mut v = serde_json::to_value(&p.identity)?;
        v["network"][field] = if let Some(n) = v["network"][field].as_u64() {
            serde_json::json!(n + 1)
        } else {
            serde_json::json!(format!("0x{}", "09".repeat(32)))
        };
        let mut wrong = p.clone();
        wrong.identity = serde_json::from_value::<ArchiveIdentity>(v)?;
        assert!(
            ArchiveStore::open(d.path(), wrong, false)
                .err()
                .unwrap()
                .to_string()
                .contains("S18"),
            "unbound {field}"
        );
    }
    let mut wrong = p.clone();
    wrong.identity.l1_anchor_hash = format!("0x{}", "09".repeat(32));
    assert!(ArchiveStore::open(d.path(), wrong, false).is_err());
    let mut wrong = p.clone();
    wrong.identity.l1_anchor_number = 1;
    assert!(ArchiveStore::open(d.path(), wrong, false).is_err());
    drop(ArchiveStore::open(d.path(), p, false)?);
    Ok(())
}
struct BloomSkip {
    row: boole_devnet_archive::input::InputRow,
}
impl boole_devnet_archive::client::Fetch for BloomSkip {
    fn get(&mut self, _: &str, route: &str, _: usize) -> Result<Vec<u8>> {
        assert!(
            route.starts_with("l1/header/"),
            "bloom-negative source requested receipts"
        );
        Ok(boole_derivation::attributes::hex_bytes(
            &self.row.header_rlp,
            "fixture",
        )?)
    }
}
#[test]
fn t72_bloom_negative_header_needs_no_receipt_request() -> Result<()> {
    let row = rows()
        .into_iter()
        .find(|r| {
            !boole_devnet_archive::input::header_needs_receipts(
                &boole_derivation::attributes::hex_bytes(&r.header_rlp, "fixture").unwrap(),
            )
            .unwrap()
        })
        .unwrap();
    let mut client = boole_devnet_archive::client::ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(BloomSkip { row: row.clone() }),
    )?;
    let verified = client.row(&row.block_hash, row.block()?.number)?.verify()?;
    assert!(verified.logs.is_empty());
    assert_eq!(client.discarded, 0);
    Ok(())
}

#[test]
fn t71_wrong_field_set_height_and_a_chain_not_linked_to_own_anchor_are_unavailable() -> Result<()> {
    use boole_derivation::{attributes::hex_string, user_tx::keccak};
    use boole_devnet_archive::{client::ArchiveClient, durability::PrivateTempDir};
    let raw = vec![0xc0];
    let key = hex_string(&keccak(&raw));
    let mut client =
        ArchiveClient::new(vec!["http://127.0.0.1:1".into()], Box::new(BytesFetch(raw)))?;
    assert!(matches!(
        client
            .header(&key, 0)
            .unwrap_err()
            .downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Availability { .. })
    ));
    let row = rows().pop().unwrap();
    let raw = boole_derivation::attributes::hex_bytes(&row.header_rlp, "fixture")?;
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(BytesFetch(raw.clone())),
    )?;
    assert!(matches!(
        client
            .header(&row.block_hash, row.block()?.number + 1)
            .unwrap_err()
            .downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Availability { .. })
    ));
    let d = PrivateTempDir::new("m4-wrong-parent")?;
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(BytesFetch(raw.clone())),
    )?;
    let error = client
        .prepare(
            row.block()?.number,
            &row.block_hash,
            Some((row.block()?.number - 1, hex_string(&[255; 32]))),
            d.path(),
            Some(&raw),
        )
        .err()
        .unwrap();
    assert!(matches!(
        error.downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Integrity {
            class: "l1-anchor",
            ..
        })
    ));
    Ok(())
}
#[path = "support/http_fixes.rs"]
mod fixes;

#[test]
fn c6_archive_gap_cannot_prove_own_transaction_absent() -> Result<()> {
    use boole_devnet_archive::{durability::PrivateTempDir, error::Error, journal::InputJournal};
    let d = PrivateTempDir::new("archive-post-proof")?;
    let mut r = rows();
    for row in &mut r {
        row.transactions = None;
    }
    let mut j = InputJournal::open(
        d.path(),
        profile().identity.network,
        || Ok(r[0].clone()),
        false,
    )?;
    j.append(&r[1])?;
    let e = j
        .transaction_receipt_since(&format!("0x{}", "77".repeat(32)), 0)
        .expect_err("archive gap was interpreted as absence and permits repost");
    assert!(matches!(
        e.downcast_ref::<Error>(),
        Some(Error::Availability {
            class: "archive-tx-proof",
            ..
        })
    ));
    Ok(())
}

#[test]
fn r4_verified_republication_counts_and_logs_quarantine_by_class() -> Result<()> {
    let dir = boole_devnet_archive::durability::PrivateTempDir::new("archive-quarantine-count")?;
    let row = &rows()[0];
    let mut p = profile();
    p.identity.l1_anchor_hash = row.block_hash.clone();
    let mut store = boole_devnet_archive::archive::ArchiveStore::open(dir.path(), p, false)?;
    let selection = selected(0);
    store.ingest(row, 0, &selection)?;
    for expected in 1..=2 {
        std::fs::write(
            dir.path().join("header").join(&row.block_hash[2..]),
            b"corrupt public header",
        )?;
        store.ingest(row, 0, &selection)?;
        assert_eq!(store.info()["quarantines"]["header"], expected);
    }
    assert_eq!(
        std::fs::read_dir(dir.path().join("header/corrupt"))?.count(),
        2
    );
    Ok(())
}

#[test]
fn m4b2_transaction_class_serves_header_authenticated_ordered_bytes() -> Result<()> {
    use boole_devnet_archive::{archive::ArchiveStore, http::ArchiveServer};
    use std::{
        io::{Read, Write},
        net::TcpStream,
        sync::{Arc, Mutex},
    };
    let dir = boole_devnet_archive::durability::PrivateTempDir::new("archive-transactions")?;
    let rows = rows();
    let mut p = profile();
    p.identity.l1_anchor_hash = rows[0].block_hash.clone();
    rows[1].verify()?;
    let mut store = ArchiveStore::open(dir.path(), p, false)?;
    store.ingest(&rows[0], 1, &selected(0))?;
    store.ingest(&rows[1], 1, &selected(1))?;
    let server = ArchiveServer::start(Arc::new(Mutex::new(store)), "127.0.0.1:0".parse()?)?;
    let mut stream = TcpStream::connect(server.addr())?;
    write!(
        stream,
        "GET /boole/archive/v1/l1/txs/{} HTTP/1.1\r\n\r\n",
        &rows[1].block_hash[2..]
    )?;
    let mut response = vec![];
    stream.read_to_end(&mut response)?;
    assert!(
        response.starts_with(b"HTTP/1.1 200"),
        "transaction class unavailable: {}",
        String::from_utf8_lossy(&response)
    );
    let at = response.windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
    let actual: Vec<String> = serde_json::from_slice(&response[at..])?;
    assert_eq!(Some(actual), rows[1].transactions);
    Ok(())
}

struct TransactionFetch {
    row: boole_devnet_archive::input::InputRow,
    mode: u8,
}
impl boole_devnet_archive::client::Fetch for TransactionFetch {
    fn get(&mut self, _: &str, route: &str, _: usize) -> Result<Vec<u8>> {
        if route.starts_with("l1/receipts/") {
            let mut receipts = self.row.receipts.clone().unwrap();
            if self.mode == 4 {
                use boole_derivation::{
                    attributes::{hex_bytes, hex_string},
                    rlp,
                };
                let raw = hex_bytes(&receipts[0], "receipt")?;
                let skip = usize::from(raw[0] < 0x80);
                let payload = &raw[skip..];
                let fields = rlp::decode(payload)
                    .map_err(anyhow::Error::msg)?
                    .children
                    .unwrap();
                let mut fields: Vec<_> = fields
                    .iter()
                    .map(|f| payload[f.span.clone()].to_vec())
                    .collect();
                fields[0] = rlp::uint(1);
                let mut forged = raw[..skip].to_vec();
                forged.extend(rlp::list(&fields));
                receipts[0] = hex_string(&forged);
            }
            return Ok(serde_json::to_vec(&receipts)?);
        }
        assert!(route.starts_with("l1/txs/"));
        let mut txs = self.row.transactions.clone().unwrap();
        match self.mode {
            1 => txs[0].replace_range(8..10, "ff"),
            2 => txs.swap(0, 1),
            3 => {
                txs.pop();
            }
            _ => {}
        }
        Ok(serde_json::to_vec(&txs)?)
    }
    fn backoff(&mut self, _: u64) {}
}
#[test]
fn m4b2_transaction_class_reconciles_exact_recorded_status_and_discards_mutants() -> Result<()> {
    use boole_derivation::attributes::hex_bytes;
    use boole_devnet_archive::client::ArchiveClient;
    let row = rows().remove(1);
    let expected = row.verify()?.canonical_receipts;
    assert!(!expected.is_empty());
    for mode in 0..=3 {
        let mut client = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(TransactionFetch {
                row: row.clone(),
                mode,
            }),
        )?;
        let result =
            client.row_from_header(&row.block_hash, 1, &hex_bytes(&row.header_rlp, "header")?)?;
        let result = client.transaction_row(&result);
        if mode == 0 {
            assert_eq!(
                result?.verify()?.canonical_receipts,
                expected,
                "verified transaction proof was dropped"
            );
        } else {
            assert!(result.is_err(), "mutated/reordered/truncated proof adopted");
            assert!(client.discarded > 0);
        }
    }
    Ok(())
}

#[test]
fn t79_archive_info_exposes_pending_environment_in_each_range() -> Result<()> {
    use boole_devnet_archive::{archive::ArchiveStore, durability::PrivateTempDir};
    let root = PrivateTempDir::new("archive-trust-axes")?;
    let rows = rows();
    let mut p = profile();
    p.identity.l1_anchor_hash = rows[0].block_hash.clone();
    let mut store = ArchiveStore::open(root.path(), p, false)?;
    store.ingest(&rows[0], 0, &selected(0))?;
    assert_eq!(
        store.info()["trust_ranges"],
        serde_json::json!([{"from":0,"through":0,"inputs":"journal","verdicts":"computed","environment":"pending"}])
    );
    Ok(())
}

#[test]
fn m2_optional_transaction_class_is_never_fetched_by_input_fold() -> Result<()> {
    use boole_devnet_archive::client::{ArchiveClient, Fetch};
    use std::sync::{Arc, Mutex};
    struct ReceiptOnly {
        row: boole_devnet_archive::input::InputRow,
        routes: Arc<Mutex<Vec<String>>>,
    }
    impl Fetch for ReceiptOnly {
        fn get(&mut self, _: &str, route: &str, _: usize) -> Result<Vec<u8>> {
            self.routes.lock().unwrap().push(route.into());
            if route.starts_with("l1/receipts/") {
                return Ok(serde_json::to_vec(self.row.receipts.as_ref().unwrap())?);
            }
            anyhow::bail!("optional class absent")
        }
        fn backoff(&mut self, _: u64) {}
    }
    let row = rows().remove(1);
    let routes = Arc::new(Mutex::new(vec![]));
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(ReceiptOnly {
            row: row.clone(),
            routes: routes.clone(),
        }),
    )?;
    let folded = client.row_from_header(
        &row.block_hash,
        1,
        &boole_derivation::attributes::hex_bytes(&row.header_rlp, "header")?,
    )?;
    assert_eq!(folded.verify()?.logs.len(), row.verify()?.logs.len());
    assert_eq!(
        routes.lock().unwrap().len(),
        1,
        "fold fetched an optional transaction proof"
    );
    assert_eq!(client.failures, vec![0]);
    assert_eq!(client.discarded, 0);
    Ok(())
}

mod support;
fn reverted_inbox_row() -> Result<boole_devnet_archive::input::InputRow> {
    support::reverted_inbox_row(rows().remove(1))
}
#[test]
fn m1_bloom_negative_inbox_block_serves_both_authenticated_proof_lists() -> Result<()> {
    use boole_devnet_archive::{
        archive::ArchiveStore, client::ArchiveClient, durability::PrivateTempDir,
        http::ArchiveServer,
    };
    use std::sync::{Arc, Mutex};
    let root = PrivateTempDir::new("negative-inbox")?;
    let genesis = rows().remove(0);
    let row = reverted_inbox_row()?;
    let mut p = profile();
    p.identity.l1_anchor_hash = genesis.block_hash.clone();
    let mut x = ArchiveStore::open(root.path(), p, false)?;
    x.ingest(&genesis, 1, &selected(0))?;
    x.ingest(&row, 1, &selected(1))?;
    assert!(
        x.transactions(&row.block_hash)?.is_some(),
        "reverted inbox transaction not served"
    );
    assert!(
        x.receipts(&row.block_hash)?.is_some(),
        "reverted inbox receipt not served"
    );
    let index = serde_json::to_value(&x.index(1, 1)?[0])?;
    assert_eq!(index["inbox_touching"], true);
    assert_eq!(index["bloom_positive"], false);
    let server = ArchiveServer::start(Arc::new(Mutex::new(x)), "127.0.0.1:0".parse()?)?;
    let mut client = ArchiveClient::http(vec![server.url()])?;
    let folded = client.row(&row.block_hash, 1)?;
    assert!(folded.transactions.is_none());
    assert!(folded.receipts.is_none());
    let proof = client.transaction_row(&folded)?;
    assert_eq!(
        proof.verify()?.canonical_receipts,
        row.verify()?.canonical_receipts
    );
    Ok(())
}

#[test]
fn m1_not_served_optional_proof_keeps_availability_without_source_penalty_or_storm() -> Result<()> {
    use boole_devnet_archive::{
        archive::ArchiveStore,
        client::{ArchiveClient, Fetch, HttpFetch},
        durability::PrivateTempDir,
        error::Error,
        http::ArchiveServer,
    };
    use std::sync::{Arc, Mutex};
    struct Count(Arc<Mutex<Vec<String>>>);
    impl Fetch for Count {
        fn get(&mut self, source: &str, route: &str, limit: usize) -> Result<Vec<u8>> {
            self.0.lock().unwrap().push(route.into());
            HttpFetch.get(source, route, limit)
        }
        fn backoff(&mut self, millis: u64) {
            self.0.lock().unwrap().push(format!("BACKOFF/{millis}"));
        }
    }
    let root = PrivateTempDir::new("optional-not-served")?;
    let mut p = profile();
    p.identity.l1_anchor_hash = rows()[0].block_hash.clone();
    let store = ArchiveStore::open(root.path(), p, false)?;
    let server = ArchiveServer::start(Arc::new(Mutex::new(store)), "127.0.0.1:0".parse()?)?;
    let calls = Arc::new(Mutex::new(vec![]));
    let mut client = ArchiveClient::new(vec![server.url()], Box::new(Count(calls.clone())))?;
    let mut row = reverted_inbox_row()?;
    row.transactions = None;
    row.receipts = None;
    for _ in 0..5 {
        let e = client
            .transaction_row(&row)
            .expect_err("withholding fabricated a posting receipt");
        assert!(
            matches!(
                e.downcast_ref::<Error>(),
                Some(Error::Availability { height: 1, .. })
            ),
            "{e}"
        );
    }
    assert_eq!(
        calls.lock().unwrap().len(),
        1,
        "not-served height refetched each tick"
    );
    client.posting_finality(2);
    assert!(client.transaction_row(&row).is_err());
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "bounded finality retry did not run"
    );
    let other = PrivateTempDir::new("optional-new-source")?;
    let other = ArchiveStore::open(other.path(), profile(), false)?;
    let other = ArchiveServer::start(Arc::new(Mutex::new(other)), "127.0.0.1:0".parse()?)?;
    client.sources = vec![other.url()];
    assert!(client.transaction_row(&row).is_err());
    assert_eq!(calls.lock().unwrap().len(), 3, "new source did not retry");
    assert_eq!(client.failures, vec![0]);
    assert_eq!(client.discarded, 0);
    Ok(())
}

#[test]
fn l7_other_block_transaction_claim_is_rejected_by_trie_binding_with_equal_counts() -> Result<()> {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        rlp,
        user_tx::keccak,
    };
    let row = rows().remove(1);
    let mut other = row.clone();
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let decoded = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let mut fields: Vec<_> = decoded
        .children
        .unwrap()
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    // A different, same-height fork can contain the same signed transactions.
    // Keep the authenticated receipt list and counts intact; vary only metadata
    // and the deliberately wrong transaction commitment.
    fields[11] = rlp::uint(row.block()?.timestamp + 1);
    fields[12] = rlp::bytes(b"alternate fork");
    let actual_root = fields[4].clone();
    fields[4] = rlp::bytes(&boole_devnet_archive::input::receipts_root(&[]));
    let raw = rlp::list(&fields);
    other.header_rlp = hex_string(&raw);
    other.block_hash = hex_string(&keccak(&raw));
    assert_eq!(
        other.receipts.as_ref().unwrap().len(),
        other.transactions.as_ref().unwrap().len()
    );
    let error = other
        .verify()
        .expect_err("another block claimed a transaction outside its trie");
    assert_eq!(
        error.to_string(),
        "transaction trie root mismatch",
        "test failed for another rule"
    );
    fields[4] = actual_root;
    let raw = rlp::list(&fields);
    other.header_rlp = hex_string(&raw);
    other.block_hash = hex_string(&keccak(&raw));
    assert_ne!(other.block_hash, row.block_hash);
    assert_eq!(
        other.verify()?.canonical_receipts,
        row.verify()?.canonical_receipts
    );
    Ok(())
}

#[test]
fn m1_forged_bloom_negative_inbox_list_cannot_supply_a_receipt() -> Result<()> {
    use boole_devnet_archive::client::ArchiveClient;
    let row = reverted_inbox_row()?;
    let expected = row.verify()?.canonical_receipts;
    let mut folded = row.clone();
    folded.transactions = None;
    folded.receipts = None;
    for mode in [0, 1] {
        let mut client = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(TransactionFetch {
                row: row.clone(),
                mode,
            }),
        )?;
        let result = client.transaction_row(&folded);
        if mode == 0 {
            assert_eq!(result?.verify()?.canonical_receipts, expected);
        } else {
            assert!(result.is_err());
            assert_eq!(client.failures, vec![3]);
            assert_eq!(client.discarded, 3);
        }
    }
    Ok(())
}

#[test]
fn m1_legacy_index_flag_never_blocks_authenticated_proof_backfill() -> Result<()> {
    use boole_devnet_archive::{archive::ArchiveStore, durability::PrivateTempDir};
    let root = PrivateTempDir::new("legacy-touching-flag")?;
    let genesis = rows().remove(0);
    let row = reverted_inbox_row()?;
    let mut p = profile();
    p.identity.l1_anchor_hash = genesis.block_hash.clone();
    let mut store = ArchiveStore::open(root.path(), p.clone(), false)?;
    store.ingest(&genesis, 1, &selected(0))?;
    store.ingest(&row, 1, &selected(1))?;
    drop(store);
    let path = root.path().join("index.ndjson");
    let mut legacy = String::new();
    for line in std::fs::read_to_string(&path)?.lines() {
        let mut value: serde_json::Value = serde_json::from_str(line)?;
        value.as_object_mut().unwrap().remove("inbox_touching");
        legacy += &serde_json::to_string(&value)?;
        legacy.push('\n');
    }
    std::fs::write(&path, &legacy)?;
    for class in ["receipts", "txs"] {
        std::fs::remove_file(root.path().join(class).join(&row.block_hash[2..]))?;
    }
    let mut store = ArchiveStore::open(root.path(), p, false)?;
    store.ingest(&row, 1, &selected(1))?;
    assert_eq!(
        std::fs::read_to_string(path)?,
        legacy,
        "diagnostic index was rewritten"
    );
    assert!(store.index(1, 1)?[0].inbox_touching.is_none());
    assert!(store.transactions(&row.block_hash)?.is_some());
    assert!(store.receipts(&row.block_hash)?.is_some());
    Ok(())
}

#[test]
fn m1_not_served_source_is_cached_even_while_another_source_returns_bad_data() -> Result<()> {
    use boole_devnet_archive::client::{ArchiveClient, Fetch, NotServed};
    use std::sync::{Arc, Mutex};
    struct Mixed(Arc<Mutex<Vec<String>>>);
    impl Fetch for Mixed {
        fn get(&mut self, source: &str, _: &str, _: usize) -> Result<Vec<u8>> {
            self.0.lock().unwrap().push(source.into());
            if source.ends_with(":1") {
                Err(NotServed.into())
            } else {
                Ok(b"invalid proof list".to_vec())
            }
        }
        fn backoff(&mut self, _: u64) {}
    }
    let calls = Arc::new(Mutex::new(vec![]));
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into(), "http://127.0.0.1:2".into()],
        Box::new(Mixed(calls.clone())),
    )?;
    let mut row = reverted_inbox_row()?;
    row.transactions = None;
    row.receipts = None;
    for _ in 0..3 {
        assert!(client.transaction_row(&row).is_err());
    }
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.ends_with(":1"))
            .count(),
        1,
        "404 source was refetched on each tick"
    );
    assert_eq!(client.failures, vec![0, 3]);
    client.posting_finality(2);
    assert!(client.transaction_row(&row).is_err());
    assert_eq!(
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.ends_with(":1"))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn f2_unknown_authenticated_l1_type_is_conservatively_stored_not_sticky() -> Result<()> {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        rlp,
        user_tx::keccak,
    };
    use boole_devnet_archive::{
        archive::ArchiveStore, durability::PrivateTempDir, input::receipts_root,
    };
    let mut row = reverted_inbox_row()?;
    let tx = vec![5, 0xc0]; // Future envelope: its opaque bytes are trie authenticated.
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let d = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let mut fields: Vec<_> = d
        .children
        .unwrap()
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    fields[4] = rlp::bytes(&receipts_root(std::slice::from_ref(&tx)));
    let raw = rlp::list(&fields);
    row.header_rlp = hex_string(&raw);
    row.block_hash = hex_string(&keccak(&raw));
    row.transactions = Some(vec![hex_string(&tx)]);
    row.verify()?;
    let root = PrivateTempDir::new("unknown-inbox-type")?;
    let genesis = rows().remove(0);
    let mut p = profile();
    p.identity.l1_anchor_hash = genesis.block_hash.clone();
    let mut store = ArchiveStore::open(root.path(), p, false)?;
    store.ingest(&genesis, 1, &selected(0))?;
    store.ingest(&row, 1, &selected(1))?;
    assert!(store.transactions(&row.block_hash)?.is_some());
    assert!(store.receipts(&row.block_hash)?.is_some());
    assert_eq!(
        serde_json::to_value(&store.index(1, 1)?[0])?["inbox_touching"],
        true
    );
    assert!(store.info()["ingest_health"].is_null());
    Ok(())
}

struct Proof404 {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    coverage: Option<u64>,
}
impl boole_devnet_archive::client::Fetch for Proof404 {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(boole_devnet_archive::client::NotServed.into())
    }
    fn observe(&mut self, _: &str, _: &str, _: usize, _: std::time::Instant) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(
            &serde_json::json!({"covered_through":self.coverage}),
        )?)
    }
    fn backoff(&mut self, _: u64) {
        panic!("404 must not incur immediate retry backoff");
    }
}
fn missing_proof_row(height: u64) -> Result<boole_devnet_archive::input::InputRow> {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        rlp,
        user_tx::keccak,
    };
    let mut row = reverted_inbox_row()?;
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let d = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let mut fields: Vec<_> = d
        .children
        .unwrap()
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    fields[8] = rlp::uint(height);
    let raw = rlp::list(&fields);
    row.header_rlp = hex_string(&raw);
    row.block_hash = hex_string(&keccak(&raw));
    row.receipts = None;
    row.transactions = None;
    row.verify()?;
    Ok(row)
}
#[test]
fn f4_covered_404_survives_finality_and_new_sources_can_retry() -> Result<()> {
    use boole_devnet_archive::client::ArchiveClient;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(Proof404 {
            calls: calls.clone(),
            coverage: Some(1000),
        }),
    )?;
    let row = missing_proof_row(1)?;
    for finality in 1000..1020 {
        client.posting_finality(finality);
        assert!(client.transaction_row(&row).is_err());
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "covered 404 was reprobed at every finality"
    );
    assert_eq!(client.failures, vec![0]);
    client.sources[0] = "http://127.0.0.1:2".into();
    client.posting_finality(1020);
    assert!(client.transaction_row(&row).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}
#[test]
fn f4_legacy_window_has_a_constant_per_finality_request_budget() -> Result<()> {
    use boole_devnet_archive::client::ArchiveClient;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(Proof404 {
            calls: calls.clone(),
            coverage: Some(1000),
        }),
    )?;
    let rows = (1..=200)
        .map(missing_proof_row)
        .collect::<Result<Vec<_>>>()?;
    for finality in 1000..1008 {
        client.posting_finality(finality);
        let before = calls.load(Ordering::SeqCst);
        let mut heights: Vec<_> = (1..=200).collect();
        client.posting_probe_order(&mut heights);
        for height in heights {
            assert!(client.transaction_row(&rows[height as usize - 1]).is_err());
        }
        assert!(
            calls.load(Ordering::SeqCst) - before <= 32,
            "requests grew with legacy window"
        );
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        200,
        "budget starved unprobed heights"
    );
    Ok(())
}
#[test]
fn f4_uncovered_404_uses_per_height_bounded_exponential_backoff() -> Result<()> {
    use boole_devnet_archive::client::ArchiveClient;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut client = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into()],
        Box::new(Proof404 {
            calls: calls.clone(),
            coverage: None,
        }),
    )?;
    let row = missing_proof_row(1)?;
    for finality in 1000..1006 {
        client.posting_finality(finality);
        assert!(client.transaction_row(&row).is_err());
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "uncovered proof was retried on every step"
    );
    client.posting_finality(1006);
    assert!(client.transaction_row(&row).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[test]
fn f6_archive_filled_negative_row_has_unknown_touching_and_no_proof() -> Result<()> {
    use boole_devnet_archive::{archive::ArchiveStore, durability::PrivateTempDir};
    let root = PrivateTempDir::new("archive-unknown-touching")?;
    let genesis = rows().remove(0);
    let mut row = reverted_inbox_row()?;
    row.receipts = None;
    row.transactions = None;
    row.verify()?;
    let mut p = profile();
    p.identity.l1_anchor_hash = genesis.block_hash.clone();
    let mut store = ArchiveStore::open(root.path(), p, false)?;
    store.ingest(&genesis, 1, &selected(0))?;
    store.ingest(&row, 1, &selected(1))?;
    assert!(
        serde_json::to_value(&store.index(1, 1)?[0])?["inbox_touching"].is_null(),
        "unknown touching was reported false"
    );
    assert!(store.transactions(&row.block_hash)?.is_none());
    assert!(store.receipts(&row.block_hash)?.is_none());
    Ok(())
}

#[test]
fn f6_bloom_negative_receipt_status_flip_cannot_replace_reverted() -> Result<()> {
    use boole_devnet_archive::client::{ArchiveClient, Fetch};
    let row = reverted_inbox_row()?;
    assert!(!row.verify()?.canonical_receipts[0].1);
    let mut transport = TransactionFetch {
        row: row.clone(),
        mode: 4,
    };
    let bytes = transport.get("http://127.0.0.1:1", "l1/receipts/forged", 10000)?;
    let mut forged = row.clone();
    forged.receipts = Some(serde_json::from_slice(&bytes)?);
    assert_eq!(
        forged.verify().unwrap_err().to_string(),
        "receipt trie root mismatch"
    );
    let mut folded = row.clone();
    folded.receipts = None;
    folded.transactions = None;
    for mode in [0, 4] {
        let mut client = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(TransactionFetch {
                row: row.clone(),
                mode,
            }),
        )?;
        let result = client.transaction_row(&folded);
        if mode == 0 {
            assert_eq!(
                result?.verify()?.canonical_receipts,
                row.verify()?.canonical_receipts
            );
        } else {
            assert!(result.is_err());
            assert_eq!(client.failures, vec![3]);
        }
    }
    Ok(())
}
