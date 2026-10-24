//! Bounded loopback HTTP/1.1 serving; no write or segment route.
use crate::{
    archive::{ArchiveStore, REQ_MAX},
    error::Error,
};
use anyhow::{ensure, Result};
use std::{
    collections::BTreeSet,
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
const TOTAL_TIME: Duration = Duration::from_secs(3);
type Health = Arc<Mutex<Option<Error>>>;
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
    sockets: Arc<Mutex<Vec<TcpStream>>>,
    thread: Option<JoinHandle<()>>,
}
impl ArchiveServer {
    pub fn start(store: Arc<Mutex<ArchiveStore>>, bind: SocketAddr) -> Result<Self> {
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
        let sockets: Arc<Mutex<Vec<TcpStream>>> = Default::default();
        let active = sockets.clone();
        let thread = thread::spawn(move || {
            let mut workers: Vec<JoinHandle<()>> = vec![];
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
                    let id = stream.peer_addr().ok();
                    if let Ok(clone) = stream.try_clone() {
                        if let Ok(mut all) = active.lock() {
                            all.push(clone)
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
                    workers.push(thread::spawn(move || {
                        let _ = handle(&mut stream, &store, &flag, &health, &logged);
                        let _ = stream.shutdown(Shutdown::Both);
                        if let Ok(mut all) = active.lock() {
                            all.retain(|s| s.peer_addr().ok() != id)
                        }
                    }));
                },
            );
            if let Ok(all) = active.lock() {
                for s in all.iter() {
                    let _ = s.shutdown(Shutdown::Both);
                }
            }
            for worker in workers {
                let _ = worker.join();
            }
        });
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
            for s in all.iter() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn transient(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::Interrupted | std::io::ErrorKind::ConnectionAborted
    ) || matches!(e.raw_os_error(), Some(libc::EMFILE) | Some(libc::ENFILE))
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
    let total = start + TOTAL_TIME;
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
                    Err(std::sync::TryLockError::Poisoned(_)) => break None,
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
    write_response(stream, response, total)
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
