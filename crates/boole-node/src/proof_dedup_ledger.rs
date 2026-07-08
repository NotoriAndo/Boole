use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::durability::{
    append_ndjson_line_durable, read_stable_prefix, write_ndjson_lines_atomic,
};

/// N2.3 — append-only NDJSON ledger of server-computed proof-canon hashes
/// that have already been credited on the `/submit` path. The dedup set is
/// kept in memory as a `HashSet<String>` of canon hashes so the admit guard
/// can answer "has this exact proof already been credited?" without
/// re-reading the file. Recovery replays the ledger from disk so a duplicate
/// proof — the same canonical bytes resubmitted under any prover pk — still
/// rejects after a process restart.
///
/// N4-pre.1 (ADR-0012) — this ledger is an ADMISSION EARLY-REJECT CACHE,
/// not the source of truth: the consensus rule that one canon_hash is
/// credited at most once on the whole chain is enforced by replay/verify
/// from block data alone (the `boole-core` replay dedup) and pre-checked by
/// `build_block_selection`. Deleting this file weakens nothing but
/// early-reject latency; a duplicate-credit block is invalid on every node
/// regardless of any local ledger state.
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

    /// N4 — atomically REPLACE the ledger file and return a fresh in-memory
    /// mirror holding exactly `canon_hashes` (deduplicated, first-seen order).
    ///
    /// This is the reorg path. The mirror is a non-authoritative admission
    /// early-reject cache (ADR-0012, see the type doc): the consensus
    /// one-credit-per-canon-hash rule is enforced by block replay, not by this
    /// file. When a reorg rewrites the canonical chain wholesale, the cache is
    /// rewritten to match the newly-adopted chain's credited proofs — dropping
    /// entries from the abandoned fork (which would otherwise wrongly
    /// early-reject a proof that is creditable again on the new chain) and
    /// adding the new chain's. Unlike `append_credit`, this truncates: the file
    /// is replaced atomically (temp-file + rename), so a crash mid-rebuild
    /// leaves either the old or the new file, never a torn splice.
    pub fn rebuild_from_credits(
        path: impl AsRef<Path>,
        canon_hashes: &[String],
    ) -> anyhow::Result<Self> {
        let mut seen = HashSet::new();
        let mut lines = Vec::with_capacity(canon_hashes.len());
        for canon_hash in canon_hashes {
            if seen.insert(canon_hash.clone()) {
                lines.push(serde_json::to_string(&ProofDedupEvent::Credit {
                    canon_hash: canon_hash.clone(),
                })?);
            }
        }
        write_ndjson_lines_atomic(path.as_ref(), &lines)?;
        Ok(Self { seen })
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
    fn rebuild_from_credits_replaces_stale_entries_atomically() {
        let dir = tmp_dir("rebuild-from-credits");
        let path = dir.join("proof-dedup.ndjson");

        // Seed the mirror as if the node sat on an abandoned fork that credited
        // proof `stale-x0`.
        let mut writer = FileProofDedupLedger::default();
        writer.append_credit(&path, "stale-x0").expect("seed stale");
        assert!(writer.contains("stale-x0"));

        // Reorg adopts a chain crediting two distinct proofs; the new chain also
        // happens to reference `y0` twice, which must collapse to one row.
        let rebuilt = FileProofDedupLedger::rebuild_from_credits(
            &path,
            &["y0".to_string(), "y1".to_string(), "y0".to_string()],
        )
        .expect("rebuild from adopted chain");

        // The in-memory mirror reflects the new chain, not the abandoned fork.
        assert!(
            !rebuilt.contains("stale-x0"),
            "abandoned-fork credit must be dropped"
        );
        assert!(rebuilt.contains("y0"));
        assert!(rebuilt.contains("y1"));
        assert_eq!(rebuilt.size(), 2, "intra-chain duplicate collapses to one");

        // A fresh recover over the rewritten file agrees — the file was replaced
        // atomically, not appended to.
        let recovered = FileProofDedupLedger::recover(&path).expect("recover rewritten file");
        assert!(!recovered.contains("stale-x0"));
        assert!(recovered.contains("y0"));
        assert!(recovered.contains("y1"));
        assert_eq!(recovered.size(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rebuild_from_credits_with_no_credits_clears_the_mirror() {
        let dir = tmp_dir("rebuild-empty");
        let path = dir.join("proof-dedup.ndjson");

        let mut writer = FileProofDedupLedger::default();
        writer.append_credit(&path, "stale").expect("seed stale");

        // A reorg to a chain that credits nothing empties the mirror + file.
        let rebuilt =
            FileProofDedupLedger::rebuild_from_credits(&path, &[]).expect("rebuild empty");
        assert_eq!(rebuilt.size(), 0);

        let recovered = FileProofDedupLedger::recover(&path).expect("recover emptied file");
        assert_eq!(recovered.size(), 0, "empty adopted chain clears the mirror");

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
