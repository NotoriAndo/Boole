# Mac-first hidden Linux execution plan v1

Status: **MAC.0 COMPLETE (closed-local Linux baseline, 2026-08-24, section 9); MAC.1 CONTRACT
FROZEN with status MAC.1-PARTIAL — OPERATOR VALUE REQUIRED (section 10); MAC.2+ NOT STARTED —
NOT IMPLEMENTED, NOT RELEASE-READY, NO ACTIVATION AUTHORITY.**

This plan defines the product boundary for running Boole's native-answer checker for Mac users.
It does not weaken or replace the current Linux production-containment authority, and it does not
make the native-shadow route available on macOS today.

## 1. Product decision

Mac users are the primary desktop users for this product. The supported product experience must
therefore be:

1. install one signed and notarized `Boole.app`;
2. start and stop mining from the app;
3. never require the user to install Docker Desktop, Linux, a hypervisor UI, `cargo`, `rustc`,
   Python or a system service manually;
4. never require the user to administer a VM, log in to a guest, use a terminal or understand that
   a Linux guest exists; and
5. recover from app restart, Mac restart, sleep/wake, interrupted image download and failed guest
   upgrade without losing or duplicating a submission verdict.

Docker may remain an explicitly developer-only diagnostic option. It is not a consumer dependency,
not a production security boundary and not a release prerequisite.

## 2. Why Linux remains inside the product

The deterministic checker semantics are not inherently Linux-only. Task and anchor binding,
submission parsing, hidden-test construction, result classification and evidence digesting are
portable logic.

The current production execution envelope is Linux-specific because it runs untrusted submitted
Rust through the whole `checker.py -> cargo -> rustc -> linker -> test binary` process tree. The
reviewed contract relies on cgroup v2, PID and mount namespaces, `clone3(CLONE_INTO_CGROUP)`,
seccomp, Landlock, bounded executable tmpfs, tree-wide accounting and verified whole-tree cleanup.
The current authority deliberately refuses to spawn that tree on non-Linux hosts.

The product architecture therefore separates two responsibilities:

```text
portable checker meaning and node-owned accounting
                    +
platform execution envelope for untrusted native code
```

For v1, the production execution envelope remains Linux. The Linux requirement is an internal
security implementation detail, not an installation requirement imposed on a Mac user. The shipped
form is a user-invisible Linux execution appliance managed entirely by `Boole.app`.

## 3. Target architecture

```text
Boole.app on macOS
  - user interface
  - node control plane
  - challenge and durable-attempt accounting
  - image verification, lifecycle, health and update control
  - no direct execution of an untrusted native submission
                    |
        authenticated, bounded host/guest channel
                    |
minimal Linux execution appliance
  - frozen checker, task/anchor authority and toolchain
  - existing minimal privileged launcher boundary
  - per-submission cgroup/tmpfs/seccomp/Landlock containment
  - canonical result/report only
  - no wallet, model credentials or unrelated host files
```

The Mac host must manage the guest through Apple's Virtualization framework, or a separately
reviewed successor with equivalent first-party lifecycle and isolation properties. The initial
implementation target is Apple's documented Virtualization framework:

- <https://developer.apple.com/documentation/virtualization>
- <https://developer.apple.com/documentation/virtualization/creating-and-running-a-linux-virtual-machine>

The host/guest protocol should reuse the already-reviewed native-shadow Execute/Report identities
and verdict vocabulary where their semantics match. It must not accept a miner-supplied `ACCEPT`,
skip node-owned challenge accounting, or create a second looser verdict path for Mac.

## 4. Architecture decision that must close before implementation

The current frozen production environment is Linux/x86_64. Apple Silicon Macs require an ARM64
Linux guest under Apple's native virtualization path. Therefore a Mac build cannot simply package
the existing x86_64 authority and call it equivalent.

The required decision is:

- **Recommended:** produce a separately frozen Linux/arm64 execution authority and prove verdict,
  binding, negative-control and resource-policy parity against the Linux/x86_64 authority; or
- use an x86 execution/emulation layer only after separately measuring its determinism, resource
  classification and user-visible cost and freezing it as a different authority.

No silent architecture fallback is allowed. A universal Mac app also does not imply a universal
guest image: Apple Silicon and Intel artifacts, if both are supported, need separate exact-byte
manifests and release gates. Initial Intel-Mac support is a product decision, not an inferred
requirement.

## 5. Ordered delivery gates

### MAC.0 — Preserve the Linux judgement baseline

Prerequisites:

- the contained Linux checker service earns its exact frozen-ACCEPT GREEN;
- the node accepts a raw answer, independently executes the checker and durably records exactly one
  outcome across disconnect/crash/restart; and
- no block, reward or BF.7 activation is inferred from that result.

This baseline is reused; it is not rerun as a new family-calibration program.

### MAC.1 — Freeze the Mac distribution contract

Freeze before implementation:

- supported minimum macOS and hardware classes;
- Apple Silicon first versus Apple Silicon plus Intel;
- bundled image versus first-run image download;
- maximum download, installed disk, idle memory, active memory, boot-time and submission-latency
  budgets, with values set from a small measurement rather than guessed;
- app signing, notarization, virtualization entitlement and update-signing identities; and
- explicit statement that Docker/Linux/manual VM installation is not part of the user contract.

### MAC.2 — Freeze and reproduce the guest authority

- Build the selected Linux guest architecture from signed, exact inputs.
- Verify the image and every runtime authority before boot and again after update.
- Run the same frozen positive fixture and the existing negative/replay/tamper matrix.
- Prove that architecture changes do not change semantic verdicts.
- Keep architecture-specific resource measurements separate; do not assume x86 resource numbers are
  ARM upper bounds.

### MAC.3 — Implement the hidden VM lifecycle

The Mac host owns exactly one app-scoped guest lifecycle:

- create/import, start, readiness, stop and force-recovery;
- authenticated local host/guest transport with bounded frames and peer identity;
- no broad host directory sharing;
- no host wallet, model credentials, environment secrets or arbitrary sockets visible to the guest;
- image version pinning, atomic upgrade, rollback and corrupt-image recovery; and
- guest shutdown/cleanup on app uninstall or explicit reset, without deleting user wallet data.

### MAC.4 — Connect the existing node route

- Keep node challenge, replay, journal and evidence ownership on the trusted side of the boundary.
- Grant the guest one bounded execution only after the node has durably entered `InFlight`.
- Map guest unavailability to retryable availability failure, never to ACCEPT or semantic reject.
- Preserve exactly-once terminal evidence and challenge consumption across host and guest crashes.
- Do not add a parallel Mac-only checker verdict or trust a guest-generated receipt without the
  existing binding verification.

### MAC.5 — One-install Mac acceptance gate

Run on clean supported Macs with Docker, Homebrew toolchains and developer tools absent:

1. install only `Boole.app`;
2. complete first-run image preparation through the app;
3. start the checker without terminal use or administrator instructions beyond ordinary macOS app
   authorization;
4. complete one real frozen ACCEPT and all required rejection controls;
5. restart during download, boot, `InFlight`, checker execution and update, proving safe recovery;
6. exercise sleep/wake, low disk, guest crash and app crash;
7. prove no external network is used by a submission; and
8. verify uninstall/reset behavior and signed rollback.

A skipped virtualization, containment or clean-machine test is not GREEN.

### MAC.6 — Release gate

Mac-ready may be claimed only when:

- MAC.0 through MAC.5 are GREEN;
- a non-developer can install and complete one checked task without Docker, Linux or terminal work;
- the exact app and guest artifacts are signed, versioned and independently digest-verifiable;
- the current Linux security envelope is not silently weakened on Mac; and
- update and rollback retain the node's durable exactly-once semantics.

Until then, wording must be **Mac packaging planned** or **closed-local qualification**, never
**Mac production checker ready**.

## 6. Explicit non-goals

This plan does not authorize:

- a native macOS untrusted-code runner presented as equivalent without a new threat model and RED
  matrix;
- a Docker Desktop dependency for end users;
- public-network mining, leaderboard or performance claims;
- BF.7 block/reward wiring while the RP0-MD supply gate is HOLD;
- changing the current checker family, challenge contract or model-calibration result; or
- silently expanding supported Mac hardware from one measured class to all Macs.

## 7. Stop conditions

Stop and report before continuing if:

- the guest architecture cannot reproduce the frozen semantic verdicts;
- the Mac host must expose wallet/model secrets or broad writable host storage to the guest;
- a submission can escape the guest's bounded execution path or survive cleanup;
- the product requires a user-managed Docker/Linux/VM installation to work;
- image provenance or update rollback cannot be verified before execution; or
- a Mac-specific shortcut changes challenge consumption, replay or verdict classification.

## 8. Current position

This plan is a required product-distribution successor, not the current execution cursor. The
current cursor remains the contained Linux checker exact-ACCEPT closure and node raw-answer
end-to-end path. Mac implementation starts only after MAC.0's reusable judgement baseline exists.
Read-only product sizing and Apple framework research may proceed in parallel, but must not modify
the Linux authority or claim Mac readiness.

### 8.1 Current position addendum (2026-08-24 — MAC.0 sealed, MAC.1 frozen)

Section 8 above is preserved as the pre-closure checkpoint; this addendum supersedes it as the
current position. The MAC.0 baseline is COMPLETE with the evidence chain in section 9, and the
MAC.1 distribution contract is frozen in section 10 with exactly one operator value outstanding.

The next gate is **MAC.2 — freeze and reproduce the Linux/arm64 guest authority with exact
verdict parity**. MAC.2 has NOT been started: the Mac VM, the Mac production checker and every
MAC.2+ gate remain NOT implemented, and no guest download, entitlement change, paid-account
setup or production activation has occurred in the sealing slice that produced sections 9–10.

## 9. MAC.0 closure record (2026-08-24)

MAC.0 status: COMPLETE — the closed-local Linux judgement baseline of section 5 is sealed.

MAC.0 is the full prerequisite chain of section 5, not a single pull request. Every link is
closed on `main` with CI evidence:

| closed link | evidence |
| --- | --- |
| Exact frozen-ACCEPT in the contained named-Linux checker service; tampered and constant controls reach deterministic checker rejection; cgroup/rootfs/seccomp/Landlock enforcement, cleanup and same-launcher-instance/node-peer binding verified | PR #206, main `854238616605f87deb9a2d3d3a61472eaee29d2d` |
| Node-owned closed-local independent grading: the feature-gated node receives a raw answer on the fixed loopback HTTP route, resolves its own registry/grant authority, executes the contained checker through the qualified launcher, and durably records journal, evidence and terminal challenge state; exact redelivery returns the stored terminal result without re-execution; the `native_busy` permit limits execution to one; an outcome unprovable as terminal fails closed as `adjudication_unknown`, `retryAuthorized=false` | PR #219, main `4de603f63d48259674ffd78fa3072bc7d7e9b5bf` |
| Launcher security invariants pinned across restart and qualification: a restarted launcher grant binds the consumed durable attempt read-only (no cross-restart reuse), an execution-time authority digest mismatch is rejected, a byte-identical ready token minted under an earlier qualification nonce never qualifies a later session, and a kernel-reported zero-PID peer stays untrusted | PR #220, main `9203156950e178277895ac4d282462147ddae23e` |
| Real process crash/restart exactly-once E2E on Linux CI: SIGKILL of the real node and launcher processes followed by a full restart preserves at-most-once adjudication and byte-exact terminal redelivery, and unresolved durable `InFlight` rows keep the replay route fail-closed after restart | PR #221, main `6553360a6291c300ad0d19c50238b8b7c9263c68`; CI run <https://github.com/NotoriAndo/Boole/actions/runs/32709400913> |

Measured crash/restart evidence from that CI run. Scenario S1 (`terminal-redelivery-across-node-kill`):
2 checker starts before the kill, 0 checker starts after restart, 3 redelivered responses, the
accepted redelivery byte-identical, the tampered redelivery differing only in the expected
redelivery flag, 10 journal rows, node and launcher PID/start-time/systemd invocation identity all
changed across the restart, and cleanup verified including reaping the inert launcher socket
inode. Scenario S2 (`unresolved-inflight-fail-closed`): 0 checker starts, 3 journal rows, restart
refused with the fail-closed message observed, the listener still refused after a restart
attempt, and cleanup verified.

Boundary of this record: this is a closed-local, loopback-only, frozen non-issuable qualification
on Linux CI. It is not public mining, not a public-network/leaderboard/paid-API claim, and not a
Mac product result.
The Mac VM, the Mac production checker and every MAC.2+ gate remain NOT implemented.
Invariants unchanged: `LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false`, `activationAllowed=false`;
consensus, block, reward and P2P paths are untouched.

## 10. MAC.1 frozen distribution contract (2026-08-24)

MAC.1 status: **MAC.1-PARTIAL — OPERATOR VALUE REQUIRED.** Exactly one operator value remains
open (the product minimum macOS version, 10.2 item 1); every other item below is frozen. This
contract is frozen before any Mac implementation, measurement or artifact exists. No line below
is a measurement claim unless it explicitly says "measured"; pre-set limits are marked
**PRE-FROZEN PRODUCT CAP** (a product commitment later results must fit) and unmeasurable items
are marked **NOT-YET-MEASURED** with the gate that must measure and freeze them.

### 10.1 Qualitative contract (frozen)

1. **Apple Silicon first.** v1 targets Apple Silicon Macs (M1 and later) as the candidate range.
   The release-claimable range at MAC.6 covers only machine classes actually exercised by the
   MAC.5 acceptance run; support is never silently expanded beyond them (section 6).
2. **Intel Macs are NOT claimed as supported.** Intel support would need its own separately
   frozen authority, artifacts, manifests and verification gates (section 4); it is never
   inferred from an arm64 result.
3. **One-install contract.** The user installs exactly one signed and notarized `Boole.app`.
   Requiring the user to install Docker Desktop, a Linux distribution, a VM manager UI, Homebrew,
   Rust, Python or any system service is forbidden. User-facing documentation never requires
   direct VM management.
4. **Hidden guest lifecycle.** The app itself creates, verifies, starts, stops, updates and
   recovers the Linux guest. The guest is not user-visible or user-administered.
5. **Authority non-equivalence.** The current Linux/x86_64 execution authority is never treated
   as equal to Linux/arm64. MAC.2 must produce and verify a Linux/arm64 successor authority (or a
   separately frozen, separately measured alternative) with exact semantic-verdict, binding,
   negative-control and resource-policy parity before any Mac execution claim.
6. **Trust boundary.** Host and guest communicate only over an authenticated, bounded local
   channel. The guest receives no node, wallet, reward or consensus authority, no model
   credentials and no broad host filesystem access; it returns canonical results only.
7. **No stale reuse.** After app quit, force-quit or Mac reboot, no prior VM instance, session,
   qualification or grant is reused; every execution session re-qualifies from scratch, matching
   the restart invariants pinned by PR #220/#221.
8. **Update and rollback.** A failed app or guest update rolls back to the last verified
   app+guest combination. An app/guest version mismatch fails closed instead of running.
9. **Offline behavior.** An installed and verified guest may operate offline. Arbitrary
   downloads, unpinned hosts and unverified fallback images are forbidden.
10. **Docker is developer-diagnostic only** — not a consumer dependency, not a security boundary
    and not a release prerequisite.

### 10.2 Numeric contract (frozen values, pre-frozen caps and measurement gates)

Basis rule: every number below comes from (a) an already-frozen in-repo containment authority
value, (b) the measured pinned x86_64 rootfs input closure, or (c) Apple's own documentation.
Nothing below is invented as a measurement.

1. **Product minimum macOS version — OPERATOR VALUE REQUIRED.** The single open operator choice.
   Apple-documented floor: the Virtualization framework is available on **macOS 11.0+**, supports
   Linux guests first-party, and requires the `com.apple.security.virtualization` entitlement
   (<https://developer.apple.com/documentation/virtualization>). The product minimum may never be
   set below that floor; the recommendation is to choose a macOS version still receiving Apple
   security updates at MAC.5 acceptance time. Needed no later than MAC.5 planning.
2. **Hardware range:** Apple Silicon (M1 and later) candidate range; release scope per 10.1
   item 1. **Intel: not claimed** (10.1 item 2).
3. **App download cap:** ≤ 536,870,912 bytes (512 MiB). PRE-FROZEN PRODUCT CAP — reuses the
   already-frozen per-artifact ceiling `networkPolicy.maxArtifactBytes` in
   `native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json`. The actual app
   size is NOT-YET-MEASURED (no app exists); measured and enforced at MAC.5/MAC.6.
4. **Guest image download cap:** ≤ 2,147,483,648 bytes (2 GiB) total. PRE-FROZEN PRODUCT CAP —
   reuses the already-frozen `networkPolicy.maxTotalBytes` from the same acquisition authority.
   Measured reference point: the pinned x86_64 rootfs input closure is 63 artifacts totalling
   211,616,381 bytes (~202 MiB) in
   `native/containment/native-shadow-runtime-rootfs-source-lock-v2.json`. The arm64 guest image
   size is NOT-YET-MEASURED; MAC.2 must measure it and prove it fits this cap.
5. **Installed guest tree cap:** ≤ 2 GiB total, ≤ 536,870,912 bytes per file, ≤ 200,000 entries.
   PRE-FROZEN PRODUCT CAP — inherits the frozen `buildRecipe.maxTotalBytes` / `maxFileBytes` /
   `maxEntries` in the rootfs source lock.
6. **Update extra-disk budget:** ≤ 2.5 GiB — exactly one additional retained verified app+guest
   combination (guest cap 2 GiB + app cap 512 MiB), required for the 10.1 item 8 rollback.
   PRE-FROZEN PRODUCT CAP derived by arithmetic over items 3 and 5.
7. **Log/cache retention cap:** ≤ 536,870,912 bytes (512 MiB). PRE-FROZEN PRODUCT CAP — reuses
   the frozen 512 MiB byte class already used by `workspace.sizeBytes` and
   `networkPolicy.maxArtifactBytes`; not a measurement.
8. **Post-install total disk cap:** ≤ 5.5 GiB = app 512 MiB + active guest 2 GiB + one rollback
   combination 2.5 GiB + logs/caches 512 MiB. PRE-FROZEN PRODUCT CAP derived by arithmetic over
   items 3, 5, 6 and 7.
9. **VM memory ceiling:** ≤ 4,294,967,296 bytes (4 GiB). PRE-FROZEN PRODUCT CAP — must dominate
   the frozen per-submission checker budget of `resourceLimits.memoryMaxBytes` = 2 GiB with
   swap 0, plus the memory-backed execution workspace tmpfs `workspace.sizeBytes` = 512 MiB,
   plus appliance overhead (NOT-YET-MEASURED), all from
   `native/containment/native-shadow-execution-policy-v1.json`. The **default** VM memory is
   NOT-YET-MEASURED: MAC.3 must measure and freeze it and prove it fits this ceiling.
10. **vCPU default and maximum:** NOT-YET-MEASURED — measurement gate MAC.3 (measure and
    freeze) and MAC.5 (enforce). Guest-internal frozen facts that bound the need: checker
    `concurrency` = 1 and `pidsMax` = 128.
11. **First-preparation time cap, warm-start time cap, VM shutdown/cleanup time cap:**
    NOT-YET-MEASURED — no VM exists to measure. Measurement gate: MAC.3 measures and freezes the
    values; MAC.5 enforces them on clean hardware; MAC.6 may not be claimed while they are
    unfrozen. Guest-internal per-submission cleanup already carries the frozen 10,000 ms
    `cleanupDeadlineMillis`.
12. **Submission-latency budget:** the guest-internal execution envelope is already frozen —
    `outerWallMillis` = 100,000 ms, `cpuTotalUsec` = 120 s CPU,
    `executionResponseTimeoutMillis` = 115,000 ms. The Mac product may add only host↔guest
    transport overhead on top; that overhead is NOT-YET-MEASURED — measurement gate MAC.4.

### 10.3 MAC.2 inputs and STOP conditions (frozen)

MAC.2 inputs: the frozen x86_64 authority set (`native-shadow-runtime-rootfs-source-lock-v2.json`,
`native-shadow-execution-policy-v1.json`, toolchain identity), the sealed MAC.0 judgement
baseline (frozen positive fixture plus the existing negative/replay/tamper matrix), and the caps
in 10.2 items 4–5. MAC.2's deliverable is a separately frozen Linux/arm64 successor authority (or
the explicitly frozen alternative of section 4) whose semantic verdicts, bindings, negative
controls and resource policy are proven in parity with x86_64, with architecture-specific
resource measurements kept separate.

MAC.2 STOP conditions — stop and report instead of continuing if:

- the arm64 guest cannot reproduce the frozen semantic verdicts exactly;
- required arm64 artifacts are unavailable from pinned, verifiable sources;
- any 10.2 cap cannot be met without weakening the Linux containment contract;
- the work would require paid accounts, unpinned downloads or production activation; or
- parity evidence would have to be inferred rather than executed.

### 10.4 MAC.1 completion accounting

- Supported Mac range: fixed (10.1 items 1–2; 10.2 items 1–2), minus the single operator value.
- Install contract: fixed (10.1 items 3–4).
- App/guest trust boundary: fixed (10.1 items 5–7).
- Resource/time/download budgets: fixed as PRE-FROZEN caps or explicit measurement gates
  (10.2 items 3–12).
- Update/rollback/fail-closed contract: fixed (10.1 items 8–9; 10.2 item 6).
- MAC.2 inputs and STOP conditions: fixed (10.3).
- Mac implementation, downloads, paid-account work performed: **0**.

Because exactly one numeric item (the product minimum macOS version) is an operator decision that
cannot be derived from existing artifacts or Apple documentation alone, the overall status is
**MAC.1-PARTIAL — OPERATOR VALUE REQUIRED** rather than COMPLETE. Supplying that one value
upgrades MAC.1 to COMPLETE with no other open items.
