# Native competing-fork qualification — 2026-09-23

Status: **PREREGISTERED — not yet measured.** This is a closed-local resource,
fork-choice and recovery scenario for eight distinct valid candidate branches,
not the prior eight copies of one candidate. It does not guarantee eight actual
adoptions: scheduling can deliver the best candidate first. It does not qualify
continually changing candidates, arbitrary RPC/inbound mixes or public operation.

## Fixed scope and acceptance

Use the same developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), Rust
1.95 debug test executable and `/usr/bin/time -l`. Compile outside measurement;
record source/tree/executable SHA-256 before and after, exact command and raw
results. No concurrent local build/test load during the large run. Use disposable
keys, state and numeric loopback only. No operator funds/keys, model/VM/paid API,
non-loopback listener, GitHub publication, release or activation.

- Build ten empty blocks plus 256 blocks with 512 valid one-atom transfers to
  distinct recipient addresses: 131,072 recipients plus one funded owner.
  Check every recipient, owner balance/nonce, issued/locked supply and count.
- Clone this verified common prefix, then construct/core-validate eight distinct
  16-block candidates. Each block has 512 consecutive owner-to-self transfers;
  candidate `i` uses amount `i+1` atoms. This yields disjoint candidate transaction
  IDs at the same sender nonces, while each candidate's final balances/supply
  agree. Use the same timestamps/reward key, assert equal cumulative work and
  distinct head hashes, and derive the winning head from the existing hash
  tiebreak rule. Every candidate must outrank the local one-empty-block extension.
- Retain only each candidate suffix and block-hash vector for its TLS fixture,
  not eight full ledger/history clones. Temporary construction clones count
  toward the measured process RSS and total time.
- Eight separately pinned TLS peers advertise their respective candidates. Page
  sizes are at most three blocks; pause each after the final-page request so all
  clients simultaneously retain fifteen blocks. Peak outgoing rounds must be
  eight; journals, canonical accounting and head remain unchanged before release.
- Release all final pages together. Within **120 seconds** of starting the
  network phase, the local canonical head must equal the best candidate and each
  fixture must observe two completed rounds whose client hello contains that
  final head. The winning peer's state is `snapshot_match`; the other seven are
  `local_chain_preferred`. Repeated completed polls after the first completed
  final-head round must request no hashes or blocks. Stale-head cancellation and
  bounded round timeouts are recorded, not confused with successful convergence.
- The final ledger has exactly 8,192 additional confirmed transfers and sender
  nonce increments, not a union of all eight candidates. Every winning transfer
  has its expected confirmed height; every losing candidate ID is unconfirmed.
  Pending is empty. All original recipient balances, owner balance, reward locks
  and supply match. Successful intermediate adoptions, if any, cannot double-count
  balances or retain conflicting orphan transactions.
- Opportunistic `try_lock` observations during convergence may see only the
  original head or one of the eight complete candidates, with matching height,
  nonce/confirmed count and empty pending state. Report observed distinct heads
  as a **lower bound**, not the exact number of adoptions. Skipped busy samples
  do not establish read latency or guarantee request availability.
- Each TLS fixture caps itself at 256 rounds and each round at 12 seconds;
  the production protocol's existing 10-second/8MiB/64-request limits remain.
  Record per-round head, completion, data requests, blocks and bytes. Client
  closure/timeout may be an expected snapshot race; the final convergence and
  subsequent no-redownload criteria still have to pass. Every transmitted page
  stays within 1MiB, and completed/partial recorded rounds stay below 8MiB total
  plaintext request/response bytes.
- Stop after convergence: within one second, workers drain to zero. Record the
  final journal digests/existence. Drop node/service/fixtures and independently
  reopen within 120 seconds, requiring the same head, all balances, confirmed
  winner IDs/loser absence, nonce, supply, locks, empty pending and journal bytes.
- Each local template/append is at most ten seconds. Entire scenario at most
  900 seconds, canonical history at most 96MiB, process peak RSS at most 1GiB.

The routine small regression uses two funded blocks and sixteen transfers per
candidate block, retaining all eight peers, sixteen-block forks and barriers.
It checks the same invariants with 256 additional confirmed transfers. The large
case is ignored by default and runs only after the small case and focused checks.

This initially tests existing runtime behavior; no artificial RED is required.
Preserve each failed result. Do not relax these criteria after observing a result
or retry an unchanged acceptance failure. Ordinary harness/infrastructure fixes
may receive at most two recorded retries under the development policy. A genuine
runtime/security failure requires diagnosis and a separately verified correction.

## Results

Not yet executed.
