# Native R1 local hardening integration — 2026-09-23

Status: **LOCAL INTEGRATION PREPARED; external publication/CI/merge remain on hold.**
R1 is incomplete, R2 has not passed and R3 has not started. This record locates
the work completed during the operator's twelve-hour autonomous-development
window; it does not turn local results into release or execution authority.

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
- The preparation worktree is `/private/tmp/boole-r1-integration-worktree.LGTdSb`.
  The root working copy remains on main with its pre-existing `tasks/lessons.md`
  user edit preserved and excluded. Subsequent handoff-document changes are not
  a claim that the earlier measured executables contained those later prose bytes.

All new local follow-up changes still need authorized publication, the required
full CI and ordinary review/merge. A compile, local pass, exact-tree rebase or
valid document is not a substitute. No main push, protection bypass or public
release was performed.

## Completed local behavior and reasons for selecting it

| Boundary | Result and reason | Owning evidence |
|---|---|---|
| Offline state recovery and accounting | Source-preserving audit/export, bounded import with independently retained expected head, long-fork recovery and missing-canonical-file refusal; recover into a new directory without silently inventing genesis | [Native recovery/audit contract](native-transfer-ledger-contract.md#bounded-offline-chain-recovery), [operator runbook](native-operator-recovery-runbook.md) |
| Pending work and recent forks | Durable-before-memory reservations/ID index and locally verified inverse transitions reduce repeated work while preserving signatures, nonce, fees, maturity, issuance and full replay; no external balance checkpoint is trusted | [Capacity](native-capacity-qualification-2026-09.md), [recent-fork correction and failed baseline](native-recent-fork-qualification-2026-09.md) |
| Peer progress and bounded repeated work | One outgoing worker per configured peer, bounded unchanged losing-fork memory, failure-phase observation and cancellable unstarted state-lock waits; a stalled peer must not serialize all healthy I/O | [Repeat qualification](native-peer-repeat-qualification-2026-09.md), [lock-wait correction](native-peer-lock-wait-2026-09.md) |
| Local admission and diagnosis | Eight ordinary permits precede body decoding and survive caller timeout until actual work finishes; two independent diagnostic slots do not claim ledger readiness; signed outboxes can be inspected offline without payment authority | [RPC/diagnostic and inspection contract](native-transfer-ledger-contract.md#native-loopback-rpc-bounds) |
| Normal native HTTP shutdown | Five-second client-I/O drain then actual socket closure; final-owner cleanup is bounded/error-reporting, while already admitted durable work remains protected and replays once | [Shutdown correction, failed outcomes and bounded alternative](native-http-shutdown-drain-2026-09.md) |
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

The exact-transfer retry optimization is **not implemented**. Its original
8,192-retry baseline took 2.748584083s against the fixed one-second criterion;
that remains [FAIL, with the shortcut on hold](native-known-transfer-qualification-2026-09.md).
Known retries still execute existing immutable signature validation. Passing
other resource scenarios neither overrides that failed criterion nor authorizes
a signature-validation change.

## Holds and genuine remaining decisions

1. **External GitHub publication:** the safety review rejected push/PR payload
   transmission twice. A user approval request is pending. The prepared branch
   has not been pushed, has no new PR/required-CI pass and is not merged.
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
