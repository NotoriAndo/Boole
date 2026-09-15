# Native test-coin transfer contract — R1 closed-local integration

Status: versioned native blocks, durable node, bounded loopback RPC and encrypted
owner-wallet CLI implemented and tested, 2026-09-15. **R1 is not complete and no
public network has launched.** [Current development status](current-development-status.md)
owns the remaining security/transport/operations work and operating boundaries.

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

The operator then selected a **1,000,000,000-coin testnet cap**. This is not final
mainnet tokenomics or permission to launch a public network. The accepted local
ADR-0010/0011 structures remain the account, nonce, fee and emission design inputs.

## Code-pinned test-only policy

[`native_testnet()`](../crates/boole-core/src/native_network.rs) is the only
constructor for the new network policy; operators cannot deserialize or override
individual values. All policy fields are committed in a domain-separated native
genesis hash. The old `boole-testnet-2` genesis and v3 rules are unchanged.

| Parameter | Native testnet value |
|---|---|
| Network / genesis schema | `boole-native-testnet-1` / `boole.native.genesis.v1` |
| Symbol / precision | `tBOOLE` / 8 decimals; 100,000,000 atoms per coin |
| Supply ceiling | 1,000,000,000 tBOOLE = `100000000000000000` atoms |
| Initial block issuance | 50,000 tBOOLE = `5000000000000` atoms |
| Halving epoch | 10,000 blocks; first halving at height 10,000 |
| Minimum transfer fee | 0.00001 tBOOLE = `1000` atoms; no burn |
| Base reward maturity | Spendable at creation height + 10 |
| Genesis issuance | 0; first payable block is height 1 |
| Hash target / adjustment | Initial and easiest target `0000` + 60 `f` digits; 60-second target interval; retarget every 2 blocks, bounded by factor 4 |
| Useful-work additional reward | OFF |

The one-billion value is a **ceiling**, not a promise to mint every atom.
With no height-zero payout and integer-floor halvings, this exact schedule ends
at **999,949,999.9986 tBOOLE**. The full emission walk is tested through height
430,010; issuance is already zero at 430,000. A 60-second target is not a measured
block cadence or a public-network security claim. These fast retarget settings
are experimental, not adopted mainnet economics.

## Executable accounting boundary

[`NativeLedger`](../crates/boole-core/src/native_ledger.rs) starts at empty
height-zero genesis. Its fixture constructor still accepts structural parameters;
the actual native chain only uses the immutable genesis-committed policy above.

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

Locked rewards are part of the total balance but not the spendable balance.
Maturity unlocks at block entry and rolls back with the whole block on failure.
Transfer fees and ordinary receipts are immediately spendable. A mempool view
reserves the next block's available balance/nonce without creating issuance or
crediting an unknown producer's fees. It is not convertible to canonical state.

## Blocks, persistence and recovery

[`NativeChain`](../crates/boole-core/src/native_chain.rs) validates actual bounded
BLAKE3 PoW, strict producer authorization, reward-recipient binding, ordered
signed-transfer commitment, network/genesis, height/linkage, recomputed target and
median-time-past. Fork choice uses independently recomputed cumulative work, then
the lower head hash. The core has no wall clock; node admission, branch import
and boot enforce the existing two-hour future-drift guard.

[`NativeNode`](../crates/boole-node/src/native_node.rs) exclusively owns its state
directory and two NDJSON journals. It verifies manifest identity and replays every
block from empty genesis; no declared balance/checkpoint is trusted. Successful
block/transaction responses follow durable publication. A partial append is
rolled back or fenced; uncertain writes require restart/recovery. Live file
loss/replacement or lock loss stops readiness and mutations. Metadata checks are
not protection against a malicious local administrator racing the filesystem.

On reorg the node first saves the old pending queue plus bounded orphaned
transactions, then atomically replaces the block journal, then filters the queue
against the winning ledger. Boot uses whichever canonical block file survived.
Confirmed, expired, conflicting, underfunded or excess rows are removed from the
non-authoritative pool; invalid stored signatures are corruption, not silently
erased. Requeue is bounded/best-effort, so the wallet must retain its signed file.
Confirmation depth is reported but never labeled irreversible finality.

## Owner-wallet CLI and closed-local RPC

The foreground server is `boole-node run-native-local --state-dir <new-directory>
--addr 127.0.0.1:8383`. The default legacy `run-local` explicitly refuses the new
native network ID. No verifier bypass or legacy credit migration is involved.

The umbrella CLI uses `boole native --node http://127.0.0.1:8383` followed by:

- `info`, `account --pk <key>` or `transaction --txid <id>`.
- `mine --vault <owner-vault> [--reward-to <key>] [--attempts <bounded-count>]`.
- `transfer --vault <owner-vault> --to <key> --amount <decimal-tBOOLE>
  --outbox <new-file>`; optional `--fee` and inclusive `--valid-before` height.
- `submit --file <saved-transfer>` to retry **the exact signature and nonce**.
- `sync --from http://127.0.0.1:<other-port>` for bounded full-chain import.

Existing `boole wallet init/address` manages the AEAD vault. Native signing uses
the wallet-agent process, with `BOOLE_WALLET_PASSPHRASE` passed on stdin to that
process, never argv; no raw seed/session-key flag is added. Native monetary
amounts are parsed without floating point, and the CLI verifies the complete
compiled network policy before signing or submitting. The exact signed transfer
is atomically created as a new mode-0600 outbox file and fsynced with its directory
**before broadcast**. Existing files are never replaced. After an ambiguous
response, query the saved transaction ID and retry that file; do not create a new
nonce as an automatic retry. Keep the encrypted vault, passphrase and outbox
backup separately; losing the vault/passphrase means this implementation cannot
recover the spending key. These are owner commands, not delegated agent rights.

[`native_http`](../crates/boole-node/src/native_http.rs) is numeric loopback only.
It rejects foreign Host, Origin/browser fetch headers, has no CORS policy,
closes connections, bounds body streams and worker/connection counts, and reuses
the header deadline. The client refuses DNS names, public IPs, proxies, redirects,
credentials and URL paths. Plain HTTP here is **not public transport security**.

| Resource | Bound |
|---|---|
| Consensus transfer / block | 4,096 / 524,288 bytes; at most 512 transfers per block |
| Pending queue / reorg recovery union | 512 / 1,024 transfers |
| Local block journal | 256 MiB and 100,000 blocks; fail closed, no automatic prune |
| Pending journal | 5 MiB; pre-read bound also applies during recovery |
| RPC full-chain import | 8 MiB and 1,024 blocks; entire candidate validated before adoption |
| Native workers / request deadline | 8 work permits / 10 seconds; timed-out work retains its permit until done |
| Single CLI mining attempt | 1–10,000,000 hashes, then return; no unlimited loop |

Full-map staged accounting and full-chain sync are a deliberately bounded local
prototype. Automatic authenticated peer propagation, incremental catch-up and
production-scale state storage are not implemented by this milestone.

## Verification and limits

[`native_ledger` integration tests](../crates/boole-core/tests/native_ledger.rs)
exercise signed reward spending, block-atomic failure, cross-network/tampered/
session-signed/weak-key rejection, nonce/expiry/double-spend rejection, full-width
amounts, overlapping accounts, halving/cap exhaustion and independent accounting
reconstruction. Missing native accounting, unsupported transfer, partial block
mutation and weak-key spending were observed RED before their implementation
or correction. Existing behavior checks were allowed to pass immediately.

Added executable evidence:

- [Compiled monetary policy](../crates/boole-core/tests/native_testnet.rs):
  maturity and minimum-fee rejection, atomic unlock rollback, full issuance walk.
- [Actual native blocks](../crates/boole-core/tests/native_chain.rs):
  mined/signed reward spending, independent replay, tamper/retarget/time rejection,
  heavier-shorter branch and deterministic equal-work tie.
- [Durable node](../crates/boole-node/tests/native_node.rs) and its append-fault
  unit test: restart, duplicate submission, reorg/orphan requeue, future guard,
  file replacement/loss, pre-read byte bounds and failed-publication fencing.
- [Two independent RPC nodes](../crates/boole-node/tests/native_http.rs): real
  PoW → reward maturity → A-to-B signed transfer → block import → identical
  balances/head/issuance, with public-bind, cross-origin, oversized-body and
  stale-readiness rejection.
- [Encrypted-vault CLI](../crates/boole-cli/tests/native_cli.rs): actual server
  and wallet-agent processes, signing/mining/transfer, saved-file retry before
  and after inclusion, exclusive outbox publication and second-node sync.

Meaningful RED observations included immature/underfee spending, missing target
adjustment/MTP checks, stale-state appends, oversized-file recovery, legacy
network impersonation and missing RPC/CLI behavior. Existing safe behavior was
allowed to pass new regression tests immediately. Compatibility gates preserve
v3 hashes/genesis/replay and session restrictions; no legacy fixture was rewritten.

These runs use disposable local directories and test keys. They are not an
operator wallet movement, public mining result or mainnet entitlement. No new
VM/model execution, useful-work reward or public activation occurred. R1 still
requires authenticated/encrypted transport, broader recovery/abuse/operations
acceptance and the selected public-facing product scope before R2/R3 launch review.
