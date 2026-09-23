# Native peer state-lock wait — 2026-09-23

Status: **focused correction and regressions PASS; large requalification pending.**
This is closed-local development, not public transport, R1 completion or R2/R3
launch approval. Signature, PoW, fork-choice, admission and publication rules are
unchanged. The separate held proposal to shortcut known-transfer verification
is not implemented by this work.

## Reason and selected scope

The [eight distinct competing-fork scenario](native-competing-forks-qualification-2026-09.md)
passed its fixed criteria, but recorded 32 transient failures and substantial
serialized state work. Review found that the ten-second peer I/O deadline and
socket shutdown did not bound a blocking `Mutex::lock` on native ledger state.
Even a peer with no validation/write in progress could therefore keep shutdown
waiting for an unrelated holder. This is a separate reproduced defect; it does
not attribute all earlier large-scenario failures to that cause.

All inbound/outbound **round** state-lock waits now use a shared private helper:
try the real mutex, check stop and the existing absolute round deadline before
and after acquiring it, and wait on the lifecycle wake condition for at most
five milliseconds before retrying. Poisoning still fails closed. No new public
interface, tunable, worker, timer extension or consensus constant is introduced.
The one-time readiness check during service startup is unchanged.

Only waiting is cancellable. Once validation or durable publication is running,
the existing mutation permit and state lock remain owned until that operation
finishes. Shutdown still crosses the unchanged mutation barrier before returning.
This does not make disk/CPU work preemptible, guarantee fair lock ordering or put
a universal wall-clock bound on stop. Normal HTTP queued/running operation
lifetimes, diagnostics admission, validation and readiness are unchanged.

## Evidence

The real mutually pinned loopback TLS test authenticates a client, holds the
actual native-state mutex, and checks whether stop returns within one second
**before** releasing that mutex. Before correction it failed in 1.14s with
`peer shutdown waited for an unrelated ledger lock holder`; cleanup released the
lock and joined both sides, so this was not a leaked/hung test. After correction
the identical test passed in 0.15s.

Further direct tests passed in 10.32s combined: the waiting outgoing round fails
at its original ten-second deadline with `lastFailureStage=local_state` while
the lock remains held, and both incoming/outgoing authenticated waits cancel on
stop. A test-fixture compile error passing `SocketAddr` instead of `&SocketAddr`
was corrected before execution; it was not a runtime result.

The queued-mutation test passed in 4.03s. It sends fully signed valid extension,
winning fork and pending-transfer data while the local ledger lock is held.
Stop returns within one second without acquiring the lock; the existing
`block_apply`, `fork_apply` and `pending_admission` diagnostics identify those
three waits. After releasing the unrelated holder, all workers are drained,
canonical state and pending remain unchanged, and independent reopen agrees.

The complete direct peer target passed 23 tests in 82.09s with one test thread.
The unchanged lifecycle target passed all five socket/wait/mutation-barrier tests
in 0.10s. All three real-router HTTP operation/diagnostic tests passed in 10.22s,
including timed-out callers retaining their actual mutation slots. The six
recent-reorg publication fault boundaries passed in 5.94s, preserving the
appropriate old/new history and pending nonce dependencies on independent reopen.
Documentation smoke, formatting and diff checks passed.

The wait tests were strengthened to require zero failed rounds before requesting
stop, so an already-disconnected peer cannot produce a false pass. Those three
wait/deadline tests passed again in 10.31s; the three queued mutation cases in
4.01s. The unchanged small distinct-fork scenario passed in 15.83s: 2.864s network
phase, winner amount 7, all eight peers stable, nine transient failed rounds,
92-microsecond stop, and 525ms independent replay. Final state remained 28 blocks,
1,025 balances, 1,280 confirmed transfers, no pending and 700,341 history bytes.
This is a regression check, not proof of a speedup or large-state qualification.
Node all-target clippy passed with warnings denied in 36.82s. The large test
executable before measurement has SHA-256
`bfe1159ab182b3bc4bceceba2b03afa1fade1c42f0bdedcadfea7930d01696b5`,
using Rust `1.95.0 (59807616e 2026-04-14)` and cargo
`1.95.0 (f2d3ce0bd 2026-03-21)`.

Before a further large run, run the unchanged small competing-fork regression,
focused peer/lifecycle/HTTP/durability tests, formatting and node clippy. Then
pin the source tree and executable and repeat the existing preregistered large
scenario once with identical criteria, no concurrent build/test load, and
preserved raw results. A timeout or convergence failure is not a reason to relax
its limits or retry unchanged code. The original 75.618s/702.84375MiB result stays
in its evidence record regardless of the new outcome.
