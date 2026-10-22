# Boole derivation specification v0

Commit and reveal extend these rules in [specification v1](devnet-commit-reveal-v1.md).

Status: devnet only v0. Rules D1–D18 are normative within this version. Tests cite
these identifiers. The devnet conventions preserve the S1b byte layout; the
DEVNET-M1b cumulative budget changes forwarding. These are not production choices.

**D1 — Configuration (devnet only).** L2 chain ID is 720926 (`0xb001e`);
L1 is anvil chain 31337. Inbox is `0xb001000000000000000000000000000000000001`,
L1-info depositor is `0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001`, and L1Block
is `0x4200000000000000000000000000000000000015`. The fixture S1b genesis enables
all OP forks through Lagoon at time zero, with L1Block/message-passer STOP stubs
and zero fee scalars. L1 block zero anchors L2 genesis.

**D2 — Inbox (devnet only).** `post(bytes)` selector `0xbaf9b369` emits
`Posted(bytes)` with topic0 keccak256(`Posted(bytes)`). Only calldata is used.
Read logs by blockHash, inbox address and topic0; sort by (transactionIndex,
logIndex). The ABI data is word 32, length, item (and optional trailing padding).
An item is kind byte followed by body. Kind `0x01` carries one EIP-2718 user tx.

**D3 — Finality and restart.** L2 height h derives only from finalized L1 height
h, one block per height including empty blocks. Anvil finality is latest minus
twice slots-per-epoch. Start at node head + 1; there is no driver journal.
Do not read unfinalized L1 blocks.

**D4 — L1 link.** For h >= 2, L1[h].parentHash must equal the origin hash in the
parent L2 block's first L1-info deposit. A mismatch stalls; M1 has no recovery.

**D5 — Attribute mapping and canonical order.** Serialize a compact serde_json
object in exactly this field order:

| Field | Value |
|---|---|
| timestamp | max(L1.timestamp, checked parent.timestamp + 1), hex quantity |
| prevRandao | L1.mixHash |
| suggestedFeeRecipient | parent.miner |
| withdrawals | [] |
| parentBeaconBlockRoot | L1.parentBeaconBlockRoot |
| transactions | L1-info deposit followed by forwarded inbox txs, unchanged |
| noTxPool | true |
| gasLimit | parent.gasLimit, hex quantity |
| eip1559Params | parent.extraData[1..9], hex bytes |
| minBaseFee | parent.extraData[9..17], big-endian u64 JSON number |

Parent extraData must have 17 bytes and version byte `0x01`. L1 height must equal
checked parent height + 1. SHA-256 of these canonical bytes is the digest.

**D6 — L1-info bytes.** Deposit is `0x7e || rlp([sourceHash, depositor, L1Block,
0, 0, 1000000, 0, data])`. sourceHash is SHA-256(`BOOLE-S1/ || L1INFO || L1.hash
|| uint256(0)`). The 178-byte data is `3db6be2b`, 16 zero bytes, timestamp u64,
number u64, baseFeePerGas u256, zero u256, L1.hash, zero 32 bytes, 14 zero bytes.
All numeric binary fields are big-endian.

**D7 — F0 envelope and input consistency.** Bad offset (word0 != 32), fewer
than 64 bytes, or 64 + length beyond data drops `INBOX-ENVELOPE-INVALID`.
Wrong address, topic0 or blockHash, removed logs, duplicate index pairs, malformed
input fields, timestamp overflow and invalid extraData are input errors and stall.

**D8 — F1 kind.** Anything other than `0x01` (including an empty item) drops
`ITEM-KIND-NOT-ACCEPTED`. M2-R1 in specification v1 assigns and reserves kinds `0x02`–`0x0f`.

**D9 — F2 type.** Any body not beginning with `0x02` drops
`USER-TYPE-NOT-ALLOWED`: empty, legacy, types 1, 3, 4, unknown and user type 0x7e.

**D10 — F3 encoding/signature.** Failure drops
`USER-DECODE-OR-SIGNATURE-INVALID`. RLP uses shortest length forms, no wrapped
single byte below 0x80, no leading zero in a length, exact list bounds, no trailing
bytes, and depth <= 8. Exactly 12 fields are required. Integers have no leading
zero: chainId/nonce/gasLimit <= 8 bytes, fees <= 16, value/r/s <= 32. Destination
is empty or 20 bytes, data is bytes, access list is a list of [20-byte address,
list of 32-byte keys]. Parity is 0 or 1, 0 < r < secp256k1 order n, 0 < s <= n/2.

**D11 — F4 sender.** Recover over keccak256(`0x02 || rlp(fields[0..9])`), the
first nine fields, excluding parity/r/s. Failure
drops `USER-SENDER-UNRECOVERABLE`.

**D12 — F5 cumulative gas budget.** Initialize `budget = parent.gasLimit -
L1INFO_GAS`, with checked subtraction and `L1INFO_GAS = 1,000,000`, the L1-info
deposit's declared gas limit. Underflow is an input error. Process F4-successful
candidates in inbox order. If `candidate.gasLimit > budget`, drop with
`USER-GAS-EXCEEDS-BLOCK-BUDGET` and consume no budget. Otherwise checked-subtract
the candidate's full gas limit and forward it. Equality passes. Reserve this
gas even when the client later skips a forwarded transaction for nonce, funds,
intrinsic gas, fees or another InvalidTx error. No state RPC enters this rule.

Sufficiency for the client's block-gas check: write B for parent.gasLimit, S for
L1INFO_GAS and g_j for earlier forwarded gas limits. The budget invariant is
`S + sum(g_j) <= B`. Cumulative pre-refund gas actually used A is at most
`S + sum(g_j)`, since each execution consumes at most its declared limit and
skipped transactions consume none. A newly forwarded g_i satisfies
`g_i <= B - S - sum(g_j) <= B - A`, exactly the client's remaining-gas check.
The system transaction also fits because initialization requires S <= B.
This closes the block-gas failure; other failure classes still follow D16.
Pinned op-reth v2.6.0 (`da6d3252491754837a778061db0cc47236ec13c6`)
checks remaining pre-refund gas in `alloy-op-evm/src/block/mod.rs:813–833`,
called at line 951. Its `execute_sequencer_transactions` treats this error as
fatal rather than InvalidTx.

**D13 — F6 forwarding.** Otherwise use
`FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION`. Chain ID, nonce, funds, intrinsic gas,
fee caps and duplicates are left to the pinned client's sequential InvalidTx
skip. Apply F0–F6 in order; the first failure wins. Each dropped item is a
recorded no-op consuming no budget; later candidates retain inbox order.

**D14 — System namespace.** Type 0x7e and the depositor belong only to the driver.
System deposits use consensus data only. Source domain L1INFO is implemented;
VERDICT (M3) and bridge DEPOSIT are reserved. M3 verdicts will follow L1-info and
precede users. No verdicts, bridge deposits or blobs exist in M1.

**D15 — Engine sequence.** (1) forkchoiceUpdatedV3 with head/safe/finalized all
parent.hash and attributes must return VALID plus payloadId. (2) getPayloadV5.
(3) Its first transaction must be the L1-info deposit, and the transaction list
must be an order-preserving subsequence of attributes.transactions, counting
duplicates. (4) newPayloadV4(payload, [], parentBeaconBlockRoot, []) must return
VALID. (5) forkchoiceUpdatedV3 with all hashes equal blockHash and null attributes
must return VALID. (6) Node head must equal blockHash at h.

**D16 — Failure classes.** Item failures drop. Everything else stalls at that
height and is recorded: input inconsistency, RPC error, L1 hash changed between
reads, link mismatch, Engine error/non-VALID, failed payload check. Never skip
a height or rebuild it differently after a failure.

**D17 — Determinism.** Derivation is a pure function of L1 fields, inbox logs,
parent fields and constants. No mempool, time, randomness, environment, filesystem,
state RPC, node-local data or map iteration order enters derivation. Wall clock is only for JWT
iat and polling pace. Pure and I/O code reside in separate crates.

**D18 — Earlier liveness limitation, fixed in DEVNET-M1b.** A single user
transaction could stop block production; closed by the cumulative budget.
The earlier I3 probes stalled both replicas with getPayloadV5 error `-38001`,
`Unknown payload`: P1 declared 30,000,000 against 29,975,900 remaining gas;
P2 declared 15,000,000 after a 16,000,000-gas creation, with 13,975,900 remaining.
With D12, P1 is dropped and produces a system-only block. P2 forwards the
creation (`0x5b600056`, consuming all 16,000,000 gas, receipt status 0) and drops
the following 15,000,000-gas transfer. A/B produce identical blocks without a stall.

History: DEVNET-M1b replaces `USER-GAS-EXCEEDS-PARENT-BLOCK-LIMIT` and its
parent-limit-only check. S1b case 9 originally included three 12,000,000-gas
transactions in a 30,000,000-gas block. D12 forwards the first two and drops the
third, leaving subsequent higher-nonce transactions to the client's skip.
Original S1b fixtures remain unchanged. The separate
`s1b-recorded-main-budget-v1.json` binds new expectations to original L1 data,
recorded A/B/C replay and live A/B/C acceptance.

Validation: pure tests cover all 18 S1b case variants, malformed encodings,
input errors, all 37 golden attribute sets and determinism. Real-client tests
reproduce the original main heights 1–23 and both basefee heights exactly;
main heights 24–35 use the accepted budget fixture. Live A/B/C comparisons
include finalized-only waiting, restart, unfinalized reorgs and advancing one
more finalized height after the reorg. I3 now produces identical blocks.
Executable identities and genesis bytes are in `fixtures/devnet-m1/`.

Known coverage gap: after the case 9 drop at main height 24, the sender has a nonce gap, so every user
transaction at heights 25–35 is skipped by the client. The real-client runs therefore no longer exercise
S1b cases 10 (same-block duplicate) and 13 (included execution failure); the I2 case 13 balance check is
vacuous. I3 P2 still covers an included status-0 transaction. Re-recording a budget-compliant S1b schedule
is a follow-up. D12 also relies on the devnet L1Block having zero DA-footprint scalars, so the client's
DA-footprint check cannot fail a block.
