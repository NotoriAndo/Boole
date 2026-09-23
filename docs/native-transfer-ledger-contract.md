# Native test-coin transfer contract — R1 closed-local integration

Status: native blocks, durable node, loopback RPC, encrypted owner-wallet recovery,
bounded offline chain recovery and mutually authenticated/encrypted incremental peer sync implemented and locally
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

New native state publishes and syncs an explicit empty canonical block file
before its first manifest. Once a manifest exists, an absent block file is
ambiguous data loss and boot fails closed; it is never treated as empty genesis.
Existing journals without a manifest are likewise preserved and refused, even
when the journal is empty after an interrupted first initialization. Preserve
that directory and recover into a fresh one. Older empty native states that had
a manifest but no block file must also use a fresh directory: absence cannot
distinguish old implicit genesis from lost funded history. No file/schema or
consensus format changes, deletion or automatic repair bypass are introduced.
This guard does not detect rollback to a complete valid prefix, including an
existing zero-byte block file; retain an independently checked head for that.

On reorg the node first saves the old pending queue plus bounded orphaned
transactions, then atomically replaces the block journal, then filters the queue
against the winning ledger. Boot uses whichever canonical block file survived.
The recovery union preserves up to 1,024 rows, retaining room for every old
pending input. Orphans are ordered by their former block/transaction order before
the old pending queue: their nonce or funding effect may be a prerequisite for a
queued successor. The filtered pool still holds at most 512 rows, with this
dependency-first order taking priority when the combined inputs exceed its cap.
Confirmed, expired, conflicting, underfunded or excess rows are removed from the
non-authoritative pool; invalid stored signatures are corruption, not silently
erased. Requeue is bounded/best-effort, so the wallet must retain its signed file.
Confirmation depth is reported but never labeled irreversible finality.

`boole native info` / `GET /native/info` includes a `resources` object for the
current local state: canonical history bytes/blocks and their hard limits,
confirmed transaction-index entries, pending journal bytes/rows and their limits,
and canonical balance/nonce-map entry counts. Counts are operational diagnostics,
not numbers of users or active wallets; zero-balance map entries are included.
Reservations do not increase canonical counts before confirmation. File identity
and readiness are checked before cached lengths are returned, and the query does
not replay signatures or sum the account maps. No filesystem paths or private
keys are returned. The existing CLI forwards this object unchanged.

Watch these values before reaching the local history limits. There is no automatic
history eviction, trusted checkpoint or pruning escape hatch. A capacity/write
failure is not permission to delete journals, bypass validation or force a weaker
fork; stop the node and preserve its state for the documented recovery path.
This endpoint reports storage usage, not process RSS or a latency guarantee.
The [preregistered developer-Mac capacity scenario](native-capacity-qualification-2026-09.md)
passes its 131,073-balance-entry / ~67MiB journal envelope, including a full queue
and two reopens. The initial code measured 327.125MiB maximum RSS and about 53s
full replay; the recent-fork source was requalified under identical criteria at
291.71875MiB and about 52s. These are source-specific samples, not an RSS guarantee.
The record preserves the initially blocked OS measurement and the identical-code
infrastructure retry. This is not qualification of the full history cap, large
forks, untrusted concurrent traffic or other hardware; recent small-fork results
are recorded separately below.

### Pending-state resource and publication boundary

The live node keeps one non-authoritative pending view and a bounded transaction-ID
index beside its ordered durable queue. Admission no longer replays all earlier
pending signatures or hashes every earlier transaction; account/status reads do
not reconstruct the queue. All new signatures, amounts, fees, nonces, expiry and
spendability still pass the same core checks. Block validation independently
verifies its actual transfers and never trusts this cache as canonical state.

Core preparation stages at most the sender, recipient and producer account
updates instead of copying the entire account map per transfer. A pending
reservation exclusively borrows its original view, has private fields and cannot
be cloned, deserialized or committed to a different/stale view. Dropping it has
no effects. The node commits it only after its bounded journal append and file
identity read succeed; an uncertain write fences the node. Canonical block
accounting still rolls back as a whole on any error.

Block confirmation, expiry, reward maturity, reorg and restart rebuild the derived
view at the current head, even if the queue's rows did not change. Pending rows,
their ID index and reservations are published together. Full-queue, rejection,
partial-write/sync-failure and orphan-recovery tests cover these transitions.
The 512-entry limit, journal byte limits and monetary rules are unchanged.

Local debug measurements on the same developer Mac observed admission exceed a
20-second regression budget after 384 of 512 transactions (26.021s) before the
change. The final focused run admitted all 512 in 3.177s and performed 100
direct full-queue account/status lookups in 1.609ms. An accounting-only fixture
with 16,384 unrelated accounts measured 512 reservations at 882.279ms before affected-account
staging and 144.372ms after it. The large fixture deliberately bypasses node
block-size policy; it is not a network block or throughput claim. These samples
are regression evidence, not production SLAs. The node still retains full
history, a canonical account map and a derived pending copy; block transitions,
startup and fork replay still have whole-state work and need larger-state
operational acceptance.

### Recent verified-prefix recovery

`NativeChain` retains internal inverses for at most its latest 256 committed
blocks. Each inverse records previous values only for changed balances/nonces,
reward locks/maturity entries, supply/height and cumulative work. Missing and
zero-valued entries remain distinct. Inverses cannot be externally constructed,
serialized or imported; replay rebuilds them from fully verified blocks.

`fork_at` produces a separate recent prefix from those verified inverses. Every
new suffix block still passes normal PoW, linkage, producer and transfer
signature, timestamp/target, amount/fee/nonce and monetary validation. The source
chain is unchanged until durable candidate publication. A prefix whose inverse
was pruned requires ordinary full replay; that is an operational cache bound,
not a new consensus rule or a trusted-checkpoint mechanism.

Complete-candidate imports compare each claimed prefix block's **entire typed
content**, including producer signature and transfer body, before reusing local
state. Header hash equality alone is insufficient. The confirmed-ID index is
retained only for that equal prefix and updated from the verified suffix; orphan
recovery examines only the replaced local suffix and stops at its existing cap.
Live peers supply the suffix directly and never force an older-prefix replay.
They report `bounded_reorg_requires_recovery` if either their downloaded suffix
exceeds 256 blocks or the required local inverse is unavailable.

The [recent-fork qualification](native-recent-fork-qualification-2026-09.md)
preserves the previous full-prefix replay failure at 58.049s against a fixed 10s
adoption bound. The corrected developer-Mac attempt passed at 3.251s, with
51.869s independent restart and 551.0625MiB peak RSS. The baseline peak RSS was
480.203125MiB, so this speedup does not claim a memory reduction. Prefix
history cloning, canonical-map copying, journal replacement and startup remain
linear costs; this is not a hard CPU deadline or constant-memory reorganization.

Direct tests compare rollback/reappend with independent replay, including
dependent and self-transfers, zero entries, fee recipients, reward maturity,
retarget state and cache pruning. They also cover same-hash signature/body
tampering and replaying every before/after-rename failure across recovery-union,
canonical-history and final-pool publication. An uncertain publication fences
the writer; restart selects the actual canonical file and recovers dependent
queued transactions without trusting stale cache state.

### Bounded offline chain recovery

[`native_archive`](../crates/boole-node/src/native_archive.rs) supplies the
operator path for a fork beyond the online suffix/full-chain RPC limits:

```sh
boole-node native-export --state-dir /private/node-a --output /private/backups/chain.ndjson
boole-node native-import --state-dir /private/recovered-node \
  --blocks /private/backups/chain.ndjson --expected-head <64-lowercase-hex-hash>
```

Stop the affected nodes first. Export requires an existing native manifest and
exclusive state ownership, writes a new 0600 file **outside the source state
directory**, fsyncs it, independently reads/verifies it back, then publishes it
without replacement. Source journals and manifest are not repaired, upgraded or
rewritten; a torn source tail is an error. Normal ownership lock files can be
opened/created. The output parent must already exist, be a direct non-symlink
directory and not be group/world writable. A crash can leave private staging
files or a complete published file; do not assume success without the receipt.
Never delete an existing destination to make a failed export succeed.

The archive is the canonical NDJSON sequence of `NativeBlock` objects, not a new
trusted checkpoint format. Obtain the expected head through the intended
administrative verification channel; the hash selects the intended history but
does not waive any consensus validation. Import reads one bounded line at a
time from one no-follow, nonblocking regular single-link descriptor, rejects
group/world-writable inputs and checks file/parent identity and stability. It
requires every line, including the last, to end in a newline; it never truncates
or edits its source. Duplicate/unknown fields, foreign network/genesis,
signatures, PoW, retarget, timestamps, transfers and monetary state are checked
from genesis before the destination is opened. Operational bounds remain
524,288 bytes per block line, 256MiB per archive and 100,000 blocks.

Import into a healthy existing state follows normal cumulative-work/tie-break
choice, preserves bounded pending/orphan recovery and reports `adopted: false`
only when the intended head is already present. It refuses a weaker/different
history instead of forcing rollback. Import opens the destination through normal
restart recovery: a previously torn destination tail or stale pending journal
may be repaired as on boot. For a damaged node, **preserve its entire directory
and use a fresh destination**. A malformed complete journal is not overwritten.
An invalid archive/expected head does not create the destination at all. A disk
failure during later publication can leave a fresh destination at genesis or
with a fully committed prefix/state; stop and inspect/restart, never claim a
success receipt that was not returned.

Receipts identify network, genesis, height, head hash and archive bytes. A new
destination reconstructs balances, locked rewards, nonces and confirmations,
but the archive does **not** contain owner vaults/passphrases, transport private
keys, allowlists, pending-only transfers or signed wallet outboxes. Back those
up separately and query each saved transaction before retrying the exact file.
After restore, compare the reported head/account/transaction state, then start
the closed-local node with the deliberately selected peer configuration.

Recovery does not remove the full-history in-memory store or bound CPU/disk time
by a hard deadline. Verified archive ownership is transferred into adoption
without a second signature replay/history clone, and journal publication streams
rows instead of allocating duplicate full-log strings. These are allocation
reductions, not a production-scale capacity claim. Normal native startup also
bounds the actual descriptor read and checks stability before tail repair.
The shared node-manifest reader now rejects nonregular/aliased inputs and files
above 64KiB; existing compatible creation provenance and legacy upgrades remain.

### Offline canonical accounting audit

For a stopped existing node or a separately restored copy:

```sh
boole-node native-audit --state-dir /private/node-a \
  --expected-head <independently-retained-64-lowercase-hex-hash>
```

`--expected-head` is optional for inspection, but necessary when checking that a
particular previously observed head survived. The command opens no sockets and
requires exclusive state/ledger ownership. It replays every canonical block from
genesis under the same signature, PoW, timestamp and accounting rules, then checks
that summed balances equal issued atoms, issuance does not exceed the compiled
cap, and per-account reward locks exactly match the unmatured reward queue. Locked
amounts must not exceed their balances; spendable atoms are balance minus locked
atoms. This is a full replay plus a derived accounting scan, not a fast readiness
query or a new trusted checkpoint.

Success prints one `boole.native.audit.v1` JSON object with scope
`confirmed_canonical`, network/genesis, head and height. Every monetary total and
height is a decimal string; bounded resource counts remain JSON numbers.
`accounting` reports `issuedAtoms`, `supplyCapAtoms`, `balanceAtoms`, `lockedAtoms`,
`spendableAtoms` and `pendingRewardEntries`. `confirmedTransfers` reports count,
gross `amountAtoms` and `feeAtoms` from canonical blocks only. Transfer/fee totals
include self-transfers and repeated spending, may exceed issuance, and are not
new supply, unique economic volume or fiat valuation. Pending transfers do not
enter these monetary totals. Fees remain producer receipts, not burns.

The `resources` object uses the same local limits as `native info`. Its
`pendingTransfers` is the valid in-memory retained queue, not the number of raw
stored journal rows. Stored pending signatures are still checked: a corrupt row
fails the audit, while valid but stale rows may be excluded from that derived
queue without altering their bytes. Source journals and manifest are never
repaired, truncated, upgraded or rewritten by this command. Ownership lock files
may be opened/created, so this is not a promise of zero filesystem writes. Missing
state/manifest, unsafe aliases, an incompatible/legacy manifest, torn or invalid
history, ownership conflicts and expected-head mismatch return failure without
a success report. Normal byte/block bounds remain in force.

Keep original state and wallet outboxes, compare independently replayed replicas
at the same intended head, and investigate mismatches before restarting service.
An internally valid prefix can still be stale or rolled back; without an
independently retained expected head the auditor cannot know newer history once
existed. The receipt is local unsigned diagnostics, not an external attestation,
finality guarantee, authorization to force a fork, public-testnet acceptance or
evidence of actual operator fund movement.

## Owner-wallet CLI and closed-local RPC

The foreground server is `boole-node run-native-local --state-dir <new-directory>
--addr 127.0.0.1:8383`. The default legacy `run-local` explicitly refuses the new
native network ID. No verifier bypass or legacy credit migration is involved.

The umbrella CLI uses `boole native --node http://127.0.0.1:8383` followed by:

- `info`, `account --pk <key>` or `transaction --txid <id>`.
- `peers` for configured transport identities, synchronization state and limits.
- `mine --vault <owner-vault> --passphrase-stdin [--reward-to <key>]
  [--attempts <bounded-count>]`.
- `transfer --vault <owner-vault> --to <key> --amount <decimal-tBOOLE>
  --outbox <new-file> --passphrase-stdin`; optional `--fee` and inclusive
  `--valid-before` height.
- `submit --file <saved-transfer>` to retry **the exact signature and nonce**.
- `sync --from http://127.0.0.1:<other-port>` for bounded full-chain import.

`boole wallet init/address` manages the AEAD vault, and `backup/restore` supports
authenticated encrypted recovery as described below. Native signing uses the
wallet-agent process. Prefer `--passphrase-stdin`: it reads one bounded line and
ignores any ambient `BOOLE_WALLET_PASSPHRASE`. The environment variable remains a
compatibility input only when that flag is absent; it is never forwarded to the
child environment. Neither path puts the passphrase on argv, and no raw
seed/session-key flag is added. Native monetary
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

Native request admission happens after the Host/browser boundary but **before**
body reading and JSON decoding. At most eight admitted requests share one pool,
including slow uploads and actual blocking node work; extra requests receive
`429 native_worker_limit` without a `100 Continue` invitation to send their body.
The same owned permit crosses extraction and execution. Body/parser errors,
disconnects and body deadlines return it; a caller timeout does not return it
while its queued/running node operation still exists. That operation can commit
after the caller receives a timeout, so query state before retrying as above.
The separate TCP/header connection bounds and per-route body byte limits remain.
Eight slow bodies can occupy all native request slots until their ten-second
deadline, including reads receiving 429; this is bounded busy admission, not a
reserved-read QoS guarantee, a bound on total process RSS or public-RPC approval.
Error response bodies are capped at 4KiB before releasing admission. Larger
input-reflecting parser diagnostics become the short JSON code
`native_error_response_limit` while preserving the HTTP error status; ordinary
bounded diagnostics and successful responses keep their existing formats.
This bounds retained error output, not all temporary parser allocations.

| Resource | Bound |
|---|---|
| Consensus transfer / block | 4,096 / 524,288 bytes; at most 512 transfers per block |
| Pending queue / reorg recovery union | 512 / 1,024 transfers |
| Local block journal | 256 MiB and 100,000 blocks; fail closed, no automatic prune |
| Offline block archive | Same history bounds; strict complete lines, expected head, full verification |
| Node state manifest | 64 KiB; stable regular single-link descriptor |
| Pending journal | 5 MiB; pre-read bound also applies during recovery |
| RPC full-chain import | 8 MiB and 1,024 blocks; entire candidate validated before adoption |
| Native request admission / deadline | 8 permits before body decode / 10 seconds; actual queued/running work retains its permit after caller timeout |
| Single CLI mining attempt | 1–10,000,000 hashes, then return; no unlimited loop |

Full-map staged accounting and the manual full-chain RPC import remain bounded
local prototypes. Automatic incremental peer synchronization is described below;
production-scale storage and broader fault/abuse acceptance are not complete.

### Encrypted owner-vault backup and restore

The selected R1 custody surface is a local encrypted owner vault, not a mnemonic,
OS keychain or long-lived unlocked daemon. The v1 JSON format, wallet AAD,
ChaCha20-Poly1305 and default Argon2id parameters (64MiB, time cost 3, one lane)
are unchanged. Backup does not rotate the key or lower/rewrite its KDF settings.

```sh
boole wallet backup --vault <owner.vault> --output <new-backup.vault> --json
boole wallet restore --backup <backup.vault> --vault <new-owner.vault> --json
boole wallet address --vault <new-owner.vault> --json
```

Each command reads the passphrase from the first stdin line. Supply it through a
private interactive `read -s`/pipe or a trusted secret-input mechanism, never a
literal in shell history, argv, logs or a committed script. No command prints a
seed or passphrase. Backup/restore first authenticates the ciphertext and its
32-byte wallet seed, copies the **same encrypted bytes** to a new mode-0600 file,
fsyncs the file and directory, reads the destination back, and returns only the
public address. `--json` uses the normal CLI envelope. Wrong passwords, modified
ciphertext and existing destinations fail without overwriting any vault.

Keep the source and a separate encrypted backup until the restored address
matches the expected owner and an appropriate local signing/recovery rehearsal
passes. Keep the passphrase separately. A backup is another copy of the **same
spending authority**, not a revoked old key or a recovery password. Losing every
copy or losing the passphrase is unrecoverable here. This local copy operation
does not certify removable-media durability, unattended remote backup, or
protection against a compromised user account.

The file must be a stable, regular, single-link 0600 file with an immediate
non-symlink parent that is not group/world writable. Reads use a no-follow,
nonblocking descriptor with before/after inode, size and timestamp checks.
FIFO, directory, direct symlink/hardlink, oversized and overpermissive files fail
closed. Newly created parent directories use 0700. Existing files are never
repaired in place or deleted. Atomic create uses a staged file and a no-overwrite
hard-link commit. A crash during publication can leave staging data or an extra
link, which is deliberately not auto-cleaned or unlocked; retain all evidence
and restore a known-good independent backup to a **new** path. An error after
publication can also leave a complete destination: do not retry by overwriting it
or discard the source based on an uncertain result.

| Wallet input/resource | Supported bound |
|---|---|
| Vault JSON / generic decrypted payload | 64KiB / 16KiB; the wallet itself requires exactly a 32-byte seed |
| Passphrase | 1–4,096 UTF-8 bytes, one line, no embedded CR/LF/NUL |
| Argon2id recovery profile | At most 256MiB, time cost 10, 8 lanes, and `memoryKiB × timeCost <= 786432` |
| Agent stdout / stderr | 4,096 bytes each, enforced while running |
| One spawned wallet-agent invocation | 60-second deadline, then process-group TERM/KILL cleanup and pipe closure |

Parsing rejects unknown/duplicate fields and validates version, algorithms,
hex/lengths and KDF cost before password derivation. Actual `open` and `seal`
also enforce the limits, so direct deserialization cannot bypass the KDF guard.
The original defaults remain supported; a custom older file above these limits
needs deliberate offline recovery review, not an automatic downgrade, ignored
header or oversized allocation. No such operator file was opened or changed.

The parent resolves the installed sibling `boole-wallet-agent` or an explicit
**absolute** `BOOLE_WALLET_AGENT_BIN`; it never searches ambient PATH. The
resolved executable and immediate parent cannot be group/world writable.
The child's environment is only fixed `PATH=/usr/bin:/bin` and `LANG=C.UTF-8`:
wallet/API variables, loader overrides and SSH agent sockets are not inherited.
Only canonical public-key/signature responses are accepted, and weak/invalid
public keys fail. The native/proof signer additionally verifies the returned
signature for the requested payload/network before returning its envelope.
The raw `wallet sign` façade checks encoding; callers verify its signature
against the expected public key and raw message.
Child diagnostics are withheld because they can contain the supplied secret.
The CLI may therefore report a generic agent failure instead of raw detail.

These are bounded I/O/lifecycle and accidental-disclosure protections, **not a
sandbox or authenticity proof for an operator-selected executable**. Such a
binary is trusted with the passphrase and can access the vault. Same-user/root
races and session-escaping malicious descendants remain outside this boundary;
release artifact/key custody remains R2 work. The 60 seconds starts at subprocess
execution, not at earlier native stdin collection, network requests or mining.

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
its entire bounded suffix and advertised head are present, verified from the
local common prefix, and win work/tie-break. A fork suffix longer than 256 blocks,
or a common prefix outside the available local undo window, is reported as
`bounded_reorg_requires_recovery`; it is not truncated into a winning chain.
Recent validated prefixes use the internal inverse described above. The suffix
bound is not a constant-time or constant-memory reorg guarantee. Full startup,
long-fork replay and broader storage/abuse acceptance remain R1 work.

After a complete candidate passes validation but loses normal fork choice, its
fixed outgoing worker remembers one local-head/remote-head pair. If both heads
are unchanged on a later authenticated hello, it returns `local_chain_preferred`
without downloading or revalidating that same losing suffix. Either head change
discards the pair; service restart loses it. Invalid, incomplete or merely
advertised candidates cannot populate it. Readiness and the local head are
checked again after the hello before using this preference. This bounded memory
can avoid work but can never authorize adoption, supply a checkpoint, bypass
validation of a new candidate or prove that the peer's current body is valid.
It does not protect against continuously changing candidates or remove the
first full verification cost. At most eight pairs exist, one per configured
worker; there is no disk cache or remote-sized map.
The [preregistered repeated-fork scenario](native-peer-repeat-qualification-2026-09.md)
passed with 131,073 balances: first full 16-block losing candidate 4.999s, seven
subsequent hello/done-only polls, 8.637s for all eight rounds, 480.078125MiB peak
process RSS and 51.791s independent replay, with unchanged journals/accounting.
This measures one peer and unchanged heads, not the general changing-fork cost.

When both heads match, each configured node periodically pulls the other's
bounded pending snapshot. Transfers pass the same signature and admission path
as RPC, with confirmed/pending duplicates idempotent. Valid but stale nonce,
funding or queue conflicts are benign rejections; invalid signatures abort the
round, and durability/ownership loss remains fatal to readiness. Gossip is
periodic pull, not instantaneous broadcast or a finality guarantee.

| Peer resource | Bound |
|---|---|
| Configured peers / outbound workers | 8 / one independent worker per configured peer, at most 8 |
| Inbound workers / handshake starts | 4 / 8 per second globally |
| Active incoming rounds per authenticated key | 1, with 500ms minimum start interval |
| TLS handshake | 2-second absolute I/O deadline and 64KiB encrypted-I/O budget |
| Native message / round plaintext bytes | 1MiB / 8MiB, counting both directions |
| Round / range | 64 requests, 256 downloaded blocks; page at most 16 blocks |
| Pending snapshot | 512 transfers; page at most 128 |
| Round I/O deadline | 10 seconds from completed authentication; not a hard real-time disk/CPU preemption guarantee |
| Outbound retry | 500ms on success; exponential 500ms–30s after consecutive failures |

Outbound scheduling is independent per fixed peer. An authenticated peer that
stops replying cannot hold the other peers behind its network timeout. Each
worker still has at most one active connection/round and its own bounded backoff;
remote messages cannot add workers or status labels. Concurrent imports serialize
through the existing state owner and retain the unchanged snapshot, signature,
fork-choice and durable-publication checks. Partial worker-start failure stops
and joins already-started workers before returning an error.

This increases the maximum concurrent outbound rounds from one to eight, trading
bounded additional sockets, threads and per-round buffers for independent progress.
It is not a global memory/CPU guarantee or protection against shared disk/state-lock
contention, all approved peers misbehaving, or an untrusted-host denial of service.
The earlier capacity qualifications did not include eight simultaneous peer
downloads. The [subsequent preregistered peer-buffer scenario](native-peer-resource-qualification-2026-09.md)
passed with 131,073 canonical balance entries and eight simultaneous incomplete
fork downloads at 551.90625MiB process peak RSS, including local TLS fixtures.
The oversized candidates were all refused, journals/accounting were preserved,
and independent restart took 51.784s. This is not qualification of concurrent
successful adoptions or arbitrary inbound/outbound traffic mixes.
The focused loopback isolation test failed at its fixed two-second
healthy-peer progress limit with the serial worker; the changed implementation
passed (0.14s total test time on the developer Mac). Eight authenticated stalled
rounds, ninth-peer refusal and shutdown interruption also pass a direct test.

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
`limits.maxOutboundWorkers` reports eight. `activeOutboundRounds` counts connect,
TLS handshake and round work but not backoff; `peakOutboundRounds` is the lifetime
maximum, bounded by the configured peer count. Shutdown drains these counts to
zero. A retained read-only monitor does not retain the node's state ownership.

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
  `native sync --from` is limited to 1,024 blocks/8MiB. Beyond that, use the
  [offline archive workflow](#bounded-offline-chain-recovery), preserving the
  original state and outboxes. Neither path bypasses fork choice or the total
  history cap; do not delete journals or trust declared balances to pass a guard.
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
- [Pending resources](../crates/boole-node/tests/native_pending_resources.rs) and
  [accounting preparation](../crates/boole-core/tests/native_pending_view.rs):
  a full 512-transfer queue, repeated reads, whole-block confirmation/restart,
  maturity/expiry/invalid nonce, discarded preparation, self-transfers and 16,384
  unrelated accounts. The durable pending append-fault test checks partial writes
  and failed sync without leaking reservations across restart/retry.
- [Offline archive CLI](../crates/boole-node/tests/native_archive.rs): recovered
  balances/nonces/confirmations, unchanged source and repeated import, 1,025-block
  long-fork recovery with orphan requeue, invalid input before destination creation,
  source-preserving refusal, no-overwrite/role-collision/unsafe-file/ownership guards,
  no forced rollback and recovery to a fresh directory without erasing corruption.
  Shared manifest and durability regressions also preserve their existing rules.
- [Offline accounting CLI](../crates/boole-node/tests/native_audit.rs): real
  replayed issuance/maturity/fee totals excluding pending, stable source bytes,
  expected-head binding, missing-history/bootstrap ambiguity, unsafe source and
  corruption refusal. Independent original/restored audit receipts agree in the
  archive test. The missing command and lost-history-as-genesis behavior were
  observed RED before implementation/correction. Core audit tests cover reward
  lock consistency, full-width totals, failed-block invariance and injected
  internal inconsistency without exposing unchecked state construction.
- [Two independent RPC nodes](../crates/boole-node/tests/native_http.rs): real
  PoW → reward maturity → A-to-B signed transfer → block import → identical
  balances/head/issuance, with public-bind, cross-origin, oversized-body and
  stale-readiness rejection.
  Raw `100-continue` connections prove the ninth request is refused before body
  extraction, completion/parser failure returns admission, and eight stalled
  bodies expire without leaking slots. A real-router lock-stall test observes
  eight HTTP timeouts while their block mutations retain permits; later
  completion and restart show one block/reward, not eight or a lost mutation.
  A 1MiB unknown JSON field initially produced a 2,097,510-byte HTTP error (RED);
  the bounded-error response keeps its 422 status and avoids the reflected field.
- [Encrypted-vault CLI](../crates/boole-cli/tests/native_cli.rs): actual server
  and wallet-agent processes, stdin-based mining, encrypted backup/restore after
  removing the primary from service, a restored-owner transfer, saved-file retry
  before/after inclusion and restart, exclusive outbox publication and second-node sync.
- [Vault primitive](../crates/boole-core/src/vault.rs),
  [wallet-agent](../crates/boole-wallet-agent/tests/wallet_agent.rs),
  [signer](../crates/boole-miner/tests/agent_signer.rs) and
  [wallet façade](../crates/boole-cli/tests/wallet_cli.rs): bounded KDF/JSON/input,
  authenticated encrypted recovery, unsafe file/agent-path rejection, no-overwrite,
  secret-bearing error suppression, live output cap and canonical verified responses.
  The shared process runner also retains its existing 13 timeout/pipe/group-cleanup
  regressions; wallet-specific tests cover the empty environment and silent deadline.
- [TLS transport](../crates/boole-p2p/tests/tls_transport.rs): mutual pins, real
  possession checks against a forged public-key presentation, anonymous/wrong
  ALPN/plaintext rejection, trickled/silent deadlines, strict duplicate-field
  decoding, byte caps and TCP interception without application plaintext.
- [Native peers](../crates/boole-node/tests/native_peers.rs): 280-block incremental
  catch-up/restart, signed-transfer propagation and single confirmation,
  partition/rejoin with orphan requeue, wrong network/version/genesis/range and
  invalid block rejection, 512-transfer block traffic exceeding the round budget,
  worker/request/byte caps, per-key throttle, independent retry backoff, one
  stalled peer alongside healthy progress, eight simultaneous stalled rounds and
  shutdown leases. Three concurrent pulls of one signed transfer retain one
  pending transaction and confirm one recipient credit/nonce after restart.
  Repeated fully verified losing forks require no new block download while both
  heads stay fixed; restart or either head change requires validation again.
  A prior invalid signature cannot suppress a corrected body at the same hash,
  and cached preference cannot hide journal loss during the hello exchange.
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
requires broader node recovery/abuse/operations acceptance, a scoped
public RPC strategy and the selected public-facing product scope before R2/R3
launch review. Closed-local TLS tests are not public-testnet operation evidence.
