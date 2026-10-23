# Devnet miner client v0

`boole-devnet-mcp` exposes mining tools over newline-delimited MCP stdio for the
closed-local devnet. It is a separate crate and executable, so the existing
`boole-mcp` native-chain tools and their tests keep their existing contract.
The client uses the workspace derivation and RPC code and existing dependencies;
it downloads no tools and adds no external package or version.

The [M2 v1.2 rules](devnet-commit-reveal-v1.2.md) and
[M1 derivation](devnet-derivation-v0.md) define the underlying protocol. This client
uses unlocked anvil development accounts and disposable test ETH to self-post.
Production posting through a batcher, custody, escrow and drand are later work.

## Start and tools

Start the existing M2 devnet, then connect the MCP host to this executable:

```text
boole-devnet-mcp --devnet --l1-rpc http://127.0.0.1:28545 \
  --node-rpc http://127.0.0.1:29545 \
  --account <unlocked-development-address> --state-dir <private-directory> \
  --m2-config <same-config-as-driver.json> --zstd <pinned-executable>
```

`--devnet` is mandatory. The L1 chain id and deployed inbox runtime must match
the devnet. The required L2 node RPC must also report the devnet chain id. Its
latest header supplies the L2 gas limit for the same S4 startup validation as
the driver; no fixed 30M limit is assumed. The validated M2 config must match
the driver, including its pool key. The default is `Config::devnet()`.
Only literal loopback RPC URLs are accepted, with proxy routing and redirects
disabled. There is no HTTP listener. The codec executable is checked against
the existing codec pin before use; installed symlink paths are resolved and
their target bytes are checked. There is no PATH fallback or network fetch.

| Tool | Arguments | Result |
|---|---|---|
| `boole.devnet_status` | none | Slot/lane, open problem, last commit height, pending commits, next draw execution height, fold height, config/slot digests, m2_root, l2_m2_root_match and any stall reason |
| `boole.devnet_problem` | `slot` or `problem_id` | Problem id, decoded package as hex and UTF-8 when valid, and finalized-fold membership status |
| `boole.devnet_commit` | `export_file`, `payee`, `problem_id` | Durable submission id and initial progress; automatic posting continues while the process runs |
| `boole.devnet_submission` | `submission_id` | Finalized/retained status, observed state and prune height, part count, transaction hashes and failures, local-check status and posting error if any |
| `boole.devnet_cancel` | `submission_id` | Persisted cancel intent and progress; a finalized live candidate is cancelled on L1 |

Hex ids have a `0x` prefix and fixed byte lengths. `payee` is the 20-byte L2
solver address bound in B. `export_file` is a bounded regular local file containing
the raw export for the problem's lane. The client selects the matching format
and an allowed environment covering the required packages. Metadata uses license
id 1, the protocol's placeholder proof terms, with undeclared model and no trace.

The MCP transport supports initialize, ping, tools/list and tools/call. Requests
are limited to 64 KiB, and eight requests may queue. A missing-id notification
cannot invoke a tool or create a submission. Invalid parameters and tool failures
are returned as MCP error content. EOF stops the worker; pending work remains in
the private directory for restart.

## Independent finalized fold

The client reads finalized L1 headers and inbox logs from genesis and applies
`boole-derivation::m2::State::step` with the same validated config and initial
config-digest root as the driver. It checks header links, the cached finalized
anchor, inbox address/topic/hash/removed flags and duplicate log positions.
Before folding it applies the same mixHash, beacon-root and baseFee format
checks as M1 derivation; a malformed mixHash cannot silently become zero.
Malformed envelopes have the same treatment as derivation. It never reads
unfinalized inbox inputs into its state.

Accepted PROBLEM_REVEAL packages are retained by their pool sequence and index.
Membership is the fold's accepted Merkle check over the carried bytes, including
codec and raw length. Codec 0 returns those bytes. Codec 1 requires one standard
zstd frame, content size, bounded window, no dictionary, checksum and no trailing
or skippable frame; the pinned decoder verifies the compressed stream. A broken
package refuses the problem/commit call as an operator fault. It does not modify
consensus or fabricate an on-chain void verdict.

A changed finalized anchor, backward finalized height, broken link, inconsistent
log or input error stalls that client instance and prevents posting. Status and
submission tools remain readable with the previously verified state and stall
reason. Transient connection,
timeout, body-read and HTTP429/5xx failures receive at most three read attempts
with 100/300ms backoff before a stall; format and application errors do not retry.
Transaction writes are reconciled rather than blindly retried. Stop, verify the
intended local node/config and preserve the private directory before restarting. Restart folds
from genesis; a cached root is never accepted as a substitute for that fold.
The store is bound to the exact config digest, L1 genesis hash and observed L2
gas limit.

After folding, the client reads both system deposits from the L2 block at its
fold height. `l2_m2_root_match` is true only when the strict M2ROOT and L1 origin
match its own root. A mismatch is a hard error that stops posting. Null means
L2 has not built that height yet (or genesis has no deposit); the worker waits
for a match before posting. Status and private progress remain readable.

The RPC endpoint supplies the finalized chain. This is independent M2 derivation,
not an Ethereum consensus light-client proof or an RPC quorum. A server cannot
supply an arbitrary M2 result to replace the fold; a dishonest L1 RPC remains
outside this local trust model.

## Submission lifecycle and private material

The client compresses the raw export using a known-size input file, builds B,
keyed part leaves, the padded Merkle tree and commit_hash with the derivation
hash primitives. It generates independent fresh 32-byte CSPRNG salt and cancel
secret for each submission. It writes the preimage and parts before any COMMIT.
The private directory is 0700, records and temporary files are 0600, and updates
use a synced temporary file and rename. A file lock excludes a second writer.
Corrupt identity/records and permissive or linked store files refuse startup.

Progress responses contain neither B, salt nor cancel secret. The salt becomes
public inside OPEN as required by M2. The cancel secret is sent only in CANCEL;
it is never sent to the local pre-check. Keep the directory private: it contains
unrevealed proof material and cancellation authority. It is not encrypted storage
or an operational wallet.

The worker waits until its own finalized fold includes h_c. It posts OPEN only
when the next L1 inclusion height is at least h_c+D, and no later than
`h_c+W−ceil(n/P_BLK)−2`. It sends parts after observing a successful canonical
OPEN receipt. At most P_BLK posts are attempted per observed head, and no reveal
item is newly posted after the window. This is conservative scheduling, not an
inclusion guarantee under censorship or a stalled chain.

Finalized state determines completion. Canonical unfinalized receipts suppress
duplicate work; missing/reorged parts can be reposted after a bounded delay.
A canonical reverted receipt marks that submission step REVERTED in its private
record and progress response, without blocking other submissions. Reposting
remains subject to the same M2 windows and budgets. An orphaned receipt is not
canonical inclusion.
An uncertain send outcome retains the same durable preimage and reconciles its
on-chain candidate before retrying. Retries use the same item for the same
submission. New submissions prepare their export again with new secrets; encoding
is deterministic and byte-unique compressed streams are not promised. Public
devnet order flow still exposes parts before finality;
the production private-order-flow/escrow protections are not implemented.

CANCEL intent is persisted first. A submission cancelled before any COMMIT is
posted can abort locally. An already posted commitment is cancelled once the
finalized fold sees it. CHECKING and EXPIRED candidates cannot cancel. If the
client stays offline, a partial or unopened reveal may expire; restart reports
EXPIRED and sends no late OPEN. Each owned record retains its observed state
and the problem's `close+W+1` prune height; EXPIRED and CANCELLED remain final
after the fold prunes tombstones, even across restart and full replay. A sent
COMMIT missing after its finalized deadline is COMMIT_NOT_INCLUDED. A closed
problem with no posting attempt is PROBLEM_CLOSED; a pre-post cancel is ABORTED.
Partial data may remain publicly decodable.

## Optional local pre-check

Configure `--precheck <executable> --precheck-sha256 <0x-sha256>` to run a pinned
local adapter before COMMIT. The adapter receives `--export-file`,
`--problem-file`, `--env-id` and `--export-format`; the files are private bounded
copies. It receives no salt or cancellation token. Exit 0 means local accept,
exit 10 means deterministic reject, and any other exit is a tool error. Reject,
pin mismatch, tool error and the 120-second timeout all refuse COMMIT. Output is
discarded and the timed-out direct process is killed and reaped.

No hook is configured by default. Submissions explicitly report `local_check:
"not_run"`; they make no gate or kernel validity claim. A configured successful
adapter reports `local_accept`, which is still a local pre-check rather than an
M3 verdict. The operator must supply the intended pinned gate adapter and its
own containment for untrusted proof execution. The client does not install or
discover a compiler, gate or network service.

## Validation and limits

Tests reproduce the same candidate's exact COMMIT/CANCEL bytes, B, commit hash,
keyed leaves and all fourteen part bodies from the existing M2 recording. Codec controls reject truncated, concatenated,
trailing, dictionary and missing-checksum frames. Offline client folding matches
all 140 roots of the rounds recording. Finality faults, private-store locking,
permissions, pin mismatch, deterministic reject, tool error and timeout are tested.
Regression tests cover pruning, missed deadlines, durable reverted receipts,
transient reads and reachable stalls, header-format parity, the actual L2 gas
limit, nonempty packages, format 1 and cancellation of OPEN with some parts.

The opt-in `live` test uses the actual MCP stdio process, anvil, one pinned
op-reth replica and the driver. It runs pool/draw/package/commit/open/parts to
CHECKING with equal client/driver roots, cancels another candidate and restarts
after an offline candidate expires and again after tombstone pruning. It uses
lane 1/format 1 with a required package and a 40M L2 gas limit, asserts exact
posting/comparison counts, and checks every submission's salt/secret is absent
from progress and MCP stdout/stderr. A rejecting local hook makes no post.
Set the existing pinned client/codec environment variables to execute it; without those variables its binary-gated entry returns early. Node runs use
the shared resource lease procedure.

This remains a synthetic closed-local devnet transport client. It does not build
a lane-2 source archive or proof, run the production gate by default, certify a
kernel proof, settle a verdict, mint rewards or enable a public network.
