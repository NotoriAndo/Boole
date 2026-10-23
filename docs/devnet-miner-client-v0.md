# Devnet miner client v1: M3 verdicts and rewards

`boole-devnet-mcp` exposes mining tools over newline-delimited MCP stdio for the
closed-local devnet. It is a separate crate and executable, so the existing
`boole-mcp` native-chain tools and their tests keep their existing contract.
The client uses the workspace derivation and RPC code and existing dependencies;
it downloads no tools and adds no external package or version.

The [M3 v1.3 rules](devnet-verdicts-v1.3.md), [M2 v1.2](devnet-commit-reveal-v1.2.md)
and [M1 derivation](devnet-derivation-v0.md) define the protocol. This client
uses unlocked anvil development accounts and disposable test ETH to self-post.
Production posting through a batcher, custody, escrow and drand are later work.

## Start and tools

Start the v1.3 devnet with its explicit M3 config, then connect the MCP host:

```text
boole-devnet-mcp --devnet --l1-rpc http://127.0.0.1:28545 \
  --node-rpc http://127.0.0.1:29545 \
  --account <unlocked-development-address> --state-dir <private-directory> \
  --m2-config <same-config-as-driver.json> --m3-forks <same-forks-as-driver.json> \
  --zstd <pinned-executable>
```

`--devnet` is mandatory. The L1 chain id and deployed inbox runtime must match
the devnet. The required L2 node RPC must also report the devnet chain id. Its
latest header supplies the L2 gas limit for the driver’s S10 M3 startup validation
(S4 in the legacy profile); no fixed 30M limit is assumed. The validated M2 config must match
the driver, including its pool key. The default constructor remains the legacy `Config::devnet()` with M3 off; the
v1.3 network requires the explicit M3 config. A missing fork file selects only
the configured genesis epoch, so supply the full table when forks exist.
Only literal loopback RPC URLs are accepted, with proxy routing and redirects
disabled. There is no HTTP listener. The codec executable is checked against
the existing codec pin before use; installed symlink paths are resolved and
their target bytes are checked. There is no PATH fallback or network fetch.

| Tool | Arguments | Result |
|---|---|---|
| `boole.devnet_status` | none | Slot/lane, open problem, last commit height, pending commits, next draw execution height, fold height, config/slot digests, m2_root, l2_m2_root_match and any stall reason |
| `boole.devnet_problem` | `slot` or `problem_id` | Problem id, decoded package as hex and UTF-8 when valid, and finalized-fold membership status |
| `boole.devnet_commit` | `export_file`, `payee`, `problem_id`; optional `license_id`, `allow_stale_gate` | Durable submission id and initial progress; automatic posting continues while the process runs |
| `boole.devnet_submission` | `submission_id` | Finalized/retained status, queue index, earliest due height, verdict, earlier candidates holding the barrier, rewards, license/terms digest, part count, transaction hashes, local-check status and posting errors |
| `boole.devnet_cancel` | `submission_id` | Persisted cancel intent for a COMMITTED or OPEN candidate |
| `boole.devnet_rewards` | `payee` | Reward entries, actual vest height and observed L2 balance changes |

Hex ids have a `0x` prefix and fixed byte lengths. `payee` is the 20-byte L2
solver address bound in B. `export_file` is a bounded regular local file containing
the raw export for the problem's lane. The client selects the matching format
and an allowed environment covering the required packages. Metadata defaults to open license id 1 (placeholder CC0 proofs / CC-BY traces),
with undeclared model and no trace. An explicit `license_id` must exist in the
config and carry the open flag. The response shows its `terms_digest`; counsel’s
final terms need a new id before testnet. A payee with any L2 code is refused,
including a second check immediately before posting COMMIT, because a reverting
payee can lose the reward transfer.

The MCP transport supports initialize, ping, tools/list and tools/call. Requests
are limited to 64 KiB, and eight requests may queue. A missing-id notification
cannot invoke a tool or create a submission. Invalid parameters and tool failures
are returned as MCP error content. EOF stops the worker; pending work remains in
the private directory for restart.

## Independent finalized fold

With M3 enabled, the client re-verifies its own authenticated input journal from genesis
([M4a storage](devnet-node-durability-v0.md))
and the corresponding actual L2 transaction bytes. Every raw transaction must
match the transaction hash listed in that block. It strictly decodes VERDICT batches, reconstructs the
queue’s due heights from fuel, and checks every REWARD against the pure fold.
The complete prefix is L1-info, M2ROOT, VERDICTs and REWARDs, followed by users.
Only after the computed post-fold root and exact prefix match does the client
adopt that height. Malformed, missing, reordered or inconsistent deposits stop
posting with a hard mismatch.

A fuel-delayed result may appear after its earliest `avail + K`. The client waits
there until later canonical L2 bytes provide its fuel, then applies the result
only at the recomputed due height. `waiting_for_l2` and a null root-match flag
keep posting paused during this wait. It never invents an empty verdict set.
The default trusts verdict outcomes from the chosen L2 node; status explicitly
reports `verdict_trust: "l2-node"`. This is not an Ethereum light-client proof.

The legacy M3-off profile continues the L1-only M2 fold. Both profiles check L1
header links, the finalized anchor, inbox address/topic/hash/removed flags and
duplicate log positions. The mixHash, beacon-root and baseFee checks match M1;
malformed fields cannot silently become zero. Unfinalized inbox data never enters
the fold. L2 parent links and cached canonical anchors are checked in M3.

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
with 100/300ms backoff before a resumable availability pause; subsequent polls
retry the same height. Format and integrity errors remain sticky.
Transaction writes are reconciled rather than blindly retried. Stop, verify the
intended local node/config and preserve the private directory before restarting. Restart folds
from genesis; a cached root is never accepted as a substitute for that fold.
The store is bound to the exact config digest, L1 genesis hash and observed L2
gas limit.

`l2_m2_root_match` is true only after the actual L1-info/M2ROOT and the computed
root agree. A mismatch stops posting; status and submission tools retain the
previously verified progress and the stall reason. Null means genesis, a lagging
L2 node or missing canonical fuel lookahead. The worker waits for a match before
posting. Restart always folds from genesis and checks the deposits again.

The RPC endpoint supplies the finalized chain. This is independent derivation with explicit L2 verdict trust,
not an Ethereum consensus light-client proof or an RPC quorum. A server cannot
supply an arbitrary M2 result to replace the fold; a dishonest L1 RPC remains
outside this local trust model.

The CLI creates a locked input manifest/journal pair in the private directory.
Its manifest additionally binds the L2 genesis and fork genesis row. An old store
without the pair is refused; preserve it and use a fresh directory. Starting
without a journal requires finalized genesis height zero. Previously captured
unfinalized inputs stay in a separate cache until their parent chain is proved
against the node's own finalized tip. Expired missing inputs pause safely with
no old-RPC fallback. `--repair-journals` explicitly permits incomplete-tail repair;
complete malformed rows and identity mismatches remain refusals.

## Submission lifecycle and private material

The client compresses the raw export using a known-size input file, builds B,
keyed part leaves, the padded Merkle tree and commit_hash with the derivation
hash primitives. It generates independent fresh 32-byte CSPRNG salt and cancel
secret for each submission. It writes the preimage and parts before any COMMIT.
Both the store identity and submission records use `schema_version: 2`. A v1
store is refused with a clear message; use a fresh private directory for the v1.3
reset and preserve the previous one. The directory is 0700, records and temporary
files are 0600, and updates
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

Finalized state determines completion. With M4a enabled, inclusion and revert
status come from the journal's re-verified transaction and receipt proofs. The
client waits for finalized inclusion before progressing; it never re-queries
historical receipts through RPC. Missing parts can be reposted after a bounded delay.
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
finalized fold sees it. CHECKING, PASSED, ACCEPTED, REJECTED, VOID, STALE, SUPERSEDED and EXPIRED
candidates cannot cancel. If the
client stays offline, a partial or unopened reveal may expire; restart reports
EXPIRED and sends no late OPEN. Each owned record retains its observed state
and the problem's `close+W+1` prune height; EXPIRED and CANCELLED remain final
after the fold prunes tombstones, even across restart and full replay. A sent
COMMIT missing after its finalized deadline is COMMIT_NOT_INCLUDED. A closed
problem with no posting attempt is PROBLEM_CLOSED; a pre-post cancel is ABORTED.
Partial data may remain publicly decodable.

## Verdict, barrier and reward progress

CHECKING ends automatic posting, while progress keeps following the chain.
The submission tool reports the original `queue_index`, earliest due height
`avail + K`, and the latest verdict’s outcome, reason code, fuel, epoch and
`vstmt_digest`. `earlier_candidates` lists unresolved candidates ahead of it in
commit priority, which can differ from completion/queue order. A PASSED candidate
can therefore wait behind an earlier CHECKING candidate or a pending recheck.

Rewards show a decimal base-unit `amount` (18 decimals), `vest_h` and LOCKED, VESTED or
CANCELLED state. A compensation entry is marked COMPENSATION while locked and
retains its compensation flag after vest. STALE cancels only an unvested reward;
vested balances remain. These statuses do not trigger another OPEN or PART.

`boole.devnet_rewards` lists entries for a payee. At an observed vest it adds
`vested_at`, `balance_before`, `balance_after` and signed hexadecimal
`balance_change`. This is the payee’s **total net change in that block**: other
transactions or multiple rewards can contribute. An unavailable balance is
reported as unavailable, rather than zero. With minting disabled, the fold still
records vesting and `payment_emitted` stays false. The tool observes the selected
L2 RPC; it does not prove account balances independently.

## Optional own-candidate verification

Add `--verify-own --m3-gates <pinned-workers.json> --gate-artifacts <directory>`
to run the configured epoch’s pinned stub adapter on this store’s candidates.
Every replay epoch needs its descriptor and authenticated binary. Startup checks
the host, binary pins, sandbox canaries, codec and component artifacts. The
worker uses a 1,024 MiB envelope, a 30-second wall guard and bounded output.

Inputs are rebuilt from the frozen L1-info anchors, original OPEN/PART/reveal
positions and authenticated artifacts. Only then may an authenticated local
cache result be used. Outcome, reason, fuel and `vstmt_digest` must all agree
with the canonical record. Mismatch or unavailable local verification pauses
posting and reports the cause; it never fabricates a rejection. Status labels
this mode `l2-node-with-own-verification`: other miners’ results remain trusted
from L2. The deterministic devnet stub exercises protocol paths, not Lean proof
validity. A local pre-check before COMMIT remains a separate advisory step.

## Optional local pre-check

Configure `--precheck <executable> --precheck-sha256 <0x-sha256>` to run a pinned
local adapter before COMMIT. For M3 also supply the adapter’s declaration with
`--precheck-gate-rule-digest <digest>`, bound alongside its executable pin. The adapter receives `--export-file`,
`--problem-file`, `--env-id` and `--export-format`; the files are private bounded
copies. It receives no salt or cancellation token. Exit 0 means local accept,
exit 10 means deterministic reject, and any other exit is a tool error. Reject,
pin mismatch, tool error and the 120-second timeout all refuse COMMIT. Output is
discarded and the timed-out direct process is killed and reaped.

No hook is configured by default. Submissions explicitly report `local_check:
"not_run"`; they make no gate or kernel validity claim. A configured successful
adapter reports `local_accept`, which is still a local pre-check rather than an
M3 verdict. It counts only when the declared digest matches the epoch at the
estimated `avail + K`, using the next COMMIT height, D and the part budget. This
is rechecked immediately before posting in case waiting crosses a fork boundary.
A mismatch returns `local_check: "stale-gate"` and refuses COMMIT. An explicit
per-call `allow_stale_gate: true` permits that advisory override and preserves
the stale label. Executable-pin, deterministic-reject and tool failures remain
refusals. The estimate is not an inclusion guarantee. The operator must supply the intended pinned gate adapter and its
own containment for untrusted proof execution. The client does not install or
discover a compiler, gate or network service.

## Fairness and validation

Team and foundation operators do not mine. A model trained or tuned by the team
on Boole data must be published under open terms before anyone mines with it.
Training may use only ACCEPTED, non-STALE public proofs and statements with the
required permissions. This client reads supplied exports and performs no model
training or private-order-flow data export. Affiliation and model publication
remain operator policy; the client enforces the open-license id and displays
its terms digest.

The [I13 recording](../fixtures/devnet-m3/miner-client-v1.3.json) retains 98 live
A/B/C/client comparisons on v1.3, with pinned own verification, process restart,
LOCKED→VESTED status and the exact balance increase at height 96. It contains
public comparison results and provenance, with no private preimages. Reproduce
with the tracked `m3_live` integration test and explicit pinned op-reth, anvil,
gate and zstd executables. The existing live test below covers the legacy profile.

Behavioral RED cases precede the v1-store, malformed-VERDICT, stale-gate and code-
payee refusals. Client tests also cover all 430 heights of the prior M3 fork/reorg
tapes, delayed fuel lookahead, own-verdict outcome/fuel/digest mutations, commit-
priority barriers, current-epoch pin rechecks and license selection.

## Retained v0 transport validation

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
kernel proof or enable a public network. The v1 client follows devnet settlement
and reward deposits; the devnet native unit has no L1 backing or bridge.
