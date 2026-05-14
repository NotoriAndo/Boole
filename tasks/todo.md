# Phase C4 / Gate P5 — `crates/boole-miner` Rust port + `boole mine start` / `mine bounty` CLI

## Goal

Port pof's TypeScript `boole-miner` package (~3346 LOC src + ~1773 LOC tests, single binary today) into a new `crates/boole-miner` Rust library + binary, and wire `mine start` / `mine bounty` subcommands into `boole-cli`. After completion:

- `cargo run -p boole-cli -- mine start --node URL --max-cycles N` runs the full pipeline (head → ticket → M × {target → LLM → verify → canonicalize → share → submit-PoW → submit}) end-to-end against a live `boole-node`.
- `cargo run -p boole-cli -- mine bounty --node URL --id <id> --prover <key> --envelope-path …` posts a single bounty proof.
- Pure-fixture tests (no live model, no live network) cover all branches; live-model + live-node tests stay out of CI (gated behind env flags).
- `boole-miner` is published as both a library (consumable from `boole-cli`) and an optional thin binary mirroring the pof CLI surface.

## Scope decisions (locked)

1. **Single new crate, library-first.** `crates/boole-miner` exposes all public surface as a library; the optional binary `boole-miner` is a thin wrapper. `boole-cli` consumes the library directly so `mine start` / `mine bounty` are first-class subcommands.

2. **Feature flags isolate external toolchains.**
   - `lake-canon` — enables `LakeCanonicalizer` (`lake exec boole_emit`); off by default in CI.
   - `lake-target` — enables `LakeTargetEmitter` (`lake exec gen_target_emit`); off by default in CI.
   - `lake-verify` — enables `LeanVerifier` (`lake exec verify_emit` + `lake env lean`); off by default in CI.
   - LLM backends behind `llm-anthropic`, `llm-openai`, `llm-google`, `llm-cli` cargo features; pure tests use `MockDriver`.

3. **POFP wire format owned by `boole-miner`, not `boole-core`.** The TS package keeps its own copy; we mirror that policy. If a future slice needs the encoder shared with `boole-node`, we lift it to `boole-core` then.

4. **State persistence reuses pof's path layout.** `~/.config/boole-miner/state.json` (or `$BOOLE_MINER_HOME` / `$XDG_CONFIG_HOME` overrides), schema-versioned envelope `{schemaVersion, sk, pk, address, createdAt, config}`. Mirrors S6 keys-store hardening (mode 0600, atomic tmp+rename).

5. **Wire constants stay byte-frozen with pof.** BLAKE3 domain tags (`ticket` / `share` / `submit` / `target`), SHA-256 for `H_canon`, POFP magic `POFP`, level/expr/lit tag bytes — all asserted against shared fixture vectors at `fixtures/protocol/miner/hash-vectors.json` (NEW).

6. **Slicing locks shippable units.** Each slice ships behind green tests + clippy + a Telegram completion ping. No half-shipped slice gates the next.

## Slice plan (5 slices)

### Slice S15 — Hash layer + PoW grinders + crate skeleton — Phase C4 entry

**Scope:** create `crates/boole-miner` (`Cargo.toml`, `src/lib.rs`), port the hash primitives and three pure grinders. Pure functions, no I/O, no async, no external services — so CI is deterministic from day one.

**File inventory:**
- `crates/boole-miner/Cargo.toml` — NEW. Dependencies: `blake3`, `sha2`, `hex`, `serde`, `serde_json`, `anyhow`. Library + binary skeleton.
- `crates/boole-miner/src/lib.rs` — NEW. Module declarations.
- `crates/boole-miner/src/hash.rs` — NEW. `H_protocol(domain, &[&[u8]]) -> [u8; 32]` (BLAKE3 with domain prefix), `H_canon(&[u8]) -> [u8; 32]` (SHA-256), `score_share(hash, t_block) -> u64`, `score_to_target(...)` math.
- `crates/boole-miner/src/grinder/mod.rs` — NEW.
- `crates/boole-miner/src/grinder/ticket.rs` — NEW. `grind_ticket(c, pk, t_ticket, opts) -> Option<TicketGrindResult>`. Counter-nonce iteration, `H_protocol("ticket", c, pk, n) < t_ticket`, max-attempts cap, optional progress callback.
- `crates/boole-miner/src/grinder/share.rs` — NEW. `grind_share(c, pk, n, canon_hash, min_share_score, t_block, opts) -> Option<ShareGrindResult>`. Sets proposer flag when hash < t_block.
- `crates/boole-miner/src/grinder/submit_pow.rs` — NEW. `grind_submission_pow(c, pk, canon_hash, t_submit, opts) -> Option<SubmitGrindResult>`. `H_protocol("submit", c, pk, nonce_s, canon_hash) < t_submit`.
- `crates/boole-miner/tests/hash_vectors.rs` — NEW. Loads `fixtures/protocol/miner/hash-vectors.json` and asserts byte-equal output against pof TS's `hash.test.ts` fixtures.
- `crates/boole-miner/tests/ticket_grinder.rs` — NEW. 4 tests: success path, max-attempts exhaustion, progress callback fires every N attempts, deterministic result for a fixed seed.
- `crates/boole-miner/tests/share_grinder.rs` — NEW. 4 tests: success path, proposer flag set when hash < t_block, canon_hash is rejected if not 32 bytes, max-attempts.
- `crates/boole-miner/tests/submit_pow_grinder.rs` — NEW. 3 tests: success path, max-attempts, threshold respected.
- `fixtures/protocol/miner/hash-vectors.json` — NEW. Mirror of pof TS hash test vectors.
- `Cargo.toml` (workspace) — add `boole-miner` to `members`.

**TDD plan:**
- Red: write all 4 test files first (15 tests total). They fail because `boole-miner` crate doesn't exist.
- Green: implement crate skeleton + hash + grinders. All 15 tests pass.

**Acceptance:** 15 new tests + 0 modified existing tests. Full regression `cargo test -p boole-core -p boole-node -p boole-cli -p boole-miner` clean. Clippy `--all-targets -- -D warnings` clean. Doc updates: §1.3 dashboard row for miner crate flips ❌ MISSING → ⏸ PARTIAL (skeleton + pure layer shipped); Appendix A · S15 entry; §11 ship log.

### Slice S16 — POFP wire format + StructuralCanonicalizer

**Scope:** port `lib/proofPackage.ts` (encoder + walker) and the `StructuralCanonicalizer` placeholder (proof embedded in a string literal). LakeCanonicalizer (production lake-call path) deferred to S19 as a feature-gated implementation.

**File inventory:**
- `crates/boole-miner/src/proof_package.rs` — NEW. POFP magic `POFP`, level/expr/lit tag bytes, `BppkBuilder`, `walk_bppk`, `encode_placeholder_bppk(seed_hex, profile, proof_source) -> Vec<u8>`.
- `crates/boole-miner/src/canonicalizer/mod.rs` — NEW. `Canonicalizer` trait (`fn canonicalize(seed_hex, profile, proof_source) -> Result<CanonResult, String>` where `CanonResult { bytes, hash }`).
- `crates/boole-miner/src/canonicalizer/structural.rs` — NEW. `StructuralCanonicalizer` impl using `encode_placeholder_bppk`.
- `crates/boole-miner/tests/proof_package.rs` — NEW. 5 tests: round-trip encode→walk byte-equal, fingerprint distinctness across distinct inputs, magic header check, level/expr/lit tag invariants, placeholder bppk well-formedness.
- `fixtures/protocol/miner/pofp-vectors.json` — NEW. Mirror of pof TS proof-package test vectors.

**Acceptance:** 5 new tests. Doc updates: dashboard, Appendix A · S16, §11.

### Slice S17 — HTTP clients + state persistence

**Scope:** port `submit.ts` (`POST /ticket`, `POST /submit`), `chainHead.ts` (`GET /head`), `bountyMode.ts` (`POST /bounties/:id/proof`), and `state.ts` (ed25519 keygen, address derivation, state.json persistence).

**File inventory:**
- `crates/boole-miner/src/http_client.rs` — NEW. Shared `HttpClient` wrapper around `reqwest` (or `ureq` for blocking simplicity — TBD).
- `crates/boole-miner/src/submit_client.rs` — NEW. `SubmitClient::announce_ticket(c, pk, n)` and `SubmitClient::submit(c, pk, n, j, nonce_s, bytes)`.
- `crates/boole-miner/src/chain_head.rs` — NEW. `HttpChainHeadFetcher::fetch() -> Result<ChainHead>`. `ChainHead { c, t_ticket, t_share, t_block, t_submit, min_share_score, m, d, profile, n? }`.
- `crates/boole-miner/src/bounty_client.rs` — NEW. `BountyClient::submit_proof(BountyProofInputs) -> Result<BountyProofResult>`.
- `crates/boole-miner/src/state.rs` — NEW. `MinerState { schema_version, sk, pk, address, created_at, config }`. `generate_miner_state()`, `pubkey_to_address()` (bech32), `load_state(path)`, `save_state(path)` with mode-0600 atomic write.
- `crates/boole-miner/tests/submit_client.rs`, `chain_head.rs`, `bounty_client.rs`, `state.rs` — NEW. ~15 tests total using `wiremock` (or `httpmock`) for HTTP, `tempfile` for state fixtures.

**Acceptance:** ~15 new tests. Doc updates: dashboard, Appendix A · S17, §11.

### Slice S18 — LLM driver + withRetry

**Scope:** port `llmDriver.ts`. Backends gated behind cargo features so a default build is dependency-light.

**File inventory:**
- `crates/boole-miner/src/llm/mod.rs` — NEW. `ProverDriver` trait, `GenerateResult { proof_source, raw }`, `extract_proof_source(text)` parser.
- `crates/boole-miner/src/llm/mock.rs` — NEW. `MockDriver` for tests.
- `crates/boole-miner/src/llm/anthropic.rs`, `openai.rs`, `google.rs` — NEW. Behind `llm-{anthropic,openai,google}` cargo features.
- `crates/boole-miner/src/llm/agent_cli.rs`, `claude_cli.rs`, `openai_compat.rs` — NEW. Behind `llm-cli` feature.
- `crates/boole-miner/src/llm/retry.rs` — NEW. `with_retry(driver, prompt, opts)` (max 3, exp backoff, no retry on `rejected`).
- `crates/boole-miner/tests/llm_driver.rs` — NEW. ~10 tests using `MockDriver` + `tokio::process::Command` stubs for CLI backends.

**Acceptance:** ~10 new tests. Doc updates: dashboard, Appendix A · S18, §11.

### Slice S19 — Mining loop + CLI wiring (`boole mine start` / `mine bounty`)

**Scope:** port `miningLoop.ts` (the orchestrator), `targetGen.ts` (Stub + optional Lake emitter), `localVerify.ts` (Lean verifier), and the production `LakeCanonicalizer`. Wire `mine start` / `mine bounty` clap subcommands into `boole-cli`. Optional thin `boole-miner` binary for parity with pof's standalone surface.

**File inventory:**
- `crates/boole-miner/src/target_emitter.rs` — NEW. `TargetEmitter` trait, `StubTargetEmitter`, optional `LakeTargetEmitter` (feature `lake-target`).
- `crates/boole-miner/src/local_verify.rs` — NEW. `LeanVerifier` (feature `lake-verify`).
- `crates/boole-miner/src/canonicalizer/lake.rs` — NEW. `LakeCanonicalizer` (feature `lake-canon`).
- `crates/boole-miner/src/mining_loop.rs` — NEW. `run_mining_loop(deps, opts) -> MiningLoopSummary`. Event types, prompt builder, cookbook.
- `crates/boole-miner/src/cli.rs` — NEW. Library-internal clap subtree usable from both `boole-cli` and the optional `boole-miner` binary.
- `crates/boole-miner/src/bin/boole-miner.rs` — NEW. Thin binary wrapper.
- `crates/boole-cli/src/main.rs` — `MineCommand::Start { … }`, `MineCommand::Bounty { … }` clap variants delegating to `boole_miner::cli`.
- `crates/boole-miner/tests/integration.rs` — NEW. End-to-end with stub collaborators (StubTargetEmitter + MockDriver + StructuralCanonicalizer + mocked HTTP) producing one accepted share.
- `crates/boole-cli/tests/mine_start_cli.rs`, `mine_bounty_cli.rs` — NEW. CLI smoke tests using the library's stub seam.

**Acceptance:** ~20 new tests across the integration + CLI surface. Manual smoke `cargo run -p boole-cli -- mine start --node http://127.0.0.1:PORT --max-cycles 1 --mock-llm-response …` produces an accepted share against a live `boole-node`. Phase C4 complete; Gate P5 PASS contingent on a 5-cycle live run (manual evidence). Doc updates: §1.2 (`boole mine start` / `mine bounty` rows flipped to PASS), §1.3 dashboard miner row → ✅ PASS, §1.4 dashboard, Appendix A · S19, §11.

## Sequencing

S15 → S16 → S17 → S18 → S19. S16/S17/S18 are independent of each other once S15 lands; in practice we ship them in numeric order because each builds the substrate the next consumes (S19 needs all four).

## Out of scope (deferred)

- Live model evidence runs (Gate B-Live posture stays unchanged).
- MCP daemon equivalent.
- `keys rotate / upgrade` + schema v3 sealed-box `sk` (S13a follow-up).
- `--allowlist <path>` for announce / status-change pks.
- RFC 8785 full canonicalization parity.
- Bounty consensus integration (N5b — separate parallel track).

## Review (filled in after each slice)

### S15 — Hash + PoW grinders + crate skeleton — SHIPPED 2026-05-09

- `crates/boole-miner` added to workspace; library-only (binary deferred to S19).
- Grinder layer: `grind_ticket`, `grind_share`, `grind_submission_pow` over Boole's 32-byte `Hex32` wire format. Pluggable `NonceSource` (`CounterNonce` for tests, `OsRngNonce` for production). `GrinderConfig` carries `max_attempts` + `report_every_hashes`; progress callback is a separate `Option<&mut dyn FnMut(GrindProgress)>` parameter.
- Hash primitives reused from `boole-core::hash` (already cross-checked vs pof TS via `fixtures/protocol/hash-pow/v1.json`); the standalone `crates/boole-miner/src/hash.rs` and `fixtures/protocol/miner/hash-vectors.json` files planned in the original inventory were dropped — boole-core is the canonical hash layer, no need to fork it.
- Wire-format divergence vs pof: pof uses 8-byte ticket nonce / 4-byte j / 8-byte nonceS; Boole consensus already standardised on 32-byte everywhere (`Hex32`). Grinders match Boole, not pof. Documented; no action needed downstream.
- Tests: 16 new (5 ticket, 5 share, 3 submit-pow, 3 nonce-source). All pass. `cargo clippy -p boole-miner --all-targets -- -D warnings` clean.
- Regression: `cargo test -p boole-miner -p boole-core -p boole-node -p boole-cli -- --test-threads=1` clean. Pre-existing `boole-lean-runner::canonical_checker_artifact_hash_matches_readme_pin` drift unrelated to S15 (lakefile.lean was already modified before this slice).
- Doc updates pending in next commit batch: dashboard row flip (❌ → ⏸), Appendix A · S15, §11 ship log.

### S16 — POFP wire format + StructuralCanonicalizer — SHIPPED 2026-05-09

- `crates/boole-miner/src/proof_package.rs` ports the POFP wire format byte-for-byte: `MAGIC = "POFP"`, `FORMAT_VERSION = 1`, level/lit/expr tag bytes, `BppkBuilder` (push, push_u32_le, push_bytes, push_string, push_name), `walk_bppk` with typed `BppkDecodeError` (BadMagic / UnsupportedVersion / UnexpectedEof / UnknownTag / RecursionLimit / TrailingBytes), and `bppk_canon_hash` (SHA-256). `MAX_WALK_DEPTH = 4096` matches pof.
- `crates/boole-miner/src/canonicalizer/{mod.rs,structural.rs}`: `Target { seed_hex, d, profile, n, render }` mirrors pof's `Target`. `Canonicalizer` trait (sync — async wrapper deferred until S18/S19 needs it). `StructuralCanonicalizer` emits a placeholder POFP whose `theoremName` includes a 16-hex-char fingerprint of `sha256("bppk-name" || seed_hex || profile || "D=<d>" || "N=<n>")` and whose `proofExpr` is `lit (strVal proof_source)`.
- `LakeCanonicalizer` deferred to S19 (needs process spawn + tokio).
- Tests: 13 new in `tests/proof_package.rs` covering walk-success, magic+version layout, distinct-proofs canonHash, distinct-targets canonHash, canonHash == sha256(bytes), determinism, proofSource embedded as strVal, plus 5 walk_bppk negative cases (badMagic, unsupportedVersion, unexpectedEOF, recursionLimit at 5000-deep succ, trailingBytes), plus a builder round-trip. All pass.
- Cross-crate regression: `cargo test -p boole-miner -p boole-core -p boole-node -p boole-cli -- --test-threads=1` clean. `cargo clippy -p boole-miner --all-targets -- -D warnings` clean.
- Cumulative: 29 tests in boole-miner (16 from S15 + 13 from S16).
- Deferred: `fixtures/protocol/miner/pofp-vectors.json` cross-check vs pof TS — pof has no equivalent fixture (its tests are structural via walkBppk). Adding one would require running `npx vitest` in pof to dump expected hex; defer to S19 along with LakeCanonicalizer if/when LakeCanonicalizer's deterministic 1765-byte SMOKE_TARGET fixture is needed for cross-language cross-check.

### S17 — HTTP clients + state persistence — SHIPPED 2026-05-09

- `crates/boole-miner/src/http_client.rs`: synchronous raw-`TcpStream` `HttpClient::{post_json, get}` mirroring `crates/boole-cli/src/main.rs` (`http_post`/`http_get`/`parse_http_response`). Only `http://` URLs supported. Default timeouts: 30s for submit/bounty, 10s for chain-head. Includes RFC-3986 `percent_encode_component` for path-segment escaping. No new HTTP crate dependency added.
- `crates/boole-miner/src/submit_client.rs`: `SubmitClient::{announce_ticket, submit}` with typed `AnnounceTicketResult` (Observed/Replay/Rejected/NetworkError) and `SubmitResult` (Accepted/Rejected/RateLimited/NetworkError). Status mapping per pof contract: 200(accepted=false) → Rejected{422}, 429 → RateLimited, 400/422 → Rejected with optional field/detail.
- `crates/boole-miner/src/chain_head.rs`: `HttpChainHeadFetcher::fetch_head() -> Result<ChainHead, ChainHeadError>` parses Boole's `/head` body (c, T_ticket/T_share/T_block/T_submit hex, MinShareScoreMultiplier in nanos, M, K_max/L/D_max, provenance) and computes `min_share_score` via `boole_core::min_share_score`. `D`, `profile`, optional `N` are caller-supplied (per-cycle policy, not on chain).
- `crates/boole-miner/src/bounty_client.rs`: `BountyClient::submit_proof(BountyProofInputs{bounty_id, prover_pk, envelope, envelope_bytes})` POSTs to `/bounties/<percent-encoded-id>/proof` with body `{proofHash: sha256(envelope_bytes), prover, envelope}`. Six-variant `BountyProofResult` (Ok/NotFound/Terminal/NoVerifier/BadRequest/NetworkError). Caller decides retry; 409/501 are terminal.
- `crates/boole-miner/src/state.rs`: `MinerState{schemaVersion=1, sk, pk, address, createdAt, config:{dispatcher, llm}}` with `generate_miner_state` (ed25519 via `OsRng.fill_bytes` + `SigningKey::from_bytes`), `load_state`/`save_state` (atomic tmp+rename, mode 0o600 set at open via `OpenOptionsExt::mode`), `state_exists`, `update_config(ConfigPatch{dispatcher_url, llm}, path)`, `signing_key_from_state`/`verifying_key_from_state` accessors. `default_state_path` precedence: `$BOOLE_MINER_HOME` > `$XDG_CONFIG_HOME/boole-miner` > `$HOME/.config/boole-miner`.
- Address-derivation deviation from pof: pof uses `bech32("boole", sha256(pk)[:20])`; Boole uses `address = pk_hex` (matches every other Boole crate, avoids pulling bech32 dep). Documented in module-level comment.
- Tests: 36 new across 5 integration test files (5 http_client, 7 submit_client, 5 chain_head, 6 bounty_client, 10 state). Mock servers use a `TcpListener::bind("127.0.0.1:0")` + thread pattern that captures the request line + body for assertions. State env-mutating tests serialize via a `Mutex` to avoid racing within the binary. All pass.
- Cross-crate regression: `cargo test -p boole-miner -p boole-core -p boole-node -p boole-cli -- --test-threads=1` clean. `cargo clippy -p boole-miner -p boole-core -p boole-cli -p boole-node --all-targets -- -D warnings` clean.
- Cumulative: 65 tests in boole-miner (29 from S15+S16 + 36 from S17).
- Deferred: doc updates (dashboard row, Appendix A · S17, §11 ship log) — batched to commit at end of S19. `LakeCanonicalizer` and CLI binary continue to land in S19.

### S18 — LLM driver + withRetry — SHIPPED 2026-05-09

- `crates/boole-miner/src/llm_driver.rs` ports the provider abstraction layer. Sync surface (no tokio dep), matching the rest of the miner crate. No new external deps.
- Types: `LLMBackend { Mock, ClaudeCli, AgentCli }` (with `as_str` / `parse`), `Strategy { Frontier, OpenWeight, Hybrid }`, `RejectionReason { EmptyResponse, NoProofBlock, NonStringResponse }`, `GenerateResult { Solved{proof_source, elapsed, tokens_used} | Rejected{reason, elapsed} | Error{cause, elapsed} }`. `ProverDriver: Send + Sync` trait object-safe (`fn name`, `fn strategy`, `fn generate`).
- `extract_proof_source` ports pof's regex-based fence stripper. Recognizes ```lean / ```lean4 / unlabeled fences. Empty-or-whitespace input → `EmptyResponse`; empty fence body → `NoProofBlock`; any other case returns the trimmed body (or full text if no fence).
- `MockDriver` for tests: vec of `MockResponse { Text(String) | Error(String) }`, optional latency. Auto-classifies text via `extract_proof_source` so a test can pass canned model output verbatim.
- `ClaudeCliDriver` shells out to `claude -p`, prompt piped via stdin. `AgentCliDriver` invokes `<command> <args...> <prompt>` (prompt as final argv item, no stdin). Both go through a pluggable `ProcessRunner` trait so unit tests use `FakeRunner` that captures call args without spawning processes; production uses `StdProcessRunner`.
- `StdProcessRunner` uses `std::process::Command` + `try_wait` polling at 50ms with deadline-based timeout enforcement (kills the child if exceeded). Handles `NotFound` (binary missing), `Timeout`, `Exit` (non-zero status with stderr capture, ≤500 chars), and generic `Io` errors.
- `with_retry(driver, prompt, RetryConfig{max_attempts, initial_backoff}, sleeper)`: retries only on `Error` outcomes (not `Rejected`); doubles backoff per attempt (1s/2s/4s with the default config). `Sleeper` trait is injectable so tests use a `RecordingSleeper` that captures durations without actually sleeping.
- `create_driver(LLMDriverConfig)` factory: returns `Err(MockNotConstructible)` for the Mock backend (use `MockDriver::new` directly), `Err(AgentCommandMissing)` if `agent_cli` is requested without a command, otherwise builds the appropriate driver.
- Tests: 26 new in `tests/llm_driver.rs` (6 extract_proof_source, 4 MockDriver, 2 ClaudeCliDriver via FakeRunner, 2 AgentCliDriver via FakeRunner, 4 create_driver, 1 LLMBackend round-trip, 4 with_retry, 3 StdProcessRunner real-process smoke against `/bin/echo` and `/bin/sleep`). All pass.
- Cross-crate regression: `cargo test -p boole-miner -p boole-core -p boole-node -p boole-cli -- --test-threads=1` clean. `cargo clippy -p boole-miner --all-targets -- -D warnings` clean.
- Cumulative: 92 tests in boole-miner (16 S15 + 13 S16 + 36 S17 + 27 S18). Test count discrepancy: llm_driver had 26 tests + 1 unintended extra somewhere — actual total verified by `cargo test` summary.
- Deferred: SDK-based backends (Anthropic/OpenAI/Google/OpenAICompat) require a TLS HTTP client; the miner's existing HttpClient is plaintext-only. Adding rustls/reqwest would be a non-trivial dep change. Deferred until a real consumer needs them; for now the production path is `claude_cli` (zero-config OAuth) or `agent_cli` (Claude Code itself). Documented at the top of `llm_driver.rs`.
- Deferred: doc updates batched to commit at end of S19.

### S19 — Mining loop + CLI wiring (`mine start` / `mine bounty`) — SHIPPED 2026-05-09

- `crates/boole-miner/src/target_emitter.rs` (NEW): `TargetEmitter` trait + `target_seed(c, pk, n, j_index)` (`H_protocol("target", c, pk, n, j_be4)` byte-for-byte vs pof). `StubTargetEmitter` (synthetic invariant for fixture-driven tests), `FixedSeedTargetEmitter` (pinned seed/render for reproducible smokes), and feature-gated `LakeTargetEmitter` (deferred — needs lake toolchain spawn).
- `crates/boole-miner/src/local_verify.rs` (NEW): `Verifier: Send + Sync` trait, `VerifyResult{accepted, reason, elapsed, stderr_tail}`, `VerifyReason::{Accepted, EmitFailed, ElaborateFailed, ElaborateTimeout, BinaryNotFound}`, `AcceptingVerifier`, `RejectingVerifier`. `LeanVerifier` gated behind `lake-verify` (pure-Rust scaffolding; real lake-call deferred until lake toolchain is wired into Boole).
- `crates/boole-miner/src/mining_loop.rs` (NEW): `run_mining_loop(deps, opts) -> MiningLoopSummary`. Pluggable `MiningLoopDeps { pk, chain_head, emitter, driver, verifier, canonicalizer, submit_client, prompt_builder, log: LogSink, sleeper }` so all collaborators can be stubbed for tests. `MiningLoopOptions` carries `max_shares`, `max_cycles`, three `GrinderConfig`s, `RetryConfig`, optional `Arc<AtomicBool>` cancel flag, and `deterministic_nonces` (swaps `OsRngNonce` → `CounterNonce` for reproducible CI runs). `MiningEvent` enum surfaces every step (head_fetched, ticket_found/announced/exhausted, target_emitted, llm_outcome, verify_outcome, share_found, share_grind_exhausted, submit_pow_found/exhausted, submit_outcome, cycle_complete, head_fetch_failed). `DefaultPromptBuilder` ships the full pof SYSTEM_PROMPT inline as a `const COOKBOOK: &str`. Sync architecture (no tokio dep) matches the rest of the crate.
- `crates/boole-miner/src/cli.rs` (NEW): clap subtree exposed as a library so `boole-miner` (standalone binary) and `boole mine ...` (boole-cli subcommand) drive the same code paths. `MineCommand::{Init, Address, Config{Get|Set}, Start, Bounty}`. State path resolution falls back to the standard `BOOLE_MINER_HOME → XDG_CONFIG_HOME → ~/.config` precedence. `iso_now()` hand-rolled via Howard Hinnant civil_from_days (no chrono/time dep). `run_start` builds `MiningLoopDeps` from state, prints each event as JSON to stdout, and emits a final `summary: {...}` JSON envelope. `run_bounty` reads the envelope file, computes `proof_hash = sha256(envelope_bytes)` inside `BountyClient::submit_proof`, and prints the typed `BountyProofResult` as JSON.
- `crates/boole-miner/src/bin/boole-miner.rs` (NEW): thin clap wrapper delegating to `boole_miner::cli::run_mine`. Bin target exposed in `Cargo.toml`. Verified `cargo build -p boole-miner --bin boole-miner` produces a working standalone binary.
- `crates/boole-miner/src/submit_client.rs` + `chain_head.rs`: added `Submitter` and `ChainHeadFetcher` traits with blanket impls on the existing concrete clients, so the mining loop only needs `Box<dyn Trait>`. No behavioural change.
- `crates/boole-cli/Cargo.toml` + `src/main.rs`: added `boole-miner` path dep, new `Command::Mine { command: boole_miner::cli::MineCommand }` variant. Single-line dispatch into `boole_miner::cli::run_mine(command)`.
- Tests: 23 new across the integration + CLI surface. `boole-miner/tests/target_emitter.rs` (5), `local_verify.rs` (3), `mining_loop.rs` (6 — full pipeline E2E with `RecordingSubmitter` + `ArcSubmitter` wrapper, `FixedChainHead`, `StubTargetEmitter`, `MockDriver`, `AcceptingVerifier`/`RejectingVerifier`, `StructuralCanonicalizer`; covers one-share happy path, verify-rejected skip, llm-rejected counter, event surface, max-shares stop, announce-rejected abort). `boole-cli/tests/mine_init_cli.rs` (5 — init/address/no-overwrite/config get-set round-trip/secret redaction), `mine_start_cli.rs` (2 — closed-port head-fetch fail summary, unpaired fixed-target-flag rejection), `mine_bounty_cli.rs` (2 — happy-path bounty submit against a real `boole-node` with mock-accept verifier, malformed-prover local rejection). All pass.
- Cross-crate regression: `cargo test -p boole-core -p boole-node -p boole-cli -p boole-miner -- --test-threads=1` clean. `cargo clippy -p boole-miner -p boole-cli --all-targets -- -D warnings` clean (after fixing 4 lints in `cli.rs`/`mining_loop.rs`).
- Cumulative: 115 tests in boole-miner + 9 new CLI smoke tests in boole-cli (16 S15 + 13 S16 + 36 S17 + 27 S18 + 14 S19 lib + 9 S19 CLI). Phase C4 mining loop now lives in Rust end-to-end.
- Deferred: live-model 5-cycle run (Gate P5 evidence) — needs an Anthropic API key + live `boole-node` instance, manual evidence to be captured outside the test harness. `LakeCanonicalizer` (`lake-canon`), `LakeTargetEmitter` (`lake-target`), and `LeanVerifier` (`lake-verify`) feature gates added but real implementations deferred until a Boole consumer wires the lake toolchain into the build. Without `lake-verify`, `mine start` falls back to `AcceptingVerifier` with a stderr warning so smoke runs still complete.
- Deferred: doc updates batched (dashboard rows, §1.2/§1.3/§1.4 dashboard, Appendix A · S15-S19, §11 ship log) — to land in a single follow-up doc commit.

---

# Phase N5b — Bounty consensus / economics integration (parallel track)

## Goal

Close the Hard-Guard gap: bounty proofs feed a side-pool that affects block selection + reward credit only when their `FamilyManifest` is signed and `activation_height ≤ runtime height`, and per-family caps bound their economic share. After completion, parity-plan §1.4 N5b row flips ❌ → ✅ PASS.

## Scope decisions (locked)

1. **Optional `caps` + `signature` on `FamilyManifest`.** Backward-compat with `fixtures/protocol/manifests/v1.json` is preserved via `Option<…> + skip_serializing_if = "Option::is_none"`. Un-promoted manifests behave as before — zero economic effect on block selection.
2. **Hard-Guard regression test in S21.** A 50-proof bounty stream against `activation_height = u64::MAX` must produce byte-identical block selection + replay digest to the no-bounty baseline.
3. **`build_block_selection` extension parameter.** New `promoted_bounty_shares: &[…]` argument; never alters `min_share_score`, `t_block`, `t_share`, or `k_max`. Caps applied in S22.
4. **Reward + replay sweep ship together in S23.** `RewardLedger::credit` on accepted+promoted bounty proofs, plus `verify_ledger_matches_replay` extended to walk `FileBountyEventLedger`.

## Slice plan

### Slice S20 — `FamilyManifest` schema hardening — DELIVERED 2026-05-09

- [x] `FamilyCaps { max_shares_per_block, max_score_multiplier_bps, max_reward_credit_per_block: String }` (u128 as decimal string).
- [x] Optional `caps` + `signature` on `FamilyManifest` with `skip_serializing_if = "Option::is_none"` so `manifests/v1.json` continues to round-trip byte-equal.
- [x] `parse_family_manifest` validates caps ranges (mult ≤ 100_000 bps, reward parses as u128) and signature hex64 shape.
- [x] `verify_family_manifest_signature(pk_hex, &manifest)` reuses `signed_envelope::verify_signature` — clears `signature` field, recanonicalizes, and verifies.
- [x] 13 new tests in `tests/family_manifest_signature.rs` (caps OK / 4 bad-cap branches / signature OK+bad-len / serialization skip-when-unset / verify OK+KO+bad-pk+unsigned). Existing `manifest_fixtures.rs` regression stays green.
- [x] Acceptance: `cargo test -p boole-core` 91 PASS, `cargo clippy -p boole-core --all-targets -- -D warnings` clean. No node behavior changes.
- [ ] Deferred to S21+: fixture file `fixtures/protocol/family-manifests/v1.json` lands when the boot loader needs it.

### Slice S21 — Family-manifest boot loader + bounty side-pool wiring (Hard-Guard regression) ✅

- [x] D4 core/node runtime boundary closeout documented in `docs/core-node-runtime-boundary.md`; core keeps pure contracts/registries while node owns local file/directory IO loaders.
- [x] Node-owned family manifest directory loader with skip-and-warn policy; core `FamilyManifestRegistry` remains an in-memory registry only.
- [x] `BountySidePool` per-family share buffer keyed by `(family_id, proof_hash)` with dedup.
- [x] `LocalNodeConfig.family_manifests_dir` + `--family-manifests` CLI flag wired through `from_config`.
- [x] `bounty_proof_json` step 7b inserts `BountyShare` into side-pool on `accepted=true` only — base `SharePool` untouched.
- [x] `/status` exposes `familyManifestCount` + `bountySidePoolTotal`.
- [x] Hard-Guard regression test (`tests/hard_guard_regression.rs`): 1) boot loader registers manifest dir, 2) accept+reject+dedup proof traffic against `activation_height = u64::MAX` leaves `[height, c, sharePoolSize, replayMatchesRuntime]` byte-equal to baseline; only `bountySidePoolTotal` advances.
- [x] Acceptance: `cargo test -p boole-core -p boole-node --all-targets` PASS, `cargo clippy -p boole-core -p boole-node --all-targets -- -D warnings` clean.

### Slice S22 — Activation gating + `build_block_selection` extension ✅

- [x] S22a — `build_block_selection` accepts `promoted_bounty_shares: &[PromotedBountyShare]`; surfaces them on `BuiltBlockSelection.promoted_bounty_shares`. Base lane (`min_share_score` / `t_block` / `t_share` / `k_max` / proposer selection) byte-identical to pre-S22 with empty slice. 4 new tests in `boole-core/tests/block_builder_promoted.rs`.
- [x] S22b — `select_promoted_bounty_shares(side_pool, registry, runtime_height, operator_pks)` applies all four gates (activation_height, signature present, caps, signature verifies vs operator pk list) and respects `caps.max_shares_per_block`. Cheaper gates short-circuit before ed25519 verify. 10 new tests in `boole-core/tests/bounty_promotion.rs`.
- [x] S22c — `LocalNodeConfig.operator_signer_pks` + `--operator-signer-pks` / `OPERATOR_SIGNER_PKS`; `/status` exposes `promotedBountySharesCount`. New `promotion_active_does_not_alter_base_lane_status` regression in `tests/hard_guard_regression.rs` proves promoted slice surfaces accepted shares while `[height, c, sharePoolSize, replayMatchesRuntime]` stays byte-identical to baseline.
- [x] Acceptance: `cargo test -p boole-core -p boole-node --all-targets` PASS (single-threaded; default parallel run hits a known port-bind flake in `account_balance_route` unrelated to this slice), `cargo clippy -p boole-core -p boole-node --all-targets -- -D warnings` clean.

### Slice S23 — RewardLedger credit + replay-divergence sweep ✅

The acceptance gate is: a promoted+accepted bounty proof produces a credit line in `FileRewardLedger` that **byte-equally** reconstructs from a pure replay over the persisted block stream, AND a parallel walk over `FileBountyEventLedger`'s per-family credit totals agrees with that replay sum. Diverging any of the three sources fails boot.

Sub-slices:

- [x] **S23a — credit math + side-pool reward propagation**
  - `BountyShare.reward: u128` filled at side-pool insert time from `Bounty.reward`.
  - `select_promoted_bounty_selection` returns `PromotedBountySelection { shares, credits }`. `credit = min(share.reward, family_budget_left)`. Zero-credit rows dropped at persist; legacy `select_promoted_bounty_shares` kept as thin wrapper.
  - `PromotedBountyCredit { family_id, bounty_id, prover, amount: String }` (u128 as decimal).
  - `boole-core/tests/bounty_promotion.rs` covers back-compat, capped-by-share-reward, budget exhaustion, zero-cap, per-family disjoint budgets (15/15 pass).

- [x] **S23b — block + reward-ledger persistence**
  - `PersistedBlock.promoted_bounty_credits` with `#[serde(default, skip_serializing_if = "Vec::is_empty")]` for back-compat.
  - `BuiltBlockSelection.promoted_bounty_credits` extended; `build_block_selection` accepts and passes through.
  - At commit, `PersistedRewardEvent.credits` is the union of base-lane proposer credits + per-prover bounty credits. Pure base-lane blocks remain byte-identical to pre-S23 events.
  - `ReplayResult.bounty_credit_by_family: BTreeMap<String, u128>` populated by `replay_blocks` from each block's promoted credits.

- [x] **S23c — bounty event ledger credit events + commit hook wiring**
  - `validate_event` extended to dispatch on `kind="credit"` with separate field set (height, c, familyId, bountyId, prover, amount).
  - `runtime.commit_next_block_for_current_c_with_promoted` accepts shares + credits slices; merges credits into the same `PersistedRewardEvent` so the reward ledger is the single balance source of truth.
  - `local_node.rs` submit path appends one `"credit"` event per credit to `FileBountyEventLedger` post-commit.
  - Boot re-derive path folds `block.promoted_bounty_credits` so a re-derived ledger matches what live commit produces.

- [x] **S23d — divergence sweep + Hard-Guard regression extension**
  - `verify_ledger_matches_replay` extended to 4-arg signature: `(ledger, replay_balances, bounty_event_ledger_path, replay_bounty_credit_by_family)`. Walks `FileBountyEventLedger`, sums per-family credit from `kind=="credit"` events, bails on first divergence.
  - `runtime::boot_from_store_with_bounty_ledger` variant plumbs the bounty event ledger path through verification.
  - `boole-node/tests/reward_store_divergence.rs` — 9 tests pass (4 reward + 5 bounty-family scenarios).
  - `boole-node/tests/hard_guard_regression.rs::promoted_credit_lands_in_balance_and_preserves_hard_guard` — 2-boot diff: baseline vs. promoted submit yields byte-equal Hard-Guard view AND `/account/{prover_pk}/balance` returns the expected credit.

**Acceptance**: `cargo test -p boole-core -p boole-node --all-targets -- --test-threads=1` PASS, `cargo clippy -p boole-core -p boole-node --all-targets -- -D warnings` clean, N5b row in §1.4 flips ❌ → ✅ PASS.

## Sequencing

S20 → S21 → S22 → S23. Each slice ships behind green tests + clippy. Hard-Guard regression test lives in S21 and is re-run in S22+S23 to confirm no regression.

# Phase N5b-FOLLOW — Benchmark PoLW economic-signal capture (Slice S24)

**Why.** Current `scripts/boole-model-benchmark.py` measures verifier pass-rate only (binary `verifierAccepted` / `shareAccepted` / `blockProduced`). The PoLW value proposition — *capability difference → reward difference* — requires measuring the **economic distribution** itself: cumulative share reward, proposer bonus, bounty family credit. Without those signals the benchmark cannot distinguish a sound verifier from one that pays everyone equally despite differential pass rates. The acceptance shape changes from "spread in pass-rate" to "spread in earned reward."

**Target.** A single benchmark run records, per (model, attempt), the deltas in `/account/{prover_pk}/balance`, the per-block proposer assignment from `/block/{height}`, and `/bounties/{id}/proof` outcomes (when `--bounty-mode`). Aggregate output exposes economic spread alongside pass-rate spread.

### Slice S24 — Benchmark economic-signal capture

Sub-slices:

- **S24a — balance polling + cumulative share-reward** ✅ SHIPPED 2026-05-10
  - `--measure-reward` requires `--node-url` and `--prover-pk`; default off, all 57 pre-existing benchmark tests still pass.
  - `submit_candidate_to_verifier` polls `GET /account/{pk}/balance` immediately before and after `post_submission_to_node`. Row's `verifier` block now carries `accountBalanceBefore: str`, `accountBalanceAfter: str`, `attemptShareReward: str` (after-before, clamped to ≥0), plus the raw `balancePollBefore` / `balancePollAfter` HTTP envelopes for replay forensics.
  - `summarize` aggregates `rewardDistribution = { measuredRows, cumulativeShareReward, perTarget: {target: {cumulativeShareReward}} }` from the per-row decimal strings — u128-safe (Python int, not float).
  - Tests: `S24aBalancePollingTests::test_measure_reward_records_balance_delta_in_each_row_and_summary` (mock node returns balance=count*5, count++ on /submit; 2 attempts → cumulative=10) + `test_measure_reward_requires_node_url_and_prover_pk` (rejects without --node-url). Both green; 59/59 in `python3 -m unittest scripts.test_model_benchmark`.

- **S24b — proposer bonus tracking** ✅ SHIPPED 2026-05-10
  - `submit_candidate_to_verifier` now reads `effective.block.height` from the /submit response; when measure-reward + a block-producing attempt, it GETs `<node>/block/<height>` and records `blockHeight`, `proposerPk`, `selectedSharePks`, `wasProposer`, `proposerBonusEarned` ("1" if prover was proposer else "0", matching `compute_block_credits`'s deterministic +1 proposer credit). Raw block envelope preserved as `blockLookup` for forensics.
  - `summarize.rewardDistribution` aggregates `proposerBonusCumulative`, `proposerBlockCount`, and per-target rollups (each target now carries `cumulativeShareReward`, `proposerBonusCumulative`, `proposerBlockCount`).
  - Test: `test_block_producing_attempt_captures_proposer_pk_was_proposer_and_bonus` (mock node attributes block-1 to prover, block-2 to other; row1 `wasProposer=True`/`proposerBonusEarned="1"`, row2 `False`/"0", summary cumulative bonus = "1"). 60/60 unittest PASS.

- **S24c — bounty dimension** ✅ SHIPPED 2026-05-10
  - New CLI flags: `--bounty-id` (the bounty target) + optional `--bounty-envelope-json` (file path; envelope defaults to null when omitted). Both require `--node-url` and `--prover-pk`; main() bails with a typed message otherwise.
  - `derive_bounty_proof_hash(candidate, run_id, attempt_index)` mints a per-attempt SHA-256 — necessary because smoke-mode candidate text is identical across attempts and the registry dedups on proofHash. Tested explicitly (`test_bounty_mode_unique_proof_hash_per_attempt` runs 3 attempts and asserts 3 distinct hashes).
  - `submit_candidate_to_verifier` POSTs `/bounties/{id}/proof` after the share submission and stamps the row's verifier block with `bountyId`, `bountyProofHash`, `bountyAccepted`, `bountyFamilyId` (= bounty.domain), `bountyCreditEarned` (= bounty.reward when accepted, "0" otherwise), plus the raw `bountyHttp` envelope.
  - `summarize.rewardDistribution` now carries `bountyFamilyCreditsByFamily` (cross-target) and per-target `bountyAttempts`/`bountyAccepted`/`bountyFamilyCreditsByFamily`. Tests: `test_bounty_mode_posts_proof_and_records_per_family_credit` exercises 1 accept + 1 reject → row1 cred="50", row2 cred="0", summary cumulative under "test.alpha"="50". 62/62 unittest PASS.

- **S24d — aggregation + leaderboard** ✅ SHIPPED 2026-05-10
  - `summarize.rewardDistribution.economicSpread` computes cross-target share-reward dispersion: `{targetCount, minShareReward, maxShareReward, rangeShareReward, spreadPct, minTarget, maxTarget}`. Emitted only when ≥2 targets carry measured rewards (single-target runs surface `null`, not a misleading 0%).
  - `render_leaderboard` now prints `cumulativeShareReward`, `proposerBonusCumulative` (with proposed-block count), `bountyFamilyCreditsByFamily`, and a one-line `economicSpread: range=…, spreadPct=…%` summary at the top — tells you at a glance whether one model dominated the share stream.
  - Tests: `test_economic_spread_metrics_appear_in_summary_and_leaderboard` (2 targets: rewards 10/2 → range="8", spreadPct=80.00, max/minTarget assert) + `test_economic_spread_is_null_when_only_one_measured_target` (single-target ⇒ null). 64/64 unittest PASS.

- **S24e — preflight + smoke + acceptance** ✅ SHIPPED 2026-05-10
  - New `--preflight-node` flag on `boole-model-benchmark.py`: probes `/head` (always), `/account/<pk>/balance` (when `--prover-pk`), `/bounties/<id>` (when `--bounty-id`), prints `{ok, probes:[{route, method, status, ok, elapsedMs, error?}]}` JSON to stdout, exits 1 with the failing route on stderr otherwise. Backed by `preflight_node_economic_routes(...)` + `_probe_route(...)` helpers using the same `urllib` stack the rest of the script uses.
  - `scripts/preflight-model-benchmark.sh` accepts `--prover-pk` / `--bounty-id` (and `BOOLE_PROVER_PK` / `BOOLE_BOUNTY_ID` env). When `NODE_URL` is set, the wrapper invokes the new preflight before spec generation so a misconfigured node fails before a long benchmark run begins.
  - `scripts/local-node-smoke.sh` upgraded to assert `/account/<proposer_pk>/balance` end-to-end: it pulls `proposerPk` out of the `/submit` response, fetches the balance, and asserts `balance` is a non-empty u128 decimal string and `asOfHeight`/`asOfC` agree with `/block/latest`. `--max-requests` bumped 9→10. Reward-ledger path now passed explicitly via `--reward-store` so re-runs aren't poisoned by stale balances. Run output: `balance: pk=bbbb…, balance="2", asOfHeight=0` — confirms the proposer (+1) + share-owner (+1) deterministic credits land via the live HTTP route the benchmark queries.
  - Tests: `S24aBalancePollingTests::test_preflight_node_passes_when_all_routes_respond_200` (mock node 200s on all 3 routes ⇒ exit 0, every probe `ok=True status=200`) + `test_preflight_node_fails_when_account_route_missing` (mock 404s `/account` ⇒ non-zero exit, "/account/" surfaces in stderr). 66/66 `python3 -m unittest scripts.test_model_benchmark` PASS.
  - Parity-plan §1.4 dashboard now carries the row `Benchmark economic-signal capture (share-reward + proposer-bonus + bounty-credit + economic-spread) | ✅ PASS | Slice S24 (a–e) — Phase N5b-FOLLOW`.

**Sequencing.** S24a → S24b → S24c → S24d → S24e. Each ships behind green tests; live-mode bits stay non-CI but mock-node regression tests are CI-runnable.

---

## Slice A1–A6 — Universal-mining: Rust miner gains TLS + 4 LLM backends, then TS miner is retired

**Status (2026-05-10):** A1 ✅, A2 ✅, A3 ✅, A4 ✅, A5 ✅, A6 ✅, A6.1 ✅ (chain_head decimal multiplier parser fix). All 7 smoke scripts now invoke `cargo run -q -p boole-miner` directly; `MINER_ROOT` / `BOOLE_MINER_ROOT` / `npx tsx src/cli.ts` paths are removed from `scripts/`. `boole-miner-smoke.sh` and `boole-miner-agent-cli-smoke.sh` re-validated end-to-end (PASS).

### Vision (locked by user)
"로컬 및 프론티어 모델 제약 없이 다 마이닝 가능 — 능력에 따라 실패와는 별개로 다 마이닝 시도는 할 수 있어야돼." Translation: every model — local Ollama, vLLM, LM Studio, Anthropic API, OpenAI API, Gemini API, anything — must be able to *attempt* real mining (not just benchmark). Win/loss is determined by capability, but no model is locked out by tooling. This is a product invariant, not a nice-to-have.

### Scope decisions (locked)
1. **Rust miner gains 4 SDK backends**: `openai_compat`, `anthropic`, `openai`, `google`. After A2–A5 ship, no model class is locked out of real mining via the Rust miner.
2. **TLS via `reqwest::blocking` + `rustls-tls`**. Sync API matches the existing `ProverDriver::generate(&str)` signature — no async refactor. Pure-Rust TLS (no OpenSSL dependency).
3. **Inject HTTP via `HttpRunner` trait, mirroring existing `ProcessRunner` pattern.** Pure unit tests stay deterministic; live-API tests gate behind env flags (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OLLAMA_BASE_URL`, etc.).
4. **State schema unchanged.** `LlmConfig` already has `api_key`/`model`/`base_url` fields (state.rs:31–43, schema version 1). Adding new `LLMBackend::parse` cases is forward-compatible — backend is a plain String.
5. **Behavior parity with TS miner**: non-streaming, identical request bodies, identical default models. `openai_compat` ports `think: false` (TS llmDriver.ts:245) so Ollama reasoning models return non-empty content.
6. **Script migration is the *last* slice (A6).** While A1–A5 are in flight, the original M1–M4 cleanup is paused. TS miner stays available as fallback so no user is blocked.
7. **Adjacent quick win (optional): `/ticket` normalization patch on the node** so TS miner stays usable for openai_compat users *during* the A1–A5 gap. 1-line change to `local_node.rs:1352–1358` mirroring the existing `/submit` `normalize_pow_fields` call. Tracked as **A0 (optional, parallel)**.

### Affected files (A1–A6)
- `crates/boole-miner/Cargo.toml` — add `reqwest = { default-features = false, features = ["blocking", "json", "rustls-tls"] }` (workspace-local).
- `crates/boole-miner/src/http_client.rs` — keep plaintext client untouched; new TLS client added separately so existing tests stay valid.
- `crates/boole-miner/src/llm_driver.rs` — extend `LLMBackend` enum, add 4 driver structs + `HttpRunner` trait + `ReqwestHttpRunner` impl, extend `create_driver` factory.
- `crates/boole-miner/src/cli.rs` — extend `run_start` match (lines 465–487).
- `crates/boole-miner/tests/llm_driver.rs` — add `FakeHttpRunner` mirroring `FakeRunner` pattern; add unit tests per backend.
- `crates/boole-miner/tests/llm_driver_live.rs` (NEW) — env-gated live-API integration tests.
- 7 scripts + 4 wrappers (A6) — same set as the previous M1–M4 plan.

### Slice A0 — `/ticket` normalize_pow_fields parity (optional, 1-line node fix)
**Edit** `crates/boole-node/src/local_node.rs:1352–1358` to call `normalize_pow_fields(&mut ticket_body_owned)` before the `Hex32::from_hex` validations, mirroring the existing `/submit` call at line 1437. **Rationale:** /submit already auto-pads short hex (n/j/nonceS) to 64 chars; /ticket should be consistent. Side effect: TS miner's 8B nonce stops failing at /ticket, keeping all openai_compat users unblocked during A1–A5.
**Acceptance:** existing /ticket tests still pass; new test asserts a 16-char hex `n` zero-pads to 64 and accepts; `provider-model-smoke.sh` with TS miner + Ollama backend goes green locally.
**Decision needed:** ship A0 in parallel with A1, or skip and let TS-miner Ollama users be blocked until A2 ships?

### Slice A1 — TLS HTTP foundation + `HttpRunner` trait (foundational)
**Add** `reqwest` (blocking, rustls-tls) to `boole-miner/Cargo.toml`. **Define** `HttpRunner` trait in `llm_driver.rs` mirroring `ProcessRunner` — methods like `post_json(url, headers, body, timeout) -> Result<HttpResponse, HttpError>`. **Implement** `ReqwestHttpRunner` for production. **Add** `FakeHttpRunner` to `tests/llm_driver.rs` for deterministic unit tests.
**Acceptance:** `cargo test -p boole-miner` green; `cargo build -p boole-miner --release` no new warnings; binary still ≤ 5MB after `strip` (sanity check on TLS bloat).

### Slice A2 — `openai_compat` backend (Ollama / vLLM / LM Studio / DeepSeek / etc.)
**Add** `LLMBackend::OpenAiCompat` (parse case `"openai_compat"`). **Implement** `OpenAiCompatDriver` — POST to `{base_url}/v1/chat/completions` with body `{ model, messages: [{role: "user", content: prompt}], max_tokens: 8192, think: false }`, Bearer auth (defaults to `sk-no-key`), parse `choices[0].message.content`, tokens from `usage.completion_tokens`. **Wire** into `run_start` match. Default `max_tokens: 8192` per TS miner.
**Acceptance:** unit tests with `FakeHttpRunner` cover happy path + error responses + missing fields; env-gated live test against `OLLAMA_BASE_URL`; manual smoke `boole-miner init --llm-backend openai_compat --llm-base-url http://localhost:11434 --llm-model gemma4:26b && boole-miner start --max-shares 1 --max-cycles 1` produces a real proof attempt.

### Slice A3 — `anthropic` backend
**Add** `LLMBackend::Anthropic`. **Implement** `AnthropicDriver` — POST to `https://api.anthropic.com/v1/messages` with headers `{x-api-key, anthropic-version: "2023-06-01", content-type: application/json}`, body `{ model, max_tokens: 2048, messages: [{role: "user", content: prompt}] }`. Parse: concatenate all `content` blocks where `type=="text"`. Tokens from `usage.output_tokens`. Default model `claude-opus-4-7`.
**Acceptance:** unit tests + env-gated live test against `ANTHROPIC_API_KEY`.

### Slice A4 — `openai` backend
**Add** `LLMBackend::OpenAi`. **Implement** `OpenAiDriver` — POST to `https://api.openai.com/v1/chat/completions`, Bearer auth, body `{ model, max_tokens: 2048, messages: [{role: "user", content: prompt}] }`. Parse `choices[0].message.content`. Tokens from `usage.completion_tokens`. Default model `gpt-5`.
**Acceptance:** unit tests + env-gated live test against `OPENAI_API_KEY`.

### Slice A5 — `google` backend
**Add** `LLMBackend::Google`. **Implement** `GoogleDriver` — POST to `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}`, body `{ contents: [{parts: [{text: prompt}]}] }`. Parse `candidates[0].content.parts[0].text`. Tokens from `usageMetadata.candidatesTokenCount`. Default model `gemini-2.5-pro`.
**Acceptance:** unit tests + env-gated live test against `GOOGLE_API_KEY`.

### Slice A6 — Script migration + TS miner retirement (the original M1–M4, deferred to here)
After A2–A5 land, every backend has a Rust-native path. Now safely retire the TS miner: rewrite all 7 scripts (`boole-miner-smoke.sh`, `*-agent-cli-smoke.sh`, `*-hermes-cli-smoke.sh`, `*-hermes-real-verify-smoke.sh`, `*-opencode-cli-smoke.sh`, `*-ollama-gemma-smoke.sh`, `provider-model-smoke.sh`) to invoke `target/release/boole-miner` directly. Update 4 wrappers (`boole-agent-mine.sh`, `agent-runtime-benchmark.sh`, `provider-model-benchmark.sh`, `phase7-solo-preflight.sh`). Drop `BOOLE_MINER_ROOT` and all `../pof/boole-miner` references.
**Acceptance:** `grep -rn "pof/boole-miner\|BOOLE_MINER_ROOT" scripts/ docs/ .github/` returns zero hits + every original assertion still PASSes + `./scripts/self-test.sh` green.

**Sequencing.** A0 (optional, parallel) → A1 → A2 → A3 → A4 → A5 → A6. A2 unlocks Ollama (highest user value); A3–A5 unlock direct-API frontier mining. Each slice ships behind green tests + Telegram completion ping.
