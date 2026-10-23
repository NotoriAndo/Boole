# Devnet node durability v0 (M4a)

This is a node-local storage profile for the v1.3 devnet. It changes no inbox,
fold, root, deposit, gate descriptor, golden or recording bytes.

The driver requires `--state-dir`. Its checkpoint and evidence files must be
inside that directory; the gate cache is `state-dir/gate-cache`. The miner uses
its existing private `--state-dir`. Each store keeps a lifetime single-writer
lock. The input manifest binds L1 chain and genesis anchor, L2 chain and genesis,
configuration digest, fork genesis row and gas limit (S18). A manifest without
its journal, or old authoritative files without both, is refused (S19). Use a
fresh directory for old stores; M4a does not invent their missing input history.

`input.ndjson` stores the canonical header RLP and full receipt and raw
transaction proofs for each acquired finalized height. Reopening streams and
re-verifies every row: header keccak, parent linkage, receipt/transaction trie,
blooms and global log indices. Re-folding binds each input to the selected L2
L1-info deposit, mandatory root and full system prefix. A saved root never
replaces replay. File versions are bound across replay and later reads.

Finality can advance several heights at once. Before expiry, the live source
captures authenticated inputs at or above the current finalized tip into a
separate content-addressed `input-cache`. These unfinalized bytes never enter
the fold directly. A segment is promoted only after its complete parent chain
links the independently obtained finalized tip back to the journal. An expired
missing input is a resumable availability stop; there is no historical RPC
fallback. Starting without any journal requires the devnet genesis anchor at
finalized height zero. Archive-assisted fresh joins belong to M4b.

Publication per height is input fsync, pure fold, Engine build/import/selection,
evidence fsync, then checkpoint replacement and directory fsync. Evidence and
input appends use the copied R1 transaction: file and directory fsync, rollback
to the confirmed prefix on failure, and a process-lifetime fence if rollback
cannot be confirmed. R1 itself is unchanged. Ingest and append stop below
15 GiB free disk (S23). A successful rollback permits retry at the same height.
An indeterminate write requires restart. No height is skipped.

A selected Engine head ahead of evidence is fully re-folded and gets an
explicit `RECOVERED` evidence row; lost Engine-call evidence is never invented.
Existing durable evidence and checkpoint must match the replay. The driver
refuses an Engine behind its checkpoint by default. Explicit
`--recover-engine-tail` re-derives the missing tail from the journal, checks the
saved L2 hash and root before adopting the checkpoint-height payload, and never
moves the saved checkpoint backwards. A mismatch is a sticky integrity stop.

Torn input/evidence tails are refused without mutation by default.
`--repair-journals` explicitly trims only an incomplete final row after every
complete prefix row has passed validation, fsyncs it and reads the prefix back.
It does not repair a complete malformed row, checkpoint or root mismatch.
Preserve the original store before operator recovery into another directory.

At boot, after the matching writer lock, stale private `scratch-*` directories
and recognized dead-owner checkpoint temps may be removed. Orphan worker groups
are killed only when the kernel executable path is inside that locked scratch
tree, the executable matches a configured binary pin, the PID is its own group
leader and its birth identity is unchanged. Their output is never read.
Unproven ownership stops cleanup and preserves the files. Cache loss or a bad
HMAC is a miss. Corrupt acquisition objects move intact into `corrupt/` before
refetch; a valid address is never overwritten.

The miner folds from its journal, checks canonical L2 VERDICT/REWARD/root bytes,
and reconciles its existing private records. Posting inclusion is read from
re-verified transaction and successful-receipt proofs in the local finalized
journal, including reverted receipts. Availability pauses posting and retries;
hash, root, prefix and file-version failures remain sticky. Private submission
bytes, salts and cancel secrets stay out of status and qualification output.

The explicit local qualification environment `BOOLE_M4_KILL_POINT`,
`BOOLE_M4_KILL_HEIGHT`, `BOOLE_M4_KILL_ROLE`, and `BOOLE_M4_KILL_MARKER` enables
default-off boundary hooks. Matching hooks fsync a metadata-only marker and
SIGSTOP their own process; the coordinator SIGKILLs that recorded child PID.
Tests cover B1–B11 and Engine-tail loss B12, including read-only refusal and
explicit repair. No hook runs with the selectors absent.

Own snapshots, archive serving/import, retention and compaction are later M4
work. Full v1.3 replay remains the default.
