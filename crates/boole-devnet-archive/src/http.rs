//! Bounded loopback HTTP/1.1 serving; no write or segment route.
use crate::{
    archive::{ArchiveStore, REQ_MAX},
    error::Error,
};
use anyhow::{ensure, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub const PREFIX: &str = "/boole/archive/v1/";
const CONNECTIONS: usize = 8;
const REQUEST_TIME: Duration = Duration::from_secs(2);
/// Shared fixed slack and minimum loopback throughput for bounded body I/O.
pub(crate) fn transfer_budget(length: usize) -> Duration {
    Duration::from_secs(2) + Duration::from_secs_f64(length as f64 / (8.0 * 1024.0 * 1024.0))
}
type Health = Arc<Mutex<Option<Error>>>;
type Work = Box<dyn FnOnce() + Send>;
type SpawnWorker = Arc<dyn Fn(Work) -> std::io::Result<JoinHandle<()>> + Send + Sync>;
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}
impl Response {
    fn error(status: u16) -> Self {
        Self {
            status,
            body: format!("{{\"archive_error\":{status}}}").into_bytes(),
        }
    }
    fn typed(status: u16, class: &str) -> Self {
        Self {
            status,
            body: format!("{{\"archive_error\":{status},\"class\":\"{class}\"}}").into_bytes(),
        }
    }
}
pub struct ArchiveServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    health: Health,
    sockets: Arc<Mutex<BTreeMap<u64, TcpStream>>>,
    thread: Option<JoinHandle<()>>,
}
impl ArchiveServer {
    pub fn start(store: Arc<Mutex<ArchiveStore>>, bind: SocketAddr) -> Result<Self> {
        Self::start_with_spawner(
            store,
            bind,
            Arc::new(|work| {
                thread::Builder::new()
                    .name("archive-http".into())
                    .spawn(work)
            }),
        )
    }
    fn start_with_spawner(
        store: Arc<Mutex<ArchiveStore>>,
        bind: SocketAddr,
        spawn: SpawnWorker,
    ) -> Result<Self> {
        ensure!(
            bind.ip().is_loopback(),
            "S21: archive bind must be loopback"
        );
        let listener = TcpListener::bind(bind)?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let health: Health = Default::default();
        let serving = health.clone();
        let sockets: Arc<Mutex<BTreeMap<u64, TcpStream>>> = Default::default();
        let active = sockets.clone();
        let serving_loop = move || {
            let mut workers: Vec<JoinHandle<()>> = vec![];
            let mut next_id = 0u64;
            let mut spawn_logged = false;
            let logged = Arc::new(Mutex::new(BTreeSet::new()));
            accept_loop(
                &flag,
                &serving,
                || listener.accept().map(|(s, _)| s),
                |mut stream| {
                    let mut i = 0;
                    while i < workers.len() {
                        if workers[i].is_finished() {
                            let _ = workers.swap_remove(i).join();
                        } else {
                            i += 1
                        }
                    }
                    if workers.len() >= CONNECTIONS {
                        let _ = write_response(
                            &mut stream,
                            Response::error(503),
                            Instant::now() + Duration::from_millis(20),
                        );
                        return;
                    }
                    // Retain only live workers' sockets; shutdown clones are used only by Drop.
                    let id = next_id;
                    let Some(next) = next_id.checked_add(1) else {
                        if let Ok(mut h) = serving.lock() {
                            *h = Some(Error::Integrity {
                                height: 0,
                                class: "archive-server",
                                detail: "connection id exhausted".into(),
                            });
                        }
                        flag.store(true, Ordering::Release);
                        return;
                    };
                    next_id = next;
                    if let Ok(clone) = stream.try_clone() {
                        if let Ok(mut all) = active.lock() {
                            all.insert(id, clone);
                        }
                    } else {
                        return;
                    }
                    let (store, flag, health, active, logged) = (
                        store.clone(),
                        flag.clone(),
                        serving.clone(),
                        active.clone(),
                        logged.clone(),
                    );
                    let cleanup = active.clone();
                    match spawn(Box::new(move || {
                        let _ = handle(&mut stream, &store, &flag, &health, &logged);
                        if let Ok(mut all) = active.lock() {
                            all.remove(&id);
                        }
                        let _ = stream.shutdown(Shutdown::Both);
                    })) {
                        Ok(worker) => workers.push(worker),
                        Err(_) => {
                            let rejected = cleanup.lock().ok().and_then(|mut all| all.remove(&id));
                            if let Some(mut rejected) = rejected {
                                let _ = write_response(
                                    &mut rejected,
                                    Response::error(503),
                                    Instant::now() + Duration::from_millis(20),
                                );
                                let _ = rejected.shutdown(Shutdown::Both);
                            }
                            if !spawn_logged {
                                eprintln!("archive availability (archive-worker): cannot start bounded connection worker");
                                spawn_logged = true;
                            }
                        }
                    }
                },
            );
            if let Ok(all) = active.lock() {
                for s in all.values() {
                    let _ = s.shutdown(Shutdown::Both);
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        };
        let thread = thread::Builder::new()
            .name("archive-accept".into())
            .spawn(serving_loop)?;
        Ok(Self {
            addr,
            stop,
            health,
            sockets,
            thread: Some(thread),
        })
    }
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    pub fn url(&self) -> String {
        format!("http://{}{}", self.addr, PREFIX)
    }
    pub fn health(&self) -> Result<()> {
        match self.health.lock() {
            Ok(h) => match h.as_ref() {
                Some(e) => Err(e.clone().into()),
                None => Ok(()),
            },
            Err(_) => Err(crate::error::integrity(
                0,
                "archive-server",
                "health lock poisoned",
            )),
        }
    }
}
impl Drop for ArchiveServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(all) = self.sockets.lock() {
            for s in all.values() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn transient(e: &std::io::Error) -> bool {
    if matches!(
        e.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::TimedOut
    ) {
        return true;
    }
    if e.raw_os_error().is_some_and(|code| {
        [
            libc::EMFILE,
            libc::ENFILE,
            libc::ENOBUFS,
            libc::ENOMEM,
            libc::EPROTO,
            libc::ENETDOWN,
            libc::ENOPROTOOPT,
            libc::EHOSTDOWN,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
            libc::ENETUNREACH,
            libc::ENETRESET,
        ]
        .contains(&code)
    }) {
        return true;
    }
    #[cfg(target_os = "linux")]
    if e.raw_os_error() == Some(libc::ENONET) {
        return true;
    }
    false
}

fn accept_loop(
    flag: &AtomicBool,
    health: &Health,
    mut accept: impl FnMut() -> std::io::Result<TcpStream>,
    mut handle: impl FnMut(TcpStream),
) {
    let mut logged = false;
    while !flag.load(Ordering::Acquire) {
        match accept() {
            Ok(s) => handle(s),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(e) if transient(&e) => {
                if !logged {
                    eprintln!("archive availability (archive-accept): transient accept failure");
                    logged = true;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                if let Ok(mut h) = health.lock() {
                    *h = Some(Error::Integrity {
                        height: 0,
                        class: "archive-server",
                        detail: "fatal accept failure".into(),
                    });
                }
                eprintln!("archive integrity (archive-server): fatal accept failure");
                break;
            }
        }
    }
}
fn remaining(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| anyhow::anyhow!("archive I/O deadline"))
}
fn write_response(stream: &mut TcpStream, response: Response, deadline: Instant) -> Result<()> {
    stream.set_nonblocking(false)?;
    let head=format!("HTTP/1.1 {} Archive\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",response.status,response.body.len());
    for bytes in [head.as_bytes(), response.body.as_slice()] {
        let mut offset = 0;
        while offset < bytes.len() {
            stream.set_write_timeout(Some(remaining(deadline)?))?;
            let end = (offset + 65536).min(bytes.len());
            let n = stream.write(&bytes[offset..end])?;
            ensure!(n > 0, "archive connection closed");
            offset += n;
        }
    }
    Ok(())
}
fn parse(raw: &[u8]) -> Result<(&str, &str)> {
    let text = std::str::from_utf8(raw)?;
    ensure!(text.ends_with("\r\n\r\n"), "request framing");
    ensure!(
        !text.replace("\r\n", "").contains(['\r', '\n']),
        "strict CRLF"
    );
    let mut lines = text.split("\r\n");
    let fields = lines.next().unwrap_or("").split(' ').collect::<Vec<_>>();
    ensure!(
        fields.len() == 3
            && !fields[0].is_empty()
            && fields[0]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            && !fields[1].is_empty()
            && fields[1].bytes().all(|b| b.is_ascii_graphic())
            && fields[2] == "HTTP/1.1",
        "request line"
    );
    let mut length = false;
    for line in lines.filter(|s| !s.is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("header colon"))?;
        ensure!(
            !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)),
            "header name"
        );
        ensure!(!value.bytes().any(|b| b < 32 && b != b'\t'), "header value");
        ensure!(
            !key.eq_ignore_ascii_case("transfer-encoding"),
            "transfer encoding"
        );
        if key.eq_ignore_ascii_case("content-length") {
            ensure!(!length && value.trim() == "0", "request body");
            length = true;
        }
    }
    Ok((fields[0], fields[1]))
}
fn handle(
    stream: &mut TcpStream,
    store: &Arc<Mutex<ArchiveStore>>,
    stop: &AtomicBool,
    health: &Health,
    logged: &Mutex<BTreeSet<String>>,
) -> Result<()> {
    stream.set_nonblocking(false)?;
    let start = Instant::now();
    let request_deadline = start + REQUEST_TIME;
    let mut raw = vec![];
    let mut part = [0; 4096];
    let response = loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let Ok(left) = remaining(request_deadline) else {
            break Response::error(408);
        };
        stream.set_read_timeout(Some(left))?;
        let n = match stream.read(&mut part) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                break Response::error(408)
            }
            Err(e) => return Err(e.into()),
        };
        raw.extend_from_slice(&part[..n]);
        if raw.len() > REQ_MAX {
            break Response::error(413);
        }
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            if raw.len() != end + 4 {
                break Response::error(400);
            }
            let Ok((method, path)) = parse(&raw) else {
                break Response::error(400);
            };
            if health.lock().map(|h| h.is_some()).unwrap_or(true) {
                break Response::typed(503, "archive-server");
            }
            let locked = loop {
                match store.try_lock() {
                    Ok(s) => break Some(s),
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        if let Ok(mut h) = health.lock() {
                            if h.is_none() {
                                *h = Some(Error::Integrity {
                                    height: 0,
                                    class: "archive-store",
                                    detail: "store mutex poisoned".into(),
                                });
                            }
                        }
                        if let Ok(mut seen) = logged.lock() {
                            if seen.insert("archive-store".into()) {
                                eprintln!(
                                    "archive integrity (archive-store): store mutex poisoned"
                                );
                            }
                        }
                        break None;
                    }
                    Err(_) => {
                        if stop.load(Ordering::Acquire) || Instant::now() >= request_deadline {
                            break None;
                        }
                        thread::sleep(Duration::from_millis(2))
                    }
                }
            };
            break match locked {
                Some(store) => dispatch(&store, method, path, logged, health),
                None => Response::typed(503, "archive-store"),
            };
        }
    };
    let deadline = Instant::now() + transfer_budget(response.body.len());
    write_response(stream, response, deadline)
}
fn number(s: &str) -> Option<u64> {
    if s == "0" || (!s.starts_with('0') && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) {
        s.parse().ok()
    } else {
        None
    }
}
fn key(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn dispatch(
    store: &ArchiveStore,
    method: &str,
    path: &str,
    logged: &Mutex<BTreeSet<String>>,
    health: &Health,
) -> Response {
    if method != "GET" {
        return Response::error(405);
    }
    let Some(route) = path.strip_prefix(PREFIX) else {
        return Response::error(404);
    };
    if route.contains(['%', '\\', '#']) || route.contains("..") {
        return Response::error(400);
    }
    // All request validation precedes access to the store. Store errors never become 400.
    let result = if route == "info" {
        store
            .check()
            .and_then(|_| Ok(Some(serde_json::to_vec(&store.info())?)))
    } else if let Some(k) = route.strip_prefix("l1/header/") {
        if !key(k) {
            return Response::error(400);
        }
        store.header(&format!("0x{k}"))
    } else if let Some(k) = route.strip_prefix("l1/receipts/") {
        if !key(k) {
            return Response::error(400);
        }
        store.receipts(&format!("0x{k}"))
    } else if let Some(obj) = route.strip_prefix("obj/") {
        let Some((class, k)) = obj.split_once('/') else {
            return Response::error(400);
        };
        if !matches!(class, "exp" | "tr" | "pkg" | "env" | "gate" | "vstmt") {
            return Response::error(404);
        }
        if !key(k) {
            return Response::error(400);
        }
        store.object(class, &format!("0x{k}"))
    } else if let Some(h) = route.strip_prefix("l2/system/") {
        let Some(h) = number(h) else {
            return Response::error(400);
        };
        store.system(h)
    } else if let Some(range) = route.strip_prefix("index/") {
        let (from, limit) = range.split_once("?limit=").unwrap_or((range, "1024"));
        let (Some(from), Some(limit)) = (number(from), number(limit)) else {
            return Response::error(400);
        };
        if limit == 0 || limit > crate::archive::SEGMENT as u64 {
            return Response::error(400);
        }
        store
            .index(from, limit as usize)
            .and_then(|v| Ok(Some(serde_json::to_vec(&v)?)))
    } else {
        return Response::error(404);
    };
    let error = match result {
        Ok(Some(body)) if body.len() <= crate::archive::OBJ_MAX => {
            return Response { status: 200, body }
        }
        Ok(Some(_)) => crate::error::integrity(0, "archive-response", "response exceeds OBJ_MAX"),
        Ok(None) => return Response::error(404),
        Err(e) => e,
    };
    if let Some(e @ Error::Integrity { class, .. }) = error.downcast_ref::<Error>() {
        if *class != "object-quarantine" {
            if let Ok(mut h) = health.lock() {
                if h.is_none() {
                    *h = Some(e.clone());
                }
            }
        }
    }
    let (status, class) = match error.downcast_ref::<Error>() {
        Some(Error::Availability { class, .. }) => (503, *class),
        Some(Error::Integrity { class, .. }) => (500, *class),
        _ => (500, "archive-store"),
    };
    if let Ok(mut seen) = logged.lock() {
        if seen.insert(class.to_owned()) {
            eprintln!("archive error ({class}): {error:#}")
        }
    }
    Response::typed(status, class)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn s2_accept_resumes_after_transient_errors() -> Result<()> {
        for e in [
            std::io::Error::from(std::io::ErrorKind::Interrupted),
            std::io::Error::from(std::io::ErrorKind::ConnectionAborted),
            std::io::Error::from_raw_os_error(libc::EMFILE),
            std::io::Error::from_raw_os_error(libc::ENFILE),
        ] {
            let flag = AtomicBool::new(false);
            let health = Health::default();
            let mut attempts = 0;
            let mut first = Some(e);
            accept_loop(
                &flag,
                &health,
                || {
                    attempts += 1;
                    if let Some(e) = first.take() {
                        Err(e)
                    } else {
                        flag.store(true, Ordering::Release);
                        Err(std::io::ErrorKind::WouldBlock.into())
                    }
                },
                |_| {},
            );
            assert_eq!(attempts, 2);
            assert!(health.lock().unwrap().is_none());
        }
        Ok(())
    }
    #[test]
    fn fatal_accept_sets_typed_health() {
        let flag = AtomicBool::new(false);
        let health = Health::default();
        accept_loop(
            &flag,
            &health,
            || Err(std::io::ErrorKind::PermissionDenied.into()),
            |_| panic!("no connection"),
        );
        assert!(matches!(
            health.lock().unwrap().as_ref(),
            Some(Error::Integrity {
                class: "archive-server",
                ..
            })
        ));
    }
}

#[cfg(test)]
mod fix2_tests {
    use super::*;
    fn store() -> Result<(crate::durability::PrivateTempDir, Arc<Mutex<ArchiveStore>>)> {
        use crate::{
            archive::{ArchiveIdentity, ArchiveProfile},
            journal::NetworkIdentity,
        };
        use boole_derivation::{attributes::hex_string, m2::Config};
        let dir = crate::durability::PrivateTempDir::new("http-fix2")?;
        let profile = ArchiveProfile {
            identity: ArchiveIdentity {
                network: NetworkIdentity::new(
                    &Config::devnet(),
                    None,
                    hex_string(&[1; 32]),
                    30_000_000,
                )?,
                l1_anchor_number: 0,
                l1_anchor_hash: hex_string(&[2; 32]),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        };
        let store = Arc::new(Mutex::new(ArchiveStore::open(dir.path(), profile, false)?));
        Ok((dir, store))
    }
    #[test]
    fn l3_poisoned_store_sets_typed_server_health() -> Result<()> {
        let (_dir, store) = store()?;
        let poisoned = store.clone();
        assert!(thread::spawn(move || {
            let _held = poisoned.lock().unwrap();
            panic!("deliberate own store poison");
        })
        .join()
        .is_err());
        let server = ArchiveServer::start(store, "127.0.0.1:0".parse()?)?;
        let mut client = TcpStream::connect(server.addr())?;
        client.set_read_timeout(Some(Duration::from_secs(3)))?;
        client.write_all(b"GET /boole/archive/v1/info HTTP/1.1\r\n\r\n")?;
        let mut response = vec![];
        client.read_to_end(&mut response)?;
        assert!(response.starts_with(b"HTTP/1.1 503"));
        let error = server
            .health()
            .expect_err("poisoned store leaves keepalive serving forever");
        assert!(matches!(
            error.downcast_ref::<Error>(),
            Some(Error::Integrity {
                class: "archive-store",
                ..
            })
        ));
        Ok(())
    }
    #[test]
    fn l2_pending_network_and_memory_accept_errors_retry() -> Result<()> {
        let codes = [
            libc::ENOBUFS,
            libc::ENOMEM,
            libc::EPROTO,
            libc::ENETDOWN,
            libc::ENOPROTOOPT,
            libc::EHOSTDOWN,
            libc::EHOSTUNREACH,
            libc::EOPNOTSUPP,
            libc::ENETUNREACH,
            libc::ENETRESET,
            libc::ECONNRESET,
            libc::ETIMEDOUT,
        ]
        .into_iter();
        #[cfg(target_os = "linux")]
        let codes = codes.chain([libc::ENONET]);
        for code in codes {
            let flag = AtomicBool::new(false);
            let health = Health::default();
            let mut attempts = 0;
            accept_loop(
                &flag,
                &health,
                || {
                    attempts += 1;
                    if attempts == 1 {
                        Err(std::io::Error::from_raw_os_error(code))
                    } else {
                        flag.store(true, Ordering::Release);
                        Err(std::io::ErrorKind::WouldBlock.into())
                    }
                },
                |_| {},
            );
            assert_eq!(
                attempts, 2,
                "pending accept error {code} stopped the listener"
            );
            assert!(health.lock().unwrap().is_none());
        }
        Ok(())
    }
    #[test]
    fn l1_worker_spawn_failure_refuses_then_recovers_without_descriptor_leak() -> Result<()> {
        use std::sync::atomic::AtomicUsize;
        let (_dir, store) = store()?;
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let spawn: SpawnWorker = Arc::new(move |work| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(std::io::Error::from_raw_os_error(libc::EAGAIN))
            } else {
                Ok(thread::spawn(work))
            }
        });
        let server = ArchiveServer::start_with_spawner(store, "127.0.0.1:0".parse()?, spawn)?;
        let request = || -> Result<Vec<u8>> {
            let mut client = TcpStream::connect(server.addr())?;
            client.set_read_timeout(Some(Duration::from_millis(300)))?;
            client.write_all(b"GET /boole/archive/v1/info HTTP/1.1\r\n\r\n")?;
            let mut bytes = vec![];
            let _ = client.read_to_end(&mut bytes);
            Ok(bytes)
        };
        assert!(
            request()?.starts_with(b"HTTP/1.1 503"),
            "worker spawn failure silently killed serving"
        );
        assert!(
            request()?.starts_with(b"HTTP/1.1 200"),
            "accept thread did not recover"
        );
        server.health()?;
        let deadline = Instant::now() + Duration::from_secs(1);
        while !server.sockets.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(server.sockets.lock().unwrap().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        Ok(())
    }
    #[test]
    fn m3_large_verified_body_gets_its_full_transfer_budget_after_dispatch() -> Result<()> {
        let (_dir, store) = store()?;
        let bytes = vec![7; 32 * 1024 * 1024];
        let address = crate::archive::sha(&bytes);
        let pin = boole_derivation::attributes::hex_bytes(&address, "pin")?
            .try_into()
            .unwrap();
        store.lock().unwrap().put_content("env", pin, &bytes)?;
        let server = ArchiveServer::start(store.clone(), "127.0.0.1:0".parse()?)?;
        let (ready, wait) = std::sync::mpsc::channel();
        let held = store.clone();
        let lock = thread::spawn(move || {
            let _guard = held.lock().unwrap();
            ready.send(()).unwrap();
            thread::sleep(Duration::from_millis(1300));
        });
        wait.recv()?;
        let mut client = TcpStream::connect(server.addr())?;
        client.set_read_timeout(Some(Duration::from_secs(3)))?;
        write!(
            client,
            "GET /boole/archive/v1/obj/env/{} HTTP/1.1\r\n\r\n",
            &address[2..]
        )?;
        let mut header = vec![];
        let mut byte = [0; 1];
        while !header.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte)?;
            header.push(byte[0]);
        }
        assert!(header.starts_with(b"HTTP/1.1 200"));
        let mut chunk = vec![0; 4 * 1024 * 1024];
        let mut received = 0;
        let body_started = Instant::now();
        while received < bytes.len() {
            let n = chunk.len().min(bytes.len() - received);
            client.read_exact(&mut chunk[..n]).map_err(|e| {
                anyhow::anyhow!(
                    "body stopped after {received}/{} bytes at {:?}: {e}",
                    bytes.len(),
                    body_started.elapsed()
                )
            })?;
            assert!(chunk[..n].iter().all(|b| *b == 7));
            received += n;
            if received < bytes.len() {
                thread::sleep(Duration::from_millis(450));
            }
        }
        lock.join().unwrap();
        assert_eq!(received, bytes.len());
        println!(
            "body_bytes={received} body_seconds={:.3}",
            body_started.elapsed().as_secs_f64()
        );
        Ok(())
    }
    #[test]
    #[allow(unsafe_code)]
    fn h1_fin_and_rst_return_registry_and_process_descriptors() -> Result<()> {
        use std::os::fd::AsRawFd;
        let (_dir, store) = store()?;
        let server = ArchiveServer::start(store, "127.0.0.1:0".parse()?)?;
        let descriptors = || {
            std::fs::read_dir(if cfg!(target_os = "linux") {
                "/proc/self/fd"
            } else {
                "/dev/fd"
            })
            .unwrap()
            .count()
        };
        let baseline = descriptors();
        for reset in [false, true] {
            for _ in 0..64 {
                let mut client = TcpStream::connect(server.addr())?;
                client.set_read_timeout(Some(Duration::from_secs(4)))?;
                client.write_all(b"GET /boole/archive/v1/info HTTP/1.1\r\n\r\n")?;
                if reset {
                    let linger = libc::linger {
                        l_onoff: 1,
                        l_linger: 0,
                    };
                    assert_eq!(
                        unsafe {
                            libc::setsockopt(
                                client.as_raw_fd(),
                                libc::SOL_SOCKET,
                                libc::SO_LINGER,
                                (&linger as *const libc::linger).cast(),
                                std::mem::size_of_val(&linger) as _,
                            )
                        },
                        0
                    );
                } else {
                    let mut bytes = vec![];
                    client.read_to_end(&mut bytes)?;
                    assert!(bytes.starts_with(b"HTTP/1.1 200"));
                }
                drop(client);
                // Ensure this request's worker cleanup, not just its response, completed.
                thread::sleep(Duration::from_millis(5));
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while !server.sockets.lock().unwrap().is_empty() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                server.sockets.lock().unwrap().is_empty(),
                "closed connections retained registry descriptors (RST={reset})"
            );
            assert!(
                descriptors() <= baseline + 2,
                "process descriptor count grew after close"
            );
        }
        Ok(())
    }
}
