# Node-native shadow binding and containment design v1 — correction

Status: **APPROVAL WITHHELD ON THE BASE DOCUMENT — this correction is itself unreviewed. No
implementation, no endpoint, no consensus change.**

`docs/node-native-shadow-binding-containment-design-v1.md` ("the base document") is preserved
unchanged as the historical record of the first design pass. Its 2026-08-22 operator review found
six concrete defects, listed below as C1–C6. This document does not rewrite the base document; it
states, section by section, what is wrong and what the corrected rule is instead. **Wherever this
document and the base document disagree, this document controls.** `boole-node` implementation may
not begin under the base document alone, and may not begin under this correction either until this
correction itself has been reviewed and approved — the base document's own sequencing rule ("boole-
node code implementing this design may only begin after this design is approved") is unchanged by
this correction, only re-applied to the corrected content.

## C1. Resource-failure and deterministic-reject were classified backwards (corrects base section 6, row 10)

The base document's row 10 said that if the actual checker exits in a controlled way (not killed)
and itself prints a resource-exhaustion message, that is `DeterministicReject`. That is backwards
for this checker's toolchain. Base section 2 correctly imported the Lean precedent's principle —
a forced kill is `RetryableUnavailable`, everything the process controls itself is
`DeterministicReject` — but row 10 misapplied it: it treated "the process wasn't killed" as
equivalent to "therefore this is deterministic," which only holds when the process's own report is
itself something a committed, host-independent budget produces (Lean's `-D maxHeartbeats` ceiling
is set by the harness, identically on every node, so "maximum number of heartbeats" reliably
reproduces from the same bytes everywhere). The native checker's compiler/linker toolchain has no
such self-imposed, host-independent budget of its own. Anything it reports about memory exhaustion,
fork failure, or being unable to allocate is a report about the *actual host's* free resources at
that instant — not a property of the submitted bytes, and not guaranteed to reproduce on a
different host, or even on the same host five minutes later.

**Corrected rule:** this checker route has no internal, host-independent compute budget of its own
today. Any signal indicating process or memory shortage is `RetryableUnavailable`, regardless of
whether it surfaces as a forced kill (already covered by rows 14/15/18) or as a controlled exit
whose own stdout/stderr reports memory allocation failure, fork failure, "Cannot allocate memory",
an `ENOMEM`-class OS error, "Resource temporarily unavailable", a linker out-of-memory message, or
an equivalent host-resource complaint. `DeterministicReject` under row 10 is reserved strictly for
the checker's semantic judgment of the **submitted code itself** — failed hidden tests, compile
errors caused by the submission's own code, or another rejection the checker's own logic reaches
independent of host resource state. If a later revision gives the checker its own committed,
host-independent budget (an analogue of Lean's heartbeat ceiling), only that budget's own exhaustion
message would become `DeterministicReject`; none exists in this design.

This does not change base section 3's stated principle; it corrects a case where that principle was
applied backwards.

## C2. The candidate digest was treated as an answer key (corrects base section 6, row 9)

Base row 9 attached the reason `intake_digest_mismatch` to "a preregistered verdict-bearing
raw-answer mutation," which implies the intake stage compares a submitted answer's digest against
some pre-registered *correct*-answer digest and rejects on mismatch. That is wrong, and row 9 is
removed as a distinct outcome. The candidate digest (computed over the exact raw-answer bytes, per
the authority spec's section 3) is only ever an identity tag binding one exact submission to its
own evidence. The registry does not hold, and must not hold, a "correct answer" digest to compare
against — the checker, not a digest table, decides correctness.

**Corrected rule:** whenever the raw-answer bytes differ — including a deliberately tampered or
mutated resubmission — the node computes a fresh candidate digest as bookkeeping only, and the
actual pinned checker independently runs those exact bytes and reaches its own real verdict: row 12
(reject) or row 14 (accept) in the corrected table below, nothing else. A tampered answer is
rejected because the checker judges the tampered code wrong, never because a digest differs from
anything. This restates precisely what the base document's section 5.4 already said about
cross-task binding: task-A evidence/submission replayed as task-B is rejected (a binding check, row
9 in the corrected table), but the same raw bytes submitted afresh against a still-active challenge
— for a different task, or resubmitted for a legitimate reason — are independently adjudicated by
that task's own checker run, never pre-judged by any digest history.

## C3. Challenge consumption and evidence persistence were not made explicit per outcome, and the state machine is missing in-flight reservation, concurrency, atomicity, crash recovery and idempotent-resubmission rules (corrects base sections 5.1 and 6)

### C3.1 Two columns on every outcome

Every outcome in the corrected table (section 7 below) carries two explicit columns, "Consumes
challenge?" and "Persists evidence?". The uniform rule:

| Outcome class | Consumes challenge? | Persists evidence? |
| --- | --- | --- |
| Any pre-check reject before the checker runs (schema, unknown identity, drift, expired, in-flight, already-consumed, cross-task binding, forged field) | No | No |
| The actual checker's own `Accepted` or its own `DeterministicReject` | Yes | Yes |
| Any `RetryableUnavailable` outcome, for any reason | No | No |
| Route OFF / non-loopback / startup refusal | not applicable — no per-submission state exists | not applicable |

This makes table-explicit what base section 5.1 already said in prose about `RetryableUnavailable`
never consuming a challenge; that prose was correct but left every other row's behavior implicit,
which is exactly where an implementation could drift.

### C3.2 A reserved `InFlight` state

```
Issued -> Active(fresh) -> InFlight -> Consumed
                        \-> Expired    \-> Active(fresh) or Expired   [crash recovery, C3.3]
```

* `Active(fresh) -> InFlight`: set atomically, as a durable compare-and-set (not an in-process
  lock — see C3.5), the instant stage 5 (checker execution) begins for a given
  `(challengeSha256, epoch)`. This is the reservation. A second submission naming the same
  challenge while it is `InFlight` is rejected immediately — `DeterministicReject`, reason
  `challenge_in_flight` (a new row in the corrected table) — it is not queued and does not wait.
* `InFlight -> Consumed`: set atomically, in the *same* durable write as persisting the evidence
  row, and only when the checker reaches its own `Accepted` or `DeterministicReject`. There is no
  state where the evidence row exists without the consumption marker, or vice versa.
* `InFlight -> Active(fresh)` (release, not consumption): set when the checker run ends in
  `RetryableUnavailable`, or when the node restarts and finds a challenge stuck `InFlight` with no
  matching durable evidence (C3.3). The challenge becomes available again exactly as if the attempt
  had never happened.

### C3.3 Crash recovery

At node startup, before serving the route, every challenge found `InFlight` is inspected against
durable storage: if no evidence row exists for it, the challenge reverts to `Active(fresh)` (or
`Expired`, if its freshness window has since elapsed) — the interrupted attempt is discarded, never
assumed consumed. If a matching evidence row does exist (the process crashed after the atomic write
but before some other bookkeeping caught up), the challenge is `Consumed`, not reverted. This
requires the evidence write and the `InFlight -> Consumed` transition to share one durable,
crash-safe store; an in-memory-only `InFlight` marker cannot support this recovery rule on its own.

### C3.4 Idempotent resubmission after a lost response

If the node reaches `Consumed` and persists evidence, but the caller never received the response
(for example, a network drop), a resubmission carrying the exact same
`(templateId, challengeSha256, epoch, rawAnswer bytes)` — the same candidate digest — as the
already-`Consumed`, already-evidenced attempt does **not** re-execute the checker and does **not**
report a fresh `challenge_replayed` rejection. It returns the previously-persisted evidence again,
unchanged (a new row in the corrected table, `idempotent_redelivery`). This must be distinguished
from a genuine reuse attempt: a *different* candidate digest, or a cross-task reuse, submitted
against a `Consumed` challenge remains `DeterministicReject` (`challenge_replayed` /
`cross_task_binding`) exactly as the base document specifies. Same-digest replay of an already-
settled outcome is redelivery, not a new adjudication.

### C3.5 Concurrency

Two submissions naming the same `(challengeSha256, epoch)` that race to begin stage 5 must not both
reach `InFlight`. The `Active(fresh) -> InFlight` transition must be a single atomic
compare-and-set against the durable store itself, not merely a lock held in one process's memory —
otherwise two node threads (or, worse, two racing requests hitting different in-process state) could
both believe they won the reservation. The loser always observes `InFlight` and is rejected per
C3.2; it never runs a second concurrent checker execution against the same challenge.

## C4. The cgroup containment contract has real escape and undercount gaps (corrects base section 4.2)

Six specific gaps in the base cgroup design, and the fix for each:

1. **Fork-then-assign race.** Moving a process into its cgroup only after it has already been
   created leaves a window where it (or something it immediately forks) runs unaccounted. Fix:
   use `clone3()` with the `CLONE_INTO_CGROUP` flag, which places the new process into the target
   cgroup atomically at creation, before it executes any instruction, on kernels that support it.
   Where it is unavailable, the harness must hold the child stopped (a stop-before-exec handshake)
   until cgroup assignment is confirmed, and only then let it proceed — it must never run, even
   briefly, before assignment.
2. **Self-escape / self-reconfiguration.** The sandboxed code must not be able to write to any
   `cgroup.procs`, `pids.max`, or other cgroup control file, including its own. The filesystem
   containment layer (Landlock on Linux) must explicitly deny write access to `/sys/fs/cgroup` from
   inside the sandbox — it must not be merely unmentioned in the existing policy.
3. **Undetected pids-ceiling hits.** The harness must read the cgroup's `pids.events` file's `max`
   counter after each run to authoritatively know whether the run struck the process-count ceiling,
   rather than inferring it only indirectly from a kill or an exit code.
4. **Per-process memory limits do not bound the tree.** `RLIMIT_AS` on the single spawned process
   (base section 4.1, reused unchanged from the Lean runner) bounds one process only; it does
   nothing to stop several children from each individually staying under that limit while
   collectively exceeding the host's real available memory. Fix: set `memory.max` (and
   `memory.swap.max`, to prevent evading the same ceiling via swap) on the cgroup as a whole — a
   tree-wide ceiling in addition to, not instead of, the existing per-process `RLIMIT_AS`.
5. **CPU and workspace usage are also tree-wide concerns.** The same reasoning applies to CPU
   (`cpu.max` quota on the cgroup, in addition to the existing per-process `RLIMIT_CPU`) and to
   workspace disk usage (a tree-wide usage ceiling on the submission's temporary workspace, in
   addition to the existing per-process `RLIMIT_FSIZE`, which bounds only a single file's size, not
   total usage across many files and processes).
6. **Cleanup must be verified, not fired-and-forgotten.** After `cgroup.kill`, the harness must poll
   the cgroup's `cgroup.events` file until its `populated` field reads `0` before considering the
   submission's containment boundary closed and before removing the leaf cgroup directory. A node
   that crashes or restarts with leftover cgroups from prior submissions must sweep and force-clean
   them (confirm `populated == 0`, then remove the directory) before serving any new submission —
   the same durable bookkeeping C3.3 requires for challenge-state recovery covers which leaf
   cgroups are orphaned.

## C5. No fail-open path may be reused from the Lean isolation (corrects base section 4.1's "reused, unchanged primitives" framing)

Base section 4.1 reused the Lean runner's containment primitives without qualifying one of their
properties: the existing Lean path's `IsolationMode::Log` posture allows the wrapped process to run
even when containment installation is not fully enforced. That property must not carry over. The
native route supports exactly one posture in production: every containment layer — seccomp and
Landlock on Linux, Seatbelt on macOS, and the cgroup contract in C4 — must install and be confirmed
successful **before** the candidate's own toolchain process is ever spawned. If any layer fails to
install for a given submission, the candidate code is never executed at all: the outcome is
`RetryableUnavailable`, reason `containment_install_failed` (a new row in the corrected table),
with no degraded-but-still-running fallback. If a startup-time probe for any of these
layers — the existing cgroup-delegation probe (base section 4.2) or an equivalent probe for
seccomp/Landlock/Seatbelt availability — fails, the node refuses to start with the route enabled,
exactly the existing startup-refusal category; it is never a per-request degraded mode.

## C6. Real (non-qualification) execution authority is not designed yet (extends base section 1's non-goals)

Every registry entry in the tracked fixtures today is `activationAllowed: false`,
`nonIssuable: true` — qualification evidence only, in the base document's own words (section 4.1).
This document, together with the base document, specifies binding, replay, containment and
classification for that qualification posture only. It does **not** specify: who or what issues a
real, `nonIssuable: false` challenge in production; what durable store holds real challenge and
evidence state across restarts at production scale (the same store C3.3 requires, but sized for a
real deployment rather than a test harness); or how a registry version identifier is bound into
evidence/state keys so a later registry update (a new pinned checker, policy or toolchain digest)
cannot be silently applied to, or confused with, challenges issued under a prior registry version.
These are open, undesigned questions. They are an explicit non-goal of both the base document and
this correction; a separate, later design must resolve them before any real execution authority is
granted. Base section 1's non-goal list is extended to say so explicitly.

## 7. Corrected RED table (supersedes base section 6 for implementation purposes)

Verdict vocabulary unchanged from the base document: `Accepted`, `DeterministicReject { reason }`,
`RetryableUnavailable { reason }` (reused from `boole_lean_runner::LeanVerdict` /
`boole_node::block_verifier::ShareEvidenceVerdict`, ADR-0016 (a-3)). "Base #" cites the base
document's row this row supersedes, refines or replaces; "new" marks a row this correction adds.

| # | Condition | Stage | Verdict | Reason | Consumes? | Persists? | Base # |
| - | --- | --- | --- | --- | --- | --- | --- |
| 1 | Malformed / oversized / unknown-field JSON | 1 | `DeterministicReject` | `schema` | No | No | 1 |
| 2 | Unknown `(familyVersion, templateId)` | 2 | `DeterministicReject` | `unknown_identity` | No | No | 2 |
| 3 | Checker/policy/anchor/toolchain digest drift, settled (not torn) | 2/5.3 | `DeterministicReject` | `registry_drift` | No | No | 3 |
| 4 | Registry file read is torn / transiently inconsistent | 2/5.3 | `RetryableUnavailable` | `registry_read_unstable` | No | No | 4 |
| 5 | Challenge `Expired` | 3 | `DeterministicReject` | `challenge_expired` | No | No | 5 |
| 6 | Challenge already `InFlight` (concurrent second attempt) | 3 | `DeterministicReject` | `challenge_in_flight` | No | No | new (C3.2/C3.5) |
| 7 | Challenge already `Consumed`, different candidate digest or cross-context reuse | 3 | `DeterministicReject` | `challenge_replayed` | No | No | 6 |
| 8 | Challenge already `Consumed`, resubmission's candidate digest exactly matches the stored evidence | 3 | *(returns stored evidence unchanged, not a fresh verdict)* | `idempotent_redelivery` | No | No | new (C3.4) |
| 9 | Task-A evidence/submission presented against task B (either direction) | 3/6 | `DeterministicReject` | `cross_task_binding` | No | No | 7 |
| 10 | Submission carries a forbidden authoritative field | 1 | `DeterministicReject` | `forged_authority_field` | No | No | 8 |
| — | *(base row 9 removed — see C2; a mutated/tampered answer is judged only by rows 12/14 below)* | | | | | | 9 (removed) |
| 11 | Actual checker: controlled exit, reports process/memory shortage (OOM, fork failure, allocator/linker resource error) | 5 | `RetryableUnavailable` | `host_resource_shortage_reported` | No | No | split from 10 (C1) |
| 12 | Actual checker: controlled exit, semantic rejection of the submitted code itself | 5 | `DeterministicReject` | `checker_rejected` | Yes | Yes | 10 (corrected, C1) |
| 13 | Actual checker: controlled exit, accepts | 5 | `Accepted` | — | Yes | Yes | 11 |
| 14 | Workspace creation failure | 5 | `RetryableUnavailable` | `workspace_unavailable` | No | No | 12 |
| 15 | Process spawn failure (fork/exec error, cgroup-assignment failure) | 5 | `RetryableUnavailable` | `spawn_failed` | No | No | 13 |
| 16 | Wall-clock containment kill | 5 | `RetryableUnavailable` | `containment_wall_clock_kill` | No | No | 14 |
| 17 | CPU/memory/address-space/file-size/open-file-count limit kill (signal death) | 5 | `RetryableUnavailable` | `containment_killed` | No | No | 15 |
| 18 | Process-count (`pids.max`) containment trip, confirmed via `pids.events` | 5 | `RetryableUnavailable` | `containment_pids_exceeded` | No | No | 16 (detection method corrected, C4.3) |
| 19 | Seccomp/Landlock/Seatbelt denies a syscall but the process survives and exits normally | 5 | *(falls through to row 11/12/13)* | not a distinct outcome | — | — | 17 |
| 20 | Seccomp/Landlock/Seatbelt-associated forced termination | 5 | `RetryableUnavailable` | `containment_killed` | No | No | 18 |
| 21 | Checker binary itself missing/unexecutable at spawn time | 5 | `RetryableUnavailable` | `checker_unavailable` | No | No | 19 |
| 22 | Any containment layer (seccomp/Landlock/Seatbelt/cgroup) fails to install/confirm before the candidate would run | 5 | `RetryableUnavailable` | `containment_install_failed` | No | No | new (C5) |
| 23 | Route is OFF | — | *(byte-for-byte no-op)* | — | n/a | n/a | 20 |
| 24 | Non-loopback bind requested | startup | *(node refuses to start)* | — | n/a | n/a | 21 |
| 25 | Any containment-layer install-capability probe fails at startup (cgroup delegation, and now also seccomp/Landlock/Seatbelt) | startup | *(node refuses to start with the route enabled)* | — | n/a | n/a | 22 (scope extended, C5) |

## 8. RED gates and STOP conditions addendum (extends base section 7)

An implementation must start with failing tests for at least:

1. A checker run that exits cleanly but reports OOM / fork failure / memory-allocation failure is
   `RetryableUnavailable`, never `DeterministicReject` (C1, table row 11 vs 12).
2. A tampered or mutated resubmission is rejected only through the actual checker's own run on the
   new bytes, never through a digest-mismatch short-circuit (C2, row 12/13, not the removed row 9).
3. The same raw-answer bytes submitted afresh against a different, still-active challenge are
   independently adjudicated by that challenge's own checker run, unaffected by any prior history of
   those bytes (C2, cross-checked against row 9's cross-task rule).
4. Every row's `Consumes?`/`Persists?` columns in section 7 are exercised and matched by test — in
   particular that a `RetryableUnavailable` outcome leaves both `false` (C3.1).
5. A challenge reserved `InFlight` rejects a concurrent second submission naming the same challenge
   without running a second checker execution (C3.2/C3.5, row 6).
6. A node restarted mid-execution reverts any `InFlight` challenge with no persisted evidence back
   to `Active`/`Expired`, never `Consumed` (C3.3).
7. A same-candidate-digest resubmission against an already-`Consumed` challenge returns the stored
   evidence rather than re-executing the checker or reporting a fresh reject; a different-digest or
   cross-task resubmission against the same `Consumed` challenge still rejects (C3.4, row 7 vs 8).
8. A submission engineered to fork children that collectively exceed a tree-wide memory ceiling is
   contained by the cgroup's `memory.max`, not only by per-process `RLIMIT_AS`, and classifies as
   `RetryableUnavailable` (C4.4, row 17/18).
9. cgroup assignment via `clone3`/`CLONE_INTO_CGROUP` (or an equivalent stop-before-exec handshake)
   leaves no window where a spawned process runs unaccounted (C4.1).
10. The sandboxed process cannot write to any cgroup control file, including its own (C4.2).
11. `pids.events`' `max` counter, not exit-code inference alone, is read to confirm an actual
    pids-ceiling hit (C4.3, row 18).
12. After a kill, `cgroup.events`' `populated` field reads `0` before the leaf cgroup is removed; a
    node restart sweeps and force-cleans any cgroup orphaned by a prior crash (C4.6).
13. If any containment layer fails to install or confirm for a submission, the candidate code is
    never executed — `RetryableUnavailable`, no degraded fallback (C5, row 22).
14. Node startup refuses to enable the route if any containment-layer install-capability probe
    fails, not only the existing cgroup-delegation probe (C5, row 25).

Stop without fallback, in addition to the base document's existing STOP conditions, if any of the
following is true:

* a durable, crash-safe store for challenge/evidence state (required by C3.3) cannot be provided —
  the `InFlight` state machine cannot be safely implemented without it;
* `clone3`/`CLONE_INTO_CGROUP` is unavailable on a required deployment kernel and no equivalent
  race-free cgroup-assignment mechanism exists — do not fall back to fork-then-assign (C4.1);
* any row in section 7's table is found to consume a challenge or persist evidence outside the
  C3.1 rule; or
* the design is used to justify implementing real (non-qualification) execution authority (C6) —
  that remains a separate, later, undesigned decision.

## 9. Status

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1: APPROVAL-WITHHELD / CORRECTION-REQUIRED
```

The base document remains the historical record of the first design pass and is not edited by this
correction beyond its own status marker pointing here. This correction document itself requires
operator review before it, or any later revision, may be marked approved. `boole-node`
implementation remains blocked until an approved revision of this design exists.
