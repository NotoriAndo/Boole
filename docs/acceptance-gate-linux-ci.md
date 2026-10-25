# Linux acceptance-gate controls

The `ubuntu-24.04` workflow provisions the tracked hardened boundary using
`scripts/setup_zk_registry_sandbox.sh`: SHA256-verified Ubuntu bubblewrap and
libseccomp packages, a non-setuid pinned bwrap path, the digest-pinned AppArmor
userns profile, native seccomp BPF and the pinned PID-1 observer. Before setup it
sets and records `kernel.apparmor_restrict_unprivileged_userns=1` (never0). It does not
weaken that restriction, accept arbitrary tool hashes or use
an untracked descriptor overlay. It requires Landlock ABI >=4, not ABI9.

The cgroup-delegated control harness has 12GB MemoryMax and TasksMax512. Real
production controls cover network protocols and independent layers, setsid and
double-fork containment, pids and OOM precedence, scoped reads/writes, missing
components, and the forbidden-command/attribute policy. The review regressions
also cover cross-stage symlinks and a host canary, bounded orphan evidence,
payload exit125 versus structured setup errors, four-CPU execution, and breaking
only the Comparator-stage launcher. Submission-controlled output cannot forge
an availability failure. Layer-disabled runs
are explicitly test-fixture and cannot qualify production. No required live
control silently skips when a mandatory layer is unavailable.

Nodes must provide at least four schedulable CPUs. The observer selects exactly
four before exec; Lean uses `-j4`, Go uses `GOMAXPROCS=4` and build `-p4`, and
Cargo/Rayon/test-runner parallelism is explicitly bounded. Tracked stage PID caps
have at least fourfold headroom over the20-item pilot and six real framework
builds:64 for compile/replay/export/Comparator,256 for native build/run. A cleanup
timeout/removal failure is retained separately and never overwrites the verdict.

These fast controls require neither Lean nor Mathlib. Full closure exports and
both kernels are verified separately against the architecture-specific tracked
tool profiles. A profile is a provisioning pin, not proof that the hosted runner
has passed. The hosted run is **pending** until the orchestrator publishes the
branch; local workflow execution does not claim a hosted result or activate a
catalog. Task Z's CI commit was fetched locally and rebased onto the hardening
branch; no public Git fetch fallback to historical gate code remains.

The advisory `det3-counter-identity-x86_64` job uses a pinned source SDK to compare 32 public
generated Init inputs against newly measured aarch64 goldens. It verifies preregistered source
and export byte identities; no private catalog proof or export is published. The committed
generator records the derivation. It cannot replace the expected golden with its own result.
Raw vectors and build manifests are uploaded even on failure. The job does not create an
x86-64 production profile and is not a required branch-protection check. Hosted runtime,
including whether it fits 120 minutes, is unmeasured. `ubuntu-latest` and apt package versions
are unpinned; actions are SHA-pinned. The orchestrator records runtime after publication.
