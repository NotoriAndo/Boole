# N2.1 (+ precondition N0-pre.10) — 2026-06-21

Goal: EXECUTION-ORDER.md [8] N2.1 (`/submit` ownership proof). Audit found the
binding precondition **N0-pre.10** (typed submit rejection) was unimplemented;
both touch the same submit-reject path, so pre.10 lands first as the enabling
step, then N2.1. Combined full gate (gates are ~12h on this loaded machine).

origin/main=bfa2164 at start.

## N0-pre.10 — typed submit rejection kind (StaleC)

- [x] RED: miner `mining_loop` test `stale_c_head_refresh_triggers_on_reason_
      code_not_substring` references new `kind`/`SubmitRejectionKind` → compile
      FAIL = RED.
- [x] GREEN:
      - core: `RejectionReason::code()` + `AdmissionDecision::reject_code()`
        (SharePool→`SharePoolRejectReason::as_str` single-sources `stale_c`).
      - node: additive `"code": decision.reject_code()` on `/submit` reject JSON
        (keeps `decision` Debug string for back-compat).
      - miner: `SubmitRejectionKind{StaleC,Other}` + `kind` field on
        `SubmitResult::Rejected`; `from_payload` prefers `code`, falls back to
        substring scan for pre-`code` nodes; `mining_loop` keys on `kind`.
- [x] focused: mining_loop stale_c 2/2, submit_client 8/8 (incl new parse test).

## N2.1 — `/submit` ownership proof enforced by default

- [x] RED: `tests/submit_requires_ownership.rs` (anonymous submit, config field
      not yet present → unknown-field compile FAIL = RED).
- [x] GREEN:
      - `LocalNodeConfig.allow_anonymous_submit: bool` (+ state mirror).
      - `submit_handler`: `checked_session.is_none() && !allow_anonymous`
        → `401 unauthenticated_submit` before admission (no credit).
      - `HttpError::unauthenticated_submit()` (401).
      - CLI `--allow-anonymous-submit` (default false); production stays secure.
      - local-mining-smoke.sh boots with `--allow-anonymous-submit` (controlled
        local smoke).
      - ~50 existing test/cli `LocalNodeConfig` literals set
        `allow_anonymous_submit: true` (preserve their anonymous behavior).
- [x] focused: submit_requires_ownership 2/2; no regression across submit_
      session_policy/submit_lean_admission_nonce/local_node/http_contract/
      account_balance_route/metrics_counters/runtime_policy_boot (40 tests).
- [x] isolation: local-mining-smoke PASS (2 blocks). runtime-smoke /
      proof-to-block-benchmark use in-process subcommands (not HTTP /submit) →
      unaffected by N2.1.
- [x] build both feature sets clean; clippy core/node/miner exit 0; fmt clean.

## Landed (2026-06-24)

- [x] combined full gate `self-test: PASS` (runtime-smoke-all 6/6,
      proof-to-block-benchmark 7 cases / 17 blocks / 0 replay-fail / 0
      invalid-accepted, local-mining-smoke 2 blocks; publicMiningEvidence=false).
- [x] commits + push (origin/main):
      - `63f3b46` node,miner: typed StaleC submit rejection via stable code (pre.10)
      - N2.1 ownership commit (this docs commit's parent)
      - EXECUTION-ORDER [8] N2.1 + pre.10 marked done (local plan doc).

## CI follow-ups landed alongside (separate commits)

- [x] `7a8623e` ci: prebuild Lean checker helpers before self-test — the
      self-test job was red on a fresh runner (deep_verify_block_roundtrip
      DeepVerifyDivergence accepted true→false) because the Lean checker's
      gitignored `.lake/build` had no `Boole.Family.V0Helpers` olean. Added a
      `lean-checker-build` gate stage + contract tests; reproduced & fixed in a
      cleared-cache run. (See lessons.md "fresh-environment".)
- [x] `26a8647` security(deps): bump quinn-proto to 0.11.15 (RUSTSEC-2026-0185)
      — supply-chain job's `cargo audit` flagged a live advisory published
      2026-06-22; minimal Cargo.lock-only bump (no reqwest update, no ignore).
      `cargo audit --deny warnings` + `cargo deny check` green.

## Decisions (mirrored in EXECUTION-ORDER 결정 로그)

- pre.10 before N2.1: plan mandates pre.10 before N2 (L671/L1227); same reject
  path → do it first to avoid double-surgery.
- reason code `unauthenticated_submit` (snake_case) over the spec's hyphenated
  form, matching every other `HttpError.reason` in the codebase.
- ~50 literals → `true` (preserve behavior); default flip only in production
  CLI + the RED test. Avoids a behavioral change to existing tests.
- `admit_submission_json` (boole-core) left unchanged — pre.10's `code` is on the
  node HTTP path (`submit_json`) the miner actually reads; core fixtures intact.

## Hazards

- Config-field default-flip blast radius: 58 LocalNodeConfig literals workspace-
  wide. `cargo build --all-targets` (both feature sets) is the catch-all.
- mining_loop.rs tests are `#![cfg(feature="dev-tools")]`.
