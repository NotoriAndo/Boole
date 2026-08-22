# Node-native shadow binding and containment design v1

Status: **DESIGN FROZEN — no implementation, no endpoint, no consensus change in this slice.**

This document designs, but does not implement, the **second** open route prerequisite named in
`docs/native-submission-shadow-verification-v1.md` section 4: "the node's binding and replay RED
matrix must independently cover task, challenge, policy, registry and evidence misuse." That
document remains the authority for the route's input contract, decision path, evidence shape and
completion label; this document only elaborates its section 5 (required decision path), extends
its section 9 (RED gates) and resolves the process-tree containment gap the tracked checker's own
release notes already flag as unsolved.

This document intentionally combines what earlier planning treated as two separate future design
efforts — the node binding/replay RED matrix, and process-tree containment — into one slice.
Reason: whether a containment event classifies as a deterministic reject or as an availability
failure is not a detail to backfill after the RED matrix is drafted; it is one of the RED matrix's
own verdicts, and getting it wrong in either direction breaks a real invariant (see section 3).

## 1. Non-goals

This slice is design-only. It does not:

* implement `boole-node` code, a route, a state machine or a containment mechanism;
* open the `POST /native-shadow/submissions` endpoint or any other endpoint;
* modify `boole-core` admission, replay, hash or block-builder code;
* modify `SharePool`, block construction, rewards, peer state, P2P or `mineable_now`; or
* close the second route prerequisite in `docs/native-submission-shadow-verification-v1.md`
  section 4. That prerequisite closes only when an implementation passes the RED gates this
  document specifies (section 6) plus one real node-process raw-answer run, per that document's
  section 11.

Per the operator's explicit sequencing: `boole-node` code implementing this design may only begin
after this design is approved.

## 2. Precedent this design reuses, not reinvents

`boole-node` already has a landed, working three-state verdict contract for exactly this shape of
problem — "did an untrusted, sandboxed compute step accept, deterministically reject, or become
unavailable" — in the Lean bounty path:

* `boole_lean_runner::LeanVerdict` (`Accepted` / `DeterministicReject { reason }` /
  `RetryableUnavailable { reason }`), and
* `boole_node::block_verifier::ShareEvidenceVerdict`, which wraps it for the single shared
  admission/ingest/reorg verifier entry (ADR-0016 (c-2): "the SAME accept / reject / unavailable
  decision from the SAME bytes, committed budget and pinned checker").

The classification rule already proven there (ADR-0016 (a-3), `classify_failed_run` and
`enforce_axiom_allowlist` in `crates/boole-lean-runner/src/lib.rs`) is:

> A forced kill with no controlled exit (wall-clock containment kill, signal death: RLIMIT_CPU
> SIGKILL, OOM kill, sandbox kill) is `RetryableUnavailable`, never a verdict. Everything the
> checked process itself reported through a controlled exit — including a resource-exhaustion
> message the process printed on its own, such as Lean's "maximum number of heartbeats" — is
> `DeterministicReject`, because it is expected to reproduce identically from the same bytes under
> the same committed policy on every honest node.

This design adopts that exact rule and that exact vocabulary for the native checker's process
tree, rather than inventing new verdict names or a new classification principle. Section 6's RED
table is that rule applied to the native checker's specific containment surface.

The native checker's own qualification release
(`native/checker/rust-tuple-struct-project-v1/README.md`) already documents the one gap the Lean
precedent does not cover: the Lean runner sandboxes a single `lean` process, but the native
checker's toolchain (`cargo` invoking `rustc` invoking a linker) is a process **tree**, and
`RLIMIT_NPROC` cannot scope a limit to that tree — it counts every process and thread owned by the
shared host user, so a valid answer could be rejected for reasons outside the task. That release
correctly removed the check rather than keep a host-state-dependent one. Section 4 of this design
is the replacement: a containment mechanism scoped to the job's own tree, not the shared user.

## 3. Why the RED matrix and containment must be one slice

Section 5 of the authority spec already states the load-bearing principle: "An OS containment
failure or unavailable checker is not converted to ACCEPT and is not silently treated as a
semantic REJECT; it stops the shadow adjudication with an availability/error result." That sentence
is a constraint on the RED matrix, not a detail below it. Two concrete failure modes make this
non-optional:

* **Availability must not consume a challenge.** If a containment kill were classified as
  `DeterministicReject`, section 6 of the authority spec's replay rule (`Active -> Consumed` on
  first adjudication) would permanently burn the challenge for a submission that never received a
  real judgment — a legitimate answer could be locked out forever by one slow or noisy host. The
  binding/replay state machine (section 5 below) therefore must know, precisely, which outcomes are
  terminal (consume the challenge) and which are not (leave it exactly as it was) — and that split
  is exactly the accept/reject/unavailable line containment classification draws.
* **A submission must not be able to buy a `RetryableUnavailable` outcome as an oracle.** If
  "forced kill" were classified as `DeterministicReject` whenever the kill was
  content-triggered (e.g. a submission that intentionally forks many children), an adversary could
  distinguish "expensive but legitimate compile" from "resource-abuse attempt" by watching which
  bucket their own submission lands in, and could also probe whether a given input reproducibly
  trips the limit on this exact host — a side channel the deterministic-reject contract (byte-exact
  reproducibility across hosts) does not intend to carry. Section 6 therefore classifies **every**
  forced-kill containment event as `RetryableUnavailable`, uniformly, regardless of whether the
  triggering behavior originated in the submission.

These two rules could not have been safely fixed by drafting the RED matrix first and bolting
containment semantics on afterward; the containment mechanism (section 4) has to name exactly which
events are "forced kill with no controlled exit" versus "the checker's own controlled exit" before
the RED table (section 6) can be written correctly.

## 4. Process-tree isolation contract

### 4.1 Reused, unchanged primitives

This design keeps every containment primitive the Lean runner already proved
(`crates/boole-lean-runner/src/lib.rs`, ADR-0008): `RLIMIT_CPU` / `RLIMIT_FSIZE` / `RLIMIT_NOFILE`
on the spawned process, `RLIMIT_AS` on Linux, a Linux seccomp-bpf network-egress denylist, Linux
Landlock filesystem-execute restriction, and the macOS Seatbelt (`sandbox_init`) profile — plus the
existing non-negotiable execution requirements from the authority spec's section 5: fresh temporary
workspace per submission, read-only checker/anchor/toolchain inputs, sanitized environment with no
inherited secrets, disabled network access, bounded input/output and file counts, and explicit
wall-clock and filesystem limits. None of this is renegotiated here.

### 4.2 New: process-tree membership and process-count containment

The one primitive the Lean runner does not need and the native checker does: every submission's
build must run inside a containment boundary that (a) automatically captures every descendant the
toolchain forks, not just the immediate child, and (b) can enforce a hard process-count ceiling
scoped **only** to that submission's own tree — never to the shared host user, matching the
qualification release's already-stated reason for removing `RLIMIT_NPROC`.

Design:

* **Linux — a dedicated cgroup v2 leaf per submission.** Before spawning the toolchain's root
  process, create one fresh, uniquely named leaf cgroup under a node-owned parent slice; write the
  pinned `pids.max` value into it; move the spawned process into that cgroup *before* it execs
  (cgroup membership is inherited across `fork`/`clone`, so every descendant the toolchain forks is
  captured automatically — unlike a plain process group, which does not, by itself, bound process
  count). On completion (success, checker-reported failure, or any kill), every PID still listed in
  the cgroup's `cgroup.procs` is force-terminated (using `cgroup.kill` where the kernel supports it,
  else an iterative SIGKILL sweep) and the leaf cgroup is then removed. A leaf cgroup that still has
  live processes when the harness begins its next submission is a bug, not a race to be tolerated:
  the harness must join-wait the previous cleanup instead of starting the next submission's cgroup
  in parallel with it.
* **macOS — no equivalent kernel primitive exists without root.** This matches, and does not
  attempt to relitigate, the qualification release's own statement: "macOS qualification does not
  claim either address-space or process-count containment." This design keeps that boundary. macOS
  therefore remains permanently qualification-only for the process-count axis; production
  activation of this route (a separate, later approval per the authority spec's section 8) requires
  a Linux host with working cgroup v2 pids-controller delegation. This is a known, explicitly
  documented gap, not a silent one.
* **Startup precondition, not a per-submission outcome.** At node startup, if the native-shadow
  route would be enabled, the node must probe that it can create, populate and remove a throwaway
  cgroup under the configured parent slice (delegation actually writable, not just present). If that
  probe fails, the node refuses to start with the route enabled — the same class of fail-closed
  startup refusal the authority spec's section 8 already requires for a non-loopback bind. This
  keeps "containment capability is missing" out of the per-submission verdict space entirely: it is
  a configuration error, not a submission outcome.
* **No fallback to a shared-scope limit.** If cgroup v2 pids delegation cannot be obtained on a
  target deployment without root, process-count containment is not implemented via `RLIMIT_NPROC`
  or any other shared-scope mechanism as a substitute — that was already tried and explicitly
  rejected by the qualification release for depending on unrelated host activity. See the STOP
  condition in section 7.

### 4.3 Syscall/filesystem denial is not, by itself, a containment event

A seccomp `Errno` action or a Landlock/Seatbelt denial that the toolchain's own process observes
and survives (the syscall fails with an error code, the process continues and eventually exits
through its own controlled path) is **not** a distinct outcome at the containment layer. Whatever
the toolchain does after seeing that error — succeed, fail its own way, print a diagnostic — flows
through the ordinary exit-code/output classification in section 6 like any other checker-reported
result. Only an actual forced termination of the process tree (signal death, no controlled exit)
triggers the `RetryableUnavailable` rule. This mirrors the Lean runner precedent exactly (a denied
syscall is not itself special-cased; only a kill is), and keeps the containment layer from becoming
a second, inconsistent verdict source.

## 5. Task/challenge/policy/replay state machine

This elaborates the authority spec's section 5 (the seven-stage decision path) and section 6
(evidence/replay rule) into an explicit state machine. It does not change the seven stages or their
order; it names the states each stage reads or writes.

### 5.1 Challenge state

```
Issued -> Active(fresh) -> Consumed
                        \-> Expired
```

* `Active(fresh)`: `(challengeSha256, epoch)` is registered, within its freshness window, and has
  not yet been consumed.
* `Expired`: freshness window elapsed without consumption. A submission naming an expired challenge
  is rejected before checker execution (`DeterministicReject`) — this is the authority spec's
  existing RED gate 5, restated as a state transition.
* `Consumed`: **written only when stage 7 ("atomically consume the challenge and persist
  node-issued shadow evidence") actually runs** — i.e. only on `Accepted` or `DeterministicReject`
  from the actual checker (a real, terminal, reproducible verdict). Any further submission naming a
  `Consumed` challenge is rejected before execution (existing RED gate 4).
* **`RetryableUnavailable` performs no transition.** If stage 5 (checker execution) or its
  containment layer produces `RetryableUnavailable`, stage 6/7 never run: no evidence is persisted,
  and the challenge remains exactly `Active(fresh)` (or transitions to `Expired` on its own clock,
  independent of this outcome). A legitimate resubmission of the same bytes against the same
  still-fresh challenge is adjudicated again, independently, exactly as if the first attempt had
  never happened. This is the state-machine consequence of section 3's first rule.

### 5.2 Task/template binding state

A submission's `(familyVersion, templateId)` resolves against the registry snapshot **currently
loaded by the node**, not against anything the submission itself claims about the checker, policy or
toolchain (the authority spec's section 3 already forbids the submission from carrying those
fields). Two node-level states apply, both evaluated at binding, before challenge freshness is
checked:

* **Unknown**: no registry row matches `(familyVersion, templateId)` — `DeterministicReject`
  (existing RED gate 8, restated).
* **Registered**: a row matches. Binding then requires the drift check in 5.3 before proceeding.

Registry loading itself is a node-startup concern (fail closed if the registry cannot be loaded or
does not parse), not a per-submission state — an unparseable or missing registry means the node does
not serve the route at all, the same posture as the cgroup-delegation startup probe in section 4.2.

### 5.3 Policy/registry drift, checked per submission, not cached from startup

The authority spec's RED gate 6 already requires that checker/policy/anchor/registry/toolchain
digest drift be rejected before execution. This design specifies **when**: the node recomputes the
digests of the checker artifact, policy, anchor and toolchain identity immediately before each
execution (not only once at startup), and compares them to the registry row's pinned values. This
closes a time-of-check/time-of-use gap: a registry snapshot loaded at startup could otherwise go
stale if any pinned on-disk input changed after startup but before a given submission's execution.
A mismatch at this point is `DeterministicReject` (drift), independent of and prior to challenge
consumption — a torn or partially-written read of the registry file itself (a transient, not a
settled, condition) is `RetryableUnavailable` instead, since it does not reflect a real, stable
committed state any two honest nodes would agree on if they re-read at the same moment.

### 5.4 Evidence/cross-task binding state

Unchanged from the authority spec's section 6: evidence is bound to exactly one
`(templateId, challengeSha256, epoch, candidate digest)` tuple. Presenting task-A's
submission/evidence against task B, or vice versa, is rejected (existing RED gate 4). An identical
raw-answer byte string submitted afresh for a different, still-`Active` task/challenge is not
pre-judged by this history — it is adjudicated independently by that task's own checker, exactly as
the authority spec already states.

## 6. Accept / reject / retryable-unavailable RED table

Verdict vocabulary reused verbatim from `boole_lean_runner::LeanVerdict` /
`boole_node::block_verifier::ShareEvidenceVerdict` (section 2): `Accepted`,
`DeterministicReject { reason }`, `RetryableUnavailable { reason }`. "Stage" refers to the authority
spec's section 5 seven-stage decision path.

| # | Condition | Stage | Verdict | Reason class |
| - | --- | --- | --- | --- |
| 1 | Malformed / oversized / unknown-field JSON | 1 | `DeterministicReject` | schema |
| 2 | Unknown `(familyVersion, templateId)` | 2 | `DeterministicReject` | unknown_identity |
| 3 | Checker/policy/anchor/toolchain digest drift at execution time (settled, not torn) | 2/5.3 | `DeterministicReject` | registry_drift |
| 4 | Registry file read is torn/transiently inconsistent | 2/5.3 | `RetryableUnavailable` | registry_read_unstable |
| 5 | Challenge `Expired` | 3 | `DeterministicReject` | challenge_expired |
| 6 | Challenge already `Consumed` (replay) | 3 | `DeterministicReject` | challenge_replayed |
| 7 | Task-A evidence/submission presented against task B (either direction) | 3/6 | `DeterministicReject` | cross_task_binding |
| 8 | Submission carries a forbidden authoritative field (verdict/receipt/digest/witness) | 1 | `DeterministicReject` | forged_authority_field |
| 9 | Preregistered verdict-bearing raw-answer mutation (candidate digest changes) | 4 | `DeterministicReject` | intake_digest_mismatch — actual checker still adjudicates the new digest independently |
| 10 | Actual checker completes with a controlled exit and reports semantic rejection (including a resource-exhaustion message the checker itself printed) | 5 | `DeterministicReject` | checker_rejected |
| 11 | Actual checker completes with a controlled exit and accepts | 5 | `Accepted` | — |
| 12 | Workspace creation failure (disk full, tmp permission error) | 5 | `RetryableUnavailable` | workspace_unavailable |
| 13 | Process spawn failure (fork/exec error, cgroup-assignment failure) | 5 | `RetryableUnavailable` | spawn_failed |
| 14 | Wall-clock containment kill | 5 | `RetryableUnavailable` | containment_wall_clock_kill |
| 15 | CPU / memory / address-space / file-size / open-file-count limit kill (signal death, no controlled exit) | 5 | `RetryableUnavailable` | containment_killed |
| 16 | Process-count (`pids.max`) containment trip, regardless of whether the submission's own behavior triggered it | 5/4.2 | `RetryableUnavailable` | containment_pids_exceeded |
| 17 | Seccomp/Landlock/Seatbelt denies a syscall but the process survives and exits normally | 5/4.3 | *(falls through to row 10 or 11 on the process's own exit)* | not a distinct outcome |
| 18 | Seccomp/Landlock/Seatbelt-associated forced termination of the process (kill, not a survived denial) | 5/4.3 | `RetryableUnavailable` | containment_killed |
| 19 | Checker binary itself missing/unexecutable at spawn time | 5 | `RetryableUnavailable` | checker_unavailable |
| 20 | Route is OFF | — | *(no-op — byte-for-byte identical to today's node; not a route outcome at all)* | existing RED gate 9 |
| 21 | Non-loopback bind requested | startup | *(node refuses to start; not a per-submission outcome)* | existing RED gate 10 |
| 22 | cgroup v2 pids-controller delegation probe fails at startup | startup | *(node refuses to start with the route enabled; not a per-submission outcome)* | section 4.2 |

Rows 12–19 are the section 4 containment surface; rows 1–3 and 5–9 are the section 5 binding/replay
surface; row 10/11 is the actual checker's own semantic judgment, unchanged from the authority
spec. No row in this table allows a containment event to become `Accepted`, and no row allows a
containment event to be silently folded into `DeterministicReject` — matching the authority spec's
section 5 constraint verbatim.

## 7. RED gates and STOP conditions (extends authority spec section 9)

In addition to the authority spec's existing eleven RED gates, an implementation of this design
must start with failing tests for at least:

1. A submission whose checker run is killed by wall-clock or signal death never produces
   `Accepted` and never produces `DeterministicReject` — always `RetryableUnavailable` (table rows
   14/15/18).
2. A challenge that only ever produced `RetryableUnavailable` remains `Active(fresh)` and a later,
   independent resubmission of the same bytes against the same still-fresh challenge is adjudicated
   on its own merits (section 5.1) — proving availability never burns a challenge.
3. A challenge that reached `Accepted` or `DeterministicReject` cannot be reused even if the second
   attempt would have produced a different outcome (existing RED gates 4/5, cross-checked against
   the state machine in section 5.1).
4. A submission engineered to fork more descendants than the pinned `pids.max` never produces
   `Accepted` and never produces `DeterministicReject` — always `RetryableUnavailable` (table row
   16), regardless of host load.
5. A killed submission's cgroup and temporary workspace are fully torn down before the next
   submission's containment boundary is created — no live process or leaked cgroup crosses between
   two submissions' trees.
6. A settled checker/policy/anchor/toolchain digest mismatch is rejected before any checker process
   is spawned (table row 3), and is distinguished in test from a simulated torn registry read (table
   row 4).
7. Node startup with the route enabled refuses to start if the cgroup v2 pids-controller delegation
   probe fails (table row 22), exactly as it already refuses a non-loopback bind (existing RED gate
   10).
8. A syscall denied by seccomp/Landlock/Seatbelt that the checker process survives is not treated as
   a distinct outcome — the process's own subsequent controlled exit decides `Accepted` versus
   `DeterministicReject` (table row 17).

Stop without fallback if any of the following is true:

* cgroup v2 pids-controller delegation cannot be obtained on a required deployment target without
  root, and no equivalently host-scoped (not shared-user-scoped) mechanism is available — do not
  fall back to `RLIMIT_NPROC` or any other shared-scope substitute (section 4.2);
* any row in section 6's table cannot be made to reproduce identically from the same bytes under
  the same committed policy across two independent clean hosts;
* a `RetryableUnavailable` outcome is found to consume or otherwise mutate challenge state;
* the design requires `boole-core`, `SharePool`, block, reward, P2P or BF.7 modification; or
* OFF-mode behavior would differ from the current node.

## 8. Relationship to the authority spec

This document is a design elaboration of
`docs/native-submission-shadow-verification-v1.md` sections 4, 5 and 9. It does not supersede that
document's trust rule, input contract, evidence shape, activation boundary or completion label. Once
an implementation passes section 7's gates (this document) together with the authority spec's
existing section 9 gates, plus one real node-process raw-answer run, the authority spec's section 4
second prerequisite closes and the combined milestone may be evaluated against that document's
section 11 `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN` label. Landing this design document alone
does not close that prerequisite and does not earn that label.

## 9. Completion label

Landing this document, reviewed and approved, earns:

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1-FROZEN
```

That label means the binding/replay state machine and the process-tree containment contract are
specified and approved for implementation. It does not mean any of it is implemented, does not
close the authority spec's section 4 second prerequisite, and does not change
`LLM-MINEABLE-ELIGIBLE-V5`, `mineable_now` or any consensus, reward or P2P state.
