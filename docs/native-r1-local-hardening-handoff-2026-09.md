# Native R1 local hardening integration — 2026-09-23

Status: **LOCAL INTEGRATION VERIFIED; ordinary GitHub publication/CI/merge authorized September 25.**
R1 is incomplete, R2 has not passed and R3 has not started. This record locates
the work completed during the operator's twelve-hour autonomous-development
window; it does not turn local results into release or execution authority.

**Publication resumption, September 25:** after the proposed next step was
explained as uploading the prepared code, passing official automated checks and
merging, the user instructed “진행해”. This resolves only the publication-approval
hold. The exact source/test/documentation feature branch is in scope; the
pre-existing `tasks/lessons.md` edit, ignored local plans and operator secrets or
state are excluded. Required full CI and ordinary protected-branch merge remain
mandatory. The [owning PR #388](https://github.com/NotoriAndo/Boole/pull/388)
records their actual result. Public P2P and the signature shortcut remain held;
the expired twelve-hour automation is not resumed.

**Window closed:** the delegated interval was 2026-09-23 04:02:30–16:02:30 UTC
(ending September 24 at 01:02:30 KST). Its follow-up automation was verified
PAUSED at closeout, with the original prompt, schedule and target preserved.
No new feature or experiment was started after the deadline. Final runtime,
test and dependency sources remain identical to `df6a32c`; subsequent commits
record verification and reconcile the already-selected operator-local RPC scope.
At window close the project-root integration branch was the handoff checkout,
while main and all three safety holds below remained unchanged. Only the pre-existing user edit in
`tasks/lessons.md` remains uncommitted; no clean-worktree claim is made.

## Main versus the prepared integration

- Recorded main `9be7cb0cd4e2f5c41ec09cd50709bb6a7e7e6960` includes native
  transfers, pinned-TLS peer integration (PR #386) and encrypted wallet recovery
  (PR #387). The original wallet feature `871b50374532737cb60dbeae5bde5e24f200c2a0`
  has exactly the same tree, `6180fdc3511d489fa4f8d4af19ca757bf702edd7`.
- The unmerged follow-up stack ended at
  `fccef21e7a543f5aaff79c7a682d31bf26c476f8`. Its 41 subsequent commits were copied
  into the new local branch `codex/native-r1-hardening-integration` and rebased
  from that identical wallet tree onto recorded main. No original branch or
  evidence commit was rewritten, and main was not changed.
- The initial rebased integration tip is
  `21df6930bbf17e2a4758a78285ab2d0f2bbec960`. Its tree and the original follow-up
  tip's tree are both `ae6e2e20872ab0f2cfe75e6faee62e9acad968e0`;
  `git diff --exit-code fccef21 21df693` passed. This avoids including already
  merged secure-peer/wallet patches again in the future PR.
- Preparation used `/private/tmp/boole-r1-integration-worktree.LGTdSb`; after
  integrated verification, the same feature branch was checked out at the durable
  project root `/Users/seoyong/projects/Boole`. Main remains at `9be7cb0`.
  All 23 worktrees created for this follow-up were checked clean, including
  ignored/untracked files, and removed without force. Every original branch and
  measurement commit was retained. Other sessions' worktrees were not pruned.
  The pre-existing `tasks/lessons.md` user edit stayed byte-identical and excluded.
  Earlier temporary paths in raw output identify historical runs; subsequent
  prose changes are not a claim that those executables contained later docs.

All new local follow-up changes use the now-authorized publication path, the
required full CI and ordinary review/merge. A compile, local pass, exact-tree rebase or
valid document is not a substitute. No main push, protection bypass or public
release was performed.

## Completed local behavior and reasons for selecting it

| Boundary | Result and reason | Owning evidence |
|---|---|---|
| Offline state recovery and accounting | Source-preserving audit/export, bounded import with independently retained expected head, long-fork recovery and missing-canonical-file refusal; recover into a new directory without silently inventing genesis | [Native recovery/audit contract](native-transfer-ledger-contract.md#bounded-offline-chain-recovery), [operator runbook](native-operator-recovery-runbook.md) |
| Pending work and recent forks | Durable-before-memory reservations/ID index and locally verified inverse transitions reduce repeated work while preserving signatures, nonce, fees, maturity, issuance and full replay; no external balance checkpoint is trusted | [Capacity](native-capacity-qualification-2026-09.md), [recent-fork correction and failed baseline](native-recent-fork-qualification-2026-09.md) |
| Peer progress and bounded repeated work | One outgoing worker per configured peer, bounded unchanged losing-fork memory, failure-phase observation and cancellable unstarted state-lock waits; a stalled peer must not serialize all healthy I/O | [Repeat qualification](native-peer-repeat-qualification-2026-09.md), [lock-wait correction](native-peer-lock-wait-2026-09.md) |
| Local admission and diagnosis | Eight ordinary permits precede body decoding and survive caller timeout until actual work finishes; two independent diagnostic slots do not claim ledger readiness; signed outboxes can be inspected offline without payment authority | [RPC/diagnostic and inspection contract](native-transfer-ledger-contract.md#native-loopback-rpc-bounds) |
| Normal native HTTP shutdown | Five-second client-I/O drain then actual socket closure; connection slots survive shutdown-descriptor cleanup, final-owner cleanup is bounded/error-reporting, and already admitted durable work remains protected and replays once | [Shutdown correction, failed outcomes and bounded alternative](native-http-shutdown-drain-2026-09.md) |
| Replay and durable publication | Reproduced late-baseline/changed-input failures now bind verified inputs and actual output descriptors, preserve changed evidence and fence ambiguous post-I/O errors; errors after a durable write still require exact-ID reconciliation | [Replay input correction](native-replay-input-fence-2026-09.md), [publication correction and qualification](native-publication-input-fence-2026-09.md) |
| Operator and membership recovery | Real three-process transfers, partition/rejoin, orphan re-confirmation, restored wallet/node and key retirement/re-enrollment preserve ownership, original material and exactly-once accounting | [Operator readiness rehearsal](native-testnet-readiness.md), [key retirement rehearsal](native-peer-key-rotation-2026-09.md) |
| Bounded state and input pressure | Preregistered original large-state, eight-candidate and combined 8/4/8-input scenarios retain their failed outcomes/thresholds and independently replay exact final state | [Competing forks](native-competing-forks-qualification-2026-09.md), [latest publication follow-ups](native-publication-input-fence-2026-09.md#large-follow-ups--pass-on-the-first-corrected-source-runs), [mixed input pressure](native-mixed-resource-qualification-2026-09.md) |
| Actual first-halving recovery | 10,024 real PoW/authorized blocks exercise halved issuance, reward maturity, removal of old funding by a recent fork, and exactly-once recovery of the original signature after new funding matures | [First-halving qualification, including the instrumentation failure](native-halving-recovery-qualification-2026-09.md) |

These choices implement the initially selected explicit peer-key membership and
operator-local RPC scope. They do not implement anonymous enrollment, a public
RPC gateway, hot key revocation, a public listener, operational custody or finality.

## Latest bounded resource results, not a universal capacity claim

All rows are developer-Mac/debug, disposable closed-local measurements. They are
separate scenarios; do not add their throughput or combine their memory peaks.

| Scenario | Latest observed outcome | Important limit |
|---|---|---|
| 131,073 balances, full 512-transfer queue, two reopens | 3.293s admission; 52.214s/52.163s reopens; 350.671875MiB RSS; exact original final head/accounting | One funded sender/recipient pattern, not the complete 256MiB/100,000-block envelope |
| Eight distinct valid competing 16-block branches at that state | 66.633s stable winner polling; 55.585s replay; 767.703125MiB RSS; 28 failed rounds retained; only winning 8,192 added IDs confirmed | Fixed candidates; not arbitrary changing candidates, concurrent successful RPC/fork traffic or hard read-latency guarantees |
| Eight incomplete thirty-block buffers + four authenticated inbound waits + eight near-8MiB HTTP uploads | After HTTP shutdown correction: 3.118s input/rejection; 39.786ms drained stop; 456µs maximum of ten diagnostic calls; 51.871s replay; 623.875MiB RSS; exact original source-state bytes unchanged | Same fixed incomplete-input criteria; not successful mixed fork validation or a public availability SLA; [original and follow-up sources remain separate](native-http-shutdown-drain-2026-09.md#unchanged-large-mixed-resource-follow-up--first-corrected-source-run-pass) |
| Actual first halving, recent fork, reward maturity and exact-signature recovery through height 10,024 | 876.337s scenario; 384ms fork adoption; 5.568s/5.583s reopens; 5.580s audit; 52.875MiB RSS; exact issuance/payment/locks | Three accounts and mostly empty blocks, not all halvings, maximum storage or large-account capacity; one earlier functional pass lacked OS RSS and is retained separately |
| Normal shutdown while all 8/4/8 inputs remain held at 131,073 balances | After slot-lifetime correction: 5.071s stop; all eight HTTP/twelve TLS connections closed without completing missing input; 51.647s replay while old clients/runtime remain alive; 621.75MiB RSS; exact original state | [Separate held-input criteria and corrective result](native-http-shutdown-drain-2026-09.md#large-held-input-follow-up-after-slot-lifetime-correction--pass); no in-flight ledger mutation in this scenario, not a whole-process CPU/disk time guarantee |

The exact-transfer retry optimization is **not implemented**. Its original
8,192-retry baseline took 2.748584083s against the fixed one-second criterion;
that remains [FAIL, with the shortcut on hold](native-known-transfer-qualification-2026-09.md).
Known retries still execute existing immutable signature validation. Passing
other resource scenarios neither overrides that failed criterion nor authorizes
a signature-validation change.

## Holds and genuine remaining decisions

1. **External GitHub publication — approval resolved September 25:** the safety
   review had rejected push/PR payload transmission twice during the earlier
   window. The user's subsequent instruction authorizes source/test/docs
   publication and the required-CI/ordinary-merge path. Earlier rejection evidence
   remains valid; this is new authority, not a bypass or a claim of CI success.
2. **Non-loopback P2P:** a proposed explicit exposure option was rejected before
   application. It remains unimplemented; all current bind/peer/client loopback
   guards remain. No tunnel/proxy/firewall workaround is approved.
3. **Known-transfer signature shortcut:** safety review held the optimization;
   the supporting ID-binding/storage-loss tests did not bypass that decision.
4. **Launch:** R1's broader accepted operating envelope/public exposure,
   authenticated native release, actual custody, participant topology/notice,
   supported-platform evidence (clean-Mac CURL.3 last or an explicit narrower
   scope), incident ownership and explicit R3 execution authority remain in the
   [readiness checklist](native-testnet-readiness.md).

No additional actual-model/paid-API run, VM run, operator wallet transaction,
operational mining, public deployment or reward/activation was performed.
Latest paid verification buyer/LOI counts were not newly verified; no old count
is promoted to a current measurement. Useful-work readiness/activation holds
remain separate from ordinary native test-coin development.

## Integrated consumer verification

### Recommended review and continuation order

The closing source review prioritizes the following consumers; it is not an
independent external review, a new full-suite result or permission to resume any
held action.

1. **Storage and accounting first:** review
   [native node publication/recovery](../crates/boole-node/src/native_node.rs),
   [shared durability helpers](../crates/boole-node/src/durability.rs),
   [manifest reading](../crates/boole-node/src/state_dir.rs) and
   [ledger reservations/undo](../crates/boole-core/src/native_ledger.rs).
   Follow their direct failure-boundary tests and the replay/publication records.
   Shared helpers also have legacy consumers, which is one reason required full
   CI remains necessary. An error after I/O is an unknown durable outcome, not
   rollback: retain original data and reconcile the same transaction/block ID.
2. **Admission and actual work lifetime:** review
   [native HTTP](../crates/boole-node/src/native_http.rs), the
   [shared bounded listener](../crates/boole-node/src/local_node.rs) and
   [native peers](../crates/boole-node/src/native_peers.rs). Keep connection,
   request, diagnostic and peer limits distinct. A caller timeout must not free
   already-running work, a process diagnostic is not ledger readiness, and
   closing client sockets does not authorize interrupting durable publication.
3. **Operator-visible recovery:** review
   [archive/audit](../crates/boole-node/src/native_archive.rs),
   [native CLI](../crates/boole-cli/src/native.rs) and the real three-process/
   owner-wallet tests linked above. Expected heads, original outboxes, independent
   accounting, key-role separation and preservation of old material are required
   outcomes, not optional cleanup details.
4. **Publication under the September 25 authorization:** recheck the actual base
   and worktree, preserve the user's unrelated edit and original evidence refs,
   then use the feature-branch/PR/full-CI/review/merge path. The local
   main/ref snapshot here is not a fresh remote-status assertion. Do not reset
   acceptance thresholds, omit failing retry evidence or describe developer-Mac
   measurements as cross-platform CI qualification.
5. **Remaining R1 work stays explicitly unqualified:** preregister a selected
   near-storage-limit envelope and successful mixed RPC/P2P workload before
   running them. Pin source/hardware, head/accounting invariants, time/RSS/disk
   budgets and failure/stop criteria; begin with a small direct companion. The
   current ~67MiB many-account history and ~7.5MiB mostly-empty halving history
   do not cover the 256MiB/100,000-block ceiling. Public P2P, independent hosts,
   actual custody/release/participants and R2/R3 remain separately held or absent;
   no local scenario can manufacture those decisions or external evidence.

### Local verification and required CI

The source-to-main range was passed through the existing local
`scripts/ci_change_scope.py`; it reports `process_only=false`. The original
publication head `8432ad7` changed no workflow, classifier, full-suite selection
or dependency lock. Its first full CI exposed the orchestration-budget issue
recorded below; only the amd64 global/job time allowances and their direct tests
were corrected. The classifier, required checks, frozen inner deadlines and
dependency lock remain unchanged. The explicitly
ignored developer-Mac large capacity/halving scenarios remain separate measured
evidence; a routine CI pass must not be described as re-running them.

### September 25 publication: initial CI timeout and orchestration correction

`8432ad7000546c296556b3543dea1e15f56286bb` was published in
[PR #388](https://github.com/NotoriAndo/Boole/pull/388), based on freshly checked
remote main `9be7cb0`. The range secret scan, docs-smoke and diff check passed;
the user's unrelated `tasks/lessons.md` edit was excluded and stayed identical.
The [first full CI](https://github.com/NotoriAndo/Boole/actions/runs/36066002628)
is retained as a failed attempt, not replaced by a later success. Its
[amd64 rootfs replay job](https://github.com/NotoriAndo/Boole/actions/runs/36066002628/job/107855834792)
exited 124 after 21m23s overall: exact rootfs construction, categorical Cargo
diagnostics, actual MCP positive/negative/redelivery checks, crash/restart and
the historical canary reported PASS before the enclosing manager hit its
1,200-second wall-clock cap. There was no final development-task PASS, so that
attempt does not qualify the complete matrix. The separate
[two-OS, four-configuration verdict workflow](https://github.com/NotoriAndo/Boole/actions/runs/36066002713)
passed all six jobs.

The wrapper still used the pre-development-task global allowance, despite
running the separately bounded 900-second task phase. The correction follows
the existing arm64 orchestration pattern: retain 1,200 seconds plus that
900-second phase (2,100 seconds total), with a 45-minute workflow allowing
600 seconds outside the manager. This is CI orchestration, not a change to
checker policy, sandboxing, service/cleanup/HTTP deadlines, reward rules or any
preregistered native resource threshold. No failed test is skipped or relabeled.
The runtime, product tests and dependencies remain identical to `df6a32c`.

A regression executes the actual shell timeout invocation with only the
external timeout command stubbed; it does not invoke sudo, a checker or Linux
services. It observed the old 1,200-second argument (RED) and the corrected
2,100-second argument with workflow reserve (GREEN). Ninety-six direct workflow,
crash/restart, canary, bounded-retry and classifier tests then passed. The initial
test-name typo and missing scratch fixture variable were test setup errors, not
the RED result. The first broad local invocation also had two sandbox-denied
temporary Unix socket binds; the same tests passed with that local permission.
Actual full Linux CI at the corrective head is mandatory before merge; the Mac
contract test alone does not establish that the full matrix finishes.

Retained failure excerpt:

```text
native-shadow production real MCP trace gate: PASS
native-shadow production crash/restart replay gate: PASS
fresh-answer-canary:real-mcp-contained-checker:PASS
Process completed with exit code 124.
```

### Earlier integrated consumer verification

The exact-tree integration passed the actual CLI three-node operator rehearsal
and combined owner-wallet/node recovery consumers after the publication-fence
corrections. Their fresh outcomes are recorded below.
Earlier focused checks and every failed/successful qualification remain in the
owning records above; this is not a new full workspace or CI run.


Both integrated consumers passed on the exact-tree integration `21df693`:

- `native_operator_rehearsal::three_real_nodes_transfer_partition_rejoin_and_restore_without_duplicate_payment`:
  scenario 78.388s; harness 90.34s including its 11.92s nested sibling-binary build.
  Three actual processes agreed at height 15, preserved all original material,
  requeued/reconfirmed the exact orphaned transaction, and independently audited
  three transfers totaling 375,000,000 amount atoms and 3,000 fee atoms.
  Issued/balance total was 75,000,000,000,000 atoms; pending was empty. The random
  wallet fixture's final head is recorded below, not expected to equal prior runs.
- `native_cli::encrypted_owner_cli_mines_transfers_and_retries_only_the_saved_signed_transaction`:
  harness 60.83s including a 6.54s nested node build. This covers the restored
  encrypted owner, bounded process-only diagnostics during readiness failure,
  normal restart, audit/export/fresh import, old outbox reconciliation without
  duplicate payment and a separately authorized nonce-1 transfer in the fixture.

Both processes exited 0 on their first integrated-source executions; runtime
source and test definitions were unchanged by this handoff documentation. No
additional large timing scenario was repeated merely because commit IDs changed.
The earlier resource measurements remain attached to their actual original
source/executable identities; exact-tree ancestry changes do not rewrite them.

Commands:

```sh
CARGO_TARGET_DIR=/Users/seoyong/projects/Boole/target cargo test -p boole-cli --test native_operator_rehearsal -- --nocapture --test-threads=1
CARGO_TARGET_DIR=/Users/seoyong/projects/Boole/target cargo test -p boole-cli --test native_cli -- --nocapture --test-threads=1
```

Raw integrated-consumer output (public disposable fixture identities only):

```text
   Compiling tokio-rustls v0.26.4
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling hyper-rustls v0.27.9
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
   Compiling boole-wallet-agent v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-wallet-agent)
   Compiling boole-testkit v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-testkit)
   Compiling reqwest v0.12.28
   Compiling boole-miner v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-miner)
   Compiling boole-cli v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 12.51s
     Running tests/native_operator_rehearsal.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_operator_rehearsal-d4777efa06a28b99)

running 1 test
test three_real_nodes_transfer_partition_rejoin_and_restore_without_duplicate_payment ...    Compiling ring v0.17.14
   Compiling boole-core v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-core)
   Compiling boole-native-shadow-protocol v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-native-shadow-protocol)
   Compiling boole-lean-runner v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-lean-runner)
   Compiling boole-evm-adapter v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-evm-adapter)
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-wallet-agent v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-wallet-agent)
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 11.92s
native-operator-phase process-ready elapsedMs=34
native-operator-phase process-ready elapsedMs=35
native-operator-phase process-ready elapsedMs=35
native-operator-phase head-agreement elapsedMs=500
native-operator-phase reciprocal-pinned-peers elapsedMs=2
native-operator-phase transaction-agreement elapsedMs=127
native-operator-phase head-agreement elapsedMs=111
native-operator-phase transaction-agreement elapsedMs=0
native-operator-phase transaction-agreement elapsedMs=137
native-operator-phase head-agreement elapsedMs=88
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=17
native-operator-phase process-ready elapsedMs=32
native-operator-phase head-agreement elapsedMs=37
native-operator-phase transaction-agreement elapsedMs=0
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=52
native-operator-phase head-agreement elapsedMs=1
native-operator-phase transaction-agreement elapsedMs=1702
native-operator-phase head-agreement elapsedMs=431
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=15
native-operator-stop elapsedMs=16
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=35
native-operator-stop elapsedMs=12
native-operator-result {"audit":{"accounting":{"balanceAtoms":"75000000000000","issuedAtoms":"75000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"25000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"375000000","count":3,"feeAtoms":"3000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"0000587cafca431222e89429c0d6b965bd3d3197e70f2f1e0053112ed98caba8","height":"15","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":3,"historyBlocks":15,"historyBytes":13262,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":3,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"balancesAtoms":["64999825000000","5000050000000","5000125000000"],"elapsedMs":78388,"head":"0000587cafca431222e89429c0d6b965bd3d3197e70f2f1e0053112ed98caba8","lockedAtoms":["40000000000000","5000000000000","5000000000000"],"originalsPreserved":true,"orphanedHead":"00000dd654f7bfd000cb6bc1721a71ee2e53c6b1a24960b193ed564f2af85a18","owners":["e966bb63d4359c4ae821e439a43cf3fa32ba753146179b1c80cd6e9333f52b67","4dc81c4ce41f6fa333e160a849ebdef38ea078ee0f0588cb666ead5812834af8","fc42148c226feba30f11e5d574f9f3436cec6e8e463b778946462a137506f831"],"publicActivation":false,"scope":"one-host-three-loopback-processes","spendableAtoms":["24999825000000","50000000","125000000"],"transactionIds":["a9bd326b3fb90ab82c5d1ec7230ed3ce9d2ccfd830f133ec2b775d2a44ee2b7d","f24310e3c98f5bce4a930a0f1b64f1657cb776186d33fd2d80c0eaa306c8aaa3","5b2decf92ca1eb6fa65ca05458d39467eb5324e91f7b0d0370dfd1e7a88c2d5b"]}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 90.34s

   Compiling ring v0.17.14
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling tokio-rustls v0.26.4
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling hyper-rustls v0.27.9
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
   Compiling reqwest v0.12.28
   Compiling boole-miner v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-miner)
   Compiling boole-cli v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.76s
     Running tests/native_cli.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_cli-580d9a89c2e7fc2b)

running 1 test
test encrypted_owner_cli_mines_transfers_and_retries_only_the_saved_signed_transaction ...    Compiling ring v0.17.14
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.54s
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 60.83s
```

### Integrated actual-process follow-up after HTTP shutdown correction

Local merge `27081dee998dc50261783ead5fb723d0ec05fbf2` (tree
`6cfbd821b3f69a7e2a9bf0f0cea76a9b3c004bfa`) retains the HTTP preregistration,
runtime correction and measured-result commits without rewriting them. Only
the handoff result table conflicted; both the first-halving result and latest
mixed-resource follow-up were preserved. `git diff ecb549c..27081de -- crates
Cargo.toml Cargo.lock` is empty: executed runtime/tests are identical to the
HTTP-qualified source, while both lines of documentation are integrated.

The actual three-process operator rehearsal passed on this integration in
79.951s (90.86s harness, including a 10.88s sibling build; outer build 20.35s).
Normal process stops were 12–17ms. All three processes agreed at height 15,
restored-wallet/partition/rejoin/original-outbox recovery passed, and independent
audits matched the same expected accounting: 750,000 tBOOLE issued, three
transfers totaling 3.75 tBOOLE, 3,000 fee atoms and empty pending. All original
material remained intact. Random fixture identities/head differ from the older
run as expected; no rule or acceptance criterion changed. This is a direct
consumer follow-up, not required CI, an external-operator run or launch approval.

```text
   Compiling rustls v0.23.45
   Compiling tokio-rustls v0.26.4
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
   Compiling hyper-rustls v0.27.9
   Compiling reqwest v0.12.28
   Compiling boole-miner v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-miner)
   Compiling boole-cli v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 20.35s
     Running tests/native_operator_rehearsal.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_operator_rehearsal-d4777efa06a28b99)

running 1 test
test three_real_nodes_transfer_partition_rejoin_and_restore_without_duplicate_payment ...    Compiling ring v0.17.14
   Compiling boole-wallet-agent v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-wallet-agent)
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.88s
native-operator-phase process-ready elapsedMs=33
native-operator-phase process-ready elapsedMs=31
native-operator-phase process-ready elapsedMs=29
native-operator-phase head-agreement elapsedMs=204
native-operator-phase reciprocal-pinned-peers elapsedMs=469
native-operator-phase transaction-agreement elapsedMs=162
native-operator-phase head-agreement elapsedMs=74
native-operator-phase transaction-agreement elapsedMs=1
native-operator-phase transaction-agreement elapsedMs=128
native-operator-phase head-agreement elapsedMs=292
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=17
native-operator-phase process-ready elapsedMs=29
native-operator-phase head-agreement elapsedMs=37
native-operator-phase transaction-agreement elapsedMs=0
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=45
native-operator-phase head-agreement elapsedMs=1
native-operator-phase transaction-agreement elapsedMs=1450
native-operator-phase head-agreement elapsedMs=505
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=12
native-operator-stop elapsedMs=14
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=35
native-operator-stop elapsedMs=16
native-operator-result {"audit":{"accounting":{"balanceAtoms":"75000000000000","issuedAtoms":"75000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"25000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"375000000","count":3,"feeAtoms":"3000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"0000fe84819bb00d4c17a49d0d0baf2b0cd3518d8aca464c64bd0597f9d46fcc","height":"15","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":3,"historyBlocks":15,"historyBytes":13266,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":3,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"balancesAtoms":["64999825000000","5000050000000","5000125000000"],"elapsedMs":79951,"head":"0000fe84819bb00d4c17a49d0d0baf2b0cd3518d8aca464c64bd0597f9d46fcc","lockedAtoms":["40000000000000","5000000000000","5000000000000"],"originalsPreserved":true,"orphanedHead":"0000c90680b33236b808511cbf3fb89659ba169aa68ef26ce5bc3b936a7a8d1d","owners":["36edc2f4fff419cafd0100f3d9732ea44d0f73f09e7cefe01f52801f0dd3ab1b","50a23f70f75ef4f6dfff75ee1b0d6694b2e2bb90a2fa6699fc6282027d06ef61","f7f9707d949950d521917a42b8e8c2900e1d6864ad633607611c965dc86cb768"],"publicActivation":false,"scope":"one-host-three-loopback-processes","spendableAtoms":["24999825000000","50000000","125000000"],"transactionIds":["928113da37391b84f1ad132d38dd125632565730f35e527d38f7d48c8b899bdd","1ac6ca00cfdc82341e9a37329f07f67211791469d2a825614ff145a0ff4ec7b4","0e7fcd4b14eeab372c9697c5053704341c512878f9b10d6af6341c93d67b791b"]}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 90.86s
```

### Final actual-process follow-up after connection-slot lifetime correction

After runtime correction `df6a32c`, the same three-process rehearsal passed on
`92781ce19ac0ba75a48083b0e142455135a86282`; runtime/test/dependency diff from
`df6a32c` was empty. This run was selected because the final correction also
affects actual foreground-node connection cleanup, not merely an in-process
test. The root contained the excluded pre-existing user edit and handoff prose
being prepared; it is not claimed to have been a fully clean worktree.

Scenario time was 79.207s, harness 86.33s including the 7.10s sibling build,
and outer build 11.25s. Normal process stops were 16–17ms. The unchanged
15s convergence/readiness, 3s normal-stop and 180s scenario criteria passed.
All three independent audits matched the expected issuance, locked/spendable
amounts, three original transfer IDs and empty pending at height 15. The
restored-wallet transfer, actual partition/rejoin and orphan reconfirmation,
fresh-node restore and old-outbox reconciliation all passed with original
wallet/backup/transport-key/archive/outbox/state bytes preserved. The final
head for these fresh disposable owners was
`00005e9815bd89dbefc1fc00aaa482e1610573b3e4f13dada38763b2c20c88ad`.
No acceptance criteria, product code or signatures were changed for this run.
This remains one-host functional evidence, not full CI or R2/R3 authority.

```text
   Compiling rustls v0.23.45
   Compiling boole-core v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-core)
   Compiling boole-native-shadow-protocol v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-native-shadow-protocol)
   Compiling boole-evm-adapter v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-evm-adapter)
   Compiling tokio-rustls v0.26.4
   Compiling boole-p2p v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-p2p)
   Compiling hyper-rustls v0.27.9
   Compiling boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
   Compiling boole-wallet-agent v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-wallet-agent)
   Compiling boole-testkit v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-testkit)
   Compiling reqwest v0.12.28
   Compiling boole-miner v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-miner)
   Compiling boole-cli v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-cli)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 11.25s
     Running tests/native_operator_rehearsal.rs (target/debug/deps/native_operator_rehearsal-d4777efa06a28b99)

running 1 test
test three_real_nodes_transfer_partition_rejoin_and_restore_without_duplicate_payment ...    Compiling ring v0.17.14
   Compiling boole-wallet-agent v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-wallet-agent)
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-p2p v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/Users/seoyong/projects/Boole/crates/boole-node)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.10s
native-operator-phase process-ready elapsedMs=28
native-operator-phase process-ready elapsedMs=26
native-operator-phase process-ready elapsedMs=35
native-operator-phase head-agreement elapsedMs=496
native-operator-phase reciprocal-pinned-peers elapsedMs=423
native-operator-phase transaction-agreement elapsedMs=235
native-operator-phase head-agreement elapsedMs=512
native-operator-phase transaction-agreement elapsedMs=1
native-operator-phase transaction-agreement elapsedMs=108
native-operator-phase head-agreement elapsedMs=525
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=17
native-operator-phase process-ready elapsedMs=36
native-operator-phase head-agreement elapsedMs=177
native-operator-phase transaction-agreement elapsedMs=0
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=50
native-operator-phase head-agreement elapsedMs=1
native-operator-phase transaction-agreement elapsedMs=1151
native-operator-phase head-agreement elapsedMs=110
native-operator-phase transaction-agreement elapsedMs=1
native-operator-stop elapsedMs=16
native-operator-stop elapsedMs=16
native-operator-stop elapsedMs=16
native-operator-phase process-ready elapsedMs=35
native-operator-stop elapsedMs=16
native-operator-result {"audit":{"accounting":{"balanceAtoms":"75000000000000","issuedAtoms":"75000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"25000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"375000000","count":3,"feeAtoms":"3000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"00005e9815bd89dbefc1fc00aaa482e1610573b3e4f13dada38763b2c20c88ad","height":"15","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":3,"historyBlocks":15,"historyBytes":13265,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":3,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"balancesAtoms":["64999825000000","5000050000000","5000125000000"],"elapsedMs":79207,"head":"00005e9815bd89dbefc1fc00aaa482e1610573b3e4f13dada38763b2c20c88ad","lockedAtoms":["40000000000000","5000000000000","5000000000000"],"originalsPreserved":true,"orphanedHead":"00003bc5e281d598234456440e7eda4fce843cca8af976d847336657a74753c7","owners":["b5911a35615a138d1d11d1f4049974d410535ce75e916eb99b6841a9ba10ddb9","864edc1c2a399e2beb12d54cf77b6f19f283d004f402e00fac0ab44c592b33e4","3c9f09dcd5d1620b72a717b1495b31c43d30042a8f1c76d974912548c3f07ada"],"publicActivation":false,"scope":"one-host-three-loopback-processes","spendableAtoms":["24999825000000","50000000","125000000"],"transactionIds":["e74dcbed896fd12cfba7ad73cc51df1258aa10aba3dae984b90a5a2ae10848a5","8817d7cd56e0d1e5fea0c8101bb27245af4d2655b39b7ea9e3260e1cc0f5e817","148aaf9e1c025639538e5d15819b81070ba5a5bcf70684264e2ddafbdc926560"]}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 86.33s
```
