# Mac-first hidden Linux execution plan v1

> **CURRENT-PROCESS-POLICY-V1 (2026-08-31):** Future engineering work under
> this plan follows
> `docs/development-throughput-and-evidence-policy-v1.md`. The historical
> one-shot production/boot records below remain exact accounts of past runs,
> but they are not the default procedure for new reversible closed-local
> builds or boots. This correction creates no A7, production run, boot or
> activation authority; code that still enforces the historical chain must be
> changed by normal TDD before it is used.

> **CURRENT-CURSOR-2026-09-01:** `ARM64 REPLICAS BYTE-IDENTICAL / MAC CLOSED READINESS PASS /
> MAC.4 VSOCK ROUND TRIP FAILED CLOSED / SUFFICIENT ROOT CAUSE IDENTIFIED / A7 NOT CREATED`.
> The next coherent development milestone is deterministic guest module indexes
> and an explicit vsock load contract, followed by a free readback/preflight.
> Another image pair or Mac boot needs new authority. No v5 production, testnet,
> mining, reward, consensus, P2P or activation run is authorized by this cursor.

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

## 41. The successor lock is sealed, and the contract still refuses it (2026-08-28)

The third of the four ordered steps. The second step built a tool and wrote
nothing; this one runs it once and seals what it produces. Two documents now exist
that did not before: the successor source lock, 359,099 bytes, and the result
that records how it was checked, 3,506 bytes. Nothing else changed. The tool was
run, not edited — its digest is pinned in the gate and recorded inside the sealed
result, so an edit to it would move both and fail here rather than quietly
reinterpreting bytes that are already sealed.

### 41.1 The supersession is on the record, not in a diff

The second step's gate required both documents to be absent, and said in its own
text that the right move when the third step ran was a step-three gate carrying
the sealed digests, not a quiet relaxation. That is what happened. Three tests
that asserted absence now assert the sealed state; the digests they were sealed at
live in the new gate; and the second step's file carries a dated note saying it
was superseded and by what.

One of the three did not become weaker. The tool's refusal — the one that names
sealing as the third step — is still reachable, and the superseding test proves
it: it removes the sealed lock, requires the tool to refuse with that same
message, and puts the bytes back. Sealing removed the absence, not the refusal.

### 41.2 What the seal is a claim about

Not that one run happened to produce these bytes. The gate rebuilds both documents
from the same inputs and requires them to equal the sealed bytes exactly, so the
seal is a claim about the inputs: change any tracked source, any account file, the
plan, or the tool, and the regenerated bytes move away from the pinned digests and
the gate fails. Both documents are also required to be canonical under the same
serializer every consumer will re-hash with, so the digest a reader computes is
the digest that was pinned.

### 41.3 The contract refuses the sealed lock, and that is a pin

Sealing bought the successor nothing from the frozen guest-init contract. It pins
the digest of both files the successor replaces, so it refuses the sealed lock for
exactly the reason it refused the unsealed one, and the gate asserts that refusal
rather than hoping for it. What answers for the rest is still the shadow: the
successor with those two sources restored to their sealed predecessors, which the
contract judges to be what it judged the predecessor to be, down to the same
single unmet requirement.

Both digests are pinned on both sides now — the successor value in the sealed
lock, the predecessor value in the contract — so a later edit that made the two
agree would have to move a pin in plain sight.

### 41.4 What the sealed documents say

Fifteen tracked sources with fifteen authority bindings, one per file. The five
account files are sealed as tracked sources, with the two password-bearing ones at
root-only permissions. The two replaced files carry their successor digests and
their successor paths while keeping the guest placement — mode, owner, group —
their predecessors had, so succession replaced the bytes and not where they land.
Both predecessors are still in the tree at their sealed digests.

The result records the status as sealed with the launcher binary deferred and no
boot authority claimed; one deferred requirement, the launcher binary, which is a
build output sealed by a separate authority; the nested runtime tree still
declared and not assembled, at the content-manifest digest the launcher compiles
against; and eight boundary flags, all false.

### 41.5 What this does not establish

Not that the guest-init contract accepts the successor — it does not, by design.
Not that the builder can stage what the lock names: its table still holds four
files, and the gate checks that it holds exactly those four and neither successor
file nor any account file. Not that the nested tree was assembled or that the
derived manifest reproduces the digest it is checked against. Not that the
launcher binary requirement is met. Not that an image would build, produce or boot.
Not that serving is reachable.

### 41.6 Cursor

```
boot source lock successor = BOOT-ROOTFS-SOURCE-LOCK-SUCCESSOR-SEALED-LAUNCHER-BINARY-DEFERRED-NOT-BOOT-AUTHORITY
  successor chain step 3 of 4; step 4 not started
  successor lock sealed: 359,099 bytes; result sealed: 3,506 bytes
  generator run, not edited; its digest pinned in the gate and in the result
  regeneration must reproduce the sealed bytes, so the seal is about the inputs
  frozen contract still refuses the sealed lock; the shadow still answers the rest
  builder staging table still names four; nested tree still declared, not assembled
  next: the builder's staging table, then remeasure immediately before assembly
```

No image was produced, no production was dispatched and no boot was performed. No
launcher source file, builder, staging table, existing generator or existing
source lock was modified, and the launcher seal is unmoved. Serving is not
claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation
false and activationAllowed=false are unchanged. No public mining, no leaderboard
claim and no paid-API benchmark is made anywhere in this section.

## 42. A builder that can read the sealed lock (2026-08-28)

The last of the four ordered steps. The lock has named fifteen files since the
third step sealed it; nothing could read it, because the builder decides which
sources may appear in a lock by comparing the lock against a table, exactly, in
both directions, and that table named ten. This step widens the table to fifteen.

It widens it by projecting the builder again rather than by editing it. The
predecessor keeps its bytes and its ten rows, so the lock it was written for still
validates against it; the successor executes the same derived source a second time
with the wider table bound into it. That is the relationship the predecessor
already has to the frozen arm64 builder, and the arm64 builder to the frozen
original — a fourth layer, not a new kind of thing.

### 42.1 It took two modules, and the reason is worth stating

The plan named one. What the work found is that a sealed lock does not reach a
builder directly: it passes through the release gate that turns a source lock into
a build input, and that gate accepts exactly one release string. So the sealed
successor was refused there, before the widened table was ever reached, with a
refusal about identity rather than about the table.

Moving that one string is the second module. Widening which lock is accepted is
not accepting both: the successor gate refuses the predecessor release exactly as
the predecessor gate refuses the successor release, and the gate asserts both
directions. Everything else about that gate is unchanged — the lock must still be
canonical exact, must still declare `activationAllowed: false`, and must still
carry the two tool roles.

### 42.2 What the widened table holds

Fifteen tracked sources against ten. Five are the account files the guest's own
identity contract reads — `passwd`, `shadow`, `group`, `gshadow` and
`nsswitch.conf`. Two are sources the successor lock supersedes, and both keep the
guest path their predecessors were given, because only the bytes moved. The
remaining eight are carried forward untouched, and the gate checks that they are
byte-identical entries rather than merely present.

### 42.3 The namespace is rebuilt, not copied

This is the part that would have been wrong the obvious way. The functions inside
a projected namespace read the table from the globals they were compiled with, so
rebinding the name in a copy of that namespace leaves every function still reading
the predecessor's ten while the module's own attribute reports fifteen. The
derived source is therefore executed a second time and the wider tables are bound
into the fresh namespace, which costs about a hundredth of a second.

Rebuilding a namespace means reproducing everything the predecessor put in it.
Three names are recomputed from the wider tables; nine are reused by identity. The
gate parses the predecessor's own source for the names it injects and requires
that set to equal the twelve, so a tenth injection added there fails the test here
rather than going unnoticed until something reads a name that is missing.

### 42.4 The evidence is end-to-end, and cost nothing to take

The sealed lock is run through the real production path — release gate, then
normalization, then validation — with no artifact store, so no package is hashed
and nothing is downloaded. Two refusals answer the question:

| builder | refusal |
| --- | --- |
| predecessor | `authority binding identity/source set differs` |
| successor | `complete source lock needs an artifact store` |

The second is the whole result. It is raised after the binding-identity and
tracked-path comparisons have both passed, so reaching it means the source shape
of the sealed lock is accepted in full; what stops the run is the absence of a
package store, which is a build input and not a contract. The first shows the
predecessor still refuses, so the widening is real rather than a rename.

### 42.5 The nested tree is declared, and deliberately not merged

The successor lock declares a second rootfs staged under
`/var/lib/boole/native-shadow/runtime-rootfs`, with its content manifest beside
the tree rather than inside it, read-only, at the digest the launcher verifies
against. This step provides the two functions that would place it: one derives the
manifest, one re-roots an assembled tree under the prefix and puts the manifest
next to it.

The manifest is derived by the *runtime* builder rather than this one, and that is
not an implementation detail. The document records the closure each entry belongs
to; derived under the boot closures it would name five where the launcher expects
three, and could never equal the digest the launcher compiles against. Deriving it
by the builder that will be read is what makes the comparison meaningful.

What this step does not do is merge that tree into a build. The sealed plan
requires the assembled byte and entry totals to be measured rather than bounded,
and requires that measurement immediately before assembly, which is a production
step this chain has not opened. A test asserts the not-merged state so it cannot
be read as done.

### 42.6 The third step's gate is superseded on its own terms

Step three's gate asserted that the fourth step had not run, and said in its own
text that the right move when it did was a step-four gate carrying the new table,
not a quiet relaxation. Its assertions are kept, under a name that now says what
they mean: the predecessor projection still holds its four rows with the
predecessor sources staged and no account file present, which is exactly why the
lock step three sealed goes on validating against it. A second test names where
the widened table went, and a dated note records the supersession.

### 42.7 What this does not establish

Not that a tree was assembled: none was. Not that the declared manifest digest was
reproduced from a real assembled tree — it is checked against the seal and the
replay expectation, and derived only from synthetic entries here. Not that the
totals are measured; they are still the bounds the plan sealed, and the plan's own
requirement to remeasure before assembly is unspent. Not that the launcher binary
requirement is met. Not that an image would build, produce or boot. Not that
serving is reachable.

### 42.8 Cursor

```
boot rootfs builder successor = BOOT-ROOTFS-BUILDER-STAGING-TABLE-ARM64-V2-SOURCE-SHAPE-GREEN-NOT-ASSEMBLED
  successor chain step 4 of 4; all four steps written
  staging table 10 -> 15 tracked files, by projection; predecessor bytes unmoved
  release gate projected too: one release string moved, both directions refused
  measured end to end with no artifact store: predecessor refuses the lock,
    successor reaches "complete source lock needs an artifact store"
  nested tree declared at the launcher's manifest digest; not merged into a build
  step three's absence test superseded on the record, not relaxed
  next: remeasure the assembled totals immediately before assembly, then open
    image production once
```

No image was produced, no production was dispatched and no boot was performed. No
package was downloaded or hashed. No launcher source file, launcher seal, frozen
builder, existing projection, existing generator or sealed source lock was
modified. Serving is not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD,
BF.7=HOLD, Base activation false and activationAllowed=false are unchanged. No
public mining, no leaderboard claim and no paid-API benchmark is made anywhere in
this section.

## 43. The assembled tree, measured rather than added up (2026-08-28)

The sealed plan said the totals had to be measured immediately before assembly,
and the fourth step left two numbers standing that looked like they could be
added: 13454 entries in the boot table, 4217 in the nested tree. Their sum is
17671. The assembled tree holds **17674**. Three directories exist in the result
that neither table listed, because assembling one tree inside another derives the
parents the merge needs. Adding the two numbers would have been wrong by three
entries and would have looked right.

That is the whole reason the plan asked for a measurement.

### 43.1 The merge went into a fifth projection, and had to

Editing the fourth step's module to add the merge would have falsified what that
step sealed: a constant saying the nested tree is not assembled, a test asserting
the not-merged state, and a section of its own gate written to say so. Changing a
sealed pass criterion to take a measurement is a stop condition, not a step. So
the merge lives in a successor projection, the fifth in the chain, and every
assertion the fourth step made stays literally true.

### 43.2 One function, not two that agree

The measurement and any future production call the same assembler. The merge is
threaded into the frozen builder's own assembly function, at the site the boot
projection already reserved — after the kernel mount points are merged, before
parent directories are derived. Three things follow from that site, and each is a
check that would otherwise have had to be re-implemented:

- the frozen merge refuses a path collision in its own words rather than
  overwriting, so a nested path landing on a boot path stops the run;
- parent derivation runs afterwards, so the nested tree's parents are derived by
  the builder rather than guessed by the caller — which is where the three extra
  directories come from;
- the entry and byte limit checks at the end of that function see the combined
  table, not either half.

Both the image-layout entry point and the measurement entry point reach that one
object. The gate proves it by replacing the assembler and watching both callers
arrive at the replacement, rather than by reading the two call sites and trusting
them to stay in step.

### 43.3 Two independent measurements of the same tree

The builder's entry table is one measurement. The other is a read-only walk of
the tree actually written to disk — `lstat` on every path, no `follow_symlinks`,
directories, regular files and symlinks counted separately. Eight keys are
compared, and all eight are identical:

| | value |
| --- | --- |
| entries | 17674 |
| directories / files / symlinks | 1736 / 15101 / 837 |
| payload bytes | 1771449867 |
| largest single file | 160096808 |
| largest file path | `opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly` |
| path collisions / duplicates / symlink escapes | 0 / 0 / 0 |
| byte-ordered path manifest | `a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736` |

Neither number comes from `du` or from an archive size. A third, throwaway
cross-check with `find` and `stat` returned the same count and the same byte
total, and is recorded here as a cross-check rather than as the result.

The nested tree's content manifest on disk is the digest the launcher compiles
against — `200f0257…`, 1285116 bytes — so the manifest is verified as a file in
the assembled tree rather than as a value carried in a table.

### 43.4 The macOS trap that would have looked like a real failure

The tree contains 20 sibling paths that differ only in case; the kernel headers
ship both `xt_mark.h` and `xt_MARK.h`. Default APFS folds case, so on an ordinary
directory the write would have collapsed those pairs, the walk would have counted
fewer files than the builder, and the run would have hit the "two measurements
disagree" stop condition — for an environment reason, with no defect anywhere in
the chain.

The tool now refuses to write into a case-folding directory before it writes
anything, by probing the destination. The real run was taken on a case-sensitive
APFS volume, and both spellings are present in the result. Case-folded siblings
are also counted on both sides, so a folding destination fails the agreement check
even if the probe were somehow satisfied.

### 43.5 The three limits, applied to the larger figure

The limits are read out of the sealed lock's own build recipe rather than typed
into the tool:

| limit | sealed value | measured | with the sealed launcher |
| --- | --- | --- | --- |
| entries | 200000 | 17674 | 17676 |
| total payload bytes | 2147483648 | 1771449867 | 1773456499 |
| single file bytes | 536870912 | 160096808 | 160096808 |

The sealed launcher is an aarch64 Linux binary and cannot exist on the measuring
host; the only local build is a macOS object the builder would refuse. Relaxing
the digest check to include it is a stop condition, and quietly leaving it out
would understate the answer. So the tree is measured without it and the record
adds its sealed size and the two entries it would place, and all three limits are
re-applied to that larger figure. The decision is taken on the complete number.

Nothing is truncated, sampled or excluded to fit; over a limit the tool fails.
The payload sits at about 83% of the total-byte limit, which is the tight one —
worth knowing before anything else is added to this tree.

### 43.6 Measurement mode cannot make an image

The tool accepts exactly two external executables, both replay tools, and refuses
every other path with a message that names measurement mode. Nine image-producing
tools are refused by name, and the gate asserts each of those names appears in the
source exactly once — in the refusal list — so one cannot be reached by a call
added later. The module does not import the production phase and does not contain
the words for kernel extraction, initrd or root disk.

### 43.7 What this does not establish

Not that an image was produced: none was. Not that an image would build, serve or
boot. Not that the launcher binary requirement is met — it is accounted for, not
present. Not that file ownership in the final image is right; a non-root writer
cannot reproduce uid 0, and no required measurement depends on it. Passing the
three limits means the preconditions for opening image production are met, and
nothing more than that.

### 43.8 Cursor

```
boot staging tree measurement = BOOT-STAGING-TREE-MEASURED-NOT-PRODUCED
  merged and measured through one assembler, in a fifth projection;
    the fourth step's sealed assertions unchanged
  17674 entries / 1771449867 payload bytes / 160096808 largest file
  builder table and independent on-disk walk agree on all eight keys
  13454 + 4217 = 17671 would have been wrong by three derived directories
  nested content manifest on disk equals the digest the launcher reads
  three sealed limits satisfied, re-applied with the launcher accounted for
  next: image production, exactly once, two replicas
```

No image was produced, no production was dispatched and no boot was performed. No
package was downloaded or hashed. No launcher source file, launcher seal, frozen
builder, existing projection, existing generator or sealed source lock was
modified. Serving is not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD,
BF.7=HOLD, Base activation false and activationAllowed=false are unchanged. No
public mining, no leaderboard claim and no paid-API benchmark is made anywhere in
this section.

## 44. The successor production path, pre-registered before it exists (2026-08-28)

### 44.1 Why a pre-registration and not just an implementation

The measurement in §43 established that the assembled tree is what it needs to be.
It did not establish that anything would produce an image from it, because nothing
does yet: the production path in the tree reads the predecessor lock, imports the
predecessor builder, and passes no nested tree at all. Dispatching it would build
the image that already booted — a run spent on a known answer, out of a budget of
one.

So the next thing written down is not the wiring. It is the record the wiring will
be judged against, written while no result exists to shade it. Pre-registration is
worth something only when it is impossible to reach back and adjust: after a
production has run, every condition in the record is a condition someone could
have chosen because it passed.

### 44.2 What the predecessor record already said

A successor criteria record was sealed earlier in the tree. It names the
predecessor workflow as its producer, and it was accurate when written: that
workflow was the only one, and the record said plainly that three things were
missing.

| Requirement it listed | State then | Closed by |
| --- | --- | --- |
| the builder stages all seven inputs | not done | the successor source lock, widening the staging table from ten tracked files to fifteen |
| a successor boot rootfs source lock covering them | not done | the successor plan sealed first, the lock generated from it |
| the builder materialises the runtime rootfs and its manifest | not done | the fifth projection's merge, exercised for real by the §43 measurement |

All three are closed. The fourth line it listed — that the record be merged to
main — was closed when it was sealed.

What cannot happen is correcting its producer field. A record edited to name a
different workflow after that workflow exists is a record describing the run
rather than committing to it, and the same edit would erase the evidence that the
three requirements were once open. So the correction goes in a successor
authority, and the predecessor's digest is pinned inside it, byte-unchanged.

### 44.3 What separation actually has to mean

"Separate successor path" is easy to say and easy to half-do. Four things are
separated, and each has a failure it prevents:

- **A separate authority record.** The predecessor's stays as sealed, so the image
  that already booted stays checkable against the criteria it was judged by.
- **A separate workflow.** The predecessor workflow keeps reading the predecessor
  lock. Re-pointing it would have made the earlier boot unreproducible, which is
  the one thing history is for.
- **A separate attempt identifier and result path.** One allowed attempt cannot be
  spent twice, and the filesystem is what says so. Sharing a result path would
  make the second dispatch look like the first.
- **Refusal in both directions.** The predecessor lock handed to the successor
  builder must refuse; the successor lock handed to the predecessor builder must
  refuse. Neither may fall back to the other, because a fallback turns a
  misconfiguration into a silently wrong image.

### 44.4 Where the budget boundary is drawn, and why there

The rule is one sentence: a refusal raised before the output directory exists does
not consume the attempt; once any output file has been created, the attempt is
consumed whatever happens next.

The reason it is drawn at the filesystem rather than at an exception type is that
the filesystem is the only witness that survives a crashed job. An authority or
preflight refusal leaves nothing behind, so the next run genuinely is the first. A
half-written image leaves something behind, so the next run is not — and calling it
the first would be the first step towards running until the answer is good.

The preflight inherits this for free: it creates no output directory at all, so no
preflight outcome can consume the production budget. That is a property of what it
is allowed to call, not a promise about how carefully it is written.

### 44.5 The numbers it commits to in advance

| Quantity | Value | Where it must appear |
| --- | --- | --- |
| entries, launcher excluded | 17674 | preflight, matching the sealed measurement |
| payload bytes, launcher excluded | 1771449867 | preflight |
| path manifest digest | `a342a1a5…3736` | preflight |
| entries, launcher included | 17676 | preflight and production |
| payload bytes, launcher included | 1773456499 | preflight and production |
| launcher size | 2006632 | rebuilt on the runner, compared with the seal |
| limits | 200000 / 536870912 / 2147483648 | both totals, refuse on exceeding |

Nothing is truncated, sampled or excluded to fit. A tree that does not fit is the
finding.

### 44.6 What this does not establish

Not that the successor path exists — it does not yet; this record is what it will
be measured against. Not that a preflight passes. Not that an image builds, boots
or serves. Not that the launcher's held condition is resolved; it stays held. The
one allowed production attempt is unspent, and the boot attempt after it is not
opened by anything here.

### 44.7 Cursor

```
successor production authority = SUCCESSOR-PRODUCTION-PRE-REGISTERED-NOT-WIRED-NOT-RUN
  predecessor criteria superseded on its own terms, bytes unchanged
  its three open requirements closed, each named with the record that closed it
  separate authority / workflow / attempt id / result path, refusal both ways
  eleven bound input digests, checked by iterating rather than by reading
  budget boundary: before an output file exists it is unspent, after it is spent
  runsAllowed=1 runsPerformed=0, preflight repeatable because it produces nothing
  next: the refusal tests, every one of them RED first
```

No image was produced, no production was dispatched and no boot was performed. No
package was downloaded or hashed. No launcher source file, launcher seal, frozen
builder, existing projection, existing generator, sealed source lock or sealed
criteria record was modified. Serving is not claimed. mineable_now=0,
REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation false and
activationAllowed=false are unchanged. No public mining, no leaderboard claim and
no paid-API benchmark is made anywhere in this section.

---

## 45. The successor path, built against its own refusals (2026-08-28)

The pre-registration in section 44 named a workflow, a result path, a budget line
and eleven input digests before any of them existed. This section is the other
half: a module that consumes them, a wrapper that runs it in the environment the
predecessor's production used, and a workflow with two modes. Nothing has been
dispatched. The one allowed attempt is still unspent.

The order was the one the pre-registration asked for. Every check was written as a
failing test first, and only then implemented — 110 of them, each naming a way the
path could quietly become the wrong path.

### 45.1 The tests are all refusals, and that is the point

A test that asserts the successor produces the right tree can pass for a bad
reason: the tree is right today. The refusals ask a different question — what has
to be true for this path to *stop*. The predecessor lock reaching the successor
builder is a stop. The successor lock reaching the predecessor builder is a stop.
A missing account file, a missing content manifest, a manifest whose digest moved,
the superseded launcher unit, a launcher whose rebuilt size is not 2006632, a
staging total that is not 17674 without the launcher or 17676 with it, a payload
that is not 1773456499, a path manifest that is not the sealed one, a single
collision or duplicate or symlink escape, a limit exceeded and quietly trimmed
rather than reported, a runtime argument with a default, an image tool reached
from the preflight — every one of those is a test, and every one of them failed
before it passed.

Two of the refusals are about the code's shape rather than its inputs, because
no input can express them. The preflight's promise to produce nothing is checked
by walking this module's own call graph from the preflight entry point: if it can
reach the production entry point, or any of the image steps, the module refuses to
start. And production and measurement are required to share one assembler *object*
— identity of the mapping, not equality of two copies — because two functions that
merely agree today are two functions.

### 45.2 The test that could not be written the way it was first written

One refusal was that the successor must not take its lock from the predecessor.
The first attempt searched the source text for the predecessor's name. It could
not work: the string it was looking for was in the file doing the looking, so the
check failed against itself. Worse, a substring search cannot tell
`materialize_runtime_lock` (the release gate, legitimate) from
`normalized_runtime_lock` (the base projection, also legitimate) from a call into
the predecessor phase (not legitimate). All three end in the same characters.

What matters is which object an attribute is taken from, which is a question about
the parse tree and not about the characters. The check now finds whatever name the
predecessor module was imported under and refuses attribute access through it —
except for the lock-independent image helpers, which are reached deliberately and
by name. The functions that may not be called are found the same way: whichever
functions over there mention the predecessor's own constant for the first lock.
There are three, and the list is derived rather than copied.

### 45.3 A defect the 102 passing tests did not have

With the tests green, the real assembly was run against the local package store —
not to produce anything, but to get past the shape checks into the code that reads
actual staged entries. It failed immediately, and the reason was worth the trip.

A staged entry carries `path`, `kind`, `mode`, `uid`, `gid` and `raw`. It carries
no digest and no size: the builder holds the bytes and hashes them when it writes
the layer. The account-database check and the content-manifest check were reading
a `sha256` key. Against the fixtures they read a digest, because the fixtures
supplied one. Against the builder they would have read nothing, and compared
nothing against a sealed value — in the production step, where the attempt is
spent.

The fix is that both checks now hash the bytes that will actually be written, and
refuse an entry staged without them. The fixture was corrected to the real key set,
and three tests were added that specifically defeat an entry claiming a digest it
does not have. The tests were richer than reality, which is the failure mode a
test suite cannot see from the inside.

### 45.4 Reuse where reuse is the requirement

The merge function, the totals, the three limits and the subprocess policy are
taken from the measurement module by reference rather than reimplemented. That is
not economy. The pre-registration refuses a path where production and measurement
use different merge functions, and the cheapest way to satisfy a rule about two
things being the same is for there to be one thing.

The same applies downstream: the kernel extraction, the initrd, the writer tree,
the root-disk plan and execution, and the image verification all come from the
predecessor's modules, which are untouched. What the successor adds is which lock
is opened and what gets merged into the tree — nothing about how an image is
written.

### 45.5 What the local run established, and what it did not

On macOS, against the local store, the successor assembly reaches 17674 entries,
1771449867 payload bytes and path manifest `a342a1a5…3736` without the launcher —
the sealed measurement exactly. All five account files, the v2 launcher unit and
the content manifest are present in that tree and pass their own checks when
hashed from the bytes.

That is an assembly on the wrong operating system and the wrong architecture. It
is evidence that the wiring is right, not the preflight the authority requires,
which has to run on arm64 Linux where production runs. No image tool was called,
no output directory was created, and nothing was downloaded: the store was already
verified and was reused as it stood.

### 45.6 The wrapper exists because the environment is part of the output

The production step could not be a bare interpreter call. The predecessor's
wrapper mounts a tmpfs for the staging tree, because the image writer walks that
tree with `readdir` and never sorts it; binds `TMPDIR` into the scratch directory;
runs the phase inside a transient unit whose arguments are printed by the sealed
producer authority rather than written out; and reads the finished disk back in a
separate stage, because that unit has private devices and a loop mount is a
device.

Two replicas produce byte-identical files only if neither of them decides
anything. A successor wrapper that skipped any of this would be asking two runners
to agree about their own disk order. It is a separate file from the predecessor's
rather than a flag on it, for the same reason the phases are separate modules: a
single script with a mode is one edit away from building the wrong image from the
right authority.

### 45.7 What this does not establish

Not that the preflight passes on arm64 Linux — it has not been run there. Not that
an image builds. Not that anything boots or serves. Not that the launcher's held
condition is resolved; it stays held. The production attempt count is zero, the
boot attempt count is zero, and neither successor result file exists.

### 45.8 Cursor

```
successor production path = WIRED-NOT-RUN
  110 refusal tests, RED before GREEN, no happy-path assertion among them
  preflight's no-output claim proved from this module's own call graph
  production and measurement share one assembler object, by identity
  staged entries hashed from their bytes; a claimed digest is refused
  local assembly reaches the sealed measurement exactly (macOS, not the gate)
  wrapper reproduces the predecessor's tmpfs / transient unit / readback
  workflow: preflight repeatable, produce one dispatch, compare byte-identical
  next: the preflight on arm64 Linux, sealed as a result before any production
```

No image was produced, no production was dispatched and no boot was performed. No
package was downloaded and no package was re-hashed. No launcher source file,
launcher seal, frozen builder, existing projection, existing generator, sealed
source lock, sealed measurement or sealed criteria record was modified; the
predecessor phase, wrapper and workflow are byte-unchanged. Serving is not
claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation
false and activationAllowed=false are unchanged. No public mining, no leaderboard
claim and no paid-API benchmark is made anywhere in this section.

### 45.9 Two things the 110 tests were still not asking (addendum)

Reading the sealed authority's own pass conditions against the code that was
supposed to satisfy them turned up two gaps. Neither is a bug in what was
written; both are questions nobody had asked.

**The unit was checked for wanting to start, not for being started.** The
authority requires the v2 unit to be "present, enabled, and carries console
output on both streams". The check read `WantedBy=multi-user.target` out of the
unit file and treated that as enablement. It is not. `WantedBy=` is what the unit
asks for at `systemctl enable` time; what systemd acts on at boot is a symlink in
`/etc/systemd/system/multi-user.target.wants/`. The source lock does stage that
symlink — it is there in the derived entries, pointing at the unit, root-owned —
but nothing asserted it. An image assembled without it would hold a launcher that
is installed, correct, capability-bounded, and never run, and every check would
have been green. The boot would have looked like a silent guest, which is exactly
the failure this whole wave exists to stop being ambiguous about.

Five new refusals now cover it: a missing link, a link pointing at another unit, a
copy of the unit staged in the wants directory instead of a link, a link owned by
somebody other than root, and the seal reading its shape out of the lock rather
than out of this file. The check runs inside `assert_launcher_unit`, which the
shared assembler calls, so it guards the production run too — and it guards it
before the output directory exists, which is the side of the budget line where a
refusal is free.

**The result recorded that the gaps were closed, not what was found.** The
authority also requires the preflight result to carry "the five account files …
with the frozen mode and ownership", the unit's evidence, and "the result digest
and the full provenance". The document had the totals and the manifest digest, and
for the rest it had the fact that no exception had been raised. That is a claim
about the run, not evidence from it.

So the three gaps are now read back off the tree the writer produced, rather than
off the entry table it was handed — the table is a declaration too, and the
authority asks for the assembled tree. `gap_evidence` walks the written staging
tree and records each account file's mode, size and digest as found, the unit's
digest and every required directive as found, the enablement link's target as
found, and the manifest's digest, size and mode as found; any disagreement with
the seal is a refusal. Ownership is deliberately *not* read from disk: the writer
says outright that it cannot reproduce ownership when it is not root, so a uid
read there would record whoever ran the preflight. The owner each entry carries
into the image comes from the table the image writer copies it from, and that is
the one recorded. A test asserts the function never touches `st_uid` or `st_gid`,
so the distinction cannot be quietly lost later.

Provenance is now a recomputed block rather than a sentence: the digest of every
module this path reads code out of, hashed from the file at run time; the
authority, source lock, measurement and launcher-build-result digests; the
resolved gpgv and zstd paths; the repository root and artifact store; and the
host's system, machine, kernel release and Python version. A later reader can
take a sealed result and find the exact text that produced it, which is the only
form of "which build was this" that survives a five-layer projection chain.

The local macOS assembly was re-run against the real lock and the already-verified
store to check that the new refusals are satisfied by the real tree rather than
only by fixtures: 17674 entries, 1771449867 payload bytes and path manifest
`a342a1a5…3736` — the sealed measurement, unchanged — with the enablement symlink
staged at mode `0777`, root-owned, pointing at the unit's guest path, and
`gap_evidence` returning the five account rows, the unit's seven directives and
four capabilities, the link's target and the manifest's digest, size and mode. The
gap readback was exercised against the three gaps written out on their own,
because this filesystem is case-insensitive and the full tree holds twenty
case-folded sibling pairs that a write here would silently merge. That is still
evidence of wiring on the wrong operating system and architecture.

The production path deliberately did **not** gain the disk-side readback. It runs
the same three assertions on the same entry table through the same shared
assembler, and it runs them before the output directory is made. Adding a second
readback after that line would only add new ways to fail on the far side of the
budget boundary, where a failure spends the one attempt there is.

```
successor production path = WIRED-NOT-RUN
  130 refusal tests, RED before GREEN, no happy-path assertion among them
  the wants symlink is now required, not inferred from WantedBy=
  the three gaps are read back off the written tree, not off the table
  the result carries what was found, and the provenance to trace it
  ownership is read from the table, never from a tree a non-root run wrote
  production keeps its refusals on the free side of the budget line
  next: the preflight on arm64 Linux, sealed as a result before any production
```

Still nothing produced, dispatched or booted. Production attempts 0, boot attempts
0, `runsPerformed=0`, and neither successor result file exists. No package was
downloaded or re-hashed. No launcher source, launcher seal, sealed lock, sealed
measurement, sealed criteria or predecessor module was modified. Bootable and
serving are not claimed. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD,
Base activation false and activationAllowed=false are unchanged. No public mining,
no leaderboard claim and no paid-API benchmark is made anywhere in this section.

### 45.10 What the first preflight run found (addendum)

The first dispatch of the successor workflow in `preflight` mode, run 33156573907
on `ubuntu-24.04-arm`, stopped at its fourth step:

```
ci-payload-acquire: cargo-rustdist is absent from the store;
it is fetched by scripts/native_shadow_boot_rustdist_acquire_arm64_v1.py,
which runs before this one
```

The `produce` job acquires the frozen Rust distribution before it acquires the
package payloads. The `preflight` job did not — it had the second acquirer and
not the first. The store the preflight assembled from was therefore never the
store the production would have assembled from, and the package acquirer said so
rather than guessing.

This is the boundary working as designed rather than an accident that happened to
be survivable. The workflow's own header says the preflight does everything the
production does except the part that costs the attempt, and acquisition is not
that part. The gap surfaced in the mode that produces nothing, before any
assembly, before any output directory, and it cost no part of the one allowed
production. Had the same asymmetry been discovered on the production path it
would have stopped a replica; had it been an asymmetry in the other direction — a
preflight that filled a store the production would not have — it would have
passed a preflight the production then failed, which is the expensive shape.

Six tests now hold the symmetry, all RED first against the workflow as dispatched:
the production runs both staging acquirers, the preflight runs every staging
acquirer the production runs, each job takes the toolchain before the packages,
the preflight acquires before it assembles, the preflight does **not** fetch the
ext4 writer set, and the production does. The writer set is excluded on purpose:
it is the tool that writes the image rather than an input the staging tree reads,
and asking a no-output mode to fetch an image writer would undo the mode. The
tests read the workflow by indentation rather than through a YAML parser, because
the runner this gate has to pass on is not promised one and the question is only
which lines sit in which job.

The preflight step added to the `preflight` job is the production's step
unchanged, including its re-proof: the sealed acquisition record is removed,
regenerated from the frozen identities, and required by `git diff --exit-code` to
come back byte for byte. An artifact already present in the store is verified in
place and never re-fetched, so this adds no download to a warm store; on a fresh
runner it is the same three archives the production path already fetches.

```
successor preflight = DISPATCHED-ONCE / REFUSED-BEFORE-ASSEMBLY / RE-RUNNABLE
  136 refusal tests, RED before GREEN
  the refusal was a wiring asymmetry, not a measurement disagreement
  nothing assembled, nothing written, no output directory, no artifact
  production attempts unchanged at 0
  next: re-dispatch the preflight, then seal its result before any production
```

Still nothing produced or booted. Production attempts 0, boot attempts 0,
`runsPerformed=0`, and neither successor result file exists. No launcher source,
launcher seal, sealed lock, sealed measurement, sealed criteria or predecessor
module was modified. Bootable and serving are not claimed. mineable_now=0,
REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation false and
activationAllowed=false are unchanged. No public mining, no leaderboard claim and
no paid-API benchmark is made anywhere in this section.

### 45.11 The second and third preflight runs, and a quantity made a property of the tree (addendum)

**The second run: a module that could not be run the way it is run.** Run
33156887243 got further than the first — every acquisition step passed and the
launcher was rebuilt and matched its seal — and then stopped before it read a
single byte of any tree:

```
ModuleNotFoundError: No module named 'scripts'
```

The workflow invokes the phase as `python3 scripts/<name>.py`. That puts
`scripts/` on the interpreter's path rather than the directory the `scripts`
package sits in, so the module's own `from scripts import ...` block had nothing
to import from. The predecessor phase carries a line that inserts the repository
root ahead of that block. The successor did not.

No test in the file could have noticed. Every one of them imports the module as
`scripts.<name>`, which puts the root on the path before the module is read. The
defect lives exactly in the gap between how the tests reach the module and how
the workflow reaches it, and a test written like its neighbours is blind to it by
construction.

It would have been expensive on the other path. The production wrapper creates
the output directory before it invokes the phase, so this same exception would
have been raised on the far side of the line the budget is drawn on. It was
raised in the mode that creates nothing instead.

Two tests hold it. The module is run in a subprocess from the repository root
with `PYTHONPATH` removed, so an inherited one cannot answer for it, and is
required to reach its own argument parser rather than an import error; and both
phases are required to put the root on the path before they import the package,
so the two cannot drift apart again. A sweep of the remaining modules found no
third case that any workflow or shell script invokes as a script.

**The third run: a quantity that was not a property of the tree.** Run
33157320718 assembled the tree, wrote it, walked it, and stopped at the
comparison:

```
the assembled table and the staging tree disagree on largestFilePath:
'opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly'
against
'var/lib/boole/native-shadow/runtime-rootfs/opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly'
```

This one is not a wiring gap, and the record has to be exact about what it does
and does not show.

What it shows is that the tree is the sealed tree. The phase measures twice: once
from the assembled entry table and once from an independent walk of what was
written. The table comparison passed on all nine quantities — 17,674 entries, the
same three kind counts, 1,771,449,867 payload bytes, the same largest size, no
collisions, no duplicates, no escapes, and the same path manifest digest
`a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736`. A path
manifest digest is a statement about which paths exist, and it matched the seal
exactly.

What failed is the walk's answer to a different question. Two files in this tree
have exactly the sealed largest size, 160,096,808 bytes: the checker toolchain's
`libLLVM` in the guest's own root, and the copy of it inside the nested runtime
rootfs that the fourth condition requires be carried for replay. Both copies
belong there. `traverse_staging_tree` kept the first file it met at the maximum
size — the comparison is a strict `>` — and it meets files in whatever order the
directory it is reading hands them to it. The table is iterated in a fixed order
and always answers `opt/…`; the walk was iterated in filesystem order and this
time answered `var/…`.

The two modes do not walk the same kind of filesystem. The production wrapper
mounts a tmpfs and builds the tree on it; the preflight job writes under
`RUNNER_TEMP`, which is the runner's ordinary disk. Directory read order is not
the same property on the two, and neither of them promises write order.

So `largestFilePath`, read off a walk, was a property of the filesystem rather
than of the tree whenever two files tie for largest. It was recorded in the
sealed measurement as though it were a property of the tree, and it agreed with
the table there because the order on that host happened to agree.

The production path does not ask this question. `produce` compares the table's
totals and never writes or walks a staging tree — the whole function was read to
confirm it: no write, no traversal, no comparison against a walk. The
disagreement was contained in the preflight's comparison, and no image production
would have failed on it.

**The decision, and why it changes no sealed value.** Resolving it meant deciding
what a sealed criterion means, which is not a decision this path may take on its
own. It was reported to the operator as a HARD STOP under *the measurement result
and the actual successor preflight disagree*, with three ways out written down
and one recommended: make the tie-break deterministic so the byte-smallest path
wins at equal size. The operator approved that one and fixed its exact wording —
*among the regular files of greatest size, the path whose canonical path bytes
sort first* — together with what may not be used to sort: no locale, no
case-insensitive order, no Unicode normalisation or case fold, and not the order
the filesystem offered. Directories and symlinks are not candidates, and size
stays the regular file's payload bytes.

That recommendation is provable rather than likely, and the proof is in the
function that produced the sealed value. `builder_totals` iterates
`sorted(paths, key=…encode("utf-8"))` — the same ordering the path manifest
digest is built from — and keeps the first file at the maximum. Those two rules
together are exactly *the byte-wise smallest path among the files of greatest
size*, taken over every path in the tree. So the sealed `opt/…` is not merely
smaller than the `var/…` the walk found; it is the minimum over all files that
tie, and no unobserved further copy can sort ahead of it. Defining the walk's
tie-break the same way reproduces the sealed value by construction. This is not a
criterion relaxed or a value revised: nine quantities are still compared and the
sealed numbers are untouched. It is the implicit rule that made the sealed value
written down where the other side of the comparison can also read it.

It is also the smaller inconsistency to fix rather than a new rule. Every other
quantity `traverse_staging_tree` returns is already order-independent — counts,
sums, and a path manifest digest computed over sorted paths. `largestFilePath`
alone was read in whatever order the directory offered.

The two alternatives were named and not taken: matching the preflight's
filesystem to the production's leaves the tie in place and relies on the order
happening to agree, and dropping the quantity from the comparison removes a
criterion rather than defining it.

**What was changed.** One function, `largest_regular_file`, holds the rule and
its reasons; `traverse_staging_tree` collects each regular file's path and size
and asks it, instead of keeping a running winner in directory order.
`builder_totals` was not touched — it already computes the rule, and the sealed
values came out of those bytes. Nine tests hold the rule, and the operator named
eight of them: the same answer from a tied pair given both ways round; the
byte-smallest of more than two ties; every permutation of an encounter order
giving one answer; a strictly larger file winning whatever the tie rule says;
paths differing only in case ordered by their bytes; the same character composed
two ways staying two paths; directories and symlinks never candidates even when
they look the right size; the sealed tree still answering `opt/…`; and the table
and the walk agreeing on a tree written once and read both ways round. The last
of those reproduces the runner's refusal on a laptop: before the fix it fails
with `'etc/aa-tie' against 'etc/zz-tie'`, which is the runner's message with
smaller names.

```
successor preflight = DISPATCHED-THRICE / REFUSED-BEFORE-OUTPUT / TIE-BREAK-FIXED
  run 2: the phase could not be run the way a workflow runs it
  run 3: the tree matches the seal; one order-dependent quantity did not
  the rule that produced the sealed value, written where both sides read it
  no sealed record, criterion or measured value changed
  nothing assembled into an image, no output directory, no artifact
  production attempts unchanged at 0
  next: re-dispatch the preflight; seal its result only if all nine agree
```

Still nothing produced or booted. Production attempts 0, boot attempts 0,
`runsPerformed=0`, and neither successor result file exists. No launcher source,
launcher seal, sealed lock, sealed measurement, sealed criteria or predecessor
module was modified. Bootable and serving are not claimed. mineable_now=0,
REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base activation false and
activationAllowed=false are unchanged. No public mining, no leaderboard claim and
no paid-API benchmark is made anywhere in this section.

### 45.12 The fourth preflight run: the tree, measured on the machine that will build it (addendum)

Run 33164208857, on `main` at `4e86eb0`, ran every step of the production except
the part that costs the attempt, and every one of them passed.

**What it did.** Resolved the host tools from `PATH`; acquired the frozen Rust
distribution and re-proved its sealed record byte for byte; acquired the frozen
package payloads; rebuilt the launcher from source and matched it against its
seal; assembled the successor staging tree for real — the account database, the
v2 unit and its enablement link, the nested runtime rootfs and its content
manifest, all merged through the one materialization function production also
consumes; wrote that tree out; walked it with a function that knows nothing about
the table that produced it; and compared both against the measurement sealed
before any of this existed.

**What it answered.** Nine quantities, three sides — the assembled table, the
independent walk, and the seal — and all three agree:

| Quantity | Value |
| --- | --- |
| entries | 17,674 |
| byKind | 1,736 directories, 15,101 files, 837 symlinks |
| payloadBytes | 1,771,449,867 |
| largestFileBytes | 160,096,808 |
| largestFilePath | `opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly` |
| pathManifestSha256 | `a342a1a59178af546c0c0d212aecd770d02333bf9c289a11b42627b271693736` |
| caseFoldedSiblings | 20 |
| duplicatePaths | 0 |
| symlinkEscapes | 0 |

`largestFilePath` is in that table for a reason. It is the quantity the previous
run refused on, and it agrees now because both sides break the tie the same way:
among the regular files of greatest size, the path whose canonical bytes sort
first. The sealed value came back, on a different filesystem, unchanged.

**The launcher, projected separately.** The measured tree does not contain the
launcher; the production-bound tree does. The rebuilt binary matched its seal at
`11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434`, 2,006,632
bytes, and the two entries it adds are recorded one row each: the directory
`/usr/libexec/boole`, and the file `/usr/libexec/boole/boole-native-shadow-launcher`.
With them the tree is 17,676 entries and 1,773,456,499 payload bytes — the
sealed with-launcher projection exactly. All three limits pass with room: 17,676
against 200,000 entries, 160,096,808 against 536,870,912 for a single file, and
1,773,456,499 against 2,147,483,648 in total.

**The three gaps, read back out of the tree rather than out of the plan.** The
account database is five files — `/etc/group`, `/etc/gshadow`, `/etc/nsswitch.conf`,
`/etc/passwd`, `/etc/shadow` — all owned by uid 0 and gid 0, the two shadow files
at mode 0400 and the rest at 0444. The launcher unit is the v2 service at
`/usr/lib/systemd/system/boole-native-shadow-launcher.service`, digest
`4c31bce411c9999b8e877977ce8787d0716a977316ae0a7677240b987181bd55`, with
`StandardOutput` and `StandardError` both `journal+console` so the host reads the
launcher's own words off the console it already collects, an empty
`AmbientCapabilities`, and a bounding set of exactly four: `CAP_SETGID`,
`CAP_SETUID`, `CAP_SETPCAP`, `CAP_SYS_ADMIN`. The enablement symlink is staged at
`/etc/systemd/system/multi-user.target.wants/`. The nested runtime content
manifest is present at `/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json`,
1,285,116 bytes, digest `200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa`
— the sealed one.

**What it did not do.** No `mke2fs`, no initrd, no root disk, no output
directory, no artifact upload, no attempt consumed. The workflow's own step —
*Require this run to have produced nothing* — passed after the phase, and the
record says `outputsCreated: false`. `imageProducedClaim`, `bootableClaim`,
`servingClaim` and `activationAllowed` are all false in it.

**What was sealed.** The bytes the runner wrote, unmodified, at the path the
authority named before any of this ran:
`native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json`,
digest `be4a84e1c058fa25804cfade07727e35613369f58b0307182b93f24a4ecfb071` — the
digest the run itself printed. Nine tests hold it, and they do not read the
record's own claims back to itself: the nine quantities are re-checked against
the sealed measurement, the launcher digests against the sealed launcher result,
the lock digest against the successor lock and against not the predecessor's, and
the two added entries against the difference between the two entry counts.

```
successor preflight = PASSED-ON-ARM64 / PRODUCED-NOTHING / SEALED
  run 33164208857 on main 4e86eb0
  nine quantities, three sides, all agreeing
  three gaps read back out of the assembled tree
  result be4a84e1...b071, sealed at the pre-registered path
  production attempts still 0, boot attempts still 0
  next: the one-shot production, two replicas, exactly once
```

Producing is not booting and booting is not serving. This run produced nothing at
all, so none of the three is claimed. Production attempts 0, boot attempts 0,
`runsPerformed=0`. mineable_now=0, REWARD_READY=0, RP0-MD=HOLD, BF.7=HOLD, Base
activation false and activationAllowed=false are unchanged. No public mining, no
leaderboard claim and no paid-API benchmark is made anywhere in this section.

### 45.13 The first production attempt, and where its preflight could not see (2026-08-28)

The production was dispatched once, on `7ae77e67`, with every pre-dispatch
requirement met: local head equal to `origin/main`, a clean tree, the authority
and the sealed preflight result matching their files on disk, a production
attempt count of zero, no result and no artifact anywhere, and the four earlier
dispatches confirmed one at a time to have run the preflight job and skipped the
produce job. Both replicas then failed at the same call, 437 and 440 milliseconds
after their unit started:

```
FileNotFoundError: [Errno 2] No usable temporary directory found
in ['/tmp', '/var/tmp', '/usr/tmp', '/']
```

**What produced nothing.** No kernel, no initrd, no root disk; no artifact was
uploaded and the manifest, evidence and comparison steps never ran. The phase
assembles the nested runtime tree in `main` and calls `produce` afterwards, and
`produce` is where the output directory is created, so the traceback stops
before the first line of the budget. The empty output directory the run did
leave behind was made by the wrapper, which has to create it because a systemd
`ReadWritePaths` entry must exist before the unit starts.

**Why the environment refused.** The production runs inside the sealed transient
unit, which mounts the filesystem hierarchy read-only except the paths it is
handed. `systemd-run` starts a unit with a cleaned environment, so the `TMPDIR`
the wrapper exported never reached the phase, and two helpers deep in the shared
rootfs builder -- the InRelease signature check and the zstd decompressor -- ask
for a temporary directory without naming a place. Both are on the produce path.
Repairing one would have moved the failure to the other.

**Why no earlier run could have caught it.** The preflight ran beside the unit,
where the whole filesystem is writable; the production ran inside it, where
almost none of it is. The fourth preflight assembled this tree, walked it, and
agreed with the sealed measurement on all nine quantities -- and none of that
could speak for an environment it never entered. The predecessor image needs no
nested runtime tree, so the predecessor production never reached a helper that
asks for a temporary directory at all. The nested runtime tree is one of the
three gaps this wave exists to close, and it brought a new environment
requirement in with it.

**The budget, and why the operator settles it.** Two pre-registered statements
disagree here and nowhere else. The standing instruction spends an attempt once
an output *file* exists, and none did. The workflow's own comment puts the line
at the output *directory*, and one existed. A runner that picked the reading
favouring itself would be granting itself a second one-shot, so the question went
to the operator with the facts and a recommendation, and no second production has
been dispatched.

**The correction, which widens nothing.** The phase now names its temporary
directory once, out of the scratch it was already handed -- the directory the
isolation was already told it may write -- so every indirect caller is answered
without editing a shared module the predecessor image is also built from. The
wrapper runs the preflight through the same sealed unit before it produces, and
creates the output directory only after that has passed, which puts the budget
line where the phase's own comment always put it. The repeatable preflight mode
runs through that same unit too, so the correction can be proved without
touching the production budget; that mode refuses an `--outputs` rather than
ignoring one.

No isolation property was changed, no non-isolated fallback was added, and the
sealed producer authority, the launcher source, the launcher binary, its sealed
digest, the predecessor path and every sealed record are byte-unchanged. Boot
attempts stay 0 and `runsPerformed` stays 0. mineable_now=0, REWARD_READY=0,
RP0-MD=HOLD, BF.7=HOLD, Base activation false and activationAllowed=false are
unchanged. No image was produced, so no image is claimed; producing is not
booting and booting is not serving.

### 45.14 The operator's budget ruling, and a boundary that cannot be crossed by accident (2026-08-28)

**Append-only.** §45.13 stands unedited, including its `OPERATOR-DECISION-PENDING`
verdict, which was true when it was written.

**The ruling.** The operator settled the case §45.13 reported: the run is one
workflow dispatch and zero consumption of the image production budget. One
production attempt remains. The grounds are the ones the record established --
only the output directory was created, zero output files were written, the run
never reached the production function, zero artifacts were uploaded, and the
pre-registered rule consumes the attempt once an output *file* exists. Creating
an empty directory is therefore not consumption.

The accounting is kept split rather than collapsed into one number:

| quantity | value |
| --- | --- |
| workflow runs dispatched | 1 |
| empty output directory created | yes |
| image output files created | 0 |
| production budget consumed | 0 |
| attempts remaining | 1 |
| prior failure record | unmodified |

**The condition attached to it.** The empty boundary has to be gone before the
attempt is used. It is not enough that this particular case was settled; the
gap that made it arguable has to close.

**Where the line moved to.** From the output directory to an `ATTEMPT-CONSUMED`
marker the phase writes on purpose, immediately before its first image file. A
directory is something the isolation requires -- a systemd `ReadWritePaths` entry
has to exist before the transient unit starts -- so its existence was never a
decision the phase made. The marker is. Above it every refusal is free, including
the layout build and the tree extraction, which write into the scratch and never
into the outputs; below it the attempt is spent whatever happens next.

It is written as a rename, not as a write: the document is built in full, flushed
to a neighbouring name that is not the marker, fsynced, renamed into place, and
the directory entry fsynced after it. A run cut off anywhere before the rename
leaves the marker absent, which is the honest reading of a run that never reached
its first image file. A test cuts the rename and asserts exactly that.

**Two ways it survives the runner.** The marker is echoed to the console the host
already collects, and a failed replica uploads it on its own. Only the marker: a
half-written image is not evidence of a production and must not be uploadable as
one. Its absence on a failed replica is a legitimate answer rather than an error.

**What was not edited.** The sealed authority, including its own budget sentence
and its `runsPerformed` of zero. The §45.13 record, including its pending
verdict. Every earlier sealed measurement, lock, preflight result and image
record. The ruling is a separate append-only document that pins both files it
rules on by digest, re-derived from the files themselves.

**The order that follows, and it is not negotiable.** Merge the isolated-preflight
correction; add the ruling as an append-only record; write the marker; treat a
failure before it as unspent and after it as spent; run the free preflight-only
mode on arm64 exactly once; and only if that passes, run the two-replica
production exactly once. If the marker or any output file exists when a
re-production fails, the budget is spent, there is no retry, and it is a hard
stop. No boot begins before both replicas have produced identical images.

Boot attempts stay 0 and `runsPerformed` stays 0. mineable_now=0, REWARD_READY=0,
RP0-MD=HOLD, BF.7=HOLD, Base activation false and activationAllowed=false are
unchanged. No image exists yet, so no image, boot or serving is claimed.

## 46. The third attempt, spent and failed on a baseline it never built against (2026-08-28)

The single production the third authority carried was dispatched once and ran to
completion on both replicas. Both wrote `ATTEMPT-CONSUMED.json`, built the
kernel, the initrd and the root disk, and wrote their result documents. Both
then failed the root-disk read-back on the same check with the same detail. The
marker is the line the budget was moved to, so the attempt is spent: no retry,
no second dispatch, no relaxed criterion, no boot.

**The failure, and what it is not.** The read-back stage reads its expectations
from the predecessor's source lock, reached through the v1 produce phase module,
while the successor phase built and verified against the successor lock. The two
locks differ precisely on the files this wave rewrote — the launcher service unit
and the runtime directory declaration that closes the third of the three holes —
so a successor image checked against the predecessor lock can fail on nothing
else. Reading both images back, read-only, confirms it: all 23 entries agree with
the successor lock on content, permission bits, owner, group and entry kind, and
the only disagreement is with the other lock, on content alone. The builder is
not at fault. The baseline the checker read is.

**What exists now.** Three appended records — the result, the stop, and the
read-back diagnosis — and two preserved, fingerprinted, disqualified sets of
files. They may not be adopted, named in a qualification, or cited as a
determinism pass; the comparison job the authority requires never ran. The
authority's own `runsPerformed` is left at zero and every earlier record is
byte-unchanged, exactly as before: the marker is what counts an attempt, and the
stop records are what count them.

**What was deliberately not done.** No code was repaired, no new production
attempt was granted, no image was adopted and no boot was begun. The cause is
stated; acting on it is a separate decision.

### 46.1 Corrected execution cursor

```text
SUCCESSOR-IMAGE-PRODUCTION  SPENT / FAILED — 2 of 2 attempts spent, 0 remaining
SUCCESSOR-IMAGE-ROOT-CAUSE  RESOLVED — read-back reads the predecessor lock
SUCCESSOR-IMAGE-REPAIR  NOT STARTED — needs a new operator grant before it runs
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
MAC.4  NOT STARTED — does not begin automatically
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged. No image is claimed to
exist, boot or serve. No clean-Mac canary, VM boot, real release build,
production key, public mining, paid API benchmark, user installation or
activation occurred.

## 47. The read-back correction (2026-08-29)

The cause named in §46 is a wire, and this section repairs the wire and nothing
else. The successor wrapper no longer calls the predecessor's read-back; it
calls a successor consumer that can reach exactly one source lock — the one the
producing phase proves and the production authority binds. The predecessor keeps
its own wrapper, its own consumer and its own lock, untouched, because that path
was never wrong.

**What makes it fail closed.** The lock is not selectable: no flag, no argument,
no environment variable, nothing read out of the image. A lock whose bytes have
moved is refused before anything is attached to a loop device. The consumer
refuses to load if its own text names the other generation's lock, so a fallback
between the two generations is not expressible rather than merely unused.
Successor material judged against the other lock is refused, and material of the
other generation judged against this one is refused, both ways.

**What a refusal now leaves.** The unqualified-diagnostic marker, written into
the outputs directory before the refusal is raised, and a result document named
for the successor. The third attempt left files that said nothing about whether
they were a production; a failed read-back now disowns them itself.

**What is deliberately still not re-sealed.** The producer fingerprint. The
wrapper's moved digest is declared, with both the sealed and the corrected
value, in `native-shadow-mac3-successor-readback-correction-arm64-v1.json`, and
checked against the live file.

### 47.1 Execution cursor after the correction

```text
SUCCESSOR-IMAGE-PRODUCTION  SPENT / FAILED — 2 of 2 prior attempts spent
SUCCESSOR-IMAGE-ROOT-CAUSE  RESOLVED — read-back read the predecessor lock
SUCCESSOR-IMAGE-REPAIR  MERGED-PENDING-AUTHORITY — wire corrected, nothing granted
SUCCESSOR-IMAGE-NEXT-ATTEMPT  NOT AUTHORISED HERE — needs a new authority, a new
  producer fingerprint over the corrected bytes, and a free preflight first
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
MAC.4  NOT STARTED — does not begin automatically
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursor in §46.1 is left as it was written; this one is added beside it.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged. No image is claimed to
exist, boot or serve. No production was dispatched, no marker was written, no
budget was granted or spent, and no clean-Mac canary, VM boot, real release
build, production key, public mining, paid API benchmark, user installation or
activation occurred.

## 48. The fourth authority (2026-08-29)

The correction in §47 repaired a wire and granted nothing. This section is the
grant: one further successor arm64 image production, dispatched exactly once,
written down before it runs.

**What the numbers say, and why they are two numbers.** The production workflow
has been dispatched three times. One of those dispatches was refused inside the
isolation before any output file existed and was ruled unspent; two went past
the marker and are spent. A dispatch and a spent attempt are not the same event,
and the authority now counts them separately: `priorWorkflowDispatches` 3,
`priorUnspentDispatches` 1, `priorProductionAttemptsSpent` 2,
`productionAttemptsRemainingBeforeThisGrant` 0,
`productionAttemptsGrantedHere` 1, `bootAttemptsUsed` 0,
`priorOfficialImages` 0. The two diagnostic replicas the third attempt left are
still unqualified and still not adoptable. Every one of those totals is
re-derived in the gate from the three detail rows it summarises, so a summary
that drifted from its detail would fail rather than read plausibly.

**What is newly bound.** The read-back consumer and the lock it is paired with.
The third attempt failed because the stage that judges the image was outside
everything that was sealed: the wrapper called whichever module it named, and no
pin noticed that the module read the other generation's baseline. The authority
now names the consumer, its digest, the source lock, the lock's digest and the
result document, and the producer fingerprint seals the consumer and its gate
alongside the wrapper, the workflow, the phase and the frozen helper.

**A contradiction found while writing it.** The third authority forbade the
preflight to "upload an artifact", while the preflight legitimately uploads a
small JSON document saying it produced nothing. Read literally the rule banned
the evidence along with the image. The fourth authority names what is actually
forbidden — a kernel, an initrd, a root disk or the consumed-attempt marker —
and names the one upload that is allowed.

**A budget claim moved off the call graph.** The preflight's "produced nothing"
step now also asks the filesystem whether the consumed-attempt marker exists,
with the marker's name read out of the phase rather than written down a second
time. That the preflight cannot reach the code that writes it was already
checked; a look at the disk is cheaper to keep true than a reading of the code.

**The direction of the binding is unchanged.** The module pins the authority's
digest, so the authority pins neither the module nor the gate — directly or
through any record that carries them. It names the producer fingerprint by path
only. The fingerprint points at the authority; nothing points back.

**What was not re-sealed, again.** The third producer fingerprint. It still pins
the bytes that produced the third attempt, and the three files that have moved
since are each declared in exactly one place: the gate in the third hard-stop
record, the wrapper in the correction record, the workflow in this authority.

### 48.1 Execution cursor after the fourth authority

```text
SUCCESSOR-IMAGE-PRODUCTION  1 ATTEMPT AUTHORISED / NOT DISPATCHED
SUCCESSOR-IMAGE-ROOT-CAUSE  RESOLVED — read-back read the predecessor lock
SUCCESSOR-IMAGE-REPAIR  MERGED — wire corrected under its own review
SUCCESSOR-IMAGE-AUTHORITY  V4 SEALED — runsAllowed 1, runsPerformed 0
SUCCESSOR-IMAGE-PREFLIGHT  NOT RUN — free, creates no marker and no image
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
MAC.4  NOT STARTED — does not begin automatically
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1 and §47.1 are left as they were written; this one is added
beside them. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged. No image
is claimed to exist, boot or serve. No production was dispatched, no marker was
written, no attempt was spent, and no clean-Mac canary, VM boot, real release
build, production key, public mining, paid API benchmark, user installation or
activation occurred.

## 49. The fourth attempt: spent, passed, and not booted (2026-08-29)

The one attempt the fourth authority granted was used, once, and it produced an
image. Nothing was booted, nothing is served, and the two claims are kept apart
on purpose: a file that exists is not a machine that runs.

**The order the operator required, in the order it happened.** The corrected
wiring went to `main` under its own review first — pull request #289, eleven
required checks green, no rerun of any kind, squash-merged as `7d18577`. Only
then was the free preflight dispatched: run 33202458054, one run, PASS. It
uploaded a single 2,450-byte JSON document and the runner reported that none of
`guest-kernel`, `guest-initrd`, `guest-root-disk` or the consumed-attempt marker
existed anywhere under its temporary tree. No budget was touched by it. Then the
production: run 33202978318, one dispatch, both replicas as matrix jobs of that
one run, success.

**What the two replicas produced.** Each wrote all three files and a read-back
result document. `guest-kernel` is
`d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336`,
`guest-initrd` is
`2fc4b4f473e6dd3d8ac3697fc5be07ab2828e93acc0c2ff5f8e20ef473c3833e`, and
`guest-root-disk` is
`51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c`. The sealed
comparison job ran and reported the three outputs identical. Both artifact sets
were then downloaded and re-hashed off the runner, and the three pairs compared
byte for byte by hand: identical. The two `PRODUCE-RESULT.json` documents differ
only in the random suffix of the scratch directory each job was given.

**The check that decided it.** `modes-owners-and-paths-match-the-lock` — the one
that failed the third attempt — passed in both replicas, against
`native-shadow-boot-rootfs-source-lock-arm64-v2.json`, with all 17,677 entries
accounted for. That is the entire repair: the builder was never wrong, and the
baseline it was judged against now belongs to the same generation as the image.
`e2fsck -f -n` returned zero for each replica with no repair option given, and
the root disk's digest afterwards is the digest before it.

**The digests match the attempt that failed.** Exactly. The third attempt's
disqualified files hash to these same three values, which is worth stating
plainly because it is the strongest available evidence that the builder was
deterministic all along and that only the judge was reading the wrong page.
It changes nothing about those files: they remain `UNQUALIFIED-DIAGNOSTIC`,
`mayNotBeAdopted` stays `true`, and the record of this attempt says in as many
words that they are not adopted. Agreeing digests are evidence, not a promotion.

**The accounting, kept whole.** Three prior workflow dispatches, one of them
unspent; two attempts spent before this one; one granted here; this one spent at
the moment the marker was written; nothing remaining; zero boot attempts used.
Zero workflow reruns, zero failed-job reruns, zero replicas dispatched by hand.

**What was not re-sealed.** The fourth producer fingerprint. It pins the bytes
that ran, taken before they ran; six of its seven pins still match the live file
exactly. The seventh is the phase gate, which grew the tests describing this run
after the run produced them. That move is declared once, in this attempt's
result record, quoting the sealed digest and deliberately carrying no corrected
one — the file that moved is the file that reads the declaration, so a digest of
its current bytes would falsify itself the moment it was written.

**Artifacts expire.** The six artifacts are held for seven days and are not
committed to the repository. The record says so, because a record that lists
files without saying how long they last ages into a false statement.

### 49.1 Execution cursor after the fourth attempt

```text
SUCCESSOR-IMAGE-PRODUCTION  SPENT / PASSED — 1 dispatch, 2 replicas, byte-identical
SUCCESSOR-IMAGE-READBACK  PASSED — successor lock v2, 17,677 entries, all tracked paths match
SUCCESSOR-IMAGE-FSCK  PASSED — e2fsck -f -n exit 0, no repair, digest unchanged
SUCCESSOR-IMAGE-BUDGET  EXHAUSTED — 3 attempts spent in total, 0 remaining
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
MAC.4  NOT STARTED — does not begin automatically
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1, §47.1 and §48.1 are left as they were written; this one is
added beside them. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain
unchanged. An image exists and is fingerprinted. It has not been booted, no
guest has run, no node has been connected, and nothing is served from it. No
clean-Mac canary, VM boot, real release build, production key, public mining,
paid API benchmark, user installation or activation occurred.

## 50. Preserving what cannot be made again (2026-08-29)

The production budget is zero. The three files the fourth attempt produced
cannot be built again under any authority that currently exists, the GitHub
artifacts holding them expire on 2026-09-04, and the download that verified them
sat in a session scratchpad the operating system is free to clear. So a second
copy was made somewhere that survives a reboot, before anything else was
considered.

**What was copied, and how.** All six artifacts, both replicas, in full: 18
files, 7,739,896,486 bytes. Each file was written under a temporary name, its
size and SHA-256 compared with the source, and renamed into place only after
both matched. The whole tree was built beside its final path and renamed there
in one step, so the final path never existed in a half-copied state. Every
digest was then re-read at the final path, and re-read a third time after the
read-only mode was applied. Files are `0444`, directories `0555`, and a write
attempt into the archive is refused by the filesystem.

**Both replicas were kept whole.** Recording the second replica's digests would
have met the requirement, since the two sets are byte-identical and the identity
is proven twice over. Both were kept as bytes anyway. The budget is zero, these
are the only copies that will ever exist, and a second set on the same disk
costs 3.6 GB against a loss that cannot be undone.

**Nothing was deleted.** The GitHub artifacts are left to expire on their own
date rather than being cleared early, the scratchpad download stays where it is,
and every sealed record is byte-unchanged. Preservation that destroys its own
source is not preservation.

**The archive layout follows the artifacts.** One directory per GitHub artifact,
named as the artifact was, so an archived directory and an artifact id refer to
the same thing without a translation table. The archive also carries its own
copy of the preservation record, so that separated from this repository it still
explains what it holds.

**What is in the repository, and what is not.** Only the record: paths, digests,
sizes, artifact ids and expiry dates. No binary is committed. The archive root
is a machine-local path, so the gate's on-disk checks skip where the archive is
not present rather than failing — a runner has nothing truthful to say about a
disk it cannot see.

**The honest limit.** One copy on one internal disk in one machine is a single
point of failure. No offsite or removable copy exists and none is scheduled;
nothing re-reads these digests on a schedule, so silent corruption between now
and the next manual check would not be noticed; the archive is not encrypted.
The record says all of this in as many words, because a preservation record that
claims safety it does not have invites exactly the loss it was written to
prevent.

### 50.1 Execution cursor after preservation

```text
SUCCESSOR-IMAGE-PRODUCTION  SPENT / PASSED — 1 dispatch, 2 replicas, byte-identical
SUCCESSOR-IMAGE-PRESERVATION  DONE — 18 files, both replicas, read-only, off the expiry clock
SUCCESSOR-IMAGE-BUDGET  EXHAUSTED — 3 attempts spent in total, 0 remaining
BOOT-PASS-CRITERIA  NOT WRITTEN — the next gate, and it is written before any boot
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched, not authorised
MAC.4  NOT STARTED — does not begin automatically
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1, §47.1, §48.1 and §49.1 are left as they were written; this
one is added beside them. MAC.3 is not complete: the position is "a bootable
candidate image exists and is preserved", not "the guest boots".
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged. Nothing was booted, no
guest ran, no node was connected, and nothing is served.

## 51. Setting the exam before opening the room (2026-08-29)

The conditions a third closed-local boot would be judged by are now sealed, and
the boot is still not approved. Those are deliberately two things. A document
that set the exam and opened the run in the same breath would make the review it
was written for impossible to fail: by the time anyone disagreed with a
condition, the run would already be permitted. So
`native/containment/native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json`
carries `grantedByThisRecord: false`, and the one approval it waits for is a
separate instruction naming this record.

**Why a third exam at all.** The first attempt failed at PID 1 for missing mount
points. The second passed, and stopped short of the launcher because the image
did not carry the material the launcher looks for — so the launcher's refusal
there was a known limit rather than a failure, and nothing was graded on it. The
successor image was produced to put that material in: the account database, the
nested runtime rootfs with its content manifest, and a launcher unit whose output
reaches the console the host captures. With all three present, a refusal is a
failure. The three gaps are recorded as `stillAbsent: false`, each bound to the
production record or authority that closed it.

**Twenty-one conditions, a strict superset.** The six from the first attempt and
the nine frozen runtime-contract conditions are carried byte-for-byte out of the
records that froze them, along with the corrected fourth condition in its
corrected form — not retyped, because an exam that drifts by a word while looking
unchanged is the failure mode worth engineering against. Every condition is
either `carriedFrom` a named path, digest and field, or marked `newInThisRecord`
with a reason. Five are new. Thirteen operator requirements each name the
conditions that answer them, and the two conditions no requirement mentions are
listed as kept anyway rather than dropped.

**Read-only is not a security control.** The archive is `0444` on files and
`0555` on directories, owned by the account that made it. That refuses a stray
write and refuses nothing at all to its owner: a `chmod` is one command away and
no alarm follows it. Reading those modes as protection would be reading a
convenience as a guarantee. The record therefore says so in as many words, and
adds the condition that closes it — the three images and both copies of the
preservation manifest are hashed again at the moment of loading, and any mismatch
stops the run before a machine exists. The second replica is named as a recovery
source for some later approved run, explicitly not as a fallback inside this one:
loading it after the first failed its digest would turn a detected corruption
into a run that proceeded anyway.

**What a pass would still not establish.** MAC.4 is not started, no node is
connected, and no mining, reward, consensus or P2P authority is granted — and
that is a graded condition rather than a note, so a run that drifted into any of
it would fail rather than be described afterwards as having gone further than
planned. Exactly one boot is allowed, counted off the filesystem: the driver
refuses to start when the receipt named by `resultPath` already exists.

**The missing offsite copy.** It does not block this run — a closed-local boot
reads the archived files and writes nothing back to them — and it remains the
single point of failure the preservation record named. The record says both, and
marks it required before anything built on these images is distributed.

### 51.1 Execution cursor after the criteria were sealed

```text
SUCCESSOR-IMAGE-PRESERVATION  DONE — 18 files, both replicas, read-only, off the expiry clock
SUCCESSOR-IMAGE-BUDGET  EXHAUSTED — 3 attempts spent in total, 0 remaining
BOOT-PASS-CRITERIA  SEALED / NOT RUN — 21 conditions, 13 operator requirements, frozen before approval
BOOT-AUTHORISATION  NOT GRANTED — this record opens no run; one separate approval is required
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
MAC.4  NOT STARTED — excluded from what a pass would cover
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1 through §50.1 are left as they were written; this one is
added beside them. MAC.3 is not complete: the position is "a preserved candidate
image exists and the exam for it is written", not "the guest boots".
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged. Nothing was booted, no
guest ran, no node was connected, and nothing is served.

## 52. Refusing to spend the one boot (2026-08-29)

The criteria for the third attempt were sealed before the program that would
carry them out existed, which was the point: an exam written after the invigilator
is an exam the invigilator can shape. What that sealing then exposed was a gap
between the two wide enough to lose the attempt in. The runner the earlier
attempts used knows two attempt names and refuses a third. It carries six judging
rules for what are now twenty-one conditions, and meets the missing ones *after*
the machine has run — the shape that spends the single boot and ends in a
traceback with no verdict written. It writes its receipt into the scratch working
directory once the machine has stopped, so a Mac that dies mid-boot leaves no
record that the boot happened at all. And it re-hashes the root disk afterwards
and nothing else, out of three images and two manifest copies.

None of those is a defect in the criteria. Each is a way the run could have been
started, gone wrong, and left nothing behind — with the attempt gone.

**What was added.** An execution contract beside the sealed criteria,
`native/containment/native-shadow-mac3-closed-local-boot-execution-contract-arm64-v3.json`,
and a runner, `scripts/native_shadow_mac3_closed_local_boot_arm64_v3.py`. The
contract edits no condition and grants no authorisation; it binds the criteria at
their digest on disk and says, for each of the twenty-one conditions, which source
of evidence answers it. A new runner rather than an extension of the old one, so
the two gates the earlier attempts were judged by stay byte-unchanged.

**The order the run does things in.** The one-use mark is created with an
exclusive open, outside the working directory, *before* the machine starts. That
ordering is the whole reason the file exists: a receipt written afterwards is no
record at all if the host dies in the middle, because the boot happened and
nothing on disk says so, and the next run would start believing the attempt was
unused. A crash now costs the attempt rather than hiding it, and a wiped scratch
directory cannot buy a second one. The five archive targets — three images and
both copies of the preservation manifest — are hashed immediately before the
machine is configured and again after it stops, compared name by name, aborting on
any mismatch. Both manifest copies are in that set because the record that says
what the images should hash to is itself a file on a disk whose owner can change
it, and trusting it without re-reading it is trusting the last time anyone looked.

**Five conditions this image cannot answer.** `launcher-prerequisites-verify-inside-the-guest`,
`launcher-executable-matches-the-sealed-digest`,
`launcher-supervises-as-root-and-submissions-run-unprivileged`,
`readiness-and-clean-shutdown-are-observed` and
`no-host-wallet-model-key-or-node-secret-in-the-guest`. Four of them need the
launcher to speak, and the launcher in this image prints nothing on its success
path — a transcript without an error in it is silence, not a pass. The fifth asks
for the produced image to be searched for secret-bearing filenames; the sealed
read-back walks the paths the lock lists and compares those, so a file that is in
the image and not in the lock is exactly what it cannot see. Each is recorded with
what is missing and what would make it observable, and observability is a change
to the image and its evidence, never to the condition.

So the runner refuses before any machine is built. A condition with no readable
evidence has two honest outcomes — judged NOT MET, or the run does not happen —
and judging it NOT MET after booting spends the single attempt to produce a
failure that was knowable beforehand. This is not a verdict on the image: nothing
here says the boot would fail. It says five of the answers would be unreadable,
which is why the attempt is being kept rather than spent. The free preflight
gathers every blocker instead of stopping at the first, and reports zero machines
started and zero marks created as numbers to be read rather than assumed.

The runner departs from the sealed judging text in two places and both are
upward: it re-hashes five targets where one condition asks for the root disk, and
it requires the socket path and the working directory absent alongside the process
counts. Both are recorded in the contract as strengthenings rather than left to be
noticed.

### 52.1 Execution cursor after the runner was written

```text
BOOT-PASS-CRITERIA  SEALED / NOT RUN — 21 conditions, frozen before approval
BOOT-RUNNER  READY TO REFUSE / HARD STOP — 21 rules, 5 conditions unobservable with this image
BOOT-AUTHORISATION  NOT GRANTED — neither record opens the run
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
ONE-USE-MARK  NOT CLAIMED — nothing was started, so nothing was marked
MAC.4  NOT STARTED — named as what would make four of the five stops observable
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1 through §51.1 are left as they were written. MAC.3 is not
complete, and the position is now "the exam is written and the room is not ready"
rather than "the guest boots". `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain
unchanged. Nothing was booted, no guest ran, no node was connected, and nothing is
served.

## 53. Where the evidence would come from (2026-08-29)

The previous section reported the runner as ready to run. That was the wrong
word. What is ready is the refusal path and the twenty-one-row judging table:
the program can now say, before any machine is built, which conditions it could
judge and which it could not. The flow that actually boots, watches, judges and
seals a result is still unwritten. Nothing here changes that, and nothing here
opens the run.

Two things were done. The first is a correction. The one-use mark was created
with an exclusive open before the machine started, which settles two runs racing
each other and says nothing at all about power. Until now the mark could still
be sitting in the page cache when the Mac lost power, leaving exactly the shape
the mark exists to prevent: the machine started and nothing on disk says so. The
mark's bytes are now flushed and synced, and then the parent directory is synced
as well, both before the launcher is invoked. The directory is second on purpose
— bytes no name points at are not a record. On this platform each sync is
followed by `F_FULLFSYNC`, which waits for the drive's own cache rather than the
filesystem's; where that call is refused the ordinary sync still stands.

The second is the design of where evidence would come from for the five
conditions the preserved image cannot answer, split by what each one actually
needs.

One of the five needs nothing from inside the guest. It asks that the produced
image contain no host wallet, model key or node secret — a question about a file
sitting on this disk right now. It was in the hard stop only because nothing had
been written that asks it. That is now written: the image is opened read-only,
every byte is searched, and the file is hashed before and after so the record
shows the sealed file went in and the sealed file came out.

The search is deliberately wider than the condition. The condition asks for
directory entries; the search reads file contents and blocks no directory points
at any more. That asymmetry is the whole design, and it cuts one way only.
Nothing found would settle the question, because an empty superset makes the
subset empty too. Something found settles nothing on its own — a manual page
that mentions a filename is a hit and is not a secret. So a hit is neither a
pass nor a failure until someone explains it, and until then the answer is no.

The scan has been run against the sealed root disk, all 2,035,625,984 bytes of
it, digest unchanged on both sides. Nothing of this host's was found: zero hits
on the host's own home path, on the archive root, on the node's key, session and
signer-nonce directories, and on every one of the wallet and model-key
environment variable names. What did come back is 135 hits on generic
secret-bearing shapes — private-key headers, a credentials filename, mnemonic
and extended-private-key wording. None of those can only come from this machine,
and none of them is dismissed here. The condition therefore stays in the hard
stop, not because something of the host's was found, but because a byte search
cannot tell a manual page from a key file, and the honest reading of an
unexplained hit is no.

Three of the remaining four are properties of a running kernel: which bytes the
launcher actually is, which prerequisites resolve inside the guest, and which
capabilities the supervising process holds. No amount of reading the image from
outside observes any of them. They need a small service inside the guest that
reads kernel state directly and prints each answer as a structured record on the
serial console the host already captures. The fifth needs the host to ask for
shutdown once it has seen readiness and to confirm the shutdown completed, since
a machine that was killed and a machine that stopped cleanly look identical from
outside unless the host asked and was answered.

Both of those change the image. A service inside the guest is new bytes, so the
preserved fingerprint would no longer describe what runs, and a new clone, a new
fingerprint and pass criteria sealed again in advance all come first. The
preserved image is not modified and not booted.

One half of one condition is reported rather than decided. The condition asks
that the launcher supervise as root and that submissions run unprivileged. A
closed boot that never receives a request cannot observe the second half, because
there is no submission to watch. It can be bound statically, judging the
per-request setup order in the code path, or left to the stage where real
requests exist. Those two readings mean different things by the same criterion,
so the choice changes what the sealed condition asserts. It is the operator's
call. The condition is not relaxed, reworded or waived here.

### 53.1 Execution cursor after the evidence channels were designed

```text
BOOT-PASS-CRITERIA  SEALED / NOT RUN — 21 conditions, frozen before approval
BOOT-RUNNER  READY TO REFUSE / HARD STOP — 21 rules, 5 conditions unobservable with this image
ONE-USE-MARK  DURABLE / NOT CLAIMED — file and parent directory synced before any start
SECRET-ABSENCE-SCAN  RUN / NOT SETTLED — host-identity 0 hits, secret-shape 135 unexplained
GUEST-EVIDENCE-HELPER  DESIGNED / NOT BUILT — would change image bytes, so a new seal comes first
BOOT-AUTHORISATION  NOT GRANTED — no record here opens the run
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
UNPRIVILEGED-SUBMISSIONS  REPORTED / NOT DECIDED — static binding or MAC.4 is the operator's call
MAC.4  NOT STARTED — named as what would make the remaining stops observable
CURL.3  DEFERRED-ENVIRONMENT-NOT-AVAILABLE / NOT PASSED — release gate retained
MAC.5 / MAC.6  BLOCKED — CURL.3 and all intervening gates remain mandatory
```

The cursors in §46.1 through §52.1 are left as they were written. The position is
now "the exam is written, one answer sheet is readable and the room is not ready"
rather than "the guest boots". `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`,
`BF.7=HOLD`, Base activation `false` and `activationAllowed=false` remain
unchanged. Nothing was booted, no guest ran, no image was produced or modified,
no node was connected, and nothing is served.

## 54. The language the guest speaks on the console (2026-08-29)

Three of the five stopped conditions are properties of a running kernel, so the
guest has to say them out loud and the serial console is the only way out of a
closed-local boot. This section fixes what it may say and who says it. Nothing
here is built into an image, and nothing here opens the run.

A console is shared and line-interleaved. Kernel messages, systemd messages and
the guest's own records land in it together, so a record is one line carrying a
fixed prefix the host can find. A record split over two lines could be cut in
half by another writer between them, which is why none of them are.

What the host refuses to conclude matters more than the format. A record is an
observation and never a verdict: the guest reports the digest it computed, and
the host compares it with a value sealed before the machine existed. A guest
that reported the wrong digest fails its own condition, because the comparison
happens on the side that already knows the answer. Beyond that, three refusals
are wired in. A record appearing twice with different payloads is dropped rather
than settled by preferring the first or the last, since nothing in a transcript
says which line describes the run being judged. A record id the protocol does
not define is counted and ignored, so the reader cannot grow a new evidence
source because a console line asked it to. And a line that carries the prefix
and then does not parse is an error rather than a shrug, because silence about a
broken record is how a reader loses evidence quietly.

No record claims that submissions ran unprivileged. A closed boot receives no
requests, so there is no submission to watch, and printing the claim anyway
would be manufacturing the evidence rather than collecting it. That half stays
unobserved and is still the operator's to decide.

The producer is the launcher itself rather than a new service beside it. The
process that knows which file it executed is the process itself; an outside
observer would have to first decide which pid is the launcher, and a wrong guess
there produces a confident record about the wrong process. The launcher already
reads `/proc/thread-self/status` to verify it holds root identity, the expected
capability sets and NoNewPrivs, so the privilege record is that same reading
reported rather than only judged, and no second implementation of it can drift
from the first. The sealed execution contract had already named the launcher
printing its own digest as one of the two things that would make that condition
observable.

A separate helper was possible: the sealed rootfs source lock lists
`/usr/bin/python3.12` and the `python3` package, so one would have run. It is
still the wrong shape, for the reasons above. Recording that it was checked
rather than assumed is the point — the guest's userland was read out of the
sealed source list, not guessed at.

Teaching the launcher to print these records changes the launcher binary, and so
the launcher digest the criteria seal and the image bytes that carry it. A new
clone, a new fingerprint and pass criteria sealed again in advance all come
first. The host-side reader is written and tested; the guest-side producer is
decided and not written.

### 54.1 Execution cursor after the console language was fixed

```text
BOOT-PASS-CRITERIA  SEALED / NOT RUN — 21 conditions, frozen before approval
CONSOLE-EVIDENCE-FORMAT  DEFINED / READER BUILT — one line per record, host refuses to resolve conflicts
CONSOLE-EVIDENCE-PRODUCER  DECIDED / NOT WRITTEN — the launcher itself, not a separate service
GUEST-USERLAND  READ FROM THE SEALED SOURCE LOCK — python3.12 present, so no toolchain was guessed
BOOT-AUTHORISATION  NOT GRANTED — no record here opens the run
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
UNPRIVILEGED-SUBMISSIONS  REPORTED / NOT DECIDED — static binding or MAC.4 is the operator's call
```

The cursors in §46.1 through §53.1 are left as they were written. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged. Nothing was booted, no image was
produced or modified, no node was connected, and nothing is served.

## 55. Walking the flow before spending the attempt

The runner could refuse and it could judge, and between those two it stopped.
The steps that would build the host, start the machine, wait for it, shut it
down and gather what it left were never written, on purpose: a complete boot
path sitting behind a check is a boot path waiting for someone to delete the
check. That reasoning still holds and this slice does not overturn it.

What it adds is the same sequence with every collaborator handed in from
outside. There is no default host and no default way to claim the mark. A
caller who wants the flow to do anything has to supply the thing that does it,
and the only supplier in the repository is a stand-in that writes files instead
of starting anything. So the sequence can now be walked from end to end for
nothing, and the real run is still unwritten rather than merely guarded.

Twelve steps, in the order the run needs them: refuse what can be refused for
free, hash the archive, claim the one-use mark, start, wait for readiness, ask
for shutdown, confirm the machine is gone, hash the archive again, count what is
left on the host, assemble the evidence, judge the twenty-one conditions, write
the result. Two adjacencies carry most of the weight. The mark is claimed
before anything starts, so a Mac that dies mid-boot leaves a spent attempt that
says so on disk. And the shutdown is *requested* only after readiness was
actually seen — a guest that never got there is taken down rather than asked,
because there is nothing on the other end to ask politely.

The rehearsal earned its keep immediately. The host-side reader returns a pair —
whether readiness was seen, and why — and the first draft of the flow tested the
pair itself for truth. A pair is always truthy, so every stalled guest read as
ready, and the flow would have asked a dead machine for a graceful shutdown and
recorded that it obeyed. That is a defect that costs nothing here and would have
cost the only attempt there.

What a rehearsal cannot do is also fixed and tested rather than promised. It
claims no one-use mark. It writes nothing to the path a sealed result belongs
at, and the refusal is checked against the live path rather than by naming a
file. Everything it writes carries the word in the file name as well as inside
the file, and lists its own substitutions: there was no host, no guest, no
archive re-read and no real free-refusal pass. Running it with a healthy
stand-in still cannot reach a full pass, because the five conditions this image
cannot answer stay NOT MET — which is the point. A rehearsal that could produce
a pass would be a way to manufacture one.

### 55.1 Cursor

```text
BOOT-FLOW-REHEARSAL  WALKED / NO MACHINE STARTED — 12 steps end to end against a stand-in
BOOT-PASS-CRITERIA  SEALED / NOT RUN — 21 conditions, frozen before approval
CONSOLE-EVIDENCE-PRODUCER  DECIDED / NOT WRITTEN — the launcher itself, not a separate service
BOOT-AUTHORISATION  NOT GRANTED — no record here opens the run
GUEST-BOOT  NOT STARTED — 0 boot attempts used, boot budget untouched
UNPRIVILEGED-SUBMISSIONS  REPORTED / NOT DECIDED — static binding or MAC.4 is the operator's call
```

The cursors in §46.1 through §54.1 are left as they were written. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged. Nothing was booted, no image was
produced or modified, no node was connected, and nothing is served.

## 56. Keeping the old zero in the generation where it was measured (2026-08-29)

Section 34 measured `native_shadow_rootfs_builder_boot_arm64_v1.py` and was
right about that file: it mentioned neither fixed runtime path, and the image
of that generation did not contain them. The mistake was not in that record.
It was in applying its zero to the later preserved v4 image after a successor
producer and builder had been added. The historical record remains unchanged;
this addendum fixes the current interpretation.

The current production chain is different and is now checked end to end rather
than inferred from one leaf file. The v2 producer pins builder v3, constructs
the nested runtime tree, makes that tree a required argument of both preflight
and production, and forwards it to the staging assembler. Builder v3 merges it
before deriving parent directories. The v4 authority pins that builder and the
two guest paths, and production run `33202978318` records PASS, read-back PASS,
17,677 entries and no failed read-back check. The third qualification record
therefore correctly records the old runtime-rootfs gap as `stillAbsent: false`.

That establishes assembly, not execution. This correction did not parse the
preserved ext4 image itself, did not boot it and did not see the launcher open
or verify the manifest at runtime. The production result and qualification
records also keep runtime compatibility, guest boot and serving false. Those
boundaries are part of the correction gate so repairing the generation error
cannot silently turn into a serving claim.

### 56.1 Cursor

```text
RUNTIME-PATH GENERATION  CORRECTED — builder-v1 zero stays historical; v4 uses producer-v2 → builder-v3
V4 IMAGE ASSEMBLY  ESTABLISHED — nested tree + manifest bound, production/read-back PASS, 17,677 entries
PRESERVED-DISK PATH RECONCILIATION  NOT YET RUN — this correction did not inspect ext4 paths directly
LAUNCHER RUNTIME VERIFICATION  NOT MEASURED — no boot and no serving claim
SECRET-ABSENCE CONDITION  NOT SETTLED — 135 generic raw hits still need path/content reconciliation
CONSOLE-EVIDENCE PRODUCER  DECIDED / NOT IN A SEALED SUCCESSOR — launcher source seal remains intact
BOOT-AUTHORISATION  NOT GRANTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated host/guest transport claim
```

The cursors in §46.1 through §55.1 remain historical records. No image was
produced or modified, no boot was performed, and no production or boot budget
was opened. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged.

## 57. Keeping the raw scan as an inventory, not a joined-path proof (2026-08-29)

Section 53's measurement remains a measurement of the sealed bytes. The root
disk was opened read-only, all 2,035,625,984 bytes were read, its SHA-256 was
`51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c`
before and after, the host tier recorded zero raw matches and the generic
secret-shape tier recorded 135. The historical result also remained
`NOT-SETTLED`. None of those facts is edited or discarded.

The inference around joined paths was too broad. An ext4 path is a chain of
directory entries: a parent entry named `.boole` can point to a directory whose
child entry is named `keys` without the contiguous bytes `.boole/keys` appearing
anywhere in the image. A raw zero for a joined multi-component needle therefore
does not prove that the logical path is absent. Single-needle occurrences and
non-occurrences remain raw-byte facts; they do not become filesystem structure
or a condition verdict by themselves.

The raw tier name also did not prove provenance. A string such as
`BOOLE_LLM_API_KEY` or `boole-artifacts` may occur in source, documentation or
an ordinary guest file. A hit is still a candidate that must be reconciled, but
the bytes alone do not prove host origin or a secret leak. The old zero host-tier
count remains an exact raw-byte fact; it is not promoted into a filesystem or
provenance result.

The append-only correction is
`native/containment/native-shadow-mac3-guest-secret-absence-raw-scan-correction-arm64-v1.json`.
It binds the historical scanner, tests, result and evidence-channel design by
their unchanged digests; binds the exact production and preservation lineage of
the root disk; names the superseded joined-path assertions instead of erasing
them; and keeps the boot condition closed.

The successor must independently enumerate the filesystem graph, bind every
candidate path and content-bearing entry to an exact sealed expectation or an
approved local-generation recipe and digest, inspect symlink targets, journal
and unmapped bytes, and attribute every raw range to one physical inode owner.
Path aliases from hard links do not create multiple physical owners. Any
journal, slack, unallocated, unmapped or unreconciled hit remains fail-closed,
and no secret-surrounding bytes may be copied into the result.

### 57.1 Cursor

```text
RAW BYTE SCAN  PRESERVED — single-needle 0/135 occurrence facts unchanged
JOINED-PATH INFERENCE  FALSIFIED — ext4 stores path components as separate directory entries
RAW-HIT HOST ORIGIN  NOT PROVEN — a marker can occur in ordinary sealed guest content
SECRET-ABSENCE CONDITION  NOT-SETTLED — no waiver, rewording or boot authority
PRESERVED-DISK PATH/CONTENT RECONCILIATION  NEXT — exact graph, source binding and physical ownership
IMAGE PRODUCTION  NOT RUN BY THIS CORRECTION — preserved image unchanged
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated host/guest transport claim
```

The cursors through §56.1 remain historical records. This correction did not
produce, modify, mount or boot an image and grants no execution authority.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged.

## 58. Reconciling every byte without hiding the producer build path (2026-08-29)

The preserved replica-one root disk was parsed directly and read-only. Its
2,035,625,984 bytes had SHA-256
`51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c`
both before and after. The result is
`native/containment/native-shadow-mac3-guest-secret-path-content-reconciliation-arm64-v1.json`,
59,038 bytes with SHA-256
`5d39cac4d9dd77ff7a34bed9cf24ca01c9e829e7ec443174c4bcd06531dcd838`.
A second complete run produced the same result byte-for-byte.

The logical tree is exact: 17,677 paths, comprising 1,738 directories, 15,102
regular files and 837 symlinks. Every regular file was hashed against the
sealed source or sealed launcher expectation: 1,773,456,499 bytes in all. Every
symlink target matched. There were no forbidden paths, forbidden targets,
inline xattr tails or external xattr blocks.

The physical account closes too. All 435,530 allocated blocks have exactly one
declared role: 422,636 file-data, 1,766 directory-data, 7 symlink-data, 8,192
journal, 24 extent-metadata, 1,440 allocation-metadata, 12 super/GDT and 1,453
resize-metadata blocks. The historical 135 generic raw hits are conserved into
135 exact logical-file occurrences, 50 inodes and 96 physical blocks. There are
zero journal, directory, symlink, slack, free, metadata, unmapped or ambiguous
hits. No secret-surrounding bytes are copied into the record.

That reconciliation is a PASS, but the qualification condition is not. The
actual producer home derived from the sealed preflight was searched in addition
to the historical local marker. Its spelling occurs 23 times in one exact
sealed file, `usr/libexec/boole/boole-native-shadow-launcher`, all as compilation
provenance embedded by Cargo. They are not wallet material, a model key or a
node secret. They do, however, violate the existing literal requirement that no
host path be present. This document does not reinterpret that requirement into
"no secret host path" after seeing the result. The condition therefore remains
NOT-SETTLED.

The next safe step is a launcher-v2 successor that removes producer build-home
paths reproducibly and carries the already designed console evidence plus
dynamic privilege measurement. That successor needs its own source seal and two
independent byte-identical builds before any image decision. This section grants
no image-production or boot budget and starts no MAC.4 transport.

### 58.1 Cursor

```text
PRESERVED EXT4 RECONCILIATION  COMPLETE — path, kind, content and every allocated block accounted
HISTORICAL RAW HITS  135/135 ATTRIBUTED — journal/slack/free/unmapped/ambiguous hits all zero
SECRET MATERIAL OBSERVATION  0 — no host wallet, model key or node-secret material observed
PRODUCER BUILD PATH  23 ATTRIBUTED / BLOCKING — sealed launcher compilation provenance
SECRET-ABSENCE CONDITION  NOT-SETTLED — literal no-host-path criterion preserved
LAUNCHER V2 PATH REMAP  NEXT — source seal, deterministic rebuild, evidence producer and privilege tests
IMAGE PRODUCTION  NOT AUTHORISED — no successor production budget opened
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated host/guest transport claim
```

The cursors through §57.1 remain historical records. The preserved image was not
modified or mounted, no image was produced, and no machine was started.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged.

## 59. Freezing launcher v2 before asking arm64 CI for bytes (2026-08-30)

The 23 producer-home strings in the preserved launcher are all Cargo dependency
source paths. The historical v1 build remapped its temporary workspace but not
Cargo's source store. The successor therefore uses two explicit remaps: the
temporary source root becomes `/boole/launcher-build`, and a fresh Cargo home
owned by each build becomes `/boole/cargo-home`. The produced ELF is refused if
it still contains the real source root, Cargo home, repository root, ambient
home or Rustup home. Only path-class names and zero counts may enter a result.

The v1 source and authority remain byte-unchanged. V2 is a complete-file overlay
applied only to a temporary `git archive` export. It carries the parked console
evidence producer, the exact `resolved` prerequisite schema, the complete root
supervisor snapshot and the table-driven post-drop child check. Every UID/GID
slot, supplementary group set, five capability sets and `NoNewPrivs` value is
checked. A closed boot still observes no submission, so this source-level matrix
does not pretend that a real answer was executed.

The build authority requires Linux arm64, Rust 1.95.0, a fresh Cargo home for
each of exactly two builds, offline compilation after prefetch, byte-identical
ELFs, no timestamp suppression and no binary post-processing. The named v2 CI
job is a separate required predecessor of `self-test`; it does not replace the
historical v1 job.

The first review found two false-green windows and closed them before any arm64
candidate existed. The console records are no longer emitted by the entry point
before the socket exists. The v2-only active-execution overlay first binds and
verifies the fixed socket's owner, group, mode, inode and `CLOEXEC`, then emits
and flushes exactly four records, and only then accepts the qualification
connection. A bind failure therefore emits no readiness; an evidence-write
failure removes the exact bound socket and returns a typed error.

The first double build may discover candidate bytes, but discovery is not a
seal. CI prints the candidate JSON and digest, then deliberately fails unless
that exact result path is already tracked by Git. Only a later run that
byte-for-byte re-proves the reviewed, tracked record may turn the required job
green. The host parser also checks JSON types rather than Python value aliases:
booleans cannot stand in for integer UID/GID or `NoNewPrivs` fields, integers
cannot stand in for prerequisite booleans, and any malformed prefixed record
invalidates the transcript. Unknown record IDs retain the v1 rule: counted and
ignored, not silently promoted into evidence.

The earlier host reader had a real schema defect: the sealed prose and parked
producer used `resolved`, while the reader accepted `present`. The append-only
correction makes `resolved` the only accepted boolean and rejects missing,
extra, duplicate and `present`-only rows. No pass condition is changed.

This section freezes source, protocol and build inputs only. The arm64 result
does not exist yet, no image-production or boot authority is granted, and no
MAC.4 request is started.

### 59.1 Cursor

```text
LAUNCHER V2 SOURCE OVERLAY  FROZEN — historical v1 source remains byte-unchanged
CONSOLE EVIDENCE SCHEMA  CORRECTED / FROZEN — resolved is the sole prerequisite field
READINESS ORDER  BIND-VERIFIED BEFORE EMIT — a failed listener cannot announce ready
DROP FAILURE MATRIX  GREEN AT UNIT LEVEL — real submission execution not yet observed
ARM64 V2 DOUBLE BUILD  PENDING CI — no launcher-v2 result sealed yet
SUCCESSOR IMAGE PRODUCTION  NOT AUTHORISED — no production budget opened
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated host/guest transport claim
```

The cursors through §58.1 remain historical records. No preserved image was
modified, no image was produced, and no machine was started. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

## 60. Tracking the first arm64 launcher-v2 candidate before reproof (2026-08-30)

The first named Linux/arm64 build run reached the deliberate discovery gate.
Both independent exports ran the overlay tests and produced byte-identical
2,025,192-byte ELFs with SHA-256
`53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd`.
All five real producer-path classes were absent. The candidate record itself is
canonical JSON with SHA-256
`0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08`.

That first run was red by design. It printed the complete candidate and then
refused it because the result path was not tracked by Git. This section places
those exact reviewed bytes at the authority's result path; it does not turn
the discovery run green after the fact. The record may enter `main` only if a
new required arm64 job performs two fresh builds and `seal_or_reprove` finds
the tracked bytes exactly equal. Any difference is a hard refusal and the seal
is never overwritten.

The result keeps every later boundary false. It is a launcher build result, not
an image-production authority, image, boot result, serving result or MAC.4
transport claim.

### 60.1 Cursor

```text
LAUNCHER V2 SOURCE OVERLAY  FROZEN — historical v1 bytes remain unchanged
ARM64 V2 CANDIDATE  TRACKED — first discovery run intentionally red
ARM64 V2 REPROOF  REQUIRED BEFORE MERGE — exact bytes or refusal
SUCCESSOR IMAGE PRODUCTION  NOT AUTHORISED — no production budget opened
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated transport claim
```

The cursors through §59.1 remain historical. No preserved image was modified,
no image was produced and no machine was started. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

## 61. Rebuilding sealed launcher-v2 bytes without opening an image boundary (2026-08-30)

The tracked launcher-v2 result is now consumed by a separate fail-closed
emitter. The emitter pins the canonical result document by its SHA-256 rather
than restating the ELF digest. It reconstructs the complete result through the
v2 build authority, requires every declared producer-path counter and no extra
counter, and requires all counters to remain zero.

One fresh Linux/arm64 build is then compared with the tracked size and digest.
Wrong or empty bytes are never written. Publication uses a new file opened with
exclusive creation, flushes complete executable bytes, and exposes the final
name with a hard link that cannot overwrite a concurrent path. Existing final
paths, dangling symlinks and stale temporary paths are all refusals. CI runs
this emitter only after the named v2 job has performed its two-build reproof
and confirmed that the result file is tracked.
The public emitter accepts only an output path: a caller cannot substitute a
builder or an alternate seal to bypass the arm64 host and path-scan checks.

The emitted ELF remains in the CI runner's temporary directory. No successor
image producer imports this emitter in this slice, no guest tree is changed,
and no production or boot budget exists. A later image generation needs a new
authority that pins this emitter and the v2 result; this section does not create
that authority implicitly.

### 61.1 Cursor

```text
ARM64 V2 DOUBLE BUILD  REPROVED BY REQUIRED CI — tracked result must match exactly
LAUNCHER V2 EMITTER  CI-GATED — one additional build, exact seal or no output
ATOMIC NO-OVERWRITE PUBLICATION  GREEN AT UNIT LEVEL — real arm64 run required before merge
SUCCESSOR IMAGE PRODUCER WIRING  NOT STARTED — no authority or budget opened
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4  NOT STARTED — no node request or authenticated transport claim
```

The cursors through §60.1 remain historical. No preserved image was modified,
no image was produced and no machine was started. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

## 62. Registering launcher-v2 image integration before wiring it (2026-08-30)

The launcher-v2 emitter was exercised by the required Linux/arm64 job on PR
#301 after the tracked two-build result was re-proved. The third build emitted
2,025,192 bytes at SHA-256
`53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd`,
and the CI shell independently compared both values with the tracked result.
The emitter result is therefore established as bytes a later free preflight may
consume; it is still not an image-production authority.

The next generation is pre-registered in
`native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json`.
It binds the v2 source, authority, result, evidence protocol and emitter; the
existing source lock, nested runtime tree, account database, service unit,
builder and release gate; and the historical v4 result, preservation and boot
criteria. The old v4 producer and workflow remain the files that explain the
past run. Their known post-run test-gate drift is preserved from the v4 result
rather than denied or re-sealed.

The launcher is the only projected content change. Its guest path is unchanged,
its size grows by 18,560 bytes, the staging entry count remains 17,676 and the
projected payload becomes 1,773,475,059 bytes. The path, kind, mode, UID and GID
remain fixed. All three frozen limits still pass. These values are re-derived by
the gate from the source lock, staging measurement and both launcher results;
the record is not trusted as a second calculator.

One more boundary was found before wiring: builder v3 still carries launcher
v1's size and digest and correctly refuses v2 bytes. A new builder projection is
therefore mandatory before a new producer or workflow. It must pin its
predecessor by digest, accept v2 and refuse v1 without changing the guest path,
and expose the same assembly function to the repeatable preflight and any later
production. A global monkeypatch is forbidden. This record implements none of
those files and grants zero image-production runs.

### 62.1 Cursor

```text
LAUNCHER V2 EMITTER ARM64 PROOF  GREEN — tracked result reproof + third exact build
LAUNCHER V2 IMAGE INTEGRATION  PRE-REGISTERED / AUTHORITY 0
SUCCESSOR BUILDER PROJECTION  NEXT — v2 accepted, v1 refused, same guest path
FREE ARM64 PREFLIGHT  NOT IMPLEMENTED — no marker, image tool or output allowed
IMAGE PRODUCTION  NOT AUTHORISED — 0 runs granted
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The cursors through §61.1 remain historical. No image tool ran, no attempt mark
or output directory was created, no preserved image was changed and no machine
was started. `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged.

## 63. Proving launcher-v2 staging on Linux/arm64 without creating an image (2026-08-30)

<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:BEGIN -->

PR #303 head `6e95d5a73a17dda26adb006cd2c0de5129a1921d` ran the new required
`native-shadow-launcher-v2-image-preflight-arm64` lane on Linux arm64. All
required checks passed and merge commit
`6a14563ad078578147ac7bcd99b15e2a760e9930` entered `main`. The named job
uploaded one artifact containing only `PREFLIGHT-RESULT.json`; it contained no
kernel, initrd, root disk, attempt mark or other image output.

The exact canonical payload is tracked as
`native/containment/native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json`.
The payload and GitHub's compressed transport envelope are deliberately kept
as two different identities:

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

In human-readable units, the archive is 3,079 bytes and the raw result is
9,409 bytes. The archive digest describes GitHub's packaging and is never used
in place of the raw result digest.

Builder v4 pins builder v3 by digest, accepts the sealed launcher-v2 bytes,
refuses launcher v1 and preserves the launcher path, mode, UID and GID. The
free preflight used that same assembler under the production isolation shape,
but had no image tool, boot entry point or attempt-consumption path.

The builder table and independent traversal agreed exactly: 17,676 entries —
1,737 directories, 15,102 files and 837 symlinks — with payload
1,773,475,059 bytes. The largest file was 160,096,808 bytes and both
observations produced path-manifest SHA-256
`0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a`.
Duplicate paths, collisions and symlink escapes were all zero. The nested
content manifest remained at SHA-256
`200f025756d4c83e15a306feac982a91aa6130979665d0265c33aee95f3987aa`.

The result re-read all 22 preregistered inputs, recorded eight repository-file
identities and the actual `gpgv` and `zstd` tool identities, and was consumed
again before upload. Its status is `PASS-NO-IMAGE-PRODUCED`, it is repeatable,
`imageProduced=false`, `bootableClaim=false`, `activationAllowed=false`, every
activation authority is false and `imageProductionRunsAllowed=0`.

This result proves staging compatibility only. It does not authorise a producer,
consume a production run, qualify an image, start a guest, or open MAC.4,
testnet, mining, reward, consensus or P2P.

### 63.1 Cursor

```text
LAUNCHER V2 BUILDER PROJECTION  GREEN — predecessor pinned, v2 accepted, v1 refused
FREE ARM64 PREFLIGHT  GREEN / RESULT SEALED — one canonical JSON, no image output
STAGING MEASUREMENTS  EXACTLY EQUAL — 17,676 entries / 1,773,475,059 bytes
SUCCESSOR PRODUCER PREREGISTRATION  NEXT — contract only, authority and runs remain 0
IMAGE PRODUCTION  NOT AUTHORISED — no one-use budget opened
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The cursors through §62.1 remain historical. No image tool ran, no attempt mark
or output was created, no preserved image changed and no machine started.
`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged.

<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:END -->

## 64. Registering the launcher-v2 producer generation before implementing it (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:BEGIN -->

The exact no-image arm64 preflight result now feeds a separate preregistration
for the next producer generation. The canonical record is
`native-shadow-mac3-launcher-v2-successor-producer-preregistration-arm64-v1.json`
at SHA-256
`576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec`
and 20,145 bytes. It binds 23 live inputs, including the raw preflight result
and the three directly executed measurement/read-back helpers,
launcher-v2 seal, builder v4, source-lock v2, nested runtime lock and the
historical production and preservation evidence.

This record freezes 11 future names before implementation: a new producer v3,
wrapper, workflow and gates; readback v3 and its gate; a producer fingerprint,
free-rehearsal result, future one-use authority and future production result.
The historical producer, wrapper, workflow and readback remain byte-bound to
their own past. Their exhausted authority cannot be transferred to the new
generation, and the already declared producer-v2 test-gate drift is retained
rather than hidden by a re-seal.

The future free rehearsal may retain one canonical JSON result only. It must
use the same producer and assembler as a later separately authorised run while
creating no output directory, attempt mark, kernel, initrd, root disk or other
production output. Calling the production entry without authority must refuse
before any marker or output path exists.

Readback v3 must bind source-lock v2 and the launcher-v2 result from repository
bytes before loop-device setup or mount. CLI, environment and image-provided
overrides are forbidden; launcher v1 is an explicit refusal. A diagnostic
failure remains `UNQUALIFIED-DIAGNOSTIC`, and the wrapper may call only the v3
reader. Its mount retains `ro,nodev,noexec,nosuid`; unmount and loop-device
cleanup run in `finally`, and cleanup failure is a hard stop. Any future
authority must bind this preregistration, the free-rehearsal result and a
producer fingerprint covering the exact seven generation files. The
fingerprint never binds future authority bytes, so the direction is acyclic.

### 64.1 Cursor

```text
FREE ARM64 LAUNCHER-V2 IMAGE PREFLIGHT  GREEN / RESULT SEALED
SUCCESSOR PRODUCER + READBACK-V3  PRE-REGISTERED / AUTHORITY 0
NEW-GENERATION PRODUCER + READBACK-V3  NEXT — implementation, then free rehearsal
IMAGE PRODUCTION  NOT AUTHORISED — 0 runs granted or performed
GUEST BOOT  NOT STARTED — 0 boot attempts used
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The cursors through §63.1 remain historical. No future producer file is
implemented by this record, no image tool ran, no output or attempt marker was
created, no preserved image changed and no machine started. `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-PREREGISTRATION-ARM64-V1-FROZEN:END -->

## 65. Correcting the preregistered repository-Python trust closure before implementation (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:BEGIN -->

The 23-row preregistration in §64 is preserved byte-for-byte as the historical
decision made before implementation. A fresh import audit then showed that its
hand-selected rows did not cover the complete repository-Python trust closure.
Recursive static import analysis found 16 missing helper modules. A fresh
process additionally observed one module imported only from projected code and
one policy JSON read while a builder module is imported. Neither is visible to
the static import walk.

The append-only correction is
`native-shadow-mac3-launcher-v2-successor-producer-import-closure-correction-arm64-v1.json`,
SHA-256
`b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498`,
10,971 bytes. It adds exactly 18 direct bindings, so future implementation must
verify the union of the historical 23 and corrected 18 — 41 unique files —
before importing any repository Python helper. The runtime audit also observed
three legacy source files that projected wrappers read and execute; each live
file is already checked against a literal SHA-256 by a now-directly-bound
wrapper before decode, compile or execution. Those three transitive checks are
listed explicitly rather than double-counted as direct rows.

This correction does not rewrite §64, grant a production or boot run, or claim
that producer v3 or readback v3 exists. It closes a trust-boundary omission
before either implementation or rehearsal can execute.

### 65.1 Cursor

```text
HISTORICAL PREREGISTRATION  PRESERVED — 23 rows, byte identity unchanged
IMPORT-TIME TRUST CLOSURE  CORRECTED — 18 direct rows added
EFFECTIVE DIRECT BINDINGS  41 — verify before repository-Python import
SUCCESSOR PRODUCER + READBACK-V3  NEXT — implementation, then free rehearsal
IMAGE PRODUCTION / GUEST BOOT  NOT AUTHORISED — 0 runs
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

`mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation
`false` and `activationAllowed=false` remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-IMPORT-CLOSURE-CORRECTION-ARM64-V1:END -->

## 66. Implementing the authority-zero successor producer and readback generation (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:BEGIN -->

The preregistered S3-B generation now exists as seven new files: producer v3,
readback v3, their strict wrapper and manual workflow, and three focused gates.
Before the wrapper invokes repository Python, its stdlib-only verifier checks
the historical 23-row record, the append-only 18-row correction and all 41
live direct inputs by path, size and SHA-256. The producer and reader repeat
the same two-record checks before their own effect boundaries. Every Python
process in the wrapper and workflow uses isolated startup (`-I -S`), and the
workflow calls a verify-only wrapper edge before its first acquisition or
launcher-emitter module, so ambient `sitecustomize` or `PYTHONPATH` code cannot
run ahead of that boundary.

The repeatable rehearsal and any future separately authorised producer share
one staging orchestration. Rehearsal accepts no output directory, attempt
marker or image surface, preserves an exhaustively observed scratch tree and
may publish exactly one create-once canonical JSON result. The current
production command is intentionally red: with authority and run count still
zero, it stops before command discovery, input preparation, scratch creation,
assembly or any image effect.

Readback v3 fixes the v2 source lock and launcher-v2 result before loop setup,
uses a read-only loop and `ro,nodev,noexec,nosuid` mount, checks all three output
file identities before and after readback and again after tree verification,
keeps their exact inodes open through the verdict, and gives `losetup` the
already-open root-disk descriptor rather than a replaceable pathname. Its sole
promotion gate requires the exact result shape, fixed generation bindings,
observed image identity and observed entry count. Qualified bytes are first
written to a private candidate, all three retained outputs are rechecked, and
only then is the candidate hard-linked create-once to the public result name;
drift leaves only an unqualified diagnostic. It writes fixed create-once pass
or unqualified documents and treats cleanup failure as a hard stop. Its wrapper
edge is deliberately declared but unreachable in this authority-zero slice;
no image exists for this generation and no readback or qualification was run.

### 66.1 Cursor

```text
S3-B IMPLEMENTATION  GREEN / ARM64 REHEARSAL NOT YET RUN
EFFECTIVE DIRECT BINDINGS  41 — wrapper verifies before repository-Python import
FREE NO-IMAGE REHEARSAL  NEXT — repeatable arm64 CI, one JSON only
READBACK-V3  IMPLEMENTED / UNREACHABLE — future qualified production only
IMAGE PRODUCTION / GUEST BOOT  NOT AUTHORISED — 0 runs
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

This implementation changes no preserved image, grants no one-use authority
and makes no bootability or serving claim. `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-S3B-AUTHORITY-ZERO:END -->

## 67. Sealing the real authority-zero arm64 rehearsal (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:BEGIN -->

The repeatable launcher-v2 successor rehearsal ran once on required native
Linux arm64 after PR #307 merged.  The production job was skipped.  The sole
artifact member is tracked byte-for-byte as
`native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v1.json`.
It is 10,168 bytes at SHA-256
`d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c`.

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

The payload independently rebinds the historical 23 rows and append-only 18
row correction to 41 live regular files.  Its two measurements agree on
17,676 entries and 1,773,475,059 payload bytes.  It reports one canonical JSON
member, zero image-effect calls, zero image files, zero production output
directories, zero attempt markers and an unchanged empty scratch tree.  The
workflow acquired frozen inputs over the network, so this is not an offline
claim.  It also does not claim absence of every runner-global transient; its
evidence is limited to the named rehearsal boundary.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-ARM64-V1-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V5-SEALED -->

The post-run fingerprint
`native-shadow-mac3-successor-producer-fingerprint-arm64-v5.json` is 5,458
bytes at SHA-256
`6ca75d732d7d3a064659047d33cb6bf7aaae9b5b01a5ad67754a843093d4f7aa`.
It seals the exact seven v3 generation files plus P1, C1 and the raw rehearsal
result.  It is historical authority-zero staging evidence only.  Readback v3
was sealed but not executed by the rehearsal, and the record does not establish
production readiness, an image, bootability, serving or MAC.4.

### 67.1 Cursor

```text
V3 FREE ARM64 REHEARSAL  GREEN / ONE CANONICAL JSON SEALED
V3 PRODUCER FINGERPRINT F5  SEALED / HISTORICAL AUTHORITY-ZERO EVIDENCE
READBACK-V3  BYTE-SEALED / NOT EXECUTED BY THE REHEARSAL
IMAGE PRODUCTION / GUEST BOOT  NOT AUTHORISED — 0 runs
PRODUCTION-ONLY SUCCESSOR GENERATION  NEXT — append-only preregistration
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The cursors through §66.1 remain historical.  No image or attempt marker was
created, no preserved image changed and no guest started.  `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

## 68. Pre-registering the separate production-only generation (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCTION-GENERATION-PREREGISTRATION-ARM64-V1-FROZEN -->

The canonical P2 record is
`native-shadow-mac3-launcher-v2-successor-production-generation-preregistration-arm64-v1.json`,
8,096 bytes at SHA-256
`4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95`.
It binds the raw R1 result and F5 historical fingerprint, then traverses F5 to
the byte-preserved P1, C1 and R1 records.  The one v3 free rehearsal remains an
observed historical run; P2 itself performs and permits zero rehearsals,
productions and boots.

P1 had reserved authority-v5 and result-v5 names before v3 existed.  Neither
path was ever created and no authority was granted.  P2 withdraws those unused
reservations for production, requires both paths to remain absent even as
dangling symbolic links, and forbids reusing them.  F5 is not withdrawn: it
remains immutable authority-zero rehearsal evidence and cannot become F6 or
authorise production.

The actual production-only namespace is producer generation v4 with fresh R2,
F6, A6 and result-v6 records.  Five new executable or gate files may be
implemented; they must pin and reuse the already proved `prepare_staging`
implementation plus readback-v3 and its security gate.  A fresh no-image R2 of
the exact v4 bytes is mandatory before F6 or A6 may exist.  The declared digest
graph points only from a later binder to already existing predecessors, so no
record stores its own or a future record's digest.

### 68.1 Cursor

```text
V3 FREE REHEARSAL R1  GREEN / RAW RESULT SEALED
V3 FINGERPRINT F5  HISTORICAL AUTHORITY-ZERO EVIDENCE / NOT PRODUCTION READY
UNUSED A5 / RESULT-V5 RESERVATIONS  WITHDRAWN UNUSED / MUST REMAIN ABSENT
P2 PRODUCTION GENERATION  PRE-REGISTERED / AUTHORITY 0
PRODUCTION-GENERATION V4 + R2  NEXT
F6 / A6 / RESULT-V6  NOT CREATED
IMAGE PRODUCTION / GUEST BOOT  NOT AUTHORISED — 0 runs
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

This record implements no v4 file and grants no run.  `mineable_now=0`,
`REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`, Base activation `false` and
`activationAllowed=false` remain unchanged.

## 69. Production-dispatch fence correction (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCTION-DISPATCH-FENCE-CORRECTION-ARM64-V1-FROZEN -->

The preceding §68 text says that P2 is 8,096 bytes.  That historical prose is
wrong and remains visible as the reason for this append-only correction.  The
byte-preserved P2 file is exactly **8,156 bytes** at SHA-256
`4c801a52d4c6d47dbbc1c9a7657eb8bce215f9f258586b97064359caefd28a95`.
The correction record is 7,295 bytes at SHA-256
`16f15bd7b9fcddeb02e104a3628d218817b047a3927fdfd77983ffaf0760910b`.

The second correction is behavioural.  A future A6 field saying
`workflowDispatchesAllowed=1` is a declaration, not a globally durable claim
that the single dispatch has been consumed.  A runner-local marker, uploaded
artifact, cache, absent result-v6 path, concurrency group or human promise
cannot replace that claim.  Before any effect, only the
`production-authority-guard` job may receive job-level `contents: write`; it
must require `github.run_attempt == 1` and atomically create one fixed,
attempt-specific annotated Git tag ref without force.  An existing ref is a
hard stop.  Its canonical message binds the live A6 digest, attempt ID, GitHub
run ID, workflow path and head SHA.  Successful ref creation is the exact run
consumption moment.  Deleting, updating or reusing that ref is forbidden.

Every replica must re-read the ref and annotated tag message, match the live A6
and checked-out head, and do so before dependency acquisition, scratch,
attempt-marker, assembly or image effects.  R2, F6 and A6 must each directly
bind the correction through the exact `productionDispatchFenceCorrection`
field (`path`, `sha256`, `sizeBytes` only); a transitive reference is not
accepted.

This record creates no tag, permits no dispatch or image production, and runs
no rehearsal or boot.  V4, R2, F6, A6 and result-v6 remain unexecuted or absent;
MAC.4, testnet, mining, reward, consensus, P2P and activation remain closed.

## 70. Implementing producer generation v4 before its fresh rehearsal (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:BEGIN -->

The exact five paths pre-registered by P2 now implement producer generation
v4.  The free path runs the v4 producer from a root-owned, byte-verified Git
anchor on required Linux arm64 and may retain exactly one canonical
`R2-RESULT.json` for seven days.  It has no writer-set acquisition, image tool,
production or image output directory, attempt-consumed marker or boot edge.

The production path is present but remains unreachable because fresh R2, F6
and A6 do not yet exist.  If a later A6 is separately sealed, the sole guard
job must create one non-forced annotated Git tag before any dependency work.
Each replica and the comparison job then fetch and independently verify that
exact tag and live A6 binding.  Only the guard has `contents: write`; the token
is confined to the create/fetch steps and is removed before repository code
validates the claim.

Each future replica runs its wrapper and direct children under one claim-bound
systemd supervisor with control-group killing, fixed wall-clock bounds and
resource accounting.  Preflight, production and qualification are three
separate claim-named transient services, each with its own cgroup and bounds.
A failed wrapper supervisor invokes exactly one different claim-bound cleanup
supervisor, which stops and verifies all three inner services before reading
recovery state.  If cleanup succeeds the original production failure is
returned; if cleanup also fails both statuses are reported and the cleanup
failure is returned.  Only qualified, sealed output plus its logical provenance
may be retained for seven days.  The comparison checks both provenance
envelopes before byte-comparing kernel, initrd and root disk.  These are
implementation contracts, not evidence that production ran.

The required `native-shadow-containment-linux` job also exercises this exact
lifecycle with disposable real systemd units: it crashes the outer wrapper
unit, runs the separate cleanup unit, and requires the outer, cleanup and all
three inner unit/cgroup identities to be absent.  It creates no image and does
not consume production authority.

### 70.1 Cursor

```text
V4 IMPLEMENTATION  GREEN / R2 NOT YET RUN
FREE ARM64 R2  NEXT — one canonical JSON, no image
F6 / A6 / RESULT-V6  NOT CREATED
PRODUCTION TAG / IMAGE / BOOT  NOT CREATED OR RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

No tag, production dispatch, image, boot or node connection was created in
this slice.  `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation `false` and `activationAllowed=false` remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V4-IMPLEMENTED-R2-PENDING:END -->

## 71. Two failed free R2 rehearsals, preserved separately (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-FAILED-ATTEMPTS-ARM64-V2-SEALED:BEGIN -->

The first two authority-zero v4 rehearsal dispatches failed before a canonical
R2 result existed.  Their append-only hard-stop record is
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-hard-stop-arm64-v2.json`,
8,120 bytes at SHA-256
`7a8cf17bfedfbe424978f41b38314d6ba5304a36df49a566a60ff4e5114328eb`.
It is deliberately separate from the still-unoccupied successful R2 path.

```text
sourceRunId=33311411461
sourceRunId=33313895353
artifactsUploadedByTheseAttempts=0
successfulR2ResultsCreatedByTheseAttempts=0
productionGuardJobs=skipped
imageProductionClaim=false
bootClaim=false
```

The first dispatch stopped because systemd rejected its address-family
property before the core service started.  The second reached systemd but the
direct program bootstrap lacked the preregistered generation binding.  Later
changes repair both defects, but R2 remains unsealed by these two attempts.
Dependency and toolchain preparation did occur, so this record makes no
network-zero or complete-cleanup claim.  It grants no A6, production, image,
boot, MAC.4, testnet, mining, reward, consensus, P2P or activation authority.

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

## 72. Fourth free R2 rehearsal hard stop (2026-08-30)

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

## 73. Fresh authority-zero R2 succeeded; production remains closed (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-SUCCESS-ARM64-V2-SEALED:BEGIN -->

The exact v4 generation at main SHA
`05ebf22e220bcece9e3104238f8ae8e2fef02a3e` completed one fresh Linux-arm64
free rehearsal in run `33321624511`.  Its sole raw result is
`native/containment/native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-arm64-v2.json`,
6,928 bytes at SHA-256
`7efe89c3bc558455313b76de2a625e708a580d0256760692914e9474eb0171f0`.
It is canonical JSON with status `PASS-NO-IMAGE-PRODUCED`.  The production
guard, producer and comparison jobs were skipped.  The raw result records zero
attempt markers, image outputs, production outputs and boot attempts; every
production, boot, MAC.4, testnet, mining, reward, consensus and P2P authority
remains false or zero.

The run acquired its already sealed dependencies over the network, so this is
not an offline claim.  It also does not claim that unnamed runner-global
transient files were absent.  The four earlier failed rehearsals and their two
scope corrections remain byte-preserved as separate historical evidence.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-SUCCESS-ARM64-V2-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-ARTIFACT-PROVENANCE-ARM64-V2-SEALED:BEGIN -->

The raw result intentionally contains no GitHub wrapper metadata.  Its separate
3,181-byte transport provenance record at SHA-256
`6d569cdf8c875d0835df64d38aacd5d7e69cb1f44e2b2eb9bea550d59b12707d`
binds run `33321624511`, free-rehearsal job `99284488125`, the three skipped
production jobs and artifact `9735090846`.  The artifact API reported exactly
one 1,901-byte ZIP with digest
`31a090eebb44b63d431d06300a8bffa24aa55f8d26cc035e40bc44110fc92dfe`.
A direct download matched that size and digest and contained exactly one safe
non-directory, non-symlink member named `R2-RESULT.json`; its 6,928 bytes were
byte-identical to the tracked raw result.  This freezes the observation made
while the artifact was available; CI does not pretend to re-download an
expiring artifact forever.

### 73.1 Cursor

```text
R2 GREEN / F6 NEXT / A6 NOT CREATED
PRODUCTION CLAIM TAG / IMAGE / BOOT  NOT CREATED OR RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

R2 is authority-zero evidence, not production permission.  F6 may now pin the
exact five v4 generation files and the successful R2 gate.  A6 remains a later,
separately authorised record and is not created here.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R2-ARTIFACT-PROVENANCE-ARM64-V2-SEALED:END -->

## 74. F6 seals the observed v4 generation without granting production (2026-08-30)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V6-SEALED -->

The fresh R2 result now feeds one authority-zero producer fingerprint:
`native/containment/native-shadow-mac3-successor-producer-fingerprint-arm64-v6.json`,
3,250 bytes at SHA-256
`0e98b02f2dc8c4752c282dba57e1aa39d1cdc62a83c57d8803d6051ea792c183`.
F6 binds P2, R1, F5 and the successful R2 in that order, plus the exact five
v4 generation files and their current regression gates.  The GitHub artifact
provenance remains a separate transport record and does not enter this
production-authority lineage.

Every F6 authority remains false or zero.  It creates neither A6 nor a result-v6
record, production claim tag, image or boot permission.  The create-once tag
fence and claim-bound supervisor are implemented future enforcement mechanisms;
neither has been exercised by this record.

### 74.1 Cursor

```text
R2 GREEN / F6 SEALED / A6 NOT CREATED / PRODUCTION AND BOOT NOT RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The next irreversible edge still requires a separately reviewed A6.  F6 alone
cannot dispatch production or boot a guest.

## 75. A6 withheld after the main-branch dispatch underbinding was found (2026-08-31)

<!-- LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-ARM64-V1-SEALED:BEGIN -->

Pre-A6 review found that the sealed v4 wrapper accepts any non-empty suffix
after the expected workflow-path prefix.  The workflow also does not require
`github.ref` to equal `refs/heads/main`.  A feature or tag ref could therefore
reach the repository-global claim creation path before the
authority was merged to main.  Creating A6 on this generation would turn the
documented main-only boundary into a false claim.

The 13,335-byte authority-zero correction record at SHA-256
`63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b`
preserves v4, R2 and F6 byte-for-byte and withdraws the unused A6/result-v6
reservations.  It preregisters an append-only v5 generation whose guard and
every claim consumer must require `workflow_dispatch`, the exact main dispatch
ref and exact main workflow ref before any dependency, scratch, output,
marker, claim or image effect.  Feature, tag, pull-request, empty and
main-prefix lookalike refs must all be rejected; pull-request-like refs remain
a wrapper hardening case rather than a currently selectable workflow-dispatch
ref.

P4 also preregisters the fresh claim namespace rather than allowing v5 to copy
the withdrawn A6 names.  The only future tag template is
`refs/tags/boole-native-shadow-mac3-successor-production-a7-{attemptId}`;
the claim schema is dispatch-claim v2 and its canonical message includes
`eventName`, `dispatchRef`, `workflowRef`, `githubRunAttempt` and the A7 digest
under `authoritySha256`.  Every consumer must recheck those live values before
effects.  Old A6 paths, schemas, tag prefix, claim schema, digest/CLI symbols
and the v4 root/recovery/artifact/schema namespace are forbidden in v5
production code.  R3, F7, A7 and result-v7 must each bind P4 directly under the
exact field `mainBranchDispatchFenceCorrection`; an indirect reference is not
enough.

Atomic tag creation prevents concurrent duplicate creation while the tag
exists.  No server-side ruleset evidence currently proves that a repository
administrator cannot later delete the tag, so this record does not call the
tag irrevocably undeletable.  A previously observed claim disappearing is a
hard stop.

### 75.1 Cursor

```text
R2 GREEN / F6 HISTORICAL / A6-V6 AND RESULT-V6 WITHDRAWN UNUSED
▶ MAIN-ONLY PRODUCER GENERATION V5 — PREREGISTERED / NOT IMPLEMENTED
⛔ R3 / F7 / A7 / RESULT-V7 — NOT CREATED
⛔ PRODUCTION CLAIM TAG / IMAGE / BOOT — 0 RUNS
⛔ MAC.4 / TESTNET / MINING / REWARD — NOT STARTED
```

This correction grants no image-production, boot, serving or activation
authority.  `mineable_now=0`, `REWARD_READY=0`, `RP0-MD=HOLD`, `BF.7=HOLD`,
Base activation=false and activationAllowed=false remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-MAIN-BRANCH-DISPATCH-FENCE-CORRECTION-ARM64-V1-SEALED:END -->

## 76. Main-only producer generation v5 is implemented; R3 remains unrun (2026-08-31)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V5-IMPLEMENTED-R3-PENDING:BEGIN -->

The five files preregistered by P4 now exist as a new append-only generation:
the v5 core, its contract test, the v5 wrapper, the manual workflow and the
workflow contract test.  The historical v4 files, successful R2 and F6 remain
byte-preserved.  V5 does not reuse the withdrawn A6/result-v6 reservation.

Every effectful workflow job rejects anything other than
`workflow_dispatch`, `refs/heads/main` and the exact v5 workflow ref at main
before checkout or another effect.  Production, claim, recovery, seal and
comparison paths pass through the wrapper and core, which recheck the same
event, dispatch ref, workflow ref, run attempt, live HEAD and future A7
digest.  Their canonical dispatch-claim v2 message has exactly ten fields.
Feature, tag, pull-request-like, empty and main-prefix lookalike values are
negative tests, not alternative production routes.

The authority-zero rehearsal is deliberately narrower.  It rechecks P4 and
the exact manual-main workflow context, requires R3, F7, A7 and result-v7 to
remain absent, and may acquire the already sealed dependencies needed to
exercise the shared assembler.  It cannot claim authority or create a marker,
image or production output; its sole permitted retained effect is one
canonical R3 JSON result.  Therefore the rehearsal does not pretend to possess
the future A7 digest that only a separately authorised production path may
consume.

The production backend also reconstructs its allowed Python import closure
from records already pinned by P4.  Historical R1 supplies the exact low-level
module identities, while historical P2 supplies the reused v3 identities.
Those identities are checked against live bytes before repository modules may
load.  They are internal import evidence only and are not added to the new
external predecessor chain: future R3 remains `[P4]`, F7 remains `[P4,R3]`
and A7 remains `[P4,R3,F7]`.

The combined v5 focused gate passes 258 tests (three Linux-only workflow tests
skip locally and remain CI responsibilities).  No fresh R3 record, F7, A7,
dispatch claim tag, result-v7, attempt marker, image or boot was created.

### 76.1 Cursor

```text
✅ MAIN-ONLY PRODUCER GENERATION V5 — IMPLEMENTED / FOCUSED GREEN
▶ FRESH AUTHORITY-ZERO R3 — NEXT / NOT RUN
⏳ F7 → SEPARATELY REVIEWED A7 → RESULT-V7
⛔ PRODUCTION CLAIM TAG / IMAGE / BOOT — 0 RUNS
⛔ MAC.4 / TESTNET / MINING / REWARD — NOT STARTED
```

Implementation is not permission.  `mineable_now=0`, `REWARD_READY=0`,
`RP0-MD=HOLD`, `BF.7=HOLD`, Base activation=false and
activationAllowed=false remain unchanged.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-V5-IMPLEMENTED-R3-PENDING:END -->

## 77. Fresh authority-zero R3 succeeded; production remains closed (2026-08-31)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-SUCCESS-ARM64-V3-SEALED:BEGIN -->

The first fresh R3 run of the main-only v5 generation completed on exact main
`f690f109ce268bc44a6b91459a373390f6bbc31f`.  Workflow run
`33347946953` used `workflow_dispatch` at `refs/heads/main`; the sole
`free-rehearsal` job (`99355609752`) succeeded, while
`production-authority-guard`, `produce` and `compare` were skipped.  The raw
canonical result is 6,012 bytes at SHA-256
`44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87`
with status `PASS-NO-IMAGE-PRODUCED`.

R3 rechecked P4 and the exact five live v5 generation files, reconstructed the
three sealed reused inputs, and measured the preregistered staging tree as
17,676 entries and 1,773,475,059 payload bytes.  Path collisions, duplicate
paths and symlink escapes were all zero.  The run acquired already sealed
dependencies, so it is not described as offline.  It created no production
claim, marker, scratch, image, boot or activation effect; every authority and
effect count remains false or zero.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-SUCCESS-ARM64-V3-SEALED:END -->

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-ARTIFACT-PROVENANCE-ARM64-V3-SEALED:BEGIN -->

Transport evidence is kept separately in the 3,288-byte provenance record at
SHA-256
`f1618b92cfa138370209a50743f9630e497b35ee4e05d117d1e0af369a95320d`.
Run `33347946953` exposed exactly one artifact (`9742685578`), a 1,744-byte
ZIP at SHA-256
`5f0b7da657d6a56077f16757e4bc461cb968fdd2921c9cdfa11ec878453bed9a`.
Its sole safe member was `R3-RESULT.json`, byte-identical to the tracked raw
result.  This is a frozen observation before the artifact's scheduled expiry,
not a promise that GitHub will retain or re-serve it forever.  Transport
provenance does not enter the future production-authority lineage.

### 77.1 Cursor

```text
R3 GREEN / F7 NEXT / A7 NOT CREATED
PRODUCTION CLAIM TAG / IMAGE / BOOT  NOT CREATED OR RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

F7 may now bind the observed v5 generation as authority-zero identity.  A7
still requires a separate risk review and record; R3 itself grants no image,
boot, serving, mining, reward, consensus, P2P or activation authority.

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-R3-ARTIFACT-PROVENANCE-ARM64-V3-SEALED:END -->

## 78. F7 seals the observed v5 generation without granting production (2026-08-31)

<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-FINGERPRINT-ARM64-V7-SEALED -->

F7 is the canonical 2,798-byte identity record at SHA-256
`3839d92c189a4a56d1d6a79a7fbfb2deaaadcf3dfaec3e636385c96aa106348c`.
It binds P4 and raw R3 in that order, the exact five v5 generation files, and
the exact R3 regression gate.  The separate GitHub artifact provenance is
transport evidence and is deliberately excluded from this authority lineage.

All F7 authority values remain false or zero.  F7 creates no A7, result-v7,
claim tag, marker, image or boot.  It records which generation passed R3; it
does not authorize that generation to produce anything.

### 78.1 Cursor

```text
R3 GREEN / F7 SEALED / A7 NOT CREATED / PRODUCTION AND BOOT NOT RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The next boundary is a separate pre-A7 risk review.  No production authority
record may be created merely because F7 exists.

## 79. Pre-A7 risk review closes without opening production (2026-08-31)

<!-- PRE-A7-RISK-REVIEW-V1-COMPLETE-AUTHORITY-NOT-GRANTED -->

The reviewed A7 shape is narrow: one exact manual dispatch from main, the
fixed workflow ref, two replicas, the fixed result path and zero performed
runs at creation.  No new frozen-contract blocker was found.  That finding is
not permission to create A7 or run production.

Two known boundaries remain unchanged.  First, administrator deletion is outside
the code-only proof; the atomic tag prevents a concurrent second creator only
while it exists, and disappearance of a previously observed tag is a hard stop.
Second, transport provenance remains outside the authority lineage; raw R3 and
its gate provide the authority-chain observation while the GitHub artifact
record proves carriage separately.

### 79.1 Cursor

```text
PRE-A7 REVIEW COMPLETE / A7 NOT CREATED / PRODUCTION AND BOOT NOT RUN
MAC.4 / TESTNET / MINING / REWARD  NOT STARTED
```

The next boundary is the operator's explicit A7 creation decision and attempt
ID.  A7 creation, production dispatch and boot remain three separate acts.

## 80. Reversible image-to-readiness preflight is green; execution remains explicit (2026-08-31)

<!-- CLOSED-LOCAL-IMAGE-READINESS-PREFLIGHT-GREEN-BUILD-AND-BOOT-NOT-RUN -->

The reversible closed-local development path now exists as two ordinary,
retryable components. PR #323 (`f4d1e9c`) added the Linux/arm64 image lane,
and PR #324 (`6af0b16`) added the Mac readiness runner. The Mac runner's local
dry run compiled and signed the existing Swift host, validated the exact
three-image receipt and closed VM shape, changed no input hash, and started
zero virtual machines.

The first arm64 preflight (run `33388707339`) reached backend PASS but exposed
an unprivileged post-step that could not inspect the root-owned workspace. It
created no image. PR #325 (`de96974`) made those checks use the same privileged
view without weakening containment. The corrected preflight, run
`33393135963`, then passed end to end with zero images and zero virtual
machines.

The real two-replica image build and the real Mac VM boot have not run. Each is
an explicit execution boundary because it consumes substantial compute or
starts Apple Virtualization.framework. No rejected execution was worked
around. This milestone creates no A7, v5 production, MAC.4, testnet, mining,
reward, consensus, P2P or activation authority.

### 80.1 Cursor

```text
CLOSED-LOCAL IMAGE LANE + MAC RUNNER MERGED / ARM64 PREFLIGHT GREEN
IMAGE BUILD AND VM BOOT NOT RUN / A7 NOT CREATED
```

Next is the explicitly approved disposable two-replica ARM64 build. If and
only if its receipt is green, a real Mac VM boot remains a second, separate
explicit approval.

## 81. Disposable image replicas matched; the first Mac readiness boot exposed one image-mode bug (2026-09-01)

The approved reversible build run
[`33414353361`](https://github.com/NotoriAndo/Boole/actions/runs/33414353361)
completed two independent arm64 replicas and compared all three outputs byte for
byte. The comparison status was `TWO-REPLICAS-BYTE-IDENTICAL`. Across the two
replicas the raw outputs totalled 7,739,926,912 bytes. The kernel, initrd and root
disk identities are recorded in
`native-shadow-closed-local-image-mac-readiness-result-arm64-v1.json`.

The comparison GREEN opened the separately approved Mac run. Its no-VM
preflight passed, then Apple Virtualization.framework started exactly one closed
VM with two CPUs, 2 GiB RAM, no network, no shared directory and one read-only
disk. Linux mounted the expected ext4 root read-only, started systemd as PID 1
and reached the launcher service. The launcher then correctly failed closed:
`/usr/share/boole/native-shadow` had generic parent mode `0755`, while the
installed-authority contract requires exact mode `0555`. No readiness record was
emitted; the host stopped the VM at its fixed timeout and all three input hashes
remained unchanged.

This is a deterministic development-image assembly bug, not an Apple VM or
kernel boot failure. The source lock tracks the files under the authority
directory but not the parent directory itself, and the Linux readback gate had
the same blind spot. A read-only streaming walk of the actual uncompressed
initrd header confirmed that directory as root-owned `0755`. The development
image path now changes that one derived
parent to root-owned `0555`; readback independently requires the same metadata
before a future image can reach replica comparison. Historical sealed producers
are not edited. No second image build or Mac boot is part of this milestone.

### 81.1 Cursor

```text
ARM64 REPLICAS BYTE-IDENTICAL / FIRST MAC VM REACHED SYSTEMD AND LAUNCHER
READINESS FAIL-CLOSED ON AUTHORITY DIRECTORY 0755 != 0555
CORRECTION IMPLEMENTED + PRE-BOOT READBACK REGRESSION GATE ADDED
NEXT: FRESH DISPOSABLE REPLICAS, THEN A SEPARATELY APPROVED MAC READINESS RETEST
A7 / PRODUCTION / MAC.4 / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION NOT STARTED
```

### 82.2 Main CI and the zero-image preflight are green

PR #331 merged the correction as `4577dade2b2d85644d21d88d59e591eadbcc79ed`
after all required checks passed. The main-only preflight run
[`33463421718`](https://github.com/NotoriAndo/Boole/actions/runs/33463421718)
then completed with status `READY-NO-IMAGE-CREATED`. Its sole 1,302-byte
canonical result hashes to
`0896bd9700cb76c7a99af139ff26a47d5d3ae242d8ec8b552bfcb49fb113c85a`.
It measured 17,676 entries and 1,773,475,059 payload bytes with zero path
collision, duplicate or symlink escape.

The result records `imagesCreated=0` and `machinesStarted=0`; every production,
boot, serving, MAC.4, testnet, mining, reward, consensus, P2P and activation
permission or claim remains false or zero. The next evidentiary step is a fresh
disposable two-replica build. It is not implied by this preflight: another
approximately 7.7 GB raw output set and any subsequent Mac VM are separate
execution decisions.

```text
TOOLCHAIN DIRECTORY CORRECTION MERGED / ZERO-IMAGE PREFLIGHT GREEN
NEXT: EXPLICITLY AUTHORISED DISPOSABLE REPLICA BUILD
IF AND ONLY IF REPLICAS MATCH: SEPARATELY AUTHORISED MAC READINESS OBSERVATION
PRODUCTION / MAC.4 / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION CLOSED
```

## 82. A second disposable build stayed deterministic; the next fixed directory was exposed (2026-09-01)

The approved development run
[`33458786844`](https://github.com/NotoriAndo/Boole/actions/runs/33458786844)
again produced two independent arm64 replicas whose kernel, initrd and root
disk were byte-identical. The output identities are recorded append-only in
`native-shadow-closed-local-image-mac-readiness-result-arm64-v2.json`. The
Mac no-VM preflight passed, then exactly one closed VM started with no network,
no shared directory and one read-only root disk. Linux reached systemd PID 1,
multi-user and graphical targets, and the launcher service.

Readiness remained false. The launcher failed closed while reverifying
`/opt/boole/native-checker-toolchain`: the directory was generic derived-parent
mode `0755`, while the fixed launcher contract requires exact mode `0555` for
both the toolchain root and its `bin` directory. The same fatal line appeared
on each launcher restart. The host stopped at the fixed timeout, emitted no
submission, and all three image hashes remained unchanged.

The source lock contains the toolchain files but no rows for those two parent
directories. The previous development correction covered only the installed
authority parent, and its mounted-tree readback repeated the same omission.
The reversible development path now narrows all three fixed security parents
to root-owned `0555`; readback independently requires both toolchain directory
modes before replica comparison. Historical sealed producers remain byte
preserved. No additional image build or Mac boot is part of this correction.

### 82.1 Cursor

```text
SECOND ARM64 REPLICA PAIR BYTE-IDENTICAL / SECOND MAC VM REACHED LAUNCHER
READINESS FAIL-CLOSED ON TOOLCHAIN ROOT 0755 != 0555
ROOT CAUSE RECORDED / DEVELOPMENT ASSEMBLER + READBACK CORRECTED
NEXT REVERSIBLE GATE: MAIN CI, THEN ZERO-IMAGE PREFLIGHT
FRESH IMAGE BUILD + ANY FURTHER MAC BOOT REQUIRE NEW EXECUTION AUTHORITY
A7 / PRODUCTION / MAC.4 / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION NOT STARTED
```

## 83. A third deterministic build reached the replay-authority boundary (2026-09-01)

The explicitly approved run
[`33466531840`](https://github.com/NotoriAndo/Boole/actions/runs/33466531840)
produced a third pair of independent disposable arm64 images. The kernel,
initrd and root disk were byte-identical across both replicas. Their exact
identities, GitHub artifact identities and the comparison receipt are recorded
append-only in
`native-shadow-closed-local-image-mac-readiness-result-arm64-v3.json`.

The approved Mac no-VM preflight passed and exactly one closed VM then started.
It used no network or shared directory and attached one read-only root disk.
Linux reached systemd PID 1, the normal targets and the launcher service. The
launcher then failed closed before readiness because the development image did
not contain
`/usr/share/boole/native-shadow/closed-local-replay-registry-overlay-v1.json`.
The same missing-file failure appeared on each service restart. The host stopped
at the fixed timeout; all image identities remained unchanged and there was no
retry.

The root cause is one generation mismatch. The historical source lock contains
the original installed checker bundle, while the newer closed-local replay
startup also requires three replay-authority records plus the frozen task and
anchor. The reversible development lane now adds exactly those five pinned
files (13,599 bytes) and requires every authority, checker, fixture and
toolchain directory to be root-owned mode `0555`. The historical staging tree
is measured again after removing only this additive overlay and must remain
identical to its sealed measurement. Linux mounted-tree readback independently
requires the five file identities and seven fixed directory contracts.
Historical sealed producers and authority records remain byte-preserved.

### 83.1 Cursor

```text
THIRD ARM64 REPLICA PAIR BYTE-IDENTICAL / THIRD MAC VM REACHED LAUNCHER
READINESS FAIL-CLOSED ON MISSING CLOSED-LOCAL REPLAY AUTHORITY MATERIAL
ROOT CAUSE RECORDED / EXACT FIVE-FILE DEVELOPMENT OVERLAY IMPLEMENTED
NEXT REVERSIBLE GATE: MAIN CI, THEN ZERO-IMAGE ARM64 PREFLIGHT
FRESH IMAGE BUILD + ANY FURTHER MAC BOOT REQUIRE NEW EXECUTION AUTHORITY
PRODUCTION / MAC.4 / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION CLOSED
```

## 84. A fourth deterministic build passed every launcher prerequisite; generic headless units blocked readiness (2026-09-01)

The development-only correction merged as PR #333 at main
`36bc14e4606f2b812bf7294a81a04e575cc6998e`. Its zero-image arm64 preflight
run `33471378609` returned `READY-NO-IMAGE-CREATED` with 17,683 entries,
1,773,488,658 payload bytes, zero image and zero virtual machine.

The subsequently approved build run
[`33471902181`](https://github.com/NotoriAndo/Boole/actions/runs/33471902181)
produced two independent replicas. The kernel, initrd and root disk were
byte-identical, totalling 7,740,038,656 raw bytes across both replicas. A Mac
host preflight started no machine and passed. Exactly one closed VM then ran
with two CPUs, 2 GiB RAM, no network or shared directory and one read-only
root disk. The image hashes remained unchanged after host shutdown.

This run crossed every earlier launcher boundary. The console contained the
exact sealed launcher identity, all nine launcher prerequisites as resolved,
and the complete fixed root-supervisor privilege shape. Readiness alone was
false because the generic Ubuntu boot policy left seven failed units:
`ldconfig.service`, `getty@tty2.service` through `getty@tty6.service`, and
`serial-getty@hvc0.service`. The first cannot rebuild its cache on the required
read-only root; the login units have no role in a closed guest with no login
surface. The empty-failed-unit readiness rule is unchanged.

The reversible development image now masks the four unit definitions that can
schedule that set (`ldconfig.service`, `getty-static.service`, `getty@.service`
and `serial-getty@.service`) by exact root-owned symlinks to `/dev/null`.
Mounted-tree readback requires every mask and target before replica comparison.
The historical sealed image producers remain byte-preserved. This correction
has not yet been exercised by another image or VM.

### 84.1 Cursor

```text
FOURTH ARM64 REPLICA PAIR BYTE-IDENTICAL / FOURTH MAC VM REACHED ALL LAUNCHER PREREQUISITES
READINESS FALSE ONLY BECAUSE GENERIC READ-ONLY/INTERACTIVE SYSTEMD UNITS ENTERED FAILED STATE
CRITERION UNCHANGED / DEVELOPMENT-ONLY UNIT MASKS + MOUNTED READBACK IMPLEMENTED
NEXT REVERSIBLE GATE: MAIN CI, THEN ZERO-IMAGE ARM64 PREFLIGHT
PRODUCTION / MAC.4 / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION CLOSED
```

## 85. The closed Mac guest reached exact readiness (2026-09-01)

The corrected main commit
`0d437f226331a76636ef15fc9f033eb0a4ac2199` first passed its zero-image
arm64 preflight in run `33476715979`: no image was created and no virtual
machine started. The separately approved development run
[`33485969541`](https://github.com/NotoriAndo/Boole/actions/runs/33485969541)
then produced two fresh arm64 replicas. The kernel, initrd and root disk were
byte-identical across both replicas; their exact sizes, identities and GitHub
artifact records are sealed in the append-only v5 result.

The Mac host preflight again started no machine and passed. Exactly one closed
Apple Virtualization.framework VM then ran with two CPUs, 2 GiB RAM, no
network device, no shared directory and one read-only root disk. Linux reached
systemd PID 1, multi-user and graphical targets, and the launcher service. The
guest reported the exact sealed launcher identity, all nine prerequisites as
resolved, the complete fixed root-supervisor privilege shape, and readiness
with an empty failed-unit set. The four development-only unit masks therefore
closed the observed headless-policy gap without relaxing the readiness rule.
The host stopped the VM at the fixed observation timeout and all three input
image identities remained unchanged. No submission was made or observed.

This is a closed-local development readiness result, not production serving or
network activation. It grants no MAC.4, node connection, testnet, mining,
reward, consensus, P2P or activation authority.

### 85.1 Cursor

```text
FIFTH ARM64 REPLICA PAIR BYTE-IDENTICAL / CLOSED MAC READINESS PASS
EXACT LAUNCHER + 9 PREREQUISITES + ROOT SUPERVISOR + FAILED UNITS 0
NEXT: MAC.4 HOST-GUEST AUTHENTICATED CHANNEL, STILL CLOSED-LOCAL
PRODUCTION / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION CLOSED
```

## 86. The first authenticated-channel observation failed closed before one round trip (2026-09-01)

The explicitly approved development run
[`33510635018`](https://github.com/NotoriAndo/Boole/actions/runs/33510635018)
produced two independent ARM64 replicas at exact feature head
`957319e0a2aa780febd25e97ea27ad8243e287d0`. Kernel, initrd and root disk were
byte-identical across the replicas. Their exact identities and the ephemeral
artifact provenance are sealed in
`native-shadow-mac4-authenticated-channel-result-arm64-v1.json`.

The same approval opened exactly one Apple Virtualization.framework boot. The
VM remained closed: no IP network or shared directory, one vsock device and
one read-only root disk. Linux reached systemd, the existing launcher reported
all nine prerequisites resolved and readiness true, and the host stopped the
machine after the fixed timeout. The new relay service failed before emitting
its ready frame, so the authenticated vsock round trip count is zero and
MAC.4 is not complete. No retry was made.

Read-only inspection identified a sufficient root cause. The exact guest
kernel sets `CONFIG_VSOCKETS`, `CONFIG_VIRTIO_VSOCKETS` and the common
transport to modular (`m`). The corresponding compressed module objects exist,
but the image has no `modules.dep`/`modules.alias` indexes. Its only
`modules-load.d` entry is a dangling symlink to an absent `/etc/modules`.
There is consequently no usable deterministic load path for the AF_VSOCK
transport. The precise relay errno was not preserved, so this record does not
claim it.

The next reversible work is to generate and pin depmod indexes during image
construction, provide an exact vsock modules-load input and require both in a
free readback/preflight. A privileged runtime module loader or custom kernel is
not the default remedy because either expands the runtime privilege or the
build/supply surface. A fresh image pair and another Mac boot remain separate
operator decisions.

```text
DISPOSABLE ARM64 REPLICAS BYTE-IDENTICAL / EXACTLY ONE MAC VM STARTED AND STOPPED
MAC.4 AUTHENTICATED ROUND TRIP 0 / FAILED CLOSED / SUFFICIENT ROOT CAUSE IDENTIFIED
NEXT: DETERMINISTIC DEPMOD INDEXES + VSOCK LOAD CONTRACT + FREE PREFLIGHT
NEW IMAGE / NEW BOOT REQUIRE NEW AUTHORITY
PRODUCTION / TESTNET / MINING / REWARD / CONSENSUS / P2P / ACTIVATION CLOSED
```
