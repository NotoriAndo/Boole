//! Archive data is untrusted availability. The caller supplies its own L1 anchor.
use crate::{
    archive::{OBJ_MAX, REQ_MAX},
    input::InputRow,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::attributes::hex_string;
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};
pub trait Fetch: Send {
    fn get(&mut self, source: &str, route: &str, limit: usize) -> Result<Vec<u8>>;
    fn backoff(&mut self, millis: u64) {
        std::thread::sleep(Duration::from_millis(millis));
    }
}
pub struct HttpFetch;
fn endpoint(source: &str) -> Result<(SocketAddr, String)> {
    let rest = source
        .strip_prefix("http://")
        .context("S22: archive requires a pinned loopback HTTP URL")?;
    let (host, path) = rest
        .split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((rest, "/boole/archive/v1/".into()));
    let addr: SocketAddr = host.parse()?;
    ensure!(addr.ip().is_loopback(), "S22: archive URL must be loopback");
    ensure!(
        matches!(path.as_str(), "/boole/archive/v1/" | "/boole/archive/v1"),
        "S22: archive URL prefix"
    );
    Ok((addr, "/boole/archive/v1/".into()))
}
impl Fetch for HttpFetch {
    fn get(&mut self, source: &str, route: &str, limit: usize) -> Result<Vec<u8>> {
        ensure!(
            limit <= OBJ_MAX && route.len() < REQ_MAX - 256,
            "archive request/response cap"
        );
        let (addr, prefix) = endpoint(source)?;
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        write!(
            stream,
            "GET {prefix}{route} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut head = vec![];
        let mut byte = [0; 1];
        while !head.ends_with(b"\r\n\r\n") {
            ensure!(Instant::now() < deadline, "archive response deadline");
            ensure!(head.len() < REQ_MAX, "archive HTTP header cap");
            stream.read_exact(&mut byte)?;
            head.push(byte[0]);
        }
        let text = std::str::from_utf8(&head)?;
        ensure!(
            text.lines()
                .next()
                .is_some_and(|l| l.split_whitespace().nth(1) == Some("200")),
            "archive unavailable response"
        );
        let mut len = None;
        for line in text.lines().skip(1) {
            if let Some((k, v)) = line.split_once(':') {
                ensure!(
                    !k.eq_ignore_ascii_case("transfer-encoding"),
                    "unsupported archive transfer encoding"
                );
                if k.eq_ignore_ascii_case("content-length") {
                    ensure!(len.is_none(), "duplicate content length");
                    len = Some(v.trim().parse::<usize>()?);
                }
            }
        }
        let len = len.context("archive content length missing")?;
        ensure!(
            len > 0 && len <= limit,
            "archive response byte cap or empty object"
        );
        let mut bytes = vec![0; len];
        stream.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}
pub struct ArchiveClient {
    pub sources: Vec<String>,
    fetch: Box<dyn Fetch>,
    pub discarded: u64,
    pub attempts: Vec<u64>,
    pub failures: Vec<u64>,
}
impl ArchiveClient {
    pub fn http(sources: Vec<String>) -> Result<Self> {
        Self::new(sources, Box::new(HttpFetch))
    }
    pub fn new(sources: Vec<String>, fetch: Box<dyn Fetch>) -> Result<Self> {
        ensure!(
            !sources.is_empty() && sources.len() <= 16,
            "S22: at least one pinned archive URL is required"
        );
        for s in &sources {
            endpoint(s)?;
        }
        let attempts = vec![0; sources.len()];
        let failures = vec![0; sources.len()];
        Ok(Self {
            sources,
            fetch,
            discarded: 0,
            attempts,
            failures,
        })
    }
    pub fn fetch_verified<T>(
        &mut self,
        route: &str,
        limit: usize,
        height: u64,
        mut verify: impl FnMut(&[u8]) -> Result<T>,
    ) -> Result<T> {
        for attempt in 0..3 {
            if attempt > 0 {
                self.fetch.backoff(if attempt == 1 { 100 } else { 300 });
            }
            for i in 0..self.sources.len() {
                self.attempts[i] = self.attempts[i].saturating_add(1);
                let result = self
                    .fetch
                    .get(&self.sources[i], route, limit)
                    .and_then(|b| {
                        ensure!(b.len() <= limit && !b.is_empty(), "archive response cap");
                        verify(&b)
                    });
                match result {
                    Ok(value) => return Ok(value),
                    Err(_) => {
                        self.discarded = self.discarded.saturating_add(1);
                        self.failures[i] = self.failures[i].saturating_add(1)
                    }
                }
            }
        }
        Err(crate::error::availability(height,"archive-fetch",format!("all pinned sources exhausted FETCH; source failures={:?}; retry after availability returns",self.failures)))
    }
    pub fn header(&mut self, key: &str, height: u64) -> Result<Vec<u8>> {
        let route = format!(
            "l1/header/{}",
            key.strip_prefix("0x").context("header address")?
        );
        self.fetch_verified(&route, 4096, height, |raw| {
            crate::archive::verify_header(key, raw)?;
            let row = InputRow {
                schema_version: 1,
                block_hash: key.into(),
                header_rlp: hex_string(raw),
                receipts: None,
                transactions: None,
            };
            ensure!(row.block()?.number == height, "archive header height");
            Ok(raw.to_vec())
        })
    }
}
impl ArchiveClient {
    pub fn row(&mut self, key: &str, height: u64) -> Result<InputRow> {
        let raw = self.header(key, height)?;
        if crate::input::header_needs_receipts(&raw)? {
            self.fetch_verified(
                &format!("l1/receipts/{}", &key[2..]),
                OBJ_MAX,
                height,
                |b| {
                    let row = InputRow {
                        schema_version: 1,
                        block_hash: key.into(),
                        header_rlp: hex_string(&raw),
                        receipts: Some(serde_json::from_slice(b)?),
                        transactions: None,
                    };
                    row.verify()?;
                    Ok(row)
                },
            )
        } else {
            let row = InputRow {
                schema_version: 1,
                block_hash: key.into(),
                header_rlp: hex_string(&raw),
                receipts: None,
                transactions: None,
            };
            row.verify()?;
            Ok(row)
        }
    }
}

/// Disk-backed descending header verification; receipts stream forward into the
/// input journal only after the whole header path has linked to the own anchor.
pub struct HeaderDownload {
    pub tip: u64,
    pub genesis_hash: Option<String>,
    hashes: std::collections::BTreeMap<u64, String>,
    cache: crate::objects::Objects,
}
impl HeaderDownload {
    pub fn row(&self, client: &mut ArchiveClient, height: u64) -> Result<InputRow> {
        let key = self
            .hashes
            .get(&height)
            .context("downloaded header height")?;
        let raw = self
            .cache
            .read(key, |b| crate::archive::verify_header(key, b))?
            .context("downloaded header unavailable")?;
        client.row_from_header(key, height, &raw)
    }
}
impl ArchiveClient {
    pub fn row_from_header(&mut self, key: &str, height: u64, raw: &[u8]) -> Result<InputRow> {
        crate::archive::verify_header(key, raw)?;
        let empty = InputRow {
            schema_version: 1,
            block_hash: key.into(),
            header_rlp: hex_string(raw),
            receipts: None,
            transactions: None,
        };
        ensure!(empty.block()?.number == height, "archive header height");
        if !crate::input::header_needs_receipts(raw)? {
            empty.verify()?;
            return Ok(empty);
        }
        self.fetch_verified(
            &format!("l1/receipts/{}", &key[2..]),
            OBJ_MAX,
            height,
            |b| {
                let mut row = empty.clone();
                row.receipts = Some(serde_json::from_slice(b)?);
                row.verify()?;
                Ok(row)
            },
        )
    }
    pub fn prepare(
        &mut self,
        tip: u64,
        tip_hash: &str,
        floor: Option<(u64, String)>,
        root: &std::path::Path,
        trusted_header: Option<&[u8]>,
    ) -> Result<HeaderDownload> {
        let cache = crate::objects::Objects::open(root)?;
        if let Some(raw) = trusted_header {
            cache.put(tip_hash, raw, |b| {
                crate::archive::verify_header(tip_hash, b)
            })?;
        }
        let start = match &floor {
            Some((n, _)) => {
                ensure!(*n <= tip, "archive anchor moved backwards");
                n.checked_add(1).context("archive height overflow")?
            }
            None => 0,
        };
        let mut hashes = std::collections::BTreeMap::new();
        let mut cursor = tip_hash.to_owned();
        for h in (start..=tip).rev() {
            let cached = cache.read(&cursor, |b| crate::archive::verify_header(&cursor, b));
            let raw = match cached {
                Ok(Some(bytes)) => bytes,
                _ => {
                    let bytes = self.header(&cursor, h)?;
                    cache.put(&cursor, &bytes, |b| {
                        crate::archive::verify_header(&cursor, b)
                    })?;
                    bytes
                }
            };
            let header = InputRow {
                schema_version: 1,
                block_hash: cursor.clone(),
                header_rlp: hex_string(&raw),
                receipts: None,
                transactions: None,
            }
            .block()?;
            ensure!(header.number == h, "archive descending header height");
            hashes.insert(h, cursor);
            cursor = header.parent_hash;
        }
        if let Some((_, hash)) = floor {
            if cursor != hash {
                return Err(crate::error::availability(
                    tip,
                    "archive-anchor",
                    "header path does not link to own manifest anchor",
                ));
            }
        } else {
            ensure!(
                cursor == format!("0x{}", "00".repeat(32)),
                "archive genesis parent"
            );
        }
        let genesis_hash = hashes.get(&0).cloned();
        Ok(HeaderDownload {
            tip,
            genesis_hash,
            hashes,
            cache,
        })
    }
    pub fn identity(
        &mut self,
        network: &crate::journal::NetworkIdentity,
        genesis_hash: &str,
    ) -> Result<()> {
        self.fetch_verified("info", REQ_MAX, 0, |b| {
            let v: serde_json::Value = serde_json::from_slice(b)?;
            let identity: crate::archive::ArchiveIdentity =
                serde_json::from_value(v["identity"].clone())?;
            ensure!(
                identity.network == *network
                    && identity.l1_anchor_number == 0
                    && identity.l1_anchor_hash == genesis_hash,
                "archive identity differs from own network/verified anchor"
            );
            Ok(())
        })
    }
    pub fn tripwire(
        &mut self,
        height: u64,
        l1_hash: &str,
        l2_hash: &str,
        root: &str,
    ) -> Vec<String> {
        let before = self.discarded;
        let result =
            self.fetch_verified(&format!("index/{height}?limit=1"), 1_048_576, height, |b| {
                let rows: Vec<crate::archive::IndexRow> = serde_json::from_slice(b)?;
                ensure!(rows.len() == 1, "archive index missing height");
                crate::derived::verify_index(&rows[0], height, l1_hash, l2_hash, root)
            });
        if result.is_err() || self.discarded > before {
            vec![format!("archive index alert at height {height}: inconsistent/unavailable sources; local fold wins")]
        } else {
            vec![]
        }
    }
}

impl ArchiveClient {
    pub fn export(
        &mut self,
        state: &boole_derivation::m2::State,
        key: boole_derivation::m2::Key,
    ) -> Result<Vec<u8>> {
        let open = state
            .candidates
            .get(&key)
            .and_then(|c| c.live())
            .and_then(|c| c.open.as_ref())
            .context("frozen export OPEN")?;
        let address = hex_string(&open.export_digest);
        self.fetch_verified(
            &format!("obj/exp/{}", &address[2..]),
            crate::derived::content_limit("exp")?,
            state.height(),
            |b| {
                crate::derived::verify_export(state, key, &address, b)?;
                Ok(b.to_vec())
            },
        )
    }
    pub fn transport(
        &mut self,
        state: &boole_derivation::m2::State,
        key: boole_derivation::m2::Key,
    ) -> Result<Vec<u8>> {
        let address = hex_string(&key.1);
        self.fetch_verified(
            &format!("obj/tr/{}", &address[2..]),
            crate::derived::content_limit("tr")?,
            state.height(),
            |b| {
                crate::derived::verify_transport(state, key, &address, b)?;
                Ok(b.to_vec())
            },
        )
    }
    pub fn content(
        &mut self,
        class: &str,
        pin: boole_derivation::m2::Hash,
        height: u64,
    ) -> Result<Vec<u8>> {
        ensure!(
            matches!(class, "env" | "gate" | "pkg" | "vstmt"),
            "public pinned content class"
        );
        let address = hex_string(&pin);
        self.fetch_verified(
            &format!("obj/{class}/{}", &address[2..]),
            crate::derived::content_limit(class)?,
            height,
            |b| {
                crate::derived::verify_content(class, &address, b, pin)?;
                Ok(b.to_vec())
            },
        )
    }
    pub fn system(&mut self, height: u64, expected: &[String]) -> Result<Vec<String>> {
        self.fetch_verified(&format!("l2/system/{height}"), OBJ_MAX, height, |b| {
            let system: Vec<String> = serde_json::from_slice(b)?;
            ensure!(
                system == expected,
                "archive system differs from locally derived exact prefix"
            );
            Ok(system)
        })
    }
}
