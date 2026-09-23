# Native test-coin transfer contract — R1 closed-local integration

Status: native blocks, durable node, loopback RPC, encrypted owner-wallet CLI and
mutually authenticated/encrypted incremental peer sync implemented and locally
tested, 2026-09-23.
**R1 is not complete and no public network has launched.**
[Current development status](current-development-status.md) owns the remaining
security/recovery/operations work and operating boundaries.

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
- `peers` for configured transport identities, synchronization state and limits.
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

Full-map staged accounting and the manual full-chain RPC import remain bounded
local prototypes. Automatic incremental peer synchronization is described below;
production-scale storage and comprehensive operator recovery are not complete.

## Mutually authenticated native peers

The initial participant policy selected under the operator's September 23
autonomous-development delegation is an **explicit node-public-key allowlist**.
It is not permissionless enrollment or public-launch authority. The native
service refuses non-loopback listeners and peer endpoints; its executable
validates these options before opening mutable state. DNS, automatic discovery, proxying, plaintext
fallback and automatic trust-on-first-use are absent.

[`TlsTransport`](../crates/boole-p2p/src/tls.rs) uses TLS 1.3, ALPN
`boole-transport/1` and Ed25519 RFC 7250 raw public keys through rustls/ring.
Both parties must prove possession of an approved private key with the real
TLS handshake signature. Outbound trust pins the exact configured key for the
numeric endpoint; inbound trust checks the configured public-key set, not the
source port. System CAs, TLS 1.2, anonymous clients, 0-RTT, session resumption,
wallet/session delegation and peer-claimed chain work grant no authority.
Transport encryption does not hide endpoint/traffic metadata or turn local RPC
into a public API. The legacy `Frame` protocol remains version 4; the native
typed message schema is separately version 1.

The optional foreground arguments are:

```sh
boole-node peer-keygen --file <private-existing-directory>/node-a.pk8
boole-node run-native-local --state-dir <node-a-state> \
  --addr 127.0.0.1:8383 --p2p-addr 127.0.0.1:9393 \
  --peer-key <private-existing-directory>/node-a.pk8 \
  --peer <NODE_B_PUBLIC_KEY>@127.0.0.1:9394
```

Configure node B reciprocally with its own key, RPC/state directory and listener.
These are instructions for a separately chosen local rehearsal, not evidence of
an operator deployment. `peer-keygen` returns only the public key and file path.
The hot transport key is **unencrypted PKCS#8**, separate from the encrypted owner
vault and without a wallet spending/reward role. The file must remain a regular,
single-link, mode-0600 file of at most 4,096 bytes. Creation never overwrites an
existing path; loading uses one bounded no-follow/nonblocking descriptor and
checks identity/metadata again. Symlink, hard-link, FIFO, overpermissive key and
group/world-writable immediate parent paths are rejected. A failed creation may
leave a partial new file; it is not silently deleted. These checks do not defend
against a malicious local administrator or a compromised process account.

[`NativePeerService`](../crates/boole-node/src/native_peers.rs) shares the same
durable `NativeNode` and shutdown boundary as RPC. Each authenticated exchange
binds its version/network/genesis and advertised head. The head is only a
snapshot hint. Hash queries locate a common ancestor; each downloaded block
still passes the core's PoW, producer signature, linkage, target/time and ledger
validation. Server replies require the advertised local snapshot to remain
current. A changing snapshot causes a bounded retry, not silent mixing of forks.

Extensions are durably applied block-by-block and continue over multiple rounds;
a timeout may retain a valid prefix. A competing branch is adopted only after
its entire bounded suffix and advertised head are present and independent replay
wins work/tie-break. A fork suffix longer than 256 blocks is reported as
`bounded_reorg_requires_recovery`; it is not truncated into a winning chain.
Replay of the known prefix still uses the existing bounded full-replay node;
the suffix bound is not a constant-time or constant-memory reorg guarantee.
Large-state replay/storage optimization remains an R1 operational acceptance item.

When both heads match, each configured node periodically pulls the other's
bounded pending snapshot. Transfers pass the same signature and admission path
as RPC, with confirmed/pending duplicates idempotent. Valid but stale nonce,
funding or queue conflicts are benign rejections; invalid signatures abort the
round, and durability/ownership loss remains fatal to readiness. Gossip is
periodic pull, not instantaneous broadcast or a finality guarantee.

| Peer resource | Bound |
|---|---|
| Configured peers / outbound workers | 8 / 1 sequential worker |
| Inbound workers / handshake starts | 4 / 8 per second globally |
| Active incoming rounds per authenticated key | 1, with 500ms minimum start interval |
| TLS handshake | 2-second absolute I/O deadline and 64KiB encrypted-I/O budget |
| Native message / round plaintext bytes | 1MiB / 8MiB, counting both directions |
| Round / range | 64 requests, 256 downloaded blocks; page at most 16 blocks |
| Pending snapshot | 512 transfers; page at most 128 |
| Round I/O deadline | 10 seconds from completed authentication; not a hard real-time disk/CPU preemption guarantee |
| Outbound retry | 500ms on success; exponential 500ms–30s after consecutive failures |

TLS also caps encrypted overhead for every message. All limits are local
operational policy, not new consensus constants. Admission occurs before TLS
work; sockets are registered before handshake so shutdown interrupts stalled
I/O. Shutdown closes admission, waits for actual in-flight mutation completion
and joins workers before releasing state ownership. A timed-out RPC or network
caller cannot release a still-running mutation's permit.

`GET /native/peers` and `boole native peers` report the fixed configured identities,
last outbound state, retry counts/delay, active/peak inbound workers and aggregate
accept/reject/authentication/round counters. Labels cannot grow from arbitrary
remote input. Failure counters can include shutdown/interruption, not just
attacks. `snapshot_match` means one completed exchange matched; `catching_up`,
`retrying`, `local_chain_preferred` and `bounded_reorg_requires_recovery` must not
be presented as synchronized/finalized. The retry delay is the last selected
interval, not a live countdown. With P2P disabled the endpoint reports disabled.

### Local peer key change and recovery limits

- Allowlist edits and key rotation require a controlled restart; there is no
  hot trust update. Stop affected nodes, generate a distinct new transport file,
  exchange its public key through an authenticated administrative channel, update
  both endpoint pins and restart. Remove the old pin to revoke it. Do not rotate
  an owner vault to repair a transport connection, or print/send the private file.
- After connection loss or a small partition, keep both state directories and
  restart with the same approved pins. The automated path revalidates and rejoins;
  orphaned transactions are bounded/best-effort requeued. Retain the wallet's
  signed outbox and query transaction status before retrying it.
- A long divergent suffix needs deliberate recovery. The existing manual
  `native sync --from` can independently validate a complete candidate only up to
  its 1,024-block/8MiB cap. It does not bypass fork choice. Beyond those limits,
  preserve the state and signed outboxes and obtain an expanded validated recovery
  workflow; do not delete journals, trust declared balances, or weaken the guard.
- Missing/replaced journals, manifest/lock loss or uncertain writes remain
  readiness failures. Encryption is not permission to keep serving poisoned state.

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
- [TLS transport](../crates/boole-p2p/tests/tls_transport.rs): mutual pins, real
  possession checks against a forged public-key presentation, anonymous/wrong
  ALPN/plaintext rejection, trickled/silent deadlines, strict duplicate-field
  decoding, byte caps and TCP interception without application plaintext.
- [Native peers](../crates/boole-node/tests/native_peers.rs): 280-block incremental
  catch-up/restart, signed-transfer propagation and single confirmation,
  partition/rejoin with orphan requeue, wrong network/version/genesis/range and
  invalid block rejection, 512-transfer block traffic exceeding the round budget,
  worker/request/byte caps, per-key throttle, retry backoff and shutdown leases.
- [Native process/key CLI](../crates/boole-node/tests/native_peer_cli.rs): actual
  two-process encrypted sync and clean signal shutdown, restart, no secret output,
  no overwrite, unsafe file rejection and public/dangling peer-configuration refusal
  before state creation. RPC and peers also share the same live mutation boundary.

Meaningful RED observations included immature/underfee spending, missing target
adjustment/MTP checks, stale-state appends, oversized-file recovery, legacy
network impersonation and missing RPC/CLI behavior. Existing safe behavior was
allowed to pass new regression tests immediately. Compatibility gates preserve
v3 hashes/genesis/replay and session restrictions; no legacy fixture was rewritten.

These runs use disposable local directories and test keys. They are not an
operator wallet movement, public mining result or mainnet entitlement. No new
VM/model execution, useful-work reward or public activation occurred. R1 still
requires broader recovery/abuse/operations and wallet-safety acceptance, a scoped
public RPC strategy and the selected public-facing product scope before R2/R3
launch review. Closed-local TLS tests are not public-testnet operation evidence.
