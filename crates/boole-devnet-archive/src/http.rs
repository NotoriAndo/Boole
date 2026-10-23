//! Bounded loopback HTTP/1.1 serving; no write or segment route.
use crate::archive::{ArchiveStore, REQ_MAX};
use anyhow::{ensure, Result};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub const PREFIX: &str = "/boole/archive/v1/";
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
}
pub struct ArchiveServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
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
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = handle(&mut stream, &store);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            addr,
            stop,
            thread: Some(thread),
        })
    }
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    pub fn url(&self) -> String {
        format!("http://{}{}", self.addr, PREFIX)
    }
}
impl Drop for ArchiveServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn handle(stream: &mut TcpStream, store: &Arc<Mutex<ArchiveStore>>) -> Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut raw = vec![];
    let mut part = [0; 4096];
    let response = loop {
        if Instant::now() >= deadline {
            break Response::error(408);
        }
        let n = stream.read(&mut part)?;
        if n == 0 {
            return Ok(());
        }
        raw.extend_from_slice(&part[..n]);
        if raw.len() > REQ_MAX {
            break Response::error(413);
        }
        if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            if raw.len() != end + 4 {
                break Response::error(400);
            }
            let text = match std::str::from_utf8(&raw) {
                Ok(t) => t,
                Err(_) => break Response::error(400),
            };
            let fields = text
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>();
            if fields.len() != 3 || fields[2] != "HTTP/1.1" {
                break Response::error(400);
            }
            let mut content_length = None;
            let mut bad = false;
            for line in text.split("\r\n").skip(1).filter(|l| !l.is_empty()) {
                let Some((key, value)) = line.split_once(':') else {
                    bad = true;
                    break;
                };
                if key.eq_ignore_ascii_case("transfer-encoding") {
                    bad = true;
                    break;
                }
                if key.eq_ignore_ascii_case("content-length") {
                    if content_length.is_some() {
                        bad = true;
                        break;
                    }
                    match value.trim().parse::<usize>() {
                        Ok(0) => content_length = Some(0),
                        _ => {
                            bad = true;
                            break;
                        }
                    }
                }
            }
            if bad {
                break Response::error(400);
            }
            let locked = store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive writer poisoned"))?;
            break dispatch(&locked, fields[0], fields[1]);
        }
    };
    write!(stream,"HTTP/1.1 {} Archive\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",response.status,response.body.len())?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}
fn dispatch(store: &ArchiveStore, method: &str, path: &str) -> Response {
    if method != "GET" {
        return Response::error(405);
    }
    let Some(route) = path.strip_prefix(PREFIX) else {
        return Response::error(404);
    };
    if route.contains(['%', '\\', '#']) || route.contains("..") {
        return Response::error(400);
    }
    let result = (|| -> Result<Option<Vec<u8>>> {
        if route == "info" {
            store.check()?;
            return Ok(Some(serde_json::to_vec(&store.info())?));
        }
        if let Some(key) = route.strip_prefix("l1/header/") {
            return store.header(&format!("0x{key}"));
        }
        if let Some(key) = route.strip_prefix("l1/receipts/") {
            return store.receipts(&format!("0x{key}"));
        }
        if let Some(object) = route.strip_prefix("obj/") {
            if let Some((class, key)) = object.split_once('/') {
                return store.object(class, &format!("0x{key}"));
            }
            return Ok(None);
        }
        if let Some(height) = route.strip_prefix("l2/system/") {
            return store.system(height.parse()?);
        }
        if let Some(range) = route.strip_prefix("index/") {
            let (from, limit) = match range.split_once("?limit=") {
                Some((from, limit)) => (from.parse()?, limit.parse()?),
                None => (range.parse()?, crate::archive::SEGMENT),
            };
            return Ok(Some(serde_json::to_vec(&store.index(from, limit)?)?));
        }
        Ok(None)
    })();
    match result {
        Ok(Some(body)) if body.len() <= crate::archive::OBJ_MAX => Response { status: 200, body },
        Ok(None) => Response::error(404),
        _ => Response::error(400),
    }
}
