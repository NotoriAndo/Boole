# Native submission shadow verification v1

Status: **CLOSED-LOCAL QUALIFICATION GREEN — tracked checker, named-Linux containment, loopback
node raw-answer HTTP adjudication and durable replay landed; production activation, P2P, block,
reward and consensus remain forbidden**

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
* Phase 3B.2b-2s — PR #184, main `4a07015`: the pre-lock-composition slice consumes that thread-bound privilege proof,
  opens the three exact installed authority files and resolves the two fixed NSS service accounts,
  in that order and without caller input, into a new opaque thread-bound prerequisite. Failure at
  any stage prevents every later stage. A named Ubuntu gate stages byte-identical root-owned
  authority files through a private read-only `/usr/share` mount and invokes this production
  composition under the exact capability service and fixed accounts. The first hosted-runner
  attempt correctly rejected the runner's unsafe host `/usr/share`; the successor kept the verifier
  strict and passed the real Linux gate plus required CI run `32614207172`. This proof is deliberately
  not launcher readiness: it takes no lock, creates no
  launcher ID, recovers no cgroup, binds no socket, changes no journal/route and spawns no checker.
* Phase 3B.2b-2t — PR #185, main `a517720`: the lifetime-lock slice consumes the opaque pre-lock prerequisite, walks
  only `/run/boole/native-shadow` through `openat` directory descriptors with `O_NOFOLLOW`, requires
  exact root/root ancestor and root:`boole-node` runtime-directory metadata, and opens only
  `launcher.lock` relative to that verified directory. A regular one-link root:`boole-node`
  mode-`0600` inode must pass `fstat` before the nonblocking lifetime `flock`; contention is typed
  busy, guard drop releases only the kernel lock, and no Rust path removes or repairs the inode.
  The named Linux gate proves a separate process is busy while the first guard lives and that
  drop permits reacquisition of the same inode; required CI run `32615499137` is GREEN. It still
  creates no launcher ID, cgroup, listener,
  route, journal transition or checker child.
* Phase 3B.2b-2g — PR #186, main `c1d74f9`: the launcher-instance slice consumes that lifetime-lock guard and accepts
  no random bytes, flags or fallback from its caller. It obtains exactly 32 bytes from one
  `getrandom(2)` call with flags zero; a syscall error or any short read fails without retry and
  without issuing a token. The resulting opaque thread-bound token retains the lifetime lock and
  hides the ID until a later recovery/readiness slice. The named Linux gate proves the real
  syscall path while lock contention remains busy. It does no cgroup recovery, probe, bind/listen,
  route, journal transition or checker work and is not readiness.
* Phase 3B.2b-2h — the tracked deployment-envelope slice adds the exact production systemd unit,
  service-account provisioning input and runtime-directory provisioning input before manager or
  restart-recovery code is written. A named Linux gate validates those tracked bytes with systemd's
  own parsers and proves distinct non-root service identities plus root:`boole-node` mode-`2750`
  runtime ownership in an alternate root. It starts no installed service and implements no manager
  cgroup, orphan recovery, readiness, listener, route, journal transition or checker execution.
* Phase 3B.2b-2m — the manager-cgroup slice consumes the opaque launcher instance, proves the
  process has exactly one thread, opens the fixed systemd service cgroup only by descriptor-relative
  fixed-component traversal on cgroup2fs, creates or safely reuses the exact `manager` child, moves
  the launcher there and verifies root emptiness, exact cpu/memory/pids controller enablement and
  exact post-move PID/TID membership. Reuse also requires root:root mode `0700`, an unfrozen domain
  cgroup, exact-empty `cgroup.subtree_control` and no nested child; the type and empty subtree
  control are rechecked after movement. Failures before movement and fatal failures at or after the
  move attempt are separate typed outcomes. The deployment-envelope gate independently proves the
  production unit bytes; this named Linux gate loads that exact tracked fragment plus one
  gate-owned drop-in that only bind-mounts byte-identical authority under a private,
  read-only `/usr/share`; this preserves the strict authority verifier instead of rewriting the
  hosted runner's unsafe host `/usr/share`. The gate proves create, safe reuse,
  frozen/nested-child rejection, multithread rejection, explicit restart and stop behavior and
  removes only its owned unit, drop-in and authority tree. It does not clean `run-*` leaves or
  claim startup recovery/readiness.
* Phase 3B.2b-2v — the startup-orphan-recovery slice consumes that opaque manager proof and accepts
  no path, operation ID, timeout or policy from its caller. Under one cumulative ten-second deadline
  it inventories every direct service-cgroup child before mutation, accepts only the exact reserved
  `manager` plus canonical `run-<64-lowercase-hex>` leaves, opens and validates every leaf before the
  first freeze, then processes leaves in canonical order through freeze confirmation, whole-cgroup
  kill, `populated=0`, empty PID/TID files and descriptor-relative removal. Leaf identity is checked
  again immediately before removal; a final inventory plus root/manager invariant check is required
  before an opaque recovery proof can be issued. The named Linux gate exercises a live process tree,
  a frozen process tree and an empty leaf, and separately proves that one unexpected child causes the
  exact fatal inventory stage without mutating the valid live leaf. This closes startup `run-*`
  cleanup only. The result is deliberately not readiness and cannot by itself bind/listen, change
  node durable state, route a submission or spawn a checker; the following 2w slice alone may consume
  it for the four fixed compatibility probes.
* Phase 3B.2b-2w — the startup-toolchain-compatibility slice consumes that opaque recovery proof,
  verifies the fixed root-owned Rust/Cargo/Python executable path shapes and runs only the exact four
  tracked manifest probes. Each command gets an independent ten-second deadline, cwd `/`, a cleared
  environment rebuilt from the four tracked entries, null stdin and separate 65,536-byte stdout and
  stderr ceilings. The toolchain directories are exact root:root `0555`; executable metadata is
  frozen to the root-owned, one-hard-link, non-special contract (Python exact `0755`), and each probe
  executes an already-opened descriptor whose identity must match the pre-probe snapshot. A mismatch,
  timeout, overflow, nonzero exit, unexpected stderr, invalid UTF-8, pathname identity change or
  post-probe manager drift is fatal after movement and withholds the opaque compatibility proof. The
  named Linux gate stages the reviewed per-commit Rust artifacts at the exact `/opt` path and proves
  recovery-to-probe ordering. This remains compatibility-only; the three installed-byte provenance
  blockers, readiness, listener, route, node durable mutation and checker execution remain open.
* Phase 3B.2b-2x — PR #191, main `2ef2cc7`: the qualification-startup assembly slice consumes the
  complete opaque 2w proof and no other caller input. It reads the already-verified authority, fixed node/checker identities
  and one launcher instance ID through crate-private forwarding accessors, binds the current
  non-zero launcher PID, and moves the entire compatibility proof chain into the existing
  non-forgeable `VerifiedQualificationStartup`. Retaining that chain keeps the launcher lifetime
  lock, verified cgroup directory descriptors and startup recovery evidence alive rather than
  copying only their numeric outputs and accidentally releasing the safety boundary. The public
  API is type-pinned so no earlier startup token can issue qualification readiness. This is token
  assembly only: no socket is bound, no frame is emitted, `activationAllowed` remains false and
  installed-byte provenance, route, durable node mutation and checker execution remain open.
* Phase 3B.2b-2y — the one-shot fixed-socket qualification slice consumes that complete startup
  token by value, so one verified startup cannot serve a second listener. It accepts no path or
  listener configuration, requires the service's frozen umask, inspects and removes only an exact
  root:`boole-node` mode-`0660` stale socket relative to the retained verified runtime-directory
  descriptor, binds only `/run/boole/native-shadow/launcher.sock`, verifies listener and accepted
  descriptors are close-on-exec, accepts exactly one connection, and removes only the same
  device/inode socket before serving the existing strict handshake. The earlier connected-stream
  helper is no longer public. Linux unit tests cover safe stale replacement and unsafe-entry
  preservation. The named Linux gate starts the reviewed root launcher through the complete
  recovery/toolchain/readiness chain, runs the staged `boole-node` test binary as the non-root
  `boole-node` account against independently installed authority, verifies the real
  nonce/peer/digest/EOF exchange, and requires successful one-shot exit without restart, socket
  absence and service-cgroup removal. This is readiness-only: it accepts no raw answer, mutates no
  journal, starts no checker and leaves `activationAllowed=false`.

There is still no route or checker spawn, no AppState/route use of the `native_busy` primitive, no
containment-backed per-submission cleanup and no native-checker execution under the combined Linux
cgroup/tmpfs/seccomp/Landlock envelope. Phase 3B.2b-2y is a real named-Linux **readiness-only** node
round trip, not the required real node-process raw-answer run.
Therefore this is not `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN`.

### 4.5 Closed-local node-process checker/HTTP closure (2026-08-24)

The paragraph above remains the historical Phase 3B.2b-2y checkpoint. Two later slices close the
authority spec's second prerequisite for the frozen, non-issuable closed-local lane:

* PR #206 (main `8542386`) ran the frozen real accepted answer in the named-Linux contained checker
  service. The tampered and constant controls reached deterministic checker rejection. The gate
  also verified cgroup/rootfs/seccomp/Landlock enforcement, cleanup, and that all three checker
  connections belonged to the same node client process and launcher instance.
* PR #219 (main `4de603f`) added the feature-gated replay node at the fixed loopback endpoint
  `127.0.0.1:8082 POST /native-shadow/submissions`. The node receives a raw answer, resolves the
  frozen grant and registry itself, invokes that contained checker through the qualified launcher,
  and durably records the attempt, evidence and terminal challenge state before replying.

The actual HTTP matrix passed the accepted, tampered, constant and empty-answer cases. Exact
redelivery of the same submission returns the stored terminal result without another checker run;
a different candidate cannot overwrite a consumed challenge; and the process-wide `native_busy`
permit rejects a second concurrent execution. After reservation, an outcome the node cannot prove
terminal is returned as HTTP 504 `adjudication_unknown` with `retryAuthorized=false` rather than
being guessed or retried. A static authority gate proves that this route has no `SharePool`, block,
reward, P2P or consensus consumer.

Client cancellation, terminal redelivery and ambiguous-result fail-closed behavior are covered by
the route's unit contract. A separate process-kill followed by full HTTP-node restart E2E is not
claimed by this milestone. This limitation does not change the completed frozen one-process
node-owned judgment claim, and it must not be silently relabelled as a production availability or
Mac-product claim.

### 4.6 Crash/restart exactly-once closure (2026-08-24 — PR #220 / PR #221)

Section 4.5's limitation line — "a separate process-kill followed by full HTTP-node restart E2E is
not claimed by this milestone" — is superseded append-only: that E2E is now claimed and closed on
Linux CI.

* PR #220 (main `9203156950e178277895ac4d282462147ddae23e`) pinned the launcher security
  invariants restart safety relies on: a restarted launcher grant binds the consumed durable
  attempt read-only so node startup recovery refuses cross-restart reuse, an execution-time
  authority digest mismatch is rejected via the exact `executionPolicyDigestHex` binding, a
  byte-identical ready token minted under an earlier qualification nonce never qualifies a later
  session, and a kernel-reported zero-PID peer is always untrusted.
* PR #221 (main `6553360a6291c300ad0d19c50238b8b7c9263c68`; CI run
  <https://github.com/NotoriAndo/Boole/actions/runs/32709400913>) added a named crash/restart
  phase to the Linux rootfs-replay gate. It SIGKILLs the real node and launcher processes mid-flow
  and restarts them through systemd, verifying process identity actually changed
  (PID/start-time/invocation ID) rather than trusting a soft restart.

Scenario S1 (`terminal-redelivery-across-node-kill`) evidence: 2 checker starts before the kill,
0 checker starts after restart, 3 redelivered responses, accepted redelivery byte-identical,
tampered redelivery differing only in the expected redelivery flag, 10 journal rows, and cleanup
verified including reaping the inert launcher socket inode. Scenario S2
(`unresolved-inflight-fail-closed`) evidence: 0 checker starts, 3 journal rows, restart refused,
the fail-closed message observed, and the listener still refused after a restart attempt.

Together with sections 4.1–4.5 this closes the full MAC.0 chain recorded in
`docs/mac-first-hidden-linux-execution-plan-v1.md` section 9. Scope is unchanged: closed-local,
loopback-only, frozen non-issuable qualification — not public mining, not production availability
and not a Mac-product claim.

### 4.7 Linux/arm64 successor-authority parity milestone (2026-08-25)

PR #224 (main `2a6de07ba6c77355d19a3d342ab718f7358fd76a`; CI
<https://github.com/NotoriAndo/Boole/actions/runs/32766488279>) reproduced the separately frozen
native authority on a native Linux/aarch64 runner. This was executed parity evidence, not an
architecture inference: the accepted fixture and byte-identical replay were accepted; empty,
tampered, constant and both cross-task directions produced the same typed deterministic rejects as
the x86_64 baseline. The arm64 authority also passed the named containment, HTTP replay and real
crash/restart gates.

Linux now has compile-time architecture/authority binding in both directions: an arm64 authority
feature cannot build as a Linux/x86_64 authority, and the default x86_64 authority cannot build on
Linux/aarch64. The executed result records exact semantic, binding, containment and resource-policy
parity, while preserving `productionByteProvenanceComplete=false`, `activationAllowed=false`,
`nonIssuable=true` and all no-consumer boundaries. It is not a Mac VM, app distribution, signing,
notarization, update or production-activation result.

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

### 11.1 Current attainment — closed-local qualification only

```text
NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN
scope = CLOSED-LOCAL / LOOPBACK-ONLY / FROZEN-NONISSUABLE-QUALIFICATION
```

This GREEN means the node process accepts a raw answer, independently runs the actual pinned
checker, and connects the result to durable evidence and terminal challenge state. It does not mean
general production routing, remote-miner access, fresh challenge issuance, public mining, block
admission, reward or consensus activation.

The authority invariants remain `loopbackOnly=true`, `p2pAllowed=false`,
`consensusAllowed=false`, `rewardAllowed=false`, `activationAllowed=false` and
`nonIssuable=true`. `LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged. Block/reward wiring is not a
missing tail of this approved slice: it is a separately authorized stage that this authority
explicitly forbids.

_2026-08-24 addendum:_ the GREEN above now also covers the real process crash/restart
exactly-once E2E and unresolved-`InFlight` fail-closed behavior closed in section 4.6 (PR #220 /
PR #221). Every other limit in this section stands unchanged, and the Mac product gates MAC.2+
remain unimplemented.

## 12. Synchronized local planning mirrors

The detailed master, execution and thesis mirrors remain under gitignored `local-docs`; the
repository must not unignore that directory wholesale. Their synchronized 2026-08-25 byte
digests are recorded here so a later local edit cannot be mistaken for this reviewed state:

| local mirror | sha256 |
| --- | --- |
| `local-docs/adr/0021-native-submission-shadow-verification.md` | `f8680ebbed2b403231478f48f1a8f44f80a4011da714a1e1bd235efa0309288d` |
| `local-docs/todo/todo-l1-network-master.md` | `676625026523d4c48e6e42006402bcf59e36abb76943cde1135a3f1c0d0452c9` (updated 2026-08-31a — A6-v6 is withheld and the event/main-only, fresh-A7-namespace v5 successor is preregistered with authority zero) |
| `local-docs/todo/EXECUTION-ORDER.md` | `3eb001ed7d1e09f6a207e67c1d1a8d9bb32ccff74fb9a715f13ee9fdd7c7a153` (updated 2026-08-31a — execution moves from withdrawn A6-v6 to workflow-dispatch/main-only producer-generation v5 with direct P4 binding) |
| `local-docs/verified-reasoning-substrate-thesis-2026-06-10.md` | `8c520a79bb6a26ef684d866928498fbd9abe456e0a99f072a430033d1ca2a76e` |
| `local-docs/todo/thesis-realization-roadmap.md` | `9c9290bb9a70cae681664408546457db114fe2744c39e73025259aef9c80dbc3` (updated 2026-08-31a — a one-use claim must bind the exact event, reviewed main ref and fresh A7 generation while authority remains zero) |
| `local-docs/boole-thesis-value-up-verified-zk-encyclopedia-2026-07-21.md` | `84d1ba7a50131d0bbd59b52ab01db382b4471a0648b5403a5ee742d185e6bf82` |

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

The 2026-08-24 synchronization supersedes that progress cursor without deleting it. PR #206 and
PR #219 closed the named-Linux contained checker and frozen loopback raw-answer route described in
sections 4.5 and 11.1. All six mirrors now carry the same append-only completion boundary: node-owned
closed-local judgment is GREEN, while process-kill/restart HTTP E2E is not claimed and
block/reward/P2P/consensus remain authority-held. The table above contains the exact post-update
SHA-256 of each local mirror. These hashes are synchronization evidence only; `local-docs` is still
not a runtime trust root.

A second, same-day (2026-08-24b) synchronization appends the MAC.0-complete cursor to three
mirrors (`todo-l1-network-master.md`, `EXECUTION-ORDER.md`, `thesis-realization-roadmap.md`): the
crash/restart exactly-once E2E of section 4.6 (PR #220 / PR #221) closes the MAC.0 chain, the
MAC.1 Mac distribution contract is frozen as MAC.1-PARTIAL — OPERATOR VALUE REQUIRED in
`docs/mac-first-hidden-linux-execution-plan-v1.md` sections 9–10, and the next gate is MAC.2
authority parity, which has not been started. The other three mirrors are byte-unchanged. The
table above holds the exact recomputed post-update SHA-256 of every mirror, replacing the
2026-08-24a values for the three edited files only.

A third, same-day (2026-08-24c) synchronization appends the MAC.1-closure cursor to the same
three mirrors (`todo-l1-network-master.md`, `EXECUTION-ORDER.md`,
`thesis-realization-roadmap.md`): the operator supplied the single outstanding MAC.1 value
(product minimum macOS 14.0, Apple Silicon M1 or later, Intel outside the v1 scope), MAC.1 is
COMPLETE per plan section 10.5 with the frozen contract unchanged, and the cursor moves to
MAC.2 — Linux/arm64 successor authority parity — which has not been started. The other three
mirrors are byte-unchanged. The table above holds the exact recomputed post-update SHA-256 of
every mirror, replacing the 2026-08-24b values for the three edited files only.

## 13. Mac-first product distribution boundary

The current Linux-only execution contract describes the security envelope for untrusted native
submissions; it is not a requirement that a Mac user install or administer Linux. The reviewed
product successor is `docs/mac-first-hidden-linux-execution-plan-v1.md`.

That plan fixes the user contract as one signed/notarized `Boole.app`, with no separately installed
Docker Desktop, Linux distribution, VM manager, Rust/Python toolchain or terminal-managed service.
The app will keep node-owned challenge/journal/evidence state on the trusted host side and manage a
user-invisible Linux execution appliance for only the untrusted compiler/process tree.

This registration does not alter this authority's non-Linux fail-closed rule or authorize a Mac
production claim. The current Linux/x86_64 execution authority cannot be silently reused as an
Apple-Silicon Linux/arm64 authority. Architecture parity, guest lifecycle, authenticated host/guest
transport, clean-Mac installation and signed update/rollback each remain explicit MAC.1–MAC.6
gates. Their implementation follows this document's exact Linux ACCEPT closure and node raw-answer
exactly-once E2E; it does not interrupt or bypass those prerequisites.

_2026-08-24 addendum:_ MAC.0 is COMPLETE (section 4.6 crash/restart closure; plan section 9) and
the MAC.1 distribution contract is frozen as MAC.1-PARTIAL — OPERATOR VALUE REQUIRED (plan
section 10). MAC.2 and every later Mac gate remain unstarted and unimplemented; this addendum
still authorizes no Mac production claim.

_2026-08-24b addendum:_ the operator supplied the single outstanding MAC.1 value, closing it in
plan section 10.5: MAC.1 is COMPLETE with product minimum **macOS 14.0 (Sonoma)** on
**Apple Silicon (M1 or later)**; Intel Macs are outside the v1 support scope. The frozen
one-install/resource/rollback contract of plan sections 10.1–10.3 is unchanged. The execution
cursor is now MAC.2 — Linux/arm64 successor guest authority reproduction and exact verdict
parity — and MAC.2 has NOT been started. The Mac production checker is still NOT ready; this
addendum grants no implementation, activation, block or reward authority and authorizes no Mac
production claim.

_2026-08-25 current-state addendum:_ plan sections 8.3 and 10.6 preserve the historical MAC.1
operator decision but correct its completion accounting. The current state is
**MAC.1-PARTIAL — DISTRIBUTION MODE, PUBLIC IDENTITY, AND MEASUREMENT PROTOCOL REQUIRED**;
**MAC.2-PARTIAL — CLOSED-LOCAL LINUX/ARM64 AUTHORITY PARITY COMPLETE; STAGED VERIFIER AND
POST-ADOPTION REVERIFICATION OPEN** by section 4.7; and
**MAC.3 BLOCKED / NOT STARTED**. MAC.4–MAC.6 remain unstarted. The Mac production checker is
still not ready and no activation, block or reward authority is granted.

The 2026-08-25 mirror synchronization appends that same current cursor to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`: MAC.0 is
complete, MAC.1 remains partial for distribution mode/public identities/measurement protocol,
MAC.2 has completed only its closed-local Linux/arm64 parity subgate while its authenticated staged
verifier and post-adoption reverification remain open, MAC.3 is blocked and later Mac gates
remain unstarted. The other three mirror files are byte-unchanged. The section 12 table contains
the recomputed post-update SHA-256 values for the three edited mirrors only. Local mirrors remain
synchronization evidence, not runtime trust roots.

_2026-08-25 operator-decision addendum:_ the current Mac decision boundary is
**MAC.1-DECISIONS-FROZEN — TEAM-ID-AND-PRODUCTION-TRUST-ROOT-OPEN**. The approved product path is a
small directly distributed Developer-ID `Boole.app` (`io.github.NotoriAndo.Boole`) that obtains an
exactly verified Linux/arm64 guest from GitHub Releases on first run. The Apple Team ID, production
certificate/notarization identity, production guest-update public trust root and its rotation and
revocation rules do not exist in the repository and remain open. A non-production KAT key may
exercise the offline MAC.2-B verifier core, but it cannot authorize an update or close MAC.2-B.
MAC.3 remains blocked; all activation, block, reward and BF.7 invariants remain unchanged.

_2026-08-25b implementation addendum:_ **MAC.2-B-CORE/KAT GREEN** is now backed by PR #226,
main `fb7142d21129852847ff1ab6c19ca3deb9713692`, and CI runs
<https://github.com/NotoriAndo/Boole/actions/runs/32790547865> and
<https://github.com/NotoriAndo/Boole/actions/runs/32790547760>. The portable `boole-core` verifier
authenticates a canonical, domain-separated Linux/arm64 manifest with an injected Ed25519 public
key, verifies all ten fixed authority roles by exact size and streaming SHA-256 under the 2 GiB
ceiling, retains verified descriptors, and rejects malformed keys/manifests, wrong identity or
target, missing/tampered bytes, rollback, replay, predecessor mismatch and counter overflow. The
focused verifier suite passed 23/23 and the existing signing/canonicalization regressions remained
green.

This closes only the verifier core and non-production known-answer tests. The production trust
root, private signing custody, first signed release manifest, initial sequence pin, downloader,
durable update floor, same-file-descriptor atomic adoption, VM lifecycle and post-adoption verdict
reverification remain absent. Consequently **MAC.2-B production OPEN**, **MAC.2-C OPEN** and
**MAC.3 BLOCKED** are the current authority boundary. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and `activationAllowed=false` are unchanged.

The 2026-08-25c mirror synchronization appends this same boundary to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`: the
offline verifier core/KAT is GREEN, production identity and durable adoption remain open, and
MAC.3 remains blocked. The other three section 12 mirrors are byte-unchanged. The section 12
table contains the recomputed SHA-256 values of all three edited local mirrors; those digests are
synchronization evidence only and do not make `local-docs` a runtime trust root.

_2026-08-25d product-form correction:_ **CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT** now
supersedes the prior GUI `Boole.app`, Developer ID and Bundle/Team-ID product-form decision. The
consumer contract is one reviewed curl command that installs a verified prebuilt macOS arm64 CLI,
an internal host controller and a versioned Linux/arm64 guest. Team ID is not a runtime authority
and is not a prerequisite for this curl-first implementation. Apple signing/notarization may be
added later as distribution hardening, while a clean supported Mac must independently prove that
the chosen Team-ID-free code signature carries the Virtualization entitlement and runs the hidden
guest without a developer toolchain.

The correction changes packaging, not verifier authority. MAC.0, MAC.2-A and
**MAC.2-B-CORE/KAT GREEN** remain valid. The production Ed25519 update trust root, signed release
manifest, durable staging/adoption and post-adoption verification remain open, so MAC.2-B
production and MAC.3-CLI remain blocked. The existing `install.sh` is a source/developer bootstrap,
not the immutable product installer. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged.

The 2026-08-25d mirror synchronization appends this corrected product form to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other
three section 12 mirrors are byte-unchanged. The section 12 table contains the recomputed SHA-256
values for the three edited mirrors; these values are synchronization evidence only, never runtime
trust roots.

_2026-08-25e implementation addendum:_ **CURL.1 CONTRACT/VERIFIER GREEN** — the immutable
curl-first product release contract `boole.curl-product-release.v1` and its offline verifier are
implemented in `boole-core` (execution-plan section 15). One signed canonical manifest, in the
dedicated `boole-curl-product-release-v1` Ed25519 domain, pins six fixed artifact roles (four
host binaries under the 512 MiB total host-payload cap plus the embedded guest update
manifest/signature pair bound by exact bytes), the macOS 14.0 arm64 target, the controller
protocol version, a replay/rollback floor, and rejects transport (`downloadUrl`) and Apple
(`teamId`) identity from the authority surface. Verified artifacts keep their open file handles,
so post-verification path replacement cannot swap verified bytes. The guest boot format is
frozen as direct Linux boot (`VZLinuxBootLoader`, uncompressed arm64 `Image` + initrd +
read-only root disk, `bootFormatVersion=1`); EFI is rejected and the current OCI rootfs content
remains NOT a bootable VM image. 37 new focused contract tests went RED→GREEN and the 23
existing guest staged-update verifier tests stayed green: the shared low-level envelope rules
moved into one internal `boole-core` helper without changing the guest verifier's messages or
semantics.

This closes only the release contract and its offline verifier under a non-production KAT key.
The product installer v2, any real release artifact, the production trust root and signing
custody, the clean-Mac Team-ID-free entitlement canary, durable adoption and the VM lifecycle
remain absent, so **CURL.2/CURL.3 NOT STARTED**, **MAC.2-B production OPEN** and
**MAC.3-CLI BLOCKED** are the current boundary. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and `activationAllowed=false` are unchanged.

The 2026-08-25e mirror synchronization appends this same state to `todo-l1-network-master.md`,
`EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other three section 12 mirrors are
byte-unchanged. The section 12 table contains the recomputed SHA-256 values for the three edited
mirrors; these values are synchronization evidence only, never runtime trust roots.

_2026-08-25f implementation addendum:_ **CURL.2-CORE INSTALLER CORE GREEN** — the verified
local installer core for the curl-first product is implemented in
`crates/boole-core/src/curl_product_install.rs` (execution-plan section 16). It consumes the
frozen CURL.1 verifier end to end before the install root is mutated in any way, copies
verified bytes from the verifier's retained file handles (post-verification source swaps
cannot change what is adopted), stages into a transient tree with per-file fsync, adopts a
version directory with a single rename and replaces the durable
`boole.curl-product-install-state.v1` record (`installed-release.json`) via a fsynced
temp-file rename. That record is the sole replay/rollback floor for the next install; a
corrupt, non-canonical, unknown-field, wrong-schema, zero-sequence or malformed-digest record
fails closed with the evidence preserved and is never silently replaced by the first-install
floor. Earlier version directories are retained as rollback material together with the exact
manifest/signature bytes; crash residue in staging or an orphan version directory is replaced,
never trusted. 19 focused installer tests went RED→GREEN and the 37 CURL.1 contract tests
stayed green.

This closes only the local adoption core under a non-production KAT key. Download/transport
(the curl entrypoint that drives this core), any real release artifact, the production trust
root and signing custody, the entitlement canary and the VM lifecycle remain absent, so
**CURL.2-TRANSPORT/CURL.3 NOT STARTED**, **MAC.2-B production OPEN** and **MAC.3-CLI BLOCKED**
are the current boundary. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base
activation `false` and `activationAllowed=false` are unchanged.

The 2026-08-25f mirror synchronization appends this same state to `todo-l1-network-master.md`,
`EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other three section 12 mirrors
are byte-unchanged. The section 12 table contains the recomputed SHA-256 values for the three
edited mirrors; these values are synchronization evidence only, never runtime trust roots.

_2026-08-25g implementation addendum:_ **CURL.2-TRANSPORT GREEN** — the fail-closed bundle
download/staging transport and the `boole product install` curl entrypoint are implemented in
`crates/boole-cli/src/curl_product_transport.rs` and the `boole-cli` binary (execution-plan
section 17). Transport is never trust: the URL, HTTP status, server headers and file names
carry no authority. The frozen download order is: validate the URL shape and staging layout;
read the durable install state (a corrupt `installed-release.json` aborts before any network
request); fetch the manifest and detached signature into memory under the frozen contract
caps, now public as
`MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES`/`MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES`;
authenticate against the injected trust root and replay floor (a forged or replayed bundle
aborts before any artifact request); download exactly the signed artifact set, each stream
bounded by the signed `byteLength` via the new authenticated-stage `artifact_byte_length`
accessor rather than `Content-Length`, into a transient staging directory that is never the
install tree; drive the CURL.2-CORE installer, which re-verifies everything and adopts
atomically; and remove the staging directory on every outcome. The transport reuses the
workspace-pinned `reqwest`, adding no new supply-chain crate, and `boole-core` stays
network-free. 18 focused tests went RED→GREEN against a loopback server with a non-production
KAT key (zero-request corrupt-state abort, two-request forged/replay abort, tampered bytes
behind HTTP 200 rejected, over-long/truncated streams rejected, staging residue replaced,
non-http(s) URLs and staging-inside-root rejected, envelope success/failure surfaces), and
the CURL.1 suite grew to 38.

This closes only the closed-local transport under a KAT key and loopback HTTP. Any real
release artifact, the production trust root and signing custody, release upload, the
entitlement canary and the VM lifecycle remain absent, so **CURL.3 NOT STARTED**, **MAC.2-B
production OPEN** and **MAC.3-CLI BLOCKED** are the current boundary. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` are unchanged.

The 2026-08-25g mirror synchronization appends this same state to `todo-l1-network-master.md`,
`EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other three section 12 mirrors
are byte-unchanged. The section 12 table contains the recomputed SHA-256 values for the three
edited mirrors; these values are synchronization evidence only, never runtime trust roots.

_2026-08-25h implementation addendum:_ **CURL.3-PREP CONTRACT FROZEN — CANARY NOT RUN.** The
acceptance grounds of the clean-Mac, Team-ID-free virtualization-entitlement canary are now a
machine-checked contract rather than prose. `crates/boole-core/src/curl_virtualization_canary.rs`
evaluates a canary report in a frozen fail-closed order: macOS 14.0 floor (pinned to the same
constant as the product minimum so the two cannot drift), Apple Silicon only, clean-machine
grounds, Team-ID-free ad-hoc signing form, the `com.apple.security.virtualization` entitlement,
entitled-and-isolated execution only, `VZLinuxBootLoader` direct kernel boot, the exact
`guest-kernel`/`guest-initrd`/`guest-root-disk` pin set reused byte-identically across the
reboot, the exact boot/shutdown/reboot sequence, and a zero-residue scan after every boundary.
The central rule is enforced structurally: **a developer machine can never be recorded as a
clean-Mac pass**, because the machine grounds are checked before any success signal is read.
23 closed-local tests pin this contract, and `scripts/curl-virtualization-entitlement-probe.sh`
gives the operator a reproducible entitlement A/B check that uses no Apple identity and boots
no guest.

On the development Mac (macOS 26.5.2, Apple M4 Max) that probe passed — the ad-hoc signature
carried no Team ID, the entitled binary reached `VZVirtualMachine` instantiation and the
unentitled binary was refused by `VZErrorDomain Code=2`. That observation is recorded as
developer-machine evidence and is explicitly **not** a CURL.3 pass: the host is not clean, not
the frozen floor version, no guest booted and no lifecycle or residue boundary was exercised.
No clean supported Mac was available, so the canary was not run and **CURL.3 stays NOT
STARTED**, alongside **MAC.2-B production OPEN** and **MAC.3-CLI BLOCKED**. No Apple identity,
certificate, provisioning profile, production key, release build, upload or installation was
created or used. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base
activation `false` and `activationAllowed=false` are unchanged.

The 2026-08-25h mirror synchronization appends this same state to `todo-l1-network-master.md`,
`EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other three section 12 mirrors
are byte-unchanged. The section 12 table contains the recomputed SHA-256 values for the three
edited mirrors; these values are synchronization evidence only, never runtime trust roots.

_2026-08-26 implementation addendum:_ **CURL.3 DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT
PASSED; BOOTABLE GUEST CONTRACT V2 GREEN.** The clean-Mac canary remains a mandatory release
qualification gate, but it is no longer a circular prerequisite for building the host controller,
boot artifacts and closed-local VM lifecycle that the canary itself needs. Development on the
current Mac may continue only as non-qualifying closed-local evidence. CURL.3 is still required
before MAC.5 clean-install acceptance, MAC.6 release readiness, any Mac production-support claim,
production trust-root distribution or activation.

The implementation also closes the previously inconsistent artifact boundary. Frozen guest-update
v1 remains exact-ten with `guest-rootfs`. Successor guest-update v2 has a separate schema/signing
domain, requires `bootFormatVersion=1`, replaces that role with the shared `guest-kernel`,
`guest-initrd` and `guest-root-disk` vocabulary and authenticates exact-twelve artifacts under the
same 2 GiB cap. Product-release v2 likewise has a separate schema/signing domain and rejects an
embedded guest manifest unless it carries the exact bootable v2 role set. Product-release v1 and
all existing v1 tests remain unchanged.

This is a contract slice, not a boot or release. CURL.2 installer/transport still consume v1; no
v2 bundle, real Linux/arm64 kernel, initrd, bootable root disk, minimal PID 1, VM lifecycle,
production key or release exists. The next execution cursor is the deterministic boot-artifact
builder. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false`
and `activationAllowed=false` remain unchanged.

The 2026-08-26 mirror synchronization appends this corrected state to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other
three section 12 mirrors are byte-unchanged. The section 12 table contains the recomputed SHA-256
values for the edited local mirrors; these values are synchronization evidence only, never runtime
trust roots.

_2026-08-26b implementation addendum (corrected before merge):_ **BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 GREEN;
CURRENT INPUT READINESS BLOCKED_MISSING_INPUTS; REAL BOOT ARTIFACTS NOT PRODUCED.** The
audit-only `scripts/native_shadow_boot_artifact_builder_arm64_v1.py` now validates the exact
Linux/arm64 rootfs source lock (62 artifacts, 181,623,999 bytes), content-addressed inputs and
the exact existing ARM64 execution policy without network access or output creation. That policy
requires Linux/aarch64, systemd, `boole-native-shadow-launcher.service` and its fixed cgroup. The
scaffold keeps the kernel, systemd guest closure and initrd/ext4 image-builder toolchain as three
unresolved authorities. This v1 scaffold/schema/tool is permanently audit-only and requires all
three fields to stay null; future populated pins require a separately reviewed successor
plan/schema/tool. Every result in this slice stays `BLOCKED_MISSING_INPUTS` with `artifactsWritten=0`,
`bootableClaim=false` and `activationAllowed=false`.

The current closed-local cache contained all 62 frozen source artifacts when audited, but this is
ephemeral availability evidence rather than a release input. No kernel, initrd, root disk, v2
product installation or VM boot exists. The next cursor is
`BOOT-GUEST-INIT-COMPATIBILITY-V1`, which must first freeze the self-contained systemd/unit/runtime
and cgroup-compatible guest closure. `BOOT-INPUT-AUTHORITY-V1` remains blocked until that contract
exists, and only then may it freeze exact kernel, guest-closure and builder bytes. The discarded
static-PID-1 shortcut contradicted the already-frozen systemd policy and is not an allowed path.
CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED` and remains mandatory before MAC.5/MAC.6 or
any production-support claim. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` are unchanged.

_2026-08-26c guest-init compatibility addendum:_ **BOOT-GUEST-INIT-COMPATIBILITY-V1 CONTRACT
GREEN; CURRENT ARM64 SOURCE LOCK BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS.** The frozen compatibility
authority requires real systemd PID 1, the exact launcher unit/sysusers/tmpfiles inputs, an ARM64
launcher binary, explicit unit enablement, a read-only root disk with named ephemeral runtime
mounts, cgroup v2 `cpu`/`memory`/`pids` delegation and the kernel facilities already required by
the ARM64 execution policy. The current 62-artifact source lock lacks exactly seven source-shape
requirements: systemd package seed, launcher binary, launcher unit, sysusers config, tmpfiles config,
empty machine-id and launcher enablement. The audit result therefore writes no artifacts and makes
no boot claim.

The Mac host node remains the durable journal authority. Explicit replay-node paths are rejected,
but source shape alone does not prove the absence of every renamed or package-embedded authority;
authenticated host/guest transport and runtime boundary verification remain deferred to MAC.3.
The current OCI rootfs, a host systemd syntax gate, a fake `/bin/true` service or a static PID 1
cannot satisfy this contract. The next cursor is `BOOT-INPUT-AUTHORITY-V1`, which may pin a
separately resolved successor source lock and exact kernel/image-builder/launcher bytes only after
the auditor reports `SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED`. That result keeps signed
closure, runtime compatibility and authority-boundary verification false; later gates must verify
all three independently. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED` and remains mandatory before MAC.5/MAC.6 or any
production-support claim. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base
activation `false` and `activationAllowed=false` are unchanged.

_2026-08-26e dependency-candidate addendum:_ **BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1 =
FROZEN-NOT-BOOT-AUTHORITY.** The canonical plan (SHA-256
`f5465cd62b8b96f2e5b1702e72d2be2b2d73d3924968974604697a4a1614681f`) and result (SHA-256
`a8329d35b480e6b40823e8823551c16eb71a0d7bbc1da40483f05b4535815f26`) replay the already-cached,
signed Ubuntu Noble ARM64 repository metadata and deterministically select 191 package rows with
208,936,876 declared payload bytes. The prior 56-row / 66,992,762-byte baseline is an exact-row
subset, leaving a fixed 135-row / 141,944,114-byte successor delta.

Signed repository metadata replay is verified; package payload acquisition and verification are
not. The candidate is not a source lock, boot authority, image, VM-boot result, production
authority or activation authority. No maintainer script ran; no uncompressed kernel `Image`, ARM64
launcher ELF, initrd or root disk was produced; runtime compatibility and production byte
provenance remain false; `bootArtifactsWritten=0`, `bootableClaim=false` and
`activationAllowed=false`. The generator executes resolver/acquirer/builder logic only from exact
pinned source bytes rather than an import cache, and the tracked result contains no local cache
path. A future networked payload-acquisition slice requires separate approval and may only fetch
and digest-check the fixed rows; it cannot silently install packages or earn boot authority.

CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`, is neither passed nor waived,
and remains mandatory before MAC.5/MAC.6, release readiness or activation. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` remain unchanged.

The 2026-08-26e mirror synchronization appends this same candidate-only boundary to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other
three section 12 mirrors are byte-unchanged. The section 12 table contains the recomputed SHA-256
values for the three edited local mirrors; these values are synchronization evidence only, never
runtime trust roots.

_2026-08-26f package-payload addendum:_ **BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1 =
PACKAGE-PAYLOADS-ACQUIRED-VERIFIED-NOT-BOOT-AUTHORITY.** A pre-registered plan (SHA-256
`43becf01889f8ca5b4fc9acff20b95b12ef78f3736dd13c9081001c5110aac2a`) fetched the missing
ARM64 `Packages.xz`, replayed the signed Ubuntu snapshot and required byte equality with the
tracked 191-row candidate before opening a package URL. It then fetched baseline 51 and verified
56/56 before fetching delta 134 and verifying 191/191. Six exact package CAS hits were reused with
zero requests. The 186 network responses totaled 209,807,900 bytes. The canonical result SHA-256
is `cb4d6bc0f85d2dead1fbae20d9dcebcc3310e734d9a2d1937855997ae22b61ea`.

The package files remain opaque bytes. No `apt`/`dpkg`, extraction, maintainer script, ARM64 Rust
distribution, launcher ELF, kernel extraction, image builder, initrd/root disk or VM boot ran.
Only package acquisition/verification are true; runtime compatibility, production provenance,
bootability and activation remain false. CURL.3 remains deferred and unpassed, and remains a
mandatory release gate. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD` and Base
activation `false` are unchanged.

The 2026-08-26f mirror synchronization appends this same payload-only boundary to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other
three section 12 mirrors are byte-unchanged. The section 12 table contains the recomputed SHA-256
values for the three edited local mirrors; these values are synchronization evidence only, never
runtime trust roots.

_2026-08-26g source-lock addendum:_ **BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1 = SEALED, SOURCE SHAPE
ONLY, LAUNCHER BINARY DEFERRED, NOT BOOT AUTHORITY.** A plan frozen before any result (SHA-256
`c047c20144167a4f28f222c4026a33e2d70b89340ee13cba79c207b7c92dc583`) fixed the acceptance grounds,
and the successor lock (SHA-256
`9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf`, 357,104 bytes) seals 197
artifacts, 191 verified package rows over 208,936,876 payload bytes, 7 seed packages including
`systemd`, 10 tracked files, 8 derived entries and 10 authority bindings. Thirty-four acceptance
tests refuse a dropped, extra, reordered, tampered, foreign-snapshot or borrowed package row, a
missing init seed, missing launcher/sysusers/tmpfiles/machine-id deployment bytes, a missing or
misdirected enablement symlink, a replay-node unit inside the guest, permitted maintainer scripts,
permitted build-time network and `activationAllowed=true`.

Six of the guest-init contract's seven requirements are closed. `tracked-file:launcher-binary`
stays deferred: the guest launcher ELF is a build output of the ARM64 launcher build authority,
which has not run, and a digest cannot be stated for a file that does not exist. The audit
therefore reads `BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS` with exactly one missing role rather
than a clean verdict, and the deferral is recorded with its cause instead of being hidden.

No package was extracted, no maintainer script ran, no ARM64 Rust distribution, launcher ELF,
kernel, initrd, root disk or image builder was produced, and no VM booted:
sealing a source lock is not a boot claim.
CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`, neither
passed nor waived. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD` and Base
activation `false` are unchanged.

_2026-08-26h rustdist-acquisition addendum:_ **BOOT-RUSTDIST-ACQUISITION-ARM64-V1 = PAYLOADS
ACQUIRED AND VERIFIED, NOT A TOOLCHAIN AUTHORITY.** The three
`aarch64-unknown-linux-gnu` Rust archives were pre-registered before a single byte was fetched
(plan SHA-256 `8ee39ab4c828c31bdd82bf8da12546d9b6595aeac8e6e9f4da9899eaacf0accc`), and the
pre-registration commit was pushed to its remote branch before the download ran. Every URL, size
and SHA-256 was carried unchanged from the already merged runtime acquisition plan (SHA-256
`3f78559e29fbdb5b417eeb316687a4347907e4624b75da057c3f255e810ff35d`); no address or digest was
introduced here. The plan also pinned the sealed boot rootfs source lock and recorded the
content-addressed store state observed before the download: all three artifacts absent,
112,995,148 bytes to fetch.

The acquirer reuses the merged payload acquirer's nofollow, atomic-link and write-once primitives
without modifying that module, adding only a host-parameterized stream that refuses redirects,
environment proxies, retries, range requests, concurrency, non-allowlisted hosts, content
encodings, a non-200 status, a `Content-Length` mismatch and any byte count differing from the
frozen size. A store hit issues no network request; the 191 previously verified package payloads
were not re-fetched. The result (SHA-256
`284b8da0a94bedb6bae8edf2f77daaeff42d2951e981cf388ed65bab0e9afa1b`) records 3 fetched, 3 verified,
0 store hits and 112,995,148 bytes, 5.3% of the 2 GiB acquisition ceiling. Re-hashing the three
stored objects with an independent tool reproduces the frozen digests exactly; each is mode
`0600`, link count 1, owned by the invoking uid, and the store grew from 250 to 253 entries with
no partial or temporary file left behind.

No archive was extracted and no toolchain was installed, so nothing here establishes that these
binaries run anywhere: `runtimeCompatibilityVerified` stays false alongside `toolchainInstalled`,
`launcherElfBuilt`, `reproducibleBuildProven`, `kernelImageExtracted`,
`imageBuilderAuthorityPresent` and `bootAuthority`.
Acquiring verified bytes is not an installed toolchain,
and it is not a launcher, kernel, root disk or boot claim. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

_2026-08-26i launcher-build addendum:_ **NATIVE-SHADOW-LAUNCHER-BUILD-ARM64-V1 = BUILD INPUTS
FROZEN AND ENFORCED, ARTIFACT NOT YET SEALED.** The rootfs source lock defers exactly one role,
`tracked-file:launcher-binary`, because a digest cannot be stated for a file that does not exist
yet. The build authority (SHA-256
`64f4ea0c6b574e1479e51a78e250da8fac6f3d3522d60cb03dde65b53da594ee`) fixes every input that decides
those bytes: 33 pinned source files with digests and sizes, the workspace manifests and lockfile,
the target triple `aarch64-unknown-linux-gnu`, the release profile with `panic = "abort"` and
overflow checks, the exact `cargo build --locked --offline` argument vector, the linker selection,
and the tool that generated the document. Only `cargo fetch` may reach the network, and it runs
before either build so `--offline` holds while code is compiled.

Two inputs are named without being frozen, and the document says so rather than implying
otherwise. The linker is whatever `cc` driver the runner image ships, so
`build.linker.byteProvenanceClosed` is false; a build authority that omitted the linker entirely
would look tidier and claim more than it knows.

The build toolchain named here is the *workspace* channel `1.95.0` declared by
`rust-toolchain.toml`, not the `rust-lang-ci` nightly acquired above. Those are two different
toolchains with two different jobs: the nightly compiles submitted proof projects inside the guest,
while the launcher is an ordinary workspace crate. The authority says so explicitly, and
`toolchainByteProvenanceClosed` stays false because the workspace toolchain is installed by a
commit-pinned action rather than unpacked from bytes this project froze.

Determinism is declared, never manufactured. Each build runs in its own temporary directory from a
`git archive` of tracked files only, and the pinned digests are re-verified *inside* the exported
tree so a dirty working file cannot slip past a check made somewhere else. `--remap-path-prefix`
is written into the authority in the open; `SOURCE_DATE_EPOCH` is deliberately left unset and a
test asserts its absence. If the two artifacts differ, the difference is reported --
`mismatchAction` is `report-the-difference-never-force-a-match`.

The double build cannot run on the development Mac, whose target is not Linux/arm64, so it runs as
the named non-skippable CI job `native-shadow-launcher-build-arm64` on an arm64 runner, and the
required `self-test` check depends on its success. The first run discovers the launcher digest and
seals it; every later run must reproduce those exact bytes and may never rewrite the seal to agree
with a newer build. Until that sealed result is committed, `tracked-file:launcher-binary` stays
deferred, and every boundary the authority declares stays false: `toolchainByteProvenanceClosed`,
`guestImageBuilt`, `kernelImageExtracted`, `launcherDeployedIntoGuest`,
`imageBuilderAuthorityPresent`, `runtimeCompatibilityVerified` and `bootAuthority`.
A byte-identical pair of builds is not a boot,
and freezing build inputs is not an image, a kernel or a running guest. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

_2026-08-26j image-builder addendum:_ **NATIVE-SHADOW-BOOT-IMAGE-BUILDER-AUTHORITY-ARM64-V1 =
IMAGE BUILDER INPUTS FROZEN, NO IMAGE BUILT.** One step between the sealed stages had no owner: the
rootfs stage produces an OCI image layout, and the boot stage consumes a kernel, an initrd and a
root disk, but nothing stated which tools turn the first into the second. The authority (SHA-256
`59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e`, 4,714 bytes) fixes that step's
inputs and its input/output roles, and does not build anything.

No tool is taken from `PATH`. `mke2fs` and `debugfs` are pinned as named members of `e2fsprogs`
1.47.0-2.4~exp1ubuntu4 (package SHA-256
`6e1cdd65bf58fe77968f8ac45f1802586baf18bfb8541f4a88fe843ab85bef8b`), with their own member
digests, sizes and mode `0755`; the kernel is a named member of `linux-image-6.8.0-31-generic`
6.8.0-31.31 (package SHA-256
`7d0168a6ea0ebc7263fed3cce3dc45153ddd4e0e4a4b2737f74f0bd72cbaa292`). Every one of those packages
was already inside the sealed source lock, so this addendum introduces no new address, no new
digest and no download; the 191 previously verified payloads were not re-fetched. Maintainer
scripts are never run, versions are never resolved to "latest", and no production signing material
-- key, Team ID, certificate or provisioning profile -- appears anywhere in the document, which a
test enforces by substring.

`mkfs.ext4` is a symlink to `mke2fs`, so the role pins `mke2fs` itself. Pinning the symlink would
let an upstream rename repoint the tool while the pinned path kept matching, which is why
`forbidSymlinkToolPins` is part of the contract rather than a habit.

The kernel ships gzip-compressed inside its package, and Apple's `VZLinuxBootLoader` wants a raw
arm64 `Image`, so both forms are recorded: compressed
`f67ad535a1b19295985d0266394d1c3a5620178a3ba61aca22cda1b6c1e27a2a` (18,199,471 bytes) and expanded
`d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336` (57,860,488 bytes), the latter
carrying the arm64 magic `ARM\x64` at offset 0x38. Declaring the decompression step now is cheaper
than discovering it when a boot fails.

Determinism is declared before anything is built, because two of its inputs are drawn from host
entropy by default: `mke2fs` would pick a filesystem UUID and a directory hash seed per run, and
either alone is enough to make two builds of identical inputs differ. Both are fixed to stated
synthetic constants. The initrd is uncompressed `cpio-newc` so no gzip implementation difference
can enter, file order is sorted by logical path bytes, ownership is `root:root` only, mtimes are
canonicalised to 0, `machine-id` is an empty file for first boot, `SOURCE_DATE_EPOCH` is left
unset, and `forbidTimestampSuppression` records that hiding a differing value to force a match is
not permitted. Two independent builds are required and `mismatchAction` stays
`report-the-difference-never-force-a-match`.

Verification is split by what each environment can actually see, and the document says which half
is which. The source lock is tracked, so CI re-hashes it and proves every pinned package, digest
and version is one the lock already froze. The content-addressed store is gitignored, so extracting
the pinned members from those packages and re-hashing the tool bytes is a local-only check; it
passed here for both tool binaries and both kernel forms, and `toolByteProvenanceVerifiedInCi`
stays false because a check CI cannot run must not be recorded as one it did.

This is a successor input, not an edit. The sealed scaffold plan named exactly
`initrd-ext4-builder-authority-v1` for its `imageBuilderToolchain` slot and left the digest null;
the scaffold file is untouched, and the preflight tool refuses any non-null value there by design.
Earlier sealed documents record `imageBuilderAuthorityPresent=false` as the state at their own
sealing and are not rewritten to agree with this one.

One process deviation is recorded rather than smoothed over: the tool, the document and the tests
were written before a failing test existed, so the RED-first order this repository requires was not
followed for this slice. The remedy applied was a mutation harness that weakens one guard at a time
and requires the test module to fail; all eight mutations were caught and the tool was restored
byte-for-byte. That is evidence the guards are load-bearing, not a substitute for the order, and
the next slice starts from RED.

Every boundary this authority declares stays false: `guestImageBuilt`, `initrdBuilt`,
`kernelImageExtracted`, `rootDiskBuilt`, `runtimeCompatibilityVerified`,
`toolByteProvenanceVerifiedInCi` and `bootAuthority`, with `bootableClaim` and `activationAllowed`
false alongside them.
Pinning the inputs of an image is not an image,
and it is not a kernel, an initrd, a root disk or a boot. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

_2026-08-26k launcher-artifact seal addendum:_ **THE ARTIFACT THE 2026-08-26i ADDENDUM LEFT OPEN
IS NOW SEALED.** That addendum recorded the launcher build inputs as frozen and the artifact as not
yet sealed, because the double build can only run on an arm64 Linux runner. It has now run: the
`native-shadow-launcher-build-arm64` job reports `builds=2 identical=yes`, and the launcher ELF is
`11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434` (2,006,632 bytes) for
`/usr/libexec/boole/boole-native-shadow-launcher`, built by `rustc 1.95.0 (59807616e 2026-04-14)`
and `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` on host `aarch64-unknown-linux-gnu`.

The sealed result (SHA-256 `eca743b903a6ef22ef214a14890042edaee3afd80af11c97503c255b67c0764c`,
1,028 bytes) is committed exactly as CI produced it. It was not retyped: the document CI printed
was parsed and re-emitted through this repository's own canonical serializer, and the digest of
those bytes equals the digest CI computed, which is what makes the local copy and the remote one
provably the same file rather than two similar ones. From this commit onward the build step no
longer seals anything -- it re-proves. A later run that produces different bytes fails and reports
the difference; it may never rewrite the seal to agree with itself, because the seal is the only
evidence that reproducibility ever held.

Sealing the artifact settles one boundary and no others. The boundaries the build authority
declares -- `toolchainByteProvenanceClosed`, `guestImageBuilt`, `kernelImageExtracted`,
`launcherDeployedIntoGuest`, `imageBuilderAuthorityPresent`, `runtimeCompatibilityVerified` and
`bootAuthority` -- all remain false in the sealed result, and the source lock's
`tracked-file:launcher-binary` role is not rewritten here; changing a sealed lock is a successor's
job, not an edit made in passing.
Two byte-identical builds are a reproducibility result, not a running program,
and an ELF that has never been executed anywhere is not a boot. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

_2026-08-26l boot-kernel extraction addendum:_ **THE FIRST REAL BOOT ARTIFACT EXISTS.** Every step
before this one pinned something. This one produces bytes: `guest-kernel`, SHA-256
`d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336`, 57,860,488 bytes, extracted
from the `linux-image-6.8.0-31-generic` package the 2026-08-26j authority froze. The compressed
member inside that package is `f67ad535a1b19295985d0266394d1c3a5620178a3ba61aca22cda1b6c1e27a2a`
(18,199,471 bytes), and both digests are recorded so that neither the input nor the output can be
swapped without the mismatch showing.

Two properties are worth stating precisely, because both are easy to overclaim. The architecture
check reads the arm64 magic at offset 0x38, the location the kernel image header defines, instead of
searching the file for those four bytes -- a search would also succeed on an x86 image that happens
to contain them somewhere, so the offset is the whole test. Independently, `file(1)` reports "Linux
kernel ARM64 boot executable Image, little-endian, 4K pages", which is a second opinion from a tool
that knows nothing about this repository. The extraction also ran twice, in two independent
temporary directories, and the two digests agree; decompressing a frozen byte string has no freedom
to differ, so what the second run rules out is state leaking between runs, not compiler
nondeterminism. It is a weaker claim than the launcher's double build and is recorded as the weaker
claim.

One boundary flips. `kernelImageExtracted` becomes true in the sealed result;
`guestImageBuilt`, `initrdBuilt`, `rootDiskBuilt`,
`launcherDeployedIntoGuest`, `runtimeCompatibilityVerified` and `bootAuthority` all stay false.
Unlike the launcher seal, CI cannot re-prove this one: the package bytes live in the gitignored
content store and the runner has never seen them, so the committed tests verify the extraction
logic against synthetic archives while the artifact digest rests on the local run. That asymmetry is
recorded rather than smoothed over.

The sealed boot source lock still lists `tracked-file:launcher-binary` in `missingRoles` with status
`BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS`, and the boot-artifact plan scaffold still carries three
null input slots. This slice answers the `kernel` slot and the 2026-08-26j addendum answered
`imageBuilderToolchain`; `systemdGuestClosure` remains open. Filling those nulls is a successor
plan's job -- the scaffold is not edited in place.
A kernel image is a file the boot loader can read, not a system that has booted,
and extracting one proves nothing about whether it starts. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

_2026-08-26m systemd guest closure addendum:_ **THE PLAN SCAFFOLD'S LAST NULL SLOT IS ANSWERED.**
The boot-artifact plan scaffold declared three inputs and left all three digests null:
`imageBuilderToolchain`, `kernel` and `systemdGuestClosure`. The 2026-08-26j addendum answered the
first, 2026-08-26l the second, and this one answers the third. The scaffold itself is still not
edited -- filling its nulls is a successor plan's job, and this is the material that successor will
have to draw on.

The question is narrow: would PID 1 be real systemd, and would systemd start the launcher? Both
halves resolve into chains of file facts, and both chains were followed rather than assumed. The
init chain: `systemd-sysv` 255.4-1ubuntu8 ships `/usr/sbin/init` -- not `/sbin/init`, because Ubuntu
24.04 is usr-merged -- as a symlink whose target is the *relative* string `../lib/systemd/systemd`.
Resolved against the link's own directory that lands on `/usr/lib/systemd/systemd`, which the
`systemd` package ships as a regular file whose ELF header carries `e_machine` 183, AArch64, digest
`ab970cc6f829555cad7e6891823b9c82b02f277b8fae081b7072b05e94f23f90`. The launcher chain: the unit's
`ExecStart` names `/usr/libexec/boole/boole-native-shadow-launcher`, the exact guest path the
2026-08-26k launcher seal recorded, and the enablement symlink sits in the wants directory that the
unit's own `WantedBy=multi-user.target` asks for. A unit enabled into the wrong target is present
and never starts, so agreement between those two is checked rather than presence alone.

Three details are worth recording because each was a place this could have quietly gone wrong. The
relative symlink is resolved against its own directory and a target that would climb above the root
is refused rather than clamped -- clamping turns an escape into a plausible-looking path. The ELF
architecture is read at its defined header offset, the same discipline the kernel magic check uses.
And the package reader accepts `control.tar`, `control.tar.gz`, `control.tar.xz` and
`control.tar.zst`, because the frozen set is not uniform: 188 packages use zstd, two use xz, and the
kernel modules package is uncompressed. A reader that knew only some of those would not fail --
it would silently skip packages, and a missing systemd would look exactly like a present one. The
first run of this audit did fail that way, on the two xz packages, and the fix was to widen the
reader rather than to narrow the claim.

The result keeps two tiers of evidence apart and labels each with `reproducibleInCi`. The lock tier
is true: those guest files are tracked in this repository, so a clean runner can check the whole
chain source file -> digest -> lock entry, and the audit does check the unit's bytes against the
digest the lock declares rather than trusting either alone. The package tier is false: the package
bytes live in the gitignored content store and the runner has never seen them. Averaging the two
into a single boolean would let the weaker half borrow the stronger half's credibility.

`/etc/machine-id` is tracked as an empty file, which is what makes systemd generate a fresh identity
on first boot instead of every image sharing one. No replay-node service is declared anywhere in the
closure, checked against source paths as well as logical ones, since a unit named innocuously but
copied from a replay-node file would still be a replay node. One boundary flips:
`systemdGuestClosureAudited`. `guestBootVerified`, `guestImageBuilt`, `initrdBuilt`, `rootDiskBuilt`,
`launcherDeployedIntoGuest`, `runtimeCompatibilityVerified` and `bootAuthority` all stay false, and
the sealed boot source lock still lists `tracked-file:launcher-binary` in `missingRoles`.
An audited closure is a set of file facts, not a system that has started,
and nothing here ran. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and Base activation `false` are unchanged.

---

_2026-08-26n successor boot artifact plan addendum:_ **THE THREE ANSWERED AUTHORITIES ARE NOW
PINNED IN A SUCCESSOR PLAN. NOTHING WAS BUILT AND NOTHING WAS BOOTED.**

The audit-only preflight in `scripts/native_shadow_boot_artifact_builder_arm64_v1.py` refuses any
plan whose three authority slots carry a digest, and its own error text names the remedy:
`belongs to an audit-only scaffold and must stay null; use a successor plan/schema/tool`. Its
module docstring names the three it deferred — the kernel, the systemd guest closure and the image
builder. All three now exist (2026-08-26l, 2026-08-26m, 2026-08-26j), so
`native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json` carries them under a
successor schema while the v1 scaffold is left byte-for-byte as it was. A test asserts the scaffold
still holds null slots, and the v1 preflight's own 13 tests still pass against it.

Two of the three slots pin an authority **document**; one pins raw **image** bytes. That asymmetry
is what the format strings say, not an inconsistency to smooth over: `initrd-ext4-builder-authority-v1`
and `systemd-rootfs-closure-authority-v1` name documents, `linux-arm64-image` names an image, and the
kernel result document carries no format field at all, so it cannot be what that slot pins.

The trap was one layer below that. The two documents do not agree on what to call the field that
declares their format: the image builder authority says `format`, the systemd closure result says
`closureFormat`. An earlier probe of ours read them through a fallback chain that tried one name and
then the other, and that fallback hid the difference — the same silent-skip shape that had already
cost a run when a package reader knew zstd but not xz. A reader with that fallback would accept
either document in either slot. The key is therefore pinned per slot, and a document offering the
right value under the other slot's key is refused.

All three pins are checkable on a clean runner, because each is compared against a document tracked
in this repository — the two authority documents by hashing their own bytes, and the kernel pin
against the sealed extraction result's `kernel` block. What a runner still cannot confirm is that
the kernel image bytes exist anywhere with that digest; that remains the separately recorded, non-CI
fact it was on 2026-08-26l, and this slice does not borrow credit from it.

Boundaries: only `bootInputAuthoritiesResolved` is true. `bootAuthority`, `guestBootVerified`,
`guestImageBuilt`, `initrdBuilt`, `rootDiskBuilt`, `launcherDeployedIntoGuest` and
`runtimeCompatibilityVerified` all stay false, and the plan sets `bootableClaim: false` and
`activationAllowed: false`. A resolved input is a pinned digest, not a built image. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation false — unchanged.

The initrd and root disk remain unproduced here, for two independent reasons re-verified on
2026-08-26: every ext4 tool is absent from this host (`mke2fs`, `mkfs.ext4`, `debugfs`, `e2fsck`,
`genext2fs`, `qemu-img`, and every container runtime), and the launcher ELF that must live inside the
image exists only on an arm64 Linux runner and is not committed. Whether to move image production to
arm64 Linux CI or to redesign toward an initramfs-only boot is an operator decision, not one this
slice takes.

_2026-08-26o image producer authority addendum:_ **THE ARM64 CI PRODUCER'S CONTRACT IS FROZEN BEFORE
ANYTHING IS PRODUCED. NOTHING WAS BUILT AND NOTHING WAS BOOTED.**

The operator answered the question the previous addendum left open: move image production to arm64
Linux CI, and do not redesign toward initramfs. The first condition attached to that approval was
that the contract be frozen before any result exists, so
`native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json` is that contract and
it is committed with every boundary still false.

Most of what such a contract needs was already sealed on 2026-08-26j. The v1 image builder authority
fixed the three output roles, the ext4 UUID and hash seed, a canonical mtime of 0, root-only
ownership, sorted file order, an uncompressed initrd, two independent builds, six prohibitions, and
the two tool binaries — `mke2fs` and `debugfs` — by their own digests and by the digest of the frozen
`e2fsprogs` package they are extracted from. This successor does not restate any of it. It pins that
document by digest and reads the tools out of it, because a second copy of a digest can drift from
the sealed one and a drift between two copies of the same fact is invisible. Where a value genuinely
had to be repeated — the output roles, the mismatch action, the canonical mtime, the platform — the
repetition is checked against v1 mechanically, and a smoke gate forbids the tool digests appearing
here at all.

What v1 could not say, because there was no producer yet, is what this document adds. It fills the
two slots v1 deliberately left open: `sourceDateEpoch` was null and is now 0, checked equal to v1's
own `canonicalMtime`; the zstd host tool stays `record-at-build-time`, so its digest is recorded on
the runner rather than guessed here. It names the runner (`ubuntu-24.04-arm`), requires the initrd
and the root disk to come from two separate jobs, and states what happens to the images afterwards:
CI artifact plus a SHA-256 manifest, never committed to git, never uploaded to a release.

"No network during the build" is not written here as a promise. The acquire phase runs with the
network and populates a scratch store from pinned URLs with pinned digests; the produce phase runs
inside a transient systemd unit with `PrivateNetwork=yes`, `ProtectSystem=strict`, `NoNewPrivileges=yes`,
`PrivateDevices=yes`, `PrivateMounts=yes` and `RestrictAddressFamilies=AF_UNIX`. A unit with no network
namespace has nothing for a download to reach, so the prohibition is enforced by the kernel rather
than by this paragraph. The shape is not invented — it is the one
`scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh` already uses.

The guest launcher is obtained the same way, by refusing a handoff. The producer runs the frozen
`scripts/native_shadow_launcher_build_arm64_v1.py` itself — that script already builds twice and
requires identical bytes — and checks the result against the sealed
`native-shadow-launcher-build-result-arm64-v1.json` before placing it at
`/usr/libexec/boole/boole-native-shadow-launcher`. Receiving the ELF as an artifact from another job
would mean trusting the handoff; rebuilding and re-checking trusts only the seal.

Seven abort conditions are named, and every one of them carries `relaxKnobAllowed: false`: two
independent jobs producing different bytes, a tool binary failing its digest, the produce phase
reaching for the network, a maintainer script copied into the assembled tree, two packages claiming
the same logical path, a required output missing or empty, and a rebuilt launcher differing from the
seal. The first is the one worth being explicit about — if the two jobs disagree on ext4 timestamps,
UUID or file order, the run stops and reports both digests. It does not lower a determinism knob
until they agree.

Two of those seven were corrected before this document was merged, by a read-only pass over the
frozen packages in the local content store. That pass is **not reproducible in CI** — the store is
gitignored — so it is recorded here as a local observation, not as a verified property. It found
15,297 entries across the 191 `data.tar` layers (3,460 directories, 11,193 regular files, 643
symlinks, 1 hardlink), 490,719,409 bytes of regular file content — 0.46 GiB against the frozen 2 GiB
cap — 11,837 distinct non-directory paths, and **262 maintainer scripts**.

That last number is the correction. The abort condition first said "a maintainer script appeared in
the consumed set", which every Debian archive violates by construction and which would therefore
have stopped the first run that ever started. The source lock's rule is `never-execute-or-copy`, so
the condition that must stop a build is one reaching the assembled tree, and that is what it now
says. The pass also found **0 path collisions**, which is why the second condition exists: nothing
in the design decides which of two colliding files wins, so union order would become observable and
the two independent jobs could disagree for a reason no digest comparison would explain. Zero today
is a measurement of this package set, not a property of it.

The document names its own generator and the generator names the document, which cannot both be a
plain file digest. The cycle is broken the way v1 already breaks it: the generator is hashed with its
own authority pin blanked to sixty-four zeros, so either side reproduces the other's value. A test
asserts that changing only the pin leaves the generator digest untouched.

Boundaries: `bootAuthority`, `guestBootVerified`, `guestImageBuilt`, `initrdBuilt`, `rootDiskBuilt`,
`launcherDeployedIntoGuest`, `runtimeCompatibilityVerified` and `toolByteProvenanceVerifiedInCi` are
all false — not one is true. `bootableClaim: false`, `activationAllowed: false`.
A frozen contract is a promise about a build, not the build.
CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED` and no dev-Mac trial substitutes
for it. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26p initrd writer addendum:_ **THE INITRD WRITER EXISTS. NO INITRD OF THE REAL ROOTFS WAS
WRITTEN, NOTHING WAS BUILT AND NOTHING WAS BOOTED.**

`scripts/native_shadow_boot_initrd_arm64_v1.py` turns a frozen OCI rootfs layer into the initrd
shape v1 sealed on 2026-08-26j: `cpio` in the `newc` format, `initrdCompression: "none"`,
`fileOrder: "sorted-by-logical-path-bytes"`, canonical mtime 0, root-only ownership. It reads a tar
and writes an archive; it does not fetch, resolve, assemble or boot. Its tests run against synthetic
layers, so this addendum records a writer, not an image.

One field in the `newc` header decides whether the two independent CI jobs can ever agree. `newc`
carries an inode number, and the obvious way to fill it — ask the filesystem — would differ between
two runners and fail the byte comparison for a reason that has nothing to do with the image.
Numbering from 1 in archive order makes the field a function of the layer alone. Compression stays
off for the same class of reason: a gzip member carries its own mtime, so compressing here would
reintroduce the timestamp the canonical mtime exists to remove, and v1's
`forbidTimestampSuppression` rules out papering over it afterwards.

The layer's ordering and ownership are re-checked rather than assumed. The frozen OCI builder
already sorts by path bytes and already forces uid/gid to 0, so these checks should never fire —
which is exactly why they are worth keeping. An invariant only ever asserted somewhere else is one
that quietly stops holding the day this writer is handed a different tar.

The archive was cross-checked against a second implementation rather than only the parser shipped
beside it: the system `cpio` lists the modes, root ownership, epoch timestamps, sizes and the
symlink target correctly, and `cpio -idm` round-trips the content byte for byte.

Two read-only findings are recorded here because they shape what comes next, and both are
**observations, not verified properties** — they were made against the gitignored local content
store and CI cannot re-prove them.

First, an earlier reading of this work concluded that no existing tool could turn the boot source
lock into a filesystem tree, and that the producer would have to assemble one. That was wrong. The
gap is already bridged by `materialize_runtime_lock()` in the portable arm64 projection, which
accepts exactly the boot lock's schema. The boot lock and the runtime arm64 lock are structurally
identical — same top-level keys, byte-identical `buildRecipe` — and the boot lock is a strict
superset in content: 197 artifacts against 62, and four extra tracked files with their bindings plus
one derived symlink, which are precisely the boot files and nothing else. Writing a second assembler
would have produced a tool that can disagree with the frozen one about what the tree is.

Second, the frozen `e2fsprogs` package (`6e1cdd65…`) yields `mke2fs` at 133,512 bytes with digest
`763be3ec…` and `debugfs` at 271,944 bytes with digest `2c0bf348…` — exactly v1's pins. The same
package also ships `./etc/mke2fs.conf`, and that matters: `mke2fs` reads its feature defaults from
that file, so running the frozen binary against a runner's conf would let a distro change pick the
ext4 feature set while both jobs could still claim to have used the frozen tool. Pointing
`MKE2FS_CONFIG` at the conf extracted from the same verified package closes it and needs no new pin,
because the package digest already covers the conf.

A third finding is recorded as an open inconsistency rather than a fix. The sealed boot source lock
holds `/usr/lib/sysusers.d` before `/usr/lib/systemd` in one `closureRoots` group. That reads as
alphabetical to a person but is not byte-sorted, and the frozen builder requires byte-sorted unique
roots, so the sealed lock cannot be fed to the builder its own `buildRecipe` names. The generator
sorted closure groups by name and copied each group's roots verbatim from the plan, and no check
covered the inner order. The ordering is provably output-neutral — the builder consumes those roots
only inside an existence test, and no root in that group is a prefix of another — but the sealed
lock and the frozen builder still disagree, and the remedy is the operator's to choose. Nothing was
edited, normalized or worked around here.

The root disk plan (`scripts/native_shadow_boot_root_disk_arm64_v1.py`, 27 tests) turns the same
layer into the argv, environment and staging order that `mke2fs` will be given. It executes nothing:
`mke2fs` is an aarch64 ELF, the host that plans is not the host that builds, and keeping the two
apart is what makes the plan reviewable before anything is written. Reading the frozen binaries
changed three things about it.

`SOURCE_DATE_EPOCH` does nothing to this build of `mke2fs`. The string is absent from the binary;
what the shipped `libext2fs.so.2.4` reads is `E2FSPROGS_FAKE_TIME`, and that is the variable the plan
sets to `0`. This does not weaken v1's `determinism.sourceDateEpoch: 0` — that field is the canonical
epoch for the build as a whole and remains 0 — but it does correct an assumption made earlier in this
same slice, which had `SOURCE_DATE_EPOCH` closing the superblock-time trap. It would not have. Both
jobs would have stamped their own wall clock into `s_mkfs_time` and differed on a field neither of
them chose. The plan's `docs-smoke` gate forbids the wrong variable from reappearing.

`mke2fs -d` does not sort. `opendir` and `readdir` are present in the binary and `scandir`,
`alphasort` and `versionsort` are all absent, so the population order is whatever the staging
filesystem returns. On ext4 a directory large enough to become an htree is returned in filename-hash
order and that hash is seeded per filesystem, so two runners would disagree. The plan stages on
`tmpfs`, whose readdir order is creation order, and creates entries in logical path byte order. That
is an assumption about the runner's kernel rather than a proof, so the plan carries it as one of
three named `unverifiedAssumptions`, each with `onMismatch: "abort-never-relax"`.

Every shared library the two tools need is already frozen. Parsing `DT_NEEDED` out of both ELF
headers gives eight sonames, and each is shipped by one of the 191 packages: `libext2fs.so.2` and
`libe2p.so.2` from `libext2fs2t64`, `libcom_err.so.2` from `libcom-err2`, `libss.so.2` from `libss2`,
`libblkid.so.1` from `libblkid1`, `libuuid.so.1` from `libuuid1`, and `libc.so.6` with
`ld-linux-aarch64.so.1` from `libc6`. A first pass through this reported `libe2p` as a gap on the
strength of no `libe2p2t64` being in the set; there is no such package in this release, and
`libext2fs2t64` carries the library. The plan records the eight providers. What it cannot settle is
which copy the loader picks at run time, so that — not availability — is the third assumption, to be
closed by recording the resolved paths at build time.

`debugfs` keeps the role v1 sealed for it. The plan names it as `ext4-image-inspector` and a test
asserts it never appears in the `mke2fs` invocation, so the determinism problems above are solved
where the sealed roles allow rather than by promoting the inspector to a writer.

Boundaries are unchanged and every one of them is still false. `bootableClaim: false`,
`activationAllowed: false`. A writer for a format is not an image, a plan for an image is not an
image, and an image is not a boot. Nothing was produced, staged, mounted or run. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26q verification stage addendum:_ **THE VERIFICATION STAGE EXISTS. NO IMAGE WAS PRODUCED,
NOTHING WAS INSPECTED FOR REAL AND NOTHING WAS BOOTED.**

`scripts/native_shadow_boot_image_verify_arm64_v1.py` (25 tests) reads produced boot artifacts back
and runs the six checks the operator named: the kernel is arm64, PID 1 is real systemd, the
launcher's digest equals the sealed build result, the launcher unit is enabled through its
`multi-user.target.wants` symlink, no replay node appears anywhere in the tree, and every tracked
path's mode, ownership and content match the sealed lock. Its tests run against synthetic trees, so
this addendum records a checker, not a verdict on any image.

It is deliberately not part of the producer. A producer that verifies its own output can only
confirm that it did what it did; these checks are written against the sealed lock and the sealed
launcher result, which means they are able to disagree with the thing that built the image.

Two of the six deserve their reasoning written down. The kernel check reads the arm64 `Image`
header — 64 bytes with the magic at offset `0x38` — rather than trusting a filename, and it rejects
a payload that is still a gzip member, because v1 froze `kernelDecompression: gzip` and a
compressed output would mean that step did not run. The launcher check fails when the launcher is
absent rather than passing vacuously, which is the difference between "the digest matched" and
"nothing contradicted us"; the same shape covers a run given no kernel at all, where an absent check
would otherwise read exactly like a passing one.

The initrd side runs anywhere, because a `newc` archive is readable without root and without a Linux
host. The root disk side does not: it is an ext4 image and the tool that reads it is `debugfs`, which
v1 sealed as `ext4-image-inspector`. This module emits read-only `debugfs` commands for that stage
and a test asserts `-w` never appears in the invocation, so the inspector cannot become a writer by
accident.

`bootableClaim: false`, `activationAllowed: false`, and the report itself carries
`guestBootVerified: false`. Reading an image is not booting it. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26r boot builder projection addendum:_ **A BUILDER CAN NOW READ THE SEALED BOOT LOCK. NO
IMAGE WAS PRODUCED AND NOTHING WAS BOOTED.**

The boot source lock was sealed against a builder that could not read it. Its
`buildRecipe.builderSha256` pins `scripts/native_shadow_rootfs_builder_arm64_v1.py`, and that file
knows six authority files, three provenance closures, and a dependency grammar narrower than the one
this closure actually uses. `scripts/native_shadow_rootfs_builder_boot_arm64_v1.py` (26 tests) and
`scripts/native_shadow_rootfs_portable_boot_arm64_v1.py` (12 tests) close that gap the way this
repository has closed every previous one: by reprojecting the frozen bytes with counted string
replacements rather than forking them.

The pin is preserved rather than worked around. The projected namespace is executed with `__file__`
pointing at the arm64 builder, which is the single place that file reads it, so `BUILDER_SHA256`
still equals the sealed pin and a test asserts that equality against the lock itself. The widening
is not inside the pin, so it gets its own digest — `BOOT_PROJECTION_SHA256`, the projection's own
bytes — instead of being smuggled under one that was computed before it existed.

Two of the five differences are ordinary configuration. The authority-file table grows from six
entries to ten, adding the guest `machine-id`, the launcher unit, and the sysusers and tmpfiles
configuration that unit depends on; a test asserts the widened table equals the lock's `trackedFiles`
exactly, so it cannot drift wider than what was sealed. The provenance-closure table grows from
three to five, adding the guest init/launcher closure and the guest kernel/module closure.

The third is an ordering difference, and the honest answer was not to sort silently. One closure in
the sealed lock lists its logical roots alphabetically but not byte-sorted, and the frozen builder
compares against `sorted(set(roots))`. `normalized_runtime_lock` sorts a copy, leaves the sealed
bytes untouched, and records the declared and normalised orders side by side with a `reordered` flag,
so the difference stays visible instead of disappearing into a comparison. It refuses outright if
sorting would also collapse a duplicate root, because the "unique" half of the builder's check has to
stay real; `sorted(set(...))` would have absorbed a duplicate without comment.

The fourth and fifth touch the frozen builder's dependency reading, so they were measured rather than
argued. `_split_dependency_groups` rejects architecture-qualified and build-profile dependencies —
correctly — but applies the pattern `\[[^\]]*\]|<[^<>=]+>` to the whole comma-joined field, where
`python3 (<< 3.13), python3 (>= 3.12~)` matches because the `<` of one constraint and the `>` of a
later one look like a profile once everything between them is ignored. Scanning all 5816 `Depends`
and `Pre-Depends` fields in the frozen Packages index, that pattern fires 98 times and is wrong all
98 times, and the same scan finds no real architecture qualifier or build profile anywhere in the
index. Applying the identical pattern per alternative instead of per field flags zero of the 5816. A
real `[arm64]` or `<!nocheck>` lives entirely inside one alternative, so narrowing the scope cannot
let one through, and tests hold that line: `libc6, gcc [arm64]`, `libc6, dpkg-dev <!nocheck>` and
`libc6 | gcc [arm64]` are all still refused.

`_dependency_matches` refuses every Multi-Arch qualifier. Exactly one qualified alternative appears
in this closure — `python3:any`, used by `python3-apt`, `python3-dbus`, `python3-pkg-resources`,
`python3-yaml` and `ubuntu-pro-client` — and all five resolve to the same `python3`, which declares
`Multi-Arch: allowed`. `:any` means "satisfied by that package from any architecture", and this
closure holds exactly one concrete architecture (`arm64`, plus `all`), so `:any` and the bare name
select the same candidate. That is a fact about this closure and not a general one, so it is guarded
twice: `assert_single_architecture` stops the build if a second concrete architecture ever appears or
if the declared architecture disagrees with the platform, and the projected matcher refuses `:any`
against a provider that does not declare `Multi-Arch: allowed`. `:native` stays unsupported.

The portable projection changes two strings and nothing else: the accepted release becomes the boot
lock's own, and the builder the chain reaches becomes the boot projection. What
`materialize_runtime_lock` verifies is unchanged, and tests assert that a non-canonical lock, an
`activationAllowed: true` lock, and a lock with the wrong tool roles are all still refused, and that
the portable release is no longer accepted here — widening which lock is accepted must not mean
accepting both. Both projections execute into their own namespace, and a test asserts the arm64
module still reports six authority files and three closures after the boot module is imported, so a
widened boot table cannot leak into the portable path.

The end-to-end test runs the whole chain — sealed lock, materialised runtime lock, normalisation,
full validation against the acquired closure — and skips where that closure is not on disk, which is
every CI runner, since `local-docs` is gitignored. What CI proves is the contract; what this host
proved is the run. Both are recorded, and neither is described as the other.

`bootableClaim: false`, `activationAllowed: false`. A builder that can read a lock has not built
anything. CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26s produce-phase isolation addendum:_ **THE WRAPPER AND THE COMPARISON EXIST. NOTHING WAS
PRODUCED, ISOLATED FOR REAL OR COMPARED.**

The frozen producer authority splits the work in two: `acquire` may reach the network to fetch pinned
URLs at pinned digests, and `produce` may not reach it at all. `scripts/native_shadow_boot_image_produce_arm64_v1.py`
(30 tests) is the part that makes the second half true rather than merely stated, plus the two pieces
that decide whether two independent runs agreed.

The isolation wrapper builds a transient `systemd-run` unit carrying the six properties the authority
sealed. Those properties are read out of the sealed document rather than restated in the script,
because a second copy of a sealed fact can drift from it and a drift between two copies of the same
fact is invisible; a test proves the derivation by adding a property to the document and finding it in
the argv. Deriving is not the same as trusting, though. A document that dropped `PrivateNetwork=yes`,
or set it to `no`, or allowed the network in the `produce` phase, or named a runner other than
`ubuntu-24.04-arm`, or stopped requiring two separate jobs, is refused rather than faithfully turned
into a weaker command line. That is the difference between inheriting a contract and inheriting
whatever the file happens to say.

`ProtectSystem=strict` makes the whole hierarchy read-only, so the produce phase needs an explicit
hole for its outputs, and the hole is where such a wrapper usually leaks. Read-write paths are the
caller's to name — the sealed list is refused if it tries to carry one — and `/` is refused outright,
as is any path at or inside `/usr`, `/etc`, `/boot`, `/bin`, `/lib` or `/sbin`. A hole that wide would
undo the property rather than make room beside it. The unit also waits and reports its own exit
status, because a fire-and-forget unit would let a failed produce read as a pass.

The manifest is `sha256sum` text, byte-sorted by name, and the parser refuses what a lenient one
would absorb: a line without the two-space separator, a digest that is not lowercase sha-256 hex, and
a repeated name — last-one-wins on a duplicate would silently drop a real disagreement. An output
that is missing or zero bytes stops with the authority's own `output-missing-or-empty`, because an
absent artifact must not read like a passing one.

The comparison is the part conditions 5 and 6 turn on, so it was written to have no other option. It
reports the difference and names both digests; an output present in only one job is a difference too,
not something to skip; and a test asserts the function takes exactly two parameters with no defaults,
so there is no knob — present or future — that could force a match. `MISMATCH_ACTION` is checked
equal to the authority's own `report-the-difference-never-force-a-match`.

The module builds argv and computes digests. A test reads its own source and asserts it contains no
process-spawning call at all, so the thing that describes the isolated build cannot quietly become
the thing that runs it.

`bootableClaim: false`, `activationAllowed: false`, `guestImageBuilt: false`. No image was produced,
no unit was started and no two manifests were compared. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26t boot closure exception addendum:_ **THE CLOSURE'S DEPARTURES ARE ENUMERATED. NO IMAGE IS
AN AUTHORITY RESULT AND NOTHING WAS BOOTED.**

The frozen builder had never been pointed at a real usrmerged Ubuntu closure. Pointed at the sealed
191-package boot lock, it refuses — four times, for four different reasons, all of them correct. A
knob would have made each refusal go away. `native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json`
writes the four down instead, and the projection reads that file rather than restating it, so the
enumeration cannot drift from the code that honours it.

The sealed rule is `ownership: root:root-only`, and eleven members of the closure arrive owned
otherwise or carrying a set-id bit — `sudo`, `mount`/`umount`, six `passwd` tools, two PAM helpers.
The builder already hardcodes `uid: 0, gid: 0` into every entry it emits, so the refusal was never
preserving those bits; it was refusing to rewrite them silently. The rewrite is therefore written
down: each row records what it changes away from, a listed path arriving with different ownership
than recorded is still refused, and an unlisted member is refused exactly as before.

The closure ships `bin`, `lib` and `sbin` as directories while the sealed lock derives `/lib` as a
symlink into `/usr`, which is a collision only because the two halves of merged-`/usr` were being
read literally. Relocation canonicalises the path and then reuses the frozen collision rule verbatim,
so it moves paths without ever deciding which of two members wins. `base-files` is not in this
closure, so nothing ships the merged-`/usr` symlinks at all; `/bin` and `/sbin` are added the same
way `/lib` already was, and `/lib64` is not, because arm64 does not use it. Nine symlinks point at
targets the closure does not contain: six resolve into `dev`, `proc`, `run`, `sys` or `tmp` and are
allowed as a category, since a filesystem mounted at boot is absent from an image by construction;
the other three are named individually, with a reason each.

Adding `/bin` fixes a layout, not an interpreter. No package among the 191 provides a shell — no
dash, no bash, no busybox — so the eighty-nine members whose shebang names one cannot run however
their path resolves, and the record says so beside the row rather than letting `/bin` read as a
repair it is not. Every unit this image enables names a binary directly, so the boot path asks for no
shell; confirming that against a produced image is a verify step's job, not this record's.

On this host, the chain does now run end to end: two independent builds of the assembled rootfs were
byte-identical, 13,444 entries, and `builderSha256` still equalled the sealed pin, which is what
makes the four departures departures of the projection rather than of the frozen builder. That is a
host observation. It is not a CI-proven result, not a produced boot artifact and not an authority
record, and the status line in the exception file says so:
`CLOSURE-EXCEPTIONS-ENUMERATED-NOT-APPLIED-NOT-BOOTED`.

`bootableClaim: false`, `activationAllowed: false`, `guestImageBuilt: false`. Enumerating a departure
is not building, booting or qualifying an image. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation false — unchanged.

_2026-08-26u launcher placement addendum:_ **THE LAUNCHER HAS A WAY IN AND A SEAL TO MATCH. NOTHING
WAS DEPLOYED OR BOOTED.**

`/usr/libexec/boole` is a closure root the sealed lock declares and no package fills. It cannot fill
itself: the launcher is a build product, so it arrives neither as a deb nor as a tracked repository
file, and the tracked-file table is an exact set match against the authority bindings — widening it
to admit a binary that has no repository bytes would be lying about what a tracked authority file is.

The frozen producer authority already said how it may arrive instead, and this is the code that obeys
it. `acquisition: rebuild-and-match-seal` is the whole rule, so `launcher_entry` checks the rebuilt
binary's size and digest against the seal separately and stops on either with the authority's own
`launcher-digest-mismatch`. The digest is read out of the authority rather than restated in the
projection, and a test asserts the literal does not appear in the projection's source, because a
second copy of a sealed fact can drift from it silently.

Supplying the launcher is the caller's decision rather than a default, so `build_oci_layout` takes it
by keyword and `verify_oci_layout` forwards it into its own rebuild — a verify that rebuilt without
the launcher would report a difference it had itself created. Building this closure with no launcher
supplied still produces the same bytes it produced before this change, which is what says the
placement is an addition and not a rewrite.

`launcherDeployedIntoGuest: false` in every sealed record still, and correctly: a path into the tree
is not a deployment. No launcher was rebuilt here, none was placed, and nothing was booted.
`bootableClaim: false`, `activationAllowed: false`, `guestImageBuilt: false`. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation false — unchanged.

_2026-08-27 root-disk determinism addendum:_ **THE DISK WAS BUILT TWICE FROM IDENTICAL INPUTS AND
THE TWO COPIES DIFFERED. THE STOP WAS TAKEN, THE CAUSE IS CONFIRMED, AND NOTHING WAS BOOTED.**

Two arm64 CI jobs produced the guest root disk. The kernel and the initrd were byte-identical. The
disk was not: `b3299ed161557a195a9f58bb899fc61979ecb7456de94b9d89ac951c2e320b96` against
`11099b116c3241a4441ba6ed3cfd4b7db6a160779c97271bcce5bf6e99b7153a`, both 1,168,314,368 bytes. The
pre-registered rule for that outcome was to stop rather than relax the criterion, and it is sealed
append-only in `native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json`
as `ROOT-DISK-BYTE-IDENTITY-FAILED-CAUSE-CONFIRMED-STOP-HELD-NOT-BOOT-AUTHORITY`. That record is not
edited and not reread as a success by anything after it.

The difference was measured, not characterised. 847 of 285,233 blocks, all of them superblock,
backup superblock or inode table; every data block, directory block, bitmap and group descriptor
identical. 80,748 differing bytes with **zero unexplained** — each is a timestamp field or a checksum
over one. A read-only walk of both images compared 13,448 entries and 1,008,783,262 bytes of file
content: **no file's contents differed**, and path, order, inode, mode, ownership, size, links,
symlink target, extended attributes and layout matched everywhere. The verdict
`METADATA-TIMESTAMPS-ONLY` describes the difference; it is not a reason to accept it, and byte
identity remains the criterion.

The cause was read out of the frozen binaries rather than inferred from the outcome.
`E2FSPROGS_FAKE_TIME=0` is a no-op because `0` is this library's "unset" sentinel, so pinning the
time to zero is indistinguishable from not pinning it, and both jobs read clocks 58 seconds apart.

`native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json`
carries the bar the fix must clear and was pushed before any production code changed. It splits one
number into two: the canonical source epoch stays `0` as the staged inputs' own meaning, while the
value handed to the ext4 writer becomes `1` — the smallest non-zero fixed value, and so the smallest
change that makes the pin take effect. Before that was written, every field that moved was traced
statically to `fs->now`: the superblock's write, last-check and mkfs times, and the inodes' access,
change, modify and creation times, each storing the pinned value directly with `time()` behind the
zero branch. Nothing is left over. That is a claim about which fields the value reaches, not a claim
that the images will match — only a produced pair settles that. The successor also emits the loader
provenance the executor already computed and discarded, and pins `e2fsck` by digest to run `-f -n`
per replica with `0` as the only accepted exit code.

The 2026-08-27b mirror synchronization appends the correction below to those same three mirrors,
each of which carried the superseded completeness claim in its own words. The claim stays as written
in all three and the correction follows it; the other three mirrors are byte-unchanged, confirmed by
recomputing all six digests from the files rather than carrying any value forward by hand. The
section 12 table holds those recomputed values.

The 2026-08-27 mirror synchronization appends this same state to `todo-l1-network-master.md`,
`EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other three section 12 mirrors are
byte-unchanged, which was confirmed by recomputing all six digests from the files themselves rather
than carrying any value forward by hand. The section 12 table holds those recomputed values; they
are synchronization evidence only, never runtime trust roots.

`bootableClaim: false`, `activationAllowed: false`, `guestBootVerified: false`. No image was adopted,
no third image was produced and nothing was booted. MAC.3 closed-local boot stays blocked until the
successor is green. CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

_2026-08-27 successor correction addendum:_ **"NOTHING IS LEFT OVER" ABOVE WAS WRONG. ONE FIELD
SURVIVES THE SUCCESSOR VALUE, IT WAS FOUND BEFORE ANY IMAGE WAS PRODUCED, AND PRODUCTION IS NOW
BLOCKED RATHER THAN ATTEMPTED.**

The sentence stays as written. The pre-verification read `libext2fs` and found every timestamp writer
there gated on `fs->now`; that part holds. It did not read `mke2fs`. Under `-d`, `mke2fs` calls
`ext2fs_write_new_inode` for each staged entry — the library path that was checked — and then
overwrites `i_atime`, `i_ctime` and `i_mtime` from the staging file's `struct stat`, without reading
`fs->now`. The overwrite is in the program, not the library, which is why reading the library looked
complete.

`objdump` over the frozen `mke2fs`, whose digest the plan already pins; nothing executed, no image
produced. The stat buffer is fixed by the `lstat64` call at `0x139f0` and the `S_IFMT` mask after it,
the inode buffer by the `i_links_count` store at `0x13d08`; the copies are at `0x13dac` and `0x13da0`
into inode offsets `0x8`, `0xc` and `0x10`, then `ext2fs_write_inode` at `0x13db0`. The inodes
`mke2fs` creates on its own account do read `fs->now`, at `0x13ca4` with the zero-branch at `0x13cc0`.
That matches the measured diff exactly: `i_atime` and `i_mtime` each differed in five inodes — the
five `mke2fs` creates itself — while `i_ctime` differed in all of them.

`st_ctime` cannot be pinned from userspace: `utimensat` sets atime and mtime only, and any metadata
change updates ctime. So the successor's writer time is **necessary but not sufficient** — it removes
`i_atime`, `i_crtime`, `i_mtime`, `s_lastcheck`, `s_mkfs_time` and `s_wtime`, and `i_ctime` survives.

The correction is appended to the successor authority as a `corrections` entry; the corrected claim
is byte-identical and the three sealed predecessor files are untouched. The produce phase refuses to
start while a named cause is recorded as present, because that record allows one production pair and
forbids retrying a pair that has produced a result. No remedy is adopted: a different `e2fsprogs`
changes the sealed source lock, `debugfs` as a post-hoc writer is refused in writing, and dropping
`i_ctime` from the comparison relaxes the criterion. The options are recorded; the choice is the
operator's. Nothing here is silent — the produce phase's timestamp audit aborts a replica with
`wall-clock-survived-in-the-image` before any comparison.

`bootableClaim: false`, `activationAllowed: false`, `guestBootVerified: false`, `imageProduced: false`.
CURL.3 remains `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation false — unchanged.

## 14. Testnet/public task-inventory correction (2026-08-27)

**TESTNET/PUBLIC TASK-INVENTORY CORRECTION.** The current native-shadow path proves closed-local
qualification and node-owned judgment. It does not yet define a public task domain, authorize a
testnet or connect an accepted answer to consensus or rewards.

`14,160 is a template/issuance-supply count, not 14,160 one-shot public answers`. A template remains
available when its frozen generator issues a fresh network/epoch/seed instance. Only that issued
instance becomes spent in its own domain. Conversely, any static or non-fresh fixture disclosed on
a testnet is permanently test-only and must never be promoted into public inventory.

For the future networked path, an "answer" in the rule below means the complete network-bound
submission envelope and receipt, not merely source-code text that might coincidentally satisfy more
than one independently generated task:

**A testnet answer MUST be rejected in every public-network domain.**

Before checker execution can authorize a terminal result, the node must independently recompute a
version-tagged, canonically length-delimited domain commitment containing at least `network_id`,
`chain_id`, `family_version`, `template_id`, `epoch` and `challenge_seed`, plus the frozen
challenge/policy/registry digests required by the successor contract. It must reject a mismatch
before accepting or redelivering checker evidence.
Testnet and public issuance, spent-instance and replay ledgers must have disjoint authority and must
not alias through a shared key that omits the network domain.

The reciprocal replay must also fail: a public-domain submission/receipt presented to the testnet
domain is rejected before checker execution or testnet-ledger mutation. This bidirectional rule
does not globally ban identical raw answer bytes from being judged independently for two genuinely
fresh tasks.

The required proof is two-sided: the same valid fresh-family workflow must succeed in its own
testnet domain, while its exact signed submission/receipt must fail in a separately configured
public-network domain without invoking the checker or mutating the public ledger. Static fixtures
are allowed only from a permanently excluded test inventory. The reciprocal public-to-testnet case
must satisfy the same no-checker, no-ledger-mutation rule.

This is a frozen future RED-gate contract, not landed protocol code. The product-level order is
runtime image, closed-local fresh issue/check, cross-network replay rejection, private
non-consensus integration testnet, BF.7 zero-reward testnet, BF.8 activation evidence, and only
then separately approved activation. This shorthand does not remove BF.3, BF.6, BF.6a, RP0-MD,
deterministic-resource, Economic ADR or separate implementation-plan gates. No step is inferred
from the previous one. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged.

The 2026-08-27c mirror synchronization appends this correction to
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and `thesis-realization-roadmap.md`. The other
three section 12 mirrors are byte-unchanged. All six digests were recomputed from the local files;
the section 12 table contains those exact post-update values. These hashes are synchronization
evidence only and never runtime authority.

## Preserved ext4 reconciliation addendum (2026-08-29)

The preserved successor root disk has now been reconciled without mounting or
modifying it. All 17,677 logical paths, 15,102 regular files and 837 symlinks
match exact sealed expectations; all 435,530 allocated blocks have one declared
owner class; and all historical 135 raw candidates belong to exact regular-file
content with no journal, slack, free, unmapped or ambiguous remainder. No host
wallet, model key or node-secret material was observed.

That result does not close the qualification condition. The actual CI producer
home appears 23 times in the exact sealed launcher as compilation provenance.
It is not secret material, but the frozen condition says no host path. The
result therefore separates `reconciliationPassed=true` from
`conditionSettled=false` instead of weakening the sentence after observation.

The next implementation step is a launcher-v2 source seal and deterministic
producer-path remap, together with the already parked console-evidence producer
and dynamic privilege tests, followed by two independent byte-identical arm64
builds. This addendum grants no image-production, boot, node, MAC.4, mining,
reward, consensus or P2P authority. The 2026-08-29 mirror synchronization also
brings the three append-only local planning mirrors' current contents back into
the section 12 digest table; the other three mirrors are byte-unchanged.

## Launcher v2 pre-build addendum (2026-08-30)

The successor source and build contract are now frozen without editing the v1
generation. A temporary export receives a digest-bound overlay containing the
console producer and table-driven child privilege checks. Each of two arm64
builds receives its own fresh Cargo home; both the source root and Cargo home
are remapped to fixed logical paths, and the ELF must contain zero occurrences
of five real producer-path classes before it can be sealed.

The host reader's prerequisite field mismatch is recorded as an append-only
correction: `resolved` is the sole accepted boolean, while the unintended
`present` spelling is refused. The shared fixture is consumed by both the Rust
producer test and Python reader test. A closed boot still executes no answer,
so the condition-four record stays partial until a later real-request gate.

The named Linux/arm64 v2 build job is required in addition to the historical v1
job. At this pre-build cursor there is no v2 result yet, no image-production or
boot authority, and no MAC.4, node, mining, reward, consensus or P2P change.

Pre-build review also pins the operational order and the first-result gate.
Readiness evidence is emitted only after the fixed listener has been bound and
its socket identity verified, and before the first accept. Exact JSON types and
malformed-prefixed-line refusal prevent Python bool/int aliases or an ambiguous
extra line from satisfying the host comparison. The first arm64 candidate is
printed but required CI fails while its result is untracked; only a committed
record that a later run reproduces byte-for-byte may pass. These changes grant
no build-result status by themselves and open no later authority.

## Launcher v2 first-candidate seal addendum (2026-08-30)

The first Linux/arm64 discovery run produced two byte-identical 2,025,192-byte
ELFs at SHA-256
`53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd`.
Both overlay test runs passed and every named producer-path counter was zero.
The job then failed deliberately because its canonical result was untracked.

Those exact candidate bytes are now reviewed and tracked at
`native-shadow-launcher-build-result-arm64-v2.json`, whose SHA-256 is
`0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08`.
The record can reach `main` only after a fresh required CI job rebuilds twice
and reproduces it byte-for-byte. It grants no image production, boot, node,
MAC.4, mining, reward, consensus or P2P authority.

## Launcher v2 sealed-emitter addendum (2026-08-30)

The tracked two-build result now has a narrow byte consumer. The v2 emitter
reconstructs the complete canonical result, runs one additional Linux/arm64
build, requires the exact producer-path class set with all counts zero, and
publishes only a byte-for-byte size and digest match. Exclusive temporary-file
creation plus no-overwrite hard-link publication prevents an existing path,
dangling link, stale temporary file or concurrent writer from being replaced.

The required CI job exercises the emitter only after re-proving the tracked
two-build result. Its output stays in runner-temporary storage. No image
producer consumes it here, so this addendum grants no image, boot, node, MAC.4,
mining, reward, consensus or P2P authority.

## Launcher-v2 image-integration preregistration addendum (2026-08-30)

PR #301 exercised the sealed emitter on Linux arm64. The tracked result was
re-proved first, then a third build emitted the same 2,025,192-byte file at
SHA-256
`53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd`;
the workflow shell compared the emitted digest and size independently.

The following preregistration binds that proved emitter to the current staging
inputs without granting production. It derives the sole expected content delta
from launcher v1 and v2 results: entries stay 17,676 and payload becomes
1,773,475,059 bytes. It also records that the current builder correctly refuses
v2 under its v1 seal, so a new predecessor-pinned builder projection is the next
step. Image production runs allowed remain zero; boot, MAC.4, testnet, mining,
reward, consensus and P2P remain unopened.

## Launcher-v2 no-image arm64 preflight result addendum (2026-08-30)

<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:BEGIN -->

Required Linux/arm64 CI exercised the predecessor-pinned builder projection and
the repeatable launcher-v2 staging preflight. PR #303 merged as
`6a14563ad078578147ac7bcd99b15e2a760e9930` after all required checks passed.
It produced one JSON-only artifact and no image output.

```text
sourcePullRequest=#303
sourceHeadSha=6e95d5a73a17dda26adb006cd2c0de5129a1921d
sourceWorkflow=.github/workflows/ci.yml
sourceRunId=33272680385
sourceRunAttempt=1
sourceJobId=99153889500
artifactId=9720614194
artifactName=launcher-v2-image-preflight-result
archiveSizeBytes=3079
archiveDigest=sha256:beb2920dcfe11ae0f827b73245a8a15bf9e7b055809ad23fac953cef4ed633c8
artifactMemberCount=1
artifactMemberName=PREFLIGHT-RESULT.json
payloadSizeBytes=9409
payloadSha256=2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d
```

The raw result is 9,409 bytes; GitHub's compressed archive is 3,079 bytes.
Their identities remain separate so transport packaging cannot stand in for
the result itself.

Both independent measurements agree on 17,676 entries, 1,773,475,059 payload
bytes and path-manifest SHA-256
`0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a`.
The result rechecks all 22 preregistered inputs and retains every authority at
zero or false. It proves staging compatibility only: no image was produced, no
production run was authorised or consumed, no guest booted and MAC.4, testnet,
mining, reward, consensus and P2P remain unopened.

The 2026-08-30e/2026-08-30i mirror synchronization appends this same boundary
to `todo-l1-network-master.md`, `EXECUTION-ORDER.md` and
`thesis-realization-roadmap.md`. The other three section 12 mirrors are
byte-unchanged. Section 12 contains the recomputed SHA-256 values for the three
edited mirrors; those values are synchronization evidence only and never
runtime authority.

<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:END -->

## Launcher-v2 successor-producer authority-zero preregistration (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:BEGIN -->

The exact no-image preflight result is now consumed by a separate canonical
preregistration at SHA-256
`576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec`
and 20,145 bytes. It binds 23 live inputs, including the three directly executed
measurement/read-back helpers, and freezes 11 new-generation names
for producer v3, readback v3, the repeatable free rehearsal and any later
separately authorised production.

The rehearsal may retain one canonical JSON result only and must create no
production directory, attempt mark or image output. Readback v3 must consume
source-lock v2 and launcher-result v2, reject launcher v1 and refuse binding
overrides before loop setup or mount. Historical producer bytes and declared
test-gate drift remain unchanged. The reader retains
`ro,nodev,noexec,nosuid`, cleans mount and loop state in `finally`, and cannot
promote a failed diagnostic. A future authority must bind this record, the
free-rehearsal result and a fingerprint of the exact seven generation files.
The fingerprint binds this preregistration but not future authority bytes, so
the chain stays acyclic. This record grants no run.

The 2026-08-30f/2026-08-30j mirror synchronization records the same cursor in
`todo-l1-network-master.md`, `EXECUTION-ORDER.md` and
`thesis-realization-roadmap.md`. Those mirror hashes are synchronization
evidence only. Image-production and boot runs remain zero, and MAC.4, testnet,
mining, reward, consensus, P2P and activation remain unopened.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:END -->

## Launcher-v2 successor producer import-closure correction addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:BEGIN -->

The historical preregistration remains byte-preserved. An append-only,
authority-zero correction expands its 23 direct bindings to an effective union
of 41 after recursive static analysis and a fresh-process import/open audit
found 18 live omissions. Future producer/readback code must validate both
records before importing repository Python. Three raw legacy sources remain
transitively protected by digest-checking projected wrappers and are listed in
the correction rather than treated as unexplained reads.

The correction performs and permits zero image-production and boot runs. It
does not open MAC.4, testnet, mining, reward, consensus, P2P or activation.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:END -->

## Launcher-v2 successor S3-B authority-zero implementation addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:BEGIN -->

The successor producer/readback generation is implemented behind the corrected
41-file import-time trust boundary. Its manual arm64 workflow exposes a
repeatable JSON-only rehearsal and a production command that must fail before
effects while authority is zero. An isolated verify-only edge runs before all
repository Python in the workflow, and all later Python startup is isolated as
well. Readback v3 exists with fixed bindings, open-file-descriptor pinning
across loop setup and verification, private staging plus post-write output
revalidation before PASS publication, exact promotion-document validation and
fail-closed cleanup, but its wrapper call is only a future declared edge and
was not executed here.

No image was produced or read, no guest booted, and MAC.4, testnet, mining,
reward, consensus, P2P and activation remain unopened.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:END -->

## Launcher-v2 successor free-rehearsal result and F5 fingerprint addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:BEGIN -->

The authority-zero v3 rehearsal ran once on required native Linux arm64 after
the implementation merged.  Production was skipped and one canonical JSON
member was uploaded and tracked without a metadata wrapper.

```text
sourceHeadSha=0649dbc92a228fb67350a7eef864a9c9c612fd3d
sourceWorkflow=.github/workflows/native-shadow-successor-produce-arm64-v3.yml
sourceEvent=workflow_dispatch
sourceRunId=33281151298
sourceRunAttempt=1
sourceJobId=99176428509
artifactId=9723056242
artifactName=launcher-v2-successor-v3-free-rehearsal
archiveSizeBytes=3424
archiveDigest=sha256:a3f6e9c5c9a79712fab1b4454b9401325f543632d5f5f632e3e34e843974b2ef
artifactMemberCount=1
artifactMemberName=REHEARSAL-RESULT.json
payloadSizeBytes=10168
payloadSha256=d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c
productionGuardJobConclusion=skipped
evidenceClass=AUTHORITY-ZERO-STAGING-EVIDENCE
offlineClaim=false
runnerGlobalTransientAbsenceClaim=false
imageProductionClaim=false
bootClaim=false
mac4Claim=false
```

The raw result binds 41 live inputs, exact staging measurements and zero image,
marker or production-output effects.  Dependency acquisition used the network,
and the record says nothing about unnamed runner-global transient files.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V5-SEALED -->

The 5,458-byte F5 record at SHA-256
`6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa`
seals the seven v3 generation files and P1/C1/R1 as historical authority-zero
staging evidence.  Readback v3 was not run by the rehearsal, and no image,
boot, serving or MAC.4 boundary is opened.

## Production-only successor generation P2 preregistration addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN -->

The 8,156-byte P2 record at SHA-256
`4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95`
binds R1 and F5 while keeping F5 as historical authority-zero evidence only.
The unused A5 and result-v5 reservations were never created or granted and are
withdrawn for production; their paths must remain absent and cannot be reused.

A distinct producer-generation v4 must use five new production-facing files,
pin the proved v3 staging function and readback-v3 boundary, and produce a
fresh authority-zero R2 before F6 or A6 can be created.  All P2 run and
authority counters are zero.  No image, boot, MAC.4, testnet, mining, reward,
consensus or P2P boundary is opened.

The 2026-08-30g/2026-08-30k mirror synchronization appended the same R1,
F5 and P2 cursor to the master, execution and thesis mirrors.  Only those three
local files changed; all six mirror digests were recalculated, while the other
three tracked values remained byte-identical.  These hashes are synchronization
evidence only and grant no runtime authority.

## Production-dispatch fence correction addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-CORRECTION-ARM64-V1-FROZEN -->

One historical paragraph reports P2 as 8,096 bytes.  The preserved P2 file is
exactly **8,156 bytes** at SHA-256
`4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95`;
it was not rewritten.  The append-only correction is 7,295 bytes at SHA-256
`16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b`.

The correction also records that `workflowDispatchesAllowed=1` alone cannot
prove global one-use dispatch.  Runner-local markers, artifacts, caches,
result-v6 absence, concurrency and human promises are explicitly rejected as
substitutes.  Only the production guard may have job-level `contents: write`.
It must require first run-attempt, then atomically create the fixed
attempt-specific annotated tag ref whose canonical message binds A6 digest,
attempt ID, GitHub run ID, workflow and head SHA.  Existing, forced, updated,
deleted or reused refs are hard stops.  Creation of that ref consumes the run.

Every replica revalidates the repository ref, tag message, checked-out head and
live A6 before dependencies or effects.  R2, F6 and A6 must directly bind the
correction in an exact `productionDispatchFenceCorrection` field containing
only path, SHA-256 and size.  This authority-zero record created no ref and ran
no V4, R2, F6, A6, result-v6, image production or boot.  MAC.4, testnet,
mining, reward, consensus, P2P and activation remain closed.

## Producer v4 implementation addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:BEGIN -->

Producer generation v4 now implements the exact five paths that P2 named.  Its
current executable boundary is a repeatable Linux-arm64, authority-zero
rehearsal that can retain only one canonical JSON result.  It acquires no image
writer and cannot create kernel, initrd, root disk, attempt marker or boot
evidence.

The later production boundary is implemented behind the append-only dispatch
fence correction: create-once annotated tag, live A6/head/context revalidation,
claim-bound production and cleanup supervisors, sealed logical provenance,
seven-day qualified artifact retention and provenance-first byte comparison.
The required Linux containment job crashes a disposable outer supervisor and
requires its separate cleanup supervisor to remove all three independently
cgroup-contained inner services.  This is unexecuted production code plus a
no-image lifecycle integration gate, not a production claim.  R2, F6, A6 and
result-v6 are absent, so no claim tag or image can be created.  MAC.4, node
connection, testnet, mining, reward, consensus, P2P and activation remain
unopened.

The 2026-08-30h/2026-08-30l mirror synchronization appends this same v4/R2
cursor to the master, execution and thesis mirrors.  All six mirror hashes in
section 12 were recalculated; the other three mirrors remain byte-identical.
Those hashes record planning synchronization only and grant no run authority.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:END -->

## Failed free R2 rehearsal history addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-ARM64-V2-SEALED:BEGIN -->

The first two authority-zero v4 rehearsal dispatches failed before a canonical
R2 result existed.  Their separate append-only hard-stop record is
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v2.json`,
8,120 bytes at SHA-256
`7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb`.
The successful R2 path remains unoccupied by this record.

```text
sourceRunId=33311411461
sourceRunId=33313895353
artifactsUploadedByTheseAttempts=0
successfulR2ResultsCreatedByTheseAttempts=0
productionGuardJobs=skipped
imageProductionClaim=false
bootClaim=false
```

The first dispatch stopped when systemd rejected its address-family property;
the second started systemd but lacked the preregistered generation binding in
its direct program bootstrap.  Later changes repair both defects.
R2 remains unsealed by these two attempts.  Dependency and toolchain
preparation did occur, so this is not a network-zero or complete-cleanup
claim.  It grants no A6, production, image, boot, MAC.4, testnet, mining,
reward, consensus, P2P or activation authority.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-ARM64-V2-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-ARM64-V3-SEALED:BEGIN -->

### 2026-08-30: third free v4 rehearsal failed before R2

The third authority-zero rehearsal reached the claim-bound systemd service,
then stopped before creating R2 because the disposable OCI scratch extractor
treated the guest-root link `etc/rmt -> /usr/sbin/rmt` as a host escape.  The
failure is sealed separately at
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v3.json`,
5,028 bytes at SHA-256
`3cfe5cb9df41c15206e3ca56d5224c7b5e03ebb0a118d8a49fd9b4154bc86e07`.

```text
sourceRunId=33316130780
freeRehearsalJobId=99269811610
sourceHeadSha=8dc57c531b01e4b2b72864969eddfdeaeb6cda5a
exactFailure=native-shadow successor producer v4: FAIL: verified layer 'etc/rmt' link escapes
artifactsUploadedByThisAttempt=0
successfulR2ResultsCreatedByThisAttempt=0
productionGuardJob=skipped
produceJob=skipped
compareJob=skipped
```

This record is not R2 and does not occupy the successful R2 path.  It makes no
offline, runner-global-cleanup, image, boot or serving claim and grants no A6,
production, MAC.4, testnet, mining, reward, consensus, P2P or activation
authority.  The earlier two-attempt record remains byte-unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-ARM64-V3-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-SCOPE-CORRECTION-ARM64-V1-SEALED:BEGIN -->

### 2026-08-30: append-only scope correction for the third failed rehearsal

The historical v3 failure record remains byte-unchanged.  Its later correction
is `native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-correction-arm64-v1.json`,
2,837 bytes at SHA-256
`88a7fc38963f48fa42018ba7e29ab5648f6767f7cecaac66d1aa4e7047c292c8`.
The service was a root-owned HEAD-bound rehearsal systemd service, not a
production-claim-bound service.  A transient OCI scratch layout existed before
the extractor rejection; only the final guest image outputs remained at zero.
No runner-global cleanup, R2, production, image, boot or activation claim is
created by this correction.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-THIRD-FAILED-ATTEMPT-SCOPE-CORRECTION-ARM64-V1-SEALED:END -->

## Fourth free R2 rehearsal hard-stop addendum (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-ARM64-V4-SEALED:BEGIN -->

The fourth authority-zero rehearsal reached the root-owned HEAD-bound systemd
service, then stopped because its content store held the sealed guest closure
but not the separately sealed ext4 writer packages.  The append-only record is
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v4.json`,
6,147 bytes at SHA-256
`96721d93d6016a6ee9c8714672ee9e49c0672336181bc1ef8082ab5445081eae`.

```text
sourceRunId=33319199252
freeRehearsalJobId=99278062868
sourceHeadSha=0029b3df45b87a2f2643abfff0f30f57f0c46d48
artifactsUploadedByThisAttempt=0
finalGuestImageOutputsCreatedByThisAttempt=0
successfulR2ResultsCreatedByThisAttempt=0
productionGuardJob=skipped
produceJob=skipped
compareJob=skipped
```

The missing objects were the already-pinned `e2fsprogs` and
`libext2fs2t64` writer packages; production already acquired them, while the
free rehearsal did not.  This is a rehearsal wiring omission, not a change to
the sealed guest closure.  A transient OCI scratch layout existed, so this is
not a runner-global cleanup claim.  The record is not R2, occupies no successful
R2 path and grants no A6, production, image, boot, MAC.4, testnet, mining,
reward, consensus, P2P or activation authority.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-ARM64-V4-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-WORDING-CORRECTION-ARM64-V2-SEALED:BEGIN -->

### Append-only scope correction for the fourth rehearsal

The v4 failure record remains byte-unchanged.  Its correction is
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-correction-arm64-v2.json`,
3,043 bytes at SHA-256
`b0f140161df0029eec5359a25d2ec6a207511d6787fa7a9000de997a95b90177`.
The production path was wired to invoke the writer-set acquirer; production
execution is not claimed and no production run or claim tag was observed.  The
recorded timestamps belong to the free-rehearsal job.  The log directly showed
the missing `e2fsprogs` object; omission of the complete two-package writer set
is a static derivation from the exact acquisition plans.  This correction
grants no R2, A6, production, image, boot or activation authority.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FOURTH-FAILED-ATTEMPT-WORDING-CORRECTION-ARM64-V2-SEALED:END -->

## Fresh authority-zero v4 rehearsal result (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-SUCCESS-ARM64-V2-SEALED:BEGIN -->

The exact v4 bytes on main
`05ebf22e220bcece9e3104238f8ae8e2fef02a3e` completed fresh Linux-arm64
run `33321624511`.  The tracked raw R2 payload is 6,928 bytes, SHA-256
`7efe89c3bc558455313b76de2a625e708a580d0256760692914e9474eb0171f0`,
and says `PASS-NO-IMAGE-PRODUCED`.  Its production guard, production and
comparison jobs were skipped; authority and effect counters remain strictly
false or zero.  Dependency acquisition was networked, so no offline claim is
made.  The four earlier failures and append-only scope corrections remain
independent historical records.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-SUCCESS-ARM64-V2-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-ARTIFACT-PROVENANCE-ARM64-V2-SEALED:BEGIN -->

Transport facts live in a separate 3,181-byte provenance record at SHA-256
`6d569cdf8c875d0835df64d38aacd5d7e69cb1f44e2b2eb9bea550d59b12707d`.
It binds run `33321624511`, free job `99284488125`, artifact `9735090846`,
the three skipped production jobs, the API ZIP digest and the direct sole
member digest.  The downloaded ZIP was exactly 1,901 bytes at SHA-256
`31a090eebb44b63d431d06300a8bffa24aa55f8d26cc035e40bc44110fc92dfe`;
its only member was safe `R2-RESULT.json`, byte-identical to the tracked raw
payload.  The observation is frozen rather than presented as a perpetual API
recheck after artifact expiry.

```text
R2 GREEN / F6 NEXT / A6 NOT CREATED
PRODUCTION CLAIM TAG / IMAGE / BOOT  NOT CREATED OR RUN
```

R2 opens no production, boot, serving, MAC.4, testnet, mining, reward,
consensus, P2P or activation boundary.  F6 is the next authority-zero binder.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-ARTIFACT-PROVENANCE-ARM64-V2-SEALED:END -->

## F6 authority-zero producer fingerprint after fresh R2 (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V6-SEALED -->

Fresh R2 permits an identity seal, not production.  F6 is the canonical
3,250-byte record at SHA-256
`0e98b02f2dc8c4752c282dba57e1aa39d1cdc62a83c57d8803d6051ea792c183`.
It binds P2, R1, F5 and raw R2, followed by the five exact v4 generation files,
the dispatch-fence correction and the successful R2 gate.  The separate
GitHub artifact provenance is not promoted into the production authority
chain.

```text
R2 GREEN / F6 SEALED / A6 NOT CREATED / PRODUCTION AND BOOT NOT RUN
```

All F6 authority and run counts remain false or zero.  A6 and result-v6 remain
absent; the global claim tag, image production, guest boot, MAC.4, testnet,
mining, reward, consensus, P2P and activation paths remain unexecuted.

The 2026-08-30i/2026-08-30m local mirror synchronization appends the same
R2-green/F6-sealed cursor to the Master Plan, Execution and thesis roadmap.
Their byte digests are updated in section 12.  The ADR, verified thesis and
encyclopedia mirrors remain byte-unchanged.  These hashes are synchronization
evidence only and grant no runtime authority.

## Main-only production-dispatch fence correction (2026-08-31)

<!-- LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-ARM64-V1-SEALED:BEGIN -->

Review before A6 found that sealed generation v4 checks the workflow path by
prefix but does not require the selected ref to be main.  A feature or tag ref
could therefore pass the historical guard and reach global
claim creation before an authority was merged to main.  A6-v6 is not created.

The canonical 13,335-byte correction at SHA-256
`63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b`
binds the live P2/P3/R1/F5/R2/F6 records and the exact five historical v4
files.  It preserves them as authority-zero evidence, withdraws the unused
A6/result-v6 reservation and preregisters producer-generation v5.  The new
generation must require the exact `workflow_dispatch` event, a dispatch-only
trigger set, the exact main dispatch ref and exact main workflow ref before all
effects, then obtain a fresh authority-zero R3 before F7 or A7.

The successor may not copy the retired A6 namespace.  P4 fixes an A7 tag
template, dispatch-claim v2 schema, v7 authority/result paths and schemas, and
the generic `authoritySha256` field.  The canonical tag message must carry the
exact event name, dispatch ref, workflow ref and run attempt, and every
consumer must compare them again with the live workflow context.  V5 also gets
fresh root, recovery, artifact and internal-schema names.  R3, F7, A7 and the
v7 result must contain the direct exact P4 identity under
`mainBranchDispatchFenceCorrection`; transitive binding does not satisfy this
contract.

```text
R2 GREEN / F6 HISTORICAL / A6-V6 WITHDRAWN UNUSED
MAIN-ONLY PRODUCER V5  PREREGISTERED / NOT IMPLEMENTED
PRODUCTION CLAIM TAG / IMAGE / BOOT  NOT CREATED OR RUN
```

Atomic create is claimed only while the tag exists.  No unobserved server-side
ruleset is used to call administrator deletion impossible.  The correction
opens no production, boot, MAC.4, testnet, mining, reward, consensus, P2P or
activation authority.

<!-- LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-ARM64-V1-SEALED:END -->
