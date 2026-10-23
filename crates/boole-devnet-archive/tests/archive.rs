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
    assert!(
        x.ingest(&r[1], 0, &selected(1)).is_err(),
        "unfinalized input was ingested"
    );
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
        !request("POST", "info")?.starts_with(b"HTTP/1.1 200"),
        "API accepted a write method"
    );
    assert!(
        !request("GET", "index/0?limit=1025")?.starts_with(b"HTTP/1.1 200"),
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
