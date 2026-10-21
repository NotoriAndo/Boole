# Boole derivation specification v0

Status: devnet only v0. Rules D1–D18 are normative within this version. Tests cite
these identifiers. The devnet conventions preserve S1b byte parity and are not
production choices.

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
`ITEM-KIND-NOT-ACCEPTED`. Kinds `0x02`–`0x0f` are reserved for M2.

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

**D12 — F5 gas limit.** gasLimit greater than the exact parent header gasLimit
drops `USER-GAS-EXCEEDS-PARENT-BLOCK-LIMIT`. Equality passes. Do not reserve the
sum of gas limits.

**D13 — F6 forwarding.** Otherwise use
`FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION`. Chain ID, nonce, funds, intrinsic gas,
fee caps and duplicates are left to the pinned client's sequential InvalidTx
skip. Apply F0–F6 in order; the first failure wins. Each dropped item is a
recorded no-op, without affecting other items.

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
state RPC or map iteration order enters derivation. Wall clock is only for JWT
iat and polling pace. Pure and I/O code reside in separate crates.

**D18 — Known client limitation (measured I3).** Both independent replicas
stalled at height 1 in each probe, with unchanged genesis heads. P1's single
transaction declared the parent limit, 30,000,000 gas; after the L1-info deposit,
only 29,975,900 remained. P2's creation (`0x5b600056`, 16,000,000 gas) exhausted
its gas, leaving 13,975,900; the next transfer declared 15,000,000. All passed
D12. In both probes forkchoiceUpdatedV3 returned VALID and a payloadId, then
getPayloadV5 returned JSON-RPC error `-38001`, `Unknown payload` (HTTP 200).
The client's warning reported the declared gas exceeding remaining block gas.
No newPayload or selecting forkchoice call followed the failure.

In pinned op-reth v2.6.0 (`da6d3252491754837a778061db0cc47236ec13c6`),
`alloy-op-evm/src/block/mod.rs:813–833` validates against remaining gas, called
at line 951. This error is outside the per-transaction InvalidTx skip in
`execute_sequencer_transactions`; it aborts the payload build. The driver records
the Engine error and stalls. D12 remains unchanged: this is a devnet v0 liveness
limitation, not permission to pre-simulate, reserve aggregate gas, skip a height,
or drop additional user transactions.

Validation: pure tests cover all 18 S1b case variants, malformed encodings,
input errors, all 37 golden attribute sets and determinism. Real-client tests
reproduce all 37 recorded block hashes/roots/receipts and compare live A/B/C
replicas, including finalized-only waiting, restart and unfinalized reorgs.
Executable identities and genesis bytes are in `fixtures/devnet-m1/`.
