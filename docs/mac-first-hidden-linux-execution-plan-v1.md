# Mac-first hidden Linux execution plan v1

Status: **PLANNED — NOT IMPLEMENTED, NOT RELEASE-READY, NO ACTIVATION AUTHORITY.**

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
