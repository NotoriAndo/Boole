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
on each finality step. Retry history expires one maximum backoff (30 seconds)
after eligibility, within 60 seconds after the last attempt. A starved height
can therefore restart its retry history at one second; rotation and the shared
32-request budget still apply. These hints prove no absence. Withheld, missing or invalid
proofs keep the resumable `archive-tx-proof` pause without guessing success,
failure or a repost.

**DROPPED cannot be proved from an archive across bloom-negative blocks whose
transaction lists are not served.** REVERTED needs the exact authenticated tx
hash and status-0 receipt; neither a 404 nor a timeout substitutes for it. The
DROPPED rule is unchanged. A finalized account nonce above the submitted nonce
is only a nonterminal “nonce consumed, inclusion unresolved” observation: the
original may already have succeeded, reverted, or been replaced. Terminal
DROPPED requires authenticated exclusion or replacement evidence; the nonce
alone is insufficient. No nonce-based terminal status is implemented.

For old rows, reconciliation restores proofs into the own `transaction-proofs/`
cache with new-file-only publication and read-time re-verification. It does not
rewrite the original input journal. Restart can use that cache without an archive
or historical L1 RPC. Cache installation runs before remote scheduling, even
with zero remote budget or backoff. It probes paths for needed heights directly,
rotating over missing-height metadata with at most 32 local presence checks per
turn and a separate allowance of at most 32 verified cache reads. Unrelated
cache files are not scanned; hints confer no authority. Presence I/O failures
are availability stops. Each install still checks the own manifest, row versions
and exact header/proof binding. A selected bounded batch is completed before
returning a hit. The subsequent remote phase excludes already installed proofs,
then skips budget, backoff and all-source covered-404 heights before reading
rows, preserving the 32-request cap. An empty transaction trie proves absence without a list.
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

## Default local retention and pin-store format (M4c2)

The archive library's `pins-v1` format is local metadata, outside consensus and
wire encodings. Its version-1 manifest binds the full own-journal network
identity and L1 anchor, and requires retention. Candidate keys contain both
`problem_id` and `commit_hash`. Accumulated reasons distinguish ACCEPTED, STALE,
kernel-fault REJECT, QUARANTINE and live CHECKING/PASSED coverage; reasons never
remove an older requirement. The interval-union index covers `h_c`, every height
in `h_o..=h_r`, and `reveal_pos.0`, including interior PART heights.

Objects are content addressed under `pins-v1/objects/0x<sha256>`. Descriptor,
host binary and environment roles bind their epoch or component identity.
Publication fsyncs bytes before a new immutable hash-chained record, then records
the selected L2 header, transaction bytes and root before a dependent checkpoint.
A checkpoint's pin anchor names a durable journal prefix. Extra complete records
or bytes after an interrupted publication are retained; they do not authorize a
checkpoint on their own. R1 publication recovery runs under the owning writer
lock. There is no pin deletion, pruning or compaction operation.

`ReadOnlyPins` binds manifest, record and artifact versions without taking an
Objects writer lock or invoking quarantine/repair. Verification rereads the
required own-journal rows and retained artifacts and checks their digests and L2
origin bindings. Missing coverage is availability, never a synthetic verdict or
a historical-L1 fallback. A changed source cannot remain a successful read-only
verification. Artifact digests are memoized only within the process and an
unchanged R1 file version (device/inode, length, modification and change times).
Fresh startup and restored or replaced versions hash again. Every verification
still checks source versions, and changed files stop before reuse. This avoids
rehashing one binary for every candidate and epoch; it is not a persisted
verification receipt. `RealGateInventoryPending` is the named unavailable seam for the
complete transitive real-gate inventory; stub artifacts cannot supply it.

Own-journal M3 driver and miner stores enable retention by default, including
when the miner's optional `--verify-own` is off. Their gate metadata is still
required on first startup. A retained descriptor and host binary replace the
original paths; a new epoch may reuse the exact retained host binary. Every
startup checks S20 before a gate rebuild or dependent publication. An indexed
missing object cannot be silently copied back from its old source. Restore the
verified bytes and restart or retry; no verdict is fabricated.

The driver publishes a prepared full header/transaction/root binding before
Engine import, then its selected binding before state, evidence and checkpoint.
The miner binds canonical headers and transactions before its retained tip,
package, reward or submission state. Their checkpoint/tip anchors name a prefix
that already contains the selected binding; a later suffix cannot satisfy an
earlier anchor. Read-only verification also binds the retention deletion guard.

A pre-M4c store can bootstrap from its complete verified own input journal and
historical local L2 headers/transactions. A durable ceiling bounds this one-time
bootstrap, and successful full replay seals it. A modern checkpoint with missing
pins must stop; it cannot re-enter bootstrap. Subsequent restart/recheck history
comes from the journal and pinned L2 bindings, with only the current L2 head
observed live. There is no historical L1 fallback, pin pruning or compaction.

After the independent auditor actually rebuilds every retained record, durable
pin verification compares its observed L2 headers/transactions and final root
against the identity-bound pin view. Only then can `k4.status=ok` and
`k4.pin_retention=ok`, with decimal `checked`, `total` and `verification_height`.
`rebuilt` records rebuild coverage separately. Missing or concurrently changed
pins remain `pending` / `local-unavailable` with checked zero and null verification
height; corrupt or foreign pins are K4 failures. Archive-only inputs have no
node-owned pin journal and remain pending. Environment/J4 stay pending, and
local retention does not assert the separate two-source readiness policy.

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

Retained K4 records are rebuilt and checked against frozen digests. Coverage is
`ok` only with the durable local verification described above; a rebuild alone
stays pending. Environment hash and verification stay pending until J4. A K4 mismatch names the verdict record's height;
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
Every private Engine operation is wrapped before Driver or supervisor error
erasure. The unresolved-fault tracker resets each replay step; a successful
retry clears only the same operation. Only failed/pending work consults it.
An outage swallowed inside an M3 rebuild is a local stop at the current replay
height / `verdict`, with the original origin height and operation as diagnostics.
The private lifecycle preflights the binary pin, executable, genesis and fixed
arguments before spawn. Bad setup is `private-engine-setup`; early exit or spawn
unavailability is `private-engine-startup-unavailable`; the absolute 60-second
readiness deadline is `private-engine-startup-time` (limit in milliseconds).
Readiness responses are capped at 4 KiB and must complete before the deadline.
Startup stops are at height 0 / `replay-setup`, side `private-engine`, exit 2,
with null mismatch. Bounded exit-code/signal evidence is kept; stderr text is
never a classification input. The lifecycle owns cleanup after every spawn.
The component comparison freezes ten symbols and every same-height word of
length 0–3 over eight status/reason seeds, two winner contexts and two prior
CANCELLED-compensation contexts. The
[compensation invariant proof](devnet-audit-compensation-seeds-v0.md) excludes
15 impossible context cells uniformly before execution; each excluded seed must
also fail the auditor's input check. The remaining 17 component contexts produce
18,887 words and 54,570 symbols: 558 accepted and 18,329 rejected, with 25
compensation cases, 22 quarantine activations/byte-mutation controls and 51
admission cases. These counts are asserted. This corpus has zero findings;
its snapshots do not certify joint full-history reachability.
A second frozen corpus derives three prior-compensation states from genesis
using the public fold and injected verdicts: LOCKED/ACCEPTED with an active
admission, VESTED/ACCEPTED, and VESTED/STALE with a retired admission. Signed
POOLs and reveals precede two COMMIT/OPEN/PART candidates; a kernel-fault loser
is accepted after another winner through a real KERNEL-FIX recheck. Empty
heights vest its reward, and a further recheck produces STALE. No state fields
are assigned to synthesize those snapshots. Every binary ACCEPT/REJECT word of
length 0–3 then activates real KERNEL-FIX rows across heights; STALE has no new
target job. All 45 histories pass, with 102 symbols/activations, 306 height
steps, 146 recheck records, 22 repeated ACCEPTs, 11 LOCKED cancellations,
11 VESTED-to-STALE transitions and 22 admission retirements. These counts are
asserted. A separate forged cross-height replay pairs a previously observed
ledger with an alternate reachable pre-grant state and a fresh reward id to
exercise the event-ledger duplicate guard. That corrupted pair is a negative
control, not a claimed valid history. The already_compensated=true reversal
branch remains defensive and unreachable by the reward invariant.
The M3 spec names unapplied CHECKING first-verdict jobs. Requiring a unique
quarantine match additionally codifies the fold's current activation behavior;
the spec does not state uniqueness. Candidates are keyed by `(problem_id, commit_hash)`, because a commitment hash
alone is not unique across problems. A genesis-derived two-problem control
commits the same hash to both problems and opens/completes only one. Its
quarantine at height 10 and due verdict at height 11 preserve the original
roots; the unrevealed candidate receives no result, reward or admission through
vesting. The independent decoder binds each record to its ordered queue job's
full key, and K4 binds a quarantine selector to the full key of the activated
first-verdict job. A wrong-problem OPEN preimage stays refused. Four additional
collision-shaped component words cover both rechecks' ACCEPT/REJECT combinations,
including two distinct compensation rewards/admissions for equal hashes under
different problems. They do not claim equal-hash OPEN preimages are constructible.
Two CHECKING jobs with that hash remain an ambiguous quarantine selector, refused
by both checks. These alias controls have zero findings.
The two original frozen corpora total 18,932 words, with different explicit
alphabets; the added alias words and controls are separate. The winner comparison checks
consistency between admissions decoded from fold-produced events and fold
state; it is not an independent prediction. S14 input validation shares
ForkTable::validate with the fold. Successful cases also check actual 0x08 bytes
and mutations, quarantine epochs/dispositions, ordered status/result changes,
compensation eligibility and reward totals. Every remaining discrepancy fails
the test. The full auditor invokes the fold before its independent ledger checks;
the consensus transition implementation is unchanged.

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
