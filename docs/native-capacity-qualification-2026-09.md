# Native state capacity qualification — 2026-09-23

Status: **PASS — initial retry 1 and the recorded new-product follow-up.** This is a disposable closed-local
engineering scenario, not a public benchmark or an operational wallet run.
The existing [native contract](native-transfer-ledger-contract.md) and monetary,
signature, replay, file-ownership and publication rules remain unchanged.
The later [replay-input version correction and requalification](native-replay-input-fence-2026-09.md)
uses these unchanged criteria and records its own source/results: 52.074s/52.020s
reopens and 350.96875MiB peak RSS, with identical final head/accounting/history.
It does not replace the historical measurements below.
The subsequent [runtime publication-fence follow-up](native-publication-input-fence-2026-09.md#large-follow-ups--pass-on-the-first-corrected-source-runs)
also passed these unchanged criteria: 52.214s/52.163s reopens, 350.671875MiB RSS
and the same exact final head/accounting/history. Its source and raw results
remain in that separate record.

## Fixed scope and acceptance criteria

These criteria are recorded before the capacity run. A failed criterion remains
a failed result; a corrective implementation or a new qualified envelope must be
reported separately, not relabeled by weakening the original threshold.

- One newly created private temporary state directory, one deterministic test
  producer and no operator keys, paid APIs, VM execution or public listener.
- The compiled `boole-native-testnet-1` policy and its current genesis, actual
  PoW and network-bound producer/owner signatures. No bypassed validation,
  imported balance snapshots, no-op filesystem or mocked journal.
- Ten initial empty blocks, then 256 blocks containing 512 transfers each to
  131,072 distinct recipient addresses. Each transfer moves one atom and pays
  the actual minimum fee. The producer is the sender, so confirmed fees return
  to that same account under the ordinary rules. Addresses need not have stored
  private keys to receive funds; none are used to forge a signature.
- At height 266, exactly 131,073 balance-map entries, 131,072 confirmed transfers
  and sender nonce 131,072; every recipient has one atom. Ledger supply, locked
  rewards and sender balance must equal the independently calculated values.
- Admit another 512 self-transfers into the durable pending queue. Admission
  must finish within 20 seconds; 100 direct account/transaction lookups within
  5 seconds. A new 513th transfer is refused without poisoning readiness.
- Close and reopen the node with that full queue. The first restart must finish
  within 120 seconds and reproduce head, supply, balances, canonical/pending
  nonce and transaction statuses. Then confirm that queue in block 267 and
  reopen again within 120 seconds, with no duplicate debit or remaining queue.
- Each measured template construction and durable block submission completes
  within 10 seconds. Entire scenario completes within 15 minutes, excluding
  compilation. A failure or deadline overrun stops the scenario.
- The canonical journal remains at or below 96MiB in this scenario. This is a
  scenario envelope, not a change to the existing 256MiB / 100,000-block node
  bounds. The process peak resident set must not exceed 1GiB, measured for the
  test executable itself, not the compiler or a parent Cargo process.
- Temporary state is removed after the run. Preserve the command/output,
  actual head and genesis, phase times, byte counts, peak memory and outcome in
  this record; test coins/keys confer no real-money or mainnet entitlement.

This sample intentionally exercises a large account map with valid bounded
network blocks. It does **not** cover maximum-length history, the full 256MiB
limit, hostile concurrent peers, slow disks, all supported hardware or public
network performance. Those claims cannot be inferred from a pass here.

## Recorded environment

- Developer Mac, model `Mac16,9`, arm64, 16 reported logical CPUs, 64GiB RAM.
- `rustc 1.95.0 (59807616e 2026-04-14)`, host `aarch64-apple-darwin`, LLVM 22.1.2.
- Debug test executable with the repository's existing build profile. Compile
  first and invoke that exact executable under the OS resource measurement tool.
- One test thread; no concurrent local build/test load intentionally scheduled.
  Remote CI is independent. Raw timings are host-specific observations, not SLAs.

## Execution results

### Attempt 1 — functional/time gates pass, memory gate unmeasured

Preregistration commit: `f491a58`. Executed source:
`8603f9f42415cda47dcb04f968f21629e037a702`, tree
`74f3de6911ea09d3fdf5ad0663557ba5e5200fd5`. The debug test executable SHA-256 was
`c276fd48befcc5ccda87c7661346f6803efb9ce4e397db38acb2e127d87cd262`.
The small workflow and three direct RPC tests passed first; core/node all-target
clippy, formatting, documentation smoke and whitespace checks also passed.
An initial test-harness compile attempted an unsafe OS call; the repository
correctly refused it. No unsafe allowance was added: RSS measurement was moved
to the existing OS tool before either capacity attempt.

Command, executed from the feature worktree at approximately 06:51 UTC:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_capacity_131072_accounts_and_full_pending_queue \
  --nocapture --test-threads=1
```

The test passed, with the following unmodified result object:

```json
{"confirmedHead":"0000da6a4a5bd3939b76de746634716bc105be8aa6fb813ab4a4586ecbc17dee","elapsedMs":267497,"fundedBlocks":256,"fundedHead":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","issued":"1335000000000000","lookup100Micros":2318,"maxAppendMs":262,"maxTemplateMs":196,"networkId":"boole-native-testnet-1","pendingAdmissionMs":3281,"resources":{"balanceEntries":131073,"confirmedTransfers":131584,"historyBlocks":267,"historyBytes":70231145,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartConfirmedMs":53040,"restartPendingMs":53042,"transfersPerBlock":512}
```

The OS wrapper then reported:

```text
267.53 real       263.33 user         0.85 sys
time: sysctl kern.clockrate: Operation not permitted
```

The command exited 1 because the sandbox blocked the measurement utility's
clock-rate query before it emitted maximum resident memory. This is **not an
overall qualification pass**, and no RSS value is inferred from journal size or
functional success. The state was disposable and the test cleaned it normally.
Under the bounded infrastructure-retry policy, retry 1 keeps the same executable,
scenario and thresholds, changing only the OS-measurement permission context.

### Attempt 2 / infrastructure retry 1 — PASS

The same executable hash was checked again and the exact command was rerun with
the OS-measurement permission available. No product code, scenario, thresholds
or compiler changed. All functional assertions passed. Both runs reached the
same funded/final heads, issuance and journal byte count.

| Criterion | Fixed bound | Observed |
|---|---:|---:|
| Final canonical state | 267 blocks / 131,073 balance entries | Exact match |
| Entire scenario | 900 seconds | 267.682 seconds |
| Maximum template / durable append | 10 seconds each | 208ms / 260ms |
| 512 durable pending admissions | 20 seconds | 3.330 seconds |
| 100 direct lookups, including resource counts | 5 seconds | 3.479ms |
| Restart with queue / after confirmation | 120 seconds each | 53.103s / 53.097s |
| Canonical history file | 96MiB | 70,231,145 bytes (~67MiB) |
| Process maximum resident set | 1GiB | 343,015,424 bytes (327.125MiB) |

Unmodified result object:

```json
{"confirmedHead":"0000da6a4a5bd3939b76de746634716bc105be8aa6fb813ab4a4586ecbc17dee","elapsedMs":267682,"fundedBlocks":256,"fundedHead":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","issued":"1335000000000000","lookup100Micros":3479,"maxAppendMs":260,"maxTemplateMs":208,"networkId":"boole-native-testnet-1","pendingAdmissionMs":3330,"resources":{"balanceEntries":131073,"confirmedTransfers":131584,"historyBlocks":267,"historyBytes":70231145,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartConfirmedMs":53097,"restartPendingMs":53103,"transfersPerBlock":512}
```

OS measurement and command exit:

```text
267.71 real       263.54 user         0.89 sys
343015424 maximum resident set size
0 average shared memory size
0 average unshared data size
0 average unshared stack size
26047 page reclaims
0 page faults
0 swaps
0 block input operations
0 block output operations
0 messages sent
0 messages received
0 signals received
3279 voluntary context switches
2288 involuntary context switches
4141126996455 instructions retired
1137030278754 cycles elapsed
172802744 peak memory footprint
exit: 0
```

The RSS criterion uses **maximum resident set size**, not the smaller, differently
defined peak-memory-footprint value. macOS's installed `getrusage` manual defines
`ru_maxrss` in bytes. Zero OS block-I/O counters are not evidence of skipped
journal writes: the scenario uses the ordinary durable node and validates its
actual files through two reopens.

This is one sender and monotonically assigned recipient addresses, not 131,073
independently operated wallets or a representative random-workload benchmark.
The 53-second full replay remains a real operational cost; no maximum-history,
large-fork, concurrency/attack, other-hardware or public-network qualification is
claimed. No history or memory limit was raised to obtain this result.

### Follow-up after recent-fork implementation — PASS, new product source

The [recent-fork implementation and separate qualification](native-recent-fork-qualification-2026-09.md)
added a bounded inverse cache and removed an unnecessary full-ledger preparation
copy. Because those changes affect memory and replay, the original capacity
scenario was run again with **all original criteria unchanged**. This is a new
product verification, not another infrastructure retry of the initial source.

Executed source `4bf45f494690af41e4c8bad5db0fd5e64a3ac3bf`, tree
`5c268c4799e1aafc325631c6d514289e50f976ea`; exact debug executable SHA-256
`86a6053d275b2cf742c8f485f4f1f2e7f06441fafd78728563492c60c4b8240a` was checked
before invoking the original command. Same developer Mac, one test thread and
no other local compile/test load intentionally scheduled; approximately
07:48–07:52 UTC. No threshold, network rule or history limit changed.

Unmodified result object:

```json
{"confirmedHead":"0000da6a4a5bd3939b76de746634716bc105be8aa6fb813ab4a4586ecbc17dee","elapsedMs":265743,"fundedBlocks":256,"fundedHead":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","issued":"1335000000000000","lookup100Micros":2895,"maxAppendMs":252,"maxTemplateMs":202,"networkId":"boole-native-testnet-1","pendingAdmissionMs":3317,"resources":{"balanceEntries":131073,"confirmedTransfers":131584,"historyBlocks":267,"historyBytes":70231145,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"restartConfirmedMs":52068,"restartPendingMs":52086,"transfersPerBlock":512}
```

OS output:

```text
265.78 real       261.56 user         0.89 sys
305889280 maximum resident set size
0 average shared memory size
0 average unshared data size
0 average unshared stack size
23782 page reclaims
0 page faults
0 swaps
0 block input operations
0 block output operations
0 messages sent
0 messages received
0 signals received
3275 voluntary context switches
2234 involuntary context switches
4079241023579 instructions retired
1121652781626 cycles elapsed
178766424 peak memory footprint
exit: 0
```

The funded/final heads, genesis, supply, account/index counts and journal bytes
match the original scenario exactly. All original bounds passed: 265.743s total,
202/252ms maximum template/append, 3.317s admission, 2.895ms for 100 reads,
52.086/52.068s reopens, 70,231,145 history bytes and 305,889,280 bytes maximum RSS
(291.71875MiB). The temporary state was cleaned normally. This one workload's
lower RSS is not a general memory-reduction guarantee: the separate fork scenario
uses additional simultaneous candidate copies and measured 551.0625MiB. The
full-history/copying, maximum-capacity, concurrency and public-operation limits
described above still apply.
