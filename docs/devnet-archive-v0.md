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
returns, retry with the same state directory. Integrity stops in the **own**
journal still require preservation and investigation as in the durability guide.

Evidence labels the fresh archive range `archive-verified`, and new live input
`l1-rpc`. On restart, existing inputs are `journal`. Verdicts remain `computed`
on the verify driver. Miner status includes `inputs`, `input_ranges`, archive
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
the tighter class cap. HTTP parsing has read/write timeouts and a header deadline.
There is no write route, directory listing or private client/cache route.

The client library's export and transport helpers require the caller's frozen
state and commit. Pinned-content helpers require a pin obtained from the verified
fold/reveal/descriptor, not a hash advertised by an archive. The system helper
compares with a locally derived prefix. Convenience object responses do not
replace the authenticated original L1 receipts used by the gate rebuild.

`segment/` is pending: no zstd crate is locked, and a bundle would never be
authority. `l1/blob` waits for M4-L5. No normalized environment producer exists in
v1.3, so normalized environment validation is pending; existing artifacts retain
their SHA-256 pins. Follow joins, environment building and the audit tool belong
to later work.

## Publication, recovery and privacy

The archive sink runs **after** the ordinary driver has computed, selected and
verified a block. It independently checks the own journal, L2 parent link,
transactions and embedded root before publishing. It only ingests finalized
heights. A publication retry retries the sink cursor, never reapplies the driver
fold. Restart replays the already verified own chain to fill a missing archive
index suffix. Existing historical source labels are retained.

Every object is written new-file-only with file fsync, hard-link publication and
directory fsync, using the existing R1 durability implementation by copy. Every
read re-verifies the object. Corrupt bytes are preserved under `corrupt/`; a
subsequent request/publication can fetch or rebuild a replacement. The index uses
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

The index is checked only **after** the local fold. An inconsistent or unavailable
index is alerted and counted; the local L2 hash and root win. It cannot produce a
verdict or redirect Engine selection.

The per-run disk floor is 15 GiB. Preserve the store and journal on a typed stop;
restore availability or repair into a fresh directory rather than editing a
content address or manifest. See [the durability guide](devnet-node-durability-v0.md)
for writer locks, explicit journal repair and checkpoint recovery.
