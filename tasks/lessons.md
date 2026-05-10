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
