# Mac-first hidden Linux execution plan v1

Status: **MAC.0 COMPLETE (closed-local Linux baseline, 2026-08-24, section 9);
MAC.1-PARTIAL — CURL-FIRST MODE FROZEN; UPDATE TRUST POLICY AND MEASUREMENT PROTOCOL OPEN
(product-form correction 2026-08-25, section 14; macOS 14/M1/Intel decision remains fixed);
MAC.2-PARTIAL — CLOSED-LOCAL LINUX/ARM64 AUTHORITY PARITY COMPLETE; STAGED VERIFIER
CORE/KAT GREEN; PRODUCTION AUTHORIZATION, ADOPTION AND POST-ADOPTION REVERIFICATION OPEN
(sections 11 and 13); CURL-FIRST-CLI-SERVICE-DISTRIBUTION — CURRENT (section 14 supersedes the
Boole.app/Developer-ID/Team-ID product-form decision); CURL.1 CONTRACT/VERIFIER GREEN —
RELEASE CONTRACT AND GUEST BOOT FORMAT FROZEN; INSTALLER, REAL RELEASE ARTIFACT AND
PRODUCTION TRUST ROOT ABSENT (section 15); MAC.3 BLOCKED /
NOT STARTED — NOT IMPLEMENTED, NOT RELEASE-READY, NO ACTIVATION AUTHORITY.**

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
