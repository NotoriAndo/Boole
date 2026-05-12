use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::SessionState;
use serde::{Deserialize, Serialize};

/// Append-only NDJSON ledger of session registry events. The store keeps a
/// `BTreeMap<sessionPk, SessionState>` in memory so the node can authorize
/// `/submit` requests without re-reading the file. Recovery replays the
/// ledger from disk; each line is one event.
///
/// Mirrors the NDJSON recovery pattern used by `FileRewardLedger` and the
/// bounty event ledger so deterministic boot is consistent across stores.
#[derive(Debug, Default)]
pub struct FileSessionStore {
    sessions: BTreeMap<String, SessionState>,
}

/// One serialized line in the session-store NDJSON file. The on-disk shape
/// is tagged so future event kinds (e.g. policy rotation) can be added
/// without breaking older ledgers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SessionEvent {
    Register {
        session: SessionState,
    },
    Revoke {
        #[serde(rename = "sessionPk")]
        session_pk: String,
        height: u64,
    },
}

impl FileSessionStore {
    /// Build an in-memory store by replaying the NDJSON ledger at `path`.
    /// Returns an empty store if the file does not yet exist.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        let mut store = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: SessionEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("sessionStore: line {} invalid JSON: {}", i + 1, err)
            })?;
            store.apply(event)?;
        }
        Ok(store)
    }

    /// Validate, persist, and apply a `register` event. Refuses to register
    /// a `sessionPk` that already has a non-revoked entry — the agent wallet
    /// plan's session-state model is keyed by `sessionPk`, so re-registering
    /// the same key would overwrite an active policy.
    pub fn append_register(
        &mut self,
        path: impl AsRef<Path>,
        session: &SessionState,
        current_height: u64,
    ) -> anyhow::Result<()> {
        if let Some(existing) = self.sessions.get(&session.session_pk) {
            if !existing.revoked {
                anyhow::bail!(
                    "sessionStore: duplicate active session for sessionPk {}",
                    session.session_pk
                );
            }
        }
        session.validate_at_height(current_height)?;
        let event = SessionEvent::Register {
            session: session.clone(),
        };
        Self::append(path, &event)?;
        self.apply(event)
    }

    /// Persist and apply a `revoke` event. Refuses unknown `sessionPk` so a
    /// stale revoke cannot create a phantom record on recover.
    pub fn append_revoke(
        &mut self,
        path: impl AsRef<Path>,
        session_pk: &str,
        height: u64,
    ) -> anyhow::Result<()> {
        if !self.sessions.contains_key(session_pk) {
            anyhow::bail!(
                "sessionStore: cannot revoke unknown sessionPk {}",
                session_pk
            );
        }
        let event = SessionEvent::Revoke {
            session_pk: session_pk.to_string(),
            height,
        };
        Self::append(path, &event)?;
        self.apply(event)
    }

    fn append(path: impl AsRef<Path>, event: &SessionEvent) -> anyhow::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        Ok(())
    }

    /// Apply an event against the in-memory map. Used by `recover` and by
    /// the two append paths after a successful disk write so callers don't
    /// have to re-read the file.
    pub fn apply(&mut self, event: SessionEvent) -> anyhow::Result<()> {
        match event {
            SessionEvent::Register { session } => {
                if let Some(existing) = self.sessions.get(&session.session_pk) {
                    if !existing.revoked {
                        anyhow::bail!(
                            "sessionStore: duplicate active session for sessionPk {}",
                            session.session_pk
                        );
                    }
                }
                self.sessions.insert(session.session_pk.clone(), session);
            }
            SessionEvent::Revoke { session_pk, .. } => {
                let entry = self.sessions.get_mut(&session_pk).ok_or_else(|| {
                    anyhow::anyhow!("sessionStore: revoke for unknown sessionPk {}", session_pk)
                })?;
                entry.revoked = true;
            }
        }
        Ok(())
    }

    pub fn get(&self, session_pk: &str) -> Option<&SessionState> {
        self.sessions.get(session_pk)
    }

    pub fn sessions(&self) -> &BTreeMap<String, SessionState> {
        &self.sessions
    }

    pub fn size(&self) -> usize {
        self.sessions.len()
    }
}
