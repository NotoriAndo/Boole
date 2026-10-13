# Linux acceptance-gate controls

The `ubuntu-24.04` workflow provisions the tracked hardened boundary using
`scripts/setup_zk_registry_sandbox.sh`: SHA256-verified Ubuntu bubblewrap and
libseccomp packages, a non-setuid pinned bwrap path, the digest-pinned AppArmor
userns profile, native seccomp BPF and the pinned PID-1 observer. It does not
change the AppArmor userns restriction, accept arbitrary tool hashes or use
an untracked descriptor overlay. It requires Landlock ABI >=4, not ABI9.

The cgroup-delegated control harness has 12GB MemoryMax and TasksMax512. Real
production controls cover network protocols and independent layers, setsid and
double-fork containment, pids and OOM precedence, scoped reads/writes, missing
components, and the forbidden-command/attribute policy. Layer-disabled runs
are explicitly test-fixture and cannot qualify production. No required live
control silently skips when a mandatory layer is unavailable.

These fast controls require neither Lean nor Mathlib. Full closure exports and
both kernels are verified separately against the architecture-specific tracked
tool profiles. A profile is a provisioning pin, not proof that the hosted runner
has passed. The hosted run is **pending** until the orchestrator publishes the
branch; local workflow execution does not claim a hosted result or activate a
catalog. Task Z's CI commit was fetched locally and rebased onto the hardening
branch; no public Git fetch fallback to historical gate code remains.
