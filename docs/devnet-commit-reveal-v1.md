# Devnet commit and reveal specification v1

Status: devnet only. M2-R1–M2-R28 extend [derivation v0](devnet-derivation-v0.md).
D1–D18 still govern input validation, user transactions and Engine execution.
M2 changes no payload transaction. All integers below are unsigned big-endian;
hashes are 32 bytes and use SHA-256 unless explicitly stated otherwise.

## Configuration

A network pins its chain ID, schema ID, base root, append-only package table,
allowed environment compositions, incompatible package pairs, terms table and
POOL public key. The devnet defaults use synthetic environment identities; they
are fixtures for derivation and do not certify an executable kernel environment.
The schema names EENV2-canonical-closure-v2. Terms ID 1 pins the immutable text
`Option A placeholder: proofs CC0-1.0; granted trace CC-BY-4.0` by its digest.
A replacement legal text receives a new ID.

| Constant | Devnet | Lean testnet | Production |
|---|---:|---:|---:|
| L2_CHAIN_ID | 720926 | per network | per network |
| D / W | 9 / 64 | 96 / 300 | 96 / 300 |
| W_SLACK_MIN / P_BLK | 16 / 6 | 64 / 6 | 64 / 6 |
| PART_SIZE / MAX_PARTS | 122880 / 128 | same | same |
| Z_CAP / RAW_CAP | 15728640 / 134217728 | same | same |
| RATIO_MAX / K_MAX / OPEN_MAX | 16 / 4 / 522 | same | same |
| PHI / G_L1 / G_COMMIT | 1/2 / 30000000 / 16000 | 1/2 / 60000000 / 16000 | same as lean testnet |
| C_COMMIT | 937 | 1875 | 1875 |
| Q_MAX / V_PER_DEPOSIT | 8 / 128 | 8 / 256 | 8 / 256 |
| K_OPEN | 1 | 1 | 1, configurable 1–3 |
| SOLVE_DEADLINE / ANNOUNCE_GAP / REVEAL_GRACE | 48 / 4 / 4 | 7200 / 300 / 25 | same as lean testnet |
| POOL_LEAD / POOL_MAX | 2 / 65536 | 7200 / 65536 | 50400 / 65536 |
| STATEMENT_MAX / TRACE_EMBARGO_MAX | 122880 / 2628000 | same | same |

C_COMMIT is floor(PHI·G_L1/G_COMMIT). Windows count finalized heights.
VERDICT_GAS is a configured M3 reservation (default 1000000); verdicts are not
implemented by M2. Startup validates the following and refuses invalid config:

- **S1:** D ≥ 2.
- **S2:** W ≥ D + ceil((MAX_PARTS+1)/P_BLK) + W_SLACK_MIN; checked arithmetic,
  nonzero P_BLK.
- **S3:** C_COMMIT ≤ Q_MAX·V_PER_DEPOSIT.
- **S4:** L1INFO_GAS + Q_MAX·VERDICT_GAS + 16777216 ≤ the L2 block gas limit,
  supplied explicitly to configuration validation (independent of G_L1).
- **S5:** the largest PART item (123160 bytes), selector and padded ABI head fit
  the pool limit (131072 bytes).
- **S6:** 1 ≤ K_OPEN ≤ 3; allowed environments contain only strictly ascending,
  known packages, at most K_MAX ≤ 4, with no incompatible pair. The EVM and
  RISC-V pair is excluded until its compatibility is established.

## Bytes and identities

**M2-R1 — Kinds.** After D7's ABI envelope, an item is kind followed by body.
Assigned kinds are 02 COMMIT, 03 REVEAL_OPEN, 04 REVEAL_PART, 07 CANCEL,
08 POOL, 09 PROBLEM_REVEAL. Kind 05 is retired; 06 reserves blob carriage;
0a and 0b reserve ZK attestations and submissions; 0c–0f remain reserved.
Retired and reserved kinds drop ITEM-KIND-NOT-ACCEPTED. Kinds 00 and ≥10
retain D8 behavior. Every assigned body begins with version 01 and has exact
length. D2 permits padding outside the item.

**M2-R2 — COMMIT.** Body (85 bytes):
`version:u8 | poster:20 | problem_id:32 | commit_hash:32`.
Commits are unsigned. The candidate key is (problem_id, commit_hash).

**M2-R3 — OPEN and commitment.** Body:
`version:u8 | poster:20 | cancel_hash:32 | B`.
B is 277+32k+model_len bytes; OPEN is 330+32k+model_len bytes.

| Offset in B | Field |
|---|---|
| 0 / 1 / 33 / 53 | version:u8 / problem_id:32 / solver:20 / salt:32 |
| 85 / 86 / 118 / 150 | mode:u8 / schema_id:32 / base_root:32 / k:u8 |
| 151 | pkg_id:32k |
| q = 151+32k | codec:u8 / export_format:u8 |
| q+2 / q+10 | raw_len:u64 / export_digest:32 |
| q+42 / q+46 / q+48 | z_len:u32 / n:u16 / data_root:32 |
| q+80 / q+82 / q+84 / q+86 | deps_len:u16 / ext_len:u16 / meta_len:u16 / meta |

Version is 1; mode is 1 (public parts); codec and export_format are 1.
Mode 2, dependency entries and extensions are reserved. deps_len and ext_len
are zero in M2. If nonzero, declared dependency entries are 64 bytes each and
extension length counts bytes; these bytes precede meta after the length fields.
A structurally complete reserved encoding fails policy. Codec 1 means exactly
one zstd frame, no skippable frame or trailing bytes, content size present and
equal to raw_len, no dictionary, valid checksum, and a window at most
min(raw_len, 2^27). The gate descriptor pins the decoder and exporter versions;
derivation does not decode the frame.

```
cancel_hash = SHA256("BOOLE-M2/CANCEL/v1" | cancel_secret)
body_digest = SHA256("BOOLE-M2/BODY/v1" | B)
commit_hash = SHA256("BOOLE-M2/COMMIT/v2" | u64(chain_id) |
                     problem_id | cancel_hash | body_digest)
```

The solver is the L2 payee. Salt and cancel secret are independent fresh 32-byte
values. The poster is outside B and cannot change the committed payee.

**M2-R4 — PART.** Body:
`version:u8 | poster:20 | commit_hash:32 | index:u16 | path:32d | data`.
d = ceil(log2 n), zero for n=1. Index is below n. Every nonfinal chunk is exactly
PART_SIZE bytes; the final chunk is z_len−(n−1)·PART_SIZE bytes. Arrival order is
arbitrary; transport bytes are reconstructed in index order.

**M2-R19 — Merkle tree.**
`leaf_i = SHA256(00 | "BOOLE-M2/PART/v1" | solver | salt | u16(i) | SHA256(data_i))`.
Internal nodes are SHA256(01 | left | right). Pad leaves with zero hashes to a
power of two. A path lists siblings bottom-up; index bits select sides. POOL
uses the same internal tree. Leaves are independent of carriage. A copier's
new solver or salt requires every part hash to rebuild its own root.

**M2-R20 — CANCEL.** Body (129 bytes):
`version:u8 | problem_id:32 | commit_hash:32 | body_digest:32 | cancel_secret:32`.
Decode first; recompute the commitment using cancel_hash(secret), otherwise
M2-CANCEL-INVALID; absent key → M2-COMMIT-NOT-FOUND; CANCELLED → M2-DUPLICATE;
EXPIRED or CHECKING → M2-OUTSIDE-WINDOW. Success is CANCELLED and
M2-CANCELLED. Anyone possessing the secret may cancel. The key remains a
tombstone; it cannot be re-committed while the problem accepts commits.

**M2-R22 — Environment.**
`env_id = SHA256("BOOLE-M2/ENV/v1" | schema_id | base_root | u8(k) | pkg_id[k])`.
Packages are strictly ascending, known, at most K_MAX, and the composition is
allowed by genesis. It must contain the problem's required packages. Tables are
append-only so existing commitments remain usable.

**M2-R17 — Metadata.**
`meta_version:u8 | model_len:u8 | model | license_id:u8 | trace_permission:u8 |
trace_hash:32 | trace_embargo:u32`; meta_len = 40+model_len. Version is 1;
model is 0–64 printable ASCII bytes (20–7e); empty means undeclared. License
ID must be nonzero and known even without a trace. Permission is 0 or 1;
1 grants the terms' trace clause and requires a nonzero trace hash. An absent
trace requires zero embargo and permission. Embargo is at most TRACE_EMBARGO_MAX
heights after completion. Policy failures use M2-POLICY-INVALID; structural
version and length failures use M2-DECODE-INVALID.

**M2-R18 — Metadata effect.** Metadata is self-reported. Apart from validity it
changes no priority, verdict or reward.

## Fold and checks

**M2-R5 — State.** Untrusted configuration values, including decoded JSON, must
pass S1–S6 before constructing state. The constructor pins the validated values
and explicit L2 gas limit; a fold refuses altered or invalid configuration.
S0 is empty. Sh = step(S(h−1), L1[h], logs[h]). Maps are
ordered. A live candidate stores its inclusion position, h_c, pending/fixed t,
COMMIT poster and status. After OPEN it also stores h_o, solver, salt, n,
z_len, raw_len, data_root, body_digest, env_id, mode, a 128-bit bitmap and h_r.
It stores no part bytes or positions. An opened-hash index finds candidates;
an expiry index orders (h_c+W, priority). EXPIRED and CANCELLED records become
compact tombstones. At close(problem)+W+1, delete its tombstones. CHECKING and
later records are never pruned in M2. Closed-problem checks precede duplicates.

**M2-R6 — Height order.** Set ties for h−1 from L1[h].mixHash; process slot
events in slot order (draw, withheld, deadline); expire candidates by ascending problem_id, then by that problem's expiry-index
key (expiry height, h_c, t, commit_hash); prune by ascending problem_id and
candidate key (problem_id, commit_hash); process logs sorted by (transactionIndex, logIndex); compute root.
D7 F0 precedes item checks. A fold requires h=checked(previous_height+1),
starting at height 1. A repeated, skipped or overflowing height is a typed input
error and stalls the driver. The first failing item check wins.

**M2-R23 — OPEN prelookup checks**, in this order:

| Row | Check | Failure |
|---|---|---|
| 1 | exact declared layout, meta_len=40+model_len, all three versions 1 | M2-DECODE-INVALID |
| 2 | mode=1 | M2-POLICY-INVALID |
| 3 | configured schema and base | M2-POLICY-INVALID |
| 4 | k≤K_MAX, known ascending packages, allowed environment | M2-POLICY-INVALID |
| 5 | codec=export_format=1, deps_len=ext_len=0 | M2-POLICY-INVALID |
| 6 | R17 metadata | M2-POLICY-INVALID |
| 7 | z_len≥1, n=ceil(z_len/PART_SIZE) | M2-POLICY-INVALID |
| 8 | n≤128, 1≤raw_len≤RAW_CAP, raw_len≤RATIO_MAX·z_len | M2-SIZE-CAP |

**M2-R7 — COMMIT checks.** Decode; unknown problem → M2-UNKNOWN-PROBLEM;
h≤h_v → M2-OUTSIDE-WINDOW; closed → M2-PROBLEM-CLOSED; existing key in any
status → M2-DUPLICATE; accepted-commit cap reached → M2-BLOCK-BUDGET-EXCEEDED.
Success records COMMITTED, h_c+W expiry, pending t; M2-COMMIT-RECORDED.

**M2-R8 — OPEN checks.** R23; look up its computed key (absent →
M2-COMMIT-NOT-FOUND; EXPIRED/CANCELLED → M2-OUTSIDE-WINDOW; OPEN/CHECKING →
M2-DUPLICATE); required package coverage → M2-POLICY-INVALID; h<h_c+D →
M2-OUTSIDE-WINDOW. Success records OPEN and M2-REVEAL-OPENED. A drop consumes
nothing and leaves the commitment usable.

**M2-R9 — PART checks.** Decode at least 55 bytes and version 1; find opened
hash (absent → M2-REVEAL-NOT-OPEN, EXPIRED/CANCELLED → M2-OUTSIDE-WINDOW,
CHECKING → M2-DUPLICATE); check index, exact canonical data/path length and
Merkle proof → M2-PART-INVALID; received index → M2-DUPLICATE. A valid part
sets its bit and gets M2-PART-RECORDED. The last missing part instead gets
M2-REVEAL-COMPLETE and freezes CHECKING at h_r. A part before OPEN drops.

**M2-R10 — Windows.** OPEN and completion must fall in inclusive
[h_c+D,h_c+W] (PART may arrive at any height after OPEN). At h_c+W+1,
COMMITTED or OPEN becomes EXPIRED. CHECKING never expires and cannot be
cancelled. A future gate fault is a node-local stall, never candidate expiry.
For registry parity, reveal_window=W+1. The registry has no minimum delay;
the Rust side retains D≥2 and vectors schedule reveals within its valid window.
COMMITTED and OPEN map to COMMITTED; CHECKING with PASS maps to VALID-REVEAL.

**M2-R11 — Admission.** Only accepted COMMITs count against C_COMMIT, globally
per height. Drops consume nothing. OPEN, PART, CANCEL, POOL and PROBLEM_REVEAL
have no derivation quota. Per-key quotas belong to batcher policy only.

**M2-R12 — Decision codes.** Every item with kind 02–0f passing F0 has
forward=false and an m2 field (kind and code). Other decisions omit this field.

| Code | Meaning |
|---|---|
| 01 / 02 / 03 | M2-DECODE-INVALID / M2-POLICY-INVALID / M2-SIZE-CAP |
| 04 / 05 | M2-UNKNOWN-PROBLEM / M2-PROBLEM-CLOSED |
| 06 / 07 / 08 | M2-COMMIT-NOT-FOUND / M2-REVEAL-NOT-OPEN / M2-PART-INVALID |
| 09 / 0a / 0b | M2-OUTSIDE-WINDOW / M2-DUPLICATE / M2-BLOCK-BUDGET-EXCEEDED |
| 0c / 0d / 0e | M2-CANCEL-INVALID / M2-POOL-INVALID / M2-MEMBERSHIP-INVALID |
| 3f | ITEM-KIND-NOT-ACCEPTED |
| 80 / 81 / 82 / 83 | M2-COMMIT-RECORDED / M2-REVEAL-OPENED / M2-PART-RECORDED / M2-REVEAL-COMPLETE |
| 84 / 85 / 86 | M2-CANCELLED / M2-POOL-RECORDED / M2-PROBLEM-OPENED |
| c1 / c2 / c3 | ROUND-DRAWN / ROUND-WITHHELD / PROBLEM-DEADLINE |

Expiry and prune have record types 02 and 04 below; they do not emit separate
c0 or c4 event codes.

**M2-R13 — Payload independence.** M2 never forwards an item, consumes no D12
gas budget, changes no user decision and causes no InputError, panic or Engine
change. Existing D7 input errors still stall.

**M2-R14 — Determinism.** Additional inputs are prior M2 state and already-read
header hash, parentHash, mixHash, timestamp and baseFeePerGas. No decompression,
gate, drand, receipts, transaction bodies, beacon data or L1 sender reads enter
the fold. Use checked arithmetic, ordered maps, SHA-256 and low-s secp256k1
POOL signatures under D10's scalar/recovery rules.

The stateless `derive` API retains M1 attribute compatibility; consensus M2
callers use `derive_with_m2`. The legacy `user_body` helper accepts only kind 01;
consensus dispatch uses the ABI item decoder and the appropriate M1 or M2 path.

**M2-R15 — Driver.** Publish Sh only after D15(6) succeeds. A stall retains
S(h−1). On startup re-fold each historical L1 header and logs by the hash in
that L2 block's L1-info deposit, never by number. RPC failures stall. Evidence
adds m2_root and M2 decisions. A journal is a checkpoint, not an authority;
the anchored re-fold validates restart state. Historical L2 headers must also
form a continuous `parentHash` chain from genesis to the selected parent.
The M2 RPC startup path verifies SHA-256 of the deployed inbox runtime against
the devnet profile (`a14704d805198421bae48bf79534bc62d974699f203abc3ed4bc751e35fe0ca6`)
before Engine access. The legacy M1 transport also checks this pin before any
M2 item is folded. The original M1 runtime is unsuitable for large M2 parts.

Evidence records the adopted state root on a stall. If checkpoint persistence
fails after a DERIVED evidence line, append a STALL line containing the retained
parent root; the earlier DERIVED line alone does not establish state adoption.

A checkpoint ahead of the selected node head causes a permanent stall. For a
deliberately restored earlier devnet database, stop the driver, preserve the
checkpoint as a backup, verify the intended genesis, configuration and selected
L2 head, and move the checkpoint aside before restarting. The driver then re-folds
the selected chain from its L1-info origin hashes. Investigate an unexpected
rollback before using this procedure; never edit a checkpoint height or root to
force acceptance. Restoring the intended L2 database is the alternative when its
newer selected head should have been retained.

**M2-R16 — Priority.** Compare only within one problem by (h_c,t), where
`t=SHA256("BOOLE-M2/TIE/v1" | commit_hash | randao_out(h_c))` and
randao_out(x)=L1[x+1].mixHash. Set t at h_c+1. Transaction and log positions
are excluded from priority. A block proposer can know its own RANDAO contribution
and grind its own commitments; this residual does not permit reordering others.

**M2-R21 — Sender of record.** The poster of the first accepted COMMIT, OPEN,
or each PART index is its sender of record. Duplicates cannot replace it. PART
positions, poster, height and length are recoverable by the consensus rescan.
Reimbursement data uses (21000+48·item_length)·L1.baseFeePerGas; payment is a
future interface, not implemented here. L1 transaction sender and receipts are
never read.

## Pools and rounds

**M2-R24 — POOL.** Body (146 bytes):
`version:u8 | pool_seq:u32 | pool_root:32 | leaf_count:u32 | start_height:u64 |
battery_digest:32 | signature:65`.
Signature is r:32, s:32, recovery parity:u8 (0 or 1), low-s secp256k1 over
`SHA256("BOOLE-M2/POOL/v1" | u64(chain_id) | body[0..81])`, recovering POOL_KEY.
Checks: decode → M2-DECODE-INVALID; signature → M2-POOL-INVALID; seq=last+1,
1≤leaf_count≤POOL_MAX, start≥h+POOL_LEAD → M2-POOL-INVALID; reused root →
M2-DUPLICATE. Success M2-POOL-RECORDED. Battery digest is evidence only.

A leaf is `SHA256(00 | "BOOLE-M2/POOL-LEAF/v1" | u64(chain_id) | leaf_kind:u8 |
req_k:u8 | req_pkg[32·req_k] | SHA256(package) | leaf_salt:32)`.
Kinds 1 and 2 mean a fixed statement and generator family respectively.

**M2-R25 — Draw.** An IDLE slot scheduled at h_d draws at h_d+1. Choose the
lowest-sequence pool with unused leaves and start≤h_d. If none exists, wait;
retry with the next finalized announcement height. Slots draw in slot order.
The first schedule is the first pool's start. Subsequent pools do not postpone
an existing draw. For ascending unused indices U:

```
seed = SHA256("BOOLE-M2/DRAW/v1" | u64(chain_id) | pool_root |
              slot:u8 | round:u32 | randao_out(h_d))
leaf_index = U[u64(seed[0..8]) mod |U|]
```

Remove the selected leaf without replacement, increment round, record DRAWN
and instance_seed=seed. A waiting exhausted slot resumes when a pool is active.

**M2-R26 — PROBLEM_REVEAL.** Body:
`version:u8 | slot:u8 | round:u32 | leaf_index:u32 | leaf_kind:u8 | req_k:u8 |
req_pkg[32·req_k] | leaf_salt:32 | depth:u8 | path[32·depth] |
package_len:u32 | package` (49+32req_k+32depth+package_len bytes).
Checks: exact decode; matching round/index → M2-MEMBERSHIP-INVALID,
OPEN/CLOSED → M2-DUPLICATE, WITHHELD → M2-OUTSIDE-WINDOW; bounded package,
kind 1/2, known ascending required packages → M2-POLICY-INVALID; exact tree
depth and valid leaf proof → M2-MEMBERSHIP-INVALID. Anyone may reveal.
Success M2-PROBLEM-OPENED opens
`problem_id=SHA256("BOOLE-M2/PROBLEM/v2" | u64(chain_id) | pool_root |
u32(leaf_index) | instance_seed | u32(epoch))`, initially epoch=0, at h_v.
Commits are accepted only in (h_v,h_v+SOLVE_DEADLINE]. After the inclusive
h_d+1+REVEAL_GRACE deadline, the next height marks an unrevealed round WITHHELD,
burns its leaf and schedules end+ANNOUNCE_GAP.

**M2-R27 — Lifecycle.** Each slot cycles IDLE→DRAWN→OPEN→IDLE. Deadline at
h_v+SOLVE_DEADLINE+1 closes the problem, emits M2-PROBLEM-DEADLINE, and schedules
end+ANNOUNCE_GAP. Closure prevents new commits but leaves existing reveal
windows and CHECKING records intact. Barriers are per problem. A future STALE
re-pose queue is consumed by the lowest scheduled IDLE slot, using the same
leaf and seed, epoch+1 and no reveal. Pools and identities bind the network.
Public catalog templates must enter pools through new unpublished instances or
draw-instantiated generators; the generator runs outside derivation.

**M2-R28 — Root.** Start root0=zero32. At height h:
`root_h=SHA256("BOOLE-M2/ROOT/v1" | root_(h−1) | u64(h) | L1[h].hash | SHA256(E_h))`.
E_h concatenates records in R6 order, without lengths or separators:

- Every F0-valid kind 02–0f log: `01 | txIndex:u64 | logIndex:u64 | kind:u8 | code:u8`.
- Height-event status change: `02 | problem_id:32 | commit_hash:32 | status:u8`;
  statuses COMMITTED=1, OPEN=2, CHECKING=3, EXPIRED=4, CANCELLED=5.
- Draw, withheld, deadline: `03 | slot:u8 | round:u32 | event:u8 |
  leaf_index:u32 | problem_id:32` (zero ID when absent); event codes c1–c3.
- Prune: `04 | problem_id:32 | count:u32`.

F0 failures and kinds 00, 01 and ≥10 remain in M1 evidence. Root identifies
positions, not log bytes; D2 padding therefore has no effect.

## Future interfaces

M2-I1 rebuilds bytes by rescanning h_o..h_r, taking the first valid part per
index and rechecking B/body_digest. M2-I2 pins codec, export schema, deterministic
fuel/memory and two-kernel AND verdicts; submission faults reject, operator
faults defer locally, missing L1 data stalls. M2-I3 schedules bounded batched
verdict deposits by completion order and capacity. M2-I4 preserves the earliest
unresolved per-problem barrier. M2-I5 closes on settled ACCEPT. M2-I6 binds the
verdict statement to problem, environment, export, candidate, solver and gate
identities. M2-I7 reserves BOOLE-S1/M2ROOT for a future deposit before M3 verdicts;
until then root is evidence only. M2-I8 rechecks quarantined candidates at their
original position; CHECKING does not expire. M2-I9 derives counters only from
consensus state. No M3 verdict, mint, reward, gate execution, blob, escrow or
batcher policy is implemented by this version.
