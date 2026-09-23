# Native replay input-version fence — 2026-09-23

Status: **PASS — corrected direct regressions and unchanged large-state criteria.**
This record concerns local boot/replay consistency. It changes no network,
signature, fork-choice or monetary authority and supplies no public launch claim.

## Reproduction and selected boundary

`NativeNode::open_inner` validates a bounded file while reading it, then replays
its retained bytes. It previously captured authoritative file stamps only after
all replay/pending validation. The shared path is also used by source-preserving
offline audit/export. A file changed after its read could therefore become the
new readiness baseline even though those new bytes were never replayed.

The regression calls the real public `NativeNode::open` on an owned disposable
one-block chain. A one-shot test-thread-local filesystem timing hook empties that
same regular file in-place immediately after its read, before core replay. The
hook is absent from production builds; it neither bypasses validation nor
constructs an unchecked chain. It makes an otherwise timing-dependent external
write reproducible through the real open/readiness boundary.

The first fixture attempt failed in 0.09s because it compared the original temp
path alias with the node's canonical path, so the requested timing mutation did
not execute. The fixture now canonicalizes the trigger and requires that the
one-shot mutation was consumed. That ordinary fixture failure is not evidence of
the product defect.

The corrected reproduction then failed in 0.08s with the actual result:

```text
replay accepted a different file version: ready/height/bytes=Some((true, 1, 0))
```

The changed zero-byte source remained on disk, but the returned live node held
the earlier verified one-block chain and reported readiness. This proves the
normal-boot mismatch. At that first reproduction, the common source-preserving
path and other inputs had not yet been separately exercised; their subsequent
regressions are recorded below.

The selected correction is to retain the exact observed file versions with the
read bytes through replay and readiness, and check all original inputs before
any intentional pending cleanup. Existing bounded reads, core validation,
permitted torn-tail recovery, current manifest/schema checks, exclusive ownership
and source-preserving audit/export behavior must remain intact. A mismatch must
fail closed without rewriting an externally changed file into the old snapshot.
This is not protection against a malicious host administrator or a claim of an
atomic multi-file filesystem transaction.

## Incremental correction and observed failures

Each behavior was exercised before its corresponding production correction:

| Boundary | RED result | Correction / first focused GREEN |
|---|---|---|
| Canonical history changes after read | Ready at height 1 with zero-byte history, 0.08s | Retain descriptor-observed version with bytes; ordinary/preserving opens refuse, 0.10s |
| Pending file changes after read | Accepted unvalidated replacement, 0.04s | Retain pending input version; combined two tests 0.11s |
| Manifest changes after validation | Accepted foreign-network manifest, 0.03s | Version-bound bounded read-only validation before replay; combined three tests 0.12s |
| Changed pending input before stale-row cleanup | Replacement evidence overwritten with an empty file, 0.05s | Check every source version and ownership guard before cleanup; combined four tests 0.16s |
| Pending output changes after cleanup publication | Accepted external replacement as new readiness baseline, 0.11s | Read back complete bounded output, compare with exactly serialized retained rows, retain observed version; combined four tests 0.15s |

The last two failures were observed during incremental development, not hidden
by a final pass. The cleanup regression checks changes to canonical history,
pending and manifest before cleanup, plus a changed pending output afterward.
Inputs changed before cleanup remain byte-for-byte in their externally modified
state; none are restored or erased by replay. The tests use only owned fixtures.

Normal startup still allows its existing manifest creation/compatibility update
and torn-tail recovery. After an intentional tail truncation, it reads the
retained prefix back, compares exact bytes and checks the same descriptor/path
version through that read. Source-preserving opens never truncate. Pending
cleanup is still based on fully signature-checked rows and core ledger rules;
its new output stamp is accepted only after exact bounded read-back. No core,
cryptographic or fork-choice checks were removed, memoized or bypassed.

Additional direct regressions exercise canonical truncation, equal-length
in-place corruption and same-content inode replacement; normal and preserving
open must refuse each changed version. The actual `audit_native_state` and
`export_native_archive` APIs are also called for each changed input role, with
no successful report/archive and all source bytes preserved. Existing six reorg
publication fault boundaries and interrupted append recovery still pass.

Direct integration consumers passed on the corrected runtime:

```text
cargo test -p boole-node --test native_node --test native_audit --test native_archive -- --test-threads=1
native_archive: 8 passed, 81.86s (including 1,025-block long-fork recovery)
native_audit:   6 passed, 0.79s
native_node:  11 passed, 5.12s
```

The final direct node-unit group passed all nine tests in 6.25s, including
same-length/inode changes, actual offline consumers, valid pending retention
after two torn-tail repairs, all six reorg publication faults and interrupted
appends. The repaired source then independently audited at height 10 with its
authentic pending transfer retained and exact original complete journal bytes.
Node all-target clippy with warnings denied passed in 38.43s.

## Preregistered large-state follow-up

Before running the corrected source at scale, select the unchanged
[`native_capacity_131072_accounts_and_full_pending_queue` scenario](native-capacity-qualification-2026-09.md#fixed-scope-and-acceptance-criteria).
Its original fixture, assertions and criteria remain: 131,073 balances, 512
pending admissions ≤20s, both independent reopens ≤120s each, measured template/
append ≤10s, total ≤900s, history ≤96MiB, executable maximum RSS ≤1GiB, exact
balances/nonces/issuance/confirmation and final empty queue. The test source is
not changed for this follow-up. The direct debug executable will run under
`/usr/bin/time -l` with one test thread and no intentionally concurrent local
build/test load. Source/tree/executable identity and raw outcome will be recorded.

This specifically checks the changed full-replay startup boundary with both a
full and an empty queue. It does not requalify public peers, continuously changing
forks, arbitrary host interference, all filesystems or the maximum 256MiB state.
The existing external-publication and public-network holds remain unchanged.

### Large-state follow-up — PASS on first execution

Executed source `c970d081626d5e1b0a1f4bf9f0caabfb622e4b57`, tree
`ca243d7da6148ea4ea86a72c8a930aaba13c8959`. The unchanged debug executable's
SHA-256 was `be6dff0e640f7f80a61e18fff3481fe1d575b67b68715589b7976ce9fe73576f`.
Source/tree, clean worktree and executable identity were checked before and after
the run at approximately 12:24–12:29 UTC on the same developer Mac. No other
local build/test load was intentionally scheduled. The capacity test and its
competing-fork support had no diff from the preregistered base.

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_capacity_131072_accounts_and_full_pending_queue \
  --nocapture --test-threads=1
```

All fixed gates passed: 512 pending admissions in 3.280s, 100 lookups in 4.035ms,
independent restart with queue in 52.074s and after confirmation in 52.020s.
Maximum measured template/append were 199/252ms. Scenario time was 265.551s,
canonical history 70,231,145 bytes, and maximum RSS 368,017,408 bytes
(350.96875MiB), below 1GiB. The final 267-block head, 131,073 balances,
131,584 confirmed transfers, sender nonce, issuance and empty queue matched the
original fixed fixture. The original result and this follow-up remain separate;
one host-specific sample is not a general performance improvement or R1/R2 pass.

Unmodified final result and OS measurement:

```text
capacity-result {"confirmedHead":"0000da6a4a5bd3939b76de746634716bc105be8aa6fb813ab4a4586ecbc17dee","elapsedMs":265551,"fundedBlocks":256,"fundedHead":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","issued":"1335000000000000","lookup100Micros":4035,"maxAppendMs":252,"maxTemplateMs":199,"networkId":"boole-native-testnet-1","pendingAdmissionMs":3280,"resources":{"balanceEntries":131073,"confirmedTransfers":131584,"historyBlocks":267,"historyBytes":70231145,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartConfirmedMs":52020,"restartPendingMs":52074,"transfersPerBlock":512}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 11 filtered out; finished in 265.60s

      267.06 real       261.40 user         0.86 sys
           368017408  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
               27579  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                   0  messages sent
                   0  messages received
                   0  signals received
                3257  voluntary context switches
                2305  involuntary context switches
       4080062280965  instructions retired
       1123704707247  cycles elapsed
           197624480  peak memory footprint
exit: 0
```
