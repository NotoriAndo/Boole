# Devnet commit/reveal v1.2: the M2 root in every L2 block

This is a devnet reset from genesis, with no activation height or state migration.
It supersedes [v1.1](devnet-commit-reveal-v1.1.md) for the current devnet. The v1.1
candidate, slot, pool, package and root-fold rules remain unchanged. Older genesis
and recording files stay frozen. These are closed-local transport rules, without
kernel certification, verdicts, minting or public-network activation.

## M2ROOT system deposit (M2-I7)

Every current L2 block at height h carries two mandatory system deposits, in this
order: L1-info, then M2ROOT, then any forwarded user transactions. The committed
value is `m2_root_h` after folding the finalized L1 block h. The L1 origin number
and L2 block number are equal in this devnet. Empty inbox blocks still carry it.

The deposit is type `0x7e` with the same eight-field canonical RLP layout as
L1-info:

| Field | Exact value |
|---|---|
| sourceHash | SHA-256(`BOOLE-S1/M2ROOT` || origin_hash || 32 zero bytes) |
| from | DEPOSITOR, `0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001` |
| to | M2_ROOT_SINK, `0x42000000000000000000000000000000000000b2` |
| mint, value | 0, 0 |
| gas | M2ROOT_GAS = 1,000,000 |
| isSystemTransaction | false, encoded as the empty RLP byte string |
| data | `0x6d32726f` || uint64 big-endian origin number || bytes32 m2_root_h |

Data is exactly 44 bytes. The new genesis predeploy has exactly `0x00` (STOP)
code and zero balance. The source domain is separate from L1-info and future
VERDICT deposits. [The independent vector](../fixtures/devnet-m2/root-system-v1.2.json)
pins its exact bytes; `python3 -I scripts/devnet/generate_m2root_vector.py`
reproduces the vector using only Python's standard library.

The strict decoder checks type, canonical RLP, field count, leaf fields, the tag,
exact data length, height, origin-bound source hash, addresses, zero mint/value,
gas and the system flag. Import additionally compares the decoded root with the
local finalized fold. Missing, reordered, malformed and divergent commitments
are rejected before Engine import or fork choice. The driver returns typed
`StallKind::M2Root { height }` and stays stopped; it does not adopt that block or
retry it in a crash loop. Payloads built by a local Engine pass the same check as
external payloads passed to the driver's `import_payload` interface. The Engine
is the pinned unmodified execution client; the driver owns this derivation and
import check.

On restart, the driver reads both system transactions from each existing L2
block and re-folds the L1 inputs anchored by its L1-info origin. It compares each
M2ROOT before adopting the recovered fold or writing a checkpoint, and names the
first mismatching height. Checkpoints do not replace replay. Transaction bytes
read through RPC must match the transaction hashes in the block. Header
extension fields cannot substitute for an actual system transaction; missing or
malformed M2ROOT bytes still produce the typed height error.

## Gas reservation and S4

The user transaction budget is now
`parent.gasLimit - L1INFO_GAS - M2ROOT_GAS`. Forwarded candidates consume their
full gas limits in the existing F6 order, even if execution later omits them.
There is no refund to a later candidate. A system deposit above the remaining
block gas is fatal, as for L1-info; the derivation refuses an insufficient system
reservation before building a block.

S4 becomes:

```text
L1INFO_GAS + M2ROOT_GAS + Q_MAX * VERDICT_GAS + 16,777,216 <= L2 gasLimit
```

All additions and multiplication are checked. S3 remains
`C_COMMIT <= Q_MAX * V_PER_DEPOSIT`. At the current devnet values S4 needs
26,777,216 gas; the configured 30M block limit satisfies it. No VERDICT deposit is
implemented by this change.

The archived `derive_m1` and stateless `derive` interfaces preserve the M1
fixture contract and cannot produce current L2 blocks. Current stateful callers
use `derive_with_m2`, including the driver. The M2ROOT reservation and deposit
apply even when no M2 inbox item appears in a current block.

## Miner observation and reset recordings

The devnet miner client reads the L2 block at its finalized fold height and
reports `l2_m2_root_match`. True means its root agrees with the strict decoded
commitment and L1 origin; false is a hard mismatch that stops posting. Null means
that height is not yet available on L2 (or height zero has no deposit). The
worker waits for a match before posting, while status and private progress stay
inspectable. RPC-supplied finality is still the local trust assumption; this is
not an Ethereum consensus proof or RPC quorum.

The [fixture index](../fixtures/devnet-m2/README.md) lists the v1.2 genesis and
recordings. A/B/C comparisons include each L2 block hash, its embedded M2 root
and the independent pure fold. They cover a complete round, mid-round restart,
replay and finality/reorg behavior. A test-only mutation omits one COMMIT from
one replica's fold: that replica must refuse the canonical peer payload at the
first affected height while the others continue. Restart from the unchanged
stored head restores the correct fold and imports matching commitments.

The service parity generator is tracked separately at
`scripts/devnet/generate_service_vectors.py`. Its move changes only the fixture's
generator provenance hash; every existing case and every other field is kept
identical. It is an explicit regeneration procedure, not a slow self-test stage.
