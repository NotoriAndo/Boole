# Node-native shadow binding and containment design v1 — correction round 2

Status: **APPROVAL WITHHELD (round 2) — this document is itself unreviewed. No implementation, no
endpoint, no consensus change.**

The 2026-08-22 first correction (`docs/node-native-shadow-binding-containment-design-v1-correction.md`,
"correction r1") fixed the six defects it was asked to fix, but a second operator review found five
further contradictions that must close before any implementation gate can be trusted. Per the same
convention r1 established: r1 and the base document are preserved unchanged as historical record.
This document does not rewrite either; it states, point by point, what still conflicts and what the
corrected rule is. **Wherever this document disagrees with r1 or the base document, this document
controls.** `boole-node` implementation may not begin under the base document or r1 alone, and may
not begin under this document either until it has itself been reviewed and approved.

## D1. `DeterministicReject` was used for two different things, contradicting the authority spec's evidence rule

The authority spec states plainly: "Success or deterministic rejection produces
`boole.native-shadow.evidence.v1`" (`docs/native-submission-shadow-verification-v1.md` section 6)
and lists its seven-stage decision path, where only stage 5/6 (executing the actual pinned checker
and converting its result to a verdict) precedes evidence persistence and challenge consumption
(stage 7) (same document, section 5). Correction r1's table nonetheless labeled schema errors,
unknown identity, settled registry drift, expired challenges, replayed challenges, cross-task
binding violations and forged fields as `DeterministicReject` while also marking their
"Persists evidence?" column `No` (r1 section 7, rows 1, 2, 3, 5, 7, 9, 10) — directly contradicting
the authority spec's own rule that a deterministic rejection produces evidence. r1's row 6,
`challenge_in_flight`, made the same category error in a different way: it labeled a transient
scheduling collision `DeterministicReject` when a `challenge_in_flight` outcome is not a stable fact
about the submission at all — resubmitting once the colliding attempt resolves can succeed.

**Corrected rule:** a new outcome class, `PrecheckReject { reason }`, is introduced for this route.
Every rejection reached during decision-path stages 1–4 (JSON decode/size, identity resolution,
challenge freshness/in-flight check, and family-specific intake) that never reaches stage 5 (the
actual checker) is `PrecheckReject` — consumes nothing, persists nothing. `DeterministicReject` is
narrowed to exactly what the authority spec's stage 5/6 describes: the actual pinned checker's own
semantic judgment of the submitted code, converted to a node-owned verdict. Under this rule every
`DeterministicReject` unconditionally persists evidence and consumes the challenge — the authority
spec's rule is no longer contradicted by any row. `PrecheckReject` is a route-local decision-path
outcome, not a change to the shared three-state vocabulary
(`Accepted`/`DeterministicReject`/`RetryableUnavailable`) used elsewhere by
`boole_lean_runner::LeanVerdict` or `boole_node::block_verifier::ShareEvidenceVerdict`; those are
unaffected. How `PrecheckReject` surfaces at a future wire boundary is an implementation detail out
of this design's scope (the base document's non-goals already exclude any endpoint).

`challenge_in_flight` is reclassified separately: it becomes `RetryableUnavailable`, not
`PrecheckReject`. Unlike an expired or already-consumed challenge, which will keep failing for that
exact input no matter how many times it is retried, an in-flight collision is a transient race —
retrying after the other attempt resolves can succeed, which is exactly what
`RetryableUnavailable` means elsewhere in this design.

`idempotent_redelivery` (r1 row 8) is not itself a new adjudication and gets neither label: it
returns whatever was already durably recorded — an `Accepted` or `DeterministicReject` verdict and
its evidence, verbatim, unchanged. This was already r1's intent; this document only makes explicit
that it does not need a `PrecheckReject`/`DeterministicReject` label of its own.

The base document's own section 5.3 text ("A mismatch at this point is `DeterministicReject`
(drift)...") predates this correction and is superseded for implementation purposes: settled
registry drift is a stage-2/5.3 precheck outcome and is `PrecheckReject` under the rule above, not
`DeterministicReject`.

## D2. The challenge-state key is missing registry and identity binding

r1's `InFlight` reservation key and idempotent-resubmission key used only `(challengeSha256, epoch)`
or `(templateId, challengeSha256, epoch, rawAnswer bytes)` (r1 sections C3.2, C3.4, C3.5). This is
too narrow: nothing stops a `challengeSha256` value from being reused across a differently-pinned
`familyVersion`/`templateId` row, or across a registry update that repins the checker/policy/anchor
digests for the same `familyVersion`/`templateId` without changing `challengeSha256` — either case
lets state meant for one problem or one registry version bleed into another.

**Corrected rule:** every challenge-state key (the `InFlight` reservation key, the durable evidence
key, and the idempotent-resubmission key) is the five-tuple
`(registryDigest, familyVersion, templateId, challengeSha256, epoch)`. `registryDigest` is not a new
digest: it is the same registry-content digest section 5.3 (base document) already recomputes
immediately before every execution for the drift check — reused here as a key component, not
computed twice. Binding `registryDigest` into the key means a registry update that repins any
digest for a `familyVersion`/`templateId` row necessarily produces a distinct key; a challenge left
`Active`/`InFlight` under the prior registry version cannot be consumed, redelivered against, or
confused with anything under the new one. This closes the key-composition gap immediately for the
qualification path in scope here; it does not itself design real (non-qualification) challenge
issuance or a registry-version migration procedure, which remain out of scope per C6 (r1) and D3
below.

## D3. The qualification path itself has no concrete storage design

Base section 4.1 and r1's C6 both note every tracked registry entry is `nonIssuable: true` —
qualification only — and r1's C6 correctly scoped real issuance/durable-store/registry-version
design as separate future work. But the qualification path is not exempt from needing its own
concrete answer today: `boole-node` cannot implement `InFlight`, crash recovery (r1 C3.3) or the
five-tuple key (D2) against an unspecified store. Three things must be pinned now, for the
qualification path only:

* **Challenge source.** For `nonIssuable: true` registry rows there is no separate issuance service:
  the registry row's own pinned `challengeSha256`/`epoch` is the pre-issued challenge. At node
  startup, for every registry template row with no existing state under its D2 key, the node
  registers a fresh `Active(fresh)` record for that key. This is specific to the qualification
  posture and is not a general answer for real issuance (still C6, still out of scope).
* **Storage method.** Reuse the durability primitives `boole-node` already has, rather than adding a
  new dependency: `crates/boole-node/src/durability.rs`'s `append_ndjson_line_durable` (durable,
  fsync'd, crash-safe single-line append) and the same recover-by-replay shape already used by
  `crates/boole-node/src/bounty_event_store.rs`'s `FileBountyEventLedger` (`append`/`recover`). A
  new file-backed NDJSON journal records every state transition as one durably-appended event
  (`{key, from_state, to_state, evidence?, leafCgroup?, timestamp}`); the current state per key is
  the fold of that journal, not a separately maintained table. `boole-node` is a single process and
  the sole writer to this journal; the `Active -> InFlight` compare-and-set and the paired
  `InFlight -> Consumed` + evidence write (r1 C3.5) are each done by holding one in-process lock for
  the full span from decision to durable append, then releasing it — this is what "atomic
  compare-and-set against the durable store" (r1 C3.5) concretely means for this single-process
  deployment target; it was never a distributed-consensus requirement, and a lock whose critical
  section includes the durable write satisfies it exactly.
* **Restart recovery order.** On startup, before the route accepts any submission: (1) replay the
  full journal in order to rebuild current state per key, mirroring
  `FileBountyEventLedger::recover`; (2) apply r1's C3.3 rule — any key whose last recorded event is
  `InFlight` with no later `Consumed` event reverts to `Active(fresh)` or `Expired`, per its
  freshness window; (3) for every registry row with no key present after replay, register a fresh
  `Active(fresh)` record (first-run bootstrap); (4) cross-reference every `InFlight` or
  just-reverted record's recorded `leafCgroup` field against cgroups actually present on disk, and
  force-clean any that do not correspond to a currently-legitimate reservation (closes r1 C4's
  orphan-cgroup sweep against real data instead of leaving it unspecified); (5) only after (1)–(4)
  complete does the node begin serving the route — extending, not replacing, the existing
  fail-closed startup posture (base document section 4.2's delegation probe; r1 C5's containment
  probes).

## D4. The cgroup containment contract is still not mechanically complete

Seven further fixes to base section 4.2 / r1 C4, needed before this contract can be implemented as
written:

1. **`cpu.max` bounds a rate, not a total.** It caps how fast the cgroup may consume CPU, not how
   much CPU-time it may consume overall — a submission stays under the rate cap indefinitely and
   still exhausts a wall-clock budget's worth of real CPU. The harness must additionally read
   `cpu.stat`'s cumulative `usage_usec` and enforce a total-CPU-time ceiling against it, in addition
   to (not instead of) the existing `cpu.max` rate limit and the existing per-process `RLIMIT_CPU`.
2. **Memory overrun must be confirmed and the whole tree killed.** Read `memory.events`' `oom_kill`
   counter to authoritatively confirm a memory-driven kill occurred (mirroring r1 C4's `pids.events`
   fix), and terminate via `cgroup.kill` so every process in the tree is stopped — not only whichever
   single process the kernel's OOM killer happened to select first.
3. **The workspace/disk ceiling needs an actual filesystem-level mechanism, not only measurement.**
   cgroup v2 has no byte-quota controller for arbitrary directories; polling directory size (e.g.
   `du`) is advisory and racy as the *sole* enforcement. The workspace must be backed by a
   size-bounded mount created for that submission alone (a `tmpfs` mounted with an explicit `size=`
   option, or an equivalent loopback-device quota) so the ceiling is enforced by the kernel at write
   time; periodic size polling may remain as a secondary, non-authoritative check.
4. **The sandboxed child must never inherit a writable cgroup control file descriptor.** Any file
   descriptor the harness opens to write cgroup control files (`cgroup.procs` and others) must be
   opened close-on-exec (or explicitly closed post-fork, pre-exec) so the untrusted child can never
   hold a handle capable of reconfiguring or escaping its own cgroup — this is the concrete mechanism
   for r1 C4's self-escape requirement, which named the property without naming how to guarantee it.
5. **Concurrency is fixed at 1 for this design.** At most one native submission executes at a time.
   This is a scope decision, not only a safety margin: it avoids having to validate N-way concurrent
   resource accounting (aggregate host memory/CPU headroom across simultaneously-running cgroups)
   before this route's first implementation, deferring that to a later, explicitly-scoped increase.
6. **Cleanup verification applies to every outcome, not only kills.** r1 C4's `populated == 0`
   polling before cgroup removal was written for the kill path; it applies identically to a clean,
   successful, or checker-rejected run that exits on its own. The node must confirm `populated == 0`
   and complete leaf-cgroup removal before it returns *any* response — success, `DeterministicReject`,
   `PrecheckReject`, or `RetryableUnavailable` alike.
7. **The iterative-SIGKILL fallback is removed, not merely deprioritized.** Base section 4.2 reads:
   "force-terminated (using `cgroup.kill` where the kernel supports it, else an iterative SIGKILL
   sweep)." The fallback clause is struck for implementation purposes. `cgroup.freeze` (stopping every
   process in the tree, including any mid-fork, before termination) followed by `cgroup.kill` is the
   only supported termination path. A kernel that lacks `cgroup.kill` is a startup capability-probe
   failure — the route refuses to start (base section 4.2's probe, r1 C5's fail-closed extension) —
   never a silent downgrade to a manual per-PID kill loop, which cannot make the same
   no-process-escapes-uncounted guarantee a manual list-then-kill race is inherently exposed to.

## D5. GREEN has no real Linux precondition, and the resource-shortage classifier is not forgery-safe

### D5.1 A Linux environment with real delegated cgroup permission is a precondition of GREEN

Base document section 4.2 already states the Linux/macOS split (macOS has no equivalent primitive
and stays permanently qualification-only for the process-count axis; this correction does not
reopen that). What was missing is a precondition on GREEN itself: this route's integration test
suite must actually pass on a Linux host where the process has real, confirmed cgroup v2 delegation
permission — not merely run and pass trivially because the containment layer silently no-ops under
insufficient privilege. A generic hosted CI runner is not assumed to have that delegation. The test
suite must explicitly detect missing delegation and **skip with a visible, named skip reason**
(never silently pass) when it is absent; CI configuration must name which runner actually provides
delegated cgroup permission, and GREEN may only be declared from a run where that suite executed
and passed, not skipped.

### D5.2 The resource-shortage classifier must be an exact allowlist, not a text scan

Correction r1's C1 fix (resource shortage is `RetryableUnavailable`, never `DeterministicReject`)
was directionally correct but specified it in terms of scanning the checker/toolchain's own
stdout/stderr for resource-shortage-sounding text ("Cannot allocate memory", "os error 12", a
linker OOM message, "an equivalent host-resource complaint"). That is unsafe: the checker executes
**untrusted submitted code**, which can print arbitrary text, including a forged string designed to
look like a resource-shortage report, to try to manipulate its own classification away from a real
semantic rejection. This is exactly the failure mode the existing precedent already avoids —
`boole_lean_runner`'s `classify_failed_run`/`enforce_axiom_allowlist` classifies from the process's
own exit code and signal, and from a fixed allowlist of permitted axioms, never from scanning
arbitrary process output for a resembling string.

**Corrected rule:** the resource-shortage classification is driven exclusively by facts the harness
observes about the process from the outside — a `RetryableUnavailable`-triggering spawn failure the
harness's own `fork`/`clone`/`exec` call reports, a specific documented cargo/rustc/linker exit code
the project maintains as an explicit, exact allowlist (not a substring match), or a host-resource
kernel signal the harness itself observes (`memory.events`' `oom_kill` counter per D4.2, a
containment-layer kill per r1 rows 14–18). It is never driven by pattern-matching the checker's own
stdout/stderr text. A dedicated anti-forgery test is required: a submission whose own printed output
contains a forged resource-shortage-looking string, but whose process-level facts show a normal
checker-owned semantic rejection, must classify as `DeterministicReject` (`checker_rejected`) —
proving the classifier never reads child-printed text as evidence of anything.

## RED gates and STOP conditions addendum (extends r1 section 8)

An implementation must start with failing tests for at least:

1. every stage-1–4 rejection (schema, unknown identity, settled registry drift, expired, replayed,
   cross-task binding, forged field) is `PrecheckReject`, persists no evidence and consumes no
   challenge; only the actual checker's stage-5/6 outcome can be `DeterministicReject`, and every
   `DeterministicReject` persists evidence (D1);
2. `challenge_in_flight` is `RetryableUnavailable`; a resubmission after the colliding attempt
   resolves is independently adjudicated, not permanently blocked (D1);
3. two challenge-state entries that differ only by `registryDigest` (same
   `familyVersion`/`templateId`/`challengeSha256`/`epoch`) are tracked, consumed and redelivered
   independently — one can never satisfy or redeliver against the other (D2);
4. a node started against an empty durable journal auto-registers every registry template row as a
   fresh `Active(fresh)` entry (D3, bootstrap);
5. a node restart correctly replays the durable journal: an `InFlight` entry with no later
   `Consumed` event reverts to `Active`/`Expired`; a `Consumed` entry idempotently redelivers its
   stored evidence without re-invoking the checker (D3, extends r1 gates 6–7);
6. a submission accumulating total CPU time across many quick children while staying under the
   `cpu.max` rate limit is still contained, via `cpu.stat`'s cumulative counter (D4.1);
7. a memory-limited submission is confirmed via `memory.events`' `oom_kill` counter and
   `cgroup.kill` terminates every process in the tree, not only the individually OOM-killed one
   (D4.2);
8. a submission attempting to exceed its workspace quota is blocked by the backing mount's own
   size limit, independent of any periodic `du`-style measurement (D4.3);
9. the sandboxed child holds no inherited file descriptor to any cgroup control file, verified by
   inspecting its open file descriptors (D4.4);
10. a second submission arriving while one native execution is in progress is rejected/queued under
    the fixed concurrency-of-1 rule and never runs concurrently with the first (D4.5);
11. a normal, non-killed completion (`Accepted`, `DeterministicReject`, or `PrecheckReject`) still
    confirms `cgroup.events`' `populated == 0` and completes leaf-cgroup removal before the node
    responds (D4.6);
12. a kernel lacking `cgroup.kill` fails the startup capability probe and the route refuses to
    start; no code path performs an iterative per-PID SIGKILL sweep (D4.7);
13. the integration suite explicitly detects and visibly skips (never silently passes) when run
    without real delegated cgroup v2 permission; CI is configured with a runner that has that
    delegation for the run that must actually pass (D5.1);
14. a submission whose own stdout/stderr contains a forged resource-shortage-looking string, but
    which the harness observes exiting via the checker's own normal semantic-reject path, classifies
    as `DeterministicReject` (`checker_rejected`) — never `RetryableUnavailable` (D5.2).

Stop without fallback, in addition to r1's existing STOP conditions, if any of the following is
true:

* the durable journal cannot be replayed to a single, unambiguous current state per key (corrupt or
  ambiguous journal) — do not guess a state, fail closed;
* the running kernel lacks `cgroup.kill` — do not fall back to an iterative SIGKILL sweep; the
  startup probe must fail closed instead;
* any `RetryableUnavailable` classification is found to depend on scanning the checker/compiler's
  own stdout/stderr text rather than a harness-observed process-level fact — remove and reclassify
  before implementation proceeds; or
* GREEN is declared from a run where the integration suite skipped for lack of real cgroup v2
  delegation — a passing run on a permission-less host does not count.

## Status

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1: APPROVAL-WITHHELD / CORRECTION-REQUIRED (round 2)
```

The base document and correction r1 remain the historical record of the first two design passes and
are not edited by this document beyond their own status markers pointing here. This document itself
requires operator review before it, or any later revision, may be marked approved. `boole-node`
implementation remains blocked until an approved revision of this design exists.
