use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Append-only NDJSON ledger of accepted `(sessionPk, nonce)` pairs for
/// session-bound `/submit` envelopes. The store keeps a `HashSet` of
/// `"<sessionPk>:<nonce>"` keys in memory so the gate can answer dedup
/// queries without re-reading the file. Recovery replays the ledger from
/// disk so restart-replay of a previously-burned nonce still rejects with
/// `nonce_replayed`.
///
/// Mirrors `FileSessionStore`'s recover/append/apply pattern so deterministic
/// boot stays consistent across stores.
#[derive(Debug, Default)]
pub struct FileNonceLedger {
    seen: HashSet<String>,
}

/// One serialized line in the submit-nonce NDJSON file. Tagged so future
/// event kinds (e.g. nonce expiry) can be added without breaking older
/// ledgers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NonceEvent {
    Burn {
        #[serde(rename = "sessionPk")]
        session_pk: String,
        nonce: String,
    },
}

impl FileNonceLedger {
    /// Build an in-memory ledger by replaying the NDJSON file at `path`.
    /// Returns an empty ledger if the file does not yet exist.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: NonceEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("nonceLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            ledger.apply(event);
        }
        Ok(ledger)
    }

    /// True if the pair has been burned previously.
    pub fn contains(&self, session_pk: &str, nonce: &str) -> bool {
        self.seen.contains(&Self::key(session_pk, nonce))
    }

    /// Persist and apply a burn event. Returns `Ok(false)` if the pair was
    /// already burned (caller must reject with `nonce_replayed`); returns
    /// `Ok(true)` after a successful append.
    pub fn append_burn(
        &mut self,
        path: impl AsRef<Path>,
        session_pk: &str,
        nonce: &str,
    ) -> anyhow::Result<bool> {
        if self.contains(session_pk, nonce) {
            return Ok(false);
        }
        let event = NonceEvent::Burn {
            session_pk: session_pk.to_string(),
            nonce: nonce.to_string(),
        };
        Self::append(path, &event)?;
        self.apply(event);
        Ok(true)
    }

    fn append(path: impl AsRef<Path>, event: &NonceEvent) -> anyhow::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        Ok(())
    }

    fn apply(&mut self, event: NonceEvent) {
        match event {
            NonceEvent::Burn { session_pk, nonce } => {
                self.seen.insert(Self::key(&session_pk, &nonce));
            }
        }
    }

    fn key(session_pk: &str, nonce: &str) -> String {
        format!("{session_pk}:{nonce}")
    }

    pub fn size(&self) -> usize {
        self.seen.len()
    }
}
