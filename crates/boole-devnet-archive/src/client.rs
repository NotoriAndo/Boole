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
    /// Optional diagnostic transport. Implementations must honor this absolute deadline.
    /// The default deliberately does not invoke a possibly blocking required-data fetch.
    fn observe(
        &mut self,
        _source: &str,
        _route: &str,
        _limit: usize,
        _deadline: Instant,
    ) -> Result<Vec<u8>> {
        anyhow::bail!("bounded observation unsupported")
    }
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
impl HttpFetch {
    fn read(source: &str, route: &str, limit: usize, budget: Option<Instant>) -> Result<Vec<u8>> {
        ensure!(
            limit <= OBJ_MAX && route.len() < REQ_MAX - 256,
            "archive request/response cap"
        );
        let start = Instant::now();
        let header_deadline = budget.map_or(start + Duration::from_secs(2), |b| {
            b.min(start + Duration::from_secs(2))
        });
        let remaining = |deadline: Instant| {
            deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
                .context("archive response deadline")
        };
        let (addr, prefix) = endpoint(source)?;
        let mut stream = TcpStream::connect_timeout(&addr, remaining(header_deadline)?)?;
        let request =
            format!("GET {prefix}{route} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        let mut sent = 0;
        while sent < request.len() {
            stream.set_write_timeout(Some(remaining(header_deadline)?))?;
            let n = stream.write(&request.as_bytes()[sent..])?;
            ensure!(n > 0, "archive closed request");
            sent += n;
        }
        let mut head = vec![];
        let mut byte = [0; 1];
        while !head.ends_with(b"\r\n\r\n") {
            ensure!(head.len() < REQ_MAX, "archive HTTP header cap");
            stream.set_read_timeout(Some(remaining(header_deadline)?))?;
            stream.read_exact(&mut byte)?;
            head.push(byte[0]);
        }
        let text = std::str::from_utf8(&head)?;
        ensure!(
            !text.replace("\r\n", "").contains(['\r', '\n']),
            "archive response framing"
        );
        ensure!(
            text.split("\r\n")
                .next()
                .is_some_and(|l| l.split(' ').nth(1) == Some("200")),
            "archive unavailable response"
        );
        let mut length = None;
        for line in text.split("\r\n").skip(1).filter(|l| !l.is_empty()) {
            let (k, v) = line.split_once(':').context("archive response header")?;
            ensure!(k.trim() == k, "archive response header name");
            ensure!(
                !k.eq_ignore_ascii_case("transfer-encoding"),
                "unsupported archive transfer encoding"
            );
            if k.eq_ignore_ascii_case("content-length") {
                ensure!(length.is_none(), "duplicate content length");
                length = Some(v.trim().parse::<usize>()?);
            }
        }
        let len = length.context("archive content length missing")?;
        ensure!(
            len > 0 && len <= limit,
            "archive response byte cap or empty object"
        );
        // Total time includes connect, headers and every body chunk. 8 MiB/s is the
        // minimum supported loopback throughput, with two seconds of fixed slack.
        let deadline = start + crate::http::transfer_budget(len);
        let deadline = budget.map_or(deadline, |b| b.min(deadline));
        let mut bytes = vec![0; len];
        let mut at = 0;
        while at < len {
            stream.set_read_timeout(Some(remaining(deadline)?))?;
            let end = (at + 65536).min(len);
            let n = stream.read(&mut bytes[at..end])?;
            ensure!(n > 0, "archive response truncated");
            at += n;
        }
        remaining(deadline)?;
        Ok(bytes)
    }
}
impl Fetch for HttpFetch {
    fn get(&mut self, source: &str, route: &str, limit: usize) -> Result<Vec<u8>> {
        Self::read(source, route, limit, None)
    }
    fn observe(
        &mut self,
        source: &str,
        route: &str,
        limit: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>> {
        Self::read(source, route, limit, Some(deadline))
    }
}
pub struct ArchiveClient {
    pub sources: Vec<String>,
    fetch: Box<dyn Fetch>,
    pub discarded: u64,
    pub attempts: Vec<u64>,
    pub failures: Vec<u64>,
    coverage: Vec<Option<u64>>,
    coverage_checked: Vec<Option<Instant>>,
    observation_retry_at: Vec<Option<Instant>>,
    next_observer: usize,
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
            sources: sources.clone(),
            fetch,
            discarded: 0,
            attempts,
            coverage: vec![None; sources.len()],
            coverage_checked: vec![None; sources.len()],
            observation_retry_at: vec![None; sources.len()],
            next_observer: 0,
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
                        receipts: Some(receipt_list(b)?),
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
    /// Called only after the corresponding own journal row has been fsynced.
    pub fn appended(&self, height: u64) -> Result<()> {
        let key = self
            .hashes
            .get(&height)
            .context("downloaded header height")?;
        self.cache
            .retire(key, |b| crate::archive::verify_header(key, b))
    }
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
                row.receipts = Some(receipt_list(b)?);
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
                if *n > tip {
                    return Err(crate::error::integrity(
                        tip,
                        "l1-anchor",
                        "own finalized anchor moved behind own journal",
                    ));
                }
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
                return Err(crate::error::integrity(
                    tip,
                    "l1-anchor",
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
    fn observation_failed(&mut self, index: usize) {
        self.discarded = self.discarded.saturating_add(1);
        self.failures[index] = self.failures[index].saturating_add(1);
        self.observation_retry_at[index] = Some(Instant::now() + Duration::from_secs(30));
    }
    pub fn tripwire(
        &mut self,
        height: u64,
        l1_hash: &str,
        l2_hash: &str,
        root: &str,
    ) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_millis(50);
        let mut alerts = vec![];
        let start = self.next_observer;
        self.next_observer = (start + 1) % self.sources.len();
        for offset in 0..self.sources.len() {
            let i = (start + offset) % self.sources.len();
            if Instant::now() >= deadline {
                break;
            }
            if self.observation_retry_at[i].is_some_and(|at| Instant::now() < at) {
                continue;
            }
            if self.coverage[i].is_none_or(|through| through < height)
                && self.coverage_checked[i].is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            {
                self.coverage_checked[i] = Some(Instant::now());
                self.attempts[i] = self.attempts[i].saturating_add(1);
                match self
                    .fetch
                    .observe(&self.sources[i], "info", REQ_MAX, deadline)
                {
                    Ok(raw) => match serde_json::from_slice::<serde_json::Value>(&raw) {
                        Ok(v) => self.coverage[i] = v["covered_through"].as_u64(),
                        Err(_) => {
                            self.observation_failed(i);
                            continue;
                        }
                    },
                    Err(_) => {
                        self.observation_failed(i);
                        continue;
                    }
                }
            }
            if self.coverage[i].is_none_or(|through| height > through) || Instant::now() >= deadline
            {
                continue;
            }
            self.attempts[i] = self.attempts[i].saturating_add(1);
            let result = self.fetch.observe(
                &self.sources[i],
                &format!("index/{height}?limit=1"),
                1_048_576,
                deadline,
            );
            if let Ok(raw) = result {
                if raw.len() <= 1_048_576 {
                    if let Ok(rows) = serde_json::from_slice::<Vec<crate::archive::IndexRow>>(&raw)
                    {
                        if rows.len() == 1
                            && rows[0].validate().is_ok()
                            && rows[0].l1_number == height
                            && rows[0].l1_hash == l1_hash
                        {
                            if rows[0].l2_hash != l2_hash || rows[0].root != root {
                                alerts.push(format!("archive index alert at height {height}: verified inconsistency; local fold wins"));
                            }
                            continue;
                        }
                    }
                }
            }
            // Absent/unavailable/unparseable data is not evidence of a conflicting index.
            self.observation_failed(i);
        }
        alerts
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
    /// Package bytes must be bound to the revealed problem's frozen POOL root.
    pub fn package(
        &mut self,
        state: &boole_derivation::m2::State,
        config: &boole_derivation::m2::Config,
        problem: boole_derivation::m2::Hash,
        pin: boole_derivation::m2::Hash,
        proof: &crate::derived::PackageProof,
    ) -> Result<Vec<u8>> {
        let address = hex_string(&pin);
        self.fetch_verified(
            &format!("obj/pkg/{}", &address[2..]),
            crate::derived::content_limit("pkg")?,
            state.height(),
            |bytes| {
                crate::derived::verify_package(state, config, problem, pin, proof, bytes)?;
                Ok(bytes.to_vec())
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
            matches!(class, "env" | "gate" | "vstmt"),
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

/// Decode the receipt list at the archive byte boundary.
pub fn receipt_list(bytes: &[u8]) -> Result<Vec<String>> {
    use serde::de::{DeserializeSeed, SeqAccess, Visitor};
    struct AtCap;
    impl<'de> DeserializeSeed<'de> for AtCap {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            _: D,
        ) -> std::result::Result<(), D::Error> {
            Err(serde::de::Error::custom("receipt count cap"))
        }
    }
    struct Bounded;
    impl<'de> Visitor<'de> for Bounded {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("at most 65536 receipt strings")
        }
        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut rows = Vec::new();
            while rows.len() < 65536 {
                match seq.next_element::<String>()? {
                    Some(row) => rows.push(row),
                    None => return Ok(rows),
                }
            }
            seq.next_element_seed(AtCap)?;
            Ok(rows)
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let rows = serde::Deserializer::deserialize_seq(&mut decoder, Bounded)?;
    decoder.end()?;
    Ok(rows)
}
