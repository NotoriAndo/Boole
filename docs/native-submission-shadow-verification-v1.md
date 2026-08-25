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
| `local-docs/todo/todo-l1-network-master.md` | `b4f019c523c281aca7229f3abfa1458f7acbc8fd68006b9c56cc39997033610f` (updated 2026-08-25g — CURL.2-TRANSPORT GREEN, fail-closed download + curl entrypoint) |
| `local-docs/todo/EXECUTION-ORDER.md` | `fc2ed77a6a6e6b4cb69ce21a8ada8abd23d81bff32ee69342e94febc0dac4998` (updated 2026-08-25g — cursor advances to CURL.3) |
| `local-docs/verified-reasoning-substrate-thesis-2026-06-10.md` | `8c520a79bb6a26ef684d866928498fbd9abe456e0a99f072a430033d1ca2a76e` |
| `local-docs/todo/thesis-realization-roadmap.md` | `b365d7998f9930c17c28c53bdc3111d547583ccb6d1aa511c2ae773725435b74` (updated 2026-08-25g — CURL.2-TRANSPORT transport path realized in code) |
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
