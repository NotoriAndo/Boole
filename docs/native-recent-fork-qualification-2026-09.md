# Native recent-fork qualification — 2026-09-23

Status: **CORRECTED PASS — the baseline failure is preserved below.** The preceding
[capacity measurement](native-capacity-qualification-2026-09.md) observed about
53 seconds for full replay of 131,072 signed transfers. Replaying that same
verified prefix for a one-block local reorganization is the next bounded
resource boundary. All runs use disposable synthetic state, not operator data.

## Fixed scenario and acceptance criteria

- Same developer Mac/debug profile and compiled native genesis as the earlier
  capacity scenario; one test process/thread, actual PoW/signatures and durable
  files. No public listeners, paid API, VM, operator funds or operational keys.
- Mine ten empty blocks and 256 blocks of 512 owner-signed one-atom transfers
  to 131,072 distinct recipients. The sender is the producer and pays the
  compiled minimum fee. The common prefix ends at height 266.
- The local block 267 confirms one additional self-transfer. A different
  producer builds two valid empty blocks from height 266. Normal cumulative
  work/tie-break rules must select that candidate without changed consensus.
- Time the existing complete-candidate `NativeNode::adopt_chain` API: adoption
  must complete within **10 seconds**, including candidate validation, orphan
  recovery and durable publication. Prefix matching must compare actual block
  contents, not just block hashes that omit producer signatures.
- The result must equal the independently maintained candidate; all recipient
  balances stay at one atom, confirmed counts and owner nonce are preserved,
  and the orphan becomes pending again with the expected reserved nonce.
- Reopen the node within **120 seconds**, independently replaying the durable
  log and recovering the same head, nonce, confirmed count and orphan queue.
- Total run must finish within **900 seconds**, template/submission phases
  within **10 seconds**, journal at most **96MiB**, peak process RSS at most
  **1GiB** measured externally by `/usr/bin/time -l` for the exact test binary.
- First measure the full-prefix replay implementation with this unchanged
  scenario and 10-second criterion. Preserve any failure, then implement and
  test the correction before running the new product attempt. Do not relax
  limits after seeing a result. Stop a failing phase rather than continuing an
  unnecessary restart; record which later gates were not executed.

The new recent-history cache is an internal optimization only: no serialized
undo, remote balances or trusted checkpoint import. Every new suffix block
still passes all existing core checks. Long forks still require full replay;
bounded live peers can report recovery required. All-history cloning/writing,
the account maps and full startup replay remain linear costs. This is not a
maximum-capacity, public-performance or concurrent-adversary qualification.

## Results

The small scenario runs in CI; the large one is explicitly ignored in routine
CI and is run only for this recorded qualification.

### Baseline — whole-prefix replay exceeds the adoption bound

Source/preregistration commit `97d841a5eed1f1f20aa8bccb35fc82cec11c17a4`, tree
`3fb5c6d49e83188d2e4bdc6fe5cd962a3314849a`. The internal undo API existed, but
node adoption still performed full replay and rebuilt all transaction IDs.
The debug executable SHA-256 was
`414bb3e281020040ddfdade249bc8c14ace8b080a2d8cb6f4d0b5b1081e04b47`.
Core direct tests (8 chain, 11 accounting, 2 pending, 4 policy) passed first.
The 1,024-recipient small fork took 467ms and its restart 395ms.

Command, approximately 07:24–07:28 UTC, with no other local compile/test load:

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_capacity-6f26ba504621f258 \
  --ignored --exact native_recent_fork_131072_accounts --nocapture --test-threads=1
```

Unmodified measured adoption result:

```json
{"adoptionMs":58049,"commonHash":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","commonHeight":266,"elapsedMs":214965,"fundedBlocks":256,"head":"000073897d36ee13414d5ab4d2687814628e4dd836bfa62b8a1123ed3970e837","resources":{"balanceEntries":131074,"confirmedTransfers":131072,"historyBlocks":268,"historyBytes":69959030,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":533,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":1}}
```

The test failed at `recent fork adoption exceeded 10s: 58.049337333s` and exited
101. Accounting/candidate equality, all recipient balances, confirmed counts
and orphan requeue passed before that assertion. The restart phase was not
executed. This is a product performance failure, not an infrastructure retry.
The temporary state was cleaned by the harness. The threshold stays 10s.

OS output:

```text
215.04 real       213.17 user         0.51 sys
503529472 maximum resident set size
0 average shared memory size
0 average unshared data size
0 average unshared stack size
30951 page reclaims
0 page faults
0 swaps
0 block input operations
0 block output operations
0 messages sent
0 messages received
0 signals received
1166 voluntary context switches
1739 involuntary context switches
3361742934075 instructions retired
923905274133 cycles elapsed
451134232 peak memory footprint
```

Maximum RSS is the `maximum resident set size` field, in bytes on this macOS;
the distinct peak-footprint field is not substituted for it.

### Corrected product attempt — PASS

Executed source `4bf45f494690af41e4c8bad5db0fd5e64a3ac3bf`, tree
`5c268c4799e1aafc325631c6d514289e50f976ea`; debug executable SHA-256
`86a6053d275b2cf742c8f485f4f1f2e7f06441fafd78728563492c60c4b8240a`.
The scenario, command and acceptance thresholds above are unchanged. This is a
new product attempt after the verified implementation correction, not a retry
of unchanged failing code. Run approximately 07:43–07:47 UTC; exit 0.

Direct preparation included core replay/inverse tests, 11 durable-node tests,
8 archive tests (including 1,025-block recovery), 11 TLS peer tests, 3 RPC tests,
2 pending-resource and 2 small capacity workflows, 19 durability unit tests and
3 native publication tests. The latter exercise all six before/after-rename
failure boundaries for recovery-union, canonical-log and final-pool replacement.
Core/node all-target clippy, formatting, docs-smoke and diff checks passed.

Unmodified result objects:

```jsonl
{"adoptionMs":3251,"commonHash":"00009943584cb04f821a9cc0675de0f42b80de834a700e6d4b7ce92d10624b0a","commonHeight":266,"elapsedMs":161173,"fundedBlocks":256,"head":"000073897d36ee13414d5ab4d2687814628e4dd836bfa62b8a1123ed3970e837","resources":{"balanceEntries":131074,"confirmedTransfers":131072,"historyBlocks":268,"historyBytes":69959030,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":533,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":1}}
{"adoptionMs":3251,"elapsedMs":213111,"resources":{"balanceEntries":131074,"confirmedTransfers":131072,"historyBlocks":268,"historyBytes":69959030,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":533,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":1},"restartMs":51869}
```

OS output:

```text
213.15 real       211.25 user         0.54 sys
577830912 maximum resident set size
0 average shared memory size
0 average unshared data size
0 average unshared stack size
35469 page reclaims
0 page faults
0 swaps
0 block input operations
0 block output operations
0 messages sent
0 messages received
0 signals received
1161 voluntary context switches
1689 involuntary context switches
3313032037073 instructions retired
909865589477 cycles elapsed
443564824 peak memory footprint
```

Adoption improved from 58.049s to 3.251s (about 17.9× in this sample). The final
head, common prefix, history bytes and transaction/account counts match the
baseline. Unlike the failed attempt, the corrected run also completed independent
startup replay and recovered the pending orphan. The journal was below 96MiB,
each measured template/submission below 10s, total below 900s, and restart below
120s. Maximum RSS increased from 480.203125MiB to 551.0625MiB; both are below the
1GiB scenario bound. Faster adoption is not a memory reduction claim. Temporary
state was cleaned normally. Full replay still takes about 52s and all-history
copying/writing remains; other hardware, larger states and concurrent abuse are
not qualified by this result.
