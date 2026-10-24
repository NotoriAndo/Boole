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
Existing durable evidence and checkpoint must match the replay. If the Engine
is behind its checkpoint, recovery is automatic with no flag. Startup re-derives
from the Engine head to the saved height using only existing, re-verified own
journal inputs, without historical L1 RPC or live acquisition during that replay.
It adopts the checkpoint-height payload only when both its L2 hash and M2/M3 root
exactly equal the saved commitments. Earlier replay steps never lower the saved
checkpoint. Recovery evidence is held until the equality check succeeds; existing
rows are checked and retained. A mismatch is a sticky typed integrity stop before
the checkpoint-height payload is imported, preserving all node-local store files.
The Engine may already have rebuilt the matching prefix below that height.

`--recover-engine-tail` now means that the operator accepts moving the checkpoint,
not permission for the automatic equal-tail recovery. This storage version refuses
a checkpoint move with a typed `operator-checkpoint-move` integrity error,
without changing files. Its append-only evidence binds one hash,
root and attribute digest per height; it has no rollback epoch and refuses a
conflicting selected row. Copying files alone would not make a rewritten live
history safe to reopen. A future move must first preserve the old checkpoint and
above-height evidence inside the store, fsync those copies and append a typed
transition with both heights, hashes and roots under a versioned evidence format.
Until that format exists, preserve the whole store and recover into a fresh one.
No preserved folder or transition row is falsely claimed for a refused operation.
A missing own-journal tail gives a typed availability stop without old-RPC fallback.

T84 covers automatic equal replay with an offline transport, unchanged checkpoints
at intermediate heights, sticky hash/root mismatches with every store file
preserved, and the typed refusal of unsafe operator checkpoint movement. I15 B12
kills the owned Engine, retains its old data directory, and rebuilds a fresh Engine
with no recovery flag through the same complete round and vest on A/B/C.

Torn input/evidence tails are refused without mutation by default.
`--repair-journals` explicitly trims only an incomplete final row after every
complete prefix row has passed validation, fsyncs it and reads the prefix back.
It does not repair a complete malformed row, checkpoint or root mismatch.
Preserve the original store before operator recovery into another directory.

At boot, after the matching writer lock, stale private `scratch-*` directories
and validated checkpoint temps may be removed. Checkpoint temps are stale under
the writer lock even when their PID is live, reused or 1. New attempts use a
PID plus nonce instead of a PID-only name. Orphan worker groups
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

Create-new publication staging is checked immediately after taking the store
lock, before single-link manifest, evidence or object reads. A stale
`.devnet-new-*` alias is removed only when its private owned staging value and
one published target are the same inode with identical bounded, stable bytes.
The alias removal and directories are synced. Empty staging containers cannot
identify their published target and are preserved with a typed stop too.
Missing, ambiguous or changed targets preserve their staging bytes
and stop with typed integrity; valid cached inputs are not quarantined merely
because B9 interrupted unlinking the publication alias. This also covers first
manifest and evidence creation. Ordinary readers still require one hard link.

Live capture asks for receipts by the captured header hash. Unfinalized input
verification failures retry as availability; finalized proof failures remain
sticky integrity errors. No inconsistent capture enters the input journal.

Posting uses a separate advisory inclusion query. It checks only the current
finalized-to-live header window, then requests a receipt only for a transaction
found at or above that tip. Canonical hash and success/revert status are checked;
a finality change pauses the lookup. Included OPEN and PART stages wait for the
fold rather than being reposted during the finality delay. These live receipts
never append journal inputs, change a fold root or become reward authority.

Fault dispatch is compiled only with the explicit `qualification-faults` Cargo
feature of the driver/miner. Ordinary builds refuse any `BOOLE_M4_KILL_*`
environment variable before argument processing or a writer lock. CI checks the
ordinary release executables and runs the pure crash matrix separately with
that feature. Qualification builds use `BOOLE_M4_KILL_POINT`,
`BOOLE_M4_KILL_HEIGHT`, `BOOLE_M4_KILL_ROLE` and `BOOLE_M4_KILL_MARKER`; matching
hooks fsync a metadata-only marker and SIGSTOP their own process for the
coordinator's SIGKILL. The wildcard height is confined to this explicit
qualification profile. These binaries are not the ordinary release profile.
Recordings state the feature and bind exact binaries and sources.

Tests cover current-PID/PID-1 B6 leftovers, nonce attempts, first-publication
aliases, B9 with mining and finality advancing while the writer is dead, exactly
one post per stage across finality, a reorg between capture calls, strict
finalized failures, production environment refusal, all crash boundaries and
no-flag B12. Full v1.3 tapes and derivation bytes remain unchanged.

Own snapshots, archive serving/import, retention and compaction are later M4
work. Full v1.3 replay remains the default.
