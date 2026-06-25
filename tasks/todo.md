# N2.3 — duplicate-proof / cross-pk farming reject — 2026-06-24

Goal: EXECUTION-ORDER [8] last remnant. A `/submit` proof credited once: the
same canonical proof bytes resubmitted under any prover pk is rejected with a
server-computed dedup key (never a client field). origin/main=4db6174 at start.

## Design (from parallel exploration)

- Dedup key = `SHA256(hex::decode(body["bytes"]))` — server recomputes; matches
  admission.rs:141 `canon_hash` (normalize_pow_fields only touches n/j/nonceS,
  not bytes). `/submit` carries no client `proofHash`, so nothing to forge.
- New durable ledger `proof_dedup_ledger.rs` modeled on `signed_nonce_ledger.rs`
  (HashSet + tagged NDJSON `Credit{canonHash}` + durability helpers + recover).
- Opt-in via `LocalNodeConfig.proof_dedup_ledger_path: Option<PathBuf>` (None =
  no dedup, preserves ~58 existing literals). CLI `--proof-dedup-ledger`.
- Guard in `submit_json` (write-lock held throughout):
  - CHECK after `Accepted` match, before burn (≈4005): canon_hash in set →
    `{ok:false, accepted:false, reason:"duplicate_proof", code:"duplicate_proof"}`
    (200, consistent with admission-reject convention; NOT 409 — that's the
    session-gate layer).
  - INSERT after block commit (≈4069): record canon_hash only on actual credit
    (NoProposer/Ambiguous early-returns do not record → "credit once").
- Test fixture: `fixtures/protocol/runtime-smoke/multiminer.v1.json` — 3 steps,
  identical `bytes`, different pk/n/j/nonceS.

## Steps

- [x] proof_dedup_ledger.rs (+ 3 unit tests 3/3) + `mod` in lib.rs → GREEN
- [x] `tests/no_duplicate_proof_credit.rs`:
      `same_proof_under_two_pks_credits_once`,
      `proof_dedup_key_is_server_computed_not_client_field`
- [x] plumbing: config field + state + from_config rehydrate + 56 literals
      (None; +new test Some + main.rs args = 58 total) + main.rs CLI flag
      `--proof-dedup-ledger`
- [x] guard in submit_json (CHECK after Accepted before burn; INSERT after
      block commit so NoProposer/Ambiguous accepts do not consume the slot)
- [x] focused GREEN 2/2 + behavioral-RED rigor check (guard `if false` → both
      FAIL: step1 admitted at live head, block 1 credited aaaa, reason Null)
- [ ] full gate `self-test: PASS` (consensus path — confirm runtime-smoke-all /
      proof-to-block-benchmark green in log)
- [ ] commit (NotoriAndo) + push + remote verify; lessons.md (fmt-before-commit,
      dyld first-launch, guard-after-admission) folded into this push

## Hazards

- Config-field blast radius: ~58 LocalNodeConfig literals → `None` (N2.1 lesson).
- Conflict: dedup ledger must NOT be read on the consensus replay path
  (reputation-style read-only); boot reconciler join is sibling-plan L7 (not
  this slice).
- macOS dyld: fresh test binary first launch is 30-60s; run via `cargo test`
  (no short `timeout`), let it pay the cost.
