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
- [ ] full gate `self-test: PASS` (boole-lean-runner is consensus-path —
      confirm runtime-smoke-all / proof-to-block-benchmark green in log)
- [ ] commit guard 2 (NotoriAndo, test-only) + push + remote verify + CI green.

## Notes

- Test-only: production `kill_child_group` unchanged (the diff is +73 lines, a
  pure test addition).
- Next on the master cursor after this groundwork: [9] lean-runner kernel
  isolation ADR — an architecture decision (seccomp/landlock/namespaces/uid
  vs current rlimits+pgroup), NOT a TDD slice; needs a design decision.
