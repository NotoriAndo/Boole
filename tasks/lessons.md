# Session lessons

## 2026-05-09 — S14 Green build error: `Path` extractor

**Pattern:** when adding a new axum handler that uses the `Path<...>` URL-parameter extractor, always grep the existing handlers in the same file for `Path` first. `crates/boole-node/src/local_node.rs` aliases `axum::extract::Path as AxumPath`, and every existing handler (block-by-height, account-by-pk, bounty-by-id, bounty-proof) uses `AxumPath(...): AxumPath<String>`. Writing the new `bounty_status_handler` with the unaliased `Path(id): Path<String>` produced three compile errors at once.

**Rule:** before writing the first occurrence of a new pattern in an existing file, run `grep -n '<symbol>' <path>` to confirm whether the file uses an aliased or fully-qualified import. Cheaper than reading a 1k+ LOC file end-to-end and cheaper than a failed compile.

## 2026-05-09 — Audit existing crates before forking a "shared" module

**Pattern:** S15's original plan (in tasks/todo.md) listed `crates/boole-miner/src/hash.rs` and `fixtures/protocol/miner/hash-vectors.json` as new files mirroring pof TS's hash layer. A 30-second `find /Users/seoyong/projects/Boole/crates -name "hash*"` revealed that `boole-core::hash` already exposes every primitive the miner needs (`ticket`, `share_hash`, `share_score`, `digest_to_biguint`, `submission_pow_hash`, `submission_pow_ok`, `min_share_score`, `Hex32`) and is already cross-checked against pof byte-for-byte via `fixtures/protocol/hash-pow/v1.json`. Forking it into the miner crate would have duplicated 130 LOC + a fixture for zero gain.

**Rule:** before scaffolding a "new" module that ports a pof TS file, run `find crates -name '<topic>*'` and `grep -rn '<key symbol>' crates --include="*.rs"`. If boole-core (or another existing Rust crate) already exposes the primitives, depend on it instead of forking. Update tasks/todo.md to drop the duplicated entries and document the deviation in the slice's Review.

## 2026-05-09 — pof wire format ≠ Boole wire format

**Pattern:** pof's `hash-vectors.json` uses 8-byte ticket nonce, 4-byte `j`, 8-byte `nonceS`; Boole's `boole-core::ticket(c, pk, n: &Hex32, ...)` already takes 32-byte nonces. The consensus layer (admission, share-pool, validator) and HTTP contract have been built on 32-byte wire format since earlier slices. The miner port MUST match Boole's consensus, not pof's wire format — otherwise the dispatcher rejects every share with a hash mismatch.

**Rule:** when porting a pof component, the source-of-truth for wire shapes is the **Boole consensus layer**, not the pof TypeScript reference. Inspect `boole-core::admission_types`, `boole-core::hash`, and the HTTP fixture in `fixtures/protocol/http-contract/v1` before building a wire-emitting component. Document any deviation from pof in the slice's Review section so future slices don't re-port pof's narrower types.

## 2026-05-09 — Communication Routing for Telegram-initiated work

**Pattern:** S13b's completion report was delivered via terminal/text output because the inbound Telegram chat_id was not in the current session context (compaction wiped the channel tag). The user reads Telegram, not the local session transcript, so the report effectively went nowhere.

**Rule:** captured in global CLAUDE.md. Tasks initiated via Telegram MUST be reported back via Telegram using `reply` with the originating chat_id. If chat_id is not available after compaction, explicitly ask where to deliver the report rather than silently defaulting to the terminal. `edit_message` is fine for incremental updates; send a fresh reply for completion so the user's device pings.

## 2026-05-10 — `max_requests` counts connections, not http-call sites

**Pattern:** S21's `hard_guard_regression` integration tests hung indefinitely after a clean reboot. Root cause was off-by-one in `max_requests`: test 1 had `max=2` but issued only 1 HTTP call; test 2 had `max=8` but issued 7. `serve_local_node`'s `ConnectionLifetime::drop` ticks the served counter once per closed connection (each test client uses `Connection: close`), and `with_graceful_shutdown` only fires once `served >= max`. So when the test count is wrong, the last `boot.handle.join()` blocks forever — superficially indistinguishable from a network/runtime hang. Diagnostic time burned: ~30 min, including a Mac reboot to rule out TIME_WAIT exhaustion.

**Rule:** when writing a new integration test using `LocalNodeConfig.max_requests`, count the literal `http_get/http_post` call sites in the test body and pass exactly that number. Treat `max_requests` as **connection count**, not request count or some round number. If the test hangs at `boot.handle.join()`, the very first hypothesis to check is the counter value, not the network stack.

## 2026-05-10 — PoLW work is verifier-defined; benchmark measures the (model × verifier) joint signal

**Pattern:** when explaining why Boole needs both a live mining session AND a model benchmark, I initially framed the benchmark as "measuring model capability." A user pushed back: it is verifier-pass capability, not model capability per se. PoLW's "Work" is whatever a given verifier admits — a model that is brilliant at general reasoning but doesn't conform to the verifier's expected output shape produces zero work. Conversely, a model narrowly tuned to satisfy the verifier produces positive work even without general capability. So "work" is a function of the (model, verifier) pair, not the model alone.

**Rule:** when discussing benchmarks, gates, or capability measurement in Boole:
- Do not say "model capability" as the measured quantity. Say **verifier-pass capability under verifier V** or **(model, verifier) joint signal**.
- The benchmark's spread ≥10pp acceptance bar is not a "model capability discriminator" — it is evidence that **the verifier itself is sound** (vacuous verifiers give spread=0; over-strict ones give pass-rate=0). It is simultaneously a model-discrimination signal AND a verifier-soundness signal; the two cannot be separated by design.
- When framing the difference between mining-session evidence (Gate P5) and benchmark evidence (Gate B-Live), state it as: mining session proves "the integrated system runs to completion"; benchmark proves "the verifier is sound and the system meaningfully discriminates." Both gates are non-CI by design (live model cost, non-determinism), but they answer different questions and must not be collapsed into one.
- This framing matters when designing future verifiers (Phase N6+ family manifests): the verifier is the actual discriminator; the model pool is just the substrate the verifier is applied to.

## 2026-05-10 — Mining session and benchmark are scale modes of the same pipeline

**Pattern:** when discussing P5 (live mining session) vs B-Live (model benchmark), I framed them as separate systems with different purposes. A user pushed back: "can't we just compute the benchmark numbers from a mining session's verified-share / share-reward / proposer-bonus rates?" That is exactly correct. The benchmark is not a different tool — it is the same mining pipeline run as **(M models × N attempts)** with seed-controlled inputs, replay verification, and metadata capture. I had artificially separated them.

**Rule:** when explaining the difference, frame it as **operational scale difference**, not architectural difference:
- **Mining session (P5)** = "does one miner run end-to-end against the integrated system" — single model, 1–2 cycles, evidence is "the chain accepted the share / committed the block / paid the credit."
- **Benchmark (B-Live)** = same pipeline at statistical scale — multi-model pool × N≥50 attempts, with four operational additions on top:
  1. **Multi-model dimension** so you get spread across models (spread=0 → verifier is vacuous; only visible with M≥2)
  2. **Seed/target control** (same problem, swap only the model — otherwise the signal isn't isolated to the model)
  3. **Replay verification** (record-and-replay each row → `replayPassed` ratio = anti-fraud signal)
  4. **Metadata capture** (model id, verifier hash version, backend, cost) so historical rows can be re-interpreted under future verifier upgrades
- They are not separate tools. They share the same code paths in `boole-miner` + node `/submit` + `/bounties/{id}/proof`. The benchmark is "mining session × operational discipline × statistical scale."
- Avoid presenting them as architecturally distinct — that misleads on what work needs to ship for each gate.

## 2026-05-10 — Migration scripts: clean stale runtime state before declaring failure

**Pattern:** when a freshly migrated `boole-miner-smoke.sh` failed with `boole-node did not become ready: [Errno 61] Connection refused`, my first instinct was to assume my migration broke something. The actual cause was a **stale `/tmp/boole-node-rewards.ndjson` reward ledger** from a previous (pre-migration) test run — the node was crashing at boot with `reward ledger divergence at pk … ledger=2 replay=0`, and the readiness probe just observed the closed port. The script's trap clears `BLOCK_STORE` but not the reward ledger.

**Rule:** when a smoke script fails with "did not become ready" or "connection refused" *immediately* after a migration:
- Do not assume the migration is broken. First diagnose: run the inner command (e.g. `cargo run -q -p boole-node -- run-local …`) directly without trap-cleanup so error output survives.
- Check for **persistent runtime state** the script does not clean: `/tmp/boole-node-rewards.ndjson`, lockfiles, leftover state directories. Most local boole-node smoke scripts share these tmp paths across runs.
- After resolving, consider whether the script *should* clean it on entry. For per-test isolation, prefer `mktemp -d` paths over fixed `/tmp/boole-node-*.ndjson` names — but a one-off `rm -f` on entry is acceptable when the path is already a known fixed location.

## 2026-05-10 — When migrating tests across two implementations, distrust fixtures that work around bugs

**Pattern:** I made `parse_decimal_nanos` public to fix `chain_head` accepting `MinShareScoreMultiplier: 1.0` (the actual wire format from boole-node `/head`). My new tests with `1.0` and `0.5` passed, but a pre-existing test `test_fetch_head_min_share_score_uses_t_share_and_multiplier` started failing because it used `"MinShareScoreMultiplier": 1000000000` (raw nanos) — a fixture that compensated for the buggy `as_u64()` parser by writing pre-multiplied integer nanos instead of the protocol's decimal.

**Rule:** when fixing a parser that previously had a bug, audit existing test fixtures for the **inverse pattern**. If parser P used to compute `f(x)` but should compute `g(x)`, any fixture written as `f⁻¹(target)` now produces a wrong value. Check:
- Pre-existing fixtures that look "magic" (round numbers, computed constants) — they may encode the old wrong path.
- Tests whose expected values are derived from the input via the buggy code rather than the protocol spec.
- If in doubt, check the actual wire format (here: live `curl /head | jq`) and align fixtures with reality, not with the previously-passing tests.

## 2026-05-10 — Telegram-initiated task: every reply goes via Telegram, not just the completion ping

**Pattern:** A Telegram message arrived asking a clarification question (`lake-verify 와이어업이 테스트를 말하는거야?`). I answered with terminal text output, treating it like an interactive CLI question. The user pushed back: "내가 텔레그램으로 대답하라고 했지." The global rule in `~/.claude/CLAUDE.md` says **all** replies for Telegram-initiated tasks go via Telegram — not only the final completion ping. Mid-conversation Q&A counts.

**Rule:** if the inbound `<channel source="telegram" ...>` block is present in the *current* message, the reply MUST go via `mcp__plugin_telegram_telegram__reply` with the originating chat_id. This applies to:
- Clarifying questions back to the user (don't ask via terminal)
- Direct answers to user follow-up questions (don't answer via terminal)
- Status updates / blockers / decision requests (don't post via terminal)
- Final completion summaries (already covered)

The terminal output is invisible to the user. Re-asking "should I do X or Y?" via terminal stalls the conversation indefinitely. Treat the Telegram chat as the sole channel for the entire task lifecycle, not just the final report.

**Self-check before sending any text response:** if the most recent inbound message has a `<channel source="telegram">` tag, my next user-facing response goes through `reply`, not bare text. Bare terminal text in a Telegram-initiated session = silent message from the user's perspective.

## 2026-05-12 — Pre-existing test failures must be verified against clean main before blaming a slice

**Pattern:** W0.2 (`wallet: redact local key secrets from default output`) green-checked `boole-cli` (13/13), `keys_sign` (5/5), `keys_verify` (5/5). The wider `cargo test --workspace --all-targets` then failed on `boole-lean-runner::canonical_checker_artifact_hash_matches_readme_pin` with a SHA-256 drift between the recomputed `BooleCheck/Main.lean + lakefile.lean` hash and the README-pinned value. My edits touched only `crates/boole-cli/src/main.rs` and `crates/boole-cli/tests/keys.rs` — nothing under `lean/` or `boole-lean-runner` — so the failure was suspicious on its face. `git stash && cargo test ... canonical_checker_artifact_hash_matches_readme_pin` against unmodified `main` reproduced the same failure with the same hashes, confirming it as a pre-existing drift independent of W0.

**Rule:** when a workspace test fails *after* a focused slice whose diff is confined to unrelated crates:

1. Run `git diff --stat` to confirm the slice touched none of the failing test's source paths.
2. `git stash && cargo test -p <failing-crate> --test <failing-test> <failing-fn>` against clean `main`.
3. If the test fails identically on clean `main`, it is pre-existing drift, not regression. Document it in the slice's review note ("workspace tests: <list> pre-existing, unrelated to this slice — green gate is per-crate") and proceed.
4. If it fails differently or only after your changes, treat it as a real regression and `git stash pop` to debug.

This matters for the wallet plan specifically: the Global verification commands at the top of `local-docs/boole-agent-wallet-node-integration-implementation-plan.md` say "workspace tests PASS" as a final-condition gate. The practical interpretation is "no new failures vs clean main", not "no failures at all" — otherwise an unrelated lean checker drift would block every wallet slice indefinitely. Fix unrelated drift in its own slice; do not entangle.
