# Native recent-fork qualification — 2026-09-23

Status: **PREREGISTERED — no result yet.** The preceding
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

Pending. The small scenario runs in CI; the large one is explicitly ignored in
routine CI and is run only for this recorded qualification.
