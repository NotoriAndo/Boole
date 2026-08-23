# Native submission shadow verification v1

Status: **AUTHORITATIVE DESIGN — tracked checker and real ACCEPT parity landed; node registry/state
durability foundation partially landed; HTTP route and contained checker execution not landed**

Slice: **`NATIVE-SUBMISSION-SHADOW-ADMISSION-V1`**

Default: **OFF**

Consensus effect: **NONE**

## 1. Purpose

This specification closes the trust gap recorded by Entry 28 of
`docs/llm-mineable-eligibility-census-p1.md`.

The previous closed-local episode proved that a real LLM answer can pass family-specific intake,
an external frozen checker, miner-side binding and local receipt/accounting wiring. It did not
prove that the actual `boole-node` process can receive a raw answer and independently reach the
same semantic verdict.

This slice adds that missing node-owned shadow judgment. It does not activate mining, rewards,
blocks or consensus.

## 2. Trust rule

The node accepts a **raw submission**, never a miner-issued verdict or receipt as authority.

The verdict must be a node-owned pure decision over:

```
raw submitted answer
+ active task/challenge identity
+ tracked pinned family registry
+ tracked pinned checker and policy
+ pinned toolchain and deterministic resource policy
```

The miner is allowed to identify the task and provide its answer. It is not allowed to select the
checker, policy, anchor, toolchain, expected answer or verdict. The node derives those from its
own pinned registry and executes the actual checker itself.

## 3. Dedicated input contract

The dedicated route is:

```
POST /native-shadow/submissions
```

The payload schema is `boole.native-shadow.submission.v1`:

```json
{
  "schema": "boole.native-shadow.submission.v1",
  "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
  "templateId": "<64 lowercase hex characters>",
  "challengeSha256": "<64 lowercase hex characters>",
  "epoch": 0,
  "rawAnswer": "<the complete untrusted model/miner response>"
}
```

All six fields are required. Unknown fields are rejected. In particular, the submission must not
carry an authoritative `verdict`, `receipt`, `checkerDigest`, `policyDigest`, `anchorDigest`,
expected answer or witness. The node computes the candidate digest over the exact UTF-8 bytes of
`rawAnswer` before family-specific extraction.

The existing endpoints are deliberately not reused:

* `/submit` is the existing PoW/share-admission contract and has a different identity, replay and
  accounting model.
* `/receipts` stores signed `boole.receipts.commit.v1` commitments. It is not a raw-work verifier
  and must not become one by accepting a native receipt-shaped payload.

## 4. Node-owned pinned registry

Before the route can be enabled even in shadow mode, the repository must contain a tracked,
byte-pinned registry sufficient for a clean node or CI runner to reproduce the judgment. For each
enabled family/template it binds at least:

* family and version;
* template identity and semantic locator;
* anchor bytes or an immutable tracked locator plus anchor digest;
* challenge/epoch policy and freshness rule;
* checker artifact and checker digest;
* checker-internal policy digest, plus a separately pinned node execution/containment policy digest;
* toolchain identity, binary provenance and invocation contract;
* proof-intake/extraction version;
* deterministic resource limits and containment limits; and
* allowed verdict/reason-code vocabulary.

Gitignored `local-docs` files, machine-global caches and handwritten digest constants are evidence
sources only. They are not runtime authority. The existing frozen checker, generator, fixtures and
toolchain inputs must first be migrated into a tracked fixture/registry surface. The migrated
surface must pass two distinct parity gates before the route is implemented:

* the tracked **actual checker** must reproduce the frozen real ACCEPT case and checker-owned
  negative controls with the same authority digests and normalized verdict/reason codes — **closed
  2026-08-21, see section 4.2**; and
* the node's binding and replay RED matrix must independently cover task, challenge, policy,
  registry and evidence misuse — **design history began 2026-08-22, see
  `docs/node-native-shadow-binding-containment-design-v1.md`; operator review 2026-08-22 withheld
  approval and required six corrections, see
  `docs/node-native-shadow-binding-containment-design-v1-correction.md`; a second 2026-08-22 review
  found five further contradictions, see
  `docs/node-native-shadow-binding-containment-design-v1-correction-r2.md`; a third 2026-08-22
  review found five further gaps and requested one consolidated implementation reference rather
  than a further append-only correction, see
  `docs/node-native-shadow-binding-containment-implementation-spec-v1.md`, which restates the full
  current rule set in one file and controls for implementation purposes. Subsequent operator
  direction approved phased implementation against that baseline: registry/state durability
  foundations are now partial; same-FD journal locking is closed by the Phase 3A.1 foundation, but
  this prerequisite remains open until generalized cleanup, AppState-owned and route-acquired
  global concurrency enforcement, Linux containment, route/checker wiring, the full RED
  matrix and a real named-Linux node run all close**.

Entry 27's `FixedVerdictChecker` reject matrix is miner-wiring evidence, not proof that the actual
checker produced those negative verdicts. Path strings, timing and telemetry need not be byte
identical; authority-bearing digests and normalized semantic outcomes must be identical.

### 4.1 Tracked qualification milestone

The first migration slice is now tracked at:

* `native/checker/rust-tuple-struct-project-v1/` — answer-free semantic checker, policy, release
  manifest and complete file digests;
* `fixtures/native-shadow/registry-v1.json` — strict qualification registry with activation
  explicitly disabled; and
* `fixtures/native-shadow/rust-tuple-struct-project-v1/` — synthetic, permanently non-issuable
  positive and negative fixtures.

`scripts/test_native_shadow_authority.py` proves from tracked files alone that the pinned checker
accepts the public positive fixture, rejects the negative controls, refuses a wrong toolchain and
detects uncoordinated registered file or digest drift. Clean CI installs and SHA-verifies the
official rust-lang per-commit artifacts for rustc `e7795af6d`; the workspace default remains Rust
1.95.0. A date-based nightly is deliberately not substituted because it resolves to a different
compiler commit.

This milestone deliberately copies no real mining answer, author witness, model transcript,
session record, census row or machine-specific compiler binary from the private experiment
archive. It also does **not**, by itself, satisfy the two route prerequisites above: at the time
this milestone landed, both the frozen real ACCEPT parity case and the node-owned binding/replay
matrix were open. A later, separate migration closed the first (see section 4.2). The registry
contains only one non-issuable fixture. A node-internal loader now exists, but no server/route call
site consumes it, and `activationAllowed` remains false.

The qualification release also makes no process-count containment claim. A clean Linux CI run
showed that `RLIMIT_NPROC` counts the shared user's existing processes and threads, so it can reject
a valid answer for reasons outside the task. That limit is removed rather than weakened or raised;
recognized process-exhaustion failures are reported as checker unavailability, and any future
activation must provide task-tree isolation with a dedicated cgroup or PID namespace. The Linux
address-space limit and the other frozen file, output, CPU and wall limits remain qualification
evidence only.

### 4.2 Real frozen-accept parity milestone (2026-08-21)

A second, independent migration slice closes the **first** of the two open route prerequisites
named in section 4: the frozen real ACCEPT case recorded by Entry 27/28 is now reproduced by the
tracked checker from Git-tracked files alone, with no dependency on the gitignored `local-docs`
experiment archive. It is tracked at:

* `fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/` — the real anchor, the real
  extracted historical candidate answer, three negative controls (empty/tampered/one-value
  mutation), task identity, provenance and a complete `SHA256SUMS`. The answer is permanently
  `nonIssuable` and `activationAllowed: false`; and
* `scripts/test_native_shadow_real_parity.py` — proves from tracked files alone, given
  `BOOLE_NATIVE_TOOLCHAIN_BIN`, that the tracked checker independently reaches ACCEPT on the real
  candidate and REJECT on every negative control and on both directions of cross-task binding,
  matching the frozen `FROZEN-PARITY.json` expectations by normalized verdict/reason code rather
  than raw string equality.

This closes `REAL-FROZEN-ACCEPT-PARITY-V1` (label: `REAL-FROZEN-ACCEPT-PARITY-GREEN`;
`docs/llm-mineable-eligibility-census-p1.md` Entry 29; PR #158; main `60814a9`). It does not
implement the `boole-node` route, an HTTP endpoint, or activation, and it does not change
`mineable_now`. The **second** prerequisite — the node's own binding and replay RED matrix — is
unaffected by this milestone and remains open.

### 4.3 Partial node registry/state durability foundation (2026-08-23)

Phased RED→GREEN work against the consolidated implementation baseline has landed three internal
`boole-node` foundation slices:

* Phase 1 — PR #166, main `131244f`: node-owned registry parsing/binding, the four-tuple state
  identity and row-owned `registryDigest`, plus static `Disabled` and terminal-history bootstrap.
* Phase 2 — PR #167, main `4e19d1e`: durable `Active(fresh)` → `InFlight` → `Consumed`
  lifecycle, journal replay and fail-closed recovery data structures.
* Phase 2C — PR #168, main `eff95658`: exact typed evidence before terminal consumption, strict
  replay, original registry-digest recovery, durable stuck-`InFlight` preservation and a single
  evidence-backed journal authority for both consumption and permanent-exhaustion projection.

At the Phase 2C checkpoint, this was partial **data-layer** progress only. `native_shadow` remained
an unwired internal module:
the follow-up must first replace its unreachable stored/bootstrap `Exhausted` branch with a typed
derived admission view over durable `Consumed` + matching terminal projection. Beyond that,
there is no `POST /native-shadow/submissions` route, no child checker spawn, no node-wide execution
permit, no lifetime same-file-descriptor `flock`, no containment-backed cleanup and no cgroup/tmpfs/
seccomp/Landlock execution. The production registry remains disabled. SharePool, blocks, rewards,
P2P and consensus are untouched. Therefore the second prerequisite and
`NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN` remain open; actual containment GREEN additionally
requires a named delegated-cgroup-v2 Linux runner rather than a skipped or generic CI substitute.

### 4.4 Derived admission and same-FD journal authority (2026-08-23)

Further route-free foundations now narrow the open prerequisite without closing it:

* Phase 2D — PR #170, main `33dcc025`: stored/bootstrap `Exhausted` was removed. The typed admission
  view derives `challenge_exhausted` only from durable `Consumed` plus its matching evidence-backed
  terminal projection; registry drift and projection mismatch fail closed.
* Phase 3A.1 — PR #171, main `6cc34b4`: one non-cloneable journal authority holds a nonblocking
  lifetime `flock`, and replay, torn-tail truncation, append and `fsync` all use that same held file
  descriptor. Final symlinks/non-regular files, path replacement, a different live authority and a
  drop/reopen attempt fail closed. The focused lock test uses two opens in one process; it does not
  replace the later real two-node-process integration gate.
* Phase 3A.2 — PR #172, main `34c33b6`: one atomic RAII single-slot primitive is ready for one
  future AppState-owned, node-wide instance. A held slot returns exact `native_busy` immediately,
  and normal, error and panic-unwind paths release it. Concurrent-thread tests admit exactly one
  contender and a route-ordering fixture keeps state and journal untouched on busy. Because no route
  invokes that fixture or primitive yet, it does not prove request-level ordering. AppState
  ownership and stage-5 route acquisition remain unimplemented, so the full request-level gate is
  still open.
* Phase 3B.0 — PR #173, the landed guarded route-free slice: the frozen checker-internal policy keeps its
  existing identity and bytes, while a separate node-owned execution/containment-policy identity is
  bound through new state rows, versioned journal events and evidence. New ACCEPT or
  `DeterministicReject` evidence uses `boole.native-shadow.evidence.v2`; legacy v1 evidence and
  unversioned journal events remain read-only replay inputs. This slice does not yet freeze the
  production containment-policy bundle or execute a checker.
* Phase 3B.1 — PR #174, main `ac4f32f`: a named `ubuntu-24.04` job
  probes delegated cgroup v2, a separate minimal privileged-launcher boundary, mount/PID namespaces,
  executable bounded tmpfs, complete privilege removal, freeze/kill/cleanup and the existing enforced
  seccomp/Landlock behavior. The first PR #174 run kept the required gate RED after the runner denied
  the earlier unprivileged-userns private-mount transition; no sysctl/AppArmor bypass or criterion
  relaxation is allowed. The second run stopped before kernel probing because the deliberately
  capability-bounded service could not traverse the runner-owned checkout path. The successor stages
  the byte-identical, root-owned launcher in `/run` instead of restoring a filesystem-override
  capability. The third run then passed the complete named job, including the injected pre-ready
  failure cleanup, normal namespace/cgroup lifecycle and enforced seccomp/Landlock checks
  ([final run 32598803995, job 97093814188](https://github.com/NotoriAndo/Boole/actions/runs/32598803995/job/97093814188)).
  Required `self-test` explicitly fails unless this job succeeds. This GREEN proves only runner
  capability; it does not implement the production launcher/IPC, execute the native checker or
  close the route gate.
* Phase 3B.2a — PR #175, main `8a773fe`: exact tracked bytes at
  `native/containment/native-shadow-execution-policy-v1.json` and
  `native-shadow-toolchain-identity-v1.json` define the disabled qualification policy, fixed
  service-account UID/GID resolution checks, root-owned install/socket layout, exact framed
  peer-credential message schemas, node-owned intake-to-checker source transfer, all five launcher
  capability sets, cgroup/tmpfs/rlimit values, seccomp/Landlock profiles and observable
  crash-cleanup contract. The registry binds both raw digests. Because this release remains
  inactive, its only normal IPC completion is the separate request-free
  `qualification-hello → qualification-ready(activationAllowed=false) → EOF`; it
  changes no durable state and starts no untrusted checker or execution child. Before socket bind,
  the launcher may run only the exact fixed Rust/Cargo/Python identity probes named by the tracked
  toolchain manifest; those trusted compatibility probes cannot consume a challenge or produce a
  report. Its ready frame is also the authenticated recovery
  barrier: the launcher must first prove a verified manager subgroup, zero active run leaves and zero
  unexpected direct cgroup children. The setgid runtime-directory contract creates the socket with
  the expected root:node group without `CAP_CHOWN`. The future execution contract uses one
  race-free `clone3(CLONE_INTO_CGROUP|CLONE_NEWNS|CLONE_NEWPID|CLONE_PIDFD)` child, exact
  root:checker workspace files/scratch modes, a verdict-specific checker reason vocabulary, report
  cross-field invariants and a total node-owned outcome/evidence/consumption map. The cgroup contract
  moves the launcher into a reserved `manager` child before enabling controllers, leaving execution
  `run-*` leaves as siblings.
  The execute message is forbidden
  until a later durable `InFlightV3` row binds its operation ID; therefore the next implementation
  slice is handshake-only. No launcher, socket, checker process, journal transition or route exists
  in this slice. The toolchain manifest deliberately marks installed Rust files, Python/stdlib and
  system linker/runtime byte provenance as still open activation blockers; its version probes are
  not described as exact installed-byte reproduction. The required Linux capability, self-test and
  supply-chain jobs all passed in
  [run 32603937417](https://github.com/NotoriAndo/Boole/actions/runs/32603937417).
* Phase 3B.2b-0 — PR #176, main `1b02592`: a new minimal shared
  protocol crate compiles in the exact tracked registry, execution-policy and toolchain-identity
  bytes; compares those bytes before parsing; rejects BOM, floats and duplicate keys in parsed
  JSON; rejects unknown or missing fields in the typed registry and qualification messages; models
  every installed registry field; exposes only validated, non-deserializable authority/message
  objects to consuming crates; and pins the disabled qualification hello/ready schemas plus four-byte
  big-endian frame limits. `boole-node` now fixes the production registry path to
  `/usr/share/boole/native-shadow/registry-v1.json`, opens its final component once with
  `O_NOFOLLOW`, verifies root ownership/mode on that same file descriptor and explicitly projects
  the full strict model into the existing lifecycle fields. This slice checks that final file
  component; root-owned/non-writable ancestor-directory verification remains part of the later
  installed-authority handshake gate. It does not connect a socket, generate a nonce, authenticate peer
  credentials, change a journal or spawn any child; those are later handshake slices.
* Phase 3B.2b-1 — PR #177, main `a5b830b`: a private client consumes one
  mock session, verifies mock peer credentials before stream I/O, binds a test-injected nonce and all
  three verified authority digests into the hello, then matches those values plus peer PID and all
  launcher/node/checker UID/GID claims in the strict ready frame. It returns only a private,
  non-serializable in-memory readiness value after node shutdown-write and clean peer EOF. Errors
  consume and drop the owned session, so a failed connection cannot be reused. Required CI and
  merge are complete. It changes no journal or route and does not claim
  a real Unix socket, `SO_PEERCRED`, `getrandom(2)`, account resolution, root launcher, child process
  or Linux handshake; those remain Phase 3B.2b-2 work.
* Phase 3B.2b-2p — PR #178, main `fb73679`: the shared protocol
  crate adds one path-free Unix entrypoint that walks only the literal
  `/usr/share/boole/native-shadow` hierarchy relative to already-opened directory descriptors with
  `O_NOFOLLOW`. It checks root ownership and non-writable ancestors, exact `0555` authority-directory
  mode, and exact root-owned regular one-link `0444` files on the same descriptors before and after
  reading, then reuses the compiled exact-byte/schema/digest verifier for all three authorities.
  Required CI passed before merge. This shared opener is not readiness: it creates no socket, nonce, account resolution, launcher,
  child, journal transition or route, and Phase 3B.2b-2 remains open.
* Phase 3B.2b-2n — PR #179, main `434534e`: one private production
  entrypoint binds the Phase 3B.2b-2p installed authority to the literal launcher socket, a
  one-second nonblocking connect deadline, one exact 32-byte `getrandom(2)` call with no fallback,
  kernel `SO_PEERCRED` before frame I/O and one cumulative five-second handshake deadline. Tests bind
  those fixed values back to the tracked policy and non-Linux refuses before touching files or a
  socket. The root launcher and fixed-account resolution do not yet exist, so this is not a real
  installed handshake and changes no route, journal or checker state.
* Phase 3B.2b-2i — PR #180, main `91de1d1`: the shared protocol boundary resolves
  only the literal `boole-node` and `boole-native-checker` accounts through libc NSS calls and
  verifies their non-root, same-named-primary-group, `/nonexistent`, non-login-shell,
  no-supplementary-group and mutual-separation invariants. The node production adapter no longer
  accepts caller-injected numeric identities; it resolves the same fixed accounts independently.
  The named Ubuntu job creates the two ephemeral system accounts and must run the real libc happy
  path without skipping. This slice creates no launcher, socket, cgroup, journal transition or
  checker child, so the real installed handshake remains open.
* Phase 3B.2b-2l — PR #181, main `89a4aa2`: a non-publishable launcher
  library accepts only an in-crate, kernel-credential session and an opaque verified-startup token.
  It authenticates the node peer before frame I/O, reads exactly one strict qualification hello,
  matches all three authority digests, writes and flushes the disabled ready frame, requires clean
  node EOF, then shuts down its write half. A second frame or any failure returns no capability and
  the owned session is dropped. This slice deliberately has no startup-token constructor, Unix
  adapter, executable, lock, socket bind, cgroup recovery, route, journal transition or checker
  child, so it is not a real installed handshake.
* Phase 3B.2b-2u — PR #182, main `6164f32`: the already-connected Unix-session slice adds the launcher-side Linux
  adapter without adding a listener. It reads real kernel `SO_PEERCRED` before any frame, applies
  one cumulative five-second deadline across peer lookup and all stream operations, and performs
  only `SHUT_WR` after the core has observed clean node EOF. The named Ubuntu job executes the real
  socketpair/half-close path, while a focused test proves credential rejection does not consume a
  queued hello. The adapter accepts no caller path, timeout or numeric identity, and the timeout and
  peer-order rules are bound to the tracked policy. There is still no constructor for the opaque
  verified-startup token, launcher binary, listener/bind, lock, cgroup recovery, route, journal
  transition or checker child, so the installed handshake remains open.
* Phase 3B.2b-2r — PR #183, main `b582869`: the launcher-privilege slice checks the calling Linux thread against the
  frozen root and exact-capability shape without accepting a path, mask or identity from a caller.
  The proof is private and cannot move to another thread. The named Ubuntu gate runs the same
  production check under the exact systemd service capabilities and proves that both a missing bit
  and an extra bit fail closed. It does not assemble launcher readiness, open authority, take a
  lifetime lock, recover cgroups, bind/listen, mutate a journal/route or run a checker.
* Phase 3B.2b-2s — the current pre-lock-composition slice consumes that thread-bound privilege proof,
  opens the three exact installed authority files and resolves the two fixed NSS service accounts,
  in that order and without caller input, into a new opaque thread-bound prerequisite. Failure at
  any stage prevents every later stage. A named Ubuntu gate stages byte-identical root-owned
  authority files and invokes this production composition under the exact capability service and
  fixed accounts. This proof is deliberately not launcher readiness: it takes no lock, creates no
  launcher ID, recovers no cgroup, binds no socket, changes no journal/route and spawns no checker.

There is still no route or checker spawn, no AppState/route use of the `native_busy` primitive, no
containment-backed per-submission cleanup and no native-checker execution under the combined Linux
cgroup/tmpfs/seccomp/Landlock envelope. The capability probe is not a real named-Linux node run.
Therefore this is not `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN`.

## 5. Required decision path

For every submission, the node performs these stages in order:

1. Strictly decode and size-check the JSON payload.
2. Resolve `familyVersion` and `templateId` from the node-owned pinned registry.
3. Verify that `(challengeSha256, epoch)` is active, fresh and unused for that task.
4. Hash the exact raw answer and run the pinned family-specific intake.
5. Execute the actual pinned checker under the pinned toolchain and resource policy.
6. Convert the checker's deterministic result to a node-owned shadow verdict.
7. Atomically consume the challenge and persist node-issued shadow evidence.

Malformed input, unknown identity, stale/replayed challenge, registry drift, checker failure,
policy mismatch, resource-limit breach and semantic rejection are distinct typed outcomes. An OS
containment failure or unavailable checker is not converted to ACCEPT and is not silently treated
as a semantic REJECT; it stops the shadow adjudication with an availability/error result.

The checker executes untrusted submitted code, so the pinned execution policy must also require a
fresh temporary workspace, read-only checker/anchor/toolchain inputs, a sanitized environment with
no inherited secrets, disabled network access, a dedicated process group, bounded input/output and
file counts, and explicit CPU, memory, wall-clock and filesystem limits. It must never compile in
the repository, a shared target directory or a mutable shared dependency cache. Process-tree
termination and temporary-workspace cleanup are containment duties; they cannot change a semantic
verdict.

## 6. Node-issued shadow evidence

Historical success or deterministic rejection records may contain
`boole.native-shadow.evidence.v1`. They remain read-only replay evidence. Every new ACCEPT or
`DeterministicReject` evidence write uses `boole.native-shadow.evidence.v2`, owned by the node.
It binds:

* submission schema and submission digest;
* family/version, template identity and anchor digest;
* challenge digest and epoch;
* exact raw-answer candidate digest;
* intake version;
* checker, checker-policy, node execution-policy and toolchain digests (`policyDigest` keeps the
  checker-policy identity; `executionPolicyDigest` is the node-owned containment-policy identity);
* deterministic verdict and reason code; and
* registry version.

An operational execution identifier and resource telemetry may accompany the evidence, but they
are not part of the deterministic verdict digest or any future BF.3 receipt mapping.

_Clarified 2026-08-22 (see `docs/node-native-shadow-binding-containment-design-v1-correction-r2.md`):
"deterministic rejection" above means the actual pinned checker's own semantic judgment
(decision-path stage 5/6 above), not any rejection reached during stages 1–4. A route may reject a
submission before ever reaching the checker (malformed input, unknown identity, stale challenge,
registry drift, and similar); such a rejection does not produce this evidence object, since no
checker verdict was ever reached._

This object is **shadow evidence, not a consensus receipt and not a share**. It cannot alter
`SharePool`, block construction, rewards, peer state or `mineable_now`. Replaying an already-used
challenge, or presenting task-A submission/evidence as if it belonged to task B, must be rejected
before a second accepted evidence row can be written. Identical raw-answer bytes submitted afresh
for task B are not automatically invalid: the node must bind them to B and let B's actual checker
decide them independently.

## 7. Placement and dependency direction

The implementation lives in a new, isolated `boole-node` module and uses node-owned data types.

Hard dependency rules:

* `boole-node` must not depend on `boole-miner` to import `NativeReceipt`, a checker verdict or a
  task context.
* This slice must not modify `boole-core` admission, replay, hash or block-builder code.
* The native checker adapter may depend only on the smallest tracked checker/runtime surface
  needed to reproduce the judgment.
* The miner-side Entry 27 wiring remains historical evidence; it is not a node backend.

## 8. Activation and non-goals

Production activation is unavailable in v1. The route is default-OFF and, in this slice, reachable
only through loopback or an in-process test harness. A non-loopback bind is a startup/configuration
error. Remote miner transport, authentication, `network_id` binding and a signed submission
envelope belong to a separately approved BF.6 successor; they are deliberately not improvised in
this qualification slice.

This slice does not touch or authorize:

* existing PoW `/submit` admission;
* existing `/receipts` commitment storage;
* `boole-core`, `SharePool`, block or chain state;
* reward, Base, bounty settlement or accounting;
* P2P frames or propagation;
* SP1/ZK proving;
* BF.7 consensus activation; or
* a change to `mineable_now` (it remains 0).

## 9. RED gates and STOP conditions

Implementation must start with failing tests for at least:

1. a valid raw answer reaches the real checker and produces node-owned ACCEPT evidence;
2. a forged miner verdict/receipt cannot bypass checker execution;
3. a preregistered verdict-bearing raw-answer mutation changes the candidate digest and is rejected
   by the actual checker;
4. task-A submission/evidence replayed under task B and cross-challenge evidence reuse are rejected,
   while a fresh identical raw answer for B is independently adjudicated by B's checker;
5. stale/replayed challenge is rejected atomically;
6. checker, checker-policy, node execution-policy, anchor, registry or toolchain digest drift is
   rejected before execution;
7. unavailable/terminated checker never becomes ACCEPT;
8. unknown/oversized/malformed JSON and forbidden fields are rejected;
9. the feature is a byte-for-byte no-op while OFF; and
10. non-loopback exposure is refused; and
11. no accepted shadow verdict changes `SharePool`, block, reward, P2P or consensus state.

Stop without fallback if any of the following is true:

* the node cannot execute the actual checker independently;
* the only available input is a miner-created receipt or verdict;
* the frozen checker cannot be migrated to tracked inputs with digest and verdict parity;
* clean-runner toolchain or fixture reproduction fails;
* the v1 endpoint can bind or listen on a non-loopback address;
* the change requires `boole-core`, SharePool, block, reward, P2P or BF.7 modification; or
* OFF-mode behavior differs from the current node.

## 10. Relationship to BF receipts

`boole.native-shadow.evidence.v1` (legacy replay only) and
`boole.native-shadow.evidence.v2` (all new ACCEPT/`DeterministicReject` evidence writes) are
temporary qualification artifacts, not a third permanent receipt family. Before any production or
BF.7 connection, a separate approved successor must map
the node-owned verdict into the **already-landed** BF.3 common `VerificationReceipt` contract and
prove the mapping preserves every binding field and reject reason required by that contract. This
is a native-adapter qualification and mapping task, not a reimplementation of BF.3 or BF.5.

The promotion rule is:

```
raw submission
  -> node-owned checker verdict
  -> native shadow evidence (this slice)
  -> native verdict mapping into the already-landed BF.3 VerificationReceipt type
  -> BF.6 remote miner commit/reveal (separate approval)
  -> BF.6a package sidecar and availability (separate approval)
  -> RP0-MD and deterministic-resource preconditions
  -> BF.7 consensus use (still HELD)
```

Once the BF.3 mapping is authoritative, shadow evidence may remain as an audit/debug record but
must not become a competing production receipt or an independently rewarded object.

## 11. Completion label

Only the full RED matrix plus one real node-process raw-answer run may earn:

```
NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN
```

That label means node-owned shadow verification works. It does not mean mining is public,
reward-bearing, peer-verified or consensus-active. It does not change
`LLM-MINEABLE-ELIGIBLE-V5 = 14,160` or `mineable_now = 0`.

## 12. Synchronized local planning mirrors

The detailed master, execution and thesis mirrors remain under gitignored `local-docs`; the
repository must not unignore that directory wholesale. Their synchronized 2026-08-23 byte
digests are recorded here so a later local edit cannot be mistaken for this reviewed state:

| local mirror | sha256 |
| --- | --- |
| `local-docs/adr/0021-native-submission-shadow-verification.md` | `6ff953ac229324045b23c283deb702473938ddd4766abe3c9affaa794a964449` |
| `local-docs/todo/todo-l1-network-master.md` | `101319f9688df80b95c4588b6cfcaea140ff282d54fad6d385ad15df56ba7650` |
| `local-docs/todo/EXECUTION-ORDER.md` | `f08f7be9db3ac279b5b0d96cb40fd16d789b1cc2c4e189aea7e9bfbdf82e16be` (updated 2026-08-23 — Phase 2C/current blocker cursor below) |
| `local-docs/verified-reasoning-substrate-thesis-2026-06-10.md` | `a7ff168696308e71caa641fc0dfa83b80d35e4027189918ff9a56a9a98b3e7fb` |
| `local-docs/todo/thesis-realization-roadmap.md` | `fbe62bec291368ecaa1285b55e8223c2841b2e197e9ccfc3948168e56016b2ee` |
| `local-docs/boole-thesis-value-up-verified-zk-encyclopedia-2026-07-21.md` | `b51fad6c2d3c0efb93566ab58f25c566153f018e5ce370cced0b5923d21caac3` |

These digests preserve synchronization evidence only. Runtime authority still requires the
tracked checker/registry migration in section 4; no node may load a `local-docs` file as a trust
root.

The 2026-08-22 update to `local-docs/todo/EXECUTION-ORDER.md` marks its execution-order step 0
complete and moves its current-position marker to node binding/replay RED-matrix design, matching
the section 4.2 closure above; it appends a new dated cursor block rather than editing the prior
one, consistent with that file's own append-only cursor convention. The other five rows remain at
their original 2026-08-21 synchronization point.

A second, same-day (2026-08-22b) update to `local-docs/todo/EXECUTION-ORDER.md` records that
`docs/node-native-shadow-binding-containment-design-v1.md` has frozen the design for the section 4
second prerequisite and moves the current-position marker to "awaiting approval of that design,"
again by appending a new dated cursor block rather than editing the prior one. The other five rows
remain at their original 2026-08-21 synchronization point.

A third, same-day (2026-08-22c) update to `local-docs/todo/EXECUTION-ORDER.md` records that
operator review of that design withheld approval and required six corrections, resolved in
`docs/node-native-shadow-binding-containment-design-v1-correction.md`, and moves the
current-position marker to "awaiting review of the correction document itself," again by appending
a new dated cursor block rather than editing the prior ones. The other five rows remain at their
original 2026-08-21 synchronization point.

A fourth, same-day (2026-08-22d) update to `local-docs/todo/EXECUTION-ORDER.md` records that a
second operator review of that correction document confirmed the original six corrections closed
but found five further contradictions, resolved in
`docs/node-native-shadow-binding-containment-design-v1-correction-r2.md`, and moves the
current-position marker to "awaiting review of the round-2 correction document itself," again by
appending a new dated cursor block rather than editing the prior ones. The other five rows remain
at their original 2026-08-21 synchronization point.

A fifth, same-day (2026-08-22e) update to `local-docs/todo/EXECUTION-ORDER.md` records that a third
operator review confirmed the round-2 correction document's D1 item closed but found five further
gaps, and requested one consolidated implementation reference rather than a further append-only
correction — resolved in
`docs/node-native-shadow-binding-containment-implementation-spec-v1.md`, which restates the full
current rule set in one file. It moves the current-position marker to "awaiting review of the
consolidated spec itself," again by appending a new dated cursor block rather than editing the prior
ones. The other five rows remain at their original 2026-08-21 synchronization point.

The 2026-08-23 synchronization updates all six mirrors with an append-only current-state addendum:
Phase 1 (`131244f`), Phase 2 (`4e19d1e`) and Phase 2C (`eff95658`) are internal data-layer
foundations on `main`, while route/checker execution, same-file-descriptor `flock`, global
`native_busy`, containment and the real node-process run remain open. It also records the named
delegated-cgroup-v2 Linux runner plus concrete UID/GID/privilege model as the Phase 3 GREEN blocker,
and narrows the thesis's Lean claim: domain-native answers are judged by their pinned deterministic
domain checker; Lean remains the final kernel only for claims formalized into the Lean-compatible
lane. The six SHA-256 values above are the byte-exact post-update mirrors.

The later 2026-08-23 implementation addendum in section 4.4 supersedes only that snapshot's
progress cursor: Phase 2D and the route-free Phase 3A.1 same-FD journal foundation are now closed.
The route-free Phase 3A.2 `native_busy` permit is also implemented, while its AppState/route wiring,
containment-backed cleanup, checker wiring, a real named-Linux node run and the full RED matrix remain
open. Phase 3B.0 is the landed typed execution-policy/v2 evidence propagation foundation. Phase 3B.1
closed the named-runner infrastructure-capability prerequisite. The current guarded Phase 3B.2a
freezes the exact disabled policy/toolchain-identity bytes, registry bindings, service-account
resolution, checker invocation and install/IPC contract; production launcher implementation and
provenance testing, authenticated handshake, durable execution ID, route/checker wiring and actual
native Linux execution remain open.
