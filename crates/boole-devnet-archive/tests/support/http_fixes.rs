use super::*;
use boole_devnet_archive::{
    archive::{sha, ArchiveStore},
    client::{ArchiveClient, Fetch},
    durability::PrivateTempDir,
    http::ArchiveServer,
};
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
fn server() -> Result<(PrivateTempDir, Arc<Mutex<ArchiveStore>>, ArchiveServer)> {
    let d = PrivateTempDir::new("archive-fix")?;
    let store = Arc::new(Mutex::new(ArchiveStore::open(d.path(), profile(), false)?));
    let s = ArchiveServer::start(store.clone(), "127.0.0.1:0".parse()?)?;
    Ok((d, store, s))
}
fn request(addr: std::net::SocketAddr, raw: &[u8]) -> Result<Vec<u8>> {
    let mut s = TcpStream::connect(addr)?;
    s.set_read_timeout(Some(Duration::from_secs(4)))?;
    s.write_all(raw)?;
    let mut response = vec![];
    s.read_to_end(&mut response)?;
    Ok(response)
}
#[test]
#[allow(unsafe_code)]
fn s1_slow_reader_does_not_serialize_info_and_drop_closes_connections() -> Result<()> {
    let (_d, store, server) = server()?;
    let bytes = vec![7; 8 * 1024 * 1024];
    let pin: boole_derivation::m2::Hash =
        boole_derivation::attributes::hex_bytes(&sha(&bytes), "pin")?
            .try_into()
            .unwrap();
    store.lock().unwrap().put_content("env", pin, &bytes)?;
    let mut slow = TcpStream::connect(server.addr())?;
    slow.set_read_timeout(Some(Duration::from_secs(4)))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let n: libc::c_int = 4096;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    slow.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    (&n as *const libc::c_int).cast(),
                    std::mem::size_of_val(&n) as _,
                )
            },
            0
        );
    }
    write!(
        slow,
        "GET /boole/archive/v1/obj/env/{} HTTP/1.1\r\nHost: local\r\n\r\n",
        &sha(&bytes)[2..]
    )?;
    let mut one = [0; 1];
    slow.read_exact(&mut one)?;
    let close = slow.try_clone()?;
    let (done, stop) = mpsc::channel();
    let reader = thread::spawn(move || {
        while stop.recv_timeout(Duration::from_millis(1900)).is_err() {
            if slow.read_exact(&mut one).is_err() {
                break;
            }
        }
    });
    let start = Instant::now();
    let answer = request(
        server.addr(),
        b"GET /boole/archive/v1/info HTTP/1.1\r\nHost: local\r\n\r\n",
    );
    let elapsed = start.elapsed();
    let start = Instant::now();
    drop(server);
    let drop_time = start.elapsed();
    let _ = done.send(());
    let _ = close.shutdown(Shutdown::Both);
    reader.join().unwrap();
    assert!(answer?.starts_with(b"HTTP/1.1 200"));
    assert!(
        elapsed < Duration::from_millis(750),
        "one slow reader delayed info by {elapsed:?}"
    );
    assert!(
        drop_time < Duration::from_millis(500),
        "Drop waited on client {drop_time:?}"
    );
    Ok(())
}
#[test]
fn s4_urls_headers_and_methods_are_canonical() -> Result<()> {
    let (_d, _store, s) = server()?;
    for path in [
        "index/00",
        "index/+1",
        "index/1?limit=01",
        "index/1?limit=+1",
        "l2/system/00",
        "l2/system/+1",
        "l1/header/AA",
        "l1/header/abc",
        "../info",
    ] {
        let raw = format!("GET /boole/archive/v1/{path} HTTP/1.1\r\nHost: local\r\n\r\n");
        assert!(
            request(s.addr(), raw.as_bytes())?.starts_with(b"HTTP/1.1 400"),
            "accepted {path}"
        );
    }
    for key in ["AB".repeat(32), "a".repeat(63), "b".repeat(65)] {
        let raw = format!("GET /boole/archive/v1/l1/header/{key} HTTP/1.1\r\n\r\n");
        assert!(request(s.addr(), raw.as_bytes())?.starts_with(b"HTTP/1.1 400"));
    }
    for raw in [
        b"GET /boole/archive/v1/info HTTP/1.1\nHost: local\r\n\r\n".as_slice(),
        b"GET /boole/archive/v1/info HTTP/1.1\r\nHost : local\r\n\r\n",
    ] {
        assert!(request(s.addr(), raw)?.starts_with(b"HTTP/1.1 400"));
    }
    assert!(request(
        s.addr(),
        b"HEAD /boole/archive/v1/info HTTP/1.1\r\nHost: local\r\n\r\n"
    )?
    .starts_with(b"HTTP/1.1 405"));
    assert!(request(
        s.addr(),
        format!(
            "GET /boole/archive/v1/obj/private/{} HTTP/1.1\r\nHost: local\r\n\r\n",
            "00".repeat(32)
        )
        .as_bytes()
    )?
    .starts_with(b"HTTP/1.1 404"));
    assert!(ArchiveServer::start(_store, "0.0.0.0:0".parse()?).is_err());
    Ok(())
}
#[test]
fn s4_incomplete_request_returns_408_within_total_deadline() -> Result<()> {
    let (_d, _store, s) = server()?;
    let mut c = TcpStream::connect(s.addr())?;
    c.set_read_timeout(Some(Duration::from_secs(4)))?;
    c.write_all(b"GET /boole/archive/v1/info HTTP/1.1\r\nHost: local\r\n")?;
    let at = Instant::now();
    let mut body = vec![];
    c.read_to_end(&mut body)?;
    assert!(at.elapsed() >= Duration::from_millis(1800));
    assert!(
        body.starts_with(b"HTTP/1.1 408"),
        "deadline closed silently: {body:?}"
    );
    Ok(())
}
#[test]
fn s3_stored_header_corruption_returns_typed_500_and_quarantines() -> Result<()> {
    let d = PrivateTempDir::new("archive-corrupt-http")?;
    let r = rows();
    let mut p = profile();
    p.identity.l1_anchor_hash = r[0].block_hash.clone();
    let mut store = ArchiveStore::open(d.path(), p, false)?;
    store.ingest(&r[0], 0, &selected(0))?;
    let file = d.path().join("header").join(&r[0].block_hash[2..]);
    std::fs::write(&file, b"corrupted stored bytes")?;
    let s = ArchiveServer::start(Arc::new(Mutex::new(store)), "127.0.0.1:0".parse()?)?;
    let response = request(
        s.addr(),
        format!(
            "GET /boole/archive/v1/l1/header/{} HTTP/1.1\r\nHost: local\r\n\r\n",
            &r[0].block_hash[2..]
        )
        .as_bytes(),
    )?;
    assert!(!file.exists());
    assert_eq!(
        std::fs::read_dir(d.path().join("header/corrupt"))?.count(),
        1
    );
    assert!(
        response.starts_with(b"HTTP/1.1 500"),
        "integrity misclassified: {}",
        String::from_utf8_lossy(&response)
    );
    assert!(String::from_utf8_lossy(&response).contains("object-quarantine"));
    Ok(())
}
#[test]
fn c1_dripped_response_expires_then_uses_clean_source() -> Result<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let bad = format!("http://{}", listener.local_addr()?);
    let slow = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut b = [0; 4096];
        let _ = stream.read(&mut b);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\na");
        thread::sleep(Duration::from_millis(1900));
        let _ = stream.write_all(b"b");
        thread::sleep(Duration::from_millis(1900));
        let _ = stream.write_all(b"c");
    });
    let good = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", good.local_addr()?);
    good.set_nonblocking(true)?;
    let (stop, rx) = mpsc::channel();
    let healthy = thread::spawn(move || loop {
        if rx.try_recv().is_ok() {
            break;
        }
        match good.accept() {
            Ok((mut s, _)) => {
                let mut b = [0; 4096];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc");
                break;
            }
            Err(_) => thread::sleep(Duration::from_millis(2)),
        }
    });
    let mut c = ArchiveClient::http(vec![bad, url])?;
    let start = Instant::now();
    let result = c.fetch_verified("obj/env/00", 3, 0, |b| {
        anyhow::ensure!(b == b"abc", "body");
        Ok(b.to_vec())
    });
    let elapsed = start.elapsed();
    let _ = stop.send(());
    slow.join().unwrap();
    healthy.join().unwrap();
    assert_eq!(result?, b"abc");
    assert_eq!(c.failures[0], 1, "slow source was accepted");
    assert_eq!(c.failures[1], 0);
    assert!(
        elapsed < Duration::from_secs(3),
        "body deadline not total: {elapsed:?}"
    );
    Ok(())
}
struct Behind;
impl Fetch for Behind {
    fn get(&mut self, _: &str, route: &str, _: usize) -> Result<Vec<u8>> {
        if route == "info" {
            Ok(br#"{"covered_through":0}"#.to_vec())
        } else {
            Ok(b"[]".to_vec())
        }
    }
}
#[test]
fn c2_not_indexed_is_not_an_inconsistency_alert() -> Result<()> {
    let mut c = ArchiveClient::new(vec!["http://127.0.0.1:1".into()], Box::new(Behind))?;
    assert!(
        c.tripwire(1, &sha(b"l1"), &sha(b"l2"), &sha(b"root"))
            .is_empty(),
        "not-yet-indexed generated an alert"
    );
    Ok(())
}

#[test]
fn c3_receipt_cap_stops_parsing_before_the_next_value() -> Result<()> {
    let mut raw = String::from("[");
    raw.push_str(&vec!["\"0x\""; 65536].join(","));
    raw.push_str(",{\"unparsed\":");
    let error = boole_devnet_archive::client::receipt_list(raw.as_bytes()).unwrap_err();
    assert!(
        error.to_string().contains("receipt count cap"),
        "parsed beyond the entry cap: {error}"
    );
    Ok(())
}
#[test]
fn s1_connection_saturation_refuses_fast() -> Result<()> {
    let (_d, _store, server) = server()?;
    let mut held = vec![];
    for _ in 0..8 {
        let mut s = TcpStream::connect(server.addr())?;
        s.write_all(b"GET /boole/archive/v1/info HTTP/1.1\r\n")?;
        held.push(s);
    }
    thread::sleep(Duration::from_millis(100));
    let at = Instant::now();
    let reply = request(
        server.addr(),
        b"GET /boole/archive/v1/info HTTP/1.1\r\n\r\n",
    );
    match reply {
        Ok(reply) => assert!(
            reply.starts_with(b"HTTP/1.1 503"),
            "unexpected saturation reply: {:?}",
            String::from_utf8_lossy(&reply)
        ),
        Err(error) => assert!(
            error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionReset),
            "unexpected refusal {error}"
        ),
    };
    assert!(at.elapsed() < Duration::from_millis(300));
    drop(server);
    drop(held);
    Ok(())
}
#[test]
fn c2_real_tcp_stall_consumes_one_shared_step_budget() -> Result<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let _ = s.read(&mut bytes);
        let _ = rx.recv_timeout(Duration::from_secs(2));
    });
    let mut c = ArchiveClient::http(vec![url, "http://127.0.0.1:1".into()])?;
    let at = Instant::now();
    assert!(c
        .tripwire(1, &sha(b"l1"), &sha(b"l2"), &sha(b"root"))
        .is_empty());
    let elapsed = at.elapsed();
    let _ = tx.send(());
    worker.join().unwrap();
    assert!(
        elapsed < Duration::from_millis(150),
        "diagnostic TCP stall exceeded shared budget: {elapsed:?}"
    );
    Ok(())
}
struct IndexFetch {
    coverage: u64,
    index: Vec<u8>,
    calls: Arc<Mutex<Vec<String>>>,
}
impl Fetch for IndexFetch {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        panic!("diagnostics used required-data retry path")
    }
    fn observe(&mut self, _: &str, route: &str, _: usize, _: Instant) -> Result<Vec<u8>> {
        self.calls.lock().unwrap().push(route.into());
        if route == "info" {
            Ok(serde_json::to_vec(
                &serde_json::json!({"covered_through":self.coverage}),
            )?)
        } else {
            Ok(self.index.clone())
        }
    }
}
#[test]
fn c2_only_a_well_formed_conflicting_index_alerts() -> Result<()> {
    let l1 = sha(b"l1");
    let l2 = sha(b"l2");
    let root = sha(b"root");
    let row = boole_devnet_archive::archive::IndexRow {
        l1_number: 1,
        l2_number: 1,
        l1_hash: l1.clone(),
        parent_hash: sha(b"parent"),
        bloom_positive: false,
        inbox_touching: Some(false),
        blob_ids: vec![],
        l2_hash: l2.clone(),
        root: sha(b"wrong"),
        inputs: "journal".into(),
        verdicts: "computed".into(),
        system_digest: sha(b"system"),
    };
    for (coverage, body, alerts, index_calls) in [
        (0, serde_json::to_vec(&vec![row.clone()])?, 0, 0),
        (1, b"[]".to_vec(), 0, 1),
        (1, b"malformed".to_vec(), 0, 1),
        (1, serde_json::to_vec(&vec![row])?, 1, 1),
    ] {
        let calls = Arc::new(Mutex::new(vec![]));
        let mut c = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into()],
            Box::new(IndexFetch {
                coverage,
                index: body,
                calls: calls.clone(),
            }),
        )?;
        assert_eq!(c.tripwire(1, &l1, &l2, &root).len(), alerts);
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.starts_with("index/"))
                .count(),
            index_calls
        );
    }
    Ok(())
}
#[test]
fn c3_exact_receipt_cap_and_empty_list_are_accepted() -> Result<()> {
    assert!(boole_devnet_archive::client::receipt_list(b"[]")?.is_empty());
    let raw = serde_json::to_vec(&vec!["0x"; 65536])?;
    assert_eq!(
        boole_devnet_archive::client::receipt_list(&raw)?.len(),
        65536
    );
    Ok(())
}
#[test]
fn s3_receipts_and_system_corruption_are_500_then_404_until_restart() -> Result<()> {
    for class in ["receipts", "system"] {
        let d = PrivateTempDir::new("archive-corrupt-classes")?;
        let r = rows();
        let mut p = profile();
        p.identity.l1_anchor_hash = r[0].block_hash.clone();
        let mut x = ArchiveStore::open(d.path(), p.clone(), false)?;
        for (h, row) in r.iter().enumerate() {
            x.ingest(row, 2, &selected(h as u64))?;
        }
        let (key, route) = if class == "receipts" {
            (
                r[1].block_hash[2..].to_owned(),
                format!("l1/receipts/{}", &r[1].block_hash[2..]),
            )
        } else {
            (
                x.index(1, 1)?[0].system_digest[2..].to_owned(),
                "l2/system/1".into(),
            )
        };
        let path = d.path().join(class).join(&key);
        assert!(path.exists(), "fixture lacks {class}: {path:?}");
        std::fs::write(&path, b"corrupt object")?;
        let server = ArchiveServer::start(Arc::new(Mutex::new(x)), "127.0.0.1:0".parse()?)?;
        let req = format!("GET /boole/archive/v1/{route} HTTP/1.1\r\n\r\n");
        let response = request(server.addr(), req.as_bytes())?;
        assert!(response.starts_with(b"HTTP/1.1 500"));
        assert!(String::from_utf8_lossy(&response).contains("object-quarantine"));
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_dir(d.path().join(class).join("corrupt"))?.count(),
            1
        );
        assert!(request(server.addr(), req.as_bytes())?.starts_with(b"HTTP/1.1 404"));
        drop(server);
        let mut x = ArchiveStore::open(d.path(), p, false)?;
        for (h, row) in r.iter().enumerate() {
            x.ingest(row, 2, &selected(h as u64))?;
        }
        assert!(path.exists(), "restart replay did not refill quarantine");
    }
    Ok(())
}
#[test]
fn s3_manifest_replacement_sets_health_and_stops_serving() -> Result<()> {
    let (d, _store, server) = server()?;
    std::fs::write(d.path().join("archive.manifest.json"), b"changed")?;
    let req = b"GET /boole/archive/v1/info HTTP/1.1\r\n\r\n";
    assert!(request(server.addr(), req)?.starts_with(b"HTTP/1.1 500"));
    assert!(matches!(
        server
            .health()
            .unwrap_err()
            .downcast_ref::<boole_devnet_archive::error::Error>(),
        Some(boole_devnet_archive::error::Error::Integrity {
            class: "archive-store",
            ..
        })
    ));
    assert!(request(server.addr(), req)?.starts_with(b"HTTP/1.1 503"));
    Ok(())
}

struct HangingCovered {
    stalls: Arc<std::sync::atomic::AtomicUsize>,
    healthy: Arc<std::sync::atomic::AtomicUsize>,
}
impl Fetch for HangingCovered {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        Ok(b"required bytes".to_vec())
    }
    fn observe(
        &mut self,
        source: &str,
        route: &str,
        _: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>> {
        use std::sync::atomic::Ordering;
        if route == "info" {
            return Ok(br#"{"covered_through":1000000}"#.to_vec());
        }
        if source.ends_with(":1") {
            self.stalls.fetch_add(1, Ordering::SeqCst);
            if let Some(left) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(left)
            }
            anyhow::bail!("archive response deadline")
        }
        self.healthy.fetch_add(1, Ordering::SeqCst);
        let h = route
            .strip_prefix("index/")
            .unwrap()
            .split('?')
            .next()
            .unwrap()
            .parse::<u64>()?;
        let row = boole_devnet_archive::archive::IndexRow {
            l1_number: h,
            l2_number: h,
            l1_hash: sha(b"l1"),
            parent_hash: sha(b"parent"),
            bloom_positive: false,
            inbox_touching: Some(false),
            blob_ids: vec![],
            l2_hash: sha(b"l2"),
            root: sha(b"root"),
            inputs: "journal".into(),
            verdicts: "computed".into(),
            system_digest: sha(b"system"),
        };
        Ok(serde_json::to_vec(&vec![row])?)
    }
}
#[test]
fn m4_hanging_covered_source_has_bounded_total_cost_during_catchup() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let stalls = Arc::new(AtomicUsize::new(0));
    let healthy = Arc::new(AtomicUsize::new(0));
    let mut c = ArchiveClient::new(
        vec!["http://127.0.0.1:1".into(), "http://127.0.0.1:2".into()],
        Box::new(HangingCovered {
            stalls: stalls.clone(),
            healthy: healthy.clone(),
        }),
    )?;
    let at = Instant::now();
    for h in 1..=1000 {
        assert!(c
            .tripwire(h, &sha(b"l1"), &sha(b"l2"), &sha(b"root"))
            .is_empty());
        assert!(
            at.elapsed() < Duration::from_millis(500),
            "hanging source charged every height: h={h} elapsed={:?}",
            at.elapsed()
        );
    }
    assert_eq!(stalls.load(Ordering::SeqCst), 1);
    assert!(healthy.load(Ordering::SeqCst) >= 999);
    assert_eq!(c.failures[0], 1);
    assert!(c.attempts[0] >= 1);
    assert_eq!(
        c.fetch_verified("obj/env/pinned", 64, 1000, |b| Ok(b.to_vec()))?,
        b"required bytes",
        "diagnostic cooldown blocked required-data FETCH"
    );
    println!(
        "catchup_heights=1000 stalled_attempts={} healthy_observations={} seconds={:.4}",
        stalls.load(Ordering::SeqCst),
        healthy.load(Ordering::SeqCst),
        at.elapsed().as_secs_f64()
    );
    Ok(())
}
struct ResponsiveOrder(Arc<Mutex<Vec<String>>>);
impl Fetch for ResponsiveOrder {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        panic!("tripwire used required FETCH")
    }
    fn observe(&mut self, source: &str, route: &str, _: usize, _: Instant) -> Result<Vec<u8>> {
        if route == "info" {
            return Ok(br#"{"covered_through":1}"#.to_vec());
        }
        self.0.lock().unwrap().push(source.into());
        let row = boole_devnet_archive::archive::IndexRow {
            l1_number: 1,
            l2_number: 1,
            l1_hash: sha(b"l1"),
            parent_hash: sha(b"parent"),
            bloom_positive: false,
            inbox_touching: Some(false),
            blob_ids: vec![],
            l2_hash: sha(b"l2"),
            root: sha(b"root"),
            inputs: "journal".into(),
            verdicts: "computed".into(),
            system_digest: sha(b"system"),
        };
        Ok(serde_json::to_vec(&vec![row])?)
    }
}
#[test]
fn m4_diagnostic_first_source_rotates_even_when_sources_are_responsive() -> Result<()> {
    let calls = Arc::new(Mutex::new(vec![]));
    let sources = vec![
        "http://127.0.0.1:1".to_owned(),
        "http://127.0.0.1:2".to_owned(),
    ];
    let mut client = ArchiveClient::new(sources.clone(), Box::new(ResponsiveOrder(calls.clone())))?;
    for _ in 0..2 {
        assert!(client
            .tripwire(1, &sha(b"l1"), &sha(b"l2"), &sha(b"root"))
            .is_empty());
    }
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            sources[0].clone(),
            sources[1].clone(),
            sources[1].clone(),
            sources[0].clone()
        ],
        "fixed source order can starve later diagnostics"
    );
    Ok(())
}
struct PartialBudget {
    deferred: Arc<std::sync::atomic::AtomicUsize>,
    metadata: bool,
}
impl Fetch for PartialBudget {
    fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
        panic!("diagnostic used FETCH")
    }
    fn observe(
        &mut self,
        source: &str,
        route: &str,
        _: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>> {
        if source.ends_with(":2")
            && (route == "info") == self.metadata
            && deadline.saturating_duration_since(Instant::now()) < Duration::from_millis(45)
        {
            self.deferred
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into());
        }
        if route == "info" {
            return Ok(br#"{"covered_through":1000}"#.to_vec());
        }
        if source.ends_with(":1") {
            let until = Instant::now() + Duration::from_millis(10);
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        }

        let h = route
            .strip_prefix("index/")
            .unwrap()
            .split('?')
            .next()
            .unwrap()
            .parse::<u64>()?;
        let row = boole_devnet_archive::archive::IndexRow {
            l1_number: h,
            l2_number: h,
            l1_hash: sha(b"l1"),
            parent_hash: sha(b"parent"),
            bloom_positive: false,
            inbox_touching: Some(false),
            blob_ids: vec![],
            l2_hash: sha(b"l2"),
            root: if source.ends_with(":1") {
                sha(b"root")
            } else {
                sha(b"other")
            },
            inputs: "journal".into(),
            verdicts: "computed".into(),
            system_digest: sha(b"system"),
        };
        Ok(serde_json::to_vec(&vec![row])?)
    }
}
#[test]
fn m4_shared_budget_expiry_does_not_cool_down_a_responsive_later_source() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for metadata in [false, true] {
        let deferred = Arc::new(AtomicUsize::new(0));
        let mut c = ArchiveClient::new(
            vec!["http://127.0.0.1:1".into(), "http://127.0.0.1:2".into()],
            Box::new(PartialBudget {
                deferred: deferred.clone(),
                metadata,
            }),
        )?;
        assert!(c
            .tripwire(1, &sha(b"l1"), &sha(b"l2"), &sha(b"root"))
            .is_empty());
        assert_eq!(
            deferred.load(Ordering::SeqCst),
            1,
            "fixture did not exercise a partial-window timeout"
        );
        assert_eq!(
            c.tripwire(2, &sha(b"l1"), &sha(b"l2"), &sha(b"root")).len(),
            1,
            "later responsive source was excluded by an unfair cooldown"
        );
        assert_eq!(
            c.failures[1], 0,
            "shared budget exhaustion was blamed on the source"
        );
    }
    Ok(())
}
