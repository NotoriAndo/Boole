# Node-native shadow binding and containment — consolidated implementation spec v1

Status: **IMPLEMENTATION BASELINE APPROVED; PHASED IMPLEMENTATION IN PROGRESS.** Registry/state
durability foundations are on `main`; containment, checker execution and the HTTP route remain
open. No consensus change.

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

Sections 5 through 10 close E1 through E5 respectively, each grounded in the actual tracked files
(`fixtures/native-shadow/registry-v1.json`,
`native/checker/rust-tuple-struct-project-v1/policy.json`,
`native/checker/rust-tuple-struct-project-v1/checker.py`,
`crates/boole-lean-runner/src/lib.rs`), not invented in the abstract.

A 2026-08-22 fourth operator review of this consolidated spec confirmed E1-E5 above closed
correctly, but found four further gaps that still block implementation approval. They are closed
**in place**, in this same document, in sections 4, 6, 7, 9 and 10 below — not by a further
append-only correction file. This document's own append-only-correction convention (used for the
base document, r1 and r2, each preserved unedited once superseded) applies once a document has been
reviewed and superseded; it does not apply to this document while this document itself remains
unapproved. A draft that has never been frozen is corrected directly, not superseded again.

* **F1** — section 6's corrected bootstrap rule still checked the permanent exhaustion ledger
  *before* ever consulting the registry's own static `nonIssuable`/`activationAllowed` declaration.
  On a clean node with an empty ledger, the one currently tracked fixture — which is both
  registry-wide `activationAllowed: false` and per-template `nonIssuable: true` — would still
  bootstrap to `Active(fresh)` and become servable once, because nothing had yet been recorded in
  the ledger to check against. A permanent exhaustion ledger can only block *revival after
  consumption*; by construction it cannot block *first activation*, since the very first encounter
  of any four-tuple is never yet in it.
* **F2** — the same forced-termination event (an OOM kill under `memory.oom.group=1`) was
  classified two contradictory ways: section 3/9 named it `RetryableUnavailable(containment_killed)`,
  while section 10's intrinsic/extrinsic axis separately said any cgroup-observed resource event is
  always `DeterministicReject(submission_resource_ceiling_breach)` — for the identical event.
  Separately, `checker.py`'s own internal 60-second `wallSeconds` timeout and the future launcher's outer
  90-second `taskTotalWallSeconds` ceiling are enforced by two different actors, and section 10 as
  written risked misclassifying a legitimate checker-internal timeout as an unconfirmed/forged
  report merely because it does not itself trip the launcher's much longer outer ceiling.
* **F3** — the concrete cgroup values section 9 pinned had no accompanying safe execution *order*:
  no `memory.swap.max`, no tmpfs mount-namespace/mount-option/unmount detail, no pre-execution
  ordering sequence, no stated mechanism stopping the submission from reopening cgroupfs to loosen
  its own limits, and the macOS statement said the path was unsupported rather than stating outright
  that no child process is ever spawned there at all.
* **F4** — section 9 still described the core containment contract as "unchanged from the base
  document ... and r2's D4's seven mechanical fixes" rather than restating those fixes in full, so a
  reader still had to open superseded documents to reconstruct the actual mechanics. Two
  implementation-blocking details were also never pinned anywhere: what happens when the registry
  file changes on disk while a challenge is `InFlight`, and the exact ordering/fail-closed rule for
  the `InFlight` → evidence/`Consumed` durable-write sequence if a write partially fails.

A 2026-08-22 fifth operator review confirmed F1-F4's design direction is correct but found the F1-F4
revision itself introduced one non-implementable execution step and two internal contradictions
between prose and RED gates, plus one remaining self-sufficiency gap — closed **in place**, again in
this same document, in sections 7, 9 and 11 below:

* **G1** — section 9's pinned tmpfs mount options included `noexec`, which makes the checker's own
  normal, legitimate work fail: `checker.py` builds and then executes the compiled test binary from
  inside this exact tmpfs workspace, so `noexec` turns even a correct, acceptable submission into a
  `Permission denied` failure before any verdict is ever reached. Separately, the pre-execution
  ordering sequence was missing three specifics needed to actually implement it: nothing gave the
  post-privilege-drop unprivileged UID/GID ownership of a workspace root-created with `mode=0700`;
  nothing made the new mount namespace's mount propagation private, so a mount event could still leak
  across submissions or back to the node's own namespace; and step 5 named `checker.py`'s own
  `_set_limits` as something `boole-node` "applies" before `exec()`, which is not implementable as
  written — `_set_limits` is a function inside `checker.py`'s own code that `checker.py` itself later
  calls on its own `cargo` child, not an entry point `boole-node` can invoke on `checker.py` from
  outside before `checker.py` even starts.
* **G2** — section 6's corrected bootstrap rule (F1) checks the registry's static issuability flags
  *before* the exhaustion ledger, so a four-tuple that is both ledger-recorded and currently statically
  disabled bootstraps to `Disabled`. Section 11's gate 6, unrevised, still claimed such a four-tuple
  bootstraps to `Exhausted` "under every registry snapshot" — the two cannot both hold for the same
  four-tuple under a snapshot where the static flags currently forbid issuance.
* **G3** — section 11's gate 22 said cargo/rustc exit code 101 with no corroborating cgroup signal is
  `DeterministicReject(checker_rejected)` "regardless of stdout/stderr content," which directly
  contradicts gate 21 (and section 10.2) for the case where the stdout/stderr text *does* match one of
  `_infrastructure_failure_reason`'s two resource-shortage patterns (genuinely or as a forged string):
  that case is supposed to go through the text-derived corroboration path, not fall straight through
  to `checker_rejected`.
* **G4** — section 7's storage-design paragraph still opened with "unchanged from r2's D3," and section
  11's STOP-condition paragraph still pointed readers at "r1/r2's STOP conditions," both reintroducing
  the same need to open superseded documents that F4 was meant to close. Section 7's single-writer
  lock was also left as an unpinned either/or ("a non-blocking `flock()` or PID-lock file") rather than
  one definitive mechanism.

**Historical creation note.** The original landing of this document was a docs-only slice: it did
not edit `policy.json`, `registry-v1.json` or any `boole-node`/`boole-lean-runner` code, and it
performed no model measurement or census work. Later implementation progress is recorded below;
that progress does not retroactively change what the original docs-only slice did.

### Implementation progress (2026-08-23)

The operator subsequently authorized phased RED→GREEN implementation against this consolidated
baseline. The list below includes landed foundations and the current guarded slice; a current-slice
entry becomes authoritative on `main` only after its required CI and merge:

* **Phase 1** — PR #166, main `131244f`: section 4's four-tuple identity and row-owned
  `registryDigest`, plus the then-current `Disabled`/`Exhausted`/`Active(fresh)` bootstrap model.
  Phase 2D removes the now-proven-unreachable stored/bootstrap `Exhausted` branch; this bullet names
  the historical contents of Phase 1, not the current normative model.
* **Phase 2** — PR #167, main `4e19d1e`: the `Active(fresh)` → `InFlight` → `Consumed`
  lifecycle, durable journal replay and fail-closed boot recovery data model.
* **Phase 2C** — PR #168, main `eff95658`: evidence-backed terminal recovery, preservation of the
  original row `registryDigest`, durable retention of stuck `InFlight` rows, strict replay, and the
  single-journal exhaustion projection specified in sections 4–7 below.
* **Phase 2D** — PR #170, main `33dcc025`: removes stored/bootstrap `Exhausted` and exposes the
  evidence-backed terminal projection as the only typed `challenge_exhausted` admission view.
* **Phase 3A.1** — PR #171, main `6cc34b4`: one non-cloneable authority holds a nonblocking
  lifetime `flock`; replay, torn-tail truncation, append and `fsync` use its same file descriptor,
  while path replacement and authority substitution fail closed.
* **Phase 3A.2** — PR #172, main `34c33b6`: one atomic RAII single-slot primitive is ready for one
  future AppState-owned, node-wide instance. Busy acquisition returns exact `native_busy`; normal,
  error and panic-unwind paths release it, and concurrent contenders admit exactly one. The actual
  AppState/route ordering remains unimplemented.
* **Phase 3B.0** — PR #173, the landed guarded policy-binding slice: the frozen checker-internal
  policy and
  the future node-owned execution/containment policy have separate identities. New rows and journal
  events bind `executionPolicyDigest`; new evidence is `boole.native-shadow.evidence.v2`, while
  legacy v1 evidence and unversioned journal events remain read-only replay inputs. The production
  containment-policy bundle and actual Linux executor are not part of this slice.
* **Phase 3B.1** — the current guarded infrastructure-capability slice: a named `ubuntu-24.04`
  job must actually exercise a separate minimal privileged-launcher boundary, delegated cgroup v2
  controls, mount/PID namespaces, bounded executable tmpfs, complete privilege/capability removal,
  cgroup freeze/kill/cleanup and the existing enforced seccomp/Landlock tests. The first PR #174 run
  proved that the earlier unprivileged-user-namespace proposal cannot make `/` recursively private
  on this runner; that path remains RED and is not weakened with a sysctl/AppArmor bypass. The
  successor probe keeps `boole-node` unprivileged and moves only the setup operations that require
  privilege into a separate transient launcher. A second run then stopped before those operations
  because the capability-bounded service could not traverse the runner-owned checkout; the next
  probe stages the byte-identical, root-owned launcher in `/run` rather than adding
  `CAP_DAC_OVERRIDE`. The third run passed the complete job, including injected pre-ready cleanup,
  the normal namespace/cgroup lifecycle and the enforced seccomp/Landlock checks
  ([run 32598640328, job 97093408375](https://github.com/NotoriAndo/Boole/actions/runs/32598640328/job/97093408375)).
  Required `self-test` fails when this job fails, is skipped or is cancelled. This GREEN proves only
  that the named runner supplies the launcher
  prerequisites; it does not implement the production launcher/IPC, freeze a production identity or
  policy, execute the native checker or close containment.

All eight are internal, currently unwired `boole-node` foundations or infrastructure gates. They do
**not** implement an
HTTP endpoint, spawn the checker, activate the production registry, change SharePool/block/reward/
P2P/consensus state, or earn `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN`. Phase 3A.1's focused
lock test uses two opens in one process and does not close the later real two-node-process gate.
Still open are section 7's containment-backed per-record cleanup, section 8's AppState ownership and
permit acquisition in the actual request path, sections 9–10's actual Linux
containment/observation integration, route wiring, the complete RED matrix and one real
node-process raw-answer run. The named Linux job has now proved the infrastructure prerequisites,
but actual containment GREEN remains blocked until the production launcher protocol, dedicated
UID/GID, policy identity and minimal privilege set are pinned and implemented. Generic
`ubuntu-latest` may not substitute for the passing named evidence.

## 1. Non-goals

This specification does not authorize an HTTP route or a
`boole-core`/`SharePool`/block/reward/P2P/consensus change. The internal `boole-node` foundation
phases with cited main commits above are approved and landed. A current-slice entry becomes landed
only through its required CI and merge; containment, route/checker execution and activation remain
outside the completed scope.

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
  resolution, challenge state check, intake). Never persists either native-shadow evidence version and
  never consumes a challenge, because the checker was never reached. Reason codes:
  `malformed_input`, `unknown_identity`, `registry_drift` (section 4 below — now covers both a
  torn/inconsistent read *and* a live registry-file edit observed against an already-bootstrapped
  row), `execution_policy_drift` (the current node-owned policy differs from the immutable row
  binding), `challenge_not_found`, `challenge_exhausted` (section 6 below), `challenge_disabled`
  (section 6 below — new, distinct from `challenge_exhausted`), `challenge_stale` (reserved for the
  future real-issuance path only — see section 6), `intake_rejected`.
* **`DeterministicReject`** — reached only in decision-path stage 5/6, always persists evidence and
  always consumes the challenge, per the authority spec section 6's rule that deterministic
  rejection produces evidence. Reason codes: `checker_rejected` (the pinned checker's own semantic
  `deterministic_reject` verdict, including an ordinary nonzero compiler/test exit);
  `submission_resource_ceiling_breach` (a clean, **non-killed** exit whose text-derived resource
  claim is corroborated by this submission's own cgroup-leaf event counters — section 10 below;
  never reached via a signal death, by construction — see section 10's kill/clean-exit rule);
  `checker_reported_reason_unconfirmed` (a clean, non-killed exit whose text-derived resource claim
  the harness's own independent cgroup-leaf observation could **not** corroborate — section 10
  below).
* **`RetryableUnavailable`** — never persists evidence and never consumes the challenge. Reason
  codes: `native_busy` (section 8 below; replaces `challenge_in_flight`), `containment_wall_clock_kill`
  (any wall-clock-triggered termination, whether checker.py's own internal deadline or the launcher's
  own outer deadline — section 10 below), `containment_killed` (**any other** signal death of the
  containment leaf, unconditionally, regardless of which specific ceiling nominally caused it —
  OOM kill under `memory.oom.group=1`, `cgroup.kill`, an `RLIMIT_*`-triggered `SIGXCPU`/`SIGXFSZ`/
  `SIGABRT`, or a submission-independent scheduling-contingent kill — section 10 below),
  `containment_environment_unavailable` (the harness itself failed to construct the
  cgroup/namespace/tmpfs/lock before the child ever ran — genuinely external, never the submission's
  fault), `checker_internal_error` (the checker's own top-level exception handler fired — a
  structural signal, not a text match, so it is trusted as-is).

`idempotent_redelivery` is not a new verdict; per r2's D1, an exact redelivery of a previously
adjudicated `(state key, candidateDigest)` pair returns the prior durable verdict verbatim rather
than re-adjudicating (section 4 below defines the key precisely).

## 4. Identity, state key and idempotency key (closes E2; revised to close F4's registry-drift gap)

Three distinct keys are in play, and conflating them is exactly what left E2, and later F4, open:

1. **Operational state key (primary storage key)** — revised from r2's D2 five-tuple to the
   **four-tuple**:
   ```
   (familyVersion, templateId, challengeSha256, epoch)
   ```
   This is also the identity used by section 6's permanent-exhaustion **projection**. There is no
   second independently writable exhaustion authority: replay derives the projection only from an
   evidence-backed `TerminalConsumed` event in the same journal.
2. **Permanent exhaustion projection key** (registry-snapshot-independent; section 6 below) — the
   identical four-tuple:
   ```
   (familyVersion, templateId, challengeSha256, epoch)
   ```
3. **Idempotency / redelivery-detection key** — the four-tuple plus the candidate's own digest, a
   five-tuple:
   ```
   (familyVersion, templateId, challengeSha256, epoch, candidateDigest)
   ```
   The four-tuple alone identifies only the *challenge*, not the submitted *answer*; two different
   candidate answers submitted against the same still-active challenge collide under the four-tuple
   alone and must not be treated as the same request. `candidateDigest` reuses, verbatim, the digest
   already defined in the authority spec section 3 — SHA-256 over the exact UTF-8 bytes of
   `rawAnswer` — no new computation. **This reuse is for redelivery/duplicate-request identification
   only and does not reintroduce r1's C2-forbidden pattern** (comparing a candidate digest against a
   pre-registered "correct answer" digest to decide correctness); correctness is decided exclusively
   by executing the checker, never by digest comparison. The underlying *state* transition
   (`Active` → `InFlight` → `Consumed`, with `challenge_exhausted` derived at the admission boundary
   from terminal replay) still keys on the four-tuple alone: the challenge
   itself, once consumed by whichever candidate reaches it first, is spent at the challenge level,
   not the candidate level — single-use semantics are unchanged from the base document.

**Why `registryDigest` is no longer part of the key — this revises E2's original resolution.**
r2's D2, and this document's own E2 resolution, made `registryDigest` a component of the storage
key so a registry-file edit would necessarily produce a distinct row. The fourth review found a real
correctness gap in that design: if the underlying registry file changes on disk *while a challenge
is `InFlight` under the old digest*, a second submission touching the same four-tuple recomputes a
*new* digest at stage 2/3 and, under a digest-keyed lookup, finds **no existing row** for its own
five-tuple — falling through to bootstrap logic and creating a **second, parallel row for the same
underlying challenge**, potentially running concurrently with the first. Digest binding was meant
to *detect drift*, not to let a live file edit spawn a second execution track for a challenge that
already has one in flight.

**Corrected design.** `registryDigest` is a **field on the operational-state row**, not a key
component, set once when that row is bootstrapped from a registry snapshot and never overwritten for
the life of that specific four-tuple's row. `registryDigest` itself is unchanged in every other
respect: the SHA-256 digest of the exact raw bytes of the tracked registry file
(`fixtures/native-shadow/registry-v1.json`) as read from disk — a whole-file content digest, with no
canonicalization or reserialization step, the same convention every other digest field in that file
already uses (`checkerArtifactHash`, `policySha256`, `anchorSha256`, `taskSha256`). It is recomputed
on every single submission at decision-path stage 2/3, never cached from node startup — the base
document section 5.3 per-submission drift-recompute discipline, applied to this field as before.
What changes is only *what the recomputed value is checked against*: every submission looks its
targeted row up **by the four-tuple**, then compares its freshly recomputed `registryDigest` against
**that row's own stored field**. A mismatch is `PrecheckReject(registry_drift)`, exactly as before —
but now the row is always found first (by the stable four-tuple identity), so a live registry-file
edit can never spawn a parallel row for a challenge that already has one: it can only ever produce a
drift rejection against the one row that already exists. This is the mechanism that closes F4's
mid-flight-registry-change gap; section 7 states the accompanying runtime rule for what happens to
an already-`InFlight` row when this occurs.

## 5. Challenge lifecycle states (final; `Disabled` added to close F1)

* **`Active(fresh)`** — registered and not yet consumed. For the `nonIssuable` qualification path
  (section 6), "fresh" means solely "not permanently exhausted and not statically disabled"; it
  carries no time component.
* **`InFlight`** — a single execution is currently running against this challenge; written
  durably before the checker is invoked and cleared on completion (section 7).
* **`Consumed`** — terminal; one execution completed and evidence was persisted for this challenge.
* **`Exhausted`** — a derived **serving/admission view**, not a durable `ChallengeState` and never a
  journal bootstrap value. The durable operational row remains `Consumed`; the route exposes it as
  `challenge_exhausted` only when replay also derives the matching permanent-exhaustion projection
  from the same evidence-backed `TerminalConsumed` event (section 6). A legacy exhaustion-only file
  cannot create this view.
* **`Disabled`** — terminal, **new in this document, closes F1**. Reached when the registry's own
  static declaration for this four-tuple (`activationAllowed: false` at the registry file's top
  level, and/or `nonIssuable: true` on the specific template) forbids issuance, checked **before**
  the challenge has ever been bootstrapped to `Active(fresh)` at all. Unlike `Exhausted`, a
  `Disabled` row records no false history: it was never run, never issued, never consumed — only
  statically forbidden. Section 6 defines exactly when a row bootstraps to `Disabled` instead of
  `Active(fresh)`.
* **`Expired`** — retained only for the future real-issuance (non-`nonIssuable`) path per the base
  document's C6/D3 out-of-scope carve-out. It is explicitly **not applicable** to the `nonIssuable`
  qualification path this document specifies — see section 6.

## 6. `nonIssuable` permanence and the freshness rule (closes E1; revised to close F1)

Direct read of `fixtures/native-shadow/registry-v1.json`: the file declares `"activationAllowed":
false` at its own top level (applying to every template the file contains) and the one tracked
template additionally carries its own `"nonIssuable": true` field. The file has **no expiration
timestamp, TTL or freshness-window field of any kind** — only `epoch: 0` and the pinned digest
fields. The base document's `Active(fresh)` definition ("registered, within its freshness window,
and has not yet been consumed") assumes a freshness-window concept that does not exist in the data
this document is meant to govern. Inventing a TTL value now would not be grounding it in real data,
it would be guessing.

**Resolution, part 1 (unchanged from this document's original E1 fix). No time-based expiration for
the `nonIssuable` path.** For a challenge with `nonIssuable: true` in the registry, "fresh" means
exclusively "not yet permanently exhausted and not statically disabled" (section 5); no wall-clock or
TTL concept applies. Time-based expiration (the `Expired` state) is explicitly deferred to the
separate, later, undesigned real-issuance path, consistent with the base document's own C6/D3
carve-out.

**Resolution, part 2, corrected to close F1: the registry's own static flags gate BEFORE the
exhaustion projection, not after.** This document's original E1 fix described a separate permanent
`registryDigest`-independent exhaustion ledger. Phase 2C closes the resulting split-authority gap:
permanent exhaustion is now derived exclusively from an evidence-backed `TerminalConsumed` event
in the same state-transition journal. The logical ordering remains unchanged. A record of past
consumption can block *revival after consumption*, but cannot prevent the **first** activation of a
four-tuple that has no terminal history. On a clean node the projection is empty, so the registry's
static gate must still run first to prevent the tracked non-issuable fixture from ever becoming
`Active(fresh)`.

**Corrected bootstrap rule, two ordered checks, for every registry-declared four-tuple with no
existing durable state row:**

1. **Static issuability gate (new, checked first).** Read the four-tuple's own declared flags
   directly from the registry snapshot: the registry file's top-level `activationAllowed` field, and
   the specific template's own `nonIssuable` field. If either says issuance is not allowed
   (`activationAllowed: false` at the file level, or `nonIssuable: true` on that template), the row
   bootstraps directly to `Disabled` (section 5) and the remaining check is never reached. This
   is a pure function of the registry's own already-trusted, already-verified content — no new
   digest, no new field, no new trust boundary — and it is what makes the current tracked fixture
   (both flags true) unreachable as `Active(fresh)` from the very first startup onward, closing F1
   exactly.
2. **Bootstrap to `Active(fresh)`.** This is reached only when check 1 permits issuance. No
   exhaustion check belongs in the no-row bootstrap branch: a valid `TerminalConsumed` event can
   exist only after `Bootstrap` → `InFlight` → `Evidence` for that same row, and replay preserves
   that durable row as `Consumed`. Therefore "no row exists" and "terminal history exists" cannot
   both be true in a valid journal.

**Existing terminal-row rule.** A replayed `Consumed` row is never bootstrapped or revived. Its
matching terminal-event-derived exhaustion projection is checked as an invariant, and the route
derives the outward `challenge_exhausted` admission result from those two facts without mutating the
stored row to a second state. A changed registry digest still follows section 4's
`PrecheckReject(registry_drift)` rule and can never create a parallel row. `Exhausted` must not be
serialized as `Bootstrap`, written as a standalone state transition, or synthesized from a legacy
exhaustion-only file.

A four-tuple's static flags are read from the current registry snapshot when a genuinely new row is
bootstrapped. Existing rows are always looked up first by four-tuple and retain their original
`registryDigest`; per-submission digest recomputation detects any later registry edit as
`registry_drift` before it can alter or duplicate that row.

On the eventual `Consumed` transition of a challenge that was statically issuable and did run, one
durable `TerminalConsumed` journal event atomically records both terminal consumption and permanent
exhaustion (section 7). There is deliberately no second exhaustion append or file that can drift
from it.

**Test-only registry required for automated tests, new in this round.** Under the corrected rule
above, the real, currently tracked production registry
(`fixtures/native-shadow/registry-v1.json`) can **never** produce a row that reaches `Active(fresh)`
at all — its one template is disabled by both flags. An automated test suite exercising the real
`Active(fresh)` → `InFlight` → `Consumed` lifecycle and derived `challenge_exhausted` admission
view therefore cannot use the production registry; it would never observe anything but `Disabled`.
The later RED→GREEN implementation slice
must add a **separate, explicitly test-only registry fixture** (for example
`fixtures/native-shadow/registry-test-only-v1.json`) containing at least one synthetic template with
`activationAllowed: true` and `nonIssuable: false`, used only by the node's own test harness, never by
production configuration. Two safeguards go with it: (a) `boole-node`'s production configuration path
must load the registry from a configuration-pinned path that is asserted, at startup, to be the real
tracked production file — never the test-only fixture; (b) the test suite itself must assert that the
test-only registry is never the file production configuration resolves to. This prevents the
test-only, deliberately-issuable fixture from ever being mistaken for a real activation gate.

## 7. Durable storage, single-writer lock, and crash recovery order (closes part of E3; revised to close F4's persist-ordering gap)

State transitions, exact node-owned evidence and the permanent-exhaustion projection share one
authoritative NDJSON journal, with no second writable exhaustion store and no new dependency. The
implementation reuses `crates/boole-node/src/durability.rs`'s durable append/fsync discipline and
the `FileBountyEventLedger` append/recover shape from
`crates/boole-node/src/bounty_event_store.rs` — confirmed no sqlite/sled dependency exists anywhere
in the workspace and none is introduced here. Phase 3 must bind replay and every append to the same
lifetime-held, flocked file descriptor; reopening only a pathname is insufficient because that path
can be replaced with a different inode while the original lock remains held.

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

1. Acquire an OS-level single-writer lock via `flock(2)` (`LOCK_EX | LOCK_NB`) on the durable ledger
   file itself, held for the process's entire lifetime by keeping the underlying file descriptor open
   — pinned to this one mechanism, not a separate PID-lock file, which would add its own
   stale-PID/crash-cleanup failure mode this design otherwise avoids. If the lock cannot be acquired
   immediately, refuse to start. This closes a real gap this document's earlier "atomic CAS" argument
   for a single in-process lock left open: an in-process lock implicitly assumes single-process
   operation but names no mechanism that actually prevents a second node process from starting against
   the same ledger file.
2. Replay the durable journal to reconstruct current per-key state.
3. For each key found in `InFlight` state without a matching `TerminalConsumed` event,
   processed **one key at a time**: (a) locate and force-clean that key's own cgroup leaf,
   private mount namespace and tmpfs workspace — freeze, `cgroup.kill`, verify `populated=0` **and**
   verify no remaining task anywhere still references that key's private mount namespace (section 9)
   — and confirm completion; (b) check whether durable evidence already exists for this key (see
   below — this is new, closes F4's persist-ordering gap); (c) only after (a) and, where evidence
   exists, (b)'s branch is resolved, durably append the appropriate transition for that **same** key.
   If the cleanup in (a) cannot be confirmed, or the durable write in (c) fails, that key is left out
   of service (not reverted, not served) and startup fails closed rather than proceeding on an
   ambiguous in-memory-only state. Per-record ordering, not two global passes, removes the crash
   window: at no point does any single key exist in a state where it is servable but its predecessor
   process might still be alive.
4. Bootstrap any registry-declared key with no existing durable record, applying section 6's two
   ordered checks (static issuability gate, then `Active(fresh)`). A terminal-event-derived
   exhaustion projection always accompanies an existing durable `Consumed` row and therefore never
   enters this no-row branch.
5. Only after steps 1-4 complete for every key does the route begin serving requests.

**`InFlight` → evidence → terminal-transition ordering, pinned (new, closes F4).** During normal,
non-restart operation, when a checker execution completes, the node must durably persist evidence
**before** advancing the row past `InFlight` to `Consumed`. The following single
`TerminalConsumed` event records consumption and permanent exhaustion together; there is no paired
write to a separate exhaustion ledger. This ordering, and not its reverse, is required because
the two possible partial-failure outcomes are not equally bad:

* If the **evidence write** fails, the row is left at `InFlight` with no terminal-state write ever
  attempted. No evidence for a real, decided outcome is ever silently lost, because none was ever
  claimed to exist.
* If evidence had instead been allowed to persist *after* an earlier terminal-state write, and that
  earlier terminal write had itself raced ahead while the evidence write then failed, the challenge
  would show as permanently spent with **no evidence on file to justify it** — an unrecoverable loss,
  since the challenge can never be reissued to re-derive that evidence. Persisting evidence first
  makes this failure mode structurally unreachable: a terminal-state write is never attempted until
  evidence for it already exists on disk.

The single node-wide try-lock (section 8) is held across this **entire** persist-then-transition
sequence, not only across the checker's own execution — it is released only after the terminal-state
write succeeds, or after the row is confirmed left at `InFlight` on failure. This closes a narrow
race the fourth review implied: without this, a second submission could observe the global slot as
free in the gap between the checker exiting and the durable writes completing.

**Generalized recovery, no longer startup-only (closes F4's mid-flight-registry-change follow-on and
the stuck-`InFlight` case).** The per-record procedure in this section is not exclusively a
startup-time procedure. Any code path that discovers a row `InFlight` while the single node-wide
try-lock is currently free — whether at node startup (steps 1-5 above), or at decision-path stage 5
on a still-running node, immediately after acquiring the (just-confirmed-free) try-lock and finding
the row it is about to act on is already `InFlight` — must run the same per-record procedure before
treating that row as usable, with one added branch:

* If durable evidence for this four-tuple already exists but the terminal-state write never
  completed (the second partial-failure mode above), recovery **completes the terminal-state write
  directly** — it does not revert to `Active(fresh)` and does not re-invoke the checker, since a
  real, decided verdict already exists and evidence must never be produced twice for one outcome.
* If no evidence exists for this four-tuple, recovery reverts the row to `Active(fresh)` once cleanup
  is confirmed, exactly as in the startup path, and the triggering submission may then proceed to
  execute against it normally.

This is also the mechanism that resolves what happens when the registry file changes on disk while a
challenge is `InFlight` (F4): because section 4 now looks a row up by its stable four-tuple identity
first, a submission arriving after such a change finds the *same* existing row rather than
bootstrapping a parallel one; if that row is `InFlight`, the generalized recovery procedure above
governs it exactly as any other `InFlight`-with-free-try-lock encounter, and if it is still genuinely
executing (try-lock held), the arriving submission is simply rejected `RetryableUnavailable(native_busy)`
by section 8's existing rule, never granted a parallel execution track.

## 8. Concurrency: the `native_busy` unification (closes the remainder of E3/D4)

r2's D4 point 5 already fixed global concurrency at exactly 1 native execution system-wide (not
per-challenge) but left the concurrent-arrival rejection behavior ambiguous ("reject or queue").
This document pins it, and in doing so retires a redundant reason code:

**Rule.** A single node-wide, non-blocking try-lock (in-process; distinct from section 7's
cross-process OS-level ledger lock) is acquired at the start of decision-path stage 5, for every
submission, regardless of which challenge key it targets. If the try-lock is already held by another
execution — whether for the same key or a different key — the submission is rejected immediately as
`RetryableUnavailable(native_busy)`. No queueing, no waiting: the decision is synchronous, made
before any workspace or cgroup setup begins. Once acquired, the try-lock is held through section 7's
full persist-then-transition sequence (including, when applicable, the generalized recovery
procedure), not only through the checker's own execution.

**Why this retires `challenge_in_flight`.** With global concurrency fixed at exactly 1, the only way
any specific challenge could ever be observed genuinely, actively `InFlight` is if the single global
execution slot is currently occupied running that exact challenge. A per-challenge-scoped rejection
reason (`challenge_in_flight`, carried from r1 and reclassified but not removed by r2's D1) is
therefore strictly redundant with, and narrower than, the correct global check — it can never fire in
a case the global `native_busy` check would not also cover. `challenge_in_flight` is retired as a
distinct outward-facing reason code. The durable per-key `InFlight` marker itself is unaffected and
still exists in the journal purely for crash-recovery bookkeeping (section 7) — it is bookkeeping,
not a rejection reason. A durably `InFlight` row encountered while the try-lock is free is not an
active execution at all; it is handled by section 7's generalized recovery procedure, never by
`native_busy`.

## 9. Process-tree containment: contract, concrete policy values, and execution order (closes E4; revised to close F3 and F4's self-sufficiency gap)

**The full containment contract, inlined — no longer "unchanged from" a superseded document.**
Every submission's toolchain build runs inside a dedicated Linux cgroup v2 leaf that wraps the
checker's *entire* process tree — `checker.py`, `cargo`, `rustc`, the linker and the compiled test
binary, not just the immediate child. Concretely, this document commits to the following seven
mechanical properties (carried forward from r2's D4, restated here in full so this document is the
only one that needs to be open):

1. **Rate vs. total CPU.** `cpu.max` bounds a rate, not a total; a tree-wide cumulative ceiling is
   separately enforced via `cpu.stat`'s `usage_usec` counter (concrete value below).
2. **Memory overrun is confirmed and the whole tree dies together.** `memory.oom.group=1` (below)
   makes any OOM event kill every process in the leaf atomically; `memory.events`' counters
   authoritatively confirm what happened, after the fact, for classification (section 10).
3. **The workspace ceiling is filesystem-enforced, not only measured.** A size- and inode-bounded
   tmpfs mount (below), not periodic `du`-style polling as the sole mechanism.
4. **The sandboxed child never inherits a writable cgroup control file descriptor.** Any descriptor
   the harness opens to write cgroup control files is closed or `FD_CLOEXEC` before the untrusted
   tree can run (folded into the pre-execution ordering below).
5. **Concurrency fixed at 1**, system-wide — section 8.
6. **Cleanup verification applies to every outcome, not only kills** — `populated=0` and leaf-cgroup
   removal confirmed before the node responds, whether the outcome was success, a checker-reported
   rejection, or a kill.
7. **`cgroup.freeze` + `cgroup.kill` is the only termination path**, no iterative SIGKILL fallback; a
   kernel lacking `cgroup.kill` is a startup capability-probe failure, fail closed.

The leaf cgroup is assigned to the spawned process race-free at spawn time (via `clone3()`'s
`CLONE_INTO_CGROUP`, or by writing to `cgroup.procs` before the process execs). macOS has no
equivalent kernel primitive; its treatment is stated in full, without qualification, below.

**Two policy owners; the frozen checker policy is byte-preserved.**
`native/checker/rust-tuple-struct-project-v1/policy.json` already
tracks real numeric ceilings — `wallSeconds: 60`, `taskTotalWallSeconds: 90`, `cpuSeconds: 120`,
`memoryBytes: 2147483648` (2 GiB), `outputBytes: 1048576`, `fileBytes: 67108864`,
`openFiles: 128` — applied today only as process-level `RLIMIT_*` values inside
`checker.py`'s `_set_limits`, which its own comment states "requires a dedicated cgroup or PID
namespace and is outside this non-activatable qualification release" for process-count containment.
That file's SHA-256 (`940bc5d8…`) is already part of the checker release, registry and real-ACCEPT
parity history and **must not be edited** to add node-level cgroup settings. Checker-internal policy
remains identified by evidence `policyDigest`.

The cgroup-level values below belong to a separate, node-owned containment-policy bundle. Its exact
raw-byte SHA-256 is `executionPolicyDigest`, bound independently through every new state row,
journal event and v2 evidence object. Phase 3B.0 implements that binding before a production bundle
exists; it does not invent placeholder policy bytes. The later Linux slice may freeze the bundle only
after its UID/GID, privilege model and enforcement profiles are concrete and its named Linux job
actually exercises them. Both the node and the separate privileged launcher must compile or otherwise
cryptographically pin the same final policy bytes rather than accept a request-, environment- or
CWD-selected policy path. Their authenticated, closed IPC and policy-agreement protocol remain open
work; Phase 3B.1 does not invent or claim that production protocol. The following are the values that
bundle must contain:

* **`pidsMax: 128`** — mirrors the existing `openFiles: 128` value for consistency. `cargo`'s
  `testArgs` already force `-j 1` (single-job build), and the submission surface is denied
  `std::thread`/`std::process`/`unsafe`/macro-invocation syntax, so the submitter cannot itself
  amplify process/thread count; the only source of multiplicity is rustc's own internal codegen
  thread pool and the linker's own threading, neither attacker-controlled. 128 is generous headroom
  over that legitimate worst case while still meaningfully bounding a hypothetical fork-bomb-style
  escape. Flagged explicitly as an initial conservative default, subject to empirical tuning once
  the Linux CI runner named in section 10 exists — not asserted as definitively optimal.
* **`memory.swap.max: 0`**, pinned, new this round. Swap is disabled for the leaf cgroup entirely.
  Rationale: with swap available, a submission approaching `memory.max` degrades into slow, highly
  host-dependent thrashing before the ceiling is actually enforced — distorting the wall-clock
  measurement section 10's classification depends on being submission-independent, and making
  behavior non-reproducible across hosts with differing swap configuration. Disabling swap makes a
  memory-ceiling breach manifest promptly and deterministically as the OOM/kill path in item 2 above.
* **Workspace quota: tmpfs, size `536870912` (512 MiB), inode ceiling `8192`, mounted in a
  dedicated private mount namespace.** cgroup v2 has no byte-quota controller of its own, so the
  workspace ceiling must come from the filesystem: this document commits definitively to a size- and
  inode-bounded **tmpfs** mount rather than a loopback-backed filesystem. Rationale: a loopback
  device requires its own
  formatting and mount/unmount teardown on every run, which adds failure modes to exactly the
  crash-recovery path section 7 just tightened (a loopback mount left attached after a crash is one
  more thing recovery must detect and clean); tmpfs is kernel-native, needs no formatting, and its
  lifetime is tied directly to the mount itself being torn down — fewer moving parts to get right
  during recovery. 512 MiB / 8192 inodes comfortably covers a debug-profile build of the tracked
  single-file, dependency-free crate (source, `target/`, a fresh empty `CARGO_HOME`) while still
  bounding disk-fill abuse from pathological monomorphization/codegen bloat, which remains possible
  in safe Rust without `unsafe` or `std::fs`. Also an initial default, not asserted as definitively
  optimal.
  * **Mount namespace, options and teardown, pinned, new this round.** The tmpfs is mounted inside a
    **private mount namespace** created for that submission alone (`unshare(CLONE_NEWNS)` on the
    process that will become the tree's ancestor, performed as part of the pre-execution ordering
    below), not the node's own default namespace — so the mount is invisible to, and cannot be
    interfered with by, any other concurrent or subsequent submission or by the node process itself.
    Mount options, corrected this round: `size=536870912,nr_inodes=8192,mode=0700,nosuid,nodev,
    uid=<containment-uid>,gid=<containment-gid>` — **not** `noexec`. `checker.py` builds and then
    executes the compiled test binary from inside this exact workspace (`cargo test` links and runs
    the test binary under `target/`), so a `noexec` mount would turn even a correct, accepted
    submission into a `Permission denied` failure before any verdict is ever reached; `noexec` is
    dropped from this mount for exactly that reason. `nosuid`/`nodev` remain — the workspace still has
    no legitimate need to host a setuid binary or a device node, and denying those costs nothing this
    submission surface needs. The workspace's isolation instead comes from layers that do not depend on
    denying execution: the dedicated unprivileged UID/GID with no supplementary groups and an empty
    capability set (pre-execution ordering step 4 below), the seccomp/Landlock ruleset (step 6 below),
    and the cgroup ceilings themselves — none of which are weakened by allowing exec on this one mount.
    `uid=`/`gid=` are new this round and close a separate, previously unstated gap: the mount is
    created while the spawning process is still privileged (pre-execution ordering step 2, before the
    privilege drop at step 4), with `mode=0700` — without an explicit owner, that leaves the workspace
    root-owned and unwritable by the unprivileged identity the process drops to moments later. Passing
    `uid=<containment-uid>,gid=<containment-gid>` — the same dedicated unprivileged UID/GID that step 4
    switches to — assigns ownership directly at mount time, so the workspace is writable by the
    process that will actually use it with no separate `chown` step, and no separate failure mode for
    that step to have.
    Teardown: a private mount namespace's lifetime is scoped to the tasks that hold a reference to
    it, not to an explicit `umount` call; once every task inside it is confirmed dead (which cleanup
    already requires, via `populated=0`, per contract item 6 above), the kernel tears the namespace
    and its tmpfs down automatically, and the tmpfs's backing pages are reclaimed as ordinary memory
    at that point. This holds identically for a normal completion and for crash-restart recovery
    (section 7): the recovery order's cleanup-confirmation step is extended to confirm **zero
    remaining tasks referencing that key's private mount namespace**, not only `populated=0` on the
    cgroup — the two are expected to coincide (every task in the namespace is also a member of the
    same cgroup leaf), and recovery treats a mismatch between them as a cleanup failure, fail-closed,
    rather than assuming one implies the other. No separate, additional `umount` call is required or
    specified; asserting an unconditional `umount` step would be a spurious extra failure mode for a
    namespace the kernel already tears down correctly on last reference.
* **`cpu.max`: left unthrottled** (`max 100000`, no rate quota; cgroup v2 still requires a numeric
  period even when the quota is `max`). Rationale: the submission surface
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
  one unit — including `checker.py`'s own process, not only its descendants — removing the
  userspace-detection race that watching `oom_kill` after the fact and then separately firing
  `cgroup.kill` would otherwise leave open. (Section 10 relies on this specific property: it is what
  makes an OOM event independently observable by the privileged launcher's direct wait-status and
  cgroup counters, without trusting anything `checker.py` itself reports. The node may consume that
  fact only through the later authenticated launcher protocol.)
* **`memory.events` monitoring extended beyond `oom_kill`** to also read the `max` and `oom` counters
  on every outcome (not only on a kill): `oom_kill`/`oom` confirm a kill actually happened (used only
  as diagnostic annotation on an already-`RetryableUnavailable` outcome — section 10); `max` confirms
  an allocation was denied *without* triggering the OOM killer, which is the one memory-related
  counter relevant to section 10's narrow corroboration rule for a clean, non-killed exit.
* **`pids.events`' `max` counter**, monitored on every outcome, for the same narrow corroboration
  role as `memory.events`' `max` (section 10) — the fork-blocked-cleanly case, distinct from any kill.
* **Concurrency: exactly 1, immediate `RetryableUnavailable(native_busy)`, no queueing** — section 8.

**Privilege boundary corrected by the first Phase 3B.1 Linux run.** The actual named-runner result
showed that an unprivileged service inside a user namespace cannot perform the required recursive
private-mount transition. Deleting that transition or disabling the host security control would
weaken the contract, so neither is allowed. `boole-node` remains unprivileged and never receives
root or `CAP_SYS_ADMIN`. A separate, minimal, root-owned launcher performs only the privileged setup,
creates a dedicated child inside that envelope, and keeps monitoring outside while only the child
irreversibly becomes the checker identity before any untrusted code executes. The production
launcher binary, closed request format/authentication, installation ownership, dedicated UID/GID,
minimal capability set and crash-recovery protocol are still open and must be frozen before route
wiring. The Phase 3B.1 transient root service is capability evidence for this boundary, not that
production implementation.

**Pre-execution ordering sequence, pinned, new this round, closes F3.** The following steps are
applied **once**, by that separate launcher, to the process that will become `checker.py` — i.e.
before `checker.py`'s own process image is `exec()`'d, not inside `checker.py`. This is deliberate and
important: cgroup membership, mount-namespace membership, dropped privileges/capabilities, and an
attached seccomp-bpf filter or Landlock ruleset are all properties the kernel makes **inherited by
every descendant** a process later forks or execs, and can only ever be **narrowed** further by a
descendant, never widened or removed. Applying the full sequence once, to `checker.py` itself, before
it starts, means every process `checker.py` later spawns via its own existing `Popen` calls (`cargo`,
and transitively `rustc`, the linker, and the test binary) automatically inherits the complete
containment envelope with **zero change required to `checker.py`'s own code** — `checker.py`'s
existing `_set_limits`-applied `RLIMIT_*` values remain in place as an inner, redundant,
defense-in-depth layer, not superseded by the outer cgroup/namespace/seccomp layer.

1. **cgroup join.** Move the about-to-be-spawned process into its dedicated, freshly created leaf
   cgroup (race-free at spawn time, as stated above) — first, so every later step is itself already
   resource-bounded.
2. **Mount/PID namespaces, private `/proc`, and tmpfs.** Create the child with
   `unshare(CLONE_NEWNS | CLONE_NEWPID)` and make it PID 1 in the new PID namespace; the privileged
   monitor launcher stays outside both namespaces. Inside the child mount namespace, immediately
   remount the root filesystem recursively as private
   (`mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)`), new this round — required because Linux's
   default root-mount propagation type is `shared` on most distributions, so `unshare(CLONE_NEWNS)`
   alone does not by itself stop a mount event inside this namespace from propagating out to the
   node's own default namespace or to any other submission's namespace, or vice versa; only after that
   remount, mount a private `proc` filesystem at `/proc` with `nosuid,nodev,noexec`, and only then
   mount the tmpfs workspace at the target path with the options pinned above. The last process in
   the PID/mount namespace exiting tears down both private mounts; cleanup must still verify the leaf
   cgroup is empty and the namespace has no remaining process reference before treating that as done.
3. **FD block.** Close, or mark `FD_CLOEXEC`, every inherited file descriptor above stdin/stdout/
   stderr the spawning process held open — including any handle onto the durable ledger, any cgroup
   control file, and any other submission's workspace — via `close_range()` covering the full
   descriptor table, so the untrusted tree can never inherit a handle it should not have.
4. **Privilege drop.** Switch to a dedicated, unprivileged, single-purpose UID/GID with no
   supplementary groups; drop the full capability set to empty; set `no_new_privs=1`
   (`prctl(PR_SET_NO_NEW_PRIVS, 1)`) so nothing downstream can ever regain a privilege this step
   removed.
5. **The launcher's node-owned outer `RLIMIT_*` application — corrected this round, distinct from
   `checker.py`'s own `_set_limits`.** Before `exec()`, the privileged launcher directly calls
   `setrlimit(2)` — mirroring `policy.json`'s own `cpuSeconds`/`memoryBytes`/`fileBytes`/`openFiles`
   ceilings as `RLIMIT_CPU`/`RLIMIT_AS`/`RLIMIT_FSIZE`/`RLIMIT_NOFILE` — on the about-to-be-`exec()`'d
   process, from the launcher's trusted pre-exec code. This is not, and was incorrectly described in
   an earlier revision as, `boole-node` "applying `checker.py`'s existing `_set_limits`": `_set_limits` is a
   function inside `checker.py`'s own Python code, invoked by `checker.py` on its own `cargo` child,
   and exposes no entry point an external process could call before `checker.py` itself has even
   started. Limits applied here, at this point in the sequence, are bound to the process image before
   `exec()` replaces it, so they persist across the exec and are inherited, by ordinary POSIX `rlimit`
   inheritance, by `checker.py` itself and by every process it later spawns — a fully separate, outer,
   redundant layer. `checker.py`'s own existing `_set_limits` is unrelated to this step and is
   unchanged: it continues to run exactly as it does today, entirely inside `checker.py`'s own code, at
   the point `checker.py` itself spawns `cargo` via `Popen` — an independent inner layer this document
   does not change and this outer step neither replaces nor calls into.
6. **seccomp/Landlock.** Apply a seccomp-bpf filter and a Landlock ruleset, layered, denying at
   minimum: `mount`/`umount2`/`unshare`/`setns` (no re-namespacing), `ptrace` (no process
   introspection), all networking beyond what is strictly required (an explicit kernel-enforced
   backstop behind the existing `CARGO_NET_OFFLINE=true` convention, not a substitute for it), and —
   **this is the specific mechanism that prevents the submission from reopening cgroupfs to loosen
   its own limits** — Landlock filesystem-path denial of any open/write under `/sys/fs/cgroup`
   entirely. Both mechanisms are inherited by every descendant and neither can be removed by a
   lower-privileged process once attached, which is what makes this guarantee hold transitively
   through `cargo` → `rustc` → the linker → the test binary without any of them needing to cooperate.
7. **Exec.** Only after steps 1-6 are confirmed applied does `exec()` replace the process image with
   `checker.py`.

The privileged launcher remains **outside the child envelope** only long enough to construct and
observe it, while `boole-node` remains unprivileged and outside both the envelope and privileged
setup. The launcher observes the leaf cgroup's control files (`cgroup.events`' `populated`,
`memory.events`, `pids.events`, `cpu.stat`) and direct child wait-status for the whole execution,
then reports only the pinned observation/result structure over the future authenticated local
protocol. The node must reject a missing, malformed, mismatched-policy or replayed launcher report.

**macOS: explicit, unconditional refusal — closes F3's macOS gap.** macOS has none of cgroups,
Linux namespaces, seccomp-bpf or Landlock. This is not treated as "unsupported" or "degraded": on any
non-Linux target, the native-shadow containment-dependent execution path refuses, at the earliest
possible check (a startup/configuration-time gate, before the route is even bound — not a
per-submission runtime branch), to spawn **any** child process for this route at all. A submission
arriving on such a host is never given the chance to reach stage 5; the route itself does not start
with this contract enabled. macOS remains permanently qualification-only for this contract,
consistent with the base document's original statement, now stated as a hard, fail-closed refusal to
spawn rather than a described limitation.

## 10. Resource-shortage classification (closes E5; revised to close F2)

**The real, already-shipped mechanism this section reconciles.**
`native/checker/rust-tuple-struct-project-v1/checker.py`'s `_infrastructure_failure_reason` function
(lines 534-567) has, precisely, three branches, in this order: (1) `code == 0` → no infrastructure
failure; (2) **`code < 0`** (the child died by signal) → unconditionally `resource_process_terminated`
— a pure exit-code-sign check, not a text scan; (3) **`code > 0`** (the child exited normally, cleanly,
without dying by signal) → *only here* does the function scan the captured stdout/stderr text for two
specific patterns, `resource_process_limit` (a fork/exec/thread-creation resource failure, e.g. a
`std::system_error`/"resource temporarily unavailable" message) and `resource_memory_limit` (an
allocation-failure message). Recognizing that branch (2) is structural and only branch (3) is
text-derived is the key that resolves both halves of F2.

### 10.1 The bright-line rule (closes F2's OOM/`memory.events` contradiction)

**Any forced or violent termination anywhere in the observed pipeline — a wall-clock-triggered kill,
an OS signal, or a cgroup enforcement kill including an OOM kill — is `RetryableUnavailable` and
never consumes the challenge, regardless of which specific ceiling nominally triggered it. Only a
clean, non-killed process exit that carries a verdict is ever `DeterministicReject`, and consumes the
challenge.** This replaces the earlier "intrinsic vs. extrinsic" axis, which is the axis that produced
the contradiction: an OOM kill is, by construction (`memory.oom.group=1`, section 9), a signal death
of the *entire* leaf, and a signal death can never legitimately reach the text-scanning branch (3) at
all — it is fully decided by branch (2) at whichever level observes it, before any resource-flavored
text is ever read. Section 3's `containment_killed` reason is the sole, uncontradicted destination
for every signal death; the earlier sentence in this section's prior revision stating that "a genuine
tree-wide resource event ... is `DeterministicReject`" is withdrawn — it described exactly the
contradiction this revision closes, and no version of that sentence survives.

`_infrastructure_failure_reason`'s own branch (2) already implements the correct half of this rule
today, mechanically, with no `checker.py` change required: any signal death of `checker.py`'s own
child is unconditionally `resource_process_terminated` → `RetryableUnavailable`, never text-scanned.

### 10.2 Which self-reports need independent corroboration, and which do not

`checker.py` can report `AuthorityUnavailable` for several distinct reasons. Precisely one pair of
them is derived from submission-influenceable text; every other reason is derived from a structural
fact the submission's own output cannot influence. The launcher preserves the checker result and its
own observations in the future authenticated report; only after verifying that report's identity,
policy binding and replay protection may `boole-node` apply the classification below:

* **Structural, trusted as-is, no corroboration needed:** `resource_process_terminated` (exit-code
  sign, branch 2 above — covers every signal death, including an OOM kill under `memory.oom.group=1`,
  a wall-clock-triggered `SIGKILL`, and any `RLIMIT_*`-triggered signal); `resource_wall_limit`
  (`checker.py`'s own internal `wallSeconds` deadline, a monotonic-clock comparison, not text — this
  is precisely the case F2 flagged as at risk of misclassification, and it requires **no**
  corroboration from the launcher's separate, much longer outer deadline at all, closing that half of
  F2); `resource_output_limit` (a byte-count comparison, not text); `contained_process_unavailable`,
  `policy_unavailable`, `toolchain_unavailable`, `scratch_root_unavailable`,
  `scratch_workspace_unavailable` (each a structural setup-failure check, not text);
  `checker_internal_error` (the top-level exception handler firing, a structural signal about
  `checker.py`'s own execution, not about the submission).
* **Text-derived, requires independent corroboration:** `resource_process_limit` and
  `resource_memory_limit` — reachable **only** via branch (3), i.e. only on a clean, non-killed
  (`code > 0`) exit. These two, and only these two, are where an untrusted submission's own printed
  text could in principle influence the reported reason, and they are the only two this section's
  corroboration rule applies to.

**Why the launcher's outer 90-second ceiling was never the right corroboration mechanism for
`checker.py`'s own internal 60-second timeout.** `checker.py`'s own process always exits `0`
regardless of its internal verdict (`main()`, `checker.py:663`), so its wait-status is uninformative
to the launcher as an outer observer — but `resource_wall_limit` itself is not text-derived at all
(it is a plain monotonic-clock comparison inside `checker.py`, unconditionally trusted per the list
above), so it never needed corroboration in the first place. The launcher's
`taskTotalWallSeconds` (90s) ceiling remains exactly what it always was: a safety net for the
structurally distinct case where `checker.py` itself never returns at all (a hang or a bug in
`checker.py`), not a mechanism for re-confirming a timeout `checker.py` already correctly reported
and already correctly acted on (by killing its own child's process group) 30 seconds earlier.

**Corroboration mechanism for the two text-derived reasons, on a clean exit only.** The launcher
checks this specific submission's own leaf-cgroup event counters (section 9): `pids.events`' `max`
counter for `resource_process_limit`; `memory.events`' **`max`** counter (specifically `max`, never
`oom_kill`/`oom` — those two counters imply a kill, which would already have produced `code < 0` and
therefore never reached branch (3) at all, so they are not the relevant counter here) for
`resource_memory_limit`. If the relevant counter is nonzero for this submission's own leaf, the claim
is corroborated: a genuine, submission-specific, host-load-independent, reproducible ceiling breach
against a fixed node-configured value really did occur → `DeterministicReject
(submission_resource_ceiling_breach)`. If the counter is zero, the claim is unconfirmed (and, given
`checker.py`'s own denylist of macro-invocation syntax, most plausibly explained by an unrelated,
ordinary compile/test failure whose message merely happens to resemble a resource complaint) →
`DeterministicReject(checker_reported_reason_unconfirmed)`. **Both outcomes are `DeterministicReject`,
never `RetryableUnavailable`** — by definition, `code > 0` on a clean exit means nothing was killed,
so this branch can never legitimately be a "retry might succeed" case; the two sub-reasons differ
only for honest audit telemetry, not for challenge-consumption behavior. This closes F2's second half
precisely: the only place independent corroboration is required, needed, or meaningful is this one
narrow, `code > 0`-scoped pair of reasons — nowhere else.
The launcher includes the raw counters and wait status in its authenticated report; `boole-node`
recomputes this classification from those bound fields rather than trusting a free-form launcher
verdict string.

### 10.3 Resolving cargo/rustc's exit code 101, mechanically (unchanged from this document's original E5 resolution)

101 is a *normal*, non-negative exit code — it is neither a timeout nor a signal death — used by
cargo/rustc both for genuine host resource shortage during compilation and for an ordinary
compile-error rejection of the submitted code, so an exit-code allowlist cannot disambiguate it in
isolation. Under section 10.2's rule, a normal, non-negative exit code that neither the text scan nor
its corroboration check flags as a resource claim always falls through to `DeterministicReject
(checker_rejected)` — this is already `checker.py`'s own separate, structurally correct branch
(`if code != 0: raise SubmissionRejected("compile_or_hidden_test_failed")`, `checker.py:624-625`);
the only defect was that `_infrastructure_failure_reason` could previously preempt that correct branch
via an uncorroborated text match. With corroboration required, an uncorroborated match now correctly
falls through to `DeterministicReject(checker_reported_reason_unconfirmed)` instead of ever reaching
`RetryableUnavailable` — never silently promoted past the checker's own semantic judgment.

**Required anti-forgery test**, unchanged from r2's D5.2: a submission whose captured stdout/stderr
contains a forged resource-shortage-looking string but whose harness-observed facts (section 10.2)
show a normal, unsignaled exit with no corroborating cgroup-counter evidence must classify as
`DeterministicReject(checker_reported_reason_unconfirmed)` — never `RetryableUnavailable`, and never
silently reclassified as a plain `checker_rejected` either, so the audit trail honestly records that
an uncorroborated resource claim was made and rejected as such.

**Named Linux CI runner requirement, revised by the first Phase 3B.1 run.** No
cgroup-v2-delegation-dependent test may declare GREEN by skipping. Per r2's D5.1, a passing run on a
permission-less host does not count, and a skip must be visible with a named reason, never silent.
CI now contains the named `native-shadow-containment-linux` job pinned to `ubuntu-24.04`, and the
required `self-test` result explicitly depends on that job succeeding. The first PR #174 run proved
that delegated cgroup controls are writable but also proved that the original unprivileged-userns
mount transition is denied by the runner's kernel/security policy. The successor therefore probes a
separate privileged launcher while keeping `boole-node` unprivileged. The second run stopped before
the probe because that capability-bounded service could not traverse the checkout; staging the exact
reviewed launcher bytes in root-owned `/run` fixes that path dependency without granting a
filesystem-override capability. The successor must still assert *actual
write access* and actual namespace, tmpfs, privilege-drop, cleanup and seccomp/Landlock behavior.
The third PR #174 run passed every one of those required operations on the named runner
([run 32598640328, job 97093408375](https://github.com/NotoriAndo/Boole/actions/runs/32598640328/job/97093408375)),
so this infrastructure-capability prerequisite is GREEN. A skipped, permission-less, generic
`ubuntu-latest` or weakened run still cannot replace that evidence. Production launcher/IPC,
dedicated identity, frozen policy and route/checker execution remain open.

## 11. Consolidated RED gates and STOP conditions

Supersedes the base document's 8 gates, r1's 25-row table and 14-gate addendum, and r2's 14-gate
addendum, for implementation purposes. This section, together with the authority spec's own section 9
gates (which are the outer contract and are unaffected), is the **complete** RED-gate and
STOP-condition list for this document — no cross-reference to the base document's, r1's or r2's own
gate/STOP lists is needed for implementation. All of the following must have a failing test before
implementation:

1. `PrecheckReject` never persists evidence and never consumes a challenge (stages 1-4).
2. `DeterministicReject` always persists evidence and always consumes the challenge (stage 5/6 only).
3. The four-tuple state key correctly identifies a challenge whose registry file changed on disk
   while the challenge was already bootstrapped: a submission recomputing a new `registryDigest`
   against the *same* four-tuple's existing row is `PrecheckReject(registry_drift)`, never a second,
   parallel bootstrap of a fresh row for the same four-tuple (section 4).
4. The five-tuple idempotency key returns the prior durable verdict verbatim on an exact redelivery,
   and treats two different `candidateDigest` values against the same four-tuple as distinct requests.
5. The currently tracked production fixture (`registry-v1.json`'s one template, both
   `activationAllowed: false` and `nonIssuable: true`) bootstraps to `Disabled` — never
   `Active(fresh)`, never `Exhausted` — on a brand-new node with no terminal journal history,
   proving first-activation is blocked, not only revival (section 6).
6. Replay of an evidence-backed `TerminalConsumed` event preserves the durable row as `Consumed`
   and reconstructs a matching permanent-exhaustion projection for the same four-tuple. The
   submission-facing resolver derives `challenge_exhausted` from those facts and never bootstraps
   `Active(fresh)` or a stored `Exhausted` row. A legacy exhaustion-only file or a terminal event
   without matching durable evidence has no authority to exhaust any challenge; registry drift
   against the existing terminal row is rejected without revival or second-row creation.
7. A test-only registry fixture with `activationAllowed: true`/`nonIssuable: false` is required to
   exercise `Active(fresh)` → `InFlight` → `Consumed` in automated tests; a test asserts production
   configuration never resolves to that test-only fixture's path (section 6).
8. No time-based expiration applies to a `nonIssuable` challenge; `Expired` is unreachable on that
   path.
9. Crash-recovery per-record ordering: for a simulated crash between cgroup/namespace cleanup and the
   durable revert-to-`Active(fresh)` write, the record is never reverted before its cleanup
   (including private-mount-namespace reference cleanup, section 9) is confirmed, and a failed
   durable write leaves the route refusing to start rather than serving an ambiguous state.
10. Two node processes cannot both start against the same ledger file (OS-level lock enforced).
11. Exactly one native execution runs system-wide; every concurrent arrival — same key or different
    key — is immediately rejected `RetryableUnavailable(native_busy)` with no queueing.
12. `challenge_in_flight` does not exist as an outward-facing reason code.
13. A row found durably `InFlight` while the global try-lock is free — at startup or at request time
    on a still-running node — is recovered via section 7's generalized procedure: reverted to
    `Active(fresh)` if no evidence exists for it, or its terminal-state write completed directly
    (never re-executed, never reverted) if evidence already exists for it.
14. Simulating an evidence-write failure leaves the row `InFlight` with no terminal-state write
    attempted; simulating a terminal-state-write failure *after* evidence already persisted is
    recovered by completing that terminal write directly, never by reverting to `Active(fresh)` and
    never by producing a second evidence record for the same outcome (section 7). A torn terminal
    tail therefore replays as evidence-backed `InFlight`, not `Consumed`/`Exhausted`.
15. Every cgroup leaf enforces `pids.max`, `memory.max` + `memory.oom.group=1` + `memory.swap.max=0`,
    the cumulative `cpu.stat` ceiling, and the tmpfs workspace size/inode ceiling from section 9.
16. An OOM kill (`memory.oom.group=1` firing) classifies `RetryableUnavailable(containment_killed)` —
    never `DeterministicReject` — proving the section 10.1 contradiction is closed.
17. A clean, non-killed (`code > 0`) exit carrying a text-derived `resource_process_limit`/
    `resource_memory_limit` self-report classifies `DeterministicReject`
    (`submission_resource_ceiling_breach` if the matching cgroup-leaf counter is nonzero,
    `checker_reported_reason_unconfirmed` if it is zero) — never `RetryableUnavailable` in either case
    (section 10.2).
18. `checker.py`'s own internal `resource_wall_limit` self-report is trusted without requiring
    corroboration from the launcher's separate, longer outer wall-clock ceiling (section 10.2).
19. The pre-execution ordering sequence (cgroup join → mount/PID namespaces → rprivate root →
    private `/proc` → tmpfs → FD block → privilege
    drop → RLIMIT → seccomp/Landlock → exec) is applied once, to `checker.py` itself, before exec; a
    test confirms the submission process cannot open or write any path under `/sys/fs/cgroup`
    (Landlock denial verified directly, not inferred).
20. On a non-Linux host, the native-shadow route refuses to start with this contract enabled and never
    spawns any child process for this route — verified as a startup-time refusal, not a per-submission
    runtime branch.
21. The anti-forgery test from section 10.3: forged resource-shortage-looking stdout/stderr text with
    a normal, unsignaled, uncorroborated exit classifies `DeterministicReject
    (checker_reported_reason_unconfirmed)`.
22. Cargo/rustc exit code 101 whose stdout/stderr text matches **neither** of
    `_infrastructure_failure_reason`'s two resource-pattern text scans classifies
    `DeterministicReject(checker_rejected)`, never `RetryableUnavailable` — a positive, unsignaled exit
    with no matching resource-shortage-looking text never enters section 10.2's text-derived path at
    all. This gate does not apply, and gate 17/21 govern instead, whenever the text *does* match one of
    those two patterns (genuinely or as a forged string): that case must go through the corroboration
    check, never straight to `checker_rejected`.
23. `cgroup.freeze` + `cgroup.kill` is the only termination path; a kernel lacking `cgroup.kill` fails
    the startup capability probe closed.
24. `populated=0`, private-mount-namespace reference cleanup, and leaf-cgroup/tmpfs removal are all
    verified on every outcome (success, checker-reported failure, or kill), not only kills.
25. GREEN is not declared from a run where the containment-dependent suite skipped for lack of real
    cgroup v2 delegation on the CI runner.
26. A correct, accepted submission successfully builds and executes its compiled test binary from
    inside the tmpfs workspace — the mount is not `noexec` (section 9), directly guarding against G1's
    regression class of the containment envelope itself blocking legitimate work.
27. The tmpfs workspace, once mounted, is writable by the same unprivileged UID/GID pre-execution
    ordering step 4 drops privileges to, verified via the `uid=`/`gid=` mount options — not
    root-owned-and-unwritable.
28. A mount performed inside one submission's private mount namespace after the `MS_REC|MS_PRIVATE`
    remount is never observable from the node's own default namespace or from any other concurrent
    submission's private mount namespace.
29. The launcher's outer `RLIMIT_*` application (pre-execution ordering step 5) and `checker.py`'s
    own internal `_set_limits` are exercised as two independent layers: a test with the outer layer's
    ceiling set below `_set_limits`'s own ceiling shows the outer layer firing first, and a test with
    only `_set_limits` active (outer layer not yet enforcing) still shows `_set_limits` independently
    bounding the `cargo` child.
30. Journal replay rejects any `TerminalConsumed` event that is not bound to a preceding durable,
    contract-valid evidence event for the same four-tuple, candidate and evidence digest.
31. A legacy standalone exhaustion-only file is non-authoritative: its presence cannot create
    `Consumed` or the derived `challenge_exhausted` admission view, while replay of a valid
    evidence-backed `TerminalConsumed` event reconstructs both the durable `Consumed` row and the
    permanent-exhaustion projection from the one journal.
32. `Exhausted` is unreachable as a serialized/bootstrapped `ChallengeState`; a focused route-free
    resolver test proves `Consumed` + matching exhaustion projection derives
    `challenge_exhausted`, while a missing/mismatched projection fails closed instead of reviving or
    running the challenge.

Stop without fallback — in addition to the authority spec's own STOP list, which governs
independently of this document — if any of the following is true:

* a `nonIssuable` challenge with `activationAllowed: false` or `nonIssuable: true` is ever observed
  `Active(fresh)`, at any point, including the very first startup of a node with no terminal journal
  history;
* any `RetryableUnavailable` classification is found to depend on scanning checker/compiler
  stdout/stderr text rather than a harness-observed process-level or cgroup-event fact;
* an OOM kill, or any other signal death, is classified `DeterministicReject` anywhere in the system;
* a registry file change observed while a four-tuple is `InFlight` ever results in two independently
  progressing rows for the same four-tuple;
* durable evidence is ever produced twice for the same, already-decided four-tuple outcome;
* a legacy exhaustion-only file, or any terminal record without matching durable evidence, makes a
  challenge appear consumed or exhausted;
* the durable ledger can be opened for writing by more than one process at once;
* a child process for this route is spawned on a non-Linux host;
* a correct, accepted submission is ever rejected because the containment envelope itself denies it a
  capability it legitimately needs (e.g. execute permission on its own compiled build output, or write
  permission on its own workspace); or
* CI declares GREEN without a named, delegation-confirmed Linux runner actually executing the
  containment-dependent suite.

## 12. Relationship to the authority spec, BF receipts, and completion label

Unchanged from the base document sections 8 and 10 and the authority spec sections 6, 10 and 11:
this document does not change the authority spec's trust rule, input contract, activation boundary
or completion label. Historical `boole.native-shadow.evidence.v1` remains a read-only replay format
whose `policyDigest` identifies the checker-internal policy. Every new ACCEPT or
`DeterministicReject` evidence write uses `boole.native-shadow.evidence.v2`, adding required
`executionPolicyDigest` for the separate node-owned containment policy; `policyDigest` keeps its
original meaning. Section 10's
classification-override annotation remains non-binding telemetry. Once
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
order, the concrete containment values, the execution order and the resource-shortage classification
rule are specified and approved as the implementation baseline. The design label alone never proves
full implementation: the actual partial implementation is enumerated in the progress section near
the top of this document. Those foundation phases do not close the authority spec's section 4
second prerequisite and do not change
`LLM-MINEABLE-ELIGIBLE-V5`, `mineable_now` (still 0), or any consensus, reward or P2P state.

## 13. Status

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1: IMPLEMENTATION-BASELINE-APPROVED
IMPLEMENTATION: PARTIAL (PHASE-1 / PHASE-2 / PHASE-2C / PHASE-2D / PHASE-3A.1 /
PHASE-3A.2 / PHASE-3B.0 LANDED; PHASE-3B.1 NAMED-LINUX-CAPABILITY GREEN, AUTHORITATIVE
ON PR #174 REQUIRED-CI GREEN + MERGE)
CONTAINMENT-ROUTE-GREEN: OPEN / PRODUCTION-LAUNCHER-IPC-POLICY-AND-ROUTE-UNIMPLEMENTED
```

The base document, r1 and r2 remain the historical record of the first three review passes and are
not edited by this document beyond their own status markers pointing here. A fourth operator review
of this document itself (2026-08-22) found four further gaps (F1-F4, listed above) and closed them in
place, in sections 4, 6, 7, 9 and 10. A fifth operator review (2026-08-22) found that revision itself
left one non-implementable execution step, two prose/RED-gate contradictions and one remaining
self-sufficiency gap (G1-G4, listed above), and this revision closes those too, in place, in sections
7, 9 and 11. Subsequent operator direction authorized phased RED→GREEN implementation, producing
the foundation slices listed above. The named runner now supplies the required delegated cgroup v2
and namespace capability evidence. Further containment/route implementation remains fail-closed:
no Phase 3 GREEN may be claimed until the production launcher protocol, dedicated containment
UID/GID, policy identity and privilege model are pinned and implemented. The partial foundation does
not authorize an endpoint, child-process execution or activation.
