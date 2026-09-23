# Native concurrent-peer resource qualification — 2026-09-23

Status: **PREREGISTERED — not yet executed.** This record qualifies one synthetic
closed-local scenario, not public connectivity, general denial-of-service
resistance or operational capacity. It follows the independent-worker change
in `bff7bd5`; all non-loopback endpoints remain refused.

## Fixed scenario and criteria

These criteria are recorded before the large run. Failures remain failures;
corrections and retries must preserve their actual outputs and identify the
changed source or infrastructure. The earlier capacity and recent-fork records
remain separate and do not establish concurrent-download capacity.

- One private disposable node directory, deterministic test-only owner/producer,
  and eight distinct ephemeral TLS transport identities. Every bind/connect is
  numeric `127.0.0.1`; no operator key/state, VM, model, paid API or public action.
- Under the unchanged compiled native genesis and validation rules, mine ten
  empty blocks and 256 blocks of 512 valid transfers each to distinct recipients.
  Validate all 131,072 one-atom balances, sender nonce, supply, locked rewards and
  131,073 canonical balance entries using the existing capacity assertions.
- Build an independently valid 33-block competing suffix at the funded head,
  each block holding 512 owner-signed self-transfers. Keep a different single
  empty block on the local canonical branch. A hash-only declaration is not the
  source of this suffix: it is produced and appended through the real core.
- Eight pinned loopback TLS fixtures advertise that same valid competing chain.
  Each serves verified common-prefix hashes and pages of three actual suffix
  blocks. After 30 blocks, each fixture pauses so all eight partial fork buffers
  coexist; no fixture can supply a final winning candidate under the existing
  8MiB round wire budget. The remaining three blocks exceed that budget.
- All eight workers must reach the partial-fork pause; active/peak outbound
  rounds must be exactly eight. Their next oversized remainder must fail the
  round, not adopt or truncate a candidate. Peer keys/worker labels remain eight.
- During the pause, 100 direct ready/resource queries complete within five
  seconds. After rejection, readiness, canonical head, supply, every balance,
  nonce, confirmed count and empty pending queue must be unchanged. A streaming
  digest confirms that both canonical and pending journal bytes/existence are
  unchanged (an unused pending journal may legitimately be absent).
- The concurrent network phase completes within 15 seconds. Stopping the peer
  service completes within one second and drains all outbound rounds. Drop all
  fixture buffers/services, then independently reopen the node within 120 seconds
  and verify the unchanged head/accounting again.
- Each local template construction/durable append remains within ten seconds.
  Entire scenario completes within 900 seconds excluding compilation; canonical
  history stays within 96MiB. Process maximum RSS must be at most 1GiB as measured
  by `/usr/bin/time -l` around the exact debug test executable, not Cargo.
- The process measurement includes the eight local TLS fixture servers and their
  shared valid suffix. It is not a isolated-node RSS measurement. No concurrent
  local build/test load is intentionally scheduled. Temporary state is removed.

The small CI harness uses two funded blocks while retaining the same eight-peer,
30+3-block wire-budget path. The explicit large test uses 256 funded blocks.
Neither test approves nonlocal P2P, changes round/consensus limits or claims the
full 256MiB/100,000-block storage envelope, all traffic mixes or all hardware.

## Environment and execution record

Developer Mac (`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), repository debug
profile and `rustc 1.95.0`. Record the exact source, executable SHA-256, command,
unmodified result JSON and OS resource output below after execution.

### Harness preparation, before the large run

The first small test stopped before opening any peer sockets: it assumed an empty
pending journal must already exist and `File::open` returned `NotFound` (test
exit101, 20.31s). The corrected comparison preserves absence as distinct from an
empty file; it does not create a file or relax any node validation. The pause is
placed when the next range request arrives, proving the client has decoded all
30 prior blocks, rather than merely observing server-side writes. The large
qualification has not run at this point and its criteria are unchanged.

The corrected small harness passed before the large run: eight simultaneous
partial forks, 8,188,312 server plaintext bytes per peer before the final page,
472ms network phase, 4,089µs for 100 ready/resource queries, 188µs shutdown,
397ms restart and 21.175s total. All eight peer rounds failed with no successful
round/adoption, and journal digests/accounting were unchanged. These figures are
the small 1,025-balance-entry fixture, not the large qualification or an RSS pass.
