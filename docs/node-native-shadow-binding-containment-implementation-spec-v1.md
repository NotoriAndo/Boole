# Node-native shadow binding and containment — consolidated implementation spec v1

Status: **CLOSED-LOCAL IMPLEMENTATION GREEN.** Registry/state durability, named-Linux containment,
actual checker execution and the feature-gated loopback HTTP route are on `main`. Production
activation, P2P, block, reward and consensus remain forbidden.

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
* **Phase 3B.1** — PR #174, main `ac4f32f`: a named `ubuntu-24.04`
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
  ([final run 32598803995, job 97093814188](https://github.com/NotoriAndo/Boole/actions/runs/32598803995/job/97093814188)).
  Required `self-test` fails when this job fails, is skipped or is cancelled. This GREEN proves only
  that the named runner supplies the launcher
  prerequisites; it does not implement the production launcher/IPC, execute the native checker or
  close containment.
* **Phase 3B.2a** — PR #175, main `8a773fe`: the exact tracked bytes at
  `native/containment/native-shadow-execution-policy-v1.json` pin the Linux platform, service
  accounts and their resolution invariants, installation ownership/modes, closed Unix-socket
  message schemas, framing and size ceilings, the exact five Linux capability sets, cgroup/tmpfs/
  rlimit values, native seccomp/Landlock profiles, checker invocation and crash-cleanup ownership.
  `native-shadow-toolchain-identity-v1.json` separately pins the Rust archive/commit authority and
  the qualification interpreter compatibility probe, while explicitly leaving installed-byte
  provenance closed to execution. The
  qualification registry binds both raw files. This slice froze authority only: no launcher
  binary, socket, child process, checker, journal transition or route was created. The required
  named-Linux, self-test and supply-chain jobs passed in CI run `32603937417` before merge.
* **Phase 3B.2b-0** — PR #176, main `1b02592`: the minimal
  `boole-native-shadow-protocol` crate embeds the three exact tracked authority files, rejects
  byte differences before interpreting installed authority, rejects duplicate keys and
  floating-point JSON, strictly rejects unknown or missing fields in every registry and
  qualification hello/ready message, keeps public verified/message objects non-deserializable and
  non-forgeable outside the crate, and provides the fixed four-byte big-endian framed
  JSON caps. `boole-node` replaces its repository-fixture production path with the literal installed
  `/usr/share/boole/native-shadow/registry-v1.json` path and converts the full strict registry into
  its lifecycle projection only after exact-byte and cross-digest verification. It creates no
  socket, launcher, nonce, child,
  checker result, journal transition or route.
* **Phase 3B.2b-1** — PR #177, main `a5b830b`: the guarded node-side behavioral-mock slice owns one
  mock session so an error cannot leave a reusable connection, obtains mock peer credentials before
  stream I/O, sends one strict qualification hello from a test-injected 32-byte nonce and the
  verified three-authority bundle, then accepts readiness only after nonce, all three digests, peer
  PID and all six launcher/node/checker UID/GID values match. Success additionally requires node
  shutdown-write followed by clean peer EOF; premature/partial/oversized input, a second frame,
  trailing bytes or either shutdown failure remains an error. The result is a private,
  non-serializable in-memory readiness value with no lifecycle, journal, route or execution handle.
  Required CI passed before merge. It does **not** open a Unix
  socket, call `SO_PEERCRED` or `getrandom(2)`, resolve accounts, start a launcher/child, or prove a
  real Linux handshake; those remain Phase 3B.2b-2 work.
* **Phase 3B.2b-2p** — PR #178, main `fb73679`: one shared Unix
  entrypoint accepts no caller path and walks the literal
  `/usr/share/boole/native-shadow` hierarchy one component at a time relative to already-opened
  directory descriptors. Every component is opened with `O_NOFOLLOW`; `/` and all ancestors must
  be root-owned and not group/other-writable, the authority directory must be exactly mode `0555`,
  and all three authority files must be root-owned regular one-link files of exact mode `0444` and
  compiled byte length. The same opened file descriptors are checked before and after reading, then
  the existing exact-byte/schema/digest verifier is applied. This is only a shared opener for the
  later node and launcher. Required CI passed before merge. It does not connect a socket, generate a nonce, resolve accounts, launch
  a process, emit readiness or change lifecycle/journal state, and therefore does not close
  Phase 3B.2b-2.
* **Phase 3B.2b-2n** — PR #179, main `434534e`: the private production
  entrypoint accepts no socket path, opens the installed authority through Phase 3B.2b-2p, connects
  only to `/run/boole/native-shadow/launcher.sock` with a bounded nonblocking-connect/poll/
  `SO_ERROR` sequence, obtains exactly 32 bytes from one `getrandom(2)` call with flags zero and no
  fallback, and authenticates the peer through `SO_PEERCRED` before the Phase 3B.2b-1 core writes a
  frame. Every read, write, flush and shutdown uses the remainder of one five-second handshake
  deadline rather than resetting a per-operation timeout. The fixed socket path, one-second connect
  limit, five-second handshake limit and nonce contract are mechanically checked against the tracked
  policy bytes. Non-Linux hosts refuse before filesystem or socket work. This remains preparation:
  expected service IDs are not yet obtained from the fixed-account resolver and there is no root
  launcher, real installed-path happy path, route, journal mutation or checker execution, so no real
  handshake GREEN is claimed.
* **Phase 3B.2b-2i** — PR #180, main `91de1d1`: one path- and argument-free
  shared resolver for only `boole-node` and `boole-native-checker`. It uses `getpwnam_r`,
  `getgrnam_r`, `getgrgid_r` and `getgrouplist`, verifies the complete account/group profile frozen
  below, and returns a field-private, non-serializable UID/GID view. The node production adapter now
  performs that independent NSS resolution itself instead of accepting caller-selected numeric
  identities. A named Ubuntu step creates the exact ephemeral accounts and executes the real libc
  success path; fake providers cover fail-closed contract branches without mutating the host.
  This remains a preflight-only slice: it adds no root launcher, lock, socket bind, cgroup recovery,
  journal transition or checker execution.
* **Phase 3B.2b-2l** — PR #181, main `89a4aa2`: a separate, non-publishable
  launcher library with only the disabled request-free qualification exchange. An owned, sealed
  session must authenticate the fixed node PID/UID/GID before reading; the core then reads one
  strict hello, matches the policy/toolchain/registry digests, builds ready solely from an opaque
  verified-startup token, flushes it, requires clean node EOF and finally shuts down its write half.
  The startup token and peer credentials have no public constructors, so production use stays
  impossible until a later in-crate Linux runtime proves root identity, installed authority, fixed
  NSS identities, a fresh launcher-instance ID and zero-leaf recovery. The library has no binary,
  listener, lock, cgroup operation, execution/report message, route, journal mutation or checker
  spawn; actual handshake GREEN remains open.
* **Phase 3B.2b-2u** — PR #182, main `6164f32`: the connected-Unix-session slice consumes one already-connected
  Linux `UnixStream`, obtains the node PID/UID/GID from kernel `SO_PEERCRED` before frame I/O, and
  gives every peer check, read, write, flush and `SHUT_WR` operation only the remainder of one
  cumulative five-second deadline. It accepts no timeout, path or numeric identity from its caller.
  A named Ubuntu gate runs the real kernel socketpair and half-close path; focused tests separately
  prove that a wrong kernel peer leaves an already queued hello unread and that the timeout and
  peer-order constants match the tracked policy. Malformed-frame, authority-binding and ready-field
  matrices remain in the shared protocol and behavioral core rather than being repeated here. This
  slice still has no startup-token constructor, launcher executable, listener, bind/stale-unlink,
  lifetime lock, cgroup recovery, route, journal transition or checker child, so it cannot make the
  installed handshake ready by itself.
* **Phase 3B.2b-2r** — PR #183, main `b582869`: the launcher-privilege slice adds one argument-free, Linux-only
  startup prerequisite. It reads the calling thread's kernel status from the fixed
  `/proc/thread-self/status` path and issues a private, thread-bound proof only when all four
  UID/GID slots are root, Effective/Permitted/Bounding are exactly the frozen four-capability mask,
  Inheritable/Ambient are empty and `NoNewPrivs` is still zero. Missing, repeated, malformed, extra
  or reduced values fail closed. A named Ubuntu gate executes the same production verifier in three
  transient root services: exact capabilities must pass, while one missing capability and one
  extra capability must both fail. This slice does not consume that proof into the startup token,
  open installed authority, acquire the launcher lock, recover cgroups, bind/listen, construct a
  launcher instance ID, mutate a journal/route or spawn a checker.
* **Phase 3B.2b-2s** — PR #184, main `4a07015`: the pre-lock-composition slice consumes the privilege proof, opens the
  fixed installed authority and resolves the fixed NSS identities, exactly once in that order,
  into an opaque thread-bound prerequisite. The entrypoint accepts no path, account name, numeric
  identity, policy or capability mask; a failed earlier stage prevents later filesystem/NSS work.
  A named Ubuntu gate stages byte-identical root-owned authority behind a private read-only
  `/usr/share` bind and invokes the production composition under the exact capability service and
  fixed accounts. The first hosted-runner attempt correctly rejected unsafe host `/usr/share`
  metadata; the strict successor passed the real Linux gate and required CI run `32614207172`.
  The resulting proof is not
  `VerifiedQualificationStartup`: this slice does not acquire the launcher lock, generate an
  instance ID, recover cgroups, bind/listen, mutate a journal/route or spawn a checker.
* **Phase 3B.2b-2t** — PR #185, main `a517720`: the lifetime-lock slice consumes that opaque prerequisite and accepts
  no path or numeric identity. It opens the fixed runtime hierarchy component-by-component relative
  to verified directory descriptors, rejects symlinks and unsafe ownership/modes, opens only
  `launcher.lock` with the frozen flags, validates its exact inode metadata before locking, and
  holds one nonblocking exclusive `flock` until the opaque guard drops without unlinking the file.
  The named Linux gate proves cross-process busy behavior and same-inode reacquisition under the
  exact launcher capabilities and NSS identities; required CI run `32615499137` is GREEN. This
  remains pre-readiness work: no launcher ID,
  cgroup recovery, bind/listen, route, journal transition or checker child is added.
* **Phase 3B.2b-2g** — PR #186, main `c1d74f9`: the instance-identity slice consumes the lifetime-lock guard, performs
  exactly one 32-byte `getrandom(2)` call with flags zero and no caller bytes, retry or fallback, and
  stores the result only inside a new opaque thread-bound token that continues to own the lock.
  Failure or any short read issues no token. The named Linux gate executes that production path
  and retains the lock across the call. This state-free step is deliberately after the lock and before
  recovery, but it is not readiness: manager-cgroup verification, orphan cleanup, fixed probes,
  bind/listen, route, journal transition and checker work remain absent.
* **Phase 3B.2b-2h** — the tracked deployment-envelope slice adds the exact production
  `boole-native-shadow-launcher.service` unit plus `sysusers.d` and `tmpfiles.d` inputs. The unit
  fixes the already-frozen root identity, `system.slice`, delegated cpu/memory/pids boundary, four-capability
  ceiling, empty ambient set, private mount namespace, umask, bounded restart loop, whole-control-
  group stop behavior and no service-level task/memory ceiling that could preempt per-run cgroups.
  Provisioning creates the two distinct non-root service identities and the root:`boole-node`
  mode-`2750` runtime directory. The named Linux job asks systemd's own parsers/provisioners to
  materialize those tracked bytes in an alternate root. This slice does not start the installed
  service and does not implement manager movement, controller enablement, orphan recovery,
  readiness, bind/listen, route, journal transition or checker execution.
* **Phase 3B.2b-2m** — the manager-cgroup slice adds one path- and policy-input-free entrypoint that
  consumes the opaque launcher instance. It proves the process has exactly one thread, traverses the
  fixed systemd cgroup root component-by-component with no-follow directory descriptors, verifies
  cgroup2fs, creates or safely reuses the exact empty `manager` child with exact-empty
  `cgroup.subtree_control`, moves the current process, then verifies exact root/manager membership,
  exact-domain manager type, still-empty manager subtree control and exact cpu/memory/pids
  controller read-back.
  Failures before movement are `PreMove`; the move attempt and every later failure are
  `PostMoveFatal`, which a future top-level must turn into immediate process exit. The separate
  deployment-envelope gate independently proves the production unit bytes; this named Linux gate
  loads that exact tracked fragment plus one gate-owned drop-in that only binds a private,
  read-only, byte-identical authority tree over `/usr/share`; it never repairs or writes the hosted
  runner's unsafe host `/usr/share`. It proves manager create/reuse, frozen/nested-child and
  multithread rejection, restart and stop against the real kernel, then removes only its owned
  unit, drop-in and authority tree. It does not scan or clean `run-*` leaves and therefore is not
  startup recovery or readiness.
* **Phase 3B.2b-2v** — the startup-orphan-recovery slice consumes the manager proof and has no
  caller-selected path, operation ID, timeout or policy. It uses one cumulative ten-second deadline,
  validates the complete direct-child inventory before any mutation, opens and validates every
  canonical `run-<64-lowercase-hex>` leaf before the first freeze, and cleans each leaf in canonical
  order through freeze confirmation, `cgroup.kill`, `populated=0`, empty process/thread files and
  descriptor-relative removal. It rechecks the leaf's device/inode identity immediately before
  removal and rescans the complete inventory plus service-root/manager invariants before returning
  an opaque recovery proof. The named Linux gate uses one live parent/child process tree, one frozen
  parent/child tree and one empty leaf, then separately injects an unexpected direct child and proves
  exact fail-closed inventory rejection without changing the valid live leaf's process/thread/event
  state. This closes only launcher-owned startup cgroup cleanup. Readiness, fixed toolchain probes,
  bind/listen, node durable recovery, route wiring and checker execution remain open.
* **Phase 3B.2b-2w** — the fixed startup-toolchain-compatibility slice consumes that opaque recovery
  proof and accepts no path, argument, environment, timeout or output limit from its caller. It
  verifies the fixed root-owned Rust/Cargo/Python executable path shapes, then runs exactly the four
  tracked manifest probes in order with a fresh ten-second deadline per probe, `env_clear`, the four
  tracked environment entries, cwd `/`, null stdin and independent 65,536-byte stdout/stderr caps.
  The toolchain root and `bin` directory are exactly root:root mode `0555`; Rust/Cargo executables
  are root-owned, owner-executable, non-special, non-group/other-writable regular files with exactly
  one hard link, and Python is the same shape with exact mode `0755`. The launcher snapshots their
  device/inode/metadata, opens each fixed executable without following the final component, executes
  that opened descriptor rather than looking the pathname up again, and requires the same pathname
  identities plus cgroup filesystem/manager metadata and inventory after all probes.
  Timeout, either-stream overflow, nonzero exit, nonempty stderr, invalid UTF-8, duplicate or drifted
  version fields and any later manager-cgroup drift are fatal after movement and issue no opaque
  compatibility proof. The limits are launcher safety constants frozen by this implementation spec;
  they do not change the separately digested toolchain-identity manifest. A named Linux gate stages
  the reviewed per-commit Rust artifacts at the exact `/opt` path and proves the full
  recovery-to-probe chain. This is compatibility-only: installed-byte provenance, readiness,
  bind/listen, route, durable node state and checker execution all remain closed.
* **Phase 3B.2b-2x** — PR #191, main `2ef2cc7`: the qualification-startup assembly slice consumes
  exactly one `VerifiedStartupToolchainCompatibility` and no caller-selected path, identity, PID, digest,
  timeout or activation value. Crate-private read-only forwarding accessors recover only values
  already bound inside the opaque chain, while the returned `VerifiedQualificationStartup` owns
  that complete chain so the lifetime `flock`, manager/root cgroup descriptors and recovery proof
  cannot drop before later socket work. A public API-shape test prevents an earlier token from
  substituting for the compatibility proof, and the impossible zero-PID branch has no retry or
  fallback. This does not bind/listen, emit a wire frame, mutate node state or spawn a checker;
  installed-byte provenance and every activation path remain closed.
* **Phase 3B.2b-2y** — the fixed one-shot listener consumes the complete 2x token by value and
  accepts no caller-selected path, mode, ownership or retry policy. Under the already verified
  root:`boole-node` setgid runtime directory and frozen umask it removes only an exact safe stale
  socket, binds the literal policy path, requires the new socket to be root:`boole-node` mode
  `0660`, verifies close-on-exec on the listener and accepted stream, accepts once, then removes
  only the recorded device/inode socket before the strict qualification exchange. A replaced path
  is preserved and fails closed, while the previously public connected-stream adapter is now
  crate-private so no caller can reuse an earlier token. Linux unit tests cover safe stale
  replacement and unsafe-entry preservation. The named Linux gate stages the reviewed launcher
  and node test binaries, runs the client as the real non-root `boole-node` service identity
  against independently installed authority bytes, and requires strict peer/digest/nonce/EOF
  success followed by launcher exit without restart, socket absence and service-cgroup removal.
  It does not accept a submission, execute the checker, write the journal or permit activation.

All listed phases are internal, currently unwired `boole-node` foundations or infrastructure
gates. They do
**not** implement an
HTTP endpoint, spawn the checker, activate the production registry, change SharePool/block/reward/
P2P/consensus state, or earn `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN`. Phase 3A.1's focused
lock test uses two opens in one process and does not close the later real two-node-process gate.
Still open are section 7's containment-backed per-record cleanup, section 8's AppState ownership and
permit acquisition in the actual request path, sections 9–10's actual Linux
containment/observation integration, route wiring, the complete RED matrix and one real
node-process raw-answer run. The named Linux job has now proved the infrastructure prerequisites,
but actual containment GREEN remains blocked until the now-pinned launcher protocol, identity,
policy and privilege contracts are implemented and exercised. Generic
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
2. Replay the durable journal to reconstruct current per-key state. Every row found `InFlight`
   without a matching `TerminalConsumed` event remains withheld; the unprivileged node does **not**
   write cgroupfs or claim that it cleaned a launcher's process tree.
3. Start the privileged launcher under its separate single-instance lock. Before binding the socket
   or sending readiness, the launcher moves itself into the exact `manager` subgroup, scans every
   direct child of its delegated service root, rejects any child other than `manager` or an exact
   `run-*` leaf, and freezes, kills, confirms empty, and removes every `run-*` leaf. Any unexpected
   child or incomplete cleanup fails closed without readiness.
4. Complete the authenticated request-free qualification handshake. The ready frame must bind the
   installed policy/toolchain/registry digests and nonce and assert, as typed fields,
   `startupRecoveryComplete=true`, `activeExecutionLeaves=0`,
   `unexpectedDirectCgroupChildren=0`, and `managerSubgroupVerified=true`. The node verifies root
   `SO_PEERCRED` UID/GID/PID and holds the route closed and its ledger lock throughout this barrier.
5. Only after step 4 succeeds may the node resolve each withheld row **one at a time**. If durable
   evidence already exists, append the missing terminal transition; otherwise append the rollback
   to `Active(fresh)`. A failed append leaves that row withheld. If the authenticated barrier never
   arrives, no row is rolled back and the route never opens. Then bootstrap registry-declared keys
   with no durable record under section 6's ordered static-issuability checks. Only after every row
   is resolved may an activation-capable future release serve requests; this qualification release
   remains route-disabled regardless.

Step 5 is the future activation-capable recovery rule, not work authorized by the next qualification
slice. The frozen qualification handshake has `nodeDurableStateChangeAllowed=false`; Phase 3B.2b
must stop after authenticating and recording the in-memory readiness barrier and must not append a
journal rollback, bootstrap a row, or open the route. A later explicitly activation-capable policy
and implementation must be reviewed before step 5 can mutate durable state.

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

**Generalized recovery never grants the node cgroup authority (closes F4's follow-on).** A live node
that encounters a stale `InFlight` row while its execution try-lock is free cannot clean or roll it
back on its own. It leaves the row withheld and requires the same authenticated launcher recovery
barrier above (normally by failing the route closed and restarting the launcher qualification
sequence). Only after that barrier may it apply this per-record branch:

* If durable evidence for this four-tuple already exists but the terminal-state write never
  completed (the second partial-failure mode above), recovery **completes the terminal-state write
  directly** — it does not revert to `Active(fresh)` and does not re-invoke the checker, since a
  real, decided verdict already exists and evidence must never be produced twice for one outcome.
* If no evidence exists for this four-tuple, recovery reverts the row to `Active(fresh)` only after
  the authenticated barrier has established that no execution leaf remains.

This is also the mechanism that resolves what happens when the registry file changes on disk while a
challenge is `InFlight` (F4): because section 4 now looks a row up by its stable four-tuple identity
first, a submission arriving after such a change finds the *same* existing row rather than
bootstrapping a parallel one; if that row is `InFlight`, the barrier-gated procedure above governs
it, and if an execution still holds the try-lock, the arriving submission is simply rejected
`RetryableUnavailable(native_busy)` by section 8's existing rule, never granted a parallel track.

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
7. **`cgroup.freeze` + `cgroup.kill` is the launcher's only submission-verdict termination path**,
   with no iterative SIGKILL fallback; a kernel lacking `cgroup.kill` is a startup
   capability-probe failure, fail closed. A systemd unit stop is an outer crash-equivalent service
   shutdown: it emits no report and the next process must complete startup recovery before ready.

The leaf cgroup is assigned to the spawned process race-free at creation only through `clone3()`'s
`CLONE_INTO_CGROUP`; post-fork writes to `cgroup.procs` are not a fallback. macOS has no
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

The cgroup-level values below belong to a separate, node-owned containment-policy bundle. Phase
3B.2a freezes its exact tracked bytes at
`native/containment/native-shadow-execution-policy-v1.json`; their raw-byte SHA-256 is
`8806708be7c624b202e7ef8a88bfbd1d99fbb78064442d1c0f1baad6252e90c2`. The registry's top-level
`executionPolicySha256` binds that digest, and the same value is the `executionPolicyDigest` bound
independently through every new state row, journal event and v2 evidence object. The checker-owned
`policy.json` remains byte-preserved and keeps its separate `policyDigest` meaning.

The bundle is deliberately `activationAllowed: false`. Freezing it neither installs nor starts a
launcher. The next slice must compile the same exact bytes into both node-side client and launcher,
recompute the raw SHA-256 independently, and refuse any request-, environment- or CWD-selected
policy path. A mismatch among compiled bytes, the root-owned installed copy and the registry digest
is a startup failure before a socket is accepted or a challenge state changes.

The separately tracked `native/containment/native-shadow-toolchain-identity-v1.json` has raw-byte
SHA-256 `29799eff53df764024de8f1020eac177b2ee936243d2ac52d192ba2423f0fa84`.
The registry's `toolchainIdentitySha256` binds it. It repeats the already-frozen rustc/Cargo commit
and archive hashes and fixes the qualification interpreter compatibility probe to root-owned CPython
3.12 at the non-symlink executable `/usr/bin/python3.12`; probe drift fails before socket bind and
again before execution. This is
not yet exact installed-runtime provenance: the manifest explicitly marks the installed Rust file
tree, Python interpreter/stdlib and system linker/runtime file manifests as activation blockers.
Until a later tracked provenance slice closes all three, execution remains forbidden even if the
version probes pass. Evidence `toolchainDigest` means this tracked compatibility/authority-manifest
digest, not a digest improvised from a request-selected channel name or a claim that unrecorded
installed bytes were reproduced.

The startup compatibility wrapper adds one independent ten-second deadline per command and separate
65,536-byte stdout and stderr ceilings. These are launcher-owned safety bounds frozen by this
implementation spec, not fields added retrospectively to the byte-preserved identity manifest.
Reaching any bound kills and reaps the fixed probe, withholds the compatibility proof and prevents
all later bind or readiness work.

Phase 3B.2b-0 (PR #176, main `1b02592`) closed the two node-side compatibility gaps originally
recorded here: production uses the literal installed
`/usr/share/boole/native-shadow/registry-v1.json` path, and the shared strict registry model requires
the top-level policy/toolchain bindings plus every per-template manifest/intake field before
explicitly projecting lifecycle fields. Recursive duplicate keys, floats, BOM, unknown and missing
registry fields fail before that projection; policy and toolchain files remain opaque exact-byte
authorities after their JSON syntax check. Phase 3B.2b-1 adds only the node-side exchange and binding
behavior against an owned mock session; it does not claim a real socket, kernel peer credentials,
nonce generation or launcher. The actual disabled handshake must still open and authenticate the
other two installed authority copies, compare all three exact bytes/digests and reach mutual EOF
without state change over the installed Unix socket. Phase 3B.2b-0 validates the registry's final
opened file descriptor; validation of every
ancestor as root-owned and non-writable remains part of that later installed-authority handshake.
The three installed-runtime provenance manifests above are a separate activation blocker and
also remain open.

**Dedicated identities, fixed without unsafe machine-global numeric assumptions.** The service
accounts are exactly `boole-node` and `boole-native-checker`, each with a same-named primary group.
Both have home `/nonexistent`, shell `/usr/sbin/nologin` or `/bin/false`, and zero supplementary
groups. Both accounts and groups must resolve through `getpwnam_r`, `getgrnam_r` and `getgrgid_r` at
launcher startup, each passwd primary GID must resolve back to the required same-named group, and
`getgrouplist` must return no supplementary group. They must resolve to non-root, mutually distinct
numeric UIDs and primary GIDs; those resolved numbers are immutable for that launcher lifetime and
all six launcher/node/checker UID/GID values are returned in the readiness/report record. Hard-coding a
distro-global number would create collisions with unrelated local accounts, so the concrete
contract is the fixed account names plus these mechanically checked resolution invariants, not one
unportable integer. A missing, root-valued, aliased or changing resolution fails closed.

**Installation and closed local IPC.** The launcher is root-owned mode `0755` at
`/usr/libexec/boole/boole-native-shadow-launcher`; the policy and toolchain-identity copies are
root-owned mode `0444` at `/usr/share/boole/native-shadow/execution-policy-v1.json` and
`/usr/share/boole/native-shadow/toolchain-identity-v1.json`, beneath a root-owned mode `0555`
authority directory. The registry and checker files are root:root regular nonsymlinks mode `0444`;
checker/fixture directories are root:root mode `0555`, fixture descendants are non-symlink authority
files mode `0444`, and the root:root mode-`0555` toolchain directory's individual content modes
remain blocked on the future provenance manifest. `/usr/bin/python3.12` is an actual root:root
mode-`0755` regular nonsymlink executable. The root-owned runtime
directory is `/run/boole/native-shadow`, group `boole-node`, mode `2750`: the setgid bit makes a
root-created socket inherit group `boole-node` without adding `CAP_CHOWN`. The launcher holds
`flock(LOCK_EX|LOCK_NB)` for its lifetime on
`/run/boole/native-shadow/launcher.lock`, root:`boole-node` mode `0600`, validates every ancestor and
the directory as root-owned/non-writable, opens that basename relative to the verified directory FD
with `O_CREAT|O_RDWR|O_CLOEXEC|O_NOFOLLOW`, and `fstat`s a regular, one-link,
root:`boole-node`, mode-`0600` inode before locking,
binds with umask `0117`, applies mode `0660`, and verifies by `lstat` that `launcher.sock` is a
root:`boole-node` socket. It may remove a stale entry only while holding that lock and only after the
same ownership/type check. The node can traverse and connect but cannot replace entries. The
preinstalled `/work` mountpoint is root:root mode `0555`; its temporary private-namespace mount is a
different per-execution contract below. IPC is one request per Unix
stream connection using a four-byte big-endian length followed by strict UTF-8 JSON. Both ends verify
kernel `SO_PEERCRED`; the launcher accepts only the resolved `boole-node` UID **and primary GID**, and
the node accepts only root UID **and GID**, with the peer PID bound to the report. The generic
root-owned-path rule applies only to the listed immutable launcher/authority/toolchain paths and the
unmounted `/work` installation point; the
runtime socket directory and socket use their separate root:`boole-node` ownership/mode contract, so
the two rules do not contradict each other. A fresh 32-byte nonce and a distinct
32-byte operation ID use lowercase hexadecimal encoding. Operation IDs come only from one successful
32-byte `getrandom(2)` read, with no fallback; an observed collision fails before child creation. No
HMAC is added (the node would know the
same key and could forge it), and no
signing-key lifecycle is invented for this synchronous local boundary. Unknown/missing/trailing,
oversized or truncated frames fail closed. The frozen limits are 16,384 raw-answer bytes, 131,072
request-frame bytes and 65,536 response-frame bytes. The extracted complete submission source also
has its own 16,384-byte outer ceiling; the checker separately retains its 8,192-byte patch-body
ceiling, so fixed scaffold bytes are not accidentally counted against the patch allowance. The
ordinary HTTP body cap is not used as a substitute for these narrower native limits.

**Closed message and checker-input contract.** An enabled execution connection has exactly this order:
`hello → ready → execute → node SHUT_WR/EOF → report → launcher SHUT_WR/EOF`. The node builds the
execute frame first; hello commits its exact byte length and a domain-separated SHA-256 over the
four-byte length prefix plus exact execute JSON payload. The launcher echoes that digest in ready,
then verifies the received execute frame byte-for-byte and observes EOF before spawning anything.
The submission digest preimage is unambiguous: after the UTF-8 domain prefix and NUL, family version
is length-prefixed UTF-8, template and challenge are each their 64 lowercase **ASCII hex bytes**
with a four-byte big-endian length, epoch is an unprefixed big-endian `u64`, and raw answer is
length-prefixed decoded UTF-8. The node accepts a report only after matching nonce, operation ID,
request/policy digests, the ready-frame launcher PID and kernel `SO_PEERCRED` PID, all resolved IDs
and final EOF; every report authority field must equal both the execute field and the independently
opened installed bytes. Duplicate JSON keys, floats/non-finite numbers,
unknown fields, a second execute frame or any bytes between/after expected frames are failures.

This qualification release is deliberately different because `activationAllowed` is false. Its
only successful connection is an authenticated readiness check with separate, request-free schemas:
`qualification-hello → qualification-ready(activationAllowed=false) → node SHUT_WR/EOF → launcher
SHUT_WR/EOF`. Those messages bind the nonce plus installed policy, toolchain-identity and registry
digests, but contain no execute-frame length or digest because no execute frame exists. Before that
ready frame, launcher-owned startup recovery must be complete; the typed frame also binds a fresh
32-byte launcher-instance ID obtained once from `getrandom(2)` at launcher startup with no fallback,
and asserts zero active leaves, zero unexpected direct cgroup children, and a verified
manager subgroup. That path changes
no durable challenge state, starts no untrusted checker or execution child and returns no report.
The launcher may, before socket bind, spawn only the exact root-owned Rust/Cargo/Python compatibility
probe commands named in the tracked toolchain-identity manifest. Those trusted fixed probes neither
receive submission bytes nor enter a challenge cgroup, and their failure withholds readiness. This
is the complete meaning of `launcherSpawnAllowed=false`; it does not contradict the manifest's
`runBeforeSocketBind` probe requirement. Receiving an execute frame under this release is a protocol
error that closes the connection without a report or untrusted spawn. This explicit path is what the
next handshake-only implementation can test; it does not pretend that a disabled release can execute
a checker.

The node, not the miner and not the launcher, runs the frozen
`RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1` extraction over the raw UTF-8 answer. The execute message
carries both base64-encoded raw-answer bytes and the extracted Rust source, with independent raw and
source SHA-256 bindings; the launcher verifies those byte digests but deliberately does not become a
second family parser. It resolves the four-tuple only against its root-owned registry, opens the
registry-selected task and anchor beneath the fixed fixture root with `openat2` beneath/no-symlink
constraints, verifies every authority digest, and writes the exact extracted source to
`/work/submission.rs`. The fixed argv is `/usr/bin/python3.12 -I -S <fixed checker.py> --task /work/task.json
--submission /work/submission.rs --toolchain-bin /opt/boole/native-checker-toolchain/bin
--scratch-root /work/scratch`, with cwd `/work`; request data, environment and cwd cannot select a
path. A request may reach execute only after a future `InFlightV3` journal record durably stores the
same operation ID. Phase 3B.2a adds no such record, so its disabled policy cannot spawn a child and
the next implementation slice is handshake-only. Parsed checker stdout must use the checker's actual
`boole.native-shadow.checker-result.v1` schema and its verdict-specific frozen reason-code set; a
similarly named schema or unknown reason is not accepted. A report exists only after a child was
created, its setup pipe carried no error record, and every cleanup field is true; that pipe's EOF is
not misrepresented as proof of exec. Its wait kind is only `exited` or `signaled`.
`valid-checker-result` requires exit zero, no outer timeout, empty stderr, exactly one JSON line and a
non-null parsed object; output-limit and parsed-null status combinations are likewise cross-checked.
Missing, malformed or contradictory reports therefore cannot manufacture ACCEPT.

The node, not the launcher, applies the policy's complete ordered outcome map. A valid checker
`accepted` result becomes ACCEPT; a valid semantic `deterministic_reject` becomes
`checker_rejected`; the two clean-exit text-derived resource reasons require their matching
`pids.events:max` or `memory.events:max` counter and otherwise become
`checker_reported_reason_unconfirmed`; wall-time and signal death remain retryable. Missing,
malformed, binding-invalid or setup-failure reports fall to retryable containment unavailability.
Only ACCEPT and deterministic rejection persist evidence and consume the challenge; every retryable
outcome does neither. The default is fail-safe retryable unavailability, making the mapping total.

The exact request/report schemas, nested wait/resource/cleanup/checker-result records, timeouts,
four launcher capabilities
(`CAP_SETGID`, `CAP_SETUID`, `CAP_SETPCAP`, `CAP_SYS_ADMIN`, explicitly no `CAP_DAC_OVERRIDE`),
seccomp syscall deny set, Landlock access/path profile, resource values and recovery assertions are
part of the JSON bytes named above rather than prose-only defaults. The following paragraphs explain
the security purpose of those frozen values:

The launcher self-checks before binding its socket that Effective, Permitted and Bounding capability
sets are exactly those four entries, while Inheritable and Ambient are empty; any extra or missing
bit is fatal. Its systemd unit independently pins the same bounding set with
`Delegate=cpu memory pids`; no unrelated controller is delegated. The
stable delegated cgroup root is
`/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service`. To satisfy cgroup v2's
no-internal-process rule, launcher startup first enumerates the fixed `/proc/self/task` view and
requires exactly one entry equal to the current TID; this makes the following process-wide move
unambiguous. It then creates the reserved direct child `manager`, moves
the launcher process there, verifies the service root's `cgroup.procs` is empty, and only then enables
`cpu`, `memory` and `pids` in the service root's `cgroup.subtree_control`. Execution leaves are the
other direct children, each exactly `run-<64-lowercase-hex-operation-id>`; recovery ignores only the
exact reserved `manager` child and scans all exact `run-*` leaves. An existing manager must be the
exact direct-child cgroup directory with empty `cgroup.procs` and `cgroup.threads` before the current
launcher moves there, both `cgroup.events:populated=0` and `frozen=0`, exact `cgroup.type=domain`,
exact-empty `cgroup.subtree_control`, and no nested child cgroups; after the move its
`cgroup.procs` must contain only that launcher PID, `cgroup.threads` must contain only the same
launcher's current TID, its type must still be exact `domain`, its `cgroup.subtree_control` must
still be empty, and it must still have no child cgroups. It is
never blindly reused. Any other
direct child fails closed without binding or readiness. The launcher cleans every run leaf before
socket bind and carries the zero-leaf result into the authenticated qualification-ready barrier.
Controller read-back failure is a startup failure before readiness.
The service-wide `UMask=0117` remains fixed because it is part of the later socket-mode contract.
For a newly created manager directory, the launcher therefore requests mode `0700`, immediately
opens the direct child by descriptor, applies `fchmod(0700)` through that descriptor, and verifies
root:root ownership plus exact mode `0700` by `fstat` before opening any child control file. Reuse
does not repair metadata: an existing manager that is not already root:root mode `0700` is rejected.
Every failure before the `cgroup.procs` move attempt is a pre-move startup failure. The move attempt
is the irreversible boundary: failure of that write or any root/manager/controller check after it is
a typed post-move fatal error, and the future top-level executable must exit immediately rather than
continuing with partially changed process placement.

The tracked deployment files are
`native/systemd/boole-native-shadow-launcher.service`,
`native/sysusers.d/boole-native-shadow.conf` and
`native/tmpfiles.d/boole-native-shadow.conf`. The unit is `Type=exec`, starts only the fixed
root-owned launcher path, runs as root:root in `system.slice`, uses `PrivateMounts=yes` and umask
`0117`, and has `Restart=on-failure`, `RestartSec=1s`, `StartLimitIntervalSec=30s` and
`StartLimitBurst=3`. `KillMode=control-group`, `TimeoutStopSec=20s` and `SendSIGKILL=yes` ensure a
service stop cannot intentionally leave launcher descendants alive while leaving more time than
the launcher's ten-second internal cleanup deadline. A systemd stop signal is a crash-equivalent
outer shutdown, produces no execution report, and requires the next startup recovery barrier; it
is not a second launcher-owned submission-verdict path. `TasksMax`, `MemoryMax` and
`MemorySwapMax` are `infinity` at the service boundary because the launcher applies the strict
values below to each direct `run-*` leaf; a broader service default must not create an unobserved
earlier limit. `RuntimeDirectory=` is deliberately absent: the service itself is group root, while
the socket directory must be root:`boole-node`; the tracked tmpfiles input creates that exact
mode-`2750` directory instead. The sysusers input creates the two fixed names with same-named
primary groups, no supplementary-group assignment, home `/nonexistent`, and the frozen non-login
shells. These deployment bytes freeze startup and restart semantics before recovery code is added;
they do not by themselves prove manager-cgroup setup or recovery. This policy-byte change is an
explicit pre-activation successor rotation: the qualification registry now binds the successor
digest, while the prior digest remains historical and was never activation-authorized.
The launcher samples tree-wide `cpu.stat usage_usec` every 10 ms and once more before any report;
at or above 120,000,000 microseconds it freezes and kills the leaf. The child outer wall is 100
seconds, cleanup has a separate 10-second deadline, and the node's response deadline is 115 seconds.
All reported cgroup counters are snapshotted after terminal wait and any required kill, after
`populated=0`, but before leaf removal; the launcher cannot remove the only observation source and
then invent counters for the report.
If cleanup misses its deadline, the launcher sends no verdict frame, closes the connection and
refuses readiness until startup recovery succeeds.

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
    **private mount namespace** created atomically for that submission's child by the `clone3`
    contract below, not the node's own default namespace — so the mount is invisible to, and cannot be
    interfered with by, any other concurrent or subsequent submission or by the node process itself.
    Mount options, corrected this round: `size=536870912,nr_inodes=8192,mode=2750,nosuid,nodev,
    uid=0,gid=<checker-primary-gid>` — **not** `noexec`. `checker.py` builds and then
    executes the compiled test binary from inside this exact workspace (`cargo test` links and runs
    the test binary under `target/`), so a `noexec` mount would turn even a correct, accepted
    submission into a `Permission denied` failure before any verdict is ever reached; `noexec` is
    dropped from this mount for exactly that reason. `nosuid`/`nodev` remain — the workspace still has
    no legitimate need to host a setuid binary or a device node, and denying those costs nothing this
    submission surface needs. The workspace's isolation instead comes from layers that do not depend on
    denying execution: the dedicated unprivileged UID/GID with no supplementary groups and an empty
    capability set (pre-execution ordering step 5 below), the seccomp/Landlock ruleset (step 7 below),
    and the cgroup ceilings themselves — none of which are weakened by allowing exec on this one mount.
    The root launcher must materialize trusted inputs after mounting but before dropping privilege,
    without `CAP_CHOWN`: setgid inheritance supplies group `boole-native-checker`.
    `/work/task.json`, `anchor.rs` and `submission.rs` are root:checker mode `0440`; `/work/scratch`
    is root:checker mode `2770` and initially empty. Bytes arrive only through parent-verified,
    sealed read-only memfds; the child verifies bytes, owner, group and mode and closes those authority
    FDs before exec. This qualification family accepts the literal task anchor path `anchor.rs`,
    matching the fixed materialized path rather than pretending arbitrary task filenames exist.
    The checker then creates its real temporary tree only beneath
    `/work/scratch/boole-native-check-*/`, including `cargo-home`, `target`, `home`, `tmp`, and `src`;
    the superseded `.cargo-home` and `/work/target` descriptions are not part of this contract.
    Teardown: a private mount namespace's lifetime is scoped to the tasks that hold a reference to
    it, not to an explicit `umount` call; once every task inside it is confirmed dead (which cleanup
    already requires, via `populated=0`, per contract item 6 above), the kernel tears the namespace
    and its tmpfs down automatically, and the tmpfs's backing pages are reclaimed as ordinary memory
    at that point. This holds identically for a normal completion and for crash-restart recovery
    (section 7): normal cleanup additionally reaps the direct child and closes every launcher-held
    pidfd/namespace FD; restart recovery, where the old launcher cannot reap its former child, instead
    requires `populated=0`, empty `cgroup.procs`/`cgroup.threads`, and leaf removal. No separate,
    additional `umount` call or fabricated namespace-reference count is required; asserting either
    would add an operation or observation Linux does not provide for this lifecycle.
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
irreversibly becomes the checker identity before any untrusted code executes. Phase 3B.2a freezes
the production binary location, closed message/authentication contract, installation ownership,
dedicated UID/GID invariants, exact capability sets and crash-recovery protocol. Their implementation
and executable-provenance test remain open. The Phase 3B.1 transient root service is capability
evidence for this boundary, not that production implementation.

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

1. **Atomic cgroup and namespace creation.** Create the execution leaf, open its cgroup FD, then call
   `clone3` exactly once with `CLONE_INTO_CGROUP | CLONE_NEWNS | CLONE_NEWPID | CLONE_PIDFD` and
   `SIGCHLD`. The child therefore begins inside the leaf and is PID 1 in its new PID namespace; the
   privileged monitor launcher stays outside both namespaces with the returned pidfd. Writing
   `cgroup.procs` after an ordinary fork or calling `unshare(CLONE_NEWPID)` in the same process are
   not fallbacks: either leaves a race or does not make that caller PID 1. If `clone3` or any required
   flag is unavailable, setup fails before a child and no report is sent.
2. **Private `/proc` and tmpfs.** Inside the child mount namespace, immediately
   remount the root filesystem recursively as private
   (`mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL)`), new this round — required because Linux's
   default root-mount propagation type is `shared` on most distributions, so `unshare(CLONE_NEWNS)`
   alone does not by itself stop a mount event inside this namespace from propagating out to the
   node's own default namespace or to any other submission's namespace, or vice versa; only after that
   remount, mount a private `proc` filesystem at `/proc` with `nosuid,nodev,noexec`, and only then
   mount the tmpfs workspace at the target path with the options pinned above. The last process in
   the PID/mount namespace exiting tears down both private mounts. Cleanup uses observable facts only:
   the direct child is reaped, `cgroup.events:populated=0`, every launcher-held pidfd/namespace FD is
   closed, and the leaf is removed. Linux exposes no stable global namespace-reference counter, so
   the superseded `namespace-reference-count=0` pseudo-check is explicitly not an implementation
   requirement.
3. **Authority materialization.** The child receives only sealed read-only memfds for the already
   verified task, anchor and submission plus a close-on-exec setup-status pipe. It writes and verifies
   the exact root:checker workspace objects and empty scratch directory frozen above while still
   privileged. The parent treats any setup error from the pipe as a pre-child-verdict failure and
   sends no report.
4. **FD block.** Close every inherited file descriptor except stdin/stdout/stderr and the setup-status
   pipe, including authority memfds after their bytes are verified, cgroup controls, the durable
   ledger, and any other submission's workspace. The setup pipe is `CLOEXEC`: EOF means only that no
   setup-error record was received; it is **not** proof of exec because a pre-exec crash also closes
   the pipe. A valid structured checker result is the only path to ACCEPT or deterministic rejection;
   a silent pre-exec death remains an outer-fact retryable outcome.
5. **Privilege drop.** Switch to a dedicated, unprivileged, single-purpose UID/GID with no
   supplementary groups; drop the full capability set to empty; set `no_new_privs=1`
   (`prctl(PR_SET_NO_NEW_PRIVS, 1)`) so nothing downstream can ever regain a privilege this step
   removed.
6. **The launcher's node-owned outer `RLIMIT_*` application — corrected this round, distinct from
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
7. **seccomp/Landlock.** Apply a seccomp-bpf filter and a Landlock ruleset, layered, denying at
   minimum: `mount`/`umount2`/`unshare`/`setns` (no re-namespacing), `ptrace` (no process
   introspection), all networking beyond what is strictly required (an explicit kernel-enforced
   backstop behind the existing `CARGO_NET_OFFLINE=true` convention, not a substitute for it), and —
   **this is the specific mechanism that prevents the submission from reopening cgroupfs to loosen
   its own limits** — Landlock denial of write/create/remove/refer operations and write-capable opens
   outside the workspace, including under `/sys/fs/cgroup`. Read-only access is deliberately not a
   handled Landlock right in this policy and is not claimed to be denied. Both mechanisms are
   inherited by every descendant and neither can be removed by a
   lower-privileged process once attached, which is what makes this guarantee hold transitively
   through `cargo` → `rustc` → the linker → the test binary without any of them needing to cooperate.
8. **Exec.** Only after steps 1-7 are confirmed applied does `exec()` replace the process image with
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

**Why the launcher's outer 100-second ceiling is not a corroboration mechanism for
`checker.py`'s own internal 60-second timeout.** `checker.py`'s own process always exits `0`
regardless of its internal verdict (`main()`, `checker.py:663`), so its wait-status is uninformative
to the launcher as an outer observer — but `resource_wall_limit` itself is not text-derived at all
(it is a plain monotonic-clock comparison inside `checker.py`, unconditionally trusted per the list
above), so it never needed corroboration in the first place. Checker-owned `taskTotalWallSeconds`
remains 90 seconds; the distinct launcher-owned outer ceiling is 100 seconds, followed by a
10-second cleanup deadline and a 115-second node response timeout. The outer ceiling is a safety net
for the structurally distinct case where `checker.py` itself never returns at all (a hang or a bug in
`checker.py`), not a mechanism for re-confirming a timeout `checker.py` already correctly reported
and already correctly acted on (by killing its own child's process group) 40 seconds earlier.

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
([final run 32598803995, job 97093814188](https://github.com/NotoriAndo/Boole/actions/runs/32598803995/job/97093814188)),
so this infrastructure-capability prerequisite is GREEN. A skipped, permission-less, generic
`ubuntu-latest` or weakened run still cannot replace that evidence. Production launcher/IPC,
authenticated handshake, launcher binary provenance and route/checker execution remain open; the
dedicated identity-resolution contract and exact disabled policy bytes are frozen by Phase 3B.2a.

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
19. The pre-execution ordering sequence (`clone3` into cgroup + mount/PID namespaces → rprivate root →
    private `/proc` → tmpfs → sealed-authority materialization → FD block → privilege
    drop → RLIMIT → seccomp/Landlock → exec) is applied once, to `checker.py` itself, before exec; a
    test confirms the submission process cannot write/create/remove/refer, or obtain a write-capable
    open, under `/sys/fs/cgroup` (Landlock denial verified directly, not inferred); a read-only open
    remains allowed and is not misreported as a security guarantee.
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
23. `cgroup.freeze` + `cgroup.kill` is the launcher's only submission-verdict termination path; a
    kernel lacking `cgroup.kill` fails the startup capability probe closed. A systemd stop is tested
    separately as a report-free crash-equivalent followed by mandatory startup recovery.
24. Normal cleanup verifies direct-child reap, `populated=0`, launcher pidfd/namespace-FD closure and
    leaf removal on every outcome; restart cleanup separately verifies `populated=0`, empty
    `cgroup.procs`/`cgroup.threads` and leaf removal. Neither path invents a namespace reference count.
25. GREEN is not declared from a run where the containment-dependent suite skipped for lack of real
    cgroup v2 delegation on the CI runner.
26. A correct, accepted submission successfully builds and executes its compiled test binary from
    inside the tmpfs workspace — the mount is not `noexec` (section 9), directly guarding against G1's
    regression class of the containment envelope itself blocking legitimate work.
27. The tmpfs root is root:checker mode `2750`, its trusted files are root:checker mode `0440`, and
    its initially empty scratch directory is root:checker mode `2770`; the checker identity can read
    only the trusted files and can write only below scratch, with no `CAP_CHOWN` step.
28. A mount performed inside one submission's private mount namespace after the `MS_REC|MS_PRIVATE`
    remount is never observable from the node's own default namespace or from any other concurrent
    submission's private mount namespace.
29. The launcher's outer `RLIMIT_*` application (pre-execution ordering step 6) and `checker.py`'s
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
33. Socket creation under the root:boole-node setgid directory produces a root:boole-node mode-0660
    socket without `CAP_CHOWN`; wrong-type, wrong-owner or unlocked stale entries fail before bind.
34. A checker-valid 8,192-byte patch body plus fixed scaffold passes the 16,384-byte complete-source
    IPC ceiling, while a complete source above that ceiling fails before execution.
35. `clone3(CLONE_INTO_CGROUP|CLONE_NEWNS|CLONE_NEWPID|CLONE_PIDFD)` starts the child as PID 1
    inside the leaf without a pre-exec cgroup race; lack of any required feature has no fallback.
36. Workspace materialization proves sealed input bytes, literal `anchor.rs`, exact owners/groups/
    modes, initially empty scratch and the checker's real `boole-native-check-*` directory layout.
37. Qualification readiness is withheld until launcher-owned startup recovery reports manager
    verified (including zero nested manager cgroups/processes before reuse), zero run leaves and zero
    unexpected direct children; without that authenticated barrier the node cannot roll back any
    stuck `InFlight` row.
38. Every checker verdict's reason belongs to the frozen verdict-specific vocabulary; unknown reasons
    and every contradictory report combination are rejected, and report PID/authority bindings match
    ready, execute, `SO_PEERCRED`, and installed bytes.
39. The total node outcome table maps every structured checker reason plus missing/malformed/timeout/
    signal path to exactly one outcome, evidence flag and consumption flag; terminal outcomes both
    persist and consume, retryable outcomes do neither.
40. Submission-digest test vectors distinguish 64-byte ASCII-hex template/challenge fields from
    decoded 32-byte values and prove the exact field ordering and length-prefix rules.
41. With `activationAllowed=false`, the launcher starts no checker or other untrusted execution
    child and accepts no execute frame, while the exact toolchain-manifest Rust/Cargo/Python identity
    probes are the only permitted pre-bind child commands; any other spawn or any probe drift
    withholds readiness without journal mutation or report creation.

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
  containment-dependent suite;
* a launcher readiness frame is accepted before startup recovery has proved zero execution leaves
  and zero unexpected direct cgroup children;
* any execution report can reach ACCEPT or deterministic rejection with a mismatched PID, authority
  digest, wait state, timeout/output status, stderr, parsed result, or unknown checker reason; or
* any caller bypasses the shared strict authority parser, substitutes a repository fixture for the
  installed `/usr/share/boole/native-shadow/registry-v1.json`, or accepts readiness without checking
  all three installed authority files against the compiled exact bytes.

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

The following block preserves the Phase 3B.2b-2y checkpoint and is superseded by section 13.1:

```
NODE-NATIVE-SHADOW-BINDING-CONTAINMENT-DESIGN-V1: IMPLEMENTATION-BASELINE-APPROVED
IMPLEMENTATION: PARTIAL (PHASE-1 / PHASE-2 / PHASE-2C / PHASE-2D / PHASE-3A.1 /
PHASE-3A.2 / PHASE-3B.0 / PHASE-3B.1 / PHASE-3B.2A / PHASE-3B.2B-0 / -1 /
PHASE-3B.2B-2P / PHASE-3B.2B-2N / PHASE-3B.2B-2I / PHASE-3B.2B-2L /
PHASE-3B.2B-2U / PHASE-3B.2B-2R / PHASE-3B.2B-2S / PHASE-3B.2B-2T /
PHASE-3B.2B-2G / PHASE-3B.2B-2H / PHASE-3B.2B-2M / PHASE-3B.2B-2V /
PHASE-3B.2B-2W / PHASE-3B.2B-2X LANDED;
PHASE-3B.2B-2Y ONE-SHOT QUALIFICATION CURRENT GUARDED SLICE)
CONTAINMENT-ROUTE-GREEN: OPEN / READINESS-HANDSHAKE-ONLY; CHECKER-EXECUTION-AND-ROUTE-UNIMPLEMENTED
```

### 13.1 Closed-local implementation closure addendum — 2026-08-24

The status block above is the preserved Phase 3B.2b-2y checkpoint. PR #206 (main `8542386`) and
PR #219 (main `4de603f`) subsequently closed this document's named-Linux containment and route RED
matrix for the frozen non-issuable lane.

The frozen accepted answer now reaches `ACCEPT` in the actual contained checker; tampered and
constant answers reach `DeterministicReject`; and an empty answer reaches `PrecheckReject` without a
checker run. The named-Linux gate verifies the cgroup/rootfs/seccomp/Landlock boundary, stable
launcher instance, node peer-PID binding, cleanup and no leftover per-submission cgroup. The
feature-gated node receives raw answers at the fixed loopback endpoint, resolves its own
registry/grant authority, and durably records `Active -> InFlight -> TerminalConsumed`, evidence and
the HTTP result.

Exact redelivery returns the stored terminal result without another checker execution. Concurrent
execution is limited to one, and an outcome that is ambiguous after durable reservation fails closed
as `adjudication_unknown`, `retryAuthorized=false`. Client cancellation and ambiguous-result
handling are proven at the route-unit boundary. A separate process-kill followed by full HTTP-node
restart E2E is not claimed.

### 13.2 Crash/restart closure addendum — 2026-08-24

Section 13.1's remaining limitation ("a separate process-kill followed by full HTTP-node restart
E2E is not claimed") is superseded append-only. PR #220 (main
`9203156950e178277895ac4d282462147ddae23e`) pinned the restart security invariants: no
cross-restart grant reuse against a consumed durable attempt, execution-time authority digest
mismatch rejection, stale ready-token non-qualification across sessions, and zero-PID peer
distrust. PR #221 (main `6553360a6291c300ad0d19c50238b8b7c9263c68`; CI run
<https://github.com/NotoriAndo/Boole/actions/runs/32709400913>) closed the real crash/restart
exactly-once E2E on Linux CI: after SIGKILL of the real node and launcher processes and a full
systemd restart with changed process identity, terminal results are redelivered byte-identically
with zero additional checker starts, and a synthetic unresolved durable `InFlight` row keeps the
replay route fail-closed after restart. Cleanup was verified in both scenarios, including reaping
the inert launcher socket inode a SIGTERM-stopped launcher leaves behind.

```text
CONTAINMENT-ROUTE-GREEN: GREEN (NAMED-LINUX, CLOSED-LOCAL)
CRASH-RESTART-EXACTLY-ONCE-E2E: GREEN (CLOSED-LOCAL, LINUX CI)
```

The boundary of section 13.1 is otherwise unchanged: loopback-only, `nonIssuable=true`,
`activationAllowed=false`, no `SharePool`/block/reward/P2P/consensus consumer, `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`. This closure feeds the MAC.0 record in
`docs/mac-first-hidden-linux-execution-plan-v1.md` section 9; it is not a Mac product or
production availability claim.

```text
CONTAINMENT-ROUTE-GREEN: GREEN (NAMED-LINUX, CLOSED-LOCAL)
NATIVE-SUBMISSION-SHADOW-ADMISSION-V1: GREEN (FROZEN NON-ISSUABLE QUALIFICATION ONLY)
```

This is not general production activation. The route is loopback-only, the fixture remains
`nonIssuable=true`, and authority remains `activationAllowed=false`. `SharePool`, block, reward,
P2P and consensus paths are not connected; the static no-consumer regression gate is GREEN.
Accordingly, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD` and `BF.7=HOLD` remain unchanged.

The base document, r1 and r2 remain the historical record of the first three review passes and are
not edited by this document beyond their own status markers pointing here. A fourth operator review
of this document itself (2026-08-22) found four further gaps (F1-F4, listed above) and closed them in
place, in sections 4, 6, 7, 9 and 10. A fifth operator review (2026-08-22) found that revision itself
left one non-implementable execution step, two prose/RED-gate contradictions and one remaining
self-sufficiency gap (G1-G4, listed above), and this revision closes those too, in place, in sections
7, 9 and 11. Subsequent operator direction authorized phased RED→GREEN implementation, producing
the foundation slices listed above. The named runner now supplies the required delegated cgroup v2
and namespace capability evidence, and Phase 3B.2a freezes the exact policy authority and
identity-resolution contract. Further containment/route implementation remains fail-closed: no
Phase 3 GREEN may be claimed until the authenticated launcher handshake, execution-ID durability,
actual checker containment/cleanup and route binding are implemented and pass the named Linux gate.
The partial foundation does not authorize an endpoint, child-process execution or activation.

### 13.3 Linux/arm64 authority-parity closure addendum — 2026-08-25

PR #224 (main `2a6de07ba6c77355d19a3d342ab718f7358fd76a`; CI
<https://github.com/NotoriAndo/Boole/actions/runs/32766488279>) rebuilt the separately frozen
authority on native Linux/aarch64 and ran the same accepted/replay/empty/tampered/constant and
two-way cross-task verdict matrix as the x86_64 baseline. Exact verdict and binding parity, the
named Linux containment gate, node HTTP replay path and crash/restart exactly-once gate were GREEN.

The implementation now rejects a Linux build whose selected authority feature does not match its
target architecture: the arm64 successor authority is native-aarch64-only and the default authority
is native-x86_64-only. Resource-policy document parity is exact except for the explicitly frozen
architecture identity, and containment/resource enforcement parity is exact.

This closes only the Linux/arm64 guest-authority parity subgate; MAC.2 remains PARTIAL while the
authenticated staged verifier and post-adoption image/runtime-authority reverification remain open.
The qualification remains loopback-only,
`nonIssuable=true` and `activationAllowed=false`; no `SharePool`, block, reward, P2P or consensus
consumer was added. It is not an Apple Virtualization VM, `Boole.app`, signed/notarized release or
Mac production result. MAC.1 remains partial for distribution mode, release identities and the
measurement protocol; MAC.3 is blocked/not started until those choices are frozen and the authenticated
MAC.2-B staged verifier is implemented and GREEN. Later measurement results are earned by MAC.3–MAC.5 rather than
being impossible preconditions to MAC.3. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`
and `BF.7=HOLD` remain unchanged.

### 13.4 MAC.3 closed-local guest boot addendum — 2026-08-27

The sealed arm64 guest image was booted on Apple Virtualization.framework on a development Mac,
once, under a qualification frozen and merged before the run. A first attempt on the predecessor
image had already been spent and sealed as FAIL; that record is referenced here and is byte-unchanged.

PR #264 (main `e7dc43e7a692aec9f7518d7e8605bc6d8d693b93`) made the boot driver select the attempt it
is run under instead of assuming one, so each attempt is judged against its own sealed digests and
seals to its own result path. The run itself is recorded in
`native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v2.json`.

All six frozen conditions were MET: the converged image was the image loaded; the machine carried no
network device, no shared directory and no writable disk, with a single read-only root attachment;
the kernel mounted the sealed ext4 read-only as its root; systemd 255.4 ran as PID 1 and reached the
image's default target; the sealed root disk hashed identically before and after; and the console was
captured and hashed in both its raw and judged forms.

What this closes is narrow. It establishes that the guest image boots to a running systemd inside the
closed local machine. It does **not** establish the launcher reaching a serving state: the unit is
reported Started, which only means systemd executed it, and the unit logs to the guest journal, which
a closed machine gives no channel to read — so where the launcher refused was not observed and is
recorded as unobserved rather than inferred. The account database and the runtime rootfs the launcher
verifies remain absent, exactly as registered before the run.

The qualification remains loopback-only, `nonIssuable=true` and `activationAllowed=false`. No
`SharePool`, block, reward, P2P or consensus consumer was added, and no wallet seed, model API key or
node secret was placed in or passed to the guest. The host binary is ad-hoc signed with
`com.apple.security.virtualization` alone: no Team ID, Developer ID certificate, provisioning profile
or notarization exists or was created. This is not clean-Mac evidence, `Boole.app`, a signed or
notarized release, or a Mac production result. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`
and `BF.7=HOLD` remain unchanged. MAC.4 route binding is not started.

### 13.5 MAC.3 guest runtime contract addendum — 2026-08-27

`native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json` freezes the shape a
MAC.3 guest runtime run must have before any run answers it. It opens work and reports none: its
status reads `MAC3-GUEST-RUNTIME-CONTRACT-FROZEN-NOT-RUN`, it carries no verdict, and
`servingClaim`, `launcherServing`, `cleanMacEvidence`, `runtimeCompatibilityVerified` and
`productRelease` are all `false`.

The record binds by digest every file its survey read, the sealed boot receipt and qualification it
follows, and the producer authority that seals the launcher. Nothing sealed is edited.

Three gaps stand between the sealed boot and a serving launcher, and each requires a file inside a
read-only, content-addressed image, so a new production is required and is recorded as *required
and not performed*, with one run allowed, criteria frozen first, byte-identical replicas, a
read-only `e2fsck`, and a hard stop rather than a retry on any post-result mismatch:

1. an account database in the image, because `systemd-sysusers` cannot write one on a read-only root;
2. the arm64 runtime rootfs and its content manifest, whose sealed digest and the digest the
   launcher is compiled against are already the same value;
3. a readable path for the launcher's own output, satisfied by sending it to the console the host
   already captures and hashes — no device, no network and no shared directory is added.

Nine of the ten minimum conditions are frozen with their checking method recorded. The tenth, *the
launcher runs under an unprivileged account*, is **held**: `privilege.rs` requires the launcher to
be root holding exactly four capabilities and to verify that mask itself, and
`native-shadow-launcher-privilege-gate.sh` proves both a missing and an extra capability fail
closed. The launcher is the component that drops to the unprivileged node and checker accounts, so
running it unprivileged would remove that delegation rather than tighten it. A least-privilege
reading that matches the sealed design is recorded beside the condition with `readingApplied:
false`; the condition itself is stored verbatim, and the contract's tests fail if it is ever marked
satisfied, waived or reworded while held.

No wallet seed, model API key or node secret is placed in or passed to the guest, and no
`SharePool`, block, reward, P2P or consensus consumer is added. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and `activationAllowed=false` are unchanged. MAC.4 route binding is not
started.

### 13.6 MAC.3 guest runtime input set addendum — 2026-08-27

`native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json` freezes the seven files a
successor image would stage. It is a record of inputs, not of a result: `imageProduced: false`,
`servingClaim: false`, `activationAllowed: false`, and no verdict field.

Five of the seven are the account database — `/etc/passwd`, `/etc/group`, `/etc/shadow`,
`/etc/gshadow`, `/etc/nsswitch.conf` — baked because `systemd-sysusers` cannot write to a read-only
root. `boole-node` resolves at 990 and `boole-native-checker` at 991, with `/nonexistent` homes and
the two shells the identity contract allows. Every clause `resolve_one` checks in
`crates/boole-native-shadow-protocol/src/service_identities.rs` is answered from those files,
including the one that requires the account's full group list to be exactly its primary gid, which
is why every member list in `group` and `gshadow` is empty. Neither shell path exists in the image;
the contract compares the shell as a string and nothing execs it, and the record says so rather than
leaving it to be discovered.

The remaining two are successors staged to the guest paths their predecessors occupy.
`boole-native-shadow-launcher-v2.service` changes exactly two lines — standard output and standard
error each gain `console` beside `journal` — and the capability set, `NoNewPrivileges` and
`User=root` are byte-identical, so observability is not bought by widening what the launcher holds.
`boole-native-shadow-v2.conf` keeps the two `/run/boole` rules and drops the three `/var/lib/boole`
rules that could not succeed on a read-only root.

`/var/lib/boole/native-shadow/runtime-rootfs` stays open. It is a builder change rather than an
input file, `verify_runtime_rootfs_replay` still refuses, and no input in the set claims to close
it. The v1 launcher unit, tmpfiles config, sysusers config and machine-id are left byte-unchanged
and are verified as such by the record's own tests.

No wallet seed, model API key or node secret is placed in or passed to the guest, and no
`SharePool`, block, reward, P2P or consensus consumer is added. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and `activationAllowed=false` are unchanged. MAC.4 route binding is not
started.

### 13.7 Successor image production criteria addendum — 2026-08-27

The input set sealed in 13.6 does not reach the guest until an image carries it. This addendum
records the criteria that image would be judged against, frozen before one exists, and the survey of
what carrying it would cost.

`native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json`
(`417d2497072519031506664553a0d9b478c53a7bf7983f431332f69bbecec4b8`) holds attempt
`MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-ATTEMPT-1` at one run allowed and none spent, with the result
path named and absent from the tree. Seven production conditions each name the check that judges
them; the containment-relevant ones are that the runtime rootfs manifest in the image must **equal**
the value the launcher compiles against rather than merely be present, that the launcher executable
must be byte-identical to the sealed one, and that the absence of wallet material, model API keys,
node secrets, network devices and shared directories is verified against the produced image rather
than inferred from the input list.

Five abort conditions stop the run. `criteria-would-have-to-be-loosened` makes rewording, waiving or
dropping a condition a reason to stop rather than a step toward passing, and `replicas-disagree`
forbids re-running until two builds coincide.

`successorChainForStaging` records the four sealed records that a staging extension would have to
supersede — the source lock plan, the lock generator, the lock, and the boot builder's staging table
— in that order, each with its current digest. Two findings are recorded because they change what is
possible: the 191 package rows are already sealed in the dependency candidate result, so a successor
lock needs no payload re-acquisition, and the boot module's own digest is computed at import rather
than pinned, so adding staging entries breaks no pin. None of the four is edited here. The re-acquisition finding is recorded as a demonstration rather
than an inference: regenerating the sealed lock in place returned the lock, its result and its plan
byte-identical, and the generator contains no `urllib`, `requests`, `socket` or `subprocess`, so it
has no path by which to fetch. What it does not establish -- that a successor pair would be correct
-- is stated in the same field. The pin finding is demonstrated the same way: a probe entry added to
the staging table imported cleanly and was refused only by the lock-coverage check failing on the
count, after which the builder was restored byte-identically, which places the block at the lock
rather than at any frozen digest.

`whyItIsNotWalkedYet` records why the chain is deferred rather than leaving the deferral
unexplained. A partial walk stages the seven inputs but not the runtime rootfs, so a production run
would fail `the-runtime-rootfs-and-its-manifest-are-in-the-image` by construction and spend the one
allowed run on a known answer; and because the runtime rootfs also adds tracked material to the plan
and the lock, splitting the work would build two append-only successor chains over the same four
records instead of one. The two are treated as a single unit of work, neither is started, and the
condition that would reverse the decision is stated.

Fifty tests bind the record, registered in `scripts/self-test.sh` and pinned in
`scripts/docs-smoke.sh`. No image was produced and no production was dispatched. No wallet seed,
model API key or node secret is placed in or passed to the guest, and no `SharePool`, block, reward,
P2P or consensus consumer is added. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD` and `activationAllowed=false` are unchanged. MAC.4 route binding is not
started.

### 13.8 Runtime serving gap measurement addendum — 2026-08-27

Appended, not edited. Section 13.7 recorded what a successor image would be judged against. This
records what such an image would have to carry for the launcher to reach serving, measured against
the tree.

The launcher's `main` runs nine stages. The seventh verifies a read-only runtime rootfs against a
content manifest whose digest is compiled into the binary; the ninth serves. Both the tree and the
manifest live at fixed absolute paths that the binary cannot be configured away from — it reads no
environment variable. The boot image builder mentions neither path, nor their shared parent, at all.
That is the gap: a property of a tracked file, not a prediction about a run.

The material for closing it is complete. The boot rootfs lock names 197 artifacts and the runtime
rootfs lock names 62; all 259 resolve against the local content-addressed cache by digest, with none
absent and nothing fetched. Those cache counts are recorded as local observations and marked
not reproducible on a clean runner, since the cache is untracked; the locks stay the authority.

An earlier reading held that the 766,556,160-byte arm64 layer was a required acquisition and
therefore a stop condition. That was wrong and is kept in the record rather than dropped: the layer
is an output, not an input, and it is rebuilt from the locked inputs on every pull request by the
`native-shadow-rootfs-replay-linux-arm64` job and compared against the sealed expectation. What
remains unestablished is whether that rebuild could run on the developer Mac; the job that works runs
on a Linux arm64 runner as root.

Four steps remain: extend the four sealed staging records, teach the builder to place both paths,
produce one image pair, boot it once. The last two are one-shot and each must be sealed after it
runs, which is why none was started here. Forty-seven tests bind the record, and three deliberate
mutations were each caught.

No image was produced, no boot was performed, nothing was fetched, and no sealed record was
modified. `activationAllowed` stays `false` and no serving claim is made.

### 13.9 Serving gap closure plan addendum (2026-08-27)

Sealed as
`native/containment/native-shadow-mac3-serving-gap-closure-plan-arm64-v1.json`
at status `MAC3-SERVING-GAP-CLOSURE-PLANNED-NOT-IMPLEMENTED`. Append-only: it
edits no earlier record and resolves no open question.

The plan exists because §13.8 measured one gap and the sealed runtime contract
names three. The launcher refuses at the earliest of them -- the account
database, which `systemd-sysusers` cannot create on a read-only root -- so an
image closing only the measured gap would spend the single allowed production
without reaching the stage it was built for. All three move the same four sealed
records in the same fixed order, so they are scheduled as one unit.

Two of the three are table edits. The third requires a second rootfs tree nested
at a compiled-in guest path, duplicated rather than symlinked, because the
verifier compares the observed path set against the manifest exactly and rejects
a kind mismatch. The plan therefore turns on a byte budget: `maxTotalBytes` is
2,147,483,648 in both locks, and the upper-bound sum of the current initrd and
the runtime layer is 1,777,489,456, leaving 369,994,192. The record states this
as a bound rather than a result, and states plainly that the entry limit is not
answered by any pinned number in the tree.

The plan also corrects a conflation that the tests caught before it was sealed:
the builder digest both locks name belongs to the arm64 projection, not to the
module whose code enforces the limits. The projection verifies the legacy
module's bytes against a digest it pins, then executes a fixed textual
projection of them. Both files are pinned separately in the record.

The held condition on launcher privilege is carried over unchanged --
`relaxed: false`, `waived: false`, `satisfied: false`, awaiting an operator
decision. 37 tests re-derive every repository-derived number from the file it
names; three deliberate mutations were each caught.

No builder, lock, plan or unit file was edited. No image was produced, no
production dispatched and no boot performed. Serving is not claimed.
mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation false
and activationAllowed=false are unchanged. MAC.4 route binding is not started.

### 13.10 Condition 4 correction addendum (2026-08-28)

`native/containment/native-shadow-mac3-condition-4-correction-arm64-v1.json`
pre-registers the corrected fourth MAC.3 condition before any image is built
against it. The operator chose the correction over the redesign.

The original wording made the launcher the process that must be unprivileged.
The launcher is the privilege manager -- it verifies the frozen capability mask
against its own kernel status before binding a socket, materializes the authority
files, and performs the descent. What must be unprivileged is the checker and the
submitted answer it creates. The sealed contract had already offered exactly this
reformulation and declined to apply it without an operator decision; that
decision has now been given.

Seven clauses, each carrying its own source wording and each required to name
code already in the tree that enforces it. The launcher half is the startup
self-check: root in all four UID and GID slots, and the effective, permitted and
bounding sets compared against the compile-time mask by equality rather than
containment, with inheritable and ambient exactly zero. Equality is the
load-bearing choice -- a containment check would accept the extra capability the
clause forbids. The submissions half is an ordering claim read out of the source:
drop-privileges at line 532, verify-privileges at 536, exec at 2088, with the
bounding set emptied while `CAP_SETPCAP` is still held and before the identity
changes, which is what forecloses reacquisition. The post-descent verification
requires the real, effective and saved identities to match, no supplementary
groups, all five capability sets exactly empty and `no_new_privs` set; a failed
stage propagates and returns before the exec rather than continuing past it.

That it is a correction rather than a relaxation is tested rather than asserted:
no clause removes a check, every clause is marked as restating an existing
refusal, and the cited files are stamped by digest and size so the tests
re-derive them from the tree. Under the original wording nothing satisfied the
condition; under the corrected wording the same unchanged tree satisfies all
seven.

The guest runtime contract is not edited. It still carries the condition at
status `held` with relaxed false, waived false, satisfied false and
`readingApplied` false, and docs-smoke pins both that and the correction, so the
state before the decision stays readable. 28 tests, registered in self-test and
pinned in docs-smoke; four deliberate mutations -- calling it a relaxation,
drifting a cited digest, adding a fifth capability to the mask, and claiming the
sealed contract had been edited -- were each caught, and regenerating restored
the identical digest.

Deciding a condition is not passing it. Nothing was booted against the corrected
wording, the descent path is read here rather than run, and the three serving
gaps are untouched: the guest still has no account database, which is why the
launcher refuses at startup stage one rather than reaching the runtime rootfs
check at stage seven.

No builder, lock, plan, unit or launcher source file was edited. No image was
produced, no production dispatched and no boot performed. Serving is not claimed.
mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation false
and activationAllowed=false are unchanged. MAC.4 route binding is not started.
