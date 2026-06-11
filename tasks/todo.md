# Batch D-rust (6) + Batch E (3) — 2026-06-10

Goal: EXECUTION-ORDER.md [1] batch D-rust → [2] batch E, run to completion.
Gate strategy (per audit-batch-d-rust-plan.md): each slice RED→GREEN→focused,
separate NotoriAndo commits, ONE combined full gate, ONE push, remote verify.

## Batch D-rust

- [x] D#1 save_state parent-dir fsync — b2fc47f (structural test; crash durability not unit-testable, per plan)
- [x] D#4 nonce-on-reject test + comment fix — cee4e0b (reject path now burns nonce, 409 on replay)
- [x] D#5 AgentSigner passphrase Zeroizing<String> — 9bbe02e (RED type-assert → GREEN; stdin write avoids unzeroized temp)
- [x] D#6 V0Helpers.lean pinned in checker hash — 9925d33 (RED tamper-invisible → GREEN; README pin 201d0457…, verify script + 5 ad-hoc fixtures updated, checker_artifact_hash now pub)
- [x] D#2 seen_tickets cap — ddf9fe7 (RED flood → GREEN clear at 1M; fixtures unaffected)
- [x] D#3 validBefore future cap — d7c0947 (RED 10y-future confirmed; cap = 300s producer window + 60s leeway → 400 bad_payload; 15 helpers renamed valid_before_fresh at now+60; 14 swept files green)

## Batch E

- [x] E#1 --deterministic-nonces dev-tools gate — 9d64a7d (RED help-text → GREEN both feature configs; boole-mcp library use noted as closed-local)
- [x] E#2 sorry e2e rejection test — fce509a (Err contains "sorry", fires pre-lake, no toolchain gate)
- [x] E#3 invalidAccepted — c85830d. DECISION: Option B-derived (not plan-recommended A): README/python benchmark schema consumers made pure removal high-blast; deriving (share_accepted && !verify_ok) at all 6 sites keeps schema + makes the sentinel real. D#6 follow-up 2fdb9e4 (benchmark scripts' ad-hoc workspaces needed the V0Helpers pin too — found via runtime_smoke_cli failure)

## Gate / push

- [ ] cargo fmt --all --check
- [ ] cargo clippy --workspace --all-targets --locked -- -D warnings (+ dev-features variant, exact self-test.sh invocation)
- [ ] RUST_TEST_THREADS=1 ./scripts/self-test.sh → grep "^self-test: PASS"
- [ ] verify runtime-smoke-all / proof-to-block-benchmark stages green in gate log (D#6, E#3 touch benchmark surface)
- [ ] docs: EXECUTION-ORDER.md marker move + plan docs status (docs-only commits)
- [ ] push origin HEAD:main, verify local SHA == origin/main, working tree clean

## Review

- 9 commits, one slice boundary each: b2fc47f (D#1) cee4e0b (D#4) 9bbe02e (D#5)
  9925d33 (D#6) ddf9fe7 (D#2) d7c0947 (D#3) 9d64a7d (E#1) fce509a (E#2)
  2fdb9e4 (D#6 follow-up) c85830d (E#3).
- Deviations from plan, with reasons:
  - D#6 blast radius was larger than spec'd: the strict V0Helpers pin also hit
    5 ad-hoc test checker fixtures and 2 benchmark scripts (workspace writers +
    python hash mirrors). Found via focused tests, not the plan.
    `checker_artifact_hash` is now pub so tests/scripts can use the production
    formula instead of drifting mirrors (canonical_checker fallback rewritten).
  - D#3 used 300s+60s leeway (not bare 300s): producers stamp now+300, so the
    raw cap would bounce any producer clock ahead of the server. Error is 400
    bad_payload (a contract violation), not 401 envelope_expired.
  - E#3 chose Option B-derived over plan-recommended A: README + python
    benchmark consumers made removal high-blast; deriving
    (share_accepted && !verify_ok) keeps the schema and makes the sentinel
    real. 12 existing == 0 assertions now check a derived value.
  - D#1/D#4/E#2 are pass-on-first-run by nature (structural durability /
    characterization of already-correct code); real RED was confirmed for
    D#2, D#3, D#5, D#6, E#1.
- Pre-existing host flake identified: real_checker 5000ms budget blows under
  concurrent cargo builds (clean-main reproduced). Green on idle host 4/4;
  full gate runs serially so unaffected.
- Full gate: `self-test: PASS` (GATE_EXIT=0, 2026-06-11, /tmp/batch-de-gate.log).
  All stages green, including the consensus-path-required runtime-smoke-all,
  proof-to-block-benchmark, and local-mining-smoke; rust-parity (pof fixture
  regeneration + second full cargo test) also PASS.
- Not a public mining / paid API benchmark claim; closed local validation only.
