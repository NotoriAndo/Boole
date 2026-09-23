# Native publication input/output version fence — 2026-09-23

Status: **CORRECTED — focused checks pass; consumer/resource verification in progress.**
This follows the [startup replay input correction](native-replay-input-fence-2026-09.md).
It is local storage consistency work, not new signature, fork-choice, monetary,
public-network or release authority. All timing faults use owned disposable files
and are absent from production builds.

## Selection and reproduced failures

Runtime mutation checked readiness at entry, then could spend time verifying a
block or fork before writing. It also captured a pathname's current metadata
after a durable write, without binding that baseline to the actual descriptor
written. The selected behavior is to preserve detected external changes and
refuse publication from stale inputs, not overwrite evidence or silently reset
to genesis. Each following failure was reproduced through the real `NativeNode`
mutation API before its corresponding correction:

| Boundary | Actual RED | Correction / first focused GREEN |
|---|---|---|
| Block input changes after core preparation | Accepted emptied history: `ready=true, height=2`, 0.15s | Recheck all source versions/ownership before append; 0.11s |
| Fork input changes after candidate validation | Accepted replacement history: `ready=true, height=2`, 0.17s | Recheck before candidate publication; full/recent routes 0.29s |
| Pending input changes after reservation preparation | Accepted lost prior queue: `ready=true, pending=2`, 0.88s | Check storage without committing the borrowed reservation; block/pending pair 0.87s |
| Block file changes after durable append | Adopted the changed pathname as the new baseline: `ready=true, height=2`, 0.20s | Descriptor-observed durable output version; block/pending pair 0.85s |
| Fork file changes after atomic replacement | Adopted changed output: `ready=true, height=2`, 0.31s | Retain actual staging/output descriptor through rename and bind its observed version; full/recent routes 0.56s |
| Old fork input changes after staging, just before rename | Overwrote external evidence and returned ready at height 2, 0.45s | Recheck all source versions/ownership after staging and before rename; routes/timing cases 0.80s |
| Same-length block input changes just before append-open | Accepted corrupted prior bytes and returned ready at height 2, 0.31s | Check the actual opened descriptor against the expected prior version; block/pending pair 1.68s |
| Another authoritative file changes after append | Returned success despite changed manifest, 0.89s | Recheck all sources before memory publication; block/pending paths 1.64s |

These are incremental development failures, not retries of an unchanged product
until a pass. Existing signatures/PoW/accounting were genuinely checked; the
failure was the subsequent source/output file-version relationship.

## Selected implementation and limits

- Before durable append or fork publication, validate state/ledger ownership,
  process write-fence state and every authoritative file version again. A pending
  reservation remains uncommitted while storage is checked; dropping it on error
  leaves prior nonce/balance reservations intact.
- A native append opens an existing expected file without recreating it. Only an
  expected-absent pending file may be newly created, and a foreign file appearing
  first is refused. Before writing, compare the actual descriptor and its path
  to the expected version. Afterward, require the exact expected length increase.
- `DurableFileVersion` retains the actual output descriptor and metadata observed
  there after successful durable I/O. Native callers compare that observed version
  to the current path; they do not establish a new baseline from a later arbitrary
  path lookup. Retaining the descriptor also prevents its inode from being freed
  before this comparison. This adds metadata checks, not a full-history reread on
  every append.
- Atomic row publication keeps its staged descriptor through rename/fsync. Its
  caller-supplied pre-publication check runs after staging and just before rename,
  preserving an input changed while a large candidate was serialized. Existing
  legacy append/line-rewrite callers retain their public `Result<()>` contract;
  their actual durability transaction and rollback/write-fence rules are unchanged.
- Check other authoritative files again before exposing the newly persisted block,
  fork or reservation in memory. Errors after publication fence the live node and
  require restart/reconciliation. They do **not** promise that nothing was written,
  and never erase a durable output merely to make a failed call look rolled back.
  The regression restores only its own known fixture manifest, independently
  reopens, and confirms the already-durable block/pending transaction exactly once.
- Startup's previously added source-version binding and exact read-back after
  permitted repair remain. Source-preserving audit/export still do not repair.
  Exact known-transfer retries still perform their existing signature validation;
  the separate optimization hold is not bypassed.

These checks do not form an atomic transaction across all files and do not defend
against a privileged hostile host changing bytes/metadata inside filesystem
operations. The existing cooperative exclusive-writer and supported-filesystem
assumptions remain. A successful point-in-time check cannot prevent a future
external write. No public listener, operational wallet/key, paid API/model, VM,
consensus/reward activation, GitHub publication or release was used.

## Focused verification

The direct node group passed 13 tests in 6.31s, including all prior replay-input
cases, normal repaired-tail recovery, interrupted append recovery and all six
reorg publication failure points. Common durability passed all 19 direct tests
in 0.06s, including rollback/fence survival, alias refusal and replacement-safe
private temporary directories. An added expected-absent pending-file race passed
with the other pending publication cases in 2.62s: no pending ID/nonce was added,
and the unexpected source file remained byte-for-byte intact.

Further consumer and resource outcomes will be recorded below without replacing
the failed outcomes above.

Consumer checks passed on the corrected runtime: archive 8 tests in 81.29s,
audit 6 in 0.81s, native-node integration 11 in 5.34s, receipt store 2 in 0.03s,
reward store 4 in 0.05s, session store 2 in 0.01s. Real pinned-TLS signed-transfer
gossip/one confirmation passed in 1.23s; partition/rejoin/orphan recovery passed
in 1.67s. All three actual-router HTTP admission/diagnostics tests passed in
10.18s, including caller timeouts retaining admission through actual durable work.
Node all-target clippy with warnings denied passed in 38.46s.

## Preregistered resource follow-up

After focused consumer checks, run the unchanged original scenarios on this
corrected source, one at a time with no intentionally concurrent local build/test
load. Compile first, record source/tree/executable identity before and after, and
invoke the debug test executable directly under `/usr/bin/time -l` with one test
thread. Use only deterministic disposable test keys and numeric loopback peers.

1. The [131,073-balance/full pending scenario](native-capacity-qualification-2026-09.md#fixed-scope-and-acceptance-criteria):
   512 admissions ≤20s; 100 lookups ≤5s; pending and confirmed reopens ≤120s each;
   template/append ≤10s; total ≤900s; history ≤96MiB; RSS ≤1GiB; exact original
   final head, balances, issuance, nonce, confirmed count and empty queue.
2. The [eight distinct valid competing-fork scenario](native-competing-forks-qualification-2026-09.md#fixed-scope-and-acceptance):
   same 131,073-balance starting state, eight fixed sixteen-block candidates and
   simultaneous fifteen-block buffers; normal winner and all peers stably polling
   its head ≤120s; no data re-download after the first completed final-head round;
   winner-only 8,192 added confirmed IDs and 57,344 conflicting loser IDs absent
   from confirmed/pending; exact balances/nonces/supply/locks; drained stop ≤1s;
   independent replay ≤120s and journal equality; template/append ≤10s; total
   ≤900s; history ≤96MiB; RSS ≤1GiB. All transient failed rounds remain recorded.

The test source, data fixtures and thresholds are not changed for this follow-up.
Run the two small companion scenarios first. Neither pass is a universal latency,
malicious-host safety, continuously changing-candidate, arbitrary mixed-traffic,
public-network or R1/R2/R3 qualification.

### Small companions — PASS before large measurement

The full-pending companion passed in 7.440s: 1,025 balances, 512 admissions in
3.304s, 100 lookups in 3.552ms, pending/confirmed reopens 739/585ms, and exact
13-block/1,536-confirmed/824,609-byte final state with empty pending. Maximum
template/append were 167/226ms.

The eight distinct-candidate companion passed in 19.503s: stable final polling
in 6.439s, drained stop 366 microseconds and independent replay 515ms. All eight
clients held fifteen blocks before release. The observer saw the initial head
plus four complete candidate heads (a lower bound of four adoptions); 14 failed
rounds and 55 successful rounds were recorded. Final winner remained candidate
amount 7, with 28 blocks, 1,025 balances, 1,280 confirmed transfers, 700,341
history bytes and no pending. Winner IDs/loser absence/accounting/journals matched.
More observed adoptions and scheduling differ from prior runs; no comparative
performance claim is inferred from these small timings.
