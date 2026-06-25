use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

/// N2.3 — append-only NDJSON ledger of server-computed proof-canon hashes
/// that have already been credited on the `/submit` path. The dedup set is
/// kept in memory as a `HashSet<String>` of canon hashes so the admit guard
/// can answer "has this exact proof already been credited?" without
/// re-reading the file. Recovery replays the ledger from disk so a duplicate
/// proof — the same canonical bytes resubmitted under any prover pk — still
/// rejects after a process restart.
///
/// The key is the server's `SHA-256(proof package bytes)`, never a
/// client-supplied field, so two miners cannot farm the same proof under
/// different prover pks for multiple credits. Parallel to `FileNonceLedger`
/// (keyed on `(sessionPk, nonce)`) and `FileSignedNonceLedger` (keyed on
/// `(signerPk, nonce)`); the three ledgers live in separate files so one
/// dedup dimension cannot mask another.
#[derive(Debug, Default)]
pub struct FileProofDedupLedger {
    seen: HashSet<String>,
}

/// One serialized line in the proof-dedup NDJSON file. Tagged so future
/// event kinds can be added without breaking older ledgers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProofDedupEvent {
    Credit {
        #[serde(rename = "canonHash")]
        canon_hash: String,
    },
}

impl FileProofDedupLedger {
    /// Build an in-memory ledger by replaying the NDJSON file at `path`.
    /// Returns an empty ledger if the file does not yet exist.
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: ProofDedupEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("proofDedupLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            ledger.apply(event);
        }
        Ok(ledger)
    }

    /// True if this proof canon hash has already been credited.
    pub fn contains(&self, canon_hash: &str) -> bool {
        self.seen.contains(canon_hash)
    }

    /// Persist and apply a credit event. Returns `Ok(false)` if the proof was
    /// already credited (caller must reject with `duplicate_proof`); returns
    /// `Ok(true)` after a successful append.
    pub fn append_credit(
        &mut self,
        path: impl AsRef<Path>,
        canon_hash: &str,
    ) -> anyhow::Result<bool> {
        if self.contains(canon_hash) {
            return Ok(false);
        }
        let event = ProofDedupEvent::Credit {
            canon_hash: canon_hash.to_string(),
        };
        Self::append(path, &event)?;
        self.apply(event);
        Ok(true)
    }

    fn append(path: impl AsRef<Path>, event: &ProofDedupEvent) -> anyhow::Result<()> {
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(event)?)
    }

    fn apply(&mut self, event: ProofDedupEvent) {
        match event {
            ProofDedupEvent::Credit { canon_hash } => {
                self.seen.insert(canon_hash);
            }
        }
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
            "boole-node-proof-dedup-ledger-{}-{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmp dir");
        dir
    }

    #[test]
    fn proof_dedup_ledger_round_trip_persists_and_dedupes() {
        let dir = tmp_dir("round-trip");
        let path = dir.join("proof-dedup.ndjson");

        let mut writer = FileProofDedupLedger::default();
        assert!(writer.append_credit(&path, "aa11").expect("first credit"));
        assert!(!writer.append_credit(&path, "aa11").expect("dup credit"));
        assert!(writer.contains("aa11"));
        assert!(!writer.contains("bb22"));

        let recovered = FileProofDedupLedger::recover(&path).expect("recover");
        assert!(recovered.contains("aa11"));
        assert_eq!(recovered.size(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn proof_dedup_ledger_recovers_and_truncates_partial_trailing_line_after_crash() {
        let dir = tmp_dir("partial-recovery");
        let path = dir.join("proof-dedup.ndjson");

        let mut writer = FileProofDedupLedger::default();
        writer
            .append_credit(&path, "aa11")
            .expect("append complete credit");

        let stable_len = fs::metadata(&path).expect("metadata").len();
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open ledger")
            .write_all(br#"{"kind":"credit","canonHash":"truncated"#)
            .expect("write partial trailing line");

        let recovered = FileProofDedupLedger::recover(&path).expect("recover ignores torn tail");
        assert!(recovered.contains("aa11"));
        assert_eq!(recovered.size(), 1);
        assert_eq!(
            fs::metadata(&path).expect("post-recover").len(),
            stable_len,
            "recovery must truncate the torn trailing line so restart is idempotent"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn proof_dedup_ledger_rejects_complete_corrupt_line_instead_of_truncating_history() {
        let dir = tmp_dir("complete-corrupt-line");
        let path = dir.join("proof-dedup.ndjson");

        let mut writer = FileProofDedupLedger::default();
        writer
            .append_credit(&path, "aa11")
            .expect("append complete credit");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open ledger")
            .write_all(b"not-json\n")
            .expect("write complete corrupt line");

        let err =
            FileProofDedupLedger::recover(&path).expect_err("complete corrupt lines must fail");
        assert!(
            err.to_string().contains("invalid JSON"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
