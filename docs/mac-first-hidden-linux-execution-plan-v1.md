# Mac-first hidden Linux execution plan v1

Status: **MAC.0 COMPLETE (closed-local Linux baseline, 2026-08-24, section 9);
MAC.1-PARTIAL — CURL-FIRST MODE FROZEN; UPDATE TRUST POLICY AND MEASUREMENT PROTOCOL OPEN
(product-form correction 2026-08-25, section 14; macOS 14/M1/Intel decision remains fixed);
MAC.2-PARTIAL — CLOSED-LOCAL LINUX/ARM64 AUTHORITY PARITY COMPLETE; STAGED VERIFIER
CORE/KAT GREEN; PRODUCTION AUTHORIZATION, ADOPTION AND POST-ADOPTION REVERIFICATION OPEN
(sections 11 and 13); CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT (section 14 supersedes the
Boole.app/Developer-ID/Team-ID product-form decision); CURL.1 CONTRACT/VERIFIER GREEN —
RELEASE CONTRACT AND GUEST BOOT FORMAT FROZEN (section 15); CURL.2-CORE INSTALLER CORE GREEN —
VERIFIED ATOMIC LOCAL ADOPTION WITH A DURABLE REPLAY FLOOR (section 16); CURL.2-TRANSPORT
GREEN — FAIL-CLOSED BUNDLE DOWNLOAD/STAGING AND THE `boole product install` CURL ENTRYPOINT;
REAL RELEASE ARTIFACT AND PRODUCTION TRUST ROOT ABSENT (section 17); CURL.3-PREP CONTRACT
FROZEN — CLEAN-MAC TEAM-ID-FREE ENTITLEMENT CANARY GROUNDS MACHINE-CHECKED; CURL.3
DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED (sections 18–19); BOOTABLE GUEST-UPDATE V2
AND PRODUCT-RELEASE V2 CONTRACT GREEN — REAL BOOT ARTIFACTS AND V2 INSTALL CONSUMER ABSENT;
BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 GREEN — ROOTFS CLOSURE AND SYSTEMD EXECUTION POLICY AUDITED,
KERNEL/SYSTEMD-GUEST/IMAGE-BUILDER AUTHORITIES UNDEFINED AND NO BOOT ARTIFACTS PRODUCED
(section 20); BOOT-GUEST-INIT-COMPATIBILITY-V1 CONTRACT GREEN — CURRENT BASELINE MISSING SEVEN
REQUIREMENTS (section 21); BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1 FROZEN — SIGNED-METADATA
SELECTION ONLY (section 22); BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1 GREEN — 191/191 PACKAGE
PAYLOADS ACQUIRED AND VERIFIED, SOURCE LOCK/BOOT AUTHORITY STILL ABSENT (section 23);
ROOT-DISK-BYTE-IDENTITY GREEN — TWO ARM64 REPLICAS CONVERGED ON ONE ROOT DISK, THE §25 FAILURE'S
CAUSE REMOVED AND THE SEALED FAILURE RECORD LEFT AS WRITTEN (section 28); THIS IS A DETERMINISTIC
WRITER, NOT A BOOTED GUEST;
MAC.3 CLOSED-LOCAL BOOT ATTEMPTED ONCE, VERDICT FAIL — THE KERNEL BOOTED FROM THE SEALED IMAGE AND
MOUNTED ITS EXT4 ROOT READ-ONLY, AND PID 1 FROZE BECAUSE THE ROOT FILESYSTEM HAS NO `/proc`, `/sys`
OR `/dev` MOUNT POINTS (section 29); THE ATTEMPT WAS NOT REPEATED AND THE CONDITIONS WERE NOT
RELAXED — NOT RELEASE-READY, NO ACTIVATION AUTHORITY.**

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

Timing correction: section 10.6 supersedes the phrase “values set from a small measurement before
implementation” above. Before implementation, MAC.1 freezes binding caps, workloads, collection
methods and acceptance rules; the actual values are earned only by MAC.2–MAC.5 when the relevant
artifact or product path exists.

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

### 8.2 Current position addendum (2026-08-24 — operator value supplied, MAC.1 COMPLETE)

Section 8.1's single outstanding operator value is now supplied in section 10.5: the product
minimum is **macOS 14.0 (Sonoma)** on **Apple Silicon (M1 or later)**, Intel Macs are outside
the v1 support scope, and MAC.1 is COMPLETE. The next gate remains **MAC.2 — freeze and
reproduce the Linux/arm64 successor guest authority with exact verdict parity**, and MAC.2 has
NOT been started.

### 8.3 Current position correction (2026-08-25 — MAC.1 accounting corrected; MAC.2 closed-local parity complete)

Sections 8.1–8.2 are preserved as historical checkpoints. Their statement that MAC.1 had only
one open value and became COMPLETE is superseded by section 10.6: the minimum macOS/hardware
decision is complete, but the distribution mode, public release identities and measurement
protocol required by section 5 remain open. Product values are later unearned results, not MAC.1
prerequisites.

The closed-local Linux/arm64 guest-authority parity subgate recorded in section 11 is COMPLETE,
but MAC.2 as a whole remains PARTIAL because the authenticated staged-update verifier and the
post-adoption image/runtime-authority reverification have not run. That result created no Apple
Virtualization VM, `Boole.app`, signed/notarized artifact, update channel or Mac production checker.
MAC.3 is BLOCKED / NOT STARTED until the MAC.1 product choices/measurement protocol are frozen and
the MAC.2-B staged verifier is implemented and GREEN; it is not blocked on measurements that only a later working product
path can produce. MAC.4–MAC.6 remain unstarted. No later gate may use MAC.2-A to bypass those inputs.

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

### 10.5 MAC.1 closure addendum (2026-08-24 — operator value supplied)

MAC.1 status: COMPLETE. Sections 10–10.4 are preserved unchanged as the frozen contract record
(including the historical MAC.1-PARTIAL status they carried); this addendum supplies the single
outstanding operator value of 10.2 item 1 and closes the completion accounting of 10.4. No other
contract line changes.

Operator decision (2026-08-24):

- Product minimum macOS version for Boole for Mac v1: **macOS 14.0 (Sonoma)**.
- Supported hardware: **Apple Silicon (M1 or later)**, per 10.1 item 1 and 10.2 item 2.
- **Intel Mac is not supported by v1** — outside the v1 support scope, per 10.1 item 2. Intel
  support is not designed, claimed or implied by this closure.

Decision basis (operator-supplied sources, re-checked against the cited pages on 2026-08-24):

- macOS 14.0 is at or above the Apple-documented Virtualization framework floor of macOS 11.0
  (10.2 item 1), so this choice narrows and never violates the frozen contract.
- Apple's official GUI Linux VM sample ships with a default deployment target of macOS 14
  (<https://developer.apple.com/documentation/virtualization/running-gui-linux-in-a-virtual-machine-on-a-mac>).
- macOS Sonoma supports every Apple Silicon Mac including the first M1 machines — MacBook Air
  (M1, 2020), MacBook Pro (M1, 2020), Mac mini (M1, 2020)
  (<https://support.apple.com/en-la/105113>) — so this minimum excludes no machine in the 10.1
  item 1 candidate range.
- The Virtualization framework path continues to require the
  `com.apple.security.virtualization` entitlement
  (<https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.virtualization>)
  and, on Apple Silicon, an ARM64 Linux guest
  (<https://developer.apple.com/documentation/virtualization/running-linux-in-a-virtual-machine>),
  matching the 10.1 item 5 requirement that MAC.2 prove Linux/arm64 authority parity.

Completion accounting after this addendum: every 10.4 line stands, and the single open operator
value is now fixed, so MAC.1 is COMPLETE with no open items. The remaining NOT-YET-MEASURED
entries in 10.2 are measurement gates owned by MAC.2/MAC.3/MAC.4/MAC.5 by design; they are not
open MAC.1 items. The frozen contract of 10.1–10.3 stays binding as written: one signed and
notarized `Boole.app` install with no user-facing Docker Desktop/Linux/VM-UI/Homebrew/Rust/Python
requirement, the hidden app-managed guest lifecycle, the ≤512 MiB app download cap, the ≤2 GiB
guest download cap, the ≤5.5 GiB post-install disk cap, the ≤4 GiB VM memory ceiling, the
one-generation update rollback, the app/guest version-mismatch fail-closed rule, and
offline operation only for an already-verified guest.

The execution cursor moves to **MAC.2 — freeze and reproduce the Linux/arm64 successor guest
authority with exact verdict parity** (inputs and STOP conditions frozen in 10.3).
MAC.2 has NOT been started.
This addendum is a docs-only closure: it grants no MAC.2 implementation authority, no production
activation authority, and no block or reward authority.
The Mac production checker is NOT ready; no VM was created or run, no guest was downloaded, no
entitlement was changed, and no Xcode project was created in this slice.
Invariants unchanged: `LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false`, `activationAllowed=false`; consensus, block,
reward and P2P paths are untouched.

### 10.6 MAC.1 completion-accounting correction (2026-08-25)

Section 10.5 is preserved as the historical operator-decision record, but its claim that the
macOS minimum was the only open MAC.1 item is incorrect relative to section 5 and is superseded
here. The macOS 14.0 minimum, Apple Silicon M1-or-later scope, Intel exclusion and
no-Docker/manual-Linux user contract remain fixed.

The still-open MAC.1 items are exactly:

1. **Distribution mode:** choose and freeze bundled guest versus first-run guest download,
   including the public distribution/update channel.
2. **Measurement protocol:** freeze the existing product caps, measurement workloads, collection
   method and acceptance rule before implementation. Measurement results are earned in MAC.2–MAC.5
   at the gates that first create the relevant artifact or product path; they cannot be prerequisites
   for that path's implementation. Existing PRE-FROZEN PRODUCT CAP values remain binding ceilings,
   not measurements. MAC.2's authority-input/layer/checker measurements are evidence for one guest
   artifact only and do not replace the later app/VM/product-path measurements.
3. **Public release identities and policy:** freeze the Bundle ID, Apple Developer Team ID,
   public signing-certificate fingerprint/policy, notarization and stapling policy, exact
   entitlement set (including virtualization), hardened-runtime/codesign options, App Sandbox
   decision and boundary, and the public app/guest update trust keys with rotation, revocation and
   rollback rules. Private keys, passwords, notary credentials and other secrets must never be
   written to this repository.

These three buckets are the unresolved portions of section 5's third, fourth and fifth bullets;
they do not reopen its completed first, second or sixth bullets. Therefore the current status is
**MAC.1-PARTIAL — DISTRIBUTION MODE, PUBLIC IDENTITY, AND MEASUREMENT PROTOCOL REQUIRED**.

The ordered gates are a dependency graph rather than an impossible demand for measurements before
their producer exists:

- **MAC.2-A — architecture authority parity:** complete by PR #224.
- **MAC.2-B — authenticated staged-update verification:** before MAC.3 may adopt an image, an
  offline verifier must authenticate the operator-frozen public update trust root and manifest,
  then verify every candidate image/runtime authority byte before adoption.
- **MAC.2-C — post-adoption re-verification:** MAC.3 implements atomic adoption/rollback and MAC.5
  exercises a real update followed by the same authority verification and verdict matrix.

MAC.3 may start only after MAC.1 freezes the distribution/update choices, public trust policy and
measurement protocol, and after MAC.2-B is GREEN. It does not wait for measurements assigned to
MAC.3–MAC.5 or for MAC.2-C, because those require the lifecycle it creates. This sequencing correction
changes no runtime authority and does not weaken any pre-frozen cap or final release gate.

## 11. MAC.2 closed-local Linux/arm64 authority-parity subgate closure (2026-08-25)

MAC.2 status: **MAC.2-PARTIAL — CLOSED-LOCAL LINUX/ARM64 AUTHORITY PARITY COMPLETE; STAGED VERIFIER AND POST-ADOPTION REVERIFICATION OPEN.** PR #224
(main `2a6de07ba6c77355d19a3d342ab718f7358fd76a`; CI
<https://github.com/NotoriAndo/Boole/actions/runs/32766488279>) rebuilt the separately frozen
authority on a native Linux/aarch64 runner and executed, rather than inferred, the parity matrix.
The executed result reports `mac2Status=PARTIAL`, completed subgate
`CLOSED-LOCAL-LINUX-ARM64-AUTHORITY-PARITY`, and open requirement
`POST-UPDATE-IMAGE-AND-RUNTIME-AUTHORITY-REVERIFICATION`.
That last value is the executed artifact's umbrella status field, not the dependency scheduler;
section 10.6 now separates its remaining work into MAC.2-B and MAC.2-C.

The accepted fixture and its byte-identical replay returned `accepted/accepted`; empty returned
`deterministic_reject/malformed_patch_region`; tampered and constant returned
`deterministic_reject/compile_or_hidden_test_failed`; both cross-task directions returned
`deterministic_reject/outside_patch_modified`. The named containment, HTTP replay and
crash/restart gates also passed under the arm64 authority.

Frozen authority SHA-256 values:

| authority | SHA-256 |
| --- | --- |
| registry | `d636e56dbf7e32d6054a1d4abfaeb97c6ebdf5119d217fe7740db0513984badd` |
| execution policy | `df8be9eb7f3d92335d22b95a7e9423d8baaa2d581a2fd3b3633f60ae63db4e3f` |
| toolchain identity | `666cdd6a6908822b35a3839e905ab03bed2846ce8e49091ccd163b5f59947f36` |
| checker release | `23b9c235a638cf08d38b2082af19d599320c9e5e5fc785bc1e14f51b4667f210` |
| registry overlay | `2962adef8d1aea9ba1c8466b8e014b71f1ec3c9555ce8b685d58ede6b631fe74` |
| closed-local replay grant | `bd5cd9fc87e5e47a23e6fa12844ec0c47bdb01ee34090cddff24568c18d7236f` |
| local execution authority | `b7ef42d084adb8d660d7446092d768546cb555a868d2bbe7a5d6f4f9b1985d09` |
| closed replay authority | `d220d20b7adaa22357929729d2f0666a8c9cbe50ce8031f90539ba1309950c6b` |

The source closure is **62 artifacts / 56 Ubuntu packages / 181,623,999 bytes** with result SHA-256
`79073e541856c9be3bfbf56bf9c4415677679dc994c1342902f631716db7f312`. The OCI rootfs layer is
`sha256:dfeafb2918764736bdcd94d0fd121ed8eee2ef88d0a82e1ef28b3e625723bc0d`,
**766,556,160 bytes**. Its content manifest is
`200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa`,
**1,285,116 bytes**, covering 4,216 entries.

The executed `MAC2-RESULT.json` recorded `"authorityInputBytes": 181623999`,
`"rootfsContentEntryCount": 4216` and these per-case measurements:

| case | wall seconds | max RSS KiB |
| --- | ---: | ---: |
| accepted | 0.39 | 139,296 |
| accepted replay | 0.39 | 139,168 |
| empty | 0.23 | 36,280 |
| tampered | 0.40 | 139,264 |
| constant | 0.39 | 137,376 |
| cross real to synthetic | 0.24 | 36,280 |
| cross synthetic to real | 0.24 | 36,280 |

The architecture-feature compile guard prevents an arm64 authority from being selected on Linux
x86_64 and prevents the default x86_64 authority from being selected on Linux/aarch64. The
existing x86_64 containment/rootfs gate remained GREEN. Its execution policy, rootfs inputs,
`checker.py` and x86_64 release manifest bytes were unchanged; the shared `SHA256SUMS` release
channel index changed only to cover the newly added arm64 release manifest.

Boundary: this platform is Linux/arm64, not macOS; `productionByteProvenanceComplete=false` and
`activationAllowed=false`. No Apple VM/app/signing/notarization/update lifecycle was built or
tested. The authenticated staged verifier (MAC.2-B) and real post-adoption reverification
(MAC.2-C) remain open. These measurements are architecture evidence, not the later MAC.3–MAC.5
product-path measurements, which are not MAC.3 prerequisites.
`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged.

## 12. MAC.1 operator decision addendum without an Apple Team ID (2026-08-25)

Current decision status: **MAC.1-DECISIONS-FROZEN — TEAM-ID-AND-PRODUCTION-TRUST-ROOT-OPEN**.
This addendum closes the three product choices explicitly approved by the operator; it does not
pretend that an unavailable Apple or update-signing identity exists.

1. **Guest delivery:** `Boole.app` remains small and uses a first-run download of a verified Linux/arm64 guest.
   The app must authenticate the signed manifest and verify the exact artifact
   size and SHA-256 before the bytes may enter the staged install area. The app must not build the
   guest from source on the user's Mac and must not require Docker, Linux, Homebrew, Rust, Python or
   a terminal-managed VM.
2. **Initial public channel:** the Mac product uses **Developer ID direct distribution**, not the
   Mac App Store. **GitHub Releases** is the initial public channel for both the app and versioned
   guest assets. A GitHub transport success is not trust: only the pinned update-verification
   policy may authorize staged bytes.
3. **Public app name:** the Bundle ID is **`io.github.NotoriAndo.Boole`**. The minimum product range
   remains macOS 14.0 or later on Apple Silicon M1 or later; Intel remains outside v1.

The following identity boundary remains deliberately open:

- The Apple Developer Team ID is not available. No placeholder Team ID, certificate fingerprint,
  entitlement profile or notarization identity may be invented.
- No production private key is generated or stored by this slice. Passwords, Developer ID
  certificates, notary credentials and guest-update signing secrets remain outside the repository.
- The production guest-update public trust root, its rotation/revocation policy and the first
  signed release manifest remain operator-owned inputs. Until they are frozen, a staged verifier
  cannot authorize a production update.
- MAC.2-B-CORE may be tested with a non-production KAT key (a public known-answer-test identity),
  but that key must be visibly test-only, must not be compiled as a production trust root and must
  not upgrade MAC.2-B or MAC.1 to COMPLETE.

The measurement protocol bucket from section 10.6 also remains open. Existing byte, disk and
memory caps continue to be binding ceilings, while the exact clean-Mac workloads, repetitions,
collection method and acceptance rule must be frozen before MAC.3 product measurement begins.

Therefore the current dependency cursor is:

```text
MAC.0    COMPLETE
MAC.1    PARTIAL — delivery/channel/Bundle ID frozen; Team ID, production trust policy,
                    signing/notarization details and measurement protocol open
MAC.2-A  COMPLETE — closed-local Linux/arm64 authority parity
MAC.2-B-CORE  AUTHORIZED — offline verifier implementation with injected trust root and KAT tests
MAC.2-B  OPEN — production public trust root + signed manifest + exact candidate artifact required
MAC.2-C  OPEN — real post-adoption re-verification
MAC.3    BLOCKED / NOT STARTED
```

No app bundle, VM lifecycle, network downloader, signing certificate, release upload or activation
is authorized by this addendum. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged.

## 13. MAC.2-B offline verifier core/KAT closure (2026-08-25)

Current subgate status: **MAC.2-B-CORE/KAT GREEN**. PR #226 (main
`fb7142d21129852847ff1ab6c19ca3deb9713692`; CI
<https://github.com/NotoriAndo/Boole/actions/runs/32790547865> and
<https://github.com/NotoriAndo/Boole/actions/runs/32790547760>) added the portable, offline
staged-update verifier core to `boole-core`. Its focused verifier suite passed 23/23, and the full
required CI set passed on Linux x86_64, Linux arm64 and the existing corpus/supply-chain lanes.

The verifier accepts an injected Ed25519 public trust root, authenticates a canonical manifest in
the fixed `boole-native-shadow-guest-update-v1` domain, requires `stable/linux/aarch64`, and binds
exactly ten fixed authority roles. It streams each artifact once while enforcing its exact length
and SHA-256 and enforces the frozen aggregate 2 GiB ceiling. It retains the verified artifact
descriptors so a later adoption layer can consume the same verified bytes rather than silently
selecting a replacement by path. Unknown fields, noncanonical or duplicate JSON, malformed or
small-order public keys, wrong signatures/domain/target, missing, truncated or tampered artifacts,
rollback, replay, predecessor mismatch and sequence overflow all fail closed.

First install requires an explicit operator-provided minimum sequence and an explicit null
predecessor. An installed update must have a strictly newer sequence and bind its predecessor to
the exact active manifest digest. The code deliberately has no permissive genesis default. These
are verifier-core invariants only; they do not create an install transaction or product authority.

Boundary: the **production trust root remains absent**. The repository contains no production
private key, Team ID, Developer ID certificate, notarization credential, downloader, release
upload, durable update floor, atomic same-file-descriptor adoption, rollback implementation, VM
lifecycle or post-adoption execution/reverification. The KAT identity is visibly test-only and is
not compiled as a production trust root. Therefore **MAC.2-B production OPEN**, **MAC.2-C OPEN**
and **MAC.3 BLOCKED / NOT STARTED** remain the only valid product accounting.

The current dependency cursor is:

```text
MAC.0    COMPLETE
MAC.1    PARTIAL — product choices frozen; Team ID, production signing/trust policy and
                    measurement protocol open
MAC.2-A  COMPLETE — Linux/arm64 authority parity
MAC.2-B-CORE/KAT GREEN — offline injected-key verifier and known-answer tests
MAC.2-B production OPEN — real public trust root, signed release manifest, durable staged
                          adoption and initial sequence pin not supplied
MAC.2-C  OPEN — adopted bytes must be reverified and execute the frozen verdict matrix
MAC.3    BLOCKED — no hidden VM lifecycle work may start through this closure
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. This is a
closed-local implementation and CI result, not public mining, an API benchmark, a released Mac
product or a production update authorization.

## 14. Curl-first CLI/service distribution correction (2026-08-25)

Current product form: **CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT**. The operator clarified
that “app” had been shorthand for the installed product, not an instruction to ship a GUI app
bundle. The earlier **Boole.app distribution decision is SUPERSEDED**. Its historical text remains
above so the correction is auditable, but it no longer controls implementation or completion
accounting.

The supported user journey is:

```text
one reviewed curl command
→ curl installer → verified prebuilt macOS arm64 CLI and host controller
→ exact signed-manifest verification of the versioned Linux/arm64 guest
→ CLI-managed, user-invisible Linux checker service
→ node-owned durable verdict/evidence/replay behavior
```

The installed product is a command-line interface plus an internal host controller/service. A GUI,
`.app` bundle, App Store listing and Bundle ID are not v1 requirements. The user may use the
terminal for the single install command and normal Boole CLI commands, but must not need to install
or administer Docker Desktop, Homebrew, Rust, Python, Linux, a VM application or a guest login.

**Team ID is not a prerequisite for the curl-first path.** Developer ID signing and Apple
notarization are optional future distribution hardening, not blockers for the release/installer
architecture or for closed-local implementation. The macOS process that calls Virtualization.framework
must still carry `com.apple.security.virtualization` in its code signature. Whether the chosen
Team-ID-free/ad-hoc signing form works on a clean supported Mac is an explicit measured acceptance
gate; it must not be inferred from a developer machine or bypassed by relying on quarantine quirks.
If that clean-Mac gate fails, stop and choose a separately reviewed execution alternative rather
than silently requiring an Apple identity or weakening containment.

GitHub Releases remains the initial transport for immutable, versioned CLI/controller/guest
artifacts. Transport is not authority. The injected Ed25519 update trust policy, canonical signed
manifest, exact sizes and SHA-256 values remain the authority. The production update public key,
private signing custody, initial sequence floor and rotation/revocation procedure still require an
operator-owned release decision, independently of any Apple Team ID.

The current `install.sh` command is a source/developer bootstrap: it clones `main` and installs
Git, Rust, Lean, Python and build tools. It is not the finished curl product installer and must not
be relabelled as one. The product successor must install an immutable release version, verify its
manifest/signature and exact artifacts before adoption, avoid developer toolchains on the user's
Mac, and provide deterministic update, rollback and uninstall behavior.

The earlier app-shaped acceptance language maps to the curl-first product as follows; it is not an
additional GUI requirement:

- MAC.3's `app-scoped` lifecycle and `app quit` boundary mean an install-scoped host
  controller/service lifecycle, explicit service stop or uninstall, and Mac reboot. No prior guest
  instance or session may survive any of those boundaries.
- MAC.5's “install only `Boole.app`” requirement means one reviewed curl install followed by normal
  Boole CLI use. Terminal use for Boole is allowed; manual VM, Linux or developer-toolchain
  administration remains forbidden.
- MAC.6's signed-app artifact means the complete host payload (CLI plus controller/service) verified
  by the Boole release manifest. The old 512 MiB app cap becomes a 512 MiB total host-payload cap;
  the guest and rollback caps remain unchanged.

Unchanged evidence: **MAC.2-B-CORE/KAT GREEN remains valid**. MAC.0 exactly-once behavior,
MAC.2-A Linux/arm64 parity, the Linux containment authority and PR #226's offline update verifier
are independent of GUI packaging and remain valid.

Corrected execution cursor:

```text
CURL.0  COMPLETE — curl-first CLI/service product contract corrected and frozen
CURL.1  OPEN — immutable prebuilt macOS arm64 CLI/controller/guest release contract
CURL.2  OPEN — product installer v2 with signature/hash/version verification and no dev toolchain
CURL.3  OPEN — clean macOS 14/M1 Team-ID-free virtualization-entitlement canary
MAC.2-B-CORE/KAT GREEN — reusable offline verifier
MAC.2-B production OPEN — real update trust root, first signed manifest and durable adoption open
MAC.3-CLI BLOCKED / NOT STARTED — CLI-managed hidden VM lifecycle follows CURL.1–CURL.3 evidence
MAC.2-C / MAC.4–MAC.6 OPEN — post-adoption, host/guest, clean-install and release gates remain
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No public
mining, paid API benchmark, release upload, user installation or production activation occurred.

## 15. CURL.1 — product release contract and guest boot format freeze (2026-08-25)

CURL.1 status: **CONTRACT/VERIFIER GREEN — INSTALLER, REAL RELEASE ARTIFACT AND PRODUCTION
TRUST ROOT ABSENT.** This section freezes the immutable release contract of the curl-first
product and the boot format of the versioned Linux/arm64 guest, and records their closed-local
verifier implementation. No release artifact was built or uploaded, no installer exists, no
download occurred, and no production key, certificate or Apple identity was created.

### 15.1 Guest boot format decision — direct Linux boot (option A) is FROZEN

The versioned guest boots by **direct kernel boot with `VZLinuxBootLoader`**
(<https://developer.apple.com/documentation/virtualization/vzlinuxbootloader>), available since
macOS 11.0 and therefore fully covered by the frozen macOS 14.0 (Sonoma) support floor. The
frozen boot contract is `bootFormatVersion=1` and names three exact host-side files that the
signed guest manifest must pin by exact byte size and SHA-256 once they are produced:

- `guest-kernel` — an uncompressed arm64 Linux kernel `Image` (Virtualization.framework does
  not accept a gzip-compressed `Image.gz` on Apple silicon);
- `guest-initrd` — the initial ramdisk; and
- `guest-root-disk` — a read-only root filesystem disk image built from verified rootfs content.

Each boot input is a plain host-side file that the host controller can hash-pin before any VM
configuration exists. The existing 2 GiB total guest cap is unchanged and covers these roles.

The EFI alternative, `VZEFIBootLoader`
(<https://developer.apple.com/documentation/virtualization/vzefibootloader>), is REJECTED for
v1: it requires an in-guest bootloader (GRUB or systemd-boot) inside the disk image, which is
not part of the pinned Ubuntu package closure, and it requires a mutable `VZEFIVariableStore`,
which contradicts immutable per-file hash pinning of every boot input.

**The current verified OCI rootfs content is NOT a bootable VM image.** The MAC.2-A closure
pinned an OCI-layout Ubuntu rootfs (62 artifacts / 4,216 content entries); it contains no
kernel, no initrd, no bootloader and no `init` (PID 1) contract. No kernel, initrd or init
binary is pinned anywhere today. Converting the verified rootfs content into the
`guest-root-disk` image and supplying a pinned `guest-kernel`/`guest-initrd` pair (for example
from a pinned `snapshot.ubuntu.com` snapshot verified with `gpgv`) is explicit follow-up work
in the guest image-build slice; this CURL.1 slice performed no downloads and built no image.

### 15.2 Frozen CURL.1 release contract and implemented verifier

The immutable product release contract is schema **`boole.curl-product-release.v1`**, signed in
the dedicated domain `boole-curl-product-release-v1` with an injected Ed25519 trust root
(weak-point keys rejected). The manifest is canonical JSON with unknown fields rejected, so
transport identity (`downloadUrl`) and Apple identity (`teamId`) can never enter the authority
surface. GitHub Releases stays transport, never trust.

One release pins exactly six artifact roles in fixed order — `host-cli`, `host-node`,
`host-wallet-agent`, `host-controller`, `guest-update-manifest`, `guest-update-signature` —
each with a unique plain file name, a non-zero exact byte length and a lowercase SHA-256
digest. The four host roles are bounded by the frozen **512 MiB total host-payload cap**; the
embedded guest update manifest and detached guest signature are bounded at 1 MiB and 4 KiB.
The manifest also pins `targetOs=macos`, `targetArch=arm64`, `minimumMacOs=14.0`,
`channel=stable`, `controllerProtocolVersion=1` and a 40-hex `sourceRevision`.

The guest is bound by exact bytes: `guestManifestSha256` must equal the `guest-update-manifest`
descriptor digest; the embedded guest manifest must itself be canonical
`boole.native-shadow.guest-update.v1` content targeting linux/aarch64 with the product's pinned
`guestReleaseSequence`/`guestReleaseVersion`; and the embedded guest signature envelope must
live in the guest-update signing domain and bind that same `guestManifestSha256`. The guest
signature is not cryptographically re-verified by the product verifier — the guest trust root
is injected separately at guest staging (MAC.2-B), and the product release pins the exact bytes
it ships.

Replay and rollback are fail-closed through a release floor (a pinned first-install minimum, or
an installed sequence plus the exact active-manifest digest that any successor must bind), and
the verified-byte boundary is TOCTOU-safe: artifact verification streams from an open file
descriptor and retains that handle, so replacing the file path after verification cannot swap
the verified bytes.

Closed-local evidence: 37 new focused contract tests went RED→GREEN in
`crates/boole-core/tests/curl_product_release.rs`, and the 23 existing guest staged-update
verifier tests stayed green after the shared low-level rules (canonical JSON envelope, safe
identifiers and file names, SHA-256 shape, previous-manifest field) moved into one internal
helper module with byte-identical messages. `cargo fmt` and both CI clippy variants are clean.
All verification used a non-production KAT key; **no production trust root, no production key
custody, no signed production release and no installer exist**.

### 15.3 Corrected execution cursor

```text
CURL.0  COMPLETE — curl-first CLI/service product contract corrected and frozen
CURL.1  CONTRACT/VERIFIER GREEN — release contract and guest boot format frozen; installer,
                                  real release artifact and production trust root absent
CURL.2  NOT STARTED — product installer v2 with signature/hash/version verification, no dev toolchain
CURL.3  NOT STARTED — clean macOS 14/M1 Team-ID-free virtualization-entitlement canary
MAC.2-B-CORE/KAT GREEN — reusable offline guest-update verifier
MAC.2-B production OPEN — real update trust root, first signed manifest and durable adoption open
MAC.3-CLI BLOCKED / NOT STARTED — CLI-managed hidden VM lifecycle follows CURL.1–CURL.3 evidence
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No public
mining, paid API benchmark, release build or upload, user installation or production activation
occurred in this slice.

## 16. CURL.2-CORE — verified local installer core (2026-08-25)

CURL.2-CORE status: **INSTALLER CORE GREEN — VERIFIED ATOMIC LOCAL ADOPTION WITH A DURABLE
REPLAY FLOOR; DOWNLOAD/TRANSPORT, REAL RELEASE ARTIFACT AND PRODUCTION TRUST ROOT ABSENT.**
This section records the first CURL.2 slice: the installer core in
`crates/boole-core/src/curl_product_install.rs`. It consumes the frozen CURL.1 verifier
(section 15) and adopts a locally staged release bundle; it performs no network I/O. Fetching
the bundle is a separate follow-up slice, and URLs/GitHub Releases remain transport, never
trust. The host-controller binary decision stays deferred to the real-release-build slice: the
installer adopts whatever bytes the signed manifest pins, so its logic is independent of which
binary fills each role.

### 16.1 Frozen install boundary and durable state contract

`install_curl_product_release` takes an install root, the raw manifest and detached-signature
bytes, the injected trust root, a pinned first-install minimum sequence and a local artifact
source directory. The whole bundle is verified through the CURL.1 chain — manifest
authentication, per-role artifact byte streaming, complete-set finish — before the install
root is mutated in any way; a tampered artifact, a missing source file, a forged signature or
a floor violation leaves the root byte-identical to its prior state.

The durable install state is schema **`boole.curl-product-install-state.v1`** stored as
`installed-release.json` at the install root: canonical JSON with unknown fields rejected,
pinning `releaseSequence`, `releaseVersion`, `manifestSha256` and `versionDirectory`. On the
next install this record is the sole floor source — the successor must advance the sequence
and bind the exact active manifest digest — and the pinned first-install minimum applies only
when no record exists. A corrupt, non-canonical, unknown-field, wrong-schema, zero-sequence or
malformed-digest record **fails closed with the on-disk evidence preserved**; it is never
silently replaced by the first-install floor.

Install layout and crash safety: verified bytes are copied from the exact file handles the
verifier streamed (TOCTOU carry-through — swapping the source directory after verification
cannot change what is adopted) into a transient `staging/` tree with per-file fsync, flipped
into a `versions/` entry (named by zero-padded sequence plus a manifest-digest prefix) with a
single directory rename, and the state record is replaced via a fsynced temp-file rename. The
durable state is therefore always the old release or the new one, never a mix. Earlier version
directories are retained as rollback material; leftover `staging/` content and orphan version
directories from an interrupted run are replaced, never trusted. Each version directory also
retains the exact `release-manifest.json`/`release-signature.json` bytes for offline re-audit.

### 16.2 Closed-local evidence

19 focused installer tests went RED→GREEN in `crates/boole-core/tests/curl_product_install.rs`:
first install end-to-end (canonical state bytes, exact adopted artifact bytes, retained
manifest/signature copies, no staging or temp residue), signed successor upgrade with the old
version retained, replay and wrong-predecessor rejection with untouched state, below-minimum
first install, tampered/missing artifact and forged signature with an untouched root, five
fail-closed corrupt-state cases, orphan-directory and leftover-staging replacement,
source-directory independence after adoption, state reload, and install-root creation. The 37
CURL.1 contract tests stayed green; `cargo fmt` and both CI clippy variants are clean. All
verification used a non-production KAT key; **no download, no release build or upload, no
production trust root and no installation of real binaries occurred**.

### 16.3 Corrected execution cursor

```text
CURL.0  COMPLETE — curl-first CLI/service product contract corrected and frozen
CURL.1  CONTRACT/VERIFIER GREEN — release contract and guest boot format frozen
CURL.2-CORE  INSTALLER CORE GREEN — verified atomic local adoption with a durable replay floor
CURL.2-TRANSPORT  NOT STARTED — bundle download/staging and the curl entrypoint that drives the core
CURL.3  NOT STARTED — clean macOS 14/M1 Team-ID-free virtualization-entitlement canary
MAC.2-B-CORE/KAT GREEN — reusable offline guest-update verifier
MAC.2-B production OPEN — real update trust root, first signed manifest and durable adoption open
MAC.3-CLI BLOCKED / NOT STARTED — CLI-managed hidden VM lifecycle follows CURL.1–CURL.3 evidence
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No public
mining, paid API benchmark, release build or upload, user installation or production activation
occurred in this slice either.

## 17. CURL.2-TRANSPORT — fail-closed bundle download and the curl entrypoint (2026-08-25)

CURL.2-TRANSPORT status: **GREEN — FAIL-CLOSED BUNDLE DOWNLOAD/STAGING AND THE
`boole product install` CURL ENTRYPOINT; REAL RELEASE ARTIFACT AND PRODUCTION TRUST ROOT
ABSENT.** This section records the second CURL.2 slice: the transport in
`crates/boole-cli/src/curl_product_transport.rs` and the `boole product install` CLI command
that drives it. The transport fetches a release bundle over HTTP(S) and hands it to the
CURL.2-CORE installer (section 16). Transport is never trust: the URL, the HTTP status code,
server headers and file names carry no authority, and every downloaded byte is verified by the
frozen CURL.1 chain (section 15) before adoption. The transport reuses the workspace-pinned
`reqwest` (blocking + rustls-tls) already linked by `boole-miner`/`boole-mcp`, so the
supply-chain surface gains no new crate. `boole-core` remains network-free.

### 17.1 Frozen fail-closed download order

`download_and_install_curl_product_release` executes exactly this order and aborts at the
first failure:

1. validate the base URL shape (http/https only) and reject a download staging directory that
   overlaps the install root in either direction;
2. read the durable install state — a corrupt `installed-release.json` aborts **before any
   network request is made**, with the on-disk evidence preserved;
3. download the manifest and detached signature into memory only, each stream hard-capped by
   the frozen CURL.1 contract limits (1 MiB / 4 KiB) regardless of server-declared lengths;
4. authenticate them against the injected trust root and the replay floor — a forged or
   replayed bundle aborts **before any artifact request**;
5. download exactly the artifacts the signed manifest declares, in the fixed role order, each
   stream bounded by its signed `byteLength` (a longer stream is aborted mid-transfer; a
   shorter one is rejected), into a transient download staging directory that is never the
   install tree, with leftover residue from an interrupted run removed first;
6. run the CURL.2-CORE installer, which re-verifies the full release end to end and adopts it
   atomically behind the durable replay floor; and
7. remove the download staging directory whether the install succeeded or failed — downloaded
   bytes never persist anywhere except the verified, atomically adopted install tree.

The declared per-artifact bound comes from a new accessor on the authenticated (pre-artifact)
CURL.1 stage, `artifact_byte_length`, so the transport caps every artifact stream from the
signed manifest instead of trusting `Content-Length`. The previously private manifest and
signature caps are now the public contract constants
`MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES`/`MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES`.

`boole product install` is the curl entrypoint over this transport: it takes the base URL,
install root, optional download staging override, the injected trust-root key id and public
key, the pinned first-install minimum sequence and a request timeout, and always emits the
unified JSON envelope (`command: "product.install"`) — success on stdout with the adopted
release identity, rejection on stderr with a typed reason token (`url-rejected`,
`download-failed`, `release-rejected`, `install-rejected`, `staging-io-failed`) and a nonzero
exit. No default trust root ships in the binary; a trust root must be injected explicitly.

### 17.2 Closed-local evidence

18 focused tests went RED→GREEN, all against a loopback HTTP server started inside the test
process and a non-production KAT key: 15 transport tests in
`crates/boole-cli/tests/curl_product_transport.rs` (happy-path first install and signed
successor with the exact 8-request order pinned; corrupt state aborting with zero network
requests; forged signature and replayed bundle aborting after exactly the two metadata
requests; a tampered artifact behind HTTP 200 rejected by digest; over-long and truncated
artifact streams rejected against the signed byte length; missing artifact and manifest error
statuses as transport failures; the manifest transport cap; an unreachable server; staging
residue replaced, never trusted; non-http(s) URLs rejected without any request; and a staging
directory inside the install root rejected), 2 CLI entrypoint tests in
`crates/boole-cli/tests/product_install_cli.rs` (envelope success and typed
`release-rejected` failure with nonzero exit), and 1 byte-length accessor test extending the
CURL.1 suite to 38. The CLI envelope inventory gained `product install` under the drift
guard. `cargo fmt` and both CI clippy variants are clean. **No public network interaction, no
download of any real release, no release build or upload, no production trust root and no
installation of real binaries occurred.**

### 17.3 Corrected execution cursor

```text
CURL.0  COMPLETE — curl-first CLI/service product contract corrected and frozen
CURL.1  CONTRACT/VERIFIER GREEN — release contract and guest boot format frozen
CURL.2-CORE  INSTALLER CORE GREEN — verified atomic local adoption with a durable replay floor
CURL.2-TRANSPORT  GREEN — fail-closed bundle download/staging and the curl entrypoint
CURL.3  NOT STARTED — clean macOS 14/M1 Team-ID-free virtualization-entitlement canary
MAC.2-B-CORE/KAT GREEN — reusable offline guest-update verifier
MAC.2-B production OPEN — real update trust root, first signed manifest and durable adoption open
MAC.3-CLI BLOCKED / NOT STARTED — CLI-managed hidden VM lifecycle follows CURL.1–CURL.3 evidence
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No public
mining, paid API benchmark, release build or upload, user installation or production activation
occurred in this slice either.

## 18. CURL.3-PREP — clean-Mac Team-ID-free entitlement canary contract frozen (2026-08-25)

CURL.3-PREP status: **CONTRACT FROZEN AND MACHINE-CHECKED; CANARY NOT RUN — NO CLEAN
SUPPORTED MAC WAS AVAILABLE.** This section freezes what a CURL.3 answer must look like and
records the closed-local implementation of its evaluator. No clean-Mac canary ran, no guest
booted, no Apple identity, certificate or provisioning profile was created or used, and no
release artifact was built, downloaded or installed.

### 18.1 Frozen canary acceptance contract

The CURL.3 question is narrow: does a host controller signed **without an Apple Team ID**,
carrying only `com.apple.security.virtualization` in an ad-hoc signature, drive a contained
Linux guest through a full lifecycle on a **clean** supported Mac? A canary report is accepted
only if every ground below holds, evaluated in this frozen fail-closed order so that an
earlier ground never depends on a later one:

1. **macOS floor** — below the frozen product minimum of macOS 14.0 the report aborts before
   anything else is read. The canary floor is pinned to the same frozen constant as the
   product minimum, so the two can never drift apart.
2. **Architecture** — Apple Silicon (M1 and later) only; Intel stays outside the v1 range.
3. **Clean-machine grounds** — cleanliness is never inferred from a successful run. Four
   grounds must be separately established: `erase-install`, `developer-toolchain-absent`,
   `boole-source-tree-absent` and `prior-boole-install-absent`. Every unestablished ground is
   named in the rejection. **A successful run on a developer machine can never be recorded as
   a clean-Mac pass** — the machine check precedes every success signal, so a flawless run on
   a dirty host is rejected outright rather than counted.
4. **Signing form** — the Team-ID-free ad-hoc signature is the subject of the experiment. A
   Team-ID signature is rejected because it answers a different question, and an unsigned
   binary is rejected because it carries no entitlement at all.
5. **Entitlement** — `com.apple.security.virtualization` must be present in that signature.
6. **Execution mode** — only an entitled, isolated virtual machine counts. An unentitled
   fallback or a workload run directly on the host is a rejection, never a degraded pass.
7. **Boot loader** — section 15.1 froze direct kernel boot with `VZLinuxBootLoader`, so the
   `VZEFIBootLoader` path is rejected here too.
8. **Fixed boot inputs** — exactly the three frozen roles `guest-kernel`, `guest-initrd` and
   `guest-root-disk`, in that order, each non-empty and pinned by a lowercase hex SHA-256
   digest, and **the reboot must reuse byte-identical pins**; a canary that re-fetches
   different inputs proves nothing about a fixed guest.
9. **Minimal lifecycle** — exactly the boot, shutdown and reboot boundaries, in that order,
   each completed. A failed boundary names the failure.
10. **Residue** — every boundary must leave no file and no process behind. One leftover path
    or one surviving process rejects the whole report.

### 18.2 Closed-local evidence

`crates/boole-core/src/curl_virtualization_canary.rs` implements this contract as a pure
evaluator: it signs nothing, downloads nothing, boots nothing and probes no host, so it is
deterministic on every platform. 23 closed-local tests pin the contract
(`crates/boole-core/tests/curl_virtualization_canary.rs` plus two in-module tests), including
the drift pin between the canary floor and the frozen product minimum, the developer-machine
rejection with every success signal intact, the Team-ID and unsigned rejections, the missing
entitlement, the unentitled and non-isolated execution rejections, the EFI rejection, the
exact boot-input role set and reboot-identity requirement, the exact lifecycle sequence and
the residue rejection at each boundary.

`scripts/curl-virtualization-entitlement-probe.sh` is the operator's first step on a clean
Mac. It signs one probe binary twice — once with the entitlement, once without — and requires
the entitled binary to reach `VZVirtualMachine` instantiation while the unentitled binary is
refused. It uses no Apple identity, boots no guest and removes its temporary tree on every
exit path. Passing it is explicitly **not** a CURL.3 pass: it covers the entitlement ground
only, on whatever machine ran it.

**Developer-machine observation, recorded as non-qualifying.** On the development Mac
(macOS 26.5.2, build 25F84, Apple M4 Max — not macOS 14, not a clean machine) that probe
passed: the ad-hoc signature reported `Signature=adhoc` with `TeamIdentifier=not set`, the
entitled binary reached `instantiate=ok`, and the unentitled binary was refused with
`VZErrorDomain Code=2 "The process doesn't have the com.apple.security.virtualization
entitlement."` This is early evidence that the Team-ID-free form is not rejected outright and
that the unentitled path fails closed. **It is not a CURL.3 pass**: the host is a developer
machine, it is not the frozen floor version, no guest booted, and no lifecycle or residue
boundary was exercised.

### 18.3 STOP condition and corrected execution cursor

No clean supported Mac was available, so the canary was not run and CURL.3 remains **NOT
STARTED**. Per section 14 this gate must be measured on a clean supported Mac and must never
be inferred from a developer machine; that rule is now enforced in code rather than by
convention. Running CURL.3 additionally requires the host-controller binary and the pinned
`guest-kernel`/`guest-initrd`/`guest-root-disk` set, none of which exist yet.

```text
CURL.0  COMPLETE — curl-first CLI/service product contract corrected and frozen
CURL.1  CONTRACT/VERIFIER GREEN — release contract and guest boot format frozen
CURL.2-CORE  INSTALLER CORE GREEN — verified atomic local adoption with a durable replay floor
CURL.2-TRANSPORT  GREEN — fail-closed bundle download/staging and the curl entrypoint
CURL.3-PREP  CONTRACT FROZEN — canary acceptance grounds machine-checked; canary NOT RUN
CURL.3  NOT STARTED — no clean macOS 14/M1 host, no host controller, no pinned boot inputs
MAC.2-B-CORE/KAT GREEN — reusable offline guest-update verifier
MAC.2-B production OPEN — real update trust root, first signed manifest and durable adoption open
MAC.3-CLI BLOCKED / NOT STARTED — CLI-managed hidden VM lifecycle follows CURL.1–CURL.3 evidence
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No public
mining, paid API benchmark, release build or upload, user installation, production key or
production activation occurred in this slice either.

## 19. CURL.3 environment deferral and bootable guest contract v2 (2026-08-26)

Status: **CURL.3 DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED; BOOTABLE GUEST-UPDATE V2
AND PRODUCT-RELEASE V2 CONTRACT GREEN; REAL BOOT ARTIFACTS, V2 INSTALL CONSUMER AND VM
LIFECYCLE ABSENT.** This is an execution-order correction, not a waiver and not a successful
canary. A clean supported Mac cannot be supplied in the current development environment, so
repeating CURL.3 preparation cannot create the missing evidence.

### 19.1 Circular prerequisite corrected

The section 18 cursor made CURL.3 a prerequisite for MAC.3 implementation even though the
CURL.3 lifecycle itself needs a real host controller and pinned boot inputs produced by that
implementation. That is a circular dependency. The corrected order separates development
permission from product qualification:

1. CURL.3 is removed only from the prerequisites for **closed-local implementation** of the
   host controller, boot artifact builder and hidden-VM lifecycle.
2. Development-machine runs are always labelled non-qualifying and can never become a CURL.3
   pass, a clean-install pass or a release claim.
3. CURL.3 remains mandatory before MAC.5 clean-install acceptance, MAC.6 release readiness,
   any Mac production-support claim, distribution of a production trust root or activation.
4. If a suitable clean supported Mac becomes available, the frozen section 18 evaluator is
   run unchanged. Until then the state is `DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`.

This correction allows useful implementation work to continue without converting unavailable
hardware into invented evidence.

### 19.2 Bootable successor contract

The earlier update and boot contracts described different file sets. The frozen guest-update
v1 authenticates exactly ten roles and carries one OCI-style `guest-rootfs`; CURL.1 direct
Linux boot requires three host-visible files: `guest-kernel`, `guest-initrd` and
`guest-root-disk`. A product could therefore pass the old release verifier while still lacking
the files its boot loader needs.

The successor closes that contract gap without reinterpreting v1:

- guest-update v1 remains byte/meaning compatible and exact-ten;
- guest-update v2 uses a separate schema and signing domain, requires
  `bootFormatVersion=1`, replaces `guest-rootfs` with the three shared boot roles and therefore
  authenticates exactly twelve artifacts in a fixed order;
- the canary and update verifier use one shared boot-role vocabulary;
- product-release v2 has its own schema/signing domain and accepts only an embedded guest v2
  manifest with the exact bootable role set; product-release v1 cannot be silently promoted;
- all twelve guest artifacts count toward the unchanged 2 GiB guest download cap.

This slice establishes contracts only. The current CURL.2 installer and transport still consume
the frozen product-release v1 entrypoint; no v2 bundle has been installed or downloaded. No
uncompressed Linux/arm64 kernel `Image`, initrd, bootable read-only root disk or minimal PID 1
has been produced, and the available ARM64 source closure is incomplete. A real artifact builder
must pin those inputs and generation tools before any VM boot attempt.

### 19.3 Corrected execution cursor

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
BOOT-CONTRACT-V2  GREEN — exact-12 bootable guest + product successor trust boundary
BOOT-ARTIFACT-BUILDER  NEXT — deterministic kernel/initrd/root-disk production
MAC.3-CLOSED-LOCAL  UNBLOCKED / NOT STARTED — controller and lifecycle development allowed
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No clean-Mac
canary, VM boot, real release build, production key, public mining, paid API benchmark, user
installation or activation occurred.

## 20. BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 (2026-08-26)

Status: **BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1 = GREEN; REAL-BOOT-ARTIFACTS =
NOT-PRODUCED.** This is an audit-only prerequisite, not an artifact builder and not a boot
claim. The command exposes only `audit`; it has no build command, no output directory and no
path that writes a kernel, initrd or root disk.

### 20.1 Frozen preflight authority

`native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json` binds the
existing Linux/arm64 source lock byte-for-byte: SHA-256
`829ca81d321d412746cce7a62d59d7e538c394b92c1b6a9a966f3016b73cede0`, exactly 62 source
artifacts and exactly 181,623,999 source bytes. It is explicitly
`NATIVE-SHADOW-BOOT-ARTIFACT-BUILD-PLAN-ARM64-V1-SCAFFOLD-NOT-ACTIVATABLE`, keeps
`activationAllowed=false`, retains
the 2 GiB guest cap and binds the already-frozen ARM64 execution policy byte-for-byte (SHA-256
`df8be9eb7f3d92335d22b95a7e9423d8baaa2d581a2fd3b3633f60ae63db4e3f`). That policy requires
Linux/aarch64, systemd, the `boole-native-shadow-launcher.service` unit and its fixed cgroup path.
The real kernel, systemd guest closure and initrd/ext4 image-builder toolchain authorities remain
explicitly unresolved. This v1 file/schema/tool is permanently audit-only: its three authority
fields must stay null forever. Future populated pins require a separately reviewed successor
plan/schema/tool; placeholder bytes cannot become release authority.

The preflight implementation in
`scripts/native_shadow_boot_artifact_builder_arm64_v1.py` enforces:

- canonical JSON with duplicate-key rejection and exact schema/key sets;
- the source-lock digest, architecture, artifact count, byte total and every artifact row;
- the execution-policy digest, Linux/aarch64 platform, systemd requirement, fixed service unit and
  fixed cgroup parent;
- symlink-free traversal of every path component and file-descriptor-based size/digest checks;
- unresolved kernel, systemd guest-closure and image-builder authorities cannot be populated in
  this scaffold; and
- no network path, no generated artifact, an unconditional `BLOCKED_MISSING_INPUTS` status and an
  unconditional `bootableClaim=false`.

Even a complete source cache cannot make this scaffold reach a ready/build state. A future build
state belongs only to a successor plan/schema/tool after all three authority contracts exist.
Thirteen focused preflight tests and the self-test registration contract are GREEN.
They include policy drift, premature authority population, parent-symlink, digest/size,
file-descriptor cleanup and network-attempt regressions.

### 20.2 Current local observation

A fresh closed-local audit found all 62 frozen rootfs source artifacts (181,623,999 bytes) in
the already-existing local content-addressed cache. This is a local availability observation,
not a tracked release input and not a promise that another machine has the same cache. The real
kernel authority, systemd guest-closure authority and image-builder-toolchain authority remain
absent, so the result is still `BLOCKED_MISSING_INPUTS`; `artifactsWritten=0` and
`bootableClaim=false`.

No package was downloaded, no source closure was changed, no kernel/initrd/root disk was made,
no VM booted and no v2 bundle was installed. Before bytes can be selected, the next slice must
freeze a self-contained systemd guest-closure compatibility contract (init system, service unit,
runtime packages and the fixed cgroup layout). Only after that may `BOOT-INPUT-AUTHORITY-V1`
freeze exact kernel, guest-closure and image-builder bytes. It may not substitute an OCI rootfs,
a static one-off PID 1 or an unpinned host tool.

### 20.3 Execution cursor

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
BOOT-CONTRACT-V2  GREEN — exact-12 bootable guest + product successor trust boundary
BOOT-ARTIFACT-BUILDER-PREFLIGHT-V1  GREEN — audit-only; zero outputs and no boot claim
BOOT-GUEST-INIT-COMPATIBILITY-V1  NEXT — freeze systemd/unit/runtime/cgroup closure contract
BOOT-INPUT-AUTHORITY-V1  BLOCKED — exact bytes wait for the compatibility contract
REAL-BOOT-ARTIFACTS  NOT-PRODUCED — builder, boot and v2 install remain absent
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. This slice
contains no public mining, paid API benchmark, production key, release upload or activation.

## 21. BOOT-GUEST-INIT-COMPATIBILITY-V1 (2026-08-26)

Status: **CONTRACT GREEN; CURRENT ARM64 SOURCE LOCK =
BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS; REAL BOOT ARTIFACTS NOT PRODUCED.** This slice freezes what a
self-contained Linux/arm64 systemd guest must contain. It does not select package bytes, build a
kernel or image, start a VM, or turn the current checker OCI rootfs into a boot claim.

### 21.1 Frozen compatibility boundary

`native/containment/native-shadow-guest-init-compatibility-arm64-v1.json` (SHA-256
`ee67e23bc89fa456a6e43a1c93d9d4a0faa8cf09e5d04b9a9678c17499d267b0`) binds the current
baseline source lock/closure, the ARM64 execution policy, the audit-only boot plan and the exact
launcher unit, sysusers and tmpfiles bytes. Those baseline hashes are comparison evidence, not a
rule that a future source lock must reuse the incomplete bytes. A separately pinned successor
source lock is checked against the same compatibility requirements before the later input-authority
slice may adopt it.

The contract requires real systemd at `/usr/lib/systemd/systemd`, a full-shape `systemd` package
row selected as a dependency-closure seed, the exact launcher service enabled under
`multi-user.target`, the exact
sysusers/tmpfiles inputs, an exact ARM64 launcher binary role and an empty root-disk
`/etc/machine-id` whose runtime identity is ephemeral. The root disk stays read-only. `/dev`,
`/proc`, `/run`, `/sys/fs/cgroup`, `/tmp` and `/var/lib/boole` are explicit boot-lifetime mounts;
`/sys` is read-only apart from the separately mounted writable cgroup v2 hierarchy. Required
kernel facilities include the `cpu`/`memory`/`pids` controllers, cgroup freeze/kill,
`clone3` with cgroup placement, PID/mount namespaces, pidfd, seccomp and Landlock ABI 3 or later.

The Mac host node remains the durable journal owner. Explicit replay-node binary, service and
enablement paths are rejected, and the guest contract forbids node, wallet, reward or consensus
authority. This source-shape audit alone does not prove the absence of every renamed or package-
embedded authority; that remains a separate input-authority and runtime gate. The authenticated bounded
host/guest transport remains a MAC.3 concern; this contract cannot silently solve that later
boundary by placing a second node in the guest. A static one-off PID 1, host `/bin/true` service
stub, host systemd syntax gate or current OCI rootfs is not guest-boot evidence.

### 21.2 Current baseline result

The read-only auditor `scripts/native_shadow_guest_init_compatibility_arm64_v1.py` reports exactly
seven missing roles in the current 62-artifact source lock:

- signed `systemd` package seed/closure;
- tracked ARM64 launcher binary;
- tracked launcher unit;
- tracked sysusers configuration;
- tracked tmpfiles configuration;
- tracked empty `/etc/machine-id`; and
- the launcher unit enablement symlink under `multi-user.target.wants`.

Therefore the current result is `BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS`, with
`artifactsWritten=0`, `bootableClaim=false` and `activationAllowed=false`. A synthetic successor
shape can reach only `SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED`. That label explicitly leaves
`signedClosureVerified=false`, `runtimeCompatibilityVerified=false` and
`authorityBoundaryVerified=false`; it is neither a signed-closure result nor a boot claim and does
not make its bytes authoritative. Seventeen focused tests are GREEN, including malformed package,
unsafe ownership/source path, extra guest-node service/binary, exact authority drift, current
missing-role accounting, read-only/writable declaration, policy/unit delegation and direct CLI
behavior. The later input-authority slice must independently validate archive signatures, the full
dependency closure and artifacts, while actual boot must verify mounts and kernel facilities.

### 21.3 Execution cursor

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
BOOT-GUEST-INIT-COMPATIBILITY-V1  CONTRACT GREEN — baseline missing seven requirements
BOOT-INPUT-AUTHORITY-V1  NEXT — pin a successor systemd package closure, launcher and build inputs
REAL-BOOT-ARTIFACTS  NOT-PRODUCED — kernel/initrd/root disk, VM boot and v2 install absent
MAC.3 HOST/GUEST TRANSPORT  NOT STARTED — guest receives no node authority
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. No network,
download, package installation, image build, VM boot, public mining, paid API benchmark,
production key, release upload or activation occurred.

## 22. BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1 (2026-08-26)

Status: **DEPENDENCY-CANDIDATE-FROZEN-NOT-BOOT-AUTHORITY.** This slice closes only deterministic
package selection from already-cached, signed Ubuntu repository metadata. It does not download or
verify the selected package payloads and does not create an executable guest.

### 22.1 Pinned candidate and conservation

The canonical plan
`native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json`
(SHA-256 `f5465cd62b8b96f2e5b1702e72d2be2b2d73d3924968974604697a4a1614681f`)
binds the exact source generator, resolver, ARM64 acquirer/builder wrappers, baseline authority
files, snapshot identity, Ubuntu archive fingerprint, `gpgv`/`zstd` executables and selection
seeds. The generated result
`native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json`
(SHA-256 `a8329d35b480e6b40823e8823551c16eb71a0d7bbc1da40483f05b4535815f26`)
replays the signed `20240425T160000Z` Ubuntu Noble ARM64 metadata and conserves:

- existing baseline: 56 packages / 66,992,762 declared payload bytes;
- dependency candidate: 191 packages / 208,936,876 declared payload bytes; and
- successor delta: 135 packages / 141,944,114 declared payload bytes.

The baseline 56 rows are an exact-row subset of the 191 rows. The result fixes five required
package identities, including systemd and the selected Linux image/modules package rows. These
figures are signed-index declarations, not downloaded-byte evidence and not installed-size or
guest-image measurements.

The generator never imports repository authority modules from Python's bytecode cache. It reads
the pinned source files without following links, compiles those exact bytes into isolated module
objects and calls only those objects. Tests pin poisoned import-cache rejection, generator/source
digest binding, exact baseline inclusion, count/byte conservation, canonical output and output
symlink rejection. A second closed-local generation was byte-identical.

### 22.2 Explicitly absent authority

The result keeps every boundary below false or zero:

- package payload acquisition and package payload verification;
- maintainer-script execution;
- uncompressed Linux/arm64 kernel `Image` extraction;
- ARM64 launcher ELF;
- image-builder authority and runtime-compatibility verification;
- initrd, read-only root disk and VM boot;
- production byte provenance, boot authority, bootable claim and activation authority; and
- generated boot artifacts (`bootArtifactsWritten=0`).

No package, tool or key was downloaded or installed. The tracked result came only from the
already-existing content-addressed metadata cache. The next networked step, if separately
approved, is exact acquisition and digest verification of the fixed 135-package delta (with the
full 191-row closure rechecked); it may not execute maintainer scripts or claim a guest image.

### 22.3 Execution cursor

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — MAC.5/MAC.6 전 필수
BOOT-GUEST-INIT-COMPATIBILITY-V1  CONTRACT GREEN
BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1  FROZEN-NOT-BOOT-AUTHORITY
BOOT-INPUT-AUTHORITY-V1  PARTIAL — signed-metadata dependency selection only
BOOT-PAYLOAD-ACQUISITION/VERIFICATION  NEXT — separate network approval required
LAUNCHER-ELF / IMAGE-BUILDER / KERNEL-EXTRACTION  OPEN
REAL-BOOT-ARTIFACTS  NOT-PRODUCED
MAC.3 HOST/GUEST TRANSPORT  NOT STARTED
MAC.5 / MAC.6  BLOCKED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. CURL.3 stays
a mandatory release qualification gate; this deferral is neither a pass nor a waiver.

## 23. BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1 (2026-08-26)

Status: **PACKAGE-PAYLOADS-ACQUIRED-VERIFIED-NOT-BOOT-AUTHORITY.** This slice acquires and
digest-checks only the package bytes selected in section 22. It does not extract or execute a
package, create a successor source lock, build an image, boot a VM or grant activation authority.

### 23.1 Pre-registered order and measured result

The network contract was fixed before the first request in
`native/containment/native-shadow-boot-rootfs-payload-acquisition-plan-arm64-v1.json`
(SHA-256 `f6589fe619e83531d9e76c998dbd5ab33436595e307579ccfecd2de644069fd1`). It pins the
candidate plan/result, baseline resolution, acquisition/generator tools, exact Ubuntu snapshot,
`gpgv`/`zstd` binaries, initial CAS inventory and these ordered gates:

1. fetch exactly the missing ARM64 `Packages.xz`, then replay the signed repository metadata;
2. require the replayed 191-row candidate to be byte-identical to the tracked candidate result;
3. reuse five exact baseline blobs and fetch the missing 51 sequentially;
4. verify all 56 baseline rows by file descriptor, SHA-256 and size before opening any delta URL;
5. reuse one exact delta blob, fetch the missing 134 sequentially, then verify all 191 rows; and
6. never request the three ARM64 Rust-distribution artifacts in this slice.

The tracked result
`native/containment/native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json`
(SHA-256 `60408c39ac48f3b7ef272e050349dee84ee28693d6c33e528c77898927f4b3df`)
records exactly 186 GETs: one 1,376,632-byte index, 51 baseline payloads / 66,498,278 bytes and
134 delta payloads / 141,932,990 bytes. Total network payload was 209,807,900 bytes. Six exact CAS
hits were verified and reused without a request. Independent post-run verification re-read all
191 package files and confirmed SHA-256, size, regular-file mode and single-link publication.

The historical section 22 forecast described only the fixed 135-row delta. The execution plan
correctly accounted for the actual persistent CAS inventory discovered before the network run:
51 of the 56 baseline rows were also absent. They were not deleted by the earlier `target/`
cleanup; this was the first persistent ARM64 payload acquisition. The baseline was therefore
recovered and fully verified before the delta, rather than silently assuming it existed.

### 23.2 Network and CAS boundary

Every request was an exact HTTPS GET to `snapshot.ubuntu.com:443` under snapshot
`20240425T160000Z`, TLS 1.2 or newer, with environment proxies, redirects, retries, Range and
parallel downloads disabled. Each response required status 200, identity encoding and exact
`Content-Length`; bytes streamed into a mode-0600 temporary file, were checked before publication,
file-synced, hard-linked without replacement and directory-synced. Exact CAS hits caused zero
network calls. A corrupt, symlinked, differently owned or insecurely permissioned existing entry
is a hard stop, never a reason to overwrite it.

The first CLI invocation supplied a relative result path and stopped before any request because
the frozen writer accepts only a normalized absolute output path. The successful invocation used
the same pre-registered plan with an absolute output path; it made the only 186 network requests.

### 23.3 Explicitly absent authority and cursor

`packagePayloadsAcquired=true` and `packagePayloadsVerified=true` are the only promoted boundary
facts. `maintainerScriptsExecuted=false`, `kernelImageExtracted=false`,
`launcherElfPresent=false`, `imageBuilderAuthorityPresent=false`,
`runtimeCompatibilityVerified=false`, `productionByteProvenanceComplete=false`,
`bootAuthority=false`, `bootArtifactsWritten=0`, `bootableClaim=false` and
`activationAllowed=false` remain fixed.

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — MAC.5/MAC.6 전 필수
BOOT-ROOTFS-DEPENDENCY-CANDIDATE-ARM64-V1  FROZEN-NOT-BOOT-AUTHORITY
BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1  COMPLETE — 191/191 exact payload bytes
BOOT-INPUT-AUTHORITY/SOURCE-LOCK SUCCESSOR  NEXT — package rows를 source closure에 봉인
ARM64 RUST DIST / LAUNCHER ELF / IMAGE BUILDER / KERNEL EXTRACTION  OPEN
REAL-BOOT-ARTIFACTS  NOT-PRODUCED
MAC.3 HOST/GUEST TRANSPORT  NOT STARTED
MAC.5 / MAC.6  BLOCKED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged. CURL.3 is
still mandatory before release qualification; this deferral is neither a pass nor a waiver.

## 24. BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1 (2026-08-26)

Status: **SEALED — SOURCE SHAPE ONLY, LAUNCHER BINARY DEFERRED, NOT BOOT AUTHORITY.** The
predecessor lock described a 56-package checker toolchain closure with no init system, no
launcher deployment bytes and no kernel seed, so it could never describe a bootable guest. This
slice seals a successor lock over the 191 verified package rows plus the guest-init deployment
bytes. It builds nothing, boots nothing and activates nothing.

### 24.1 Acceptance grounds frozen before any result

`native/containment/native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json`
(SHA-256 `c047c20144167a4f28f222c4026a33e2d70b89340ee13cba79c207b7c92dc583`, 14,099 bytes) was
written before the successor lock existed. It binds the Ubuntu snapshot `20240425T160000Z` and
its fixed repository paths, the package selection policy and canonical ordering, the systemd
package seed, the launcher binary's guest placement, the launcher unit, the sysusers and tmpfiles
configuration, the empty machine-id rule, the unit enablement symlink, the read-only root against
the ephemeral runtime mounts, cgroup delegation through the existing launcher policy, the
generator/verifier/builder source digests, `maintainerScriptsExecuted=false` and
`activationAllowed=false`. Every one of the guest-init contract's seven requirements is listed by
role together with the ground that closes it.

The plan pins the generator by digest and the generator pins the plan by digest. A literal digest
cannot be part of its own preimage, so the generator is hashed with its embedded plan digest
replaced by 64 zeros — the same cycle-breaking rule the payload acquirer already uses. A naive
`sha256` of the generator therefore differs from the pinned value by design, not by tampering.

Two deployment files the frozen contract had pinned by digest since 2026-08-23 were confirmed to
exist at `native/sysusers.d/boole-native-shadow.conf` and
`native/tmpfiles.d/boole-native-shadow.conf` with exactly the pinned bytes. Only the empty
machine-id source was genuinely absent; it was created as a zero-byte file whose digest is the
SHA-256 of the empty string the contract already required.

### 24.2 Sealed successor lock

`native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json`
(SHA-256 `9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf`, 357,104 bytes) carries
197 artifacts, 191 package rows totalling 208,936,876 payload bytes, 7 seed packages including
`systemd`, 10 tracked files, 8 derived entries and 10 authority bindings. Every package row is
byte-identical to the verified dependency candidate closure, every payload digest and size matches
the artifact row, and every package appears in the payload acquisition result, so no row can be
borrowed from a different source lock or a different snapshot.

Thirty-four acceptance tests fail first and pass only against the sealed documents. They refuse a
dropped package, an extra package, reordered packages or artifacts, a tampered size, a tampered
digest, a mixed snapshot, a mixed repository base, a missing systemd seed, a renamed systemd
package, a missing launcher unit, a missing sysusers or tmpfiles configuration, a missing or
non-empty machine-id, a missing or misdirected enablement symlink, a replay-node service tracked
or enabled inside the guest, permitted maintainer-script execution, permitted build-time network,
`activationAllowed=true`, a package borrowed from the predecessor lock, the predecessor lock
offered as its own successor, an invented launcher binary digest, tracked source bytes that do not
match the pinned digest, and an authority binding whose digest does not match the file on disk.

### 24.3 The one requirement that stays open, and why

Six of the guest-init contract's seven requirements are closed. The seventh —
`tracked-file:launcher-binary` — is recorded as deferred rather than closed:

```text
role:       tracked-file:launcher-binary
guestPath:  /usr/libexec/boole/boole-native-shadow-launcher
cause:      the guest launcher ELF is a build output of the ARM64 launcher build authority,
            which has not run; a digest cannot be stated for a file that does not exist
resolvedBy: arm64-launcher-build-authority
```

The contract itself already refuses a launcher binary digest at this stage, and a tracked file row
requires a real digest bound to real bytes. The honest outcome is therefore
`BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS` with exactly one missing role, not
`SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED`. Reaching that ceiling verdict later would still
mean source shape only: it would say the declared inputs are present and internally consistent,
and it would say nothing about runtime compatibility, boot authority or boot success.

### 24.4 Execution cursor

`launcherElfPresent=false`, `imageBuilderAuthorityPresent=false`, `kernelImageExtracted=false`,
`maintainerScriptsExecuted=false`, `runtimeCompatibilityVerified=false`, `guestBootVerified=false`,
`bootAuthority=false`, `bootArtifactsWritten=0`, `bootableClaim=false` and
`activationAllowed=false` are all unchanged by this slice.

```text
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — MAC.5/MAC.6 전 필수
BOOT-ROOTFS-PAYLOAD-ACQUISITION-ARM64-V1  COMPLETE — 191/191 exact payload bytes
BOOT-ROOTFS-SOURCE-LOCK-ARM64-V1  SEALED — 191 verified package rows + guest-init deployment bytes
LAUNCHER BINARY DIGEST  DEFERRED — ARM64 launcher build authority 미실행
ARM64 RUST/LAUNCHER/IMAGE-BUILDER INPUT AUTHORITY  NEXT
REAL-BOOT-ARTIFACTS  NOT-PRODUCED
MAC.3 HOST/GUEST TRANSPORT  NOT STARTED
MAC.5 / MAC.6  BLOCKED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain unchanged.

## 25. The root disk was produced twice and the two copies differed

### 25.1 What happened

Two arm64 Linux CI jobs built the guest root disk from byte-identical inputs and produced two
different images. The kernel and the initrd matched. The disk did not:
`b3299ed161557a195a9f58bb899fc61979ecb7456de94b9d89ac951c2e320b96` against
`11099b116c3241a4441ba6ed3cfd4b7db6a160779c97271bcce5bf6e99b7153a`, both 1,168,314,368 bytes.

The pre-registered response to that outcome was to stop rather than to relax the criterion, and the
stop was taken. It is sealed in
`native/containment/native-shadow-boot-root-disk-determinism-hard-stop-arm64-v1.json` with status
`ROOT-DISK-BYTE-IDENTITY-FAILED-CAUSE-CONFIRMED-STOP-HELD-NOT-BOOT-AUTHORITY`. That record is
append-only. It is not revised, softened or reread as a success by anything that follows it.

### 25.2 What differed, measured rather than asserted

847 of 285,233 blocks differed — 0.297 per cent — and every one of them was superblock, backup
superblock or inode table. Every data block, every directory block, every bitmap and all nine group
descriptors were identical.

At byte granularity, 80,748 bytes differed and **zero were unexplained**. Each one is a timestamp
field or a checksum computed over one: `s_wtime`, `s_lastcheck`, `s_mkfs_time`, `s_checksum`,
`i_atime`, `i_ctime`, `i_mtime`, `i_crtime`, `i_checksum_lo`, `i_checksum_hi`.

A read-only walk of both images compared 13,448 entries — 1,445 directories, 653 symlinks, 11,350
files, 1,008,783,262 bytes of file content hashed. Path, walk order, inode number, kind, mode, uid,
gid, size, link count, flags, content digest, symlink target, extended attributes and block layout
were identical in every entry. **The number of files whose contents differed is zero.** The
filesystem UUID, hash seed and journal inode were identical too. The verdict recorded is
`METADATA-TIMESTAMPS-ONLY`, which is a description of the difference and explicitly not a reason to
accept it.

### 25.3 The cause, read out of the frozen binaries

`E2FSPROGS_FAKE_TIME=0` is a silent no-op. In this build of `libext2fs`, `fs->now == 0` is the
"nobody pinned a time" sentinel: `ext2fs_initialize` reads the variable, stores it at offset `0xb8`,
then branches on zero and calls `time()` on the zero path. Setting the pin to the sentinel is
indistinguishable from not setting it, so both jobs read their own clock and their clocks were 58
seconds apart.

Nothing was executed to establish this. It was read statically out of the exact binaries the plan
pins, and the disassembly sites are recorded in the sealed record beside the digests of the files
they came from.

### 25.4 What this record does not authorise

Neither image is adopted. No third image was produced. No image was booted. `bootableClaim: false`,
`activationAllowed: false`, `guestBootVerified: false`, `runtimeCompatibilityVerified: false`. A
metadata-only difference is still a difference, and byte identity remains the criterion.

### 25.5 The successor, pre-registered before the fix exists

`native/containment/native-shadow-boot-root-disk-determinism-successor-authority-arm64-v1.json`
carries the bar the fix has to clear. It was written and pushed before any production code changed,
because a bar written after a result is not a bar.

It separates two numbers that used to be one. The canonical source epoch stays `0`: that is what the
staged inputs mean and it is unchanged. The ext4 writer time becomes `1` — the smallest value that
is both non-zero and fixed, and therefore the smallest change that makes the pin take effect at all.
Substituting `SOURCE_DATE_EPOCH`, the current time or an arbitrary release timestamp is refused in
writing, and `debugfs` may not be promoted from inspector to writer.

Before any of that was written down, the claim was checked mechanically against the frozen binaries.
Every field that moved is traced to `fs->now`: `s_wtime` through `ext2fs_flush2` at `0x13774`,
`s_lastcheck` and `s_mkfs_time` through `ext2fs_initialize` at `0x2cd3c` and `0x2cd40`, and
`i_ctime`, `i_mtime`, `i_atime` and `i_crtime` through `ext2fs_write_new_inode` at `0x30508`,
`0x30514`, `0x30520` and `0x305c4`. In every case the store takes the pinned value directly and
`time()` sits behind the zero branch. The checksums follow from the fields they cover. Nothing is
left over. That says a fixed non-zero value reaches every field that moved; it does not say the
images will match, because only a produced pair says that.

The successor also closes two things the predecessor recorded as open. The executor already computes
which library files the loader will use and then discards the answer, so the successor records that
value rather than adding a second computation that could disagree with the one that fed the loader.
And no filesystem check was ever run, so the successor pins `e2fsck` by digest and runs it `-f -n`
per replica: forced, because a check that exits 0 without reading anything is close enough to not
running to be worthless, and read-only, with `0` as the single accepted exit code.

### 25.6 Execution cursor

```text
ROOT-DISK-BYTE-IDENTITY  FAILED — 두 복제본 불일치, 원인 확정, HARD STOP 봉인 완료
DETERMINISM SUCCESSOR  PRE-REGISTERED — 구현 전, 생산 전
PHASE D (MAC.3 closed-local boot)  BLOCKED — successor GREEN 이후에만 허용
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged. No public mining, no
paid-API benchmark and no release claim is made anywhere in this section.

## 26. The §25.5 completeness claim was wrong, and the successor value is not sufficient

### 26.1 What the earlier claim said and what it left out

§25.5 above says of the pre-verification: "Nothing is left over." That sentence stays as written. It
was wrong, and this section says how, because a record that quietly repairs its own past is not
evidence of anything.

The pre-verification read `libext2fs` and found every timestamp writer in the library gated on
`fs->now`. That part holds and is unchanged. What it did not read is `mke2fs` itself. When `mke2fs`
is given `-d`, it walks the staging tree and, for each entry it copies in, calls
`ext2fs_write_new_inode` — the library path that was checked — and then immediately overwrites three
of the fields that call just wrote, from the staging file's own `struct stat`. That second write
never reads `fs->now`. It is in the program, not the library, which is why reading the library
looked complete.

### 26.2 How it was read

`objdump` over the frozen `mke2fs`, whose digest the plan already pins. Nothing was executed and no
image was produced. The stat buffer is identified by the `lstat64` call site at `0x139f0` and the
`S_IFMT` mask that follows it; the inode buffer by the `i_links_count` store at `0x13d08`. Between
them, at `0x13dac` and `0x13da0`, sit two stores that move `st_atime`, `st_ctime` and `st_mtime` into
inode offsets `0x8`, `0xc` and `0x10`, followed at `0x13db0` by `ext2fs_write_inode`. For contrast,
the inodes `mke2fs` creates on its own account do read `fs->now`, at `0x13ca4`, with the zero-branch
at `0x13cc0` — so the pinned value does reach those, and only those.

This reconciles exactly with what the predecessor measured. Of the inodes that differed between the
two images, `i_atime` differed in five and `i_mtime` in five — and those five are the inodes `mke2fs`
creates itself — while `i_ctime` differed in all of them.

A second pass replaced the two weak links in that reading. The stat buffer no longer rests on an
inference from `S_IFMT`: `0x13970` computes its address outright and `0x13974` spills it to the slot
`lstat64` is handed, so the base is read rather than deduced. And the walk's displacements are no
longer trusted as transcribed — the record now carries each field's offset inside its struct beside
its address, and the test module recomputes every address from the base. Six fields are recomputed
against each base, three of which — the mode and the two ownership ids — are no part of the claim,
which is what makes them useful: a base wrong by any amount would land the ownership somewhere it
visibly is not. The load-bearing half of the finding is an absence, so it is counted rather than
asserted: across the whole window from `0x13d1c` to `0x13db0`, the number of loads from the
`ext2_filsys` pointer is zero, and the number at the `fs->now` displacement `0xb8` is zero. The
contrast at `0x13ca4` is the control that makes the zero mean something — the same register and the
same displacement, in the same binary, a few instructions earlier.

### 26.3 Why the fix does not follow from this

`st_atime` and `st_mtime` can be pinned from userspace; `st_ctime` cannot. `utimensat` sets atime and
mtime only, and any call that changes an inode's metadata updates its ctime as a side effect. So
every staged entry carries the wall clock of the moment it was staged, and copying that into the
image reproduces the original failure in one field.

The successor's writer time is therefore **necessary but not sufficient**. It removes `i_atime`,
`i_crtime`, `i_mtime`, `s_lastcheck`, `s_mkfs_time` and `s_wtime`. `i_ctime` survives it.

### 26.4 What was done about it, and what was not

The correction is recorded in the successor authority as an append-only `corrections` entry; the
claim it corrects stays byte-identical, and the three sealed predecessor files are untouched. The
produce phase now refuses to start while a named cause is still recorded as present, because this
record allows exactly one production pair and forbids retrying a pair that has produced a result —
spending that attempt on a known outcome would burn it.

No remedy is adopted here. Every candidate reaches past this record's authority: pinning a different
`e2fsprogs` changes the sealed source lock, promoting `debugfs` to a post-hoc writer is refused in
writing, and excluding `i_ctime` from the comparison relaxes the acceptance criterion. The options
are stated in the record and the choice is the operator's.

Had this been missed, it would not have been silent: the timestamp audit added to the produce phase
aborts a replica with `wall-clock-survived-in-the-image` before any comparison runs.

### 26.5 Execution cursor

```text
ROOT-DISK-BYTE-IDENTITY  FAILED — 두 복제본 불일치, HARD STOP 봉인 유지
DETERMINISM SUCCESSOR  BLOCKED — 원인 staged-inode-ctime-is-not-fs-now 미해소, 생산 금지
PHASE C (production pair)  NOT DISPATCHED — 운영자 결정 대기
PHASE D (MAC.3 closed-local boot)  BLOCKED — successor GREEN 이후에만 허용
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged. No public mining, no
paid-API benchmark and no release claim is made anywhere in this section.

## 27. Testnet/public task-instance separation and activation-order correction (2026-08-27)

### 27.1 What this corrects

The arm64 image work in sections 18–26 prepares a deterministic place to run the checker. It does
not issue a mining task, consume a task, start a testnet or authorize mining. A bootable guest is a
runtime prerequisite only. Treating its completion as an activation step would skip the network
and replay gates that still have to be built and measured.

`LLM-MINEABLE-ELIGIBLE-V5=14,160` is the number of currently counted templates from which the
frozen families can issue instances. It is not a stockpile of 14,160 answer strings and it is not a
claim that a model solved 14,160 individual tasks. An anchor is source material. A template binds
that anchor to a frozen family/generator/checker contract. An issued instance is the network- and
epoch-specific challenge made from that template.

### 27.2 Frozen separation rule

The successor protocol gate is named `TESTNET-INSTANCE-DOMAIN-SEPARATION-V1`. Before a private
testnet may use these families, every challenge commitment, submission binding, receipt and replay
key must be bound to at least:

```text
network_id || chain_id || family_version || template_id || epoch || challenge_seed
```

This expression names fields; it does not authorize ambiguous string concatenation. The successor
must use a versioned domain tag and canonical length-delimited encoding. It may bind additional
frozen fields such as the challenge, policy and registry digests, but it may not remove any field
above. The node must recompute the binding from its own network configuration and registry rather
than trust values supplied by a miner. A receipt or submission envelope created for the testnet
must therefore be unusable in the public-network domain. Testnet and public issuance/consumption
ledgers must be separate even when they use the same template family.

The rejection gate is bidirectional: a public-domain envelope/receipt must likewise be unusable in
the testnet domain. This is domain replay rejection, not a global blacklist of identical raw source
bytes submitted independently to two fresh tasks.

`TEMPLATE-INVENTORY-DOES-NOT-DECREASE-ON-FRESH-ISSUANCE`: issuing a fresh instance with a new
network/epoch/seed does not consume its template. What is consumed is that particular issued
instance in that particular network domain. This statement applies only to families whose frozen
generator can create fresh instances and whose checker remains bound to them. It is not a claim of
unlimited freshness for every counted family.

`STATIC-INSTANCE-EXPOSED-NEVER-PROMOTED`: a static, non-fresh or one-shot fixture exposed on a
testnet may never later be counted or issued as a public-network task. Such fixtures require a
test-only inventory and permanent public-domain exclusion. Passing a testnet with fixtures is not
evidence that public inventory grew.

### 27.3 Correct execution ladder

The product-level order is:

```text
runtime image -> closed-local issue/check -> cross-network replay gate -> private testnet -> BF.7 zero-reward testnet -> BF.8 activation evidence -> separately approved activation
```

Here `private testnet` means a non-consensus integration network before BF.7. This short product
view does not remove or replace the formal BF.3, BF.6, BF.6a, RP0-MD or deterministic-resource
prerequisites already frozen by the master plan. Each arrow is a gate, not an automatic
continuation. In particular:

1. the current root-disk successor and later Mac boot can only establish the runtime-image step;
2. the closed-local step must issue a fresh instance and complete the node-owned check path without
   block or reward effects;
3. the replay gate must prove that a testnet-bound submission/receipt is rejected under a distinct
   public-network domain, that the reverse direction is also rejected and that the two ledgers
   cannot alias;
4. the private testnet must run with rewards and activation disabled;
5. BF.7 remains its own zero-reward protocol gate and BF.8 remains a later evidence gate, not an
   activation implementation; and
6. actual activation still requires the formal prerequisites, an Economic ADR, a separate
   implementation plan and an operator decision after those gates are green.

This correction is a plan and test contract, not its implementation. No network-domain binding,
testnet run or activation is claimed here. Section 26.5 remains unchanged as a historical record.
The current cursor is that the plucky writer remedy and sealed production path are landed on
`main`; the one-shot Phase C production pair is ready but has not been dispatched, and Phase D
remains blocked until that pair is GREEN.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false`,
`activationAllowed=false` and `bootableClaim=false` remain unchanged.

## 28. The root disk was produced twice again, and the two copies were identical

Run `33045285925` dispatched the produce workflow once, at commit
`cb44b34011b491e652fb961ed4dcb13fd3d1ac1b`. Two arm64 jobs each produced exactly once, on separate
runners, neither able to read the other's output. They converged:

```text
guest-kernel     d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336     57,860,488 B
guest-initrd     4674128144befeea20b1cbeb5af340b981b7b125d32d43630c721bb4b0aecab2  1,010,932,716 B
guest-root-disk  9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc  1,168,314,368 B
```

This section supersedes the closing cursor sentence of 27.3: the pair has now been dispatched
once and is GREEN. Section 27 itself is left as written.

The sealed digest is not one replica's file chosen over the other's; it is the value both arrived at
independently. The `compare` job reported `identical: 3 outputs`. The result is sealed append-only in
`native/containment/native-shadow-boot-root-disk-determinism-green-arm64-v1.json`.

### 28.1 What removed the cause

Section 26 established that `i_ctime` survived the successor's writer time because `set_inode_extra`
runs after `ext2fs_write_new_inode` and copies each staged file's `st_ctime`, which no caller can
set. The writer was replaced rather than worked around: an official `e2fsprogs 1.47.2-1ubuntu1` build
reads `SOURCE_DATE_EPOCH` first, which both sets `fs->now` and arms the flag `set_inode_extra`
branches on. `E2FSPROGS_FAKE_TIME` is still read, but sets the time without arming that flag — which
is the branch the sealed failure ran on.

The writer was added as an image-production tool only. The 191 guest packages were not replaced and
not deleted: the guest tree and the tool tree are extracted separately from the same layer bytes, so
adding a writer changes what writes the image and not one byte of what is written. The produce phase
re-reads the guest lock off disk and refuses if it has moved.

### 28.2 What the record does not do

The sealed failure record and the successor authority are not edited. A green result is exactly when
softening them would be easiest and would cost the most, so the new record binds them by digest
instead, and the tests hash them back off disk. The successor authority still says the cause is open,
because against the writer it was written for the cause *is* open. What allows production is a
separate derivation, keyed on the cause by name, so a second cause or a renamed one has no clearance
and still refuses.

An earlier dispatch, run `33042531739` at commit `c362c8e`, refused at that gate on both replicas
before the output directory was created. Nothing was written and nothing was uploaded, so it did not
spend the one production pair.

### 28.3 The seven conditions, and the evidence for each

Every condition was registered before the run.

- Kernel, initrd and root disk byte-identical across replicas — the three digests above, plus the
  `compare` job's own verdict.
- No wall-clock timestamp anywhere in the image — the audit read every superblock and inode stamp on
  both replicas and counted zero values outside `[0, 1]`.
- Loader provenance inside the frozen closure — every library the writer and the checker opened is
  recorded by path and `sha256`, and the two replicas' lists are equal. The writer and the checker do
  not share `libext2fs.so.2`: the writer loads the selected build's, the checker the frozen one's.
- Frozen `e2fsck -f -n` exits 0 on both — the checker is a different binary from the writer and did
  not write the image, `-f` forces a full check so that a clean superblock cannot buy a pass that
  checked nothing, and `-n` opens the filesystem read-only. No repair option was used.
- Zero unexplained differences — the only textual difference between the two results is the ephemeral
  scratch directory name; masking it makes the two files byte-identical.

### 28.4 What this is not

Two identical images say the writer is deterministic. They do not say the guest boots. `bootableClaim`
is false in the record, and the record refuses clean-Mac evidence, CURL.3, product release, public
mining and activation along with it. What it unlocks is MAC.3 closed-local development-Mac boot work
and nothing else.

### 28.5 Execution cursor

```text
ROOT-DISK-BYTE-IDENTITY  GREEN — 두 복제본 바이트 동일, run 33045285925, append-only 봉인 완료
DETERMINISM SUCCESSOR  RESOLVED — 원인 staged-inode-ctime-is-not-fs-now 제거 (기존 봉인 기록은 무수정)
PHASE C (production pair)  COMPLETE — 1회 dispatch, replica당 1회 생산, 재실행 없음
PHASE D (MAC.3 closed-local boot)  UNBLOCKED / IN PROGRESS — 개발용 Mac 로컬 부팅 자격 확인
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged. No public mining, no
paid-API benchmark and no release claim is made anywhere in this section.

## 29. The guest was booted once on a development Mac, and did not pass

The one attempt allowed by the frozen MAC.3 qualification was performed and is sealed in
`native/containment/native-shadow-mac3-closed-local-boot-result-arm64-v1.json`. Verdict: **FAIL**.
Five of the six pre-registered conditions were met; `guest-systemd-is-pid-1` was not. The attempt
was not repeated and no condition was reworded to make it pass.

### 29.1 What the run established

The kernel from run `33045285925` booted on Virtualization.framework with no initrd, on a machine
built with no network device, no shared directory and no writable disk. virtio-blk presented the
sealed image as `/dev/vda` at its exact size, `2281864` 512-byte blocks. The kernel mounted the
ext4 filesystem read-only from it, reported the root mounted, and executed
`/usr/lib/systemd/systemd` as its init process. Afterwards the image hashed to
`9834036f7738f3848fff23e5c3d1be85cd1f288f7ca43d2094b815eca2b378cc`, the value it had before, so the
read-only attachment held.

That is the boot path — loader, disk, filesystem, hand-off — working end to end against the sealed
image. It is not a booted userspace, and this section does not claim one.

### 29.2 What failed, and why it is the image rather than the boot

PID 1 stopped four lines after the hand-off:

```text
Run /usr/lib/systemd/systemd as init process
Failed to mount proc (type proc) on /proc (...): No such file or directory
Failed to mount sysfs (type sysfs) on /sys (...): No such file or directory
Failed to mount devtmpfs (type devtmpfs) on /dev (...): No such file or directory
Freezing execution.
```

The root filesystem has no mount-point directories for the kernel's virtual filesystems. Its top
level holds `usr`, `etc`, `opt`, `var`, `boot` and the usr-merge symlinks, and nothing else. The
kernel had already said as much before the hand-off, with `devtmpfs: error mounting -2` and
`Failed to create /dev/root: -2`. The frozen marker for the failed condition is a `systemd[1]:` log
line, which systemd emits only once those mounts exist, so the condition is not met and the record
says so.

Three things this is not. Not the boot loader: the kernel ran and handed over. Not the image's
determinism: the bytes read here are the bytes both replicas produced. Not the absent account
database registered before the run: systemd froze long before any user lookup.

### 29.3 What the failure does and does not move

The determinism result stands unchanged — two identical images are still two identical images. What
the run adds is that a deterministic image is not yet a booting userspace, which is exactly the gap
section 28 said two identical images could not speak to.

The three missing mount points are recorded as found *by this run*, not back-dated into the list of
gaps registered before it. That list still holds exactly the two it held: the absent account
database and the absent runtime rootfs.

Continuing requires a new guest image whose builder creates those empty directories, and with them
the `/run` and `/tmp` the same startup needs. That is a new production of the root disk. The
standing instruction allows exactly one production pair and it has been spent, so a new one requires
a separate operator approval before it may be produced or booted again.

### 29.4 Execution cursor

```text
ROOT-DISK-BYTE-IDENTITY  GREEN — 두 복제본 바이트 동일, run 33045285925, 봉인 유지
PHASE C (production pair)  COMPLETE — 1회 dispatch, replica당 1회 생산, 소진됨
PHASE D (MAC.3 closed-local boot)  RUN ONCE / VERDICT FAIL — 조건 6개 중 5개 충족,
  PID 1이 /proc·/sys·/dev 없음으로 정지; 재실행 없음, 기준 완화 없음
NEXT (blocked on approval)  이미지 빌더가 마운트 지점 디렉터리를 만들도록 고친 뒤 새 생산 1회
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false`, `activationAllowed=false` and `bootableClaim=false` remain unchanged. No
public mining, no paid-API benchmark and no release claim is made anywhere in this section.

## 30. The successor guest was booted once, and passed (2026-08-27)

Section 29 recorded a run that did not pass. This records the run that followed it. Nothing in
section 29 is edited by this one: the first attempt's qualification, its sealed failure and the two
determinism records were re-hashed immediately before this run and are byte-unchanged.

### 30.1 What was frozen before the run

The successor qualification
(`native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v2.json`, sha256
`bf703945ec02f1f66b492f7bb0c6e4080190caea17dc063e2868ef688669abb7`) was merged before it could be
run. It opened one allowance, named its own result path so neither attempt could overwrite the
other's evidence, and carried the six pass conditions **byte-identical** to the first attempt's. The
driver re-checks that identity against the first attempt's file before it will build a machine, so a
reworded bar is refused rather than run.

The subject is the image two arm64 replicas independently converged on: kernel
`d29e317d…f1336`, initrd `3ae76ced…88ca` (hashed, not attached — this image boots without one) and
root disk `566614b6…c41b`.

One correctness defect was found and fixed in the driver before the run, in PR #264: the six judging
rules resolved the expected digests internally, which meant the *first* attempt's subject. A
successor run would have been judged against the failed image's digests. Each rule now receives the
digests of the attempt actually selected.

### 30.2 Preflight, then exactly one run

Twenty read-only checks ran immediately before the machine was created: the tree matched its remote
and was clean at `e7dc43e7a692aec9f7518d7e8605bc6d8d693b93`, the record was at the digest it was
merged with, the attempt was unspent, no result file existed, the three sealed images matched by
size and digest, the six conditions were byte-identical to the first attempt's, and the declared
machine was closed. A dry run built and validated the configuration without creating a virtual
machine, which costs nothing against the allowance.

The run then happened once: 15:27:27Z to 15:30:44Z, 195.1 seconds inside the machine, stopped by the
host at its 180-second limit because a closed guest has no channel to be told to shut down. Host
exit status 0. No host process, socket or temporary file of the machine outlived it.

### 30.3 The verdict

All six conditions MET, judged by the frozen wording:

```text
loads-the-converged-image                MET
closed-local-configuration               MET   0 network devices, 0 shared directories, 1 read-only disk
kernel-reaches-its-root-filesystem       MET   EXT4-fs (vda): mounted filesystem ... ro
guest-systemd-is-pid-1                   MET   systemd 255.4-1ubuntu8, arm64, vm-other
sealed-image-unchanged-after-the-run     MET   566614b6…c41b before and after
console-transcript-captured-and-hashed   MET   20409 bytes, sha256 e7d09560…8f2160
```

The five empty directories the successor builder adds were the whole of the fix. PID 1 mounted
`/proc`, `/sys` and `/dev` and did not freeze, which is precisely where the first attempt stopped,
and the guest went on to reach `sysinit.target`, `basic.target`, `multi-user.target` and its default
`graphical.target`. Exactly one unit entered a failed state: `ldconfig.service`, which rebuilds the
dynamic linker cache and cannot, because the root is attached read-only. That is the containment
working, not a defect.

The transcript has two digests because it has two forms. The raw file is 20409 bytes, sha256
`e7d09560…8f2160`. The judge reads it as text, which collapses the 230 CRLF pairs the guest emitted
into single newlines: 20179 bytes, sha256 `22c4551c…38f2e`. 20409 − 20179 = 230, exactly the pairs
counted. The file is valid UTF-8 and nothing was dropped. Both digests are recorded so neither has
to be taken on trust.

### 30.4 What the pass does not say

`boole-native-shadow-launcher.service` is reported by systemd as **Started**. That means systemd
executed its `ExecStart` — the unit is `Type=exec` with `User=root`, so it needed no account lookup
and the absent `/etc/passwd` did not block it. Started is not a serving state and is not read as one
here.

Where the launcher refused is **not observable from this run**. The unit sets
`StandardOutput=journal` and `StandardError=journal`, so its own output went to the guest journal; a
machine with no network device, no shared directory and no writable disk has no channel out, and the
run was stopped from the host rather than shut down from inside. The console shows no failed state
for the unit across the 195 seconds, and that is the whole of what this run establishes about it.
Making it observable means routing the launcher's output to the console the host already captures,
or the authenticated transport MAC.4 is for.

The two gaps registered before the first run are still gaps: the account database the guest's own
sysusers file expects to fill in, and the runtime rootfs and manifest under `/var/lib/boole` the
launcher verifies before it serves anything.

This is a closed-local boot on one development Mac, ad-hoc signed with
`com.apple.security.virtualization` and no Team ID, Developer ID certificate, provisioning profile or
notarization. It is not clean-Mac evidence, not a product release, not runtime compatibility, and not
public mining, a paid-API benchmark or a leaderboard claim.

### 30.5 Execution cursor

```text
ROOT-DISK-BYTE-IDENTITY  GREEN — 두 복제본 바이트 동일, 봉인 유지
PHASE C (production pair)  COMPLETE — 1회 dispatch, replica당 1회 생산, 소진됨
PHASE D (MAC.3 closed-local boot, attempt 1)  RUN ONCE / VERDICT FAIL — 기록 보존, 수정 없음
PHASE D (MAC.3 closed-local boot, attempt 2)  RUN ONCE / VERDICT PASS — 6/6 충족,
  기본 target 도달, 봉인 이미지 전후 동일, 재실행 없음, 기준 완화 없음
NEXT  MAC.3 guest runtime / launcher serving — 계정 DB·런타임 rootfs·읽을 수 있는 출력 경로
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`LLM-MINEABLE-ELIGIBLE-V5=14,160`, `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged. `bootableClaim` becomes
`true` only in the scope written above and in the sealed result: this image, this closed
configuration, one development Mac. No public mining, no paid-API benchmark and no release claim is
made anywhere in this section.

## 31. The MAC.3 guest runtime contract, frozen before anything answers it (2026-08-27)

The guest boots. That is the whole of what §30 sealed, and the launcher's own refusal point stayed
invisible inside it. This section opens the next step by writing down what "the launcher serves"
would have to mean, before any run is allowed to claim it.

### 31.1 The read-only survey

Seven areas were surveyed without producing anything, and each finding is bound in
`native/containment/native-shadow-mac3-guest-runtime-contract-arm64-v1.json` to the digest of the
file it was read from.

| Area | What is there today |
| --- | --- |
| Account database | No `/etc/passwd`, no `/etc/group`. The sysusers configuration that would create the two accounts ships in the image, but nothing can write them on a read-only root. |
| Launcher binary and digest | Sealed at `11b5d1cf…`, 2,006,632 bytes, at `/usr/libexec/boole/boole-native-shadow-launcher`, admitted only by rebuild-and-match-seal. |
| Service and enablement wiring | Unit staged into `/usr/lib/systemd/system`; the enablement symlink is a declared derived entry. §30 recording the unit as started is that wiring working. |
| Runtime directory | `/run/boole` and `/run/boole/native-shadow` (`2750`, `root:boole-node`), lock at mode `0600`. `/run` is a tmpfs, so these are writable already. |
| Socket path | `/run/boole/native-shadow/launcher.sock`, same tmpfs, peer uid and gid checked against the resolved node account on every connection. |
| tmpfiles and sysusers configuration | Both ship. Three tmpfiles lines ask for directories under `/var/lib/boole`, which is on the read-only root and cannot take them; the two under `/run` can. |
| Writes needed on a read-only root | The runtime directory, socket and lock need nothing. `/var/lib/boole/native-shadow/node-state` is not on a tmpfs, and the replay node unit declares it read-write. |

### 31.2 The three gaps, and what closes each

- **The account database.** The launcher resolves `boole-node` and `boole-native-checker` before it
  does anything else. Closing this means baking the database into the image, precisely because the
  thing that would normally create it cannot write to a read-only root.
- **The runtime rootfs.** The launcher verifies `/var/lib/boole/native-shadow/runtime-rootfs`
  against a content manifest whose digest it is compiled against. That digest and the one the arm64
  replay expectation already seals are the same value, so nothing new is invented to close this.
- **A readable path for the launcher's own output.** Sending its standard output and error to the
  console the host already captures and hashes adds no device, no network and no shared directory,
  and turns "not observable from this run" into an observation.

Each gap is a file that has to be *inside* an image that is read-only and content-addressed, so
none of them can be added to the one that already booted. A new production is required. It is
recorded as required and not as performed.

### 31.3 The ten conditions, and the one that is held

Nine of the ten minimum conditions are frozen with the method that will check each of them written
beside it. The tenth — *the launcher runs under an unprivileged account* — is held, not softened.

The launcher is a privilege manager. It refuses to start unless it is root holding exactly four
capabilities, verifies that mask itself before anything else, and is what drops to the unprivileged
node and checker accounts on the way to the work. `crates/boole-native-shadow-launcher/src/privilege.rs`
requires it and `scripts/native-shadow-launcher-privilege-gate.sh` proves both a missing and an
extra capability fail closed. Running the launcher itself unprivileged is therefore not a
configuration change; it would remove the delegation and account separation the containment is
built on.

A reading that keeps the intent — least privilege, with every served execution unprivileged — is
recorded beside the condition and explicitly **not applied**. Which of the two the project takes is
the operator's decision, and the record fails its own tests if that condition is ever marked
satisfied, waived or reworded while it is still held.

### 31.4 Execution cursor

```
PHASE D (MAC.3 closed-local boot, attempt 2)  RUN ONCE / VERDICT PASS — sealed, §30
MAC.3 runtime contract                        FROZEN / NOT RUN — ten conditions, one held
MAC.3 guest runtime inputs                    OPEN — account database, runtime rootfs, readable output
NEW IMAGE PRODUCTION                           REQUIRED / NOT PERFORMED — one pair, frozen criteria first
MAC.4 authenticated host-guest transport       NOT STARTED — not begun in this session
CURL.3                                         DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` are unchanged by this section. No serving claim, no clean-Mac claim, no
public mining and no paid-API benchmark is made anywhere in it.

## 32. The MAC.3 guest runtime input set, frozen before an image exists (2026-08-27)

Section 31 named three gaps. This section closes two of them as files and refuses to pretend it
closed the third. Nothing here was built, booted or served; §30's sealed boot result and §31's
frozen contract are byte-unchanged.

Record: `native/containment/native-shadow-mac3-guest-runtime-inputs-arm64-v1.json`, status
`MAC3-GUEST-RUNTIME-INPUTS-FROZEN-NOT-BUILT`, `imageProduced: false`, `servingClaim: false`,
`activationAllowed: false`. It carries no verdict field, and its tests fail if one is added.

### 32.1 The account database

`systemd-sysusers` is the thing that would normally write `/etc/passwd`, and it cannot write to a
read-only root. So the database is baked. Five files:

| Repo path | Guest path | What it settles |
| --- | --- | --- |
| `native/etc/passwd` | `/etc/passwd` | `boole-node` at 990 and `boole-native-checker` at 991, homes `/nonexistent`, shells `/usr/sbin/nologin` and `/bin/false` |
| `native/etc/group` | `/etc/group` | same-named groups at the same numbers, every member list empty |
| `native/etc/shadow` | `/etc/shadow` | every entry `*`, which no password matches |
| `native/etc/gshadow` | `/etc/gshadow` | same groups, locked, no members |
| `native/etc/nsswitch.conf` | `/etc/nsswitch.conf` | every database resolves from files and nothing else |

The identity contract in `crates/boole-native-shadow-protocol/src/service_identities.rs` asks eight
questions, and all eight are answered from these files rather than at runtime: the name matches; uid
and gid are both non-zero; home is `/nonexistent`; the shell is one of the two allowed; a same-named
group exists at the passwd primary gid; that gid resolves back to exactly that group; the account's
full group list is exactly its primary gid, which is why every member list is empty; and node and
checker share neither number. `sysusers` still runs, still finds both accounts present, and in that
case writes nothing — so the read-only root stops being a problem rather than being worked around.

Three things are stated in the record rather than left for a later reader to find. Neither shell
named exists in the image: the boot package set contains no `login` and no `coreutils`. The contract
compares the shell as a string and never executes or stats it, and nothing in the guest runs a shell
for these accounts, so resolution is unaffected. Root's shell is `nologin` rather than the `/bin/bash`
`base-passwd` would install, because the image ships neither bash nor dash and naming an unreachable
shell is worse than naming the one that refuses by design. And the database is authored rather than
derived from `base-passwd`, whose entries are installed by a maintainer script the deterministic
build does not run; root and nobody keep the numbers `base-passwd` uses.

`nsswitch.conf` names no `systemd` module — the image ships no `libnss-systemd` — and no `dns`,
which would want a network the guest does not have.

### 32.2 A channel the refusal can be read on

`native/systemd/boole-native-shadow-launcher-v2.service` differs from v1 in exactly two lines, and
the tests assert that as a line-by-line comparison rather than as prose: `StandardOutput` and
`StandardError` each gain `console` alongside `journal`. The console is a channel the host already
captures and hashes, so this adds no device, no network and no shared directory. The capability set,
`NoNewPrivileges`, `User=root` and every other line are byte-identical to v1.

### 32.3 Writes, by construction rather than by luck

`native/tmpfiles.d/boole-native-shadow-v2.conf` keeps the two `/run/boole` rules and drops the three
`/var/lib/boole` rules. Those three ask for directories on a filesystem mounted read-only; they
could never have succeeded. Removing them makes "every write lands on tmpfs" true because nothing
else is asked for, rather than true because the failures happened to be harmless. Nothing the
launcher needs was under `/var/lib/boole`: the replay node unit is the only thing that names
`node-state`, and that unit is not staged into this image.

Both successors go to the **same guest paths** as the files they replace. The v1 files stay in the
tree at their sealed digests, because four authority records name them and those records are not
reopened.

### 32.4 The gap that stays open

`/var/lib/boole/native-shadow/runtime-rootfs` is not an input file. It is a tree the build
materialises from the already-sealed package set, so closing it is a change to the builder and to
what the image contains. It stays open in this record and in §31's contract, the launcher still
refuses at `verify_runtime_rootfs_replay`, and no condition below is answered by these inputs.

### 32.5 Execution cursor

```
PHASE D (MAC.3 closed-local boot, attempt 2)  RUN ONCE / VERDICT PASS — sealed, §30
MAC.3 runtime contract                        FROZEN / NOT RUN — ten conditions, one held
MAC.3 guest runtime inputs                    FROZEN / NOT BUILT — two gaps closed as files, one open
MAC.3 runtime rootfs in the image             OPEN — a builder change, not an input file
NEW IMAGE PRODUCTION                          REQUIRED / NOT PERFORMED — one pair, frozen criteria first
MAC.4 authenticated host-guest transport      NOT STARTED — not begun in this session
CURL.3                                        DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED
```

No wallet seed, model API key or node secret is named in or staged into the guest by any file in
this set. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false`
and `activationAllowed=false` are unchanged. No serving claim, no public mining and no paid-API
benchmark is made anywhere in this section.

## 33. The successor image production criteria, and the chain that precedes them (2026-08-27)

Section 32 froze seven files and left one gap open. Those files change nothing until an image
carries them, and carrying them means producing an image. This section records what a produced
image would be judged against, written before one exists, and what producing it would actually
cost, measured without editing anything.

`native/containment/native-shadow-mac3-successor-image-production-criteria-arm64-v1.json`
(18385 bytes, `417d2497072519031506664553a0d9b478c53a7bf7983f431332f69bbecec4b8`) is the record.
Attempt identity `MAC3-SUCCESSOR-IMAGE-PRODUCTION-ARM64-ATTEMPT-1`, one run allowed, none spent,
result path named and empty. It carries no `verdict`, `passed` or `result` field, and its tests
fail if one appears.

### 33.1 What would count as acceptable

| id | condition |
| --- | --- |
| `exactly-one-production-pair` | dispatched once, two replicas |
| `both-replicas-agree-byte-for-byte` | kernel, initrd and root disk all identical |
| `the-root-disk-passes-a-read-only-check` | `e2fsck` clean, and the disk's digest unchanged afterwards |
| `the-seven-inputs-are-in-the-image-at-their-frozen-digests` | a missing path and a wrong digest are the same failure |
| `the-runtime-rootfs-and-its-manifest-are-in-the-image` | equal to what the launcher compiles against; merely present is not enough |
| `the-launcher-binary-is-unchanged` | changing what the guest runs would need its own record |
| `nothing-secret-and-nothing-connected-is-in-the-image` | checked on the produced image, not argued from the input list |

Each row carries the check that judges it, so none of them is a sentence that could be satisfied by
agreement. The read-only check re-takes the disk's digest afterwards, because a filesystem check
that repaired something and reported success would otherwise pass.

### 33.2 Where it stops instead of continuing

Five abort conditions. `criteria-would-have-to-be-loosened` is the load-bearing one: if passing
requires any condition above to be reworded, waived or dropped, the run stops and an operator
decides. `replicas-disagree` stops rather than retries — re-running until two builds happen to
agree would replace determinism with sampling, and a build that does not reproduce is itself the
finding.

### 33.3 The chain that has to be walked first, measured read-only

Staging the seven inputs is not a builder edit; it is four sealed records in a fixed order. The
record's `successorChainForStaging` lists each with its current digest and what a successor would
have to change:

1. the boot rootfs source lock **plan** — ten tracked files and ten authority bindings grow to
   seventeen, `expected.trackedFileCount` follows, and `guestInitRoles` gains a role per file;
2. the lock **generator** — it reads the successor plan and writes the successor lock. Its digest
   is pinned inside the plan and the plan's digest inside it, so the pair is re-sealed with the
   embedded plan digest zeroed while the tool is hashed, which is how the existing pair already
   breaks that cycle;
3. the **lock** itself — regenerated from the successor plan;
4. the boot builder's staging table — seven entries added.

Measuring it produced two findings worth recording. First, **no package is re-downloaded**: the 191
package rows come from the dependency candidate result already sealed in the tree, so a successor
lock does not need the payload cache that this host does not have. Second, **extending the staging
table invalidates no pin**: the two builder digests the boot module pins belong to other files, and
its own digest is computed at import rather than written as a literal.

The first of those two was promoted from an inference to a demonstration. The sealed lock was
regenerated in place with the generator's `--write` mode and the tree compared afterwards: the
lock, its result and its plan all came back byte-identical, at lock digest
`9eb70e05e0daf8cc56c0741c5c8ca266cad819d059ca28bcadeaecf84c0531cf`. The generator also contains no
network code at all -- no `urllib`, `requests`, `socket` or `subprocess` -- so there is no path by
which it could have fetched anything. What that does not show is recorded too: reproducing the
existing pair establishes that the generator is deterministic and offline, not that a different
pair would pass.

The second was demonstrated as well. One probe entry was added to the staging table and the module
imported cleanly, so no pinned digest refused it; the only check that refused was the one asserting
the authority table covers every tracked file in the boot lock, and it failed on the count -- eleven
against the lock's ten. The builder was restored byte-identically and its tests are green. That
fixes which step is actually blocking: step four is held up by the lock not listing the file yet,
which is what steps one to three produce, rather than by a digest that would need re-sealing. A
single probe shows which check refuses first, not that a full seven-entry extension passes, and the
record says so.

Nothing in this section opens any of the four. Surveying the chain is not walking it, and the record
says so in its own `whatThisIsNot` field.

Not walking it is a decision rather than a silence, so the record states it. Walking these four
steps stages the seven input files and nothing else, which leaves the runtime rootfs gap open; a
production run under these criteria would then fail
`the-runtime-rootfs-and-its-manifest-are-in-the-image` by construction, spending the one allowed run
on an answer already known. Closing the runtime rootfs afterwards would require a second successor
for the same four records, because it also adds tracked material to the plan and the lock. Every
successor here is append-only, so doing it twice costs two full chains, two sets of superseded
records and two rounds of consumer updates to reach a state one chain could have reached. The
staging extension and the runtime rootfs are therefore treated as one unit of work, and neither is
started. The record also names what would reverse that: if the runtime rootfs turns out not to touch
the plan or the lock, the two stop being one unit and the staging extension can go first alone.

### 33.4 Execution cursor

```
MAC.3 successor image production criteria = PRE-FROZEN / NOT RUN
  runsAllowed = 1, runsPerformed = 0, result path empty
  successor chain measured, four records left byte-unchanged
  next = walk steps 1-4, then one production, only with time to seal the result
```

No image was produced and no production was dispatched. No wallet seed, model API key or node
secret is named in or staged into the guest. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and `activationAllowed=false` are unchanged. No
serving claim, no public mining and no paid-API benchmark is made anywhere in this section.

## 34. What the launcher would need, and what it would cost to get there (2026-08-27)

Section 33 froze what a successor image would be judged against. This section answers the question
that sits underneath it: what would have to be *in* such an image for the launcher to reach serving,
and is that material obtainable at all. The answer was measured against the tree, not recalled, and
one earlier reading of it was wrong in a way worth keeping on the record.

`native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json` holds the
measurement. It dispatched no production, performed no boot, and fetched nothing.

### 34.1 The launcher's startup chain, read out of the binary

The launcher's `main` runs nine stages in a fixed order, each handing its result to the next. The
seventh opens the runtime rootfs read-only and compares the whole tree against a frozen content
manifest whose digest is compiled into the binary. Serving is the ninth. A guest that fails the
seventh therefore never reaches the ninth — the launcher does not start degraded, it does not start.

Two things must exist at fixed absolute paths inside the guest for that seventh stage to pass:

| Guest path | What it is | Constraint |
| --- | --- | --- |
| `/var/lib/boole/native-shadow/runtime-rootfs` | The read-only tree each checker execution is confined to | Must be a read-only mount; a writable one is refused even if every byte is right |
| `/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json` | The byte-level description of that tree | Exactly 1,285,116 bytes at the arm64 digest the binary compiles against |

Neither path is a configurable value. The binary reads no environment variable, so there is no
setting that could relocate them and no way to satisfy the stage other than by putting them there.

### 34.2 The gap, stated as a property of the builder

The boot image builder mentions neither path. It mentions their shared parent directory zero times
as well. That is the whole gap, and it is a fact about a tracked file rather than a prediction about
a run: the image the builder produces cannot contain what the launcher requires, so a launcher
started inside it fails at the seventh stage.

This is why MAC.3 is not closed by the successor boot passing. Booting and serving are two claims,
and only the first has been established.

### 34.3 The material, counted rather than assumed

A build needs two locked input sets. Both were resolved artifact by artifact against the local
content-addressed cache, by digest lookup, fetching nothing and re-hashing nothing.

| Lock | Artifacts required | Present locally | Absent |
| --- | --- | --- | --- |
| Boot rootfs (the guest's own userland, including the three Rust distribution archives) | 197 | 197 | 0 |
| Runtime rootfs (the tree the launcher verifies) | 62 | 62 | 0 |

Nothing is missing. The counts about the local cache are recorded as local observations and marked
as not reproducible on a clean runner, because the cache is untracked; the tests bound to this record
check that they are *labelled* that way rather than trying to reproduce them. The locks themselves
remain the authority, and their digests are re-derived by those tests.

### 34.4 An earlier reading that was wrong, kept rather than dropped

Before the locks were resolved, the runtime rootfs was read as blocked: the arm64 layer the
expectation names is 766,556,160 bytes and none of its four content digests is present locally, which
was taken to mean a new external acquisition was required, and therefore a stop condition.

That was wrong, and the record says so in its own field. The absent object was searched for as an
input when it is an output. Its digests are missing locally because nothing has built it here, not
because its sources are missing — and every source is present, as the table above shows. The layer
is rebuilt from those locked inputs on **every pull request**, by the `native-shadow-rootfs-replay-linux-arm64`
job on a Linux arm64 runner, and compared against the sealed expectation. Reproducibility of that
layer is therefore already demonstrated continuously.

What the correction would have cost, had it not been caught: stopping a line of work that is not
blocked, and asking the operator to approve a download that nothing requires. It was caught by
resolving the lock's artifact list against the cache instead of looking for the finished layer.

What this still does not establish is recorded alongside it — in particular that the rebuild could
run on the developer Mac. The job that is known to work runs on a Linux arm64 runner as root, and
the Mac is neither. Portability of the build host is an open question; reproducibility of the layer
is not.

### 34.5 What remains, and why it was not started here

| Step | What | One-shot |
| --- | --- | --- |
| 1 | Extend the four sealed staging records so the boot lock covers the runtime rootfs material | no |
| 2 | Teach the image builder to place both paths, the tree read-only | no |
| 3 | Produce one image pair under the criteria frozen in section 33 | yes |
| 4 | Boot it once against the ten MAC.3 runtime conditions | yes |

Steps three and four are each allowed exactly once and each must be sealed after it runs. Steps one
and two are a single unit whose review cycle is measured in tens of minutes per change. Starting the
chain with too little time left risks reaching a one-shot step that cannot be sealed before the
session ends, which is the one outcome that cannot be undone. The material being complete removes
the question of whether this is possible; it does not by itself make the remaining time sufficient.

### 34.6 Execution cursor

```
MAC.3 runtime serving gap = MEASURED / NOT CLOSED
  launcher needs two fixed guest paths; builder mentions them 0 times
  material complete: boot 197/197, runtime 62/62, absent 0
  derived layer rebuilt every PR on Linux arm64 -- not an acquisition
  next = steps 1-2 (chain + builder), then one production, then one boot
```

Forty-seven tests bind the record, and three deliberate mutations — a drifted builder mention count,
a lock digest pointed away from the file it names, and a local observation relabelled as a repo fact
— were each caught, after which regenerating restored the identical digest.

No image was produced, no production was dispatched, no boot was performed and nothing was fetched.
No sealed record, image or prior boot result was modified. No wallet seed, model API key or node
secret was read, copied or staged anywhere. CURL.3 remains
`DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED`. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and `activationAllowed=false` are unchanged. No
serving claim, no public mining, no testnet and no paid-API benchmark is made anywhere in this
section.

## 35. Closing the serving gap: all three holes, and whether the big one fits (2026-08-27)

Section 34 measured one gap. Reading the sealed MAC.3 runtime contract back shows
three, and the launcher refuses at the earliest of them. That changes the
schedule, because exactly one image production is allowed.

### 35.1 Three gaps, not one

| # | What is missing | Where the launcher stops | Kind of change |
|---|---|---|---|
| 1 | the account database at `/etc/passwd` | stage 1, the service-identity prerequisite | rows in the lock and the staging table |
| 2 | the runtime rootfs and its content manifest | stage 7, `verify_runtime_rootfs_replay` | a second tree nested under a prefix |
| 3 | a channel the refusal can be read on | nowhere -- it hides the refusal | one edit to the launcher unit |

The account database is missing for a structural reason rather than an oversight:
`systemd-sysusers` would create those accounts at boot and cannot, because the
root is read-only. So an image that closes only gap 2 still refuses at stage 1,
and the single allowed production would have answered nothing. The three are one
unit of work.

### 35.2 The one that is a decision rather than a table edit

Gaps 1 and 3 are edits. Gap 2 is not: the launcher requires an actual tree at a
compiled-in path, and it verifies that tree against a manifest whose digest it
compiles against.

Three checks in the verifier decide what that costs. The directory must be on a
read-only mount. The manifest must be a regular file, root-owned, mode 0444,
with exactly one link and exactly the sealed size. And the observed path set must
equal the manifest's exactly -- every path present must be named, every path
named must be present, kind included.

That last one rules out the cheap answer. Pointing the nested path at the copy
the boot image already carries at top level would cost almost no bytes, and a
symlink where the manifest says file is a mismatch. The tree has to be
duplicated, so the question becomes a byte question.

### 35.3 Whether it fits

Both locks carry the same build recipe, and the recipe is enforced rather than
advisory -- the builder raises rather than truncates.

| Quantity | Bytes |
|---|---|
| `maxTotalBytes`, sealed in both locks | 2,147,483,648 |
| current boot initrd, built uncompressed from the single input layer | 1,010,933,296 |
| runtime rootfs layer to be nested | 766,556,160 |
| upper-bound sum | 1,777,489,456 |
| headroom | 369,994,192 |

This is an upper bound, not a result. Both sizes are archives and carry headers
and padding the assembled payload does not, so the real sum is smaller. What it
establishes is that the pinned numbers leave room; what it does not establish is
that the assembled result fits, and it says nothing at all about the entry limit,
because neither tree's entry count is pinned anywhere in the repository and
neither can be counted without a build.

### 35.4 Why the nested tree is not new material

The boot rootfs and the runtime rootfs are not different kinds of object. One
builder digest is named by both locks, both carry the same schema and the same
recipe, and the boot closure roots are a strict superset of the runtime ones. So
the nested tree is content the boot image already carries, placed a second time
under a prefix.

Worth recording precisely, because the first reading got it wrong: the digest the
locks name belongs to the arm64 *projection*, a small module that reads the
frozen legacy builder, refuses unless those bytes match the digest it pins,
applies a fixed list of replacements and executes the result. The limit checks
live in the legacy module and are reached only through a projection that has
already verified it. Two files, one of which checks the other.

### 35.5 The staging path is not masked

A read-only path is worth nothing if something mounts over it at boot. Every
writable path in the sealed contract is under `/run` on tmpfs, and the tmpfiles
configuration creates directories under `/var/lib/boole` rather than mounting
anything there. So both fixed guest paths stay visible.

### 35.6 What is still held

The held condition -- that the launcher itself run under an unprivileged account
-- is still awaiting an operator decision, still not relaxed and still not
satisfied. A boot judged against a condition under question could not return a
clean verdict, which is a second reason the one allowed production is not spent
yet. The reformulation offered in the contract stays offered rather than applied.

### 35.7 Cursor

```
MAC.3 serving gap closure = PLANNED / NOT IMPLEMENTED
  three gaps, not one -- earliest refusal is the account database at stage 1
  gap 2 is the only design decision: duplication, not symlinks
  byte headroom 369,994,192 as an upper bound; entry count unknown
  nothing built, staged, produced or booted; held condition unrelaxed
```

No image was produced, no production was dispatched and no boot was performed.
Serving is not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD,
Base activation false and activationAllowed=false are unchanged. No public
mining, no leaderboard claim and no paid-API benchmark is made anywhere in this
section.

## 36. The fourth condition, corrected rather than relaxed (2026-08-28)

The fourth MAC.3 condition was the one thing this work could not decide for
itself. It read "the launcher runs under an unprivileged account", and nothing in
the tree satisfied it, because the launcher is root by design. Section 35 carried
it forward unrelaxed and stopped. The operator has now decided it, choosing the
correction over the redesign.

### 36.1 What was wrong with the sentence

The subject. The launcher is the privilege manager: it verifies the frozen
capability mask against its own kernel status before it binds a socket, it
materializes the authority files, and it is what performs the descent to the
unprivileged accounts. The processes that must be unprivileged are the ones it
creates to run the checker and the submitted answer -- not the manager itself.
Under the original wording, satisfying the condition would have meant deleting
the delegation the containment is built on.

The sealed contract had already noticed this and offered the reformulation in its
`readingOffered` field, then declined to apply it, on the grounds that applying a
reading the operator had not given would decide the question the record was
holding open. The decision applies that reading.

### 36.2 The corrected condition

| clause | what it requires |
| --- | --- |
| 1 | the launcher runs as root as the least-privilege supervisor that prepares the isolated environment |
| 2 | it holds exactly `CAP_SETGID`, `CAP_SETUID`, `CAP_SETPCAP`, `CAP_SYS_ADMIN` -- four, no more, no fewer |
| 3 | one missing or one additional capability refuses startup fail-closed |
| 4 | the ambient capability set is empty |
| 5 | the submitted answer and the checker start only after descent to a sealed unprivileged account |
| 6 | a failed descent, an answer running while still root, or any possibility of regaining privilege is refused |
| 7 | the root launcher is given no wallet, model API key, reward, block, consensus or peer-to-peer authority |

### 36.3 Why this is a correction and not a relaxation

The distinction is the whole weight of the decision, so it is tested rather than
asserted. Every one of the seven clauses is required to name code already in the
tree that enforces it, and the record stamps those files by digest and size so
the tests re-derive them rather than quoting them.

- Clauses 1 to 4 are the launcher's startup self-check. It reads its own kernel
  status, requires root in all four UID and all four GID slots, and compares the
  effective, permitted and bounding sets against a compile-time mask by
  **equality, not containment** -- which is what makes an extra capability a
  failure rather than a pass. Inheritable and ambient must be exactly zero. The
  same four names appear in the sealed execution policy and in the systemd unit,
  and a launcher test compares the compiled mask against the tracked policy so
  the two cannot drift apart quietly.
- Clause 5 is an ordering claim, so the order is read out of the source: the
  per-request setup runs drop-privileges, then verify-privileges, then the
  runtime identity lookup, and the exec is the last stage. Everything untrusted
  is on the far side of it.
- Clause 6 has three parts and all three are already closed. A failed stage
  propagates its error and returns before the exec rather than continuing past
  it. The post-descent verification re-reads the kernel and requires the real,
  effective and saved identities to be the target account, no supplementary
  groups, all five capability sets exactly empty, and `no_new_privs` set.
  Reacquisition is closed earlier still: the entire bounding set is dropped while
  `CAP_SETPCAP` is still held and **before** the identity changes, so there is
  nothing left to regain.
- Clause 7 holds because the inherited environment is cleared and per-request
  overrides are refused, the node and checker capability lists are empty, and the
  guest contract already freezes both that no host wallet, model key or node
  secret is in the guest and that the machine has no network device.

The signature of a misdescription rather than a weakened rule: under the original
wording nothing in the tree satisfied the condition; under the corrected wording
the same unchanged tree satisfies every clause. No check was removed. What would
have made it a relaxation is written down too -- permitting the checker or the
answer to execute while still root, accepting a capability set by containment
instead of equality, or letting a failed descent reach the exec. None of the
three is written, and all three remain refused.

### 36.4 Append-only, not an edit

The guest runtime contract is left exactly as sealed. It still carries the
condition at status `held` and the held block at relaxed false, waived false,
satisfied false, `readingApplied` false. The correction succeeds it in a separate
file rather than replacing it in place, so the state before the decision stays
readable afterwards, and docs-smoke pins both halves.

### 36.5 What this does not establish

Deciding a condition is not passing it. Nothing has been booted against the
corrected wording; the descent path is read here, not run. The three serving
gaps are untouched and still open -- the guest still has no account database,
which is why the launcher refuses at its first startup stage today, long before
the runtime rootfs check at stage seven. Production remains one dispatch,
unspent, and still gated on an entry-count check that no pinned number in the
tree answers.

### 36.6 Cursor

```
MAC.3 condition 4 = CORRECTED / PRE-REGISTERED / NOT IMPLEMENTED
  operator chose 1-a: correct the subject, keep the delegation
  seven clauses, each naming code that already refuses -- not one removes a check
  sealed contract unedited; it still reads held
  three serving gaps still open; nothing built, produced or booted
```

No image was produced, no production was dispatched and no boot was performed.
Serving is not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD,
Base activation false and activationAllowed=false are unchanged. No public
mining, no leaderboard claim and no paid-API benchmark is made anywhere in this
section.

## 37. The entry count, bounded — and a sentence of mine that was too strong (2026-08-28)

Production is gated on two budgets. The byte budget was answered when the closure
plan was sealed: the pinned sizes sum to 1,777,489,456 against a 2,147,483,648
limit. The entry budget was left open there, and the reason given was that no
entry count was pinned anywhere in the repository.

That reason was wrong, and it was wrong in my own record. It is corrected here
rather than fixed in place.

### 37.1 What the earlier sentence said

The closure plan's list of things it did not establish contains, verbatim:

> that the entry count fits, because neither tree's entry count is pinned
> anywhere in the repository and neither can be counted without a build

Section 36.5 of this document repeated it, ending on *"still gated on an
entry-count check that no pinned number in the tree answers."* Both stay as
written. Neither is edited.

### 37.2 Which half was overstated

The clause **"neither tree's entry count is pinned anywhere in the repository."**
One is. When the two boot root disk replicas failed byte identity, the
investigation had to walk both trees from the root inode in directory-block order
and compare every entry field by field. It could not do that without counting
them, and the count it reached is sealed in the hard-stop record: **13,448
entries** — 1,445 directories, 11,350 files, 653 symlinks, which sum exactly.

I wrote the earlier sentence after searching the *build inputs* for a pinned
count. There is none there, which is true and still true. I did not look at the
records describing *produced output*, which is where the number was.

### 37.3 What stays true

The rest of it. Neither tree's assembly-input entry count is pinned, and neither
can be counted exactly without a build. So what the pinned number buys is a
**bound**, not the count the builder's limit is actually applied to.

### 37.4 The bound

The nested arrangement is one runtime tree inside one boot tree. The runtime
closure is contained in the boot closure — this is recomputed from the two locks
rather than recalled: every one of the runtime lock's 3 closure roots is among
the boot lock's 5, and every one of its 62 artifact digests is among the boot
lock's 197, with nothing on the runtime side that is not also on the boot side.
The boot side adds the guest init, the launcher and the kernel; the runtime side
adds nothing of its own.

So a runtime tree cannot hold more entries than a boot tree does, and two
boot-sized trees bounds the pair:

| | entries |
|---|---|
| counted boot root disk | 13,448 |
| bound on the nested pair (×2) | 26,896 |
| sealed limit, both locks | 200,000 |
| headroom | 173,104 |

A margin of 7.44×. The bound is deliberately loose — the runtime side is three of
five roots and sixty-two of a hundred and ninety-seven artifacts, so the real
total sits well under it.

### 37.5 Why this is still a bound and not a count

The builder compares its limit against the entry set it has **assembled and is
about to write**, and raises `assembled rootfs exceeds entry limit` rather than
trimming the tree to fit — a truncating limit would have made this bound
worthless, since a tree that fits by being cut is not the tree that was staged.
The pinned number counts entries found in the image **after** it was written,
including the two the filesystem tool creates for itself. Close, but not the same
number, and the record does not claim they are.

What would make the bound wrong is entries that come from neither closure:
generated mount points, a second copy of a manifest, per-layer padding. The
margin is wide enough that none of these is likely to matter — which is a reason
to expect the pre-assembly check to pass, not a reason to skip it. It stays
required.

### 37.6 Was this a hard stop?

No. The over-statement was a negative claim inside a list of things the record
explicitly did *not* establish. Erring toward "this is not known" left every gate
in place; correcting it only widens what is known. Nothing was skipped, relaxed
or passed because of it. It is recorded because a record that quietly improves
itself is worth less than one that shows where it was wrong.

### 37.7 Cursor

```
nested entry budget = BOUNDED / NOT MEASURED
  bound 26,896 <= limit 200,000, headroom 173,104, margin 7.44x
  rests on: 13,448 counted + runtime closure contained in boot closure
  bytes 1,777,489,456 <= 2,147,483,648 unchanged from the closure plan
  both pre-production budgets now have bounds under their limits
  pre-assembly entry check still required; closure plan unedited
```

No tree was assembled, no image was produced, no production was dispatched and no
boot was performed. Serving is not claimed. mineable_now=0, REWARD_READY=0,
RP0-MD=HOLD, BF.7=HOLD, Base activation false and activationAllowed=false are
unchanged. No public mining, no leaderboard claim and no paid-API benchmark is
made anywhere in this section.

## 38. The descent, fixed as a source contract — and the half that stays unmeasured (2026-08-28)

The corrected fourth condition has two clauses about the descent: the submitted
answer and its checker start only after the launcher has dropped to the sealed
unprivileged identity, and a failed descent, a root-state execution or a chance of
regaining privilege is refused. Section 36 recorded both as already enforced and
named one source file for each. Checking that turned up a real gap, and closing
it turned out to be blocked by a seal. This section is what could honestly be done
instead, and what it does not buy.

### 38.1 The gap

The function both clauses point at — the one that reads the kernel's own answer
after the descent and refuses when the answer is wrong — has one call site and no
test. Its module compiles only on Linux, so it never built on the machine this
work is done on. Its counterpart on the launcher side avoids exactly this by
splitting the decision out as a pure function that compiles under test on every
platform; the descent side never was.

### 38.2 What was tried, and why it was reverted

The decision half was extracted the same way, given five table-driven tests, and
checked by weakening the checks six different ways: every weakening was caught.
The Linux-only module cross-compiled and passed clippy with warnings denied for
the Linux target. It worked. It was still reverted.

The launcher build seal is a chain of three links. The sealed build result pins
the produced binary's digest and the digest of the build authority; the build
authority pins the digest of all 33 launcher source files; and the image producer
acquires the launcher by rebuilding it and requiring the bytes to equal the seal.
So a change to any launcher source file, test code included, is refused at the
first link. That is not a tooling accident — it is the point of the chain. Two
pinned files had changed, so the work was reverted to the sealed bytes and kept
only as prose. Being correct and passing is not enough to spend a seal that the
next step depends on.

### 38.3 What was added instead

A source contract, in `scripts/`, which the build authority does not pin, so it
costs no seal. It reads the sealed source and requires thirteen conditions to
hold: the source is byte-for-byte the sealed source; a root uid is refused; a root
gid is refused independently; the real, effective and saved identities are
compared directly and refused when any slot differs; a retained supplementary
group is refused; all five kernel capability sets are required to be exactly
empty; no_new_privs is required to be set; a missing, duplicated, empty or
unparsable status field refuses rather than reading as a satisfied check; every
refusal sits after the decision that makes it one; the refusal counts are exact,
so nothing was deleted and no unaccounted early exit was added; there is exactly
one success path; the descent is dropped, then verified, before the exec, with a
failed stage propagating; and the shipped code has exactly one child creation and
exactly one exec of the answer with none of the ordinary spawn shortcuts, so
there is no road to a submitted answer that the verification does not guard.

A gate that only reads for text can rot into a gate that reads for nothing. So
the gate carries seventeen weakened variants of the sealed source, held in memory
and never written to disk. Each deletes one condition or inverts one order, and
each must be caught by the condition it weakens. A condition that stopped
checking anything would let its own variant through and fail the gate. Nine of
the seventeen are caught by two or three conditions independently.

### 38.4 Two kinds of evidence, kept apart

Reading source and running a kernel answer different questions, and a green
static gate must not later be read as a descent that was observed.

What has run on a real kernel is the launcher-side capability policy: a
three-case fault-injection matrix under systemd on ubuntu-24.04, in CI, where the
exact sealed set passes and a missing or an extra capability is refused through
the production verifier with its exact mask.

What has not run on a real kernel is the checker-side descent verification. Not
its five failure paths, which were never fault-injected anywhere, and not its
normal path either: the function runs only inside the contained child that
per-request execution creates, the launcher crate's six real-kernel gated tests
are all in other modules, and the one real guest boot refused before serving
because the guest has no account database. An earlier reading of this work
assumed the normal path had already run in an existing end-to-end run. It has
not. The weaker true claim is recorded rather than the stronger convenient one.

### 38.5 Why the stronger test needs a successor chain, not a re-seal

The image about to be produced contains the launcher as it is sealed now. Adding
tests to launcher source and re-sealing afterwards would leave the image and the
seal describing different binaries; re-sealing before production would change the
artifact production is meant to reproduce. Either way the test would not be
evidence about the binary in the image.

The tests belong to a separate launcher v2 chain, walked from the start: a new
source authority pinning every launcher file including the new tests, one double
arm64 build to seal the new binary against it, an image produced from that
binary, and boot criteria written for that image before the run that judges it.
That chain would move the drop failure matrix from not measured to measured, for
the v2 binary and no other. It cannot be used retroactively as evidence for the
current image.

### 38.6 What this does not establish

Not a behavioural test: no synthetic kernel status is fed to the parser, so how it
answers a forged or adversarial status is still unobserved. Not that the refusals
are correct — only that they are present, ordered, counted and reached before the
single exec. Not that the descent was observed on a booted guest; it is read here,
never run. Not that the syscall half is exercised. Not that the launcher reaches
serving: the three sealed runtime gaps are untouched and still open. Not that the
corrected fourth condition is satisfied; it remains pre-registered and unbuilt.

### 38.7 Cursor

```
descent contract = STATIC-SOURCE-CONTRACT-GREEN
  UNIT-LEVEL-DROP-FAILURE-MATRIX = NOT-MEASURED
  13 conditions over the sealed source, 17 weakening variants, each caught
  launcher source unchanged: linux.rs and privilege.rs at their sealed digests
  launcher not re-sealed; no re-seal scheduled by this section
  real-kernel evidence: launcher-side capability matrix only
  checker-side descent: normal path and 5 failure paths both unobserved
  next: account DB, runtime rootfs + manifest, refusal output to host console
```

No image was produced, no production was dispatched and no boot was performed. No
launcher source file, builder, source lock or systemd unit was modified. Serving
is not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base
activation false and activationAllowed=false are unchanged. No public mining, no
leaderboard claim and no paid-API benchmark is made anywhere in this section.

## 39. The three gaps become one plan, and the first step of it is written down (2026-08-28)

The one real guest boot refused before it served anything, and the measurement of
that refusal named three gaps: the guest has no account database, so the launcher
refuses at its first check; the runtime rootfs and the content manifest the
launcher verifies before serving are not in the image; and the launcher's refusal
goes only to the journal, which is why the first boot showed nothing about why it
stopped. Closing them takes four steps in a fixed order, because each step is the
input the next one checks against. This section is the first of the four. It names
files. It generates no lock, changes no builder and produces no image.

### 39.1 What the successor names

The boot source lock plan is superseded by a second plan, written as a new file
next to the first. The predecessor keeps its bytes; four authority records name it
at its sealed digest and none of them is touched.

The tracked source list goes from ten files to fifteen. Five are added — the
passwd, group, shadow, gshadow and name-service switch files, all frozen in an
earlier record and already in the tree. Two are replaced by successor files rather
than edited, so the count of ten becomes fifteen with eight rows carried through
unchanged. Each of the fifteen keeps the identity the predecessor gave it, so the
two replacements inherit the names their predecessors had rather than inventing
new ones.

### 39.2 The account database, checked against the launcher's own eight clauses

The launcher resolves each of its two fixed accounts and refuses unless eight
things are true at once: the passwd name is exactly the compiled-in name; the user
number is not root's and the group number is not root's; the home directory is the
non-existent one; the shell is one of the two that refuse to run anything; a group
of the same name exists with the same number; looking that number back up returns
that same group; the account's group list is exactly its own primary group and
nothing else; and the two accounts share neither number.

The plan does not assert this. The gate that accompanies it re-derives all eight
from the bytes of the two files themselves, by parsing them the way the C library
would. The two accounts are numbered 990 and 991, both non-root, both with the
non-existent home, one refusing through nologin and the other through false,
each with a same-named group at its own number, and neither listed as a member of
any group — including its own, which is how a primary-only group list is written
in this format.

### 39.3 The two files that are replaced rather than edited

The launcher unit gains the console alongside the journal on both of its output
streams. Two lines change; every other line, the capability set and the account
the unit runs as included, is byte-identical. The console is a channel the host
already captures, so this adds no device, no network and no shared directory.

The runtime directory rules lose three of their five lines. Those three ask for
directories under a path on a filesystem mounted read-only. They could never have
succeeded. Removing them makes "every write lands on a memory-backed filesystem"
true by construction rather than true because the failures happened to be
harmless. The two that remain are both under the run directory, which is
memory-backed, and the launcher's lock and socket live there and nowhere else.

Both predecessors stay in the tree at their sealed digests. Succession here is by
new file, never by edit.

### 39.4 The nested runtime tree, and the manifest that is derived rather than committed

The launcher verifies a second, complete rootfs nested inside the boot one, and
before it will serve anything it reads that tree's content manifest and requires
its digest, its byte count and its schema to equal three values compiled into the
sealed binary. The plan declares that tree: where it sits, which lock drives its
assembly, and that it is declared and not assembled.

Which lock drives it is not a detail. The manifest records, for every entry, the
closure names that entry came from. Assembled from the boot lock those names would
be the boot lock's five, and the document would not match the digest the launcher
compiles against. So the nested assembly is driven by the runtime lock, whose
sixty-two artifacts the boot lock already carries in full.

The closure plan expected one tracked-source row for the manifest. This plan
refines that and says so. A tracked row would mean committing 1,285,116 bytes of
build output as though they were a source, when the builder already emits that
document from the entries it assembled. Deriving it and requiring the derived
digest to equal the value the launcher compiles against is the same check with one
fewer copy of the same bytes in the repository.

### 39.5 A sealed contract that disagrees with the image that booted

The launcher opens the nested tree and refuses unless the filesystem under it
reports itself read-only. Whether that check can pass depends on what is mounted
where, and the guest-init contract sealed earlier says the path the tree sits
under is a memory-backed filesystem mounted at every boot. If that were true, a
tree baked there would be hidden the moment the guest started, and the read-only
check would fail on a writable mount.

It is not true of the image that booted. The mount table was decoded out of the
shipped systemd library and the seven mount units in the image were read directly;
no fstab is present, so nothing generates more. Five top-level paths are mounted,
and the one in question is not among them, nor is anything below it. A tree staged
there is neither masked nor made writable, and the read-only requirement is
answered by the root disk's own mount rather than by a mount added for it.

The contract is not edited. Four authority records name it at its sealed digest.
The plan records the audited image as the fact, the contract clause as the stale
half, and the disagreement as a disagreement. This was not a hard stop: no pass
condition moved, and the direction of the correction removes a mount rather than
adding one.

### 39.6 The two budgets are still bounds

Both pre-production budgets were answered earlier — the byte total against the
sealed limit with room left over, and the entry total as an upper bound at roughly
a seventh of its limit. The plan carries both and labels each as what it is: a
bound read back from a sealed record, not a count of an assembled tree. Neither is
evidence that the tree fits. The measurement that decides is taken immediately
before assembly, and the plan says so rather than letting a comfortable margin
retire the check.

### 39.7 What this does not establish

Not that any gap is closed: no lock was generated and no tree was assembled. Not
that the successor lock will validate, since the generator has not been changed to
emit it. Not that the builder accepts the five new sources — its staging table
still names four. Not that the nested tree fits, since only bounds were compared.
Not that the derived manifest reproduces the digest the launcher compiles against.
Not that a produced image would pass, which the sealed production criteria are
what would judge. Not that serving is reachable, which only a boot would show.

### 39.8 Cursor

```
boot source lock plan = BOOT-ROOTFS-SOURCE-LOCK-PLAN-SUCCESSOR-FROZEN-LOCK-NOT-GENERATED
  successor chain step 1 of 4; steps 2-4 not started
  tracked sources 10 -> 15: 5 added, 2 superseded by new file, 8 carried
  account database: all 8 identity clauses re-derived from the bytes
  nested runtime tree: declared, not assembled; manifest derived, not tracked
  guest-init contract clause on the writable mount: contradicted, not edited
  both budgets remain bounds; remeasure immediately before assembly
  next: the lock generator, then the lock successor, then the staging table
```

No image was produced, no production was dispatched and no boot was performed. No
launcher source file, builder, source lock or generator was modified, and the
launcher seal is unmoved. Serving is not claimed. mineable_now=0, REWARD_READY=0,
RP0-MD=HOLD, BF.7=HOLD, Base activation false and activationAllowed=false are
unchanged. No public mining, no leaderboard claim and no paid-API benchmark is
made anywhere in this section.

## 40. The lock generator, and the sealed contract that has to refuse it (2026-08-28)

The first step named fifteen files. This section is the second: the tool that
knows how to build a successor lock out of them and how to refuse one that is
wrong. It deliberately seals nothing. The successor lock and its result document
are still absent when this step finishes, and the tool refuses to check them with
a message that hands sealing to the third step by name. That refusal is pinned in
the documentation gate, so a later step that wants to seal has to supersede the
pin on the record rather than let it lapse.

### 40.1 The successor keeps the shape thirty consumers already read

The sealed guest-init contract opens a source lock and requires its top-level keys
to be exactly twelve, no more. About thirty files read that lock. So the successor
carries the same twelve keys, and the assembly that produces them is the
predecessor's own function, called with the successor plan rather than
reimplemented beside it.

That is also why the nested runtime tree is not a key of the lock. It is declared
in the plan, checked by this tool against the two places its digest is already
sealed, and recorded in the result the tool emits — never smuggled into a document
whose shape is frozen.

### 40.2 The predecessor's grounds are run, not restated

Eight of the acceptance grounds the first lock had to pass — identity, build
recipe, repository, ordering, package closure, seeds, tracked files, authority
bindings — are imported from the predecessor generator and executed. None is
retyped. Restating a ground in new words is exactly how one gets quietly weaker,
and there is no wording here to weaken. On top of those, this tool adds its own:
the release string, the derived entries read from the contract's own requirement
list, one binding per tracked file and no more, the two recorded replacements, all
eight account clauses, the nested tree, and the audit below.

### 40.3 The sealed contract has to refuse the successor, and that is correct

The contract pins the digest of both files the successor replaces. It compares
each tracked file against its pin and raises on a difference. So it cannot accept
the successor lock: the successor's whole purpose is that those two digests moved.

There were three ways out and two of them were wrong. Editing the contract would
move a seal four authority records name. Relaxing the digest comparison would be
loosening a pass condition, which is a stop condition, not an engineering choice.

### 40.4 The shadow lock

The third way is to ask the contract a question it can answer. Alongside the real
successor lock the tool builds a shadow of it — never written to disk — in which
the two replaced sources are restored to their sealed predecessors and everything
else is left exactly as the successor has it. Before the shadow is handed over,
the tool requires that the only rows differing between the successor and its
shadow are precisely the two the plan records as replacements; an unrecorded third
move survives into the shadow, and the contract then refuses it by name.

The contract's verdict on the shadow must equal the predecessor's sealed verdict:
the same status, and the same single unmet requirement — the launcher binary,
which is a build output and is deferred on purpose. That is a proof, through the
contract's own public entry point with no pass condition touched, that the
successor is additive everywhere except the two replacements it records.

The successor's own requirement list is meanwhile strictly stricter than the
contract's: every one of the contract's rows is kept, the two replaced rows are
given concrete successor digests instead of inherited ones, five account rows are
added, and only the launcher binary is left open.

### 40.5 What the tool checks that the plan could only assert

The account database is re-parsed from the bytes of the passwd and group files and
all eight launcher clauses are answered again here, in the tool, rather than read
back from the plan. The two password-bearing files are required to be root-only.

The nested tree is checked against the launcher binary's compiled-in constants —
and that check is sharper than it looks, because each of those three constants is
declared twice in the launcher source, once for this architecture and once for
everything else, with different values. The tool reads the architecture-gated
declaration specifically. The same digest is then checked against the sealed
replay expectation, and the lock that drives the assembly is required to be the
sealed runtime lock, at its sealed digest, with its sixty-two artifacts and three
closure roots.

Seventy-three tests cover this, twenty-two of them mutations that feed the tool a
deliberately broken input and require one particular refusal, named by its own
words. The naming is the point. A test that only checks that something was refused
passes just as happily when the broken input trips over an unrelated check on the
way, and one of these did exactly that until it was corrected.

### 40.6 What this does not establish

Not that the successor lock is sealed — it is not written, and writing it is the
third step. Not that the builder accepts the fifteen sources; its staging table
still names four. Not that the nested tree was assembled, or that the derived
manifest reproduces the digest the launcher compiles against, since nothing was
assembled. Not that the launcher binary requirement is met; it is deferred and the
verdict says so. Not that any of the three gaps is closed at runtime, which only a
boot would show.

### 40.7 Cursor

```
boot source lock generator = SUCCESSOR-LOCK-GENERATOR-WRITTEN-NOTHING-SEALED
  successor chain step 2 of 4; steps 3-4 not started
  lock shape unchanged: the same 12 keys the frozen contract requires
  predecessor grounds imported and executed, not restated
  frozen contract refuses the successor by design; answered by a shadow lock
  shadow verdict must equal the predecessor's, missing role: launcher binary only
  account clauses re-derived in the tool; nested tree checked against arm64 pins
  next: the lock successor, then the builder's staging table
```

No document was sealed, no image was produced, no production was dispatched and no
boot was performed. No launcher source file, builder, existing generator or
existing source lock was modified, and the launcher seal is unmoved. Serving is
not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base
activation false and activationAllowed=false are unchanged. No public mining, no
leaderboard claim and no paid-API benchmark is made anywhere in this section.
