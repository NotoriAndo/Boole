# Devnet archive v0 (M4b1)

An archive is an ordinary v1.3 **verify** node with a separate public object store.
It changes no consensus encoding, fold rule, gate fingerprint or recording.
The store is an availability source. A joining node derives and verifies its own
L2 blocks; the archive index never supplies a consensus answer.

## Running and joining

Add `--archive-dir PATH --archive-bind 127.0.0.1:PORT` to the normal driver command.
The directory must be private and have exactly one lifetime writer. A different
network/config/fork genesis, L2 genesis, L2 gas limit or L1 genesis is refused.
A public bind is refused (S21). Archive input pruning is refused (S20).
The driver logs `archive_url=.../boole/archive/v1/`. After `--until`, an archive
node keeps serving that finalized range until the operator stops it.

For a new driver or miner, repeat `--archive http://127.0.0.1:PORT/boole/archive/v1/`
in priority order. The other normal L1, L2, configuration and pinned gate options
are still required. M4b1 supports verify joins only. Archive HTTP URLs must name
literal loopback socket addresses; credentials, redirects, proxies and DNS are
not used. There are at most 16 explicitly pinned sources.

The finalized anchor comes from **the joining node's own L1 endpoint** in one
`finalized` header response. Header hashes are checked while walking backwards
from that anchor to the existing own journal anchor, or genesis for a new node.
The seven fold fields are decoded from those RLP bytes. A bloom-negative header
needs no receipts. Otherwise the entire ordered typed-receipt list must reproduce
`receiptsRoot`, and inbox positions use the block-global log index.

Verified inputs stream forward into the node's own fsynced, re-verified input
journal. Later replay and restart use that journal under TIP_ONLY; they never
request old L1 heights. An interrupted join can resume its verified prefix. Bad
archive replies are discarded, with per-source failure counters. FETCH makes
three rounds over the pinned sources, waiting 100 ms and 300 ms between rounds.
Exhaustion is typed, resumable availability, never a REJECT. After availability
returns, retry with the same state directory. An archive header chain that does not link the own L1 finalized anchor to the
own journal floor is integrity `l1-anchor`, just as on the non-archive path.
Integrity stops in the **own** journal still require preservation and investigation
as in the durability guide. Disposable `archive-download` headers are retired only
after their authenticated rows are fsynced into the own journal. Restart also
retires copies left after a crash between those two operations.

Evidence labels the fresh archive range `archive-verified`, and new live input
`l1-rpc`. On restart, existing inputs are `journal`. Verdicts remain `computed`
on the verify driver. Newly recovered evidence rows also carry the current
input label; already published evidence bytes retain their original labels. Miner status includes `inputs`, `input_ranges`, archive
source counters and bounded index alerts alongside its existing verdict trust.
The miner independently verifies archive inputs; its default verdict trust still
comes from its configured L2 node. `--verify-own` retains its existing meaning.

## Public API and bytes

The HTTP/1.1 API accepts GET requests only below `/boole/archive/v1/`:

| Route | Returned bytes / check |
| --- | --- |
| `info` | Identity, covered finalized range, historical input labels, pending classes |
| `l1/header/<block hash without 0x>` | Full RLP header; keccak and pinned schema |
| `l1/receipts/<block hash without 0x>` | JSON array of raw typed receipt hex; ordered trie against that header |
| `l1/txs/<block hash without 0x>` (M4b2) | Ordered raw transaction hex array for bloom-positive heights; ordered trie against that header’s `transactionsRoot` |
| `obj/exp/<export digest>` | Raw export; frozen OPEN length and digest |
| `obj/tr/<commit hash>` | Compressed transport; frozen OPEN length and keyed part Merkle root |
| `obj/pkg/<package digest>` | Original carried package bytes; original reveal's leaf/path binds them to POOL |
| `obj/env/<artifact SHA-256>` | Pinned public environment artifact bytes |
| `obj/gate/<binary SHA-256>` | Pinned public gate binary; caller also checks its FORK_TABLE descriptor |
| `obj/vstmt/<statement digest>` | Public verdict statement preimage; SHA-256 equals the applied record |
| `index/<from>?limit=<1..1024>` | At most 1,024 contiguous diagnostic index rows |
| `l2/system/<height>` | JSON array of the verified exact system prefix; never an input to verify-mode derivation |

Addresses are lowercase hex. Missing objects return 404 with a nonempty error
body. Requests are limited to 65,536 bytes; request bodies and transfer encodings
are refused. Replies have bounded Content-Length: at most 135,266,304 bytes and
the tighter class cap. Heights and limits use canonical decimal (no leading zero
or plus); hash keys have exactly 64 lowercase hex digits. Request lines and headers
use strict CRLF; whitespace before the header colon is refused. There are at most
eight concurrent connections; saturation attempts a fast 503 and closes (a peer
with unread request bytes can observe a connection reset). Each connection has
a two-second request deadline. After dispatch and object re-verification, writes
receive a fresh total deadline of two seconds plus body length at 8 MiB/s, with
all bounded chunks sharing that deadline. Local lock/disk verification does not
consume the body transfer allowance. Shutdown closes active sockets before joining workers.
Connections have monotonic IDs; registry entries are removed before shutdown,
including after FIN/RST or worker unwind. Worker creation uses fallible thread builders: failure
releases its descriptor and returns 503 while the accept loop continues. Transient
accept failures (descriptor/memory exhaustion and pending network errors) are
logged once and retried with bounded backoff; a fatal accept failure sets typed
`archive-server` health, observed by the driver. A poisoned store also sets typed
health, so serving cannot remain silently unhealthy.
Malformed requests receive 400, unsupported methods 405, and incomplete requests
408. Store integrity failures receive typed 500, availability/stopped serving 503;
these errors are logged once per class. A response over OBJ_MAX is internal failure,
never a malformed-request 400. Replaced store identity/lock/index stops serving.
Client body reads use chunks under a total deadline of two seconds plus length
at 8 MiB/s minimum throughput, including connect/headers, then discard and fail
over. Receipt and transaction JSON decoding refuse entry 65,537 before decoding or
allocating its value.
There is no write route, directory listing or private client/cache route.

The client library's export and transport helpers require the caller's frozen
state and commit. The package helper requires the frozen problem, configuration
and original reveal metadata (kind, codec, raw length, salt, Merkle path). It checks
the exact leaf index and path to that problem's POOL root, along with the carried
bytes' SHA-256. The generic pinned-content helper refuses `pkg`; a hash alone is
insufficient. Pinned-content helpers require a pin obtained from the verified
fold/reveal/descriptor, not a hash advertised by an archive. The system helper
compares with a locally derived prefix. Convenience object responses do not
replace the authenticated original L1 receipts used by the gate rebuild.

`segment/` is pending: no zstd crate is locked, and a bundle would never be
authority. `l1/blob` waits for M4-L5. No normalized environment producer exists in
v1.3, so normalized environment validation is pending; existing artifacts retain
their SHA-256 pins. Environment building remains deferred to J4.

## Publication, recovery and privacy

The archive sink runs **after** the ordinary driver has computed, selected and
verified a block. It independently checks the own journal, L2 parent link,
transactions and embedded root before publishing. It only ingests finalized
heights. A publication retry retries the sink cursor, never reapplies the driver
fold. Deterministic replay mismatches are sticky typed `archive-replay` integrity
stops; I/O/RPC and typed rebuild/decoder availability can retry without moving the cursor. The finality guard
uses the own input journal tip, independently of the derived height. Restart replays the already verified own chain to fill a missing archive
index suffix. Existing historical source labels are retained.

Every object is written new-file-only with file fsync, hard-link publication and
directory fsync, using the existing R1 durability implementation by copy. Every
read re-verifies the object. Corrupt bytes are preserved under `corrupt/`; a
subsequent archive GET returns 404 for that address until restart. Restart sink
replay reconstructs the public object from the own journal/Engine (and public
artifacts), preserving the quarantined bytes. There is no online repair on GET. Verified publication treats a newly quarantined
existing copy as absent and re-publishes it in the same call. Retirement likewise
treats a quarantined disposable cache copy as gone; every quarantined byte remains
preserved. Other integrity errors still stop typed. The index uses
version-bound append, confirmed-prefix checks, explicit torn-tail repair and
indeterminate-write fencing. A replaced writer lock, manifest or index stops the
store. The serving API does not turn such failures into valid object bytes.

Transport storage retains only the already public OPEN preimage and compressed
transport, so it can re-check the commit address and keyed Merkle root on every
read; the API returns the transport bytes. Verdict statement capture rebinds the
existing authenticated cache to the reconstructed public gate input, or reruns
the pinned gate to reconstruct the preimage. It publishes only when the typed
result and statement digest equal the already applied verdict. It never copies
a cache MAC, private key, client store, cancellation secret or unrevealed material.

The index is checked only **after** the local fold, under one shared 50 ms budget
per step across all sources, with one attempt and no inline backoff. The starting source rotates, and failed
observations put that source on a 30-second cooldown. Info probes also count in
source statistics. A later probe whose window was shortened by earlier work is
retried on its next rotated turn rather than blamed for exhausting the shared
budget. This cooldown never delays required-data FETCH. Sources' untrusted
`covered_through` hints skip heights not yet indexed (refreshed at most once per
second while behind). A transport without a bounded observation implementation
skips diagnostics rather than invoking its blocking required-data path. An alert
requires a well-formed index row claiming the same own L1 height/hash but a
conflicting L2 hash/root. Missing, malformed or unavailable data is not an
inconsistency alert. The local L2 hash and root win; diagnostics cannot produce a
verdict or redirect Engine selection.

An M4b2 archive supplies the ordered transaction and receipt lists at
bloom-positive heights and at finalized **inbox-touching** heights: at least one
transaction has `to == INBOX`, even when it reverted and emitted no log. The
index's `inbox_touching` flag is a diagnostic tripwire, never proof. An
unknown authenticated transaction envelope is conservatively inbox-touching;
its recipient layout cannot prove the absence of an inbox call. If an archive
joins from another archive without transaction lists, the flag is `null`
(unknown), and it serves no `l1/txs` before its own join height unless it later
obtains complete authenticated proofs. Other bloom-negative blocks may leave
those optional classes unserved. Older index
rows may omit the flag; replay preserves those diagnostic bytes while
backfilling authenticated proof objects.

Folding fetches only the receipts required by the bloom. Posting reconciliation
lazily requests `l1/txs` and, when needed, `l1/receipts` within its own intent's
finalized-height floor. Both complete lists must reproduce the own authenticated
header's trie roots before binding a known hash to the receipt at the same
index. Only that binding supplies exact success or REVERTED. A dropped hash is
never assigned another transaction's failure. A 404 for an optional class has no
source penalty. A 404 at or below the source's coverage when observed is cached
per block, class and source for 30 seconds across finality steps. Uncovered or
unavailable heights use exponential retry spacing (2–64 finality steps or
1–30 seconds). New source configuration clears these availability hints.
Optional proof I/O, including coverage probes, is limited to 32 requests per
new finality step or 30-second retry turn when finality is unchanged. The own
pause window rotates after the last attempted height, preventing an old missing
prefix from starving a later post. The work never refetches an entire window
on each finality step. These hints prove no absence. Withheld, missing or invalid
proofs keep the resumable `archive-tx-proof` pause without guessing success,
failure or a repost.

**DROPPED cannot be proved from an archive across bloom-negative blocks whose
transaction lists are not served.** REVERTED needs the exact authenticated tx
hash and status-0 receipt; neither a 404 nor a timeout substitutes for it. The
DROPPED rule is unchanged. A proof from the own L1 account nonce at the finalized
tip is a design question for M4c, not part of this implementation.

For old rows, reconciliation restores proofs into the own `transaction-proofs/`
cache with new-file-only publication and read-time re-verification. It does not
rewrite the original input journal. Restart can use that cache without an archive
or historical L1 RPC. An empty transaction trie proves absence without a list.
Each posting intent retains its finalized-height floor; legacy intents use zero.
A missing proof at or below a new intent's floor does not delay that intent.

## Archive follow ranges and trust labels (M4b2)

The driver accepts `--mode follow --follow-from FROM --follow-through TO --follow-archive` with
one or more `--archive` sources. It is mutually exclusive with
`--follow-canonical`. Input headers and receipts still use the own finalized L1
endpoint as their anchor. In the explicit range the driver takes `l2/system/<h>`
from the archive, strictly decodes L1-info, M2ROOT and V6 records, and checks the
entire system prefix against the local fold before building a block. A decoded
M2ROOT that differs from the local root stops; matching roots establish consistency.
Outside the range verdicts are computed by the own supervisor.

Driver evidence, archive info and miner status expose `trust_ranges`: decimal
`from`/`through` heights with independent `inputs`, `verdicts` and `environment`
axes. Inputs distinguish `l1-rpc`, `archive-verified` and reread `journal` rows.
Follow verdicts are `checkpoint-trusted`; own verification is `computed`. A miner
without `--verify-own` also labels M3 verdicts `checkpoint-trusted`. The environment
axis is `pending` throughout: neither Pass A nor Pass B is implemented here.
Range labels retain the weakest applicable trust and split at source/follow
boundaries. Existing evidence bytes retain their original labels.

## Independent read-only audit (M4b2)

`boole-devnet-audit` reads `--journal DIR`, or repeated `--archive URL` with
`--l1-rpc URL` for its own finalized anchor. It also takes `--node-rpc URL`,
`--m2-config FILE`, and for M3 `--m3-forks FILE --m3-gates FILE
--gate-artifacts DIR` (and `--zstd PATH` when required). Journal sources open
without a writer lock, tail repair or cleanup. File versions are bound and
checked throughout the audit. Archive downloads, gate scratch and caches live in
a fresh private directory outside the source; the audited node only receives
read RPC calls.

Supply `--expected-l1-number N --expected-l1-hash HASH` from an independent
finalized observation. That exact pair must occur on the authenticated input
chain, as an ancestor or its tip. A stale valid prefix cannot cover a newer
independent head. Optional `--expected-l2-number N --expected-l2-hash HASH`
binds that exact audited L2 block.

The default `--mode pure` recomputes every fold, gate verdict, root and system
prefix, and compares the prefix byte for byte with the audited node. It verifies
L2 timestamp and gas limit against derivation, header, transaction and OP receipt
trie commitments, reward lock/terminal and
compensation rules, mint/depositor conservation, contiguous admission indices,
retirement, FORK_TABLE activation records and kernel-fix recheck ordering.
`--mode replay --op-reth PATH --l2-genesis FILE` additionally starts a pinned
op-reth in a new private datadir, using loopback endpoints and no peers, and
reproduces every L2 block hash and execution receipt. The auditor owns and stops
that Engine; it never accepts an external Engine write endpoint.

Output is unsigned local JSON with schema `boole.devnet.audit.v1`: config and
fork-table digests, anchor and heads, last verified root, three trust axes, counts
by status and reason range, reward totals, admissions, environment hash, K4
coverage, and `first_mismatch` (decimal height and class) or null. Amounts, counts
and heights use decimal strings. The unsigned audit FORK_TABLE digest is SHA-256
of `BOOLE-M4/AUDIT-FORKS/v1`, a u32 big-endian row count, then each row’s
u64 big-endian activation height and 69-byte R-4 activation body in table order.
It binds the schedule as well as each record body; it is not a new consensus byte.
Pure mode reports execution as `not-run`; replay
reports its reproduced block count and private Engine cleanup. The audit exits
nonzero at its first input/fold/prefix/verdict/ledger/admission/K4/environment
mismatch and preserves sources. It never repairs an audited file.

Current retained K4 records are rebuilt and checked against frozen digests, but
coverage stays `pending` because durable retention pins belong to M4c. Environment
hash and verification also stay pending until J4; a successful audit does not
claim either deferred property. A K4 mismatch names the verdict record's height;
its decoder scratch stays inside the auditor's private work directory.

The audited-node RPC retains one authenticated view per height for its header,
transactions and receipt statuses. Responses and the block's accumulated raw
transaction hex are each bounded to 16 MiB. One shared budget allows at most
65,536 JSON values (including containers) for ordinary methods. For
`eth_getBlockReceipts`, the value cap is 8,388,609 (16 MiB / 2 + 1), enough
for any JSON value count within the wire cap. Nested arrays share the budget.
Objects allow 256 fields and nesting is limited to 64 levels. Conservative
allocation credits bound the receipt DOM to 512 MiB (128 MiB for ordinary
methods), charging container roots, map entries, strings and vector growth.
The wire reader can transiently allocate up to 32 MiB; authenticated raw
transaction hex retains its separate 16 MiB bound, while receipt encoding is
bounded by the authenticated fields in that capped response.
These are buffer/DOM bounds, not a process RSS promise. The RPC result is moved
out of its envelope, and OP receipt encoding avoids cloning the receipt DOM.
A local byte, node, depth, field, row or allocation limit returns a typed
`resource_stop` with decimal height/limit and a `resource-stop` check, leaves
`first_mismatch` null, and exits 2. It does not claim a chain mismatch; genuine
input mismatches still exit 1. Successful receipt bytes are unchanged.
The 15 s whole-response deadline, covering both response headers and body reads,
is also a typed `rpc-time` stop; its `limit` is milliseconds (15,000, or 3,000
for the existing connection deadline). Refused/reset connections and malformed
HTTP remain transport failures, not resource bounds on the audited-node side.

Replay preserves resource types and the current phase at the actual block height,
including private RPC initialization, headers, receipts and depositor balance.
Failures of the auditor-owned private Engine/RPC are local diagnostics, not
claims about the audited chain. After owned-input revalidation, an Availability
stall, the driver's public `rpc_unavailable` class, or a private-side transport
failure uses `resource_stop.resource=private-engine-unavailable`,
`side=private-engine`, the current height and phase, and exit 2 with null
`first_mismatch`. Its `limit=0` is an unavailable-limit sentinel, not a zero
budget. The driver's public transient class erases whether the cause was a send
timeout, HTTP 429 or 5xx, so the audit does not infer that cause from error text.
An already typed private RPC resource/time stop keeps its kind and numerical
limit and gains the same private side label. INVALID payload verdicts, hash
and ledger differences remain mismatches; audited-node transport failures keep
their existing phase classification. Successful receipt bytes are unchanged.
Two private-side paths still report a mismatch and are left to M4c: a private
Engine outage inside an M3 verdict rebuild, which the supervisor records as no
verdict yet (`verdict`), and a private Engine that exits or times out during
startup (`replay-setup`).
Owned inputs are rechecked before classifying a per-block failure and again
before emitting the final receipt, even after a resource stop. Verified source
change outranks that stop: it becomes an input mismatch with exit 1, and the
prior local bound is retained as `superseded_resource_stop`.
Otherwise the first stop wins. A later Engine cleanup failure is still recorded
in `private_engine_cleanup` and its `engine-cleanup` check, but keeps the earlier
resource stop, null `first_mismatch`, original execution check and exit 2.
A cleanup failure with no earlier stop remains its own mismatch. K4 retained
record rebuilds also preserve typed resource stops from their input history.
The depositor balance uses the already authenticated block hash with EIP-1898
`requireCanonical: true`, alongside the cached header/transaction/receipt view. Under M2 the
verdict, admission and fork checks are `not-applicable`. Replay setup and private
Engine cleanup have separate `replay-setup` and `engine-cleanup` check classes;
cleanup uses the last replayed height and preserves any earlier mismatch. Every
first mismatch is retained. A source change discovered by the unconditional
final recheck still sets `checks.input=mismatch` after an earlier mismatch,
without replacing that first class or height; exit 1 stays unchanged. When an
input failure creates the first mismatch (including a resource-stop supersede),
its Integrity/Availability height is used if present, otherwise the input tip.
Cleanup finishes before this final recheck. Resource stops are separate and
private error details are omitted.

The per-run disk floor is 15 GiB. Preserve the store and journal on a typed stop;
restore availability or repair into a fresh directory rather than editing a
content address or manifest. See [the durability guide](devnet-node-durability-v0.md)
for writer locks, explicit journal repair and checkpoint recovery.
