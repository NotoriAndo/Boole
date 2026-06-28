# ADR 0008: Lean-runner kernel isolation

## Status

Status: **Proposed — deferred to the N3 approach** (2026-06-28). This ADR
records options and a recommended direction for EXECUTION-ORDER [9]
("lean-runner 커널 격리 ADR + 착륙"), required before N3.2 (share-gossip ingress).
It is a decision document for review — no isolation change is implemented yet.

**Deferral is intentional and safe.** Kernel isolation is only needed once
untrusted proofs arrive over the network (N3.2). The current phase is
closed-local (proofs are operator/fixture sourced), which the baseline +
the three landed characterization guards already cover. The plan marks [9] as
"N3 직전 필수", so the decision (Open decisions below) and the landing (TDD) are
deliberately taken just before N3.2, not now. The hard constraint: this MUST
land before N3.2 opens network ingress.

The process-group / env-scrub / CPU-rlimit **characterization guards already
landed** (`3fec7fa`, `6269b73`, `2100e79`) pin the *current* baseline so any
ADR-driven change has a regression net. They are groundwork, not the decision.

## Context

`boole-lean-runner` is Boole's trusted OS-syscall boundary: it runs
`lake exec boole_check <proof>` — an **untrusted** Lean compilation — and returns
an evidence envelope. Today the input proofs are operator-local or fixture
sourced. At **N3.2 the threat model changes**: proofs arrive over share-gossip
from untrusted network peers, so a hostile proof that triggers
arbitrary-ish behavior in `lean`/`lake` (a compiler with macro/elaboration
surface) becomes a live attack vector against every verifying node.

### Current isolation baseline (already implemented, `crates/boole-lean-runner/src/lib.rs`)

- **Process group + SIGKILL** — `setpgid(0,0)` in `pre_exec`; on timeout
  `killpg(SIGKILL)` reaps the whole group incl. the `lean` grandchild
  (guard: `kill_child_group_reaps_grandchild_not_just_direct_child`).
- **Wall-clock timeout** (default 10s) + watchdog kill.
- **rlimits** via `pre_exec`: `RLIMIT_CPU=(timeout/1000)+5`, `RLIMIT_FSIZE=256MiB`,
  `RLIMIT_NOFILE=1024`; `RLIMIT_AS`/`RLIMIT_DATA` on **Linux only** (macOS
  rejects them). (guard: `configure_child_sandbox_caps_cpu_time`.)
- **Environment scrub** — `env_clear()` + minimal PATH/HOME/LANG allowlist, so
  the checker cannot read operator secrets (guard:
  `child_environment_is_scrubbed_to_minimal_allowlist`).
- **Output cap** (64 KiB/stream, drained off-thread), **stdin null**.
- **Pre-spawn static rejects**: forbidden tokens (`sorry`/`axiom`/
  `native_decide`), symlinks in the checker package.

### What the baseline does NOT cover

- **Network egress** — the checker can open sockets (exfiltration / SSRF / C2).
- **Filesystem reach** — read/write anywhere the node's uid can, beyond the
  checker package dir (only file *size* and *count* are capped, not *paths*).
- **`exec` of arbitrary binaries** — `lake` legitimately spawns `lean`; a
  hostile proof that induces further `exec` is unconstrained.
- **Memory on macOS** — `RLIMIT_AS` is a no-op on Darwin; only CPU-time +
  wall-clock bound a memory bomb there.

### Portability constraint

Kernel isolation primitives (seccomp-bpf, Landlock, namespaces) are **Linux
only**. Boole dev is macOS; CI `self-test` and production are Linux. Any
kernel-layer hardening must be `cfg(target_os = "linux")` with the portable
baseline as the macOS fallback, and the divergence documented (macOS dev runs
with strictly weaker isolation than production).

## Options

- **(A) Keep the baseline, defer kernel isolation.** Cheapest. Leaves network
  egress + FS reach open — unacceptable once N3.2 admits untrusted proofs.
- **(B) seccomp-bpf syscall allowlist** (Linux). Blocks `socket`/`connect`/
  `execve`-of-non-lean / `ptrace` / etc. via a `pre_exec` filter. Highest
  security-per-effort against egress + arbitrary-exec; needs a carefully tuned
  allowlist (lake/lean's real syscall set) or it breaks the checker. Crate:
  `seccompiler` (Rust, no_std-friendly) — supply-chain review required.
- **(C) Landlock LSM** (Linux ≥ 5.13, unprivileged). Restricts filesystem
  access to an explicit ruleset (read-only toolchain + checker pkg dir, no
  write elsewhere). Complements (B); does not cover network. Crate: `landlock`.
- **(D) Namespaces / `unshare`** (Linux). A network namespace with no
  interfaces kills egress cleanly; mount/pid namespaces isolate FS/process
  view. More moving parts; user-namespace availability varies by host policy.
- **(E) Drop privileges** — run the checker under a dedicated unprivileged
  uid/gid. Portable-ish, limits FS reach to that uid's perms; coarse, needs a
  provisioned user; doesn't stop egress.
- **(F) External sandbox** (bubblewrap / gVisor / container-per-check). Strong,
  but heavy operational + supply-chain + latency cost; over-scoped for a
  per-proof in-process check.

## Recommended direction (for ratification)

A **layered, Linux-primary** approach, landed before N3.2, with the existing
baseline as the portable floor:

1. **seccomp-bpf allowlist (B)** as the primary kernel layer — its main job is
   to **deny network egress and arbitrary `execve`**, the two gaps that matter
   most for untrusted-proof verification.
2. **Landlock (C)** as the filesystem layer — read-only toolchain, no write
   outside a scratch dir; cheap to add alongside (B).
3. Keep **rlimits + pgroup + env-scrub** (current baseline) as the
   cross-platform floor; **macOS dev = baseline only**, documented as weaker
   than production.
4. Defer (D)/(E)/(F) unless (B)+(C) prove insufficient; network namespace (D)
   is the natural fallback if a clean seccomp egress-deny is hard.

Scope/landing: a `cfg(target_os="linux")` extension of `configure_child_sandbox`
(filter installed in `pre_exec` after the existing rlimits), behind an
`Option`-gated config so it can be tightened iteratively; new Linux-CI
characterization guards (egress blocked, non-lean exec blocked, FS write
denied) mirroring the three baseline guards.

## Open decisions (need your call)

1. **Scope**: (B)+(C) now, or start with (B) only / defer to a network
   namespace (D)? 
2. **Dependencies**: `seccompiler` + `landlock` crates are new supply-chain
   surface (cargo-deny/audit gating). OK to add, or prefer raw `libc`
   `prctl`/`seccomp` to avoid deps?
3. **macOS posture**: accept "dev = baseline-only, prod = full" divergence
   (recommended), or invest in a macOS sandbox (`sandbox_init`/Seatbelt)?
4. **Enforcement default**: ship the filter **on by default** in release, or
   opt-in (`cfg`/config) first while the allowlist is tuned against real
   lake/lean workloads?

## Consequences

- Verifying nodes gain egress + exec + FS containment against hostile proofs
  before N3 opens the network ingress — the defining safety property for a
  public verifier.
- A documented, tested isolation divergence between macOS dev and Linux prod.
- New supply-chain dependencies (pending decision) under the existing
  `cargo-deny`/`cargo-audit` gate.
- The three landed baseline guards remain the regression net for the portable
  floor; the kernel layer needs its own Linux-CI guards.
