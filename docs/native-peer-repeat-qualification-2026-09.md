# Native repeated-fork qualification — 2026-09-23

Status: **PREREGISTERED — not yet executed.** This is one synthetic closed-local
qualification of unchanged, fully verified losing-fork polls. It is separate from
the earlier [eight-peer buffer test](native-peer-resource-qualification-2026-09.md),
recent-fork adoption and full-pending capacity scenarios. No public operation,
general denial-of-service resistance or full-capacity claim follows.

## Fixed scenario and acceptance criteria

Record exact source, executable SHA-256, command, output and resource result after
execution. Preserve failures and criteria; do not reinterpret a failed run as a
pass. Use the existing developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM),
Rust 1.95 debug test executable and `/usr/bin/time -l` without concurrent local
build/test load. Compilation is excluded from the measured test process.

- Build one disposable native state from ten empty blocks and 256 blocks of 512
  real signed one-atom transfers to distinct recipients. Verify all 131,072
  recipient balances, sender nonce, issued supply, locked rewards and counts.
- From that common verified head, build an alternative 16-block suffix, each
  containing 512 valid owner-signed self-transfers. Validate it through the core.
  Keep 17 different empty blocks on the local canonical chain and assert the
  alternative loses normal cumulative-work/hash fork choice. Only the suffix
  and hash vector remain in the TLS fixture; discard its full candidate chain.
- One fixed mutually pinned numeric `127.0.0.1` peer advertises that same losing
  head in eight sequential rounds. First round serves all 16 blocks in pages of
  at most three, staying within unchanged 1MiB-message/8MiB-round limits. It must
  complete successfully within ten seconds, with the local chain unchanged.
- The next seven rounds must exchange hello/done only: zero hash, block or pending
  requests. They cannot reuse unverified work or change the local ledger. This
  proves omission of repeated download/validation, not a speedup inferred from
  a noisy timing difference. TLS authentication and readiness checks still run.
- After the first round, 100 direct readiness/resource queries finish within five
  seconds. All eight rounds plus these queries finish within 15 seconds.
- Before/after streaming journal digests and existence match. Canonical head,
  every balance, nonce, confirmed count, issuance and empty pending queue remain
  unchanged. Peer status has at least eight successful rounds and no failures
  before deliberate shutdown; worker count remains one.
- Stop drains outgoing workers within one second. Drop the service, fixture and
  original node, independently reopen within 120 seconds and repeat accounting,
  head and journal checks. This is ordinary genesis replay, not a cached audit.
- Each local template/durable append finishes within ten seconds. The entire
  scenario finishes within 900 seconds; canonical history is at most 96MiB and
  measured process peak RSS is at most 1GiB. RSS includes the TLS fixture.
- Only disposable deterministic test keys/state and loopback sockets are used.
  No actual operator key/funds, non-loopback bind/connect, VM/model/paid API,
  release, GitHub publication or activation occurs.

The small routine-CI harness uses two funded blocks but the same 16-block fork,
17-block canonical continuation and eight polls. The large test is ignored by
default and must be explicitly selected. Previously observed focused wire tests
already distinguish first validation, unchanged polls, both head changes,
restart, invalid signatures and storage loss; this measurement supplements them
with large canonical state, not a new validation bypass.

## Results

### Small harness — PASS before the large run

The two-funded-block scenario passed on its first execution: 1,025 balance
entries, 1,024 confirmed transfers, 29 canonical blocks and 565,202 history bytes.
The first round downloaded all 16 alternative blocks in six requests, took
3,166,316 microseconds and sent 4,364,479 server plaintext bytes. Each of the
next seven rounds requested no hashes/blocks/pending, sending only 250 server
bytes and 266 client bytes. Their 517–525ms times include the fixed 500ms poll
wait, not just computation. Eight rounds took 6.818s; 100 ready/resource queries
took 4,324 microseconds, stop 127 microseconds and independent restart 414ms.
Total was 19.636s; unchanged journals, head and accounting passed. This is the
small fixture, not a large-state or RSS result.

### Large run

Pending. General first-candidate cost, changing candidates, eight concurrent
successful adoptions, arbitrary traffic mixes and other hardware remain outside
any pass of this bounded scenario.
