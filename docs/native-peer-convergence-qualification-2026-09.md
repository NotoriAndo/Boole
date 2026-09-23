# Native concurrent-candidate convergence — 2026-09-23

Status: **PREREGISTERED — not yet executed.** This qualification covers eight
fixed authenticated peers concurrently providing the same valid winning fork.
It is separate from incomplete-fork buffer rejection and repeated losing-fork
polls. It does not qualify eight distinct successful reorgs, continuously
changing adversarial candidates, arbitrary inbound/RPC mixes or public operation.

## Fixed scenario and acceptance criteria

Record source/tree/executable SHA-256, exact command, raw output and resources.
Preserve every failed attempt and these bounds. Use the existing developer Mac
(`Mac16,9`, arm64, 16 logical CPUs, 64GiB RAM), Rust 1.95 debug test executable
and `/usr/bin/time -l`, with no concurrent local build/test load. Compilation is
outside the measured process. Only disposable keys/state and loopback are used;
there is no operator state/key access, non-loopback operation, VM/model/paid API,
release, GitHub publication or activation.

- Build ten empty blocks plus 256 blocks containing 512 owner-signed one-atom
  transfers to distinct recipients. Verify all 131,072 recipient balances,
  131,073 balance entries, sender nonce, issued/locked supply and confirmed count.
- From that common verified head, construct and core-validate a 16-block fork,
  each block containing 512 valid signed owner-to-self transfers with consecutive
  nonces. On the local node instead append one empty block. Assert the candidate
  wins normal cumulative-work/hash fork choice. Retain only the candidate suffix
  and hash vector in the eight TLS fixtures, not eight full chain clones.
- All eight independently pinned numeric `127.0.0.1` peers advertise this same
  candidate and send pages of at most three blocks. Pause each after the client
  requests the final page, proving all eight clients hold the preceding fifteen
  blocks simultaneously. All eight outgoing workers must be active; no journals,
  accounting or head may have changed before release.
- Release all final pages together. The node must converge to the advertised
  head once, with exactly 8,192 additional confirmed self-transfers and the
  candidate's supply/height/nonce, not eight times that accounting. Candidate
  data from a changed local snapshot must be abandoned safely; subsequent rounds
  with that same candidate must match the new head without requesting blocks.
- All eight peer statuses must reach a successful `snapshot_match` within 15
  seconds of starting the network phase. Each peer's first round sends exactly
  sixteen blocks; subsequent rounds send zero blocks. Seven initial snapshot
  races are expected, not evidence of invalid blocks or network authentication
  failure. One initial round completes, the other seven retry after head change.
- After releasing the final pages, 100 direct readiness/resource queries finish
  within ten seconds and observe only the original or the complete candidate
  head. This bound allows the shared validation/publication lock; it is not a
  reserved read-availability guarantee or an HTTP request latency measurement.
- After convergence, stop drains workers within one second. Recheck every
  recipient balance, nonce, supply, locks and confirmed count; pending is empty.
  Save both journal digests/existence. Drop service, node and fixtures, reopen
  independently within 120 seconds and require identical accounting/head/journals.
- Each local template/append finishes within ten seconds. Total scenario is at
  most 900 seconds, canonical history at most 96MiB and process peak RSS at most
  1GiB, including all eight TLS fixtures and simultaneous candidate buffers.

The small routine-CI case uses two funded blocks but the same sixteen-block
fork, eight-peer barrier and exact accounting checks. The large case is ignored
by default. Existing validation, fork choice, mutation barrier, publication and
protocol limits remain unchanged unless an actual failure motivates a separately
recorded RED/fix; no acceptance criterion may be relaxed after measurement.

## Results

Pending small-harness verification, source review and one explicit large attempt.
