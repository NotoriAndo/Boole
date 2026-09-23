# Native state capacity qualification — 2026-09-23

Status: **PREREGISTERED — not yet executed.** This is a disposable closed-local
engineering scenario, not a public benchmark or an operational wallet run.
The existing [native contract](native-transfer-ledger-contract.md) and monetary,
signature, replay, file-ownership and publication rules remain unchanged.

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

No capacity attempt has run yet. Implementation and the small direct-consumer
tests are prepared first. This status must be replaced with the actual outcome,
while the fixed criteria above and every attempted result remain preserved.
