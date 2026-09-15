# Native transfer ledger contract — R1 foundation

Status: core accounting implemented, 2026-09-15. **Not a running transfer
network.** [Current development status](current-development-status.md) owns the
remaining R1–R3 work and operating boundaries.

## Approved scope

The operator approved the following after comparing the testnets of Iron Fish,
Aleo, Quai and Alephium:

- New base-testnet issuance goes entirely to the block producer's authenticated
  reward recipient. All transfer fees go there too; no fee burn.
- Legacy per-share development credits are not native coin issuance. Existing
  v3 blocks, hashes, genesis, evidence and replay remain unchanged. There is no
  balance migration or mainnet entitlement.
- Useful-work additional rewards remain OFF. U1/U2 still owns the separately
  gated receipt and economic experiments.
- The eventual acceptance test must mine a reward and spend it through an
  actual signed on-chain transfer. A faucet, participation point or verifier
  receipt is not that evidence.

This is a base-testnet scope decision, not final mainnet tokenomics or permission
to launch a public network. The accepted local ADR-0010/0011 structures remain
the account, nonce, fee and emission design inputs.

## Executable accounting boundary

[`NativeLedger`](../crates/boole-core/src/native_ledger.rs) starts at empty
height-zero genesis. Its immutable `EmissionSchedule` uses caller-supplied
structural parameters; **no named-network monetary preset is introduced**.
Future network integration must take those values from one code-pinned,
genesis-committed policy, never independently configurable node values.

`apply_block(height, authenticated_reward_pk, transfers)` assumes that the
caller has already validated block identity, linkage, PoW and producer reward
authorization. It is not a replacement for block validation or fork choice.
The ledger enforces:

- Consecutive accounting heights, beginning at 1; no genesis allocation.
- Strict, network-bound Ed25519 signatures from the debited account itself.
  There is no session delegation path. A work-session signature cannot debit
  the owner's balance. Existing `canTransfer=false` / `canWithdraw=false`
  signer policies and legacy signature consumers are unchanged.
- Payload schema `boole.transfer.v1`, with exactly `schema`, `from`, `to`,
  `amount`, `fee`, `nonce`, `validBefore`. The signature remains in the existing
  `boole.signed.v1` envelope; its signer must equal `from`.
- All payload integers are canonical decimal **strings**. Amount/fee are u128;
  nonce/height are u64. Signs, leading zeros, floats, exponents, unknown fields
  and out-of-range values are rejected. No client-side JSON float rounding is
  required to encode the full range.
- Nonces begin at 0 and are strictly sequential. `height <= validBefore` is
  accepted; the following height is expired. Amount must be positive.
- The full `amount + fee` is checked and debited before recipient/producer
  credits. All monetary arithmetic is checked, including overlapping accounts.
- Transfers execute in supplied order; ordinary received funds may be spent by
  a later transfer in the same block. Base issuance is applied last, so it
  cannot fund a transaction in that same block.
- `INITIAL_REWARD >> (height / HALVING_EPOCH)`, clipped to the remaining supply
  cap. Shifts beyond 127 emit zero, not a wrapped/restarted reward.
- Any failure leaves **the entire ledger unchanged**, including prior transfers
  in that block, fees, nonces, height and issuance.

The kernel currently credits issuance without an additional maturity lock and
allows zero fees. These are not an adopted public-network maturity/minimum-fee
policy: those choices must be resolved before integration, with matching replay
tests. The height-zero convention also means the first halving is at height
`HALVING_EPOCH`, not at `HALVING_EPOCH + 1`.

## Verification and limits

[`native_ledger` integration tests](../crates/boole-core/tests/native_ledger.rs)
exercise signed reward spending, block-atomic failure, cross-network/tampered/
session-signed/weak-key rejection, nonce/expiry/double-spend rejection, full-width
amounts, overlapping accounts, halving/cap exhaustion and independent accounting
reconstruction. Missing native accounting, unsupported transfer, partial block
mutation and weak-key spending were observed RED before their implementation
or correction. Existing behavior checks were allowed to pass immediately.

Focused compatibility checks cover v3 hashes, genesis commitment, replay
fixtures, signed envelopes/network binding and session policy. The historical
fixture regeneration helper stays ignored; no fixtures were rewritten.

Still required before R1 can pass:

1. Decide and pin test-only monetary values, denomination, minimum-fee and
   reward-maturity rules; define the new rule/network/genesis while preserving
   v3. This kernel is not an operational genesis or a new consensus activation.
2. Implement Hash-only base block construction/validation, transfer commitment,
   network propagation, bounded mempool/admission and owner-wallet/CLI flow.
3. Integrate canonical storage, recovery and replay/reorg. The tests here use
   in-memory accounting inputs, not a filesystem restart, mined PoW block,
   public P2P run or multi-node convergence result.
4. Exercise resource bounds and wallet safety, then run the closed-local
   mining → reward → A-to-B transfer → consistent balance/nonce/fee/supply path.
   Full-map staging in this initial kernel is not a scalability certification.

No VM/model run, wallet transaction, public network, useful reward activation
or new mainnet promise was executed by this milestone.
