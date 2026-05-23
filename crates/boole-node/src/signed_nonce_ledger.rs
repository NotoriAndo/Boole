use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

/// P1.6b — append-only NDJSON ledger of accepted `(signerPk, nonce)`
/// pairs for non-session signed envelope routes (`/sessions`,
/// `/sessions/{pk}/revoke`, `/bounties`, `/bounties/{id}/status`,
/// `/bounties/{id}/proof`, `/receipts`). The store keeps a `HashSet` of
/// `"<signerPk>:<nonce>"` keys in memory so the gate can answer dedup
/// queries without re-reading the file. Recovery replays the ledger from
/// disk so restart-replay of a previously-burned nonce still rejects with
/// `nonce_replayed`.
///
/// Parallel to `FileNonceLedger`, which keys on `sessionPk` for the
/// session-bound `/submit` path. The two ledgers live in separate files
/// so a session-bound replay cannot mask a per-signer envelope replay or
/// vice versa.
#[derive(Debug, Default)]
pub struct FileSignedNonceLedger {
    seen: HashSet<String>,
}

/// One serialized line in the signed-envelope nonce NDJSON file. Tagged
/// so future event kinds can be added without breaking older ledgers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SignedNonceEvent {
    Burn {
        #[serde(rename = "signerPk")]
        signer_pk: String,
        nonce: String,
    },
}

impl FileSignedNonceLedger {
    /// Build an in-memory ledger by replaying the NDJSON file at `path`.
    /// Returns an empty ledger if the file does not yet exist.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: SignedNonceEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("signedNonceLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            ledger.apply(event);
        }
        Ok(ledger)
    }

    /// True if the pair has been burned previously.
    pub fn contains(&self, signer_pk: &str, nonce: &str) -> bool {
        self.seen.contains(&Self::key(signer_pk, nonce))
    }

    /// Persist and apply a burn event. Returns `Ok(false)` if the pair was
    /// already burned (caller must reject with `nonce_replayed`); returns
    /// `Ok(true)` after a successful append.
    pub fn append_burn(
        &mut self,
        path: impl AsRef<Path>,
        signer_pk: &str,
        nonce: &str,
    ) -> anyhow::Result<bool> {
        if self.contains(signer_pk, nonce) {
            return Ok(false);
        }
        let event = SignedNonceEvent::Burn {
            signer_pk: signer_pk.to_string(),
            nonce: nonce.to_string(),
        };
        Self::append(path, &event)?;
        self.apply(event);
        Ok(true)
    }

    fn append(path: impl AsRef<Path>, event: &SignedNonceEvent) -> anyhow::Result<()> {
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(event)?)
    }

    fn apply(&mut self, event: SignedNonceEvent) {
        match event {
            SignedNonceEvent::Burn { signer_pk, nonce } => {
                self.seen.insert(Self::key(&signer_pk, &nonce));
            }
        }
    }

    fn key(signer_pk: &str, nonce: &str) -> String {
        format!("{signer_pk}:{nonce}")
    }

    #[cfg(test)]
    pub fn size(&self) -> usize {
        self.seen.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;

    fn tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-signed-nonce-ledger-{}-{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmp dir");
        dir
    }

    #[test]
    fn signed_nonce_ledger_round_trip_persists_and_dedupes() {
        let dir = tmp_dir("round-trip");
        let path = dir.join("signed-nonces.ndjson");

        let mut writer = FileSignedNonceLedger::default();
        assert!(writer
            .append_burn(&path, "aabb", "n-1")
            .expect("first burn"));
        assert!(!writer.append_burn(&path, "aabb", "n-1").expect("dup burn"));
        assert!(writer.contains("aabb", "n-1"));
        assert!(!writer.contains("aabb", "n-2"));
        assert!(!writer.contains("ccdd", "n-1"));

        let recovered = FileSignedNonceLedger::recover(&path).expect("recover");
        assert!(recovered.contains("aabb", "n-1"));
        assert_eq!(recovered.size(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn signed_nonce_ledger_recovers_and_truncates_partial_trailing_line_after_crash() {
        let dir = tmp_dir("partial-recovery");
        let path = dir.join("signed-nonces.ndjson");

        let mut writer = FileSignedNonceLedger::default();
        writer
            .append_burn(&path, "aabb", "n-1")
            .expect("append complete burn");

        let stable_len = fs::metadata(&path).expect("metadata").len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open ledger")
            .write_all(br#"{"kind":"burn","signerPk":"truncated"#)
            .expect("write partial trailing line");

        let recovered = FileSignedNonceLedger::recover(&path).expect("recover ignores torn tail");
        assert!(recovered.contains("aabb", "n-1"));
        assert_eq!(recovered.size(), 1);
        assert_eq!(
            fs::metadata(&path).expect("post-recover").len(),
            stable_len,
            "recovery must truncate the torn trailing line so restart is idempotent"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn signed_nonce_ledger_rejects_complete_corrupt_line_instead_of_truncating_history() {
        let dir = tmp_dir("complete-corrupt-line");
        let path = dir.join("signed-nonces.ndjson");

        let mut writer = FileSignedNonceLedger::default();
        writer
            .append_burn(&path, "aabb", "n-1")
            .expect("append complete burn");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open ledger")
            .write_all(b"not-json\n")
            .expect("write complete corrupt line");

        let err =
            FileSignedNonceLedger::recover(&path).expect_err("complete corrupt lines must fail");
        assert!(
            err.to_string().contains("invalid JSON"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
