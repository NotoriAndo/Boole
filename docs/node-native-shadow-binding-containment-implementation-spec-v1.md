# Node-native shadow binding and containment — consolidated implementation spec v1

Status: **APPROVAL WITHHELD — CONSOLIDATED SPEC UNDER REVIEW.** No implementation, no endpoint, no
consensus change.

This document consolidates the base design
(`docs/node-native-shadow-binding-containment-design-v1.md`), correction round 1
(`docs/node-native-shadow-binding-containment-design-v1-correction.md`, "r1") and correction round 2
(`docs/node-native-shadow-binding-containment-design-v1-correction-r2.md`, "r2") into a single,
self-contained implementation reference. All three prior documents are preserved unchanged as
historical record; none of them are rewritten, and none of them are superseded as *records of what
was reviewed and when*. Only their status lines and closing sections gain a minimal forward pointer
to this document. **Wherever this document disagrees with the base document, r1 or r2, this
document controls.** A future implementer needs this document plus the authority spec
(`docs/native-submission-shadow-verification-v1.md`); reconstructing current rules by
cross-referencing four design documents is no longer necessary.

A 2026-08-22 second operator review of r2 confirmed r1's original six defects (C1-C6) closed
correctly, but found five further gaps that block implementation approval:

* **E1** — the `nonIssuable` (permanently non-reissuable) qualification fixture was, under r2's D3,
  auto-registered as an *active* challenge on every node startup, directly contradicting
  "permanently non-reissuable." Separately, the base document's `Active(fresh)` state definition
  depends on a "freshness window" / TTL concept that has no field anywhere in the tracked registry.
* **E2** — r2's D2 five-tuple state key does not by itself detect duplicate *submissions* against
  the same still-active challenge (it identifies the challenge, not the candidate answer), and the
  exact algorithm and recompute timing of `registryDigest` were never pinned.
* **E3** — r2's D3 five-step restart recovery order reverts `InFlight` records to `Active(fresh)`
  as one global pass, then cleans orphaned cgroups as a second global pass; a crash between the two
  passes can leave a reverted, servable challenge racing an still-alive orphaned process. No
  OS-level single-writer lock was specified, and no rule covered what happens if the durable revert
  write itself fails.
* **E4** — r2's D4 cgroup fixes named which controls matter (`pids.max`, `cpu.max` vs `cpu.stat`,
  workspace quota, `memory.events`) without pinning any concrete value, without choosing between
  tmpfs and a loopback device for the workspace quota, without pinning `memory.oom.group`, and
  without pinning the concurrent-arrival rejection behavior beyond "concurrency fixed at 1."
* **E5** — r2's D5.2 anti-forgery rule ("derive resource shortage from harness-observed facts, not
  child stdout/stderr text") is correct as a principle but was not mechanically closed: cargo/rustc
  exit code 101 is used both for genuine host resource shortage and for ordinary compile-error
  rejection, so an exit-code allowlist alone cannot resolve it, and no real Linux CI environment
  with actual cgroup v2 delegation exists yet to ever produce a trustworthy GREEN.

Sections 5 through 10 below close E1 through E5 respectively, each grounded in the actual tracked
files (`fixtures/native-shadow/registry-v1.json`,
`native/checker/rust-tuple-struct-project-v1/policy.json`,
`native/checker/rust-tuple-struct-project-v1/checker.py`,
`crates/boole-lean-runner/src/lib.rs`), not invented in the abstract. This is a **docs-only slice**:
it does not edit `policy.json`, `registry-v1.json` or any `boole-node`/`boole-lean-runner` code, and
it performs no new model measurement or census work. Where a concrete numeric default is pinned, it
is pinned as the value to implement in the later RED→GREEN slice, not as an edit landed here.

## 1. Non-goals

Carried unchanged from the base document section 1: this document does not implement `boole-node`
code, an HTTP route, or a `boole-core`/`SharePool`/block/reward/P2P/consensus change.
`boole-node` implementation may begin only after this document is itself reviewed and approved — it
is not yet approved.

## 2. Precedent reused

Carried from the base document section 2 and r2's D5.2, unchanged: `boole_lean_runner`'s
`LeanVerdict`/`ShareEvidenceVerdict` three-state vocabulary, and its
`classify_failed_run`/`enforce_axiom_allowlist` functions, are the reused precedent for how a
harness distinguishes an availability failure from a deterministic verdict. Section 10 below
restates the exact mechanism this document mirrors and explains precisely how it resolves the
cargo exit-101 ambiguity that r2 left open.

## 3. Decision path and verdict vocabulary (final)

The seven-stage decision path is unchanged from the authority spec section 5 and base section 5.
The verdict vocabulary, after r1's C1 split and r2's D1 split, is final as follows:

* **`PrecheckReject`** — route-local only; does not change `boole_lean_runner::LeanVerdict` or
  `ShareEvidenceVerdict`. Reached only in decision-path stages 1-4 (decode/size, identity
  resolution, challenge state check, intake). Never persists `boole.native-shadow.evidence.v1` and
  never consumes a challenge, because the checker was never reached. Reason codes:
  `malformed_input`, `unknown_identity`, `registry_drift`, `challenge_not_found`,
  `challenge_exhausted` (section 6 below), `challenge_stale` (reserved for the future real-issuance
  path only — see section 6), `intake_rejected`.
* **`DeterministicReject`** — reached only in decision-path stage 5/6, always persists evidence and
  always consumes the challenge, per the authority spec section 6's rule that deterministic
  rejection produces evidence. Reason codes: `checker_rejected` (the pinned checker's own semantic
  `deterministic_reject` verdict, including an ordinary nonzero compiler/test exit);
  `submission_resource_ceiling_breach` (the submission deterministically exceeded a node-configured,
  submission-independent ceiling — section 10 below); `checker_reported_reason_unconfirmed` (the
  checker self-reported `retryable_unavailable` but the harness's own independent observation found
  no corroborating resource signal — section 10 below).
* **`RetryableUnavailable`** — never persists evidence and never consumes the challenge. Reason
  codes: `native_busy` (section 8 below; replaces `challenge_in_flight`), `containment_wall_clock_kill`,
  `containment_killed` (signal death of the containment leaf — OOM kill, `cgroup.kill`, or a
  submission-independent scheduling-contingent kill), `containment_environment_unavailable` (the
  harness itself failed to construct the cgroup/workspace/lock before the child ever ran — genuinely
  external, never the submission's fault), `checker_internal_error` (the checker's own top-level
  exception handler fired — a structural signal, not a text match, so it is trusted as-is).

`idempotent_redelivery` is not a new verdict; per r2's D1, an exact redelivery of a previously
adjudicated `(state key, candidateDigest)` pair returns the prior durable verdict verbatim rather
than re-adjudicating (section 5 below defines the key precisely).

## 4. Identity, state key and idempotency key (closes E2)

Three distinct keys are in play, and conflating them is exactly what left E2 open:

1. **Operational state key** (registry-snapshot-scoped; governs `Active`/`InFlight`/`Consumed`
   transitions) — the five-tuple from r2's D2:
   ```
   (registryDigest, familyVersion, templateId, challengeSha256, epoch)
   ```
2. **Permanent exhaustion-ledger key** (registry-snapshot-independent; section 6 below) — the
   four-tuple:
   ```
   (familyVersion, templateId, challengeSha256, epoch)
   ```
3. **Idempotency / redelivery-detection key** — the operational state key plus the candidate's own
   digest, a six-tuple:
   ```
   (registryDigest, familyVersion, templateId, challengeSha256, epoch, candidateDigest)
   ```
   The five-tuple alone identifies only the *challenge*, not the submitted *answer*; two different
   candidate answers submitted against the same still-active challenge collide under the five-tuple
   alone and must not be treated as the same request. `candidateDigest` reuses, verbatim, the digest
   already defined in the authority spec section 3 — SHA-256 over the exact UTF-8 bytes of
   `rawAnswer` — no new computation. **This reuse is for redelivery/duplicate-request identification
   only and does not reintroduce r1's C2-forbidden pattern** (comparing a candidate digest against a
   pre-registered "correct answer" digest to decide correctness); correctness is decided exclusively
   by executing the checker, never by digest comparison. The underlying *state* transition
   (`Active` → `InFlight` → `Consumed`) still keys on the five-tuple alone: the challenge itself,
   once consumed by whichever candidate reaches it first, is spent at the challenge level, not the
   candidate level — single-use semantics are unchanged from the base document.

**`registryDigest` — algorithm and timing, pinned.** `registryDigest` is the SHA-256 digest of the
exact raw bytes of the tracked registry file (`fixtures/native-shadow/registry-v1.json`) as read
from disk — a whole-file content digest, with no canonicalization or reserialization step. This is
the same convention every other digest field already inside that same file uses
(`checkerArtifactHash`, `policySha256`, `anchorSha256`, `taskSha256` are all whole-file content
digests of their respective tracked files); `registryDigest` is not a new kind of digest, it is the
same convention applied to the registry file itself. Timing: it is recomputed on every single
submission at decision-path stage 2/3 (identity/challenge resolution), never cached from node
startup — this is not new machinery, it is the base document section 5.3 per-submission
drift-recompute discipline ("the node recomputes the digests of the checker artifact, policy, anchor
and toolchain identity immediately before each execution ... not only once at startup") applied to
this one additional field for consistency. A mismatch between the `registryDigest` embedded in an
existing durable state-key row and a freshly recomputed value is registry drift and is handled
exactly like every other digest-drift case already specified: `PrecheckReject(registry_drift)`, per
r2's D1 — this is not a new failure mode, it is the existing one applied uniformly.

## 5. Challenge lifecycle states (final)

* **`Active(fresh)`** — registered and not yet consumed. For the `nonIssuable` qualification path
  (section 6), "fresh" means solely "not permanently exhausted"; it carries no time component.
* **`InFlight`** — a single execution is currently running against this challenge; written
  durably before the checker is invoked and cleared on completion (section 7).
* **`Consumed`** — terminal; one execution completed and evidence was persisted for this challenge.
* **`Exhausted`** — terminal, new in this document (section 6); permanent-exhaustion-ledger-backed,
  specific to `nonIssuable` challenges, survives registry file churn.
* **`Expired`** — retained only for the future real-issuance (non-`nonIssuable`) path per the base
  document's C6/D3 out-of-scope carve-out. It is explicitly **not applicable** to the `nonIssuable`
  qualification path this document specifies — see section 6.

## 6. `nonIssuable` permanence and the freshness rule (closes E1)

Direct read of `fixtures/native-shadow/registry-v1.json`: the one tracked template carries
`"nonIssuable": true` and `"activationAllowed": false`, and the file has **no expiration timestamp,
TTL or freshness-window field of any kind** — only `epoch: 0` and the pinned digest fields. The base
document's `Active(fresh)` definition ("registered, within its freshness window, and has not yet
been consumed") assumes a freshness-window concept that does not exist in the data this document is
meant to govern. Inventing a TTL value now would not be grounding it in real data, it would be
guessing.

**Resolution.** Two changes close this:

1. **No time-based expiration for the `nonIssuable` path.** For a challenge with `nonIssuable: true`
   in the registry, "fresh" means exclusively "not yet permanently exhausted" (section 5); no wall-clock
   or TTL concept applies. Time-based expiration (the `Expired` state) is explicitly deferred to the
   separate, later, undesigned real-issuance path, consistent with the base document's own C6/D3
   carve-out — this document does not invent freshness data that the registry does not carry.
2. **A permanent exhaustion ledger, separate from the registry-snapshot-scoped operational key.**
   The five-tuple operational state key (section 4) is scoped to a specific `registryDigest`
   snapshot by design, so that unrelated registry file changes are detected as drift. But a
   `nonIssuable` fixture's exhaustion must survive registry file churn: it must never become
   re-usable merely because some unrelated byte elsewhere in the registry file changed and produced
   a new whole-file `registryDigest`. A digest-scoped key alone cannot express "permanently spent,
   independent of which registry snapshot is active" — hence the separate, `registryDigest`-independent
   four-tuple ledger key from section 4, checked **first**, on every encounter of that four-tuple
   under **any** registry snapshot, before any `Active(fresh)` bootstrap is considered.

**Startup bootstrap rule, corrected.** r2's D3 said every registry-declared key with no existing
durable state row is auto-registered `Active(fresh)` at startup — for a `nonIssuable` fixture that
has already been consumed under any prior registry snapshot, this is exactly the contradiction E1
flagged (a permanently-spent fixture coming back to life as "active" on every restart). The
corrected bootstrap rule (also restated in section 7's recovery order): for each registry-declared
key with no existing durable state row, check the four-tuple exhaustion ledger first. If it is
already recorded there, bootstrap the key directly to `Exhausted`, never `Active(fresh)`. Only a
four-tuple with no exhaustion-ledger entry bootstraps to `Active(fresh)`. On the eventual `Consumed`
transition of a `nonIssuable` challenge, the four-tuple is durably appended to the exhaustion ledger
in the same synchronous write as the `Consumed` transition (same append-only durable file, no new
storage dependency — section 7), so the two records can never drift apart.

## 7. Durable storage, single-writer lock, and crash recovery order (closes part of E3)

Storage design is unchanged from r2's D3: no new dependency. State transitions and the exhaustion
ledger both reuse `crates/boole-node/src/durability.rs`'s durable NDJSON append primitive
(`append_ndjson_line_durable`) and the `FileBountyEventLedger` append/recover shape from
`crates/boole-node/src/bounty_event_store.rs` — confirmed no sqlite/sled dependency exists anywhere
in the workspace and none is introduced here.

**What r2's D3 got wrong, precisely.** Its five-step recovery order performed two *global* passes
over every key in sequence: first revert every `InFlight`-without-`Consumed` record to
`Active(fresh)`, then, as a separate later pass, cross-reference and force-clean orphaned cgroups. A
crash between those two passes leaves a window where a record has already been reverted to
`Active(fresh)` (and is therefore servable) while its associated orphaned cgroup/workspace from
before the crash has not yet been confirmed cleaned — a new execution could then start against that
"active" challenge while the old orphaned process might still be alive, breaking the concurrency-
fixed-at-1 invariant (section 8) at exactly the moment containment matters most.

**Corrected recovery order — per-record, not two global passes, plus a fail-closed rule and an
OS-level lock:**

1. Acquire an OS-level single-writer lock on the durable ledger file (e.g. a non-blocking `flock()`
   or PID-lock file), held for the process's entire lifetime. If the lock is already held, refuse to
   start. This closes a real gap r2's D3 left open: its "atomic CAS" argument for a single
   in-process lock implicitly assumed single-process operation but named no mechanism that actually
   prevents a second node process from starting against the same ledger file.
2. Replay the durable journal to reconstruct current per-key state.
3. For each key found in `InFlight` state without a matching terminal `Consumed`/`Exhausted` record,
   processed **one key at a time**: (a) locate and force-clean that key's own cgroup leaf and
   workspace — freeze, `cgroup.kill`, verify `populated=0`, remove the leaf cgroup directory, remove
   the workspace directory — and confirm completion; only once that specific cleanup is confirmed,
   (b) durably append the revert-to-`Active(fresh)` transition for that **same** key. If the cleanup
   in (a) cannot be confirmed, or the durable write in (b) fails, that key is left out of service
   (not reverted, not served) and startup fails closed rather than proceeding on an ambiguous
   in-memory-only state. Per-record ordering, not two global passes, removes the crash window: at no
   point does any single key exist in a state where it is servable but its predecessor process might
   still be alive.
4. Bootstrap any registry-declared key with no existing durable record, applying the section 6
   exhaustion-ledger check first: `Exhausted` if the four-tuple is already recorded there, else
   `Active(fresh)`.
5. Only after steps 1-4 complete for every key does the route begin serving requests.

## 8. Concurrency: the `native_busy` unification (closes the remainder of E3/D4)

r2's D4 point 5 already fixed global concurrency at exactly 1 native execution system-wide (not
per-challenge) but left the concurrent-arrival rejection behavior ambiguous ("reject or queue").
This document pins it, and in doing so retires a redundant reason code:

**Rule.** A single node-wide, non-blocking try-lock (in-process; distinct from section 7's
cross-process OS-level ledger lock) is acquired at the start of decision-path stage 5, for every
submission, regardless of which challenge key it targets. If the try-lock is already held by another
execution — whether for the same key or a different key — the submission is rejected immediately as
`RetryableUnavailable(native_busy)`. No queueing, no waiting: the decision is synchronous, made
before any workspace or cgroup setup begins.

**Why this retires `challenge_in_flight`.** With global concurrency fixed at exactly 1, the only way
any specific challenge could ever be observed `InFlight` is if the single global execution slot is
currently occupied running that exact challenge. A per-challenge-scoped rejection reason
(`challenge_in_flight`, carried from r1 and reclassified but not removed by r2's D1) is therefore
strictly redundant with, and narrower than, the correct global check — it can never fire in a case
the global `native_busy` check would not also cover. `challenge_in_flight` is retired as a distinct
outward-facing reason code. The durable per-key `InFlight` marker itself is unaffected and still
exists in the journal purely for crash-recovery bookkeeping (section 7) — it is bookkeeping, not a
rejection reason.

## 9. Process-tree containment: contract and concrete policy values (closes E4)

The containment contract is unchanged from the base document section 4.2 and r2's D4's seven
mechanical fixes: a dedicated Linux cgroup v2 leaf per submission wraps the checker's entire process
tree (checker.py, cargo, rustc, the linker and the compiled test binary — not just the immediate
child), assigned race-free at spawn time, with the control file descriptors close-on-exec so the
untrusted tree can never inherit a writable handle, and `cgroup.freeze` + `cgroup.kill` as the sole
termination path (no iterative SIGKILL fallback; a kernel lacking `cgroup.kill` is a startup
capability-probe failure, fail closed). macOS has no equivalent and remains permanently
qualification-only for this contract; the containment-dependent path is Linux-only.

**Concrete values, pinned.** `native/checker/rust-tuple-struct-project-v1/policy.json` already
tracks real numeric ceilings — `wallSeconds: 60`, `taskTotalWallSeconds: 90`, `cpuSeconds: 120`,
`memoryBytes: 2147483648` (2 GiB), `outputBytes: 1048576`, `fileBytes: 67108864`,
`openFiles: 128` — applied today only as process-level `RLIMIT_*` values inside
`checker.py`'s `_set_limits`, which its own comment states "requires a dedicated cgroup or PID
namespace and is outside this non-activatable qualification release" for process-count containment.
This document pins the cgroup-level values that close that stated gap, as new fields the later
RED→GREEN implementation slice adds to that same tracked file (not edited in this docs-only round):

* **`pidsMax: 128`** — mirrors the existing `openFiles: 128` value for consistency. `cargo`'s
  `testArgs` already force `-j 1` (single-job build), and the submission surface is denied
  `std::thread`/`std::process`/`unsafe`/macro-invocation syntax, so the submitter cannot itself
  amplify process/thread count; the only source of multiplicity is rustc's own internal codegen
  thread pool and the linker's own threading, neither attacker-controlled. 128 is generous headroom
  over that legitimate worst case while still meaningfully bounding a hypothetical fork-bomb-style
  escape. Flagged explicitly as an initial conservative default, subject to empirical tuning once
  the Linux CI runner named in section 10 exists — not asserted as definitively optimal.
* **Workspace quota: tmpfs, size `536870912` (512 MiB), inode ceiling `8192`.** cgroup v2 has no
  byte-quota controller of its own, so the workspace ceiling must come from the filesystem: this
  document commits definitively to a size- and inode-bounded **tmpfs** mount over a loopback device.
  Rationale: a loopback device requires its own formatting and mount/unmount teardown on every run,
  which adds failure modes to exactly the crash-recovery path section 7 just tightened (a loopback
  mount left attached after a crash is one more thing recovery must detect and clean); tmpfs is
  kernel-native, needs no formatting, and its lifetime is tied directly to the mount itself being
  torn down — fewer moving parts to get right during recovery. 512 MiB / 8192 inodes comfortably
  covers a debug-profile build of the tracked single-file, dependency-free crate (source, `target/`,
  a fresh empty `CARGO_HOME`) while still bounding disk-fill abuse from pathological
  monomorphization/codegen bloat, which remains possible in safe Rust without `unsafe` or `std::fs`.
  Also an initial default, not asserted as definitively optimal.
* **`cpu.max`: left unthrottled** (`max max`, no rate quota). Rationale: the submission surface
  already has both a wall-clock deadline (below) and a cumulative CPU-time ceiling (below); throttling
  the *rate* in addition would only slow down legitimate work without adding a security property
  neither ceiling already provides.
* **Cumulative CPU-time ceiling: `120` seconds**, monitored via `cpu.stat`'s cumulative `usage_usec`
  field against the same `cpuSeconds: 120` value the process-level `RLIMIT_CPU` already enforces.
  This closes the exact gap r2's D4 named: `cpu.max` alone is a rate limit, not a total-time ceiling,
  and `RLIMIT_CPU` only counts one process's own CPU time, not the whole tree's (cargo + rustc +
  linker + test binary combined) cumulative usage. The cgroup-level check is a tree-wide backstop,
  mirroring the existing `boole-lean-runner` pattern of `RLIMIT_CPU` as a secondary defense-in-depth
  layered behind a primary wall-clock timeout.
* **`memory.max: 2147483648`** (2 GiB), mirroring `memoryBytes`, applied tree-wide (closing the same
  per-process-vs-whole-tree gap for memory that `RLIMIT_AS` alone leaves open).
* **`memory.oom.group: 1`**, pinned. This makes the kernel atomically OOM-kill the *entire* cgroup as
  one unit, removing the userspace-detection race that watching `oom_kill` after the fact and then
  separately firing `cgroup.kill` would otherwise leave open.
* **`memory.events` monitoring extended beyond `oom_kill`** to also read the `max` and `oom` counters
  on every outcome (not only on a kill), per the explicit request this round: `oom_kill` alone tells
  the harness a kill happened, but `max`/`oom` additionally distinguish "the hard ceiling was hit"
  from "the kernel's OOM killer engaged," which matters for section 10's classification rule.
* **Concurrency: exactly 1, immediate `RetryableUnavailable(native_busy)`, no queueing** — section 8.

## 10. Resource-shortage classification (closes E5)

**The real, already-shipped mechanism this section reconciles.**
`native/checker/rust-tuple-struct-project-v1/checker.py`'s `_infrastructure_failure_reason` function
(lines 534-567) classifies a completed child process's `resource_process_limit` /
`resource_memory_limit` outcome by regex/substring-scanning the captured stdout/stderr bytes — for
example matching a line that starts with `error:` and ends with `cannot allocate memory (os error
12)`. This is a real, already-tracked instance of exactly the text-scanning anti-pattern r2's D5.2
warns against; it is not hypothetical. `checker.py`'s own `forbiddenPatterns` denylist blocks any
macro-invocation syntax (`\b[A-Za-z_][A-Za-z0-9_]*\s*!\s*[({\[]`), which forecloses the most direct
forgery vector (e.g. `compile_error!("...")` injecting literal diagnostic text). But the underlying
design flaw is broader than forgeability, and does not require forged text to matter: `_set_limits`
applies `RLIMIT_AS`/`RLIMIT_CPU`/`RLIMIT_FSIZE` as **fixed, node-configured, submission-independent**
ceilings before every run. A submission that deterministically exceeds one of them (for example,
`Vec::with_capacity(usize::MAX)`, legal in safe Rust without `unsafe` or `std::fs`) produces a *real*
allocator-abort message, not a forged one — but the resulting classification is still wrong, because
resubmitting the identical bytes against the identical fixed ceiling reproduces the identical failure
every time, on any host, regardless of load. That is the definition of deterministic, not retryable.

**The classification principle.** The retryable/deterministic axis turns on whether the observed
ceiling is *intrinsic* to the submission's own resource demand or *extrinsic*/host-load-contingent:

* **Intrinsic** (CPU-seconds actually consumed, memory bytes actually requested/resident, output or
  file bytes actually produced) — all host-load-independent measurements of the submission's own
  demand against a fixed, node-configured ceiling. Breaching one is always `DeterministicReject`
  (`submission_resource_ceiling_breach`), because the same submission against the same fixed ceiling
  reproduces the same outcome forever.
* **Extrinsic** (wall-clock elapsed time, which is sensitive to scheduling contention independent of
  the submission's own CPU consumption; the harness's own containment-envelope setup — cgroup
  creation, tmpfs mount, lock acquisition — failing before the child ever ran; the global execution
  slot being occupied) — genuinely host-load- or timing-contingent. These remain
  `RetryableUnavailable` (`containment_wall_clock_kill`, `containment_environment_unavailable`,
  `native_busy`).

This is exactly what `boole_lean_runner::classify_failed_run` already implements and is the precedent
this document mirrors, not a new policy: its two gates are (1) did the harness's own enforced
wall-clock deadline fire, and (2) did the process die by signal (a negative/sentinel exit status, as
recorded by the harness's own `wait4`/`waitpid` — covering `SIGKILL` from `cgroup.kill`/OOM-kill,
`SIGXCPU` from `RLIMIT_CPU`, `SIGXFSZ` from `RLIMIT_FSIZE`, `SIGSEGV`/`SIGABRT` from an
`RLIMIT_AS`-triggered allocator abort). Only after both gates fail to fire does the function apply
any further, narrower text-based sub-classification (`lean_output_reports_budget_exhaustion`) — and
even then, only to pick *among* `DeterministicReject` sub-reasons; a process that reaches that point
already exited normally (non-negative code), so the text scan can never promote it back up to
`RetryableUnavailable`. This resolves the signal-death cases cleanly by the existing precedent's own
structure without needing any new rule.

**Resolving cargo/rustc's exit code 101, mechanically.** 101 is a *normal*, non-negative exit code —
it is neither a timeout nor a signal death — used by cargo/rustc both for genuine host resource
shortage during compilation and for an ordinary compile-error rejection of the submitted code, so an
exit-code allowlist cannot disambiguate it in isolation, exactly as flagged. Under the two-gate
structure above, a normal, non-negative exit code that fails both gates can **never** become
`RetryableUnavailable` — it always falls through to `DeterministicReject`. This is already
`checker.py`'s own separate, structurally correct branch (`if code != 0: raise
SubmissionRejected("compile_or_hidden_test_failed")`, `checker.py:624-625`); the only defect is that
`_infrastructure_failure_reason` is consulted and can preempt that correct branch via text-scanning
*before* the `code != 0` check ever runs (`checker.py:621-623`).

**What boole-node's own harness must do (design rule, not a `checker.py` edit — out of scope for
this docs-only round).** `checker.py`'s own process always exits `0` regardless of its internal
verdict (`main()`, `checker.py:663`), so its exit code is uninformative to an *outer* observer by
design; boole-node cannot apply the two-gate structure to checker.py's own wait-status. It must
instead apply the two gates using facts it observes independently of checker.py's self-report:

1. Did boole-node's own enforced wall-clock deadline for the whole checker.py invocation fire?
2. Did the section 9 cgroup leaf's own event counters (`memory.events`' `oom_kill`/`oom`/`max`,
   `pids.events`' `max`) show a tree-wide resource event for this specific submission's leaf?

This is precisely why section 9's whole-tree cgroup leaf is not only a containment measure but the
*only* structurally available independent signal: it captures events from cargo, rustc, the linker
and the test binary regardless of whether checker.py's own Python process ever saw or reported them.
If neither gate fires, boole-node ignores/overrides checker.py's self-reported `retryable_unavailable`
whenever it is accompanied by a "resource"-flavored reason, and reclassifies the outcome as
`DeterministicReject(checker_reported_reason_unconfirmed)` — the underlying cargo/rustc exit was, by
elimination, a normal nonzero exit, functionally identical to `checker.py`'s own already-correct
`checker_rejected` branch. This annotation is recorded only in the non-binding execution
telemetry the authority spec section 6 already allows alongside evidence ("an operational execution
identifier and resource telemetry may accompany the evidence, but they are not part of the
deterministic verdict digest") — no new evidence field is introduced. If gate (2) *does* show a
genuine tree-wide resource event, that event is intrinsic per the classification principle above
(the cgroup ceilings mirror the node's own fixed `RLIMIT_*` values) and the outcome is
`DeterministicReject(submission_resource_ceiling_breach)`, not `RetryableUnavailable` — the same
principle applies whether the ceiling was hit inside checker.py's own `RLIMIT_*` or the outer cgroup.

**Required anti-forgery test**, unchanged from r2's D5.2: a submission whose captured stdout/stderr
contains a forged resource-shortage-looking string but whose harness-observed facts (gates 1/2 above)
show a normal, unsignaled exit must still classify as `DeterministicReject`.

**Named Linux CI runner requirement.** No cgroup-v2-delegation-dependent test may declare GREEN by
skipping. Per r2's D5.1, a passing run on a permission-less host does not count, and a skip must be
visible with a named reason, never silent. This document adds the concrete, currently-missing
acceptance criterion the second review flagged: CI configuration must name, explicitly, which
job/runner provides confirmed cgroup v2 delegation (for example a labeled self-hosted runner, or a
documented GitHub-hosted runner capability) and the integration suite must assert *actual write
access* to the relevant controller files (not merely that `cgroup2` is mounted) before running any
containment-dependent test. As of this document, CI runs only generic `ubuntu-latest`, which does not
satisfy this. **This is a blocking infrastructure prerequisite for GREEN — it is not deferred, not
waived, and not satisfied by a generic runner.** Standing up that runner is implementation work for
the later RED→GREEN slice, not something this docs-only round performs.

## 11. Consolidated RED gates and STOP conditions

Supersedes the base document's 8 gates, r1's 25-row table and 14-gate addendum, and r2's 14-gate
addendum, for implementation purposes. All of the following must have a failing test before
implementation, in addition to the authority spec's own section 9 gates (which are the outer
contract and are unaffected):

1. `PrecheckReject` never persists evidence and never consumes a challenge (stages 1-4).
2. `DeterministicReject` always persists evidence and always consumes the challenge (stage 5/6 only).
3. The five-tuple state key rejects a stale row from a different `registryDigest`, `familyVersion` or
   `templateId` even when `challengeSha256`/`epoch` collide.
4. The six-tuple idempotency key returns the prior durable verdict verbatim on an exact redelivery,
   and treats two different `candidateDigest` values against the same five-tuple as distinct requests.
5. A `nonIssuable` challenge already recorded in the exhaustion ledger bootstraps to `Exhausted`,
   never `Active(fresh)`, on every startup, under every registry snapshot.
6. No time-based expiration applies to a `nonIssuable` challenge; `Expired` is unreachable on that
   path.
7. Crash-recovery per-record ordering: for a simulated crash between cgroup cleanup and the durable
   revert-to-`Active(fresh)` write, the record is never reverted before its cleanup is confirmed, and
   a failed durable write leaves the route refusing to start rather than serving an ambiguous state.
8. Two node processes cannot both start against the same ledger file (OS-level lock enforced).
9. Exactly one native execution runs system-wide; every concurrent arrival — same key or different
   key — is immediately rejected `RetryableUnavailable(native_busy)` with no queueing.
10. `challenge_in_flight` does not exist as an outward-facing reason code.
11. Every cgroup leaf enforces `pids.max`, `memory.max` + `memory.oom.group=1`, the cumulative
    `cpu.stat` ceiling, and the tmpfs workspace size/inode ceiling from section 9; breaching any of
    them classifies `DeterministicReject(submission_resource_ceiling_breach)`, never
    `RetryableUnavailable`.
12. A harness-observed wall-clock timeout or containment-environment setup failure classifies
    `RetryableUnavailable`, never `DeterministicReject`.
13. The anti-forgery test from section 10: forged resource-shortage-looking stdout/stderr text with
    a normal, unsignaled exit classifies `DeterministicReject`.
14. `checker.py`'s self-reported `retryable_unavailable` is downgraded to
    `DeterministicReject(checker_reported_reason_unconfirmed)` whenever neither harness-observed gate
    (wall-clock, signal/cgroup-event) corroborates it.
15. Cargo/rustc exit code 101 with no corroborating harness-observed resource signal classifies
    `DeterministicReject`, never `RetryableUnavailable`, regardless of stdout/stderr content.
16. `cgroup.freeze` + `cgroup.kill` is the only termination path; a kernel lacking `cgroup.kill` fails
    the startup capability probe closed.
17. `populated=0` plus directory removal is verified on every outcome (success, checker-reported
    failure, or kill), not only kills.
18. GREEN is not declared from a run where the containment-dependent suite skipped for lack of real
    cgroup v2 delegation on the CI runner.

Stop without fallback, in addition to the authority spec's existing STOP list and r1/r2's STOP
conditions, if any of the following is true:

* a `nonIssuable` challenge is ever observed `Active(fresh)` after having been recorded in the
  exhaustion ledger;
* any `RetryableUnavailable` classification is found to depend on scanning checker/compiler
  stdout/stderr text rather than a harness-observed process-level or cgroup-event fact;
* the durable ledger can be opened for writing by more than one process at once; or
* CI declares GREEN without a named, delegation-confirmed Linux runner actually executing the
  containment-dependent suite.

## 12. Relationship to the authority spec, BF receipts, and completion label

Unchanged from the base document sections 8 and 10 and the authority spec sections 6, 10 and 11:
this document does not change the authority spec's trust rule, input contract, evidence shape,
activation boundary or completion label. `boole.native-shadow.evidence.v1`'s shape is unchanged —
section 10's classification-override annotation is non-binding telemetry, not a schema change. Once
an implementation passes this document's section 11 gates together with the authority spec's own
section 9 gates, plus one real node-process raw-answer run on the named Linux runner (section 10),
the authority spec's section 4 second prerequisite closes and the combined milestone may be
evaluated against the authority spec's section 11 `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN`
label. Landing this document alone does not close that prerequisite and does not earn that label.

Landing this document, reviewed and approved, earns the same completion label the base document
originally defined:

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1-FROZEN
```

That label means the binding/replay state machine, the identity/idempotency keys, the crash-recovery
order, the concrete containment values and the resource-shortage classification rule are all
specified and approved for implementation. It does not mean any of it is implemented, does not close
the authority spec's section 4 second prerequisite, and does not change `LLM-MINEABLE-ELIGIBLE-V5`,
`mineable_now` (still 0), or any consensus, reward or P2P state.

## 13. Status

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1: APPROVAL-WITHHELD / CONSOLIDATED SPEC UNDER REVIEW
```

The base document, r1 and r2 remain the historical record of the first three review passes and are
not edited by this document beyond their own status markers pointing here. This document itself
requires operator review before it, or any later revision, may be marked
`NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1-FROZEN`. `boole-node` implementation remains
blocked until an approved revision of this design exists. If this document closes without further
contradiction, the recommended next step is to proceed directly to RED→GREEN implementation against
it, rather than iterating a further design-document round.
