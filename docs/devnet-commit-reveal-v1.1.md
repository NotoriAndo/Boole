# Devnet commit/reveal v1.1

**Superseded for the current devnet by [v1.2](devnet-commit-reveal-v1.2.md).**
The v1.1 recordings remain frozen historical evidence.

This specification replaces v1 on a devnet recreated from genesis. There is no
activation height or conversion of v1 state. [M1](devnet-derivation-v0.md) and the
[v1 rules](devnet-commit-reveal-v1.md) remain the base except where replaced below.
COMMIT, OPEN, PART, CANCEL, commitment and environment hashes, keyed part leaves,
R16 priority, drop codes, gas budgeting and finalized-only input rules stay intact.
POOL and PROBLEM_REVEAL use version 2; other assigned kinds use version 1.
A v1 POOL or PROBLEM_REVEAL drops `M2-DECODE-INVALID`.

Derivation never decompresses, executes a gate, reads a transaction sender,
receipt, drand or beacon data. M2 items are never forwarded or charged to the
M1 user gas budget. Gate, batcher and holder policy below is not consensus code.

## M2-R30: slot configuration and startup

`SLOT_TABLE` rows are indexed 0 through n−1. A row is
`lane:u8 | SOLVE_DEADLINE:u64 | ANNOUNCE_GAP:u64 | REVEAL_GRACE:u64`.
`K_OPEN = n`; there is no separate configuration field for it or global windows.
The fixed lane mapping is lane 1 → export_format 1, lane 2 → export_format 2.
A lane may appear in more than one slot.

| Slot | Lane | Devnet SD / GAP / GRACE | Testnet and production SD / GAP / GRACE |
|---|---:|---|---|
| 0 | 2 | 48 / 4 / 4 | 1,800 / 300 / 25 |
| 1 | 1 | 96 / 8 / 4 | 7,200 / 3,600 / 25 |
| 2 | 2 | absent | 1,800 / 300 / 25 |

Slot 0 supplies catalog ratchets, slot 1 theorem problems, and slot 2 derived
successor pools on testnet. Pool content classification is holder policy; the
consensus checks a pool's slot and the problem's lane only. Global D, W,
POOL_LEAD, C_COMMIT and all unchanged profile values retain their v1 definitions.
`STATEMENT_MAX = 122,880` carried bytes; `STATEMENT_RAW_MAX = 1,048,576`;
`SLOT_LEAVES_MAX = 1,048,576`. SOURCE_MAX is a gate constant, also 1,048,576.

S1–S4 remain unchanged. Startup refuses these additional violations, returning
the first applicable S number:

- **S5:** the existing largest-PART bound and
  `68 + pad32(1 + 54 + 32·len(PKG_TABLE) + 32·ceil(log2(POOL_MAX)) + STATEMENT_MAX)`
  must fit `pool_limit`. `pad32(x) = 32·ceil(x/32)`. Required packages are not
  limited by the OPEN composition cap; their encoded table and item size bound them.
- **S6:** 1 ≤ n ≤ 3, every lane is 1 or 2, and the existing environment/table
  validity conditions hold.
- **S7:** each SD, GAP and GRACE is in [1, 2^32]; POOL_LEAD ≥ 1.
- **S8:** 1 ≤ STATEMENT_MAX ≤ STATEMENT_RAW_MAX ≤ 2^27 and
  POOL_MAX ≤ SLOT_LEAVES_MAX ≤ 2^20.

Unknown config or slot-row fields are refused, including legacy `k_open`,
`solve_deadline`, `announce_gap` and `reveal_grace` at the top level.

The slot encoding is `n:u8 | n × (lane:u8 | SD:u64be | GAP:u64be | GRACE:u64be)`.
`slot_table_digest = SHA256("BOOLE-M2/SLOTS/v1" | encoding)` is printed at startup
and recorded on every driver evidence line, including waits and stalls.

## M2-R24: POOL v2

The 147-byte body is:

| Offset | Field |
|---:|---|
| 0 | version:u8 = 2 |
| 1 | slot:u8 |
| 2 | pool_seq:u32be |
| 6 | pool_root:32 |
| 38 | leaf_count:u32be |
| 42 | start_height:u64be |
| 50 | battery_digest:32 |
| 82 | sig:65 |

The low-s secp256k1 signature uses parity 0/1 and signs
`SHA256("BOOLE-M2/POOL/v2" | L2_CHAIN_ID:u64be | body[0..82])`.
Checks in order:

1. Exact length and version, else 0x01.
2. Recover POOL_KEY under this domain, else 0x0D.
3. slot < n; one global sequence `last + 1`; 1 ≤ leaf_count ≤ POOL_MAX;
   start ≥ h + POOL_LEAD; the sum of unused leaves of all recorded pools of
   this slot plus leaf_count ≤ SLOT_LEAVES_MAX, else 0x0D.
4. Root not previously recorded in any slot, else 0x0A.

Success is 0x85. Record slot, root, start, count and unused indices 0..count−1.
Only the named slot is touched. If idle and unscheduled, schedule its first draw
at start_height. battery_digest is evidence. Neither derived-pool classification
nor per-key admission quotas enter derivation.

## M2-R25: union draw

An idle slot scheduled at h_d draws at the start of h_d+1, after its own K3
re-pose head if one exists. Otherwise take all pools of this slot with
start_height ≤ h_d and nonempty unused sets, ordered by pool_seq. Empty union:
wait and set h_d to the current height for a retry at the next height.

`seed = SHA256("BOOLE-M2/DRAW/v2" | L2_CHAIN_ID:u64be | slot:u8 | round:u32be |
L1[h_d+1].mixHash)`; round is the next round number. Pool roots are absent
from this seed. Let N be the total unused leaves and
`j = u64be(seed[0..8]) mod N`. Select the j-th `(pool_seq, leaf_index)` pair
in lexicographic order and remove it, without replacement. Thus pools with
unused sets 1:{0,2,5} and 3:{1} map j=2 to (1,5), j=3 to (3,1).
Unstarted pools and other slots never enter the union. Record pool_seq,
leaf_index, seed, h_d and DRAWN; slots are processed in slot order.

## M2-R26 and R32: carried statement packages

PROBLEM_REVEAL v2 is:

`version:u8=2 | slot:u8 | round:u32be | leaf_index:u32be | leaf_kind:u8 |
req_k:u8 | req_pkg:32·req_k | leaf_salt:32 | pkg_codec:u8 | pkg_raw_len:u32be |
depth:u8 | path:32·depth | package_len:u32be | package`.

Body length is `54 + 32·req_k + 32·depth + package_len`. The package field is
the transmitted byte sequence. Leaf v2 is

`SHA256(0x00 | "BOOLE-M2/POOL-LEAF/v2" | L2_CHAIN_ID:u64be | leaf_kind:u8 |
req_k:u8 | req_pkg | pkg_codec:u8 | pkg_raw_len:u32be | SHA256(package) | leaf_salt)`.

Checks in order:

1. Exact layout and version, else 0x01.
2. Round and leaf index match, else 0x0E; OPEN/CLOSED → 0x0A;
   WITHHELD → 0x09.
3. leaf_kind is 1 or 2; required packages are strictly ascending and in
   PKG_TABLE; codec is 0 or 1; carried length ≤ STATEMENT_MAX. Codec 0 requires
   raw length = carried length. Codec 1 requires nonempty carried bytes and
   1 ≤ raw length ≤ STATEMENT_RAW_MAX. Failure → 0x02.
4. Depth is ceil(log2(count)) for the round's pool, and the v2 leaf path reaches
   its root, else 0x0E.

Success 0x86 retains the existing problem_id formula, using the selected pool
root, index, instance seed and epoch. The problem records its slot.

Codec 0 carries the package directly. Codec 1 uses the existing codec-1 frame
contract: one zstd frame, no skippable/trailing frame, content size present and
matching pkg_raw_len, window ≤ pkg_raw_len ≤ 2^27, no dictionary, valid checksum
and pinned decoder. There is no package compression-ratio cap. Derivation checks
only ids, lengths and membership of the transmitted bytes. The gate and MCP
perform decoding; failure is an operator fault that voids the problem's
candidates without penalty and closes it.

## M2-R7, R8, R23 and R27: lane and lifecycle

R23 row 5 permits export_format 1 or 2; codec remains 1 and deps_len/ext_len
remain zero. R8 checks required-package coverage after candidate lookup, then
requires export_format = the problem slot's lane format. A mismatch returns
0x02 before the reveal-window check. Its commitment stays live and can be
cancelled; B binds the wrong format, so it cannot later open correctly.
Derivation reads no bundle payload or header.

COMMIT is late after h_v + SD[problem.slot]. A round closes at h_v + SD_s + 1,
is withheld after h_d + 1 + GRACE_s, or ends by settlement; schedule the next
draw at end + GAP_s. The M3 K3 re-pose queue is per slot. A queued STALE problem
reopens only in its own slot, at epoch+1, retaining its original pool_seq,
leaf_index and instance_seed, without another reveal. The queue is an M3 seam;
ordinary unsolved re-posing below is pool-holder policy.

## M2-R28: config-seeded root v2

Normalize the validated Config into compact JSON in this field order:

`chain_id, d, w, w_slack_min, p_blk, ratio_max, k_max, c_commit, q_max,
v_per_deposit, verdict_gas, l1_gas_limit, pool_limit, slot_table,
statement_raw_max, slot_leaves_max, pool_lead, pool_max, statement_max,
trace_embargo_max, schema_id, base_root, pkg_table, env_allowed, incompatible,
terms_table, pool_key`.

Slot rows use `lane, solve_deadline, announce_gap, reveal_grace` order. Integers
are decimal; hashes/addresses are arrays of byte integers. Sets are arrays in
lexicographic byte order; terms_table uses decimal-string u8 keys in increasing
numeric order. There is no whitespace. The
[config vectors](../fixtures/devnet-m2/config-golden-v1.1.json) include exact
canonical bytes for all profiles.

`config_digest = SHA256("BOOLE-M2/CONFIG/v1" | canonical_config_json)`;
`m2_root_0 = config_digest`.

`m2_root_h = SHA256("BOOLE-M2/ROOT/v2" | m2_root_(h−1) | h:u64be |
L1[h].hash | SHA256(E_h))`.

Round record: `0x03 | slot:u8 | round:u32be | event:u8 | pool_seq:u32be |
leaf_index:u32be | problem_id:32`, 47 bytes. K3 uses the original pool_seq.
Other event records and cross-problem ordering are unchanged. The driver retains
the v1 checkpoint, anchored re-fold, parentHash validation and stall/recovery
contract; checkpoint config identity includes the new table. Roots stay evidence
until the future M2ROOT deposit interface is implemented.

## M2-R31: format-2 bundle contract for the gate

This section defines the shared wire contract; it adds no derivation parser.
The decompressed export is a two-entry container:

| Offset | Field | Rule |
|---:|---|---|
| 0 | magic:8 | BOOLE-RB, hex 424f4f4c452d5242 |
| 8 / 9 | bundle_version:u8 / count:u8 | 2 / 2 |
| 10 + 42i | role:u8, format:u8, len:u64be, digest:32 | roles (2,3), format 1 each; len ≥ 1; SHA256(payload) |
| 94 | payloads | contiguous in table order; 94 + len0 + len1 = raw_len |

Role 2 is a lean4export 076e8e57 NDJSON delta under the canonical environment
schema, including Boole.Ratchet.Cand, its c⟨k⟩ chunks, claim and proof. Role 3 is
a canonical ustar of candidate files only: regular files sorted by path bytes,
mtime/uid/gid zero, mode 0644, no pax/GNU headers, no snapshot files. Its length
is at most SOURCE_MAX. Construction of the source archive is an MCP rule; the
gate checks its digest/length but never parses it. Bundle version 1, count 3
and role 1 are invalid.

`delta_digest` and `src_digest` are the two table digests, bound through
export_digest, body_digest and commit_hash. `cand_digest` is computed by C4
from typed Cand data, not a bundle header field.

Gate stages, first failure wins:

1. Codec, lengths and export digest → codec-invalid / digest-mismatch.
2. Bundle header/order/formats/lengths/digests/source cap → bundle-invalid.
3. C0–C3 delta decode, package/environment binding, policy/import/interface
   checks → export-parse / env-coverage / policy:<label>.
4. C4 declaration set, typed grammar, canonical literals, caps and literal-m
   claim shape (TERMS_MAX 32,768, 64 items per chunk, 4,096 per list) →
   data-form:<decl,grammar,literal,size>, statement-mismatch or
   policy:reserved-name.
5. N4 template closure + m + cand_digest identity → statement-mismatch.
6. N5 replay of environment, package and delta in two kernels under AND,
   typed fuel, memory and axiom audit → the existing kernel/fuel/memory reasons.

There are no native constraint parsers, metric-bound stage or CAND_GEN. The
kernel checks normal form, metric m within bound and equivalence over F_p with
Ref. Source changes with updated digests preserve verdict, m and cand_digest;
only src_digest changes. An off-chain pinned compile/renderer may publish
source-matched, source-mismatch or source-unverified. These have no consensus
or testnet reward effect.

Future M2-I6 lane-2 ACCEPT adds
`metric_schema:u8 | m_count:u8 | metric[m_count]:u64be | cand_digest:32 |
src_digest:32`. Schema 1 (R1CS) is [nl,lin,nl+lin]; schema 2 (ACIR) is
[priced,nl,rb,lb,mem,bb0…bb(K−1)], K≤250. Component 0 is priced. Values come
from the accepted claim type. This is an interface, not an M2 verdict implementation.

## Non-consensus holder and MCP policy

- **M2-P1:** each settled lane-2 ACCEPT in slot 0 or 2 yields a successor for
  slot 2: delta=max(1,ceil(m0/100)), bound=m0−delta, rung+1, record=m and
  predecessor=(problem_id,commit_hash). Keep the original reference/statement;
  other components follow the catalog rule. Publish floor, battery, size or
  flat when excluded, with the applicable battery/flattening rule.
- **M2-P2:** an unsolved deadline-closed leaf may return in its own slot with a
  fresh CSPRNG salt at most REPOSE_MAX=2 times. WITHHELD leaves are burned.
- **M2-P3:** derived pools contain successors or re-poses only; publish their
  full index/kind/required packages/codec/raw length/package/salt preimages at
  posting. Slot 2 holds derived leaves only.
- **M2-P4:** publish fresh-pool preimages when its last leaf is drawn, and each
  withheld leaf at round end.
- **M2-P5:** publish an append-only per-slot audit trail for ACCEPT metrics,
  delta, bound, successor pair or skip reason, and re-pose predecessor/pair.
- **M2-W7:** read slot/lane and choose its format before COMMIT; cancel a wrong
  format commitment immediately.
- **M2-W8:** for lane 2, use pinned compiler/extractor and canonical BooleCS
  emitter, evaluate m, prove/export against env_id, and pass gate stages locally.
  Include candidate source only, with no secrets or snapshot files.
- **M2-W9:** decode codec-1 problem packages under R32 before committing;
  report a broken package as an operator fault and do not commit.
- **M2-W10:** display lane, last commit height, pending commits and next draw
  per slot. The batcher's advisory checks include bundle framing and package decode.

The [fixture index](../fixtures/devnet-m2/README.md) identifies the reset recordings
and the archived v1 inputs. No production activation or gate/kernel certification
follows from these closed-local devnet results.
