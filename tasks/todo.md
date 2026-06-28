# lean-runner process-group kill characterization — 2026-06-25

Groundwork before EXECUTION-ORDER [9] (lean-runner kernel-isolation ADR):
pin the existing process-group SIGKILL guarantee with a regression guard so the
later ADR-driven isolation work has a safety net. origin/main=9de7e2b at start.

## Context (from isolation coverage audit)

The verifier runs `lake exec boole_check` in its OWN process group
(`configure_child_sandbox` -> `setpgid(0,0)`) so a timeout kill
(`kill_child_group` -> `killpg(SIGKILL)`) reaps the whole group, including the
`lean` compiler `lake` forks as a grandchild. The pre-existing
`child_kill_on_drop_*` tests only cover a single direct child — the
grandchild/process-group path was UNTESTED. (Other untested isolation gaps —
OOM rlimit, env scrub — are platform-sensitive/Linux-only and deferred.)

## Steps

- [x] guard 1 `kill_child_group_reaps_grandchild_not_just_direct_child`
      (in-lib `#[cfg(unix)]`, no lake): /bin/sh forks a backgrounded sleep
      (grandchild), real `configure_child_sandbox` groups it, `kill_child_group`
      must reap the grandchild. GREEN + behavioral-RED (single-pid kill ->
      grandchild survives -> FAIL). **Landed `3fec7fa`, CI green.**
- [x] guard 2 `child_environment_is_scrubbed_to_minimal_allowlist`: a secret
      set as a Command override is wiped by `configure_child_environment`'s
      `env_clear()` (checker cannot read operator secrets); only PATH/HOME/LANG
      restored. Race-free (no process-env mutation). GREEN + behavioral-RED
      (drop `env_clear()` -> `SECRET=do-not-leak` leaks -> FAIL).
      **Landed `6269b73`, CI green.**
- [x] guard 3 `configure_child_sandbox_caps_cpu_time`: `RLIMIT_CPU` =
      (timeout_ms/1000)+5 = 15s (the runaway backstop on macOS where RLIMIT_AS
      is a no-op), read via `ulimit -t`. GREEN + behavioral-RED (drop the
      RLIMIT_CPU set -> child reports `unlimited` -> FAIL).
- [x] full gate `self-test: PASS` (gate10; runtime-smoke-all / bench /
      local-mining-smoke green; publicMiningEvidence=false).
- [x] commit guard 3 `2100e79` (NotoriAndo, test-only) + push + remote verify +
      CI green (self-test ✓ + supply-chain ✓).

## Done — all three lean-runner isolation guards landed & CI-green

`3fec7fa` process-group SIGKILL · `6269b73` env scrub · `2100e79` CPU rlimit.
The locally-verifiable isolation surface is now pinned. Remaining gaps
(OOM/RLIMIT_AS) are Linux-only / not locally verifiable -> deferred to the
[9] ADR landing rather than pushed unverified.

Next master-cursor item: [9] lean-runner kernel-isolation **ADR** — an
architecture decision (논의 후 결정), drafted at `docs/adr/0008-*` (Proposed).

## Notes

- Test-only: production `kill_child_group` unchanged (the diff is +73 lines, a
  pure test addition).
- Next on the master cursor after this groundwork: [9] lean-runner kernel
  isolation ADR — an architecture decision (seccomp/landlock/namespaces/uid
  vs current rlimits+pgroup), NOT a TDD slice; needs a design decision.
