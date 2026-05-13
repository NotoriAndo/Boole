# Boole — Node + CLI Parity Plan and Benchmark Fix Plan

**Status:** in progress. **Slices S1a + S1b + S2 + S3 + S4 + S5 + S6 + S7 + S8 + S8b + S9 + S10 + S11 + S12 + S13a + S13b + S14 + S15 + S16 + S17 + S18 + S19 SHIPPED (2026-05-09) → Gate P0 PASS, Gate P1 PASS, Gate B B3 + B4 + B5 + B7(a) + B1+B7(b) deterministic-CI PASS (5-way mixed family), Gate P5 PARTIAL PASS (cargo test green for `boole-miner`; live-model evidence remains a manual gate), Gate P6 C0+C1+C2+C3+C4 PASS (C4 via Slices S15-S19 — `boole mine init / address / config / bounty / start` end-to-end; full bounty CLI surface complete: list / get / submit / announce / status). S14 ships the `POST /bounties/:id/status` operator status-transition route (`boole.signed.v1` envelope wrapping a `boole.bounty.status.v1` payload, ed25519 verify, `BountyRegistry::update_status`, audit-ledger `kind:"status_change"` event with server-derived `prevStatus`), boot replay of status changes on top of the static catalog (illegal-transition overlap warned and skipped), and the `boole bounty status` CLI driver — closing Phase N5's operator status surface. S13b ships the `POST /bounties` announce route (`boole.signed.v1` envelope wrapping a `boole.bounty.announce.v1` payload, ed25519 verify, `BountyRegistry::create`, audit-ledger `kind:"create"` event), boot replay of dynamically-announced bounties on top of the static `bounties_path` catalog (static wins on overlap with stderr warning), and the `boole bounty announce` CLI driver. S13a ships ed25519 signing primitives (`boole-core::canonicalize` + `boole-core::SigningKeyV2` + `boole-core::verify_signature`), keys schema v2 (`{schema:"boole.keys.v2", id, pk, sk, createdAt}` envelopes carrying the ed25519 secret seed), and `boole keys sign / verify` CLI surface — the cryptographic toolkit S13b's `POST /bounties` announce flow consumes.** S1a froze `/ticket` payload contract, standardized the `{ok:false, reason:<kebab>, ...}` JSON error envelope via the new `boole-node::http_error` module, and co-fixed the benchmark `--use-node-ticket` request shape. S1b added `/health`, `/block/latest`, `/block/:height` with goldens (including the intentional empty-chain pof divergence) and an end-to-end `scripts/local-node-smoke.sh`. S2 stripped `<think>…</think>` blocks in `normalize_model_output`, lifted CoT preamble extraction (`last_proof_term_line`) to the primary path for all providers, moved the forbidden-token check to final-candidate-only, and renamed `ollama-final-line` → provider-agnostic `final-proof-line`. S3 added `boole node start` (binary fan-out via `BOOLE_NODE_BIN`), `boole block latest`, `boole block get --height N` driving the now-frozen routes; the boole-node `run-local` command grew env-var fan-out (`PORT`, `BLOCKSTORE_PATH`, `GENESIS_C`) plus `LocalNodeConfig.genesis_override` to mirror the pof booleCli wrapper, and `examples/boole-block-demo.sh` ships an end-to-end demo whose JSON output is suitable for the frontpage card. S4 added `replayInvoked: bool` to per-row score (and a `replayInvoked` count to `attemptHierarchy`), recomputed summary `replayPassed` so vacuous all-rejected runs report `null` instead of legacy `true`, and shipped `docs/benchmark-target-families.md` documenting `boole.calibration.pow.v1` + `boole.smoke.true.v1` with a CI lint that asserts every script literal has a section. S5 replaced the hardcoded `boole-model-benchmark-ollama-v0` literal with `fixtures/benchmarks/verifier-hashes.json` (`active` + `versions` map), added `load_verifier_hashes` / `resolve_verifier_hash` helpers in the benchmark driver, and wrote `verifierHashVersion` onto every row so historical replays pin to the recorded version regardless of future `active` bumps. S6 added the `boole keys new/list/show` CLI surface (C2): local key storage at `$BOOLE_KEYS_DIR` (or `$HOME/.boole/keys`) with mode-0600 atomic tmp+rename writes, the schema-versioned envelope `{id, pk, createdAt, schema:"boole.keys.v1"}`, `--dev` for deterministic-seeded keys (blake3 of the id), `--dry-run` for plumbing tests, and a typed-error helper that emits `{ok:false, reason:<kebab>, ...}` on stderr with pof-contract exit codes (2 = bad_request, 3 = key_already_exists / key_not_found). S7 swapped the local HTTP transport from hand-rolled raw TCP to `axum 0.8` on a multi-thread `tokio` runtime: every N0 route, JSON envelope, and HTTP status code stays byte-identical (the existing `tests/http_contract.rs`, `tests/local_node.rs`, `scripts/local-node-smoke.sh`, and `examples/boole-block-demo.sh` are the regression net and pass unchanged), the raw-TCP path (`read_http_request` / `handle_connection` / `write_json_response` / `write_error_response`) is fully deleted, a 16-concurrent `/submit` regression test (`tests/concurrency.rs`) proves admission still serializes under contention, and `--max-requests N` semantics are preserved via a per-connection `tower::Service` lifetime token (`Arc<ConnectionLifetime>`) whose `Drop` increments served exactly once when hyper finishes a connection — matching the raw-TCP "count an accepted-and-closed connection" model the readiness-probe pattern depends on. The full N0 contract surface is now frozen on `axum`, the demo-ready CLI slice is live, the benchmark cannot vacuously claim `replayPassed: true`, the verifier hash is version-keyed, and local key management is shipped; S8 ported pof's `v031-lp` length-preserving family (narrowed to N=1 lp-only) into Boole as `targetFamily: "boole.calibration.pow.v2"`: a new `Boole.Family.V0Helpers` Lean library exposes `mapAdd / mapMul / sortAsc` plus the three `length_*` lemmas; the benchmark driver gained a `mining-v2` mode whose per-attempt cursor (sha256-seeded) emits `∀ (xs : List Int), <chain xs>.length = xs.length` theorems closed via `Eq.trans`-composition of those lemmas — non-`rfl`-equivalent by construction; v1 stays byte-frozen and callable for back-compat. S8b extends the family to the full pof v031 5-way mixed generator as `targetFamily: "boole.calibration.pow.v3"` (mode `mining-v3`): `Boole.Family.V0Helpers` grows the four pof "truthy" witness lemmas (`all_filterByPred_self`, `nodup_dedup`, `pairwise_sortAsc`, `partition_eq_filter_filter`) plus the `filterByPred` and `dedup` op definitions, and the driver dispatches each per-attempt theorem into one of `{allSatisfy, sortedAsc, dedupFirst, partitionEq, lengthPreserved}` closed by the corresponding witness lemma (lengthPreserved retains the v2 `Eq.trans`-composition shape). v2 stays byte-frozen via a separate cursor-seed string (`|v031-mixed-cursor`) and a separate target family — the existing v2 golden fixture replays bit-for-bit. Gate B B1+B7(b) deterministic-CI now PASS via the original 9 `MiningV2FamilyTests` plus 10 new `MiningV3FamilyTests` (mode dispatch, attempt-context shape, per-invariant witness-lemma matching, 5-way coverage in 50 samples, non-rfl-equivalence, `.trans` for lengthPreserved long chains, wrapped Lean module shape with `invariantClass / chainLen / D` lottery-binding comments, prompt content covering all 5 branches and helper names, argparse choice, and a 10-instance golden-fixture replay regression at `fixtures/benchmarks/v031-mixed/golden-instances.json` covering all 5 invariant classes with at least one chainLen=1 and chainLen≥3 case). One wrapped candidate per invariant class (5 files) elaborates cleanly through `lake exec boole_check` (exit 0 each). Live evidence (50-attempt v3 run, blocksProduced + per-model spread) remains a manual / non-CI gate. See **§11. Slice ship log** for the per-slice change record.

Revised after operator review (2026-05-09): N0 phase added before transport rewrite; B-track priorities re-ordered; `uniqueShares=1` collapse marked resolved by proof-source binding (verified against `live-claude-{opus,sonnet}-50-node-ticket-postfix-20260509`).
**Scope:** bring `boole-node` and `boole-cli` to feature parity with the historical TypeScript pof reference (used as a porting source; the in-tree Rust miner has fully superseded the TS miner as of Slice A6, 2026-05-10), then exceed it where the Rust runtime allows; in parallel, fix the model benchmark harness.
**Companion:** see `docs/migration-status-and-next-steps.md` for the existing Phase A–G roadmap. This document refines and sequences the work specific to node/CLI surface and benchmark soundness.

**Sequencing principle (revised):** lock the JSON contract on the existing raw-TCP server first (Phase N0), then swap transport to `axum` (N1). This keeps each diff narrow and reviewable. CLI demo slice (C0+C1) lands on top of N0 for early product surface.

---

## 1. Parity matrix — current vs. target

### 1.1 HTTP/RPC surface

Reference: `projects/pof/dispatcher/src/httpServer.ts`. Current Rust: `crates/boole-node/src/local_node.rs:142–356`.

| Route | pof TS | boole-node Rust | Gap |
|---|---|---|---|
| `GET /health` | ✓ | ✓ | match — Slice S1b (PASS 2026-05-09) — pof shape `{status:"ok", sharePoolSize, provenance, ...}`; Boole-native `/status` retained alongside |
| `GET /head` | ✓ | ✓ | parity |
| `GET /config` | — | ✓ | Rust-only addition; keep |
| `GET /status` | — | ✓ | Rust-only; keep |
| `GET /block/latest` | ✓ | ✓ | match — Slice S1b (PASS 2026-05-09) — empty-chain returns `{ok:true, block:null, height:null, c:GENESIS_C}` |
| `GET /block/:height` | ✓ | ✓ | match — Slice S1b (PASS 2026-05-09) — typed `not_found` / `bad_request` envelopes for miss/bad-parse |
| `GET /account/:pk/balance` | ✓ | ✓ | match — Slice S9 (PASS 2026-05-09) — `{ok:true, pk, balance, asOfHeight, asOfC}`; unknown pk returns `balance:"0"` (not 404) |
| `GET /bounties` | ✓ | ✓ | match — Slice S11 (PASS 2026-05-09) |
| `GET /bounties/:id` | ✓ | ✓ | match — Slice S11 (PASS 2026-05-09) |
| `POST /bounties` (announce) | ✓ | ✓ | match — Slice S13b (PASS 2026-05-09) — `boole.signed.v1` envelope wrapping `boole.bounty.announce.v1` payload, ed25519 verify, `BountyRegistry::create`, audit-ledger `kind:"create"` event, restart-replay (static-wins on overlap) |
| `POST /bounties/:id/proof` | ✓ | ✓ | match — Slice S12 (PASS 2026-05-09) — kind-dispatched verifier registry, dedup peek, audit ledger append, restart-replay |
| `POST /bounties/:id/status` | ✓ | ✓ | match — Slice S14 (PASS 2026-05-09) — `boole.signed.v1` envelope wrapping `boole.bounty.status.v1` payload, ed25519 verify, `BountyRegistry::update_status`, audit-ledger `kind:"status_change"` event, restart-replay (audit deltas layered on static catalog) |
| `GET /work` | ✓ | ✓ | match — Slice S10 (PASS 2026-05-09) |
| `GET /work/:id` | ✓ | ✓ | match — Slice S10 (PASS 2026-05-09) |
| `POST /ticket` | ✓ | ✓ | match — Slice S1a (PASS 2026-05-09) — payload locked to pof `TicketBody = {c, pk, n}`; extra fields rejected with `unexpected_field` 400; observe-only |
| `POST /submit` | ✓ | ✓ | parity |

### 1.2 CLI surface

Reference: `projects/pof/dispatcher/src/booleCli.ts`. Current Rust: `crates/boole-cli/src/main.rs`.

| Command | pof TS | boole-cli Rust | Gap |
|---|---|---|---|
| `boole version` | — | ✓ | Rust-only; keep |
| `boole node start` | ✓ | ✓ | parity (Slice S3) |
| `boole block latest` | ✓ | ✓ | parity (Slice S3) |
| `boole block get --height` | (via `/block/:height`) | ✓ | parity (Slice S3) |
| `boole account balance --pk` | ✓ | ✓ | match — Slice S9 |
| `boole chain replay` | ✓ | ✓ | parity |
| `boole work list` | ✓ | ✓ | match — Slice S10 (PASS 2026-05-09) |
| `boole work get --id` | ✓ | ✓ | match — Slice S10 (PASS 2026-05-09) |
| `boole mine start` | ✓ | ✓ | match — Slice S19 (PASS 2026-05-09) — full pipeline (head fetch → ticket grind → announce → per-j target/LLM/verify/canonicalize/share grind/submission PoW → submit), pluggable trait collaborators, `--max-cycles` / `--max-shares` stop conditions, `--mock-llm-response` / `--mock-verify-accept` smoke knobs. Falls back to `AcceptingVerifier` without the `lake-verify` feature. |
| `boole mine bounty` | ✓ | ✓ | match — Slice S19 (PASS 2026-05-09) — reads envelope file, computes `proofHash = sha256(envelope_bytes)`, POSTs `boole-miner::BountyClient::submit_proof` to `/bounties/:id/proof`, prints typed `BountyProofResult` JSON. |
| `boole keys new \| list \| show` | ✓ | ✓ | parity (Slice S6); `keys new` issues `boole.keys.v2` envelopes (with `sk`) by default since Slice S13a (PASS 2026-05-09); v1 envelopes remain readable |
| `boole keys sign` | ✓ | ✓ | match — Slice S13a (PASS 2026-05-09) — refuses v1 keys with `legacy_v1_key` exit 3 |
| `boole keys verify` | ✓ | ✓ | match — Slice S13a (PASS 2026-05-09) — stateless; `valid`/`invalid` both exit 0 |
| `boole bounty list` | (via REST) | ✓ | match — Slice S11 (PASS 2026-05-09) |
| `boole bounty get --id` | (via REST) | ✓ | match — Slice S11 (PASS 2026-05-09) |
| `boole bounty submit` | (via REST) | ✓ | match — Slice S12 (PASS 2026-05-09) — bare `<status>` word default, `--json` envelope, `--envelope` accepts file path or inline JSON |
| `boole bounty announce` | (via REST) | ✓ | match — Slice S13b (PASS 2026-05-09) — bare bounty id default, `--json` returns full server envelope, refuses v1 keys with `legacy_v1_key` exit 3, `--verifier-metadata` accepts inline JSON or file path |
| `boole bounty status` | (via REST) | ✓ | match — Slice S14 (PASS 2026-05-09) — bare `<newStatus>` default, `--json` returns full server envelope, refuses v1 keys with `legacy_v1_key` exit 3, clap-level enum validation on `--new-status` |

### 1.3 Adjacent components missing in Rust

- **Miner loop crate** — DELIVERED 2026-05-09 (Slices S15-S19). `crates/boole-miner` ports pof's TypeScript `boole-miner/` to a Rust library + thin binary, consumed by `boole-cli mine ...`. Module map: S15 → `state` (init + atomic round-trip) + `cli` (init/address/config) + types (`MinerConfig`, `MiningSummary`, `MiningEvent`); S16 → `grinder/{ticket, share, submit_pow}` PoW grinders + `submit_client::SubmitClient` + `bounty_client::BountyClient` + `cli::bounty`; S17 → `chain_head` (`ChainHeadFetcher` + `HttpHead`) + `http_client` shared blocking transport + `proof_package` envelope assembly; S18 → `target_emitter` (`TargetEmitter` + `boole.calibration.pow.v3` deterministic emitter behind `lake-target`) + `local_verify` (`Verifier` trait + `AcceptingVerifier` + lake-backed `LakeVerifier` behind `lake-verify`) + `llm_driver` (`ProverDriver` impls + `MockProverDriver`) + `canonicalizer/{structural}` behind `lake-canon`; S19 → `mining_loop::run_mining_loop` + `cli::start` glue + boole-cli `Mine { Start, Bounty }` subtree. Lake-toolchain integration (`lake-canon`, `lake-target`, `lake-verify`) is feature-gated and off by default — without features, the loop falls back to `AcceptingVerifier` and the `boole.calibration.pow.v3` fixture-only emitter. Tracked under Phase C4 / Gate P5 (PARTIAL PASS — cargo test green, live-model evidence remains a manual gate).
- **MCP daemon equivalent** — pof has `boole-mcp/`. Out of scope for parity v1; track separately.
- **Bounty side-pool admission + `FamilyManifest` activation gating** — `BountyRegistry` is wired into `LocalNodeState` and the four bounty routes (read / proof / announce / status) are end-to-end live with audit-log durability since Slice S14, BUT bounty submissions still do not enter `build_block_selection` (no side-pool wiring on `RuntimeAdmissionState`) and there is no `FamilyManifest`-driven promotion path. Phase N5 long tail; Gate P4 last bullet.
- **Bounty → reward ledger credit** — `FileRewardLedger` is wired into `RuntimeAdmissionState` (per-share + per-block-proposer credit), but `accepted:true` bounty proofs do NOT call `RewardLedger::credit` for the bounty reward; the audit ledger records `credit:reward` on the event but the runtime ledger is not updated. Pending alongside side-pool admission.
- **Reward-ledger replay divergence check on bounty events** — `verify_ledger_matches_replay` runs on the base-chain replay; once bounty credit lands, the divergence check needs to extend across bounty-event totals.

### 1.4 Remaining work dashboard

Snapshot of what's left versus what's shipped, organized by surface. The acceptance gates in §7 reference the same items.

| Surface | Status | Tracking |
|---|---|---|
| HTTP/RPC contract surface (N0–N4 + bounty routes) | ✅ PASS | §1.1, Slices S1a/S1b/S9/S10/S11/S12/S13b/S14 |
| Bounty registry persistence (announce / proof / status / audit ledger / boot replay) | ✅ PASS | Slices S11/S12/S13b/S14 |
| Bounty consensus integration (side-pool, `FamilyManifest`, activation height, base-chain difficulty isolation) | ✅ PASS | Slices S20/S21/S22/S23 — Phase N5b |
| Bounty → `RewardLedger` credit on accepted proofs | ✅ PASS | Slice S23 (credit math + replay-divergence sweep) |
| `boole keys` (`new`/`list`/`show`/`sign`/`verify`) | ✅ PASS | Slices S6 + S13a |
| `boole bounty` (`list`/`get`/`submit`/`announce`/`status`) | ✅ PASS — full surface complete | Slices S11/S12/S13b/S14 |
| `boole work` / `boole account` / `boole block` / `boole chain` / `boole node` | ✅ PASS | Slices S3/S9/S10 |
| Miner loop crate (`boole-miner`) + `boole mine start` / `mine bounty` | ✅ PASS (cargo) / ⏸ PARTIAL (live evidence) | Slices S15-S19 — Phase C4 / Gate P5 |
| MCP daemon equivalent | 🚫 OUT OF SCOPE | tracked separately |
| Operator key rotation (`keys rotate / upgrade`) + schema v3 sealed-box `sk` | ⏸ DEFERRED | S13a follow-up |
| `--allowlist <path>` for announce / status-change pks | ⏸ DEFERRED | S13b/S14 follow-up |
| RFC 8785 full canonicalization parity (current: 8785-lite) | ⏸ DEFERRED | S13a follow-up |
| Benchmark soundness Gate B-CI (deterministic) | ✅ PASS | Slices S2/S4/S5/S6/S(B6)/S8/S8b |
| Benchmark soundness Gate B-Live (live evidence) | ⏸ MANUAL ONGOING | not a CI gate |
| Benchmark `--admission-nonce` offline diversification | ✅ PASS | Slice S(B2) |
| Benchmark economic-signal capture (share-reward + proposer-bonus + bounty-credit + economic-spread) | ✅ PASS | Slice S24 (a–e) — Phase N5b-FOLLOW |

**Reading the dashboard:**
- ✅ PASS = end-to-end live, tests green, doc + ship-log entry filed.
- ⏸ PARTIAL = some path live, named gap remaining, blocking item identified.
- ⏸ DEFERRED = design known, no consumer asking yet, not blocking parity.
- ❌ MISSING = required for parity but no Rust implementation yet.
- 🚫 OUT OF SCOPE = explicitly tracked outside this doc.

As of Slice S23, the bounty consensus integration (N5b) is shipped end-to-end: side-pool, signed `FamilyManifest` activation gating, per-family caps + budget-bounded credit propagation, `RewardLedger` credit folding, and a 4-arg replay-divergence sweep that cross-checks reward + bounty event ledgers against pure replay. The miner loop crate is shipped (S15-S19); live-model evidence for `mine start` remains a manual / non-CI gate. Everything else is either shipped, deferred-by-design, or live-evidence-gated outside CI.

---

## 2. Node parity plan (`boole-node`)

Sequenced phases. Each phase ships behind a green `cargo test --workspace` and a smoke command. All work assumes the existing pof TS implementation as the spec — when in doubt, port behavior, not interpretation.

**Sequencing principle:** lock the contract on the existing raw-TCP server first (N0), then swap transport (N1). Combining "feature add" with "transport rewrite" inflates regression cost; separating them keeps each diff small and reviewable.

### Phase N0 — Contract fixes before transport rewrite

**Why first:** before swapping HTTP frameworks, the JSON contract surface must be stable. Doing this on raw TCP is cheap (a few hundred LOC) and prevents axum migration from re-litigating contract decisions.

Tasks:
1. **`POST /ticket` payload compatibility** — collapse Boole's accepted body to pof's `TicketBody = {c, pk, n}`. Reject extra fields with `{ok:false, reason:"unexpected_field", field:...}` (HTTP 400). Move `normalize_pow_fields` (`local_node.rs:344`) downstream into `/submit`-only.
2. **`GET /health`** — pof shape: `{status:"ok", sharePoolSize, provenance, ...}`. Distinct from existing `/status` (which can stay as a Boole-native dev endpoint).
3. **`GET /block/latest`** — read from `FileBlockStore::latest()` (add the method if absent).
   - **Non-empty chain**: HTTP 200 with `{ok:true, block:<PersistedBlock>, height:<u64>, c:<hex32>}`.
   - **Empty chain (no blocks committed yet)**: HTTP 200 with `{ok:true, block:null, height:null, c:"<GENESIS_C>"}`. Empty chain is a valid state, not an error — clients always receive a chain-head reference (`GENESIS_C`) so they can post `/ticket` and `/submit` against the genesis head without a separate bootstrap call. Mirrors pof's `chainReplay` returning genesis for an empty chain.
   - Add a contract test row covering the empty-chain case (boot a runtime against an empty NDJSON, hit `/block/latest`, assert the JSON shape).
4. **`GET /block/:height`** — read by height; numeric validation, bounded.
   - Valid height with a committed block: HTTP 200 with `{ok:true, block:<PersistedBlock>, height, c}`.
   - Out-of-range or unparseable height: HTTP 400 `{ok:false, reason:"bad_request", detail:"height must be a non-negative integer"}` or 404 `{ok:false, reason:"not_found", detail:"no block at height N"}` depending on parse vs lookup.
5. **Standardize JSON error envelope** — all 4xx/5xx responses use `{ok:false, reason:<kebab>, detail?, ...context}`. Add `not_found`, `bad_request`, `bad_hex`, `unexpected_field` reasons. Codify in a new `crates/boole-node/src/http_error.rs` module.
6. **Smoke script** — `scripts/local-node-smoke.sh`: boots node, hits all routes (existing + new), validates JSON shapes against fixture goldens at `fixtures/protocol/http-contract/v1/`.
7. **Contract tests** — add `crates/boole-node/tests/http_contract.rs` that asserts the response schema for each route on a temp-port test server.

Acceptance: `scripts/local-node-smoke.sh` PASS, `cargo test --workspace` PASS, fixture goldens regenerate cleanly. No behavior change for existing routes outside the listed gaps.

### Phase N1 — HTTP framework swap (transport-only)

**Re-scoped:** pure transport migration. No new routes, no contract changes. With N0 contract frozen, this becomes "same contract, async server" — a reviewable single-purpose diff.

Tasks:
1. Adopt `axum` on `tokio` runtime.
2. Replace `serve_local_node` (`local_node.rs:50`) with an async server.
3. Lock strategy on `RuntimeAdmissionState` via `Arc<RwLock<...>>`:
   - **Write lock**: `/submit` and block-commit only — these mutate consensus state (share pool, candidate tracking, block store, runtime head).
   - **Read lock + interior mutability**: `/ticket` is observe-only at the consensus boundary (no admission state mutation). Its rate-limiter side effects must use interior mutability (e.g. `DashMap`, atomic counters, or a `Mutex`-per-pk slot inside the rate-limiter struct) so concurrent `/ticket` calls do not contend on the outer write lock. `/head`, `/config`, `/status`, `/health`, `/block/latest`, `/block/:height` are read-only.
4. Port all N0-frozen routes (`/health /status /head /config /ticket /submit /block/latest /block/:height`) with no behavior change. Add typed handlers + serde deserialization (no manual line-by-line HTTP parsing).
5. Re-run the N0 smoke + contract tests against the new server — they must pass without modification.
6. Delete raw-TCP path.

Acceptance: existing `crates/boole-node/tests/runtime_smoke_cli.rs` passes unchanged; existing `tests/http_contract.rs` from N0 passes unchanged; new concurrency test sends 16 concurrent `/submit` calls and observes serialized admission with no race in `share_pool`.

### Phase N2 — (folded into N0)

Block read APIs (`/block/latest`, `/block/:height`) ship as part of N0. Phase number reserved to keep downstream phase numbering stable.

### Phase N3 — Account/balance + reward ledger runtime integration — **PASS 2026-05-09 (Slice S9)**

Two-step: wire ledger first, then expose endpoint.

Tasks:
1. Add `RewardLedger` to `RuntimeAdmissionState`. Mirror pof TS shape: per-block `compute_block_credits(proposer_pk, share_owner_pks) → Map<pk, u64>`, append-on-commit. **DONE** — `FileRewardLedger` (path-bound NDJSON) and `compute_block_credits` were pre-built in earlier slices; S9 wires them into `RuntimeAdmissionState` as an optional handle (`reward_ledger: Option<FileRewardLedger>`, `reward_ledger_path: Option<PathBuf>`). `commit_using_cache` appends `PersistedRewardEvent { height, c, credits }` between block append and `apply_block_unchecked` so a crash between the two is recoverable from the block log alone.
2. Persist as NDJSON next to block store; recover on boot, re-derive from existing blocks if absent. **DONE** — `boot_from_store(config, block_path, reward_ledger_path)` recovers the ledger from `reward_ledger_path` when present, re-derives by re-issuing `compute_block_credits` over each replayed block when absent, and bails with `reward ledger divergence at pk … ledger=… replay=…` when the on-disk ledger does not match the replay-balance map (`verify_ledger_matches_replay`).
3. Add `RuntimeAdmissionState::balance_for(pk)` and `ledger_head() -> Option<(u64, String)>`. **DONE.**
4. `GET /account/{pk}/balance` returns `{ok:true, pk, balance:"<u128>", asOfHeight, asOfC}`. **DONE** — axum route on the local node; well-formed-hex32 check rejects non-`[0-9a-f]{64}` inputs with HTTP 400 `{ok:false, reason:"malformed_pk"}`; unknown but well-formed pks return HTTP 200 with `balance:"0"` (parity with pof contract); `asOfHeight` / `asOfC` come from the ledger head, falling back to the configured `genesis_c` for the empty chain.
5. Replay-vs-ledger divergence verification. **DONE via `verify_ledger_matches_replay`.** Note that `replay_blocks` itself was already producing `balances: BTreeMap<String, u128>`; S9 just wires that map into the divergence check at boot. If/when a snapshot-style endpoint lands, the same helper is the natural plug-in.

Boot scenarios covered (driven from `fixtures/protocol/replay/v1.json`):
- file present + matches replay → boot succeeds, ledger recovered, balances served from disk.
- file absent → re-derive from `replay_blocks(blocks).balances` and write the ledger so subsequent boots are file-recovery boots; `recovered.size() == fix.blocks.len()` after re-derive.
- file present + tampered (e.g. one `credits.amount` bumped from `"1"` to `"99"`) → boot bails with `reward ledger divergence at pk …`.
- `reward_ledger_path = None` → legacy boot, `balance_for(_) == 0`, `ledger_head() == None`. Useful for `submit-lean` and other one-shot CLI surfaces that don't need persistent rewards.

Acceptance: 4 ledger-divergence tests + 4 runtime boot scenarios + 4 axum route tests + 4 CLI surface tests, plus a manual smoke against the seeded `replay/v1.json` ledger (`PK_2 → balance 3`, unknown pk `→ "0"`, malformed pk → exit 2 with typed envelope) — see `crates/boole-node/tests/{reward_store_divergence,runtime_reward_ledger,account_balance_route}.rs`, `crates/boole-cli/tests/account_balance_cli.rs`, and §11 ship log Slice S9.

### Phase N4 — Work API (read-only) — **PASS 2026-05-09 (Slice S10)**

Tasks:
1. Define `WorkManifest` type in `boole-core` (port `boole-mcp` work manifest spec from pof `local-docs/work-manifest.md` if present, otherwise `pof/dispatcher/src/work*.ts`). ✅ pre-existed at `crates/boole-core/src/work_manifest.rs`.
2. `GET /work` returns the static manifest list; `GET /work/:id` returns one. ✅ Slice S10.
3. CLI: `boole work list / get` (Phase C below). ✅ Slice S10 — bundled with the node-side work for end-to-end coverage, mirroring the N3+C3-account bundling of S9.

Acceptance: unit test loads a fixture manifest and a `cargo test` integration test fetches both endpoints. ✅ 2 (boole-core loader) + 4 (boole-node routes) + 5 (boole-cli) = 11 new tests in Slice S10.

### Phase N5 — Bounty API + bounty-aware admission

Phase N5 splits cleanly into two halves with different ship states:

#### N5a — Bounty API + CLI surface — **PASS 2026-05-09 (Slices S11 + S12 + S13b + S14)**

Read surface (`GET /bounties`, `GET /bounties/:id`) shipped in Slice S11. Proof-write surface (`POST /bounties/:id/proof`) + Lean verifier wiring + audit ledger + `boole bounty submit` shipped in Slice S12. Announce-write surface (`POST /bounties`) shipped in Slice S13b: validates a `boole.signed.v1` outer envelope wrapping a `boole.bounty.announce.v1` payload, calls `boole_core::verify_signature` against the announcer's `pk`, calls `BountyRegistry::create`, and appends a `kind:"create"` event to the same audit ledger as proofs. Status-transition surface (`POST /bounties/:id/status`) shipped in Slice S14: same `boole.signed.v1` envelope shape wrapping a `boole.bounty.status.v1` payload (`{schema, id, newStatus, reason?, ts}`), the announcer's `pk` is recorded on a `kind:"status_change"` audit event with server-derived `prevStatus`, and `BountyRegistry::update_status` enforces the existing terminal-state guards (mapped to 409 `bounty_terminal`) plus URL/payload id cross-check (400 `bounty_id_mismatch`) and enum validation (400 `bad_status_value`). Boot replays the audit ledger over a `BountyRegistry`-backed `LocalNodeState` so `solved` (proof) status, dynamically-announced bounties, and operator status changes (e.g. `withdrawn`) all survive restart; static `bounties_path` catalog seeds first, audit-log creates and status changes layer on top, conflicts (duplicate id on create, illegal transition on status) fall to the static version with a single-line stderr warning. Verifier dispatch uses a `HashMap<String, Arc<dyn BountyProofVerifier>>` registry; production binary registers `lean → LeanBountyVerifier` when `--lean-checker-dir` (or `LEAN_CHECKER_DIR`) is set; tests use the `LocalNodeConfig.bounty_verifiers` injection seam to stub mock kinds. Full bounty CLI surface (`list` / `get` / `submit` / `announce` / `status`) matches pof. **Acceptance:** all four routes plus all five subcommands have integration tests + manual smoke; audit ledger durability + boot replay verified. **N5a status: PASS.**

#### N5b — Bounty consensus / economics integration — **PARTIAL (design-locked, not wired)**

The consensus + economics half of Phase N5 remains deferred to a later slice. **N5b status: PARTIAL.** What's still missing:

- **Side-pool admission on `RuntimeAdmissionState`.** Bounty submissions live in `LocalNodeState.bounty_registry` (Arc<RwLock<BountyRegistry>>) but do not enter `build_block_selection`. Base-chain replay is therefore unaffected by bounty traffic — by design, until promotion lands.
- **`FamilyManifest` activation gating.** `boole-core::FamilyManifest` parses + validates manifests, but there is no `RuntimeAdmissionState` consumer that checks `activation_height` against runtime height before letting a promoted family enter the share pool.
- **Bounty → `RewardLedger::credit` link on accepted proofs.** `FileRewardLedger` is wired into `RuntimeAdmissionState` for base-chain credit (per-share + per-block-proposer); `POST /bounties/:id/proof` records `credit:reward` on the audit event but does NOT update the runtime reward ledger. The bounty audit log is the source of truth for bounty payouts today; cross-ledger reconciliation is pending.
- **Per-family caps + replay-divergence check on bounty events.** Per-family caps on share count, score multiplier, and reward credit are spec'd in the hard-guard section below but not enforced; `verify_ledger_matches_replay` does not yet sweep bounty-event totals.

**Hard guard (consensus invariant):** Bounty lane must NOT alter base mining/block difficulty (`T_share`, `T_block`, `MinShareScore`, `K_max`) until promoted to an official family via:
- a signed family manifest committed to the chain or to a known-hash file fixture,
- an explicit `activation_height` field consumed by `RuntimeAdmissionState` boot,
- per-family caps on share count, score multiplier, and reward credit so a bounty cannot exceed the base lane's economic envelope.

Until promotion, bounty submissions are tracked in a side-pool with their own metrics but do not enter `build_block_selection`. This keeps base-chain replay deterministic against pre-bounty fixtures.

Tasks:
1. Wire `bounty_registry.rs` into `RuntimeAdmissionState`. Boot loads bounties from a configured path; bounty submissions enter a side-pool, NOT the main share pool. (S11 read API was `Vec<Bounty>`-backed; S12 swapped `LocalNodeState.bounties` for `bounty_registry: Arc<RwLock<BountyRegistry>>` so proof submission can flip status. Side-pool admission for promoted bounties is still pending.)
2. `GET /bounties`, `GET /bounties/:id` ✅ Slice S11. `POST /bounties/:id/proof` ✅ Slice S12 (kind-dispatched verifier registry, dedup peek before verifier call, audit ledger append, restart-replay). `POST /bounties` (announce, with operator key signing) ✅ Slice S13b (`boole.signed.v1` envelope, `boole.bounty.announce.v1` payload, ed25519 verify, `kind:"create"` audit event, boot replay with static-wins overlap policy). `POST /bounties/:id/status` (operator status transition) ✅ Slice S14 (`boole.bounty.status.v1` payload, ed25519 verify, `BountyRegistry::update_status`, `kind:"status_change"` audit event with server-derived `prevStatus`, boot replay layered on the static catalog).
3. Define `FamilyManifest { id, version, activation_height, caps:{max_shares, score_multiplier, max_reward_per_block} }` in `boole-core`. Refuse to promote a bounty to base lane without a valid manifest and a runtime height ≥ `activation_height`.
4. Extend `replay_blocks` to verify (a) base-chain replay is unaffected by side-pool bounty traffic, and (b) once promoted, bounty-credit caps are enforced per block.
5. Add a regression test: a bounty with `activation_height = u64::MAX` submitted en masse must not alter base block production for any height ≤ that bound.

Acceptance: full pof `bounty_registry` parity tests adapted into `crates/boole-node/tests/`. `POST /bounties` end-to-end with a key from `boole-cli keys`. Base-chain difficulty/replay invariant test PASS.

### Phase N6 — Beyond-pof improvements (optional, but cheap given Rust)

Pick what fits the current goal. None are blockers for parity.

- **Streaming `/blocks?from=X` SSE** for replay observers. pof doesn't have this.
- **Prometheus `/metrics`** — counters for `submit_accepted`, `submit_rejected{reason}`, `block_built`, `kernel_check_ms_p99`.
- **Backpressure on `/submit`** — bounded mpsc channel into the admission task.
- **Snapshot endpoint** — `GET /snapshot` returns a tarball of block store + reward ledger for new node bootstrap. pof has none.
- **Async kernel check pool** — a worker pool of `boole-lean-runner` processes with a small in-memory `(canon_hash → accepted)` cache. Today every `/submit` re-spawns Lean.

---

## 3. CLI parity plan (`boole-cli`)

Build on top of the node phases. CLI surfaces consume what the node exposes. **C0/C1 lands on top of N0, before axum** — this gives a frontpage/demo-ready surface (`boole node start` + `boole block latest`) without waiting on the framework swap.

### Phase C0 — Bootstrap (no new node features required)

- `boole node start [--port N] [--data-dir DIR] [--genesis HEX32]` — spawn the daemon (raw-TCP server in N0 era; transparently reused after N1 swap). Mirror pof `booleCli.ts:116–134` env var fan-out (`PORT`, `BLOCKSTORE_PATH`, `REWARDLEDGER_PATH`, `GENESIS_C`).
- `boole chain replay --blocks <path>` — already exists; verify it still works against N0-era output.

### Phase C1 — Block queries (depends on N0 routes)

- `boole block latest [--node URL] [--json]`
- `boole block get --height N [--node URL]`

Together with C0, this is the **demo-ready slice**: `boole node start` then `boole block latest` produces a JSON snapshot suitable for a frontpage card or VC demo. Reward/account fields are stubbed until N3 + C3.

### Phase C2 — Keys

Local key management. Targets pof `booleCli.ts:268–331` shape:
- `boole keys new --id <name> [--dev] [--dry-run]`
- `boole keys list`
- `boole keys show --id <name>`

Storage: `~/.boole/keys/<id>.json`, mode 0600, atomic write tmp+rename. Honor user CLAUDE.md "no Co-Authored-By trailers" rule; no telemetry.

Reuse pof JSON contract: success → JSON object on stdout; errors → `{ok:false, reason:<kebab>, ...context}` on stderr. Exit codes 0/2/3/64 per pof `docs/cli-json-contract.md`.

### Phase C3 — Account, work, bounty queries

After Phases N3, N4, N5:
- `boole account balance --pk <hex32>`
- `boole work list / get --id <id>`
- `boole bounty list / get --id`
- `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32> --envelope <path|inline> [--node URL] [--json]` — kind-dispatched verifier (lean wired in S12), bare `<status>` word default, `--json` envelope. ✅ Slice S12.
- `boole bounty announce --id <id> --domain <domain> --problem-hash <hex32> --verifier-kind <kind> --verifier-metadata <path|inline> --reward <u128> --deadline <u64-ms> --signing-key <id> [--ts <u64-ms>] [--node URL] [--json]` — local hex32 lint, key resolved from C2 (v1 refused with `legacy_v1_key` exit 3), payload signed via `SigningKeyV2`, POSTed as a `boole.signed.v1` envelope. Bare bounty id default, `--json` returns the full server envelope. ✅ Slice S13b.
- `boole bounty status --id <id> --new-status <open|solved|expired|withdrawn> [--reason <text>] --signing-key <id> [--ts <u64-ms>] [--node URL] [--json]` — clap-level enum validation on `--new-status`, key resolved from C2 (v1 refused with `legacy_v1_key` exit 3), `boole.bounty.status.v1` payload signed via `SigningKeyV2` and POSTed as a `boole.signed.v1` envelope to `/bounties/:id/status`. Bare `<newStatus>` word default, `--json` returns the full server envelope. ✅ Slice S14.

### Phase C4 — Mining — **PASS 2026-05-09 (Slices S15-S19)**

The Rust miner crate (`crates/boole-miner`, see §4) is shipped. `boole-cli` exposes the `Mine { Start, Bounty }` subtree by re-using `boole_miner::cli::{MineCommand, run_mine}` so the standalone `boole-miner` binary and `boole mine ...` share the same code paths (no subprocess fan-out — `--dry-run` short-circuits cleanly):

- `boole mine init / address / config get / config set` — Slice S15. State init at `$BOOLE_MINER_STATE` (or `~/.boole/miner-state.json`) with mode-0600 atomic tmp+rename writes; `boole.miner.state.v1` envelope; secret redaction by default on `config get` with `--reveal` flag.
- `boole mine bounty --id <id> --node URL --prover <key> --envelope-path <file>` — Slice S16. Reads envelope file, computes `proofHash = sha256(envelope_bytes)`, POSTs to `/bounties/:id/proof` via `boole-miner::BountyClient`, prints typed `BountyProofResult` JSON.
- `boole mine start --max-cycles N --max-shares N [--head-timeout-ms ms] [--mock-llm-response <text>] [--mock-verify-accept] [--fixed-target-seed-hex <hex> --fixed-target-render <text>]` — Slice S19. Drives `mining_loop::run_mining_loop` with pluggable trait collaborators (`ChainHeadFetcher`, `TargetEmitter`, `ProverDriver`, `Verifier`, `Canonicalizer`, `Submitter`, `PromptBuilder`, `Sleeper`, `LogSink`); falls back to `AcceptingVerifier` without the `lake-verify` feature; emits a final `summary:` envelope (`{cyclesRun, sharesAccepted, ticketsFound, networkErrors, ...}`).

Live-model evidence (claude_cli driver, ollama_local driver, anthropic_api driver) remains a manual / non-CI gate — Gate P5 PARTIAL.

### Phase C5 — JSON contract + agent surface

Port the contract spec wholesale: `docs/cli-json-contract.md` (NEW, copy-and-adapt from `pof/docs/cli-json-contract.md`). All commands emit:
- success → `{ok:true, ...}` on stdout
- error → `{ok:false, reason:<kebab>, ...context}` on stderr
- `--json` accepted as no-op (already implied)
- `--dry-run` for state-changing commands

Add `boole --help` block describing the agent integration pattern (so AFK agents can shell into it).

---

## 4. Rust miner crate (new) — `crates/boole-miner` — **SHIPPED 2026-05-09 (Slices S15-S19)**

A new workspace member. Originally ported from the historical pof TS miner reference; as of Slice A6 (2026-05-10) it is the only first-class miner — the TS implementation is retired. Synchronous Rust library + thin binary; consumed by `boole-cli` via the re-exported `boole_miner::cli::{MineCommand, run_mine}` entry point. No `tokio` dependency — matches the rest of the workspace's sync HTTP idiom (`reqwest::blocking`).

**Module map (slice each module first lands in is parenthesized):**

- `state.rs` — persistent `$BOOLE_MINER_STATE` (default `~/.boole/miner-state.json`); mode-0600 atomic tmp+rename writes; `boole.miner.state.v1` envelope `{schema, version, identity:{pk}, config:{dispatcher_url, llm:{backend, ...}, ...}, createdAt, updatedAt}`. (S15)
- `cli.rs` — internal subcommands `init`, `address`, `config get`, `config set`, `bounty`, `start`. Pluggable error helper `emit_typed_error(reason, exit_code, ...)`. Re-exported by `boole-cli` as `Mine { ... }`. (S15 + S16 + S19)
- `grinder/` (`mod.rs`, `ticket.rs`, `share.rs`, `submit_pow.rs`) — ticket-PoW grinder (find `n` such that `ticket_hash < T_ticket`), share grinder (`share_hash < T_share`), submission-PoW grinder (`submission_pow_hash < T_submit`). Per-attempt nonce iteration with `Hex32` byte representation (Boole consensus deviation from pof's 8/4/8-byte nonces). (S16 + S17)
- `submit_client.rs` — `Submitter` trait + `SubmitClient` blocking-HTTP impl posting `/submit` envelopes; `with_retry` adapter. (S16)
- `bounty_client.rs` — `BountyClient` blocking-HTTP impl posting `/bounties/:id/proof`; computes `proofHash = sha256(envelope_bytes)`, parses typed `BountyProofResult`. (S16)
- `chain_head.rs` — `ChainHeadFetcher` trait + `HttpHead` blocking-HTTP impl for `GET /head`; honors `--head-timeout-ms` for fail-fast smoke testing. (S17)
- `http_client.rs` — shared blocking-`reqwest` transport (timeouts, retries, status mapping) consumed by `submit_client` / `bounty_client` / `chain_head`. (S16 + S17)
- `proof_package.rs` — assembles the submission envelope (ticket + target + canonical proof + share + submission PoW) before signing. (S17)
- `target_emitter.rs` — `TargetEmitter` trait + deterministic `boole.calibration.pow.v3` emitter behind the `lake-target` feature; without the feature, only fixture-driven `--fixed-target-{seed-hex,render}` is available. (S18)
- `local_verify.rs` — `Verifier` trait + `AcceptingVerifier` (default fallback) + `LakeVerifier` (local `lake exec boole_check` pre-flight) behind the `lake-verify` feature. (S18)
- `llm_driver.rs` — `ProverDriver` trait + HTTP and process-spawn drivers + `MockProverDriver` for tests; honors `--mock-llm-response` for CLI smoke tests. (S18)
- `canonicalizer/` (`mod.rs`, `structural.rs`) — `Canonicalizer` trait + structural canonicalizer; lake-backed canonicalizer engaged behind the `lake-canon` feature. (S18)
- `mining_loop.rs` — orchestrates `fetch_head → grind_ticket → announce_ticket → for j ∈ [0, M): emit → with_retry(driver.generate) → verify → canonicalize → grind_share → grind_submission_pow → submit`. `MiningLoopDeps` boxes every collaborator; `run_mining_loop(deps, opts) → MiningSummary`; `LogSink = Box<dyn Fn(&MiningEvent) + Send + Sync>` for emitting structured `MiningEvent`s. (S19)
- `bin/boole-miner.rs` — thin binary wrapping `cli::run_mine`. (S15)

**Feature gates** (all OFF by default):
- `lake-canon` — wires `lake exec boole_emit` (canonicalizer). When off, the loop uses an identity canonicalizer and accepts whatever the prover emits.
- `lake-target` — enables the deterministic `v3` target emitter. When off, only `--fixed-target-{seed-hex,render}` works.
- `lake-verify` — enables `LakeVerifier`. When off, the loop falls back to `AcceptingVerifier` (every share is admitted to grinding).

**Tests:** 14 lib tests + 9 boole-cli integration tests. Full pipeline coverage with stub collaborators lives in `boole-miner/tests/mining_loop.rs`; CLI smoke tests verify argument parsing + summary envelope shape (closed-port head fetch for fail-fast).

**Acceptance:** `cargo test --workspace` PASS. `scripts/local-mining-smoke.sh` deferred — live-model evidence (any of `claude_cli`, `ollama_local`, `anthropic_api`) is a manual gate. Gate P5 PARTIAL PASS.

---

## 5. Benchmark fix plan

Reference: `scripts/boole-model-benchmark.py`. Latest runs under `artifacts/model-benchmarks/live-{claude-opus,claude-sonnet}-50-node-ticket-postfix-20260509/` and `regression-rfl-unique-20260509T001637Z/`.

**Status update (2026-05-09):** the `uniqueShares=1` collapse seen in the 2026-05-08 controlled run is **already resolved** by proof-source binding. Latest opus/sonnet runs show `uniqueShares=50/50, blocksProduced=50/50`. The `regression-rfl-unique` run confirms 3/3 unique even when every output is `rfl`. What remains is benchmark **soundness, capability signal, and reporting integrity** — not collapse.

**Priority order (revised):** B3 → B4 → B5 → B7(a) → B1 + B7(b) → B6. B2 demoted to nice-to-have.

### B3 — CoT and "Thinking..." stripping in extraction (priority 1, **PARTIAL**)

**Status: PARTIAL.** Some hardening already shipped. Remaining work scoped below.

**Already implemented:**
- ANSI escape stripping in `normalize_model_output` (`boole-model-benchmark.py:257-263`).
- "thinking"-prefix line skip inside the `last_proof_term_line` fallback (`:266+`).

**Remaining (this slice's scope):**
1. **`<think>…</think>` block stripping** — add a regex-based strip (multiline, non-greedy) to `normalize_model_output` so reasoning blocks emitted by reasoning-style models are removed before any extraction step.
2. **Final-candidate-only forbidden-token validation** — currently `\b(sorry|admit)\b` is checked on the raw normalized stdout. Move the check so it runs only on the final extracted candidate string (after fence-strip, JSON-key extraction, and last-proof-term-line resolution). This prevents the regex from firing on prompt-recall in CoT (Gemma's failure mode where the prompt's own forbidden-word list — `"sorry, or admit"` — is echoed verbatim and triggers the rejection).
3. **General CoT preamble stripping (provider-agnostic)** — for any provider, peel leading non-Lean prose lines (lines that start with prose tokens like `"Thinking"`, `"Here"`, `"Let"`, `"The"`, `"First"`, etc., and don't look like a Lean identifier/paren) until the first line matching `^[A-Za-z_(]` that also parses as a plausible Lean term. Already covered for ollama in the fallback path; lift to primary path and apply to all providers.

### B4 — `replayInvoked` flag, `replayPassed=null` for vacuous (priority 2)

**Problem.** `zero_score()` (`boole-model-benchmark.py:114`) defaults `replayPass=True`. A run where every attempt is rejected (Gemma 50/50) reports `replayPassed: true`. The replay report (`:1334`) cannot distinguish "replay verified the chain" from "replay was never run because no attempt reached the verifier."

**Fix.** Add `replayInvoked: bool` to per-row score and to summary. Recompute `replayPassed`:
- if no row has `replayInvoked == True` → summary `replayPassed = null` (vacuous, not "passed")
- otherwise `replayPassed = all rows where replayInvoked have replayPass == True`

Update leaderboard rendering to display `null` as "—" with a tooltip explaining "replay not invoked".

### B5 — `verifier_hash` versioning (priority 3)

**Problem.** `verifier_hash` is hard-coded `"boole-model-benchmark-ollama-v0"` for both ollama and claude-cli (`boole-model-benchmark.py:755`). The string flows into the POFP package via `proof_bridge.rs:235` and thus into `canon_hash`. Replacing the string in-place invalidates the canon hashes of every prior benchmark run that referenced it.

**Fix.** Move `verifier_hash` to `fixtures/benchmarks/verifier-hashes.json` with **version-keyed entries**:

```json
{
  "active": "v1",
  "versions": {
    "v0": { "hash": "boole-model-benchmark-v0", "deprecated": true,  "notes": "legacy ollama-only naming, retained for historical run validation" },
    "v1": { "hash": "boole-model-benchmark-v1", "deprecated": false, "notes": "provider-agnostic verifier semantics" }
  }
}
```

Rules:
- The benchmark driver reads `active` and uses that version's `hash` for new runs.
- Past runs persist their version key (`v0`, `v1`, …) in `benchmark-rows.ndjson`. Replay/validation looks the row's version up in `versions{...}` to recover the original `hash`.
- Bumping creates a new `vN+1` entry. Prior entries are **never deleted or rewritten** — they remain valid for historical runs.
- Drop the `ollama` substring from the new entry; the legacy `v0` keeps its original string for hash compatibility.

**Acceptance:** historical benchmark runs that recorded `verifierHashVersion: "v0"` continue to validate against the same `canon_hash` after a `v1` bump lands. New runs at `v1` use the new hash. No file rename or migration is needed; version coexistence is the contract.

### B7 — Target family documentation (split into two passes)

**Pass (a) — v1 spec + known limitations** (priority 4):
Add `docs/benchmark-target-families.md` documenting the **current** `boole.calibration.pow.v1` family:
- theorem template `theorem {name} : "{challenge}" = "{challenge}" := rfl`
- admissible proofs (essentially `rfl` and equivalent reductions)
- per-attempt diversification source (proof-source binding via challenge string in theorem name + comment headers)
- known limitation: family is **not a meaningful model-capability benchmark** because the optimal proof is structurally identical for all challenges. Document this explicitly so v1 is not misread as a leaderboard for proof-writing capability.

**Pass (b) — v2 design proposal** (paired with B1 below):
Same doc, separate section: design rationale + acceptance criteria for the next family.

Splitting B7 into two passes prevents Pass (a) from anchoring v2 design to v1's quirks.

### B1 — Non-trivial target family v2 (priority 5)

**Problem (revised).** `rfl`-only targets are not a meaningful model-capability benchmark. The proof writer never has to reason about anything beyond pattern-matching `rfl`. Block production now succeeds (collapse fixed) but the leaderboard signal is degenerate — every model that knows `rfl` ranks equally.

**Fix.** Introduce `boole.calibration.pow.v2` as a new `targetFamily` value (does NOT replace v1; v1 stays callable for back-compat smoke runs):
- (a) Port pof's `v031-lp` family from `projects/pof/lean/GenTests.lean` + `pof/calibration/`. Each challenge produces a different list-of-int problem; canonical proof differs structurally per challenge. This makes proof writing the bottleneck.
- (b) Alternative if (a) is too heavy initially: a small bespoke v2 family — `theorem T : f(challenge_bytes) = expected_value := by decide` where `f` requires non-trivial computation.

Recommendation: (a). Pair with B7 Pass (b) to document v2 admissibility and acceptance criteria up-front.

Acceptance: a 50-attempt run on v2 with claude opus produces non-zero `blocksProduced` AND meaningful per-model spread (≥10 percentage-point range across opus/sonnet/gemma).

### B6 — Timeout ergonomics (priority 6) — **PASS 2026-05-09**

**Problem.** `--timeout-sec` default 300 silently rejected frontier-model attempts on cold start + large prompts (latency frequently approached or exceeded 300s).

**Fix (shipped):**
1. **Per-attempt default raised 300 → 600s** (CLI + `run_benchmark` signature).
2. **`--timeout-sec 0`** documented as the no-per-attempt-timeout opt-out (plumbing already routes `0 → None`).
3. **`--max-run-seconds <N>`** wall-clock cap on the entire run, cooperative (in-flight attempts run to completion; new launches stop at deadline). Default `0` = no cap. On cap-trip the run finalizes its summary with rows-so-far and records `summary.runTerminationReason = "max-run-seconds"`. Exit code 0 (operator-designed termination, not failure).
4. **`summary.latencyDistribution = {p50Ms, p90Ms, p99Ms, sampleCount}`** emitted across `skipped=false ∧ elapsedMs>0` rows (linear-interp / numpy type-7 quantile contract). Leaderboard renders the three percentiles + `runTerminationReason` (when present) under the top-level summary block.

Evidence: 50 → 57 deterministic-CI tests PASS (new `B6TimeoutErgonomicsTests` × 7); CLI smoke `--max-run-seconds 1` against a 5-row × 0.6s mock spec ends with 2 rows + `runTerminationReason="max-run-seconds"` + exit 0; v1/v2/v3 family byte-frozen tests unchanged.

### B2 — Admission-nonce override (nice-to-have, demoted) — **PASS 2026-05-09 (Slice S(B2))**

**Status:** the original "Wire benchmark nonce into share envelope" framing was based on a misreading. Both pof and Boole's `/ticket` are observe-only — neither issues `n` to the miner. Miners must grind `n` themselves; the live-node `--use-node-ticket` path does not solve `n` diversification on its own.

**Shipped surface:** `boole-node submit-lean --admission-nonce <64-hex>` overrides `ProofSubmissionTemplate.n` (defaulting to the fixture's `constants.n` when unset). Validation (`len == 64 && all ascii_hexdigit`) runs *before* fixture parse + Lean spawn, so a malformed value fails fast with a typed JSON envelope on stderr (`{ok:false, command:"submit-lean", accepted:false, error:"malformed-admission-nonce", shareAccepted:false, blockProduced:false, invalidAccepted:0}`, exit 1) and never pays for `lake exec boole_check`. Reason code parallels S9's `malformed_pk` so downstream tooling can pattern-match across both surfaces. See `crates/boole-node/tests/submit_lean_admission_nonce.rs` (3 tests covering malformed reject + override changes share-hash + default uses fixture).

**Priority:** shipped after B1+B7(b) per plan order.

---

## 6. Sequencing and dependencies

```
N0 (contract fixes on raw TCP: /ticket payload, /health, /block/latest, /block/:height, JSON error envelope)
  ├─ C0 (CLI: boole node start) — wraps existing daemon binary
  ├─ C1 (CLI: boole block latest, boole block get)        ◄── demo-ready slice
  └─ N1 (axum migration — pure transport, contract frozen)
       ├─ N3 (account + reward ledger)
       │    └─ C3 (CLI: account balance, work, bounty)
       ├─ N4 (work API)
       ├─ N5 (bounty API + side-pool, behind activation guard)
       │
       └─ Miner crate (§4)
            └─ C4 (CLI: mine start/bounty)

C2 (keys) — independent, can ship anytime after N0
C5 (JSON contract spec) — drafted with C0/C1, finalized with C4

Benchmark track (independent of node/CLI work, runs in parallel):
  B3 (CoT/Thinking stripping)         ◄── highest priority hardening
   → B4 (replayInvoked + null semantics)
   → B5 (verifier_hash → fixture file)
   → B7(a) (v1 spec + known limitations docs)
   → B1 + B7(b) (v2 family port + design proposal)
   → B6 (timeout default 600s + opt-outs)

  B2 (admission-nonce override) — nice-to-have, after B1+B7(b)
```

Critical path to "pof parity claimed": N0 → N1 → N3 → N4 → N5 → miner crate → C1–C4. **Demo-ready milestone (C0+C1) lands at N0+0**, well before the full critical path completes.

---

## 7. Acceptance gates

Each gate is a `cargo test` + script combination. PASS requires green on all listed.

### Gate P0 — Contract surface frozen (raw TCP) — **PASS 2026-05-09**
- `scripts/local-node-smoke.sh` PASS
- `crates/boole-node/tests/http_contract.rs` PASS — every route's response schema matches `fixtures/protocol/http-contract/v1/` golden
- `POST /ticket` rejects unknown fields with typed `unexpected_field`; pof-shape `{c,pk,n}` body succeeds
- `GET /block/latest` and `GET /block/:height` return `PersistedBlock` round-trip identical to disk

### Gate P1 — Transport swap parity (axum) — **PASS 2026-05-09**
- All P0 gates remain green **without modifying the contract test fixtures**
- `cargo test --workspace` PASS
- 16 concurrent `/submit` regression test PASS (no race in `share_pool`)
- raw-TCP path deleted

### Gate P2 — (folded into P0)
Block read APIs ship as part of P0; gate number reserved for sequence stability.

### Gate P3 — Reward ledger parity — **PASS 2026-05-09 (Slice S9)**
- `replay_blocks` re-derives balances byte-for-byte from block store alone (no ledger file). **PASS** — driven from `fixtures/protocol/replay/v1.json` via `boot_re_derives_ledger_when_file_absent`, which boots without a seeded ledger and asserts `recovered.size() == fix.blocks.len()` and per-pk balances match `compute_block_credits` over each block.
- Multi-miner fixture: 2 blocks, 2 distinct proposers + 3 share owners; reward sums equal expected per-share + per-proposer-bonus formula. The Slice S9 acceptance fixture (`fixtures/protocol/replay/v1.json`) covers the multi-pk case (`PK_A=1, PK_1=1, PK_2=3, PK_3=1, PK_B=1` ↔ proposer credit + per-share credit per block); the protocol-level 4-block fixture remains a future hardening task but is not blocking parity since `compute_block_credits` and the divergence check are exercised end-to-end against this smaller fixture.
- `/account/:pk/balance` matches replay output. **PASS** — `account_balance_returns_recovered_balance_for_known_pk` asserts the live node's `/account/{pk}/balance` returns the expected balance + `asOfHeight` + `asOfC` for a node booted off the replay fixture.

### Gate P4 — Work + bounty parity
- **Work portion PARTIAL PASS 2026-05-09 (Slice S10):** `GET /work` and `GET /work/:id` serve a static catalog loaded at boot from `LocalNodeConfig.work_manifests_path` (flag `--work-manifests`, env `WORK_MANIFESTS_PATH`). Hit returns `{ok:true, work:<WorkManifest>}`; miss returns 404 + `{ok:false, reason:"work_not_found", id}`. With no path configured, `/work` still serves with empty list (catalog optional). Fixture `fixtures/protocol/work/v1.json` (2 manifests: bounty-derived + direct).
- **Bounty read portion PARTIAL PASS 2026-05-09 (Slice S11):** `GET /bounties` and `GET /bounties/:id` serve a static catalog loaded at boot from `LocalNodeConfig.bounties_path` (flag `--bounties`, env `BOUNTIES_PATH`). Hit returns `{ok:true, bounty:<Bounty>}`; miss returns 404 + `{ok:false, reason:"bounty_not_found", id}`. With no path configured, `/bounties` still serves with empty list. Fixture `fixtures/protocol/bounties/v1.json` (2 bounties: alpha-1 lean/open, beta-1 wasm/solved).
- **Bounty proof-write portion PARTIAL PASS 2026-05-09 (Slice S12):** `POST /bounties/:id/proof` accepts `{proofHash:hex32, prover:hex32, envelope:any}`, dispatches by `bounty.verifier.kind` against a `HashMap<String, Arc<dyn BountyProofVerifier>>` registry on `LocalNodeState`, dedup-peeks the registry before any verifier call, flips status to `solved` on accept via `BountyRegistry::submit_proof`, and appends an audit-ledger event (`{schemaVersion:1, kind:"proof", workId, problemHash, verifierKind, ts, proofHash, solverPk, accepted, reward, credit}`) to the configured `--bounty-events` NDJSON file. 8-branch validation order matches pof byte-frozen: 404 `bounty_not_found` → 400 `bad_proof_hash` → 400 `bad_prover` → dedup peek (200 `duplicate:true, accepted:cached`, no verifier call) → 501 `no_verifier` → 409 `bounty_terminal` → verifier dispatch → 502 `verifier_error` on `Err(msg)`. Lean kind auto-registered via `LeanBountyVerifier` adapter when `--lean-checker-dir` is set; `verifierHash` is read from `bounty.verifier.metadata` and bound into `LeanRunnerConfig::new(verifier_hash)`. Boot replays the audit ledger so `solved` status survives node restart. CLI: `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32> --envelope <path|inline>` prints bare `<status>` (`solved`/`open`/`duplicate`) by default; `--json` forwards the server envelope. Fixture `fixtures/protocol/bounties/v1-mock.json` (4 entries: gamma-1 mock-accept, delta-1 mock-reject, epsilon-1 withdrawn, zeta-1 unknown-kind) for the test-only verifier-injection seam (`LocalNodeConfig.bounty_verifiers`).
- `bounty_registry.rs` integration tests pass against live `RuntimeAdmissionState` — blocked on N5 side-pool wiring (S13+).
- **Bounty announce flow PASS 2026-05-09 (Slice S13b):** `boole keys new --dev` → `boole bounty announce …` → bounty appears on `GET /bounties` and a `kind:"create"` line lands in the configured `--bounty-events` ledger; restart-replay rehydrates the dynamically-announced bounty over the static catalog (static wins on overlap with a stderr warning). 8 announce-route tests + 6 CLI tests + 2 ledger tests cover the full path.
- **Bounty status-transition flow PASS 2026-05-09 (Slice S14):** `boole keys new --dev` → `boole bounty status …` → bounty status flips on `GET /bounties/:id` and a `kind:"status_change"` line (with server-derived `prevStatus`, `announcerPk`, `verifierKind`) lands in the same `--bounty-events` ledger; restart-replay rehydrates the post-transition state (e.g. `withdrawn`) on top of the static catalog. Reason vocab: 400 `bad_status_value` (handler-side enum check), 400 `bounty_id_mismatch` (URL `:id` vs payload `id`), 400 `invalid_status_transition` (fallback for future stricter rules), 409 `bounty_terminal` (terminal-state guard from `BountyRegistry::update_status`), 401 `signature_invalid`, 404 `bounty_not_found`. 6 status-route tests + 4 CLI tests cover the full path.
- Replay verifies bounty-credit invariants — blocked on S13+ (audit ledger captures accepted proofs in S12; reward credit to `RewardLedger` is the next hardening step).

### Gate P5 — Miner parity — **PARTIAL PASS 2026-05-09 (Slices S15-S19)**
- **`boole-miner` crate: cargo test PASS for all modules. PASS** — 14 lib tests + 9 boole-cli integration tests green; full pipeline coverage with stub collaborators in `boole-miner/tests/mining_loop.rs`; CLI smoke tests verify argument parsing + summary envelope shape (closed-port head fetch for fail-fast). Module map in §4.
- **`scripts/local-mining-smoke.sh`: end-to-end live-model evidence. PENDING** — script not yet written; live-model evidence (any of `claude_cli`, `ollama_local`, `anthropic_api` driver) is a manual / non-CI gate. Once at least one driver shows ≥1 accepted share against `boole node start` end-to-end, this bullet flips to PASS.

### Gate P6 — CLI parity
- **C0 + C1 PASS 2026-05-09 (Slice S3):** `boole node start --port --data-dir [--scenario] [--genesis] [--max-requests]` spawns boole-node as a child process; `boole block latest --node URL --json` and `boole block get --height N --node URL --json` print the typed envelope on stdout and forward the server's `{ok:false, reason:<kebab>, ...}` envelope on stderr (non-zero exit). End-to-end demo `examples/boole-block-demo.sh` verifies empty-chain → submit → latest → get-by-height with stable block-c across the three calls.
- **C2 PASS 2026-05-09 (Slice S6):** `boole keys new --id <name> [--dev] [--dry-run]`, `boole keys list`, `boole keys show --id <name>` ship the local-key surface at `$BOOLE_KEYS_DIR` (or `$HOME/.boole/keys`) with the schema-versioned envelope `{id, pk, createdAt, schema:"boole.keys.v1"}`, mode-0600 atomic tmp+rename writes, and the typed-error vocabulary (`bad_request` exit 2, `key_already_exists` / `key_not_found` exit 3) on stderr. `--dev` seeds deterministically from id (blake3 of `"boole-keys-dev::" + id`); `--dry-run` emits the envelope without touching disk.
- **C2 PASS extended 2026-05-09 (Slice S13a):** `keys new` now generates ed25519 keypairs and emits `boole.keys.v2` envelopes `{id, pk, sk, createdAt, schema:"boole.keys.v2"}`, where `sk` is the 32-byte ed25519 seed (hex64). `--dev` seeds the ed25519 keygen via `blake3("boole-keys-dev::" + id)` so dev keys remain deterministic. `keys list` / `keys show` continue to surface pre-S13a v1 envelopes byte-equal (no implicit retrofit). New `boole keys sign --id <id> --payload <path|inline> [--json]` signs `SHA-256(canonical_json(payload))` with the stored v2 key and prints either the bare hex64 signature or a full `boole.signed.v1` envelope under `--json`; v1 keys are refused with `legacy_v1_key` exit 3. New `boole keys verify --pk <hex32> --signature <hex64> --payload <path|inline> [--json]` is stateless (no keys-dir read) and prints bare `valid`/`invalid` (both exit 0 because verification ran successfully); wire-malformed `--pk` / `--signature` hex emit `bad_pk` / `bad_signature` typed envelopes on stderr with exit 2. The signing primitives (`boole-core::canonicalize`, `boole-core::SigningKeyV2`, `boole-core::verify_signature`, `boole-core::SignedEnvelope`) are the foundation for S13b's `POST /bounties` announce flow.
- **C3 PARTIAL PASS 2026-05-09 (Slice S9):** `boole account balance --pk <hex32> [--node URL] [--json]` prints the bare `balance` to stdout by default and the full server envelope (`{ok:true, pk, balance, asOfHeight, asOfC}`) under `--json`. Local clap-level validation of `--pk` short-circuits malformed inputs with stderr `{ok:false, reason:"malformed_pk", pk}` and exit 2 (no node round-trip). Unknown but well-formed pks return `balance:"0"` with exit 0. Work / bounty (`boole work …`, `boole bounty …`) remain blocked on N4 / N5.
- **C3 PARTIAL PASS 2026-05-09 (Slice S10):** `boole work list [--node URL] [--json]` prints terse `<workId>\t<familyId>\t<status>` lines by default and the full server envelope under `--json`. `boole work get --id <ID> [--node URL] [--json]` prints the bare `verifier.metadata.verifierHash` by default and the full envelope under `--json`. Unknown id forwards the server's typed `{ok:false, reason:"work_not_found", id}` envelope on stderr with exit 1 (matches `block get` precedent). Bounty (`boole bounty …`) still blocked on N5.
- **C3 PARTIAL PASS 2026-05-09 (Slice S11):** `boole bounty list [--node URL] [--json]` prints terse `<id>\t<domain>\t<status>\t<reward>` lines by default (4-column tab-separated) and the full server envelope under `--json`. `boole bounty get --id <ID> [--node URL] [--json]` prints the bare `verifier.metadata.verifierHash` by default and the full envelope under `--json`. Unknown id forwards the server's typed `{ok:false, reason:"bounty_not_found", id}` envelope on stderr with exit 1 (matches `work get` precedent). `boole bounty announce` deferred to S13+ (write surface S12 covers `submit`).
- **C3 PARTIAL PASS 2026-05-09 (Slice S12):** `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32> --envelope <path|inline> [--node URL] [--json]` prints a bare `<status>` word by default (`solved` on accept, `open` on reject, `duplicate` when the server returns `duplicate:true`) and the full server envelope under `--json`. `--envelope` accepts inline JSON or a file path (1 MiB cap on file reads). 4xx/5xx responses (e.g. `bounty_not_found`, `bad_proof_hash`, `bad_prover`, `no_verifier`, `bounty_terminal`, `verifier_error`) forward the server's typed envelope on stderr with exit 1, matching the `block get` / `bounty get` precedent. `boole bounty announce` deferred to S13+.
- **C3 PASS 2026-05-09 (Slice S13b):** `boole bounty announce --id <id> --domain <domain> --problem-hash <hex32> --verifier-kind <kind> --verifier-metadata <path|inline> --reward <u128> --deadline <u64-ms> --signing-key <id> [--ts <u64-ms>] [--node URL] [--json]` builds a `boole.bounty.announce.v1` payload, signs it with the stored v2 key (refuses v1 with `legacy_v1_key` exit 3), and POSTs a `boole.signed.v1` envelope to `/bounties`. Bare bounty id default, `--json` returns the full server envelope. Local hex32 lint on `--problem-hash` short-circuits malformed inputs with `malformed-problem-hash` exit 2 (no network round-trip). `--verifier-metadata` accepts inline JSON or a file path (1 MiB cap, shared `read_json_arg` helper). Server 4xx/5xx (e.g. `bounty_already_exists`, `signature_invalid`, `bad_envelope`, `bad_payload`) forwards the typed envelope on stderr with exit 1.
- **C3 PASS extended 2026-05-09 (Slice S14):** `boole bounty status --id <id> --new-status <open|solved|expired|withdrawn> [--reason <text>] --signing-key <id> [--ts <u64-ms>] [--node URL] [--json]` builds a `boole.bounty.status.v1` payload, signs it with the stored v2 key (refuses v1 with `legacy_v1_key` exit 3), and POSTs a `boole.signed.v1` envelope to `/bounties/:id/status`. Bare `<newStatus>` word default, `--json` returns the full server envelope. Clap-level `value_parser` constrains `--new-status` to the four enum values locally (no network round-trip on a typo). Server 4xx/5xx (e.g. `bounty_terminal`, `bounty_id_mismatch`, `invalid_status_transition`, `bad_status_value`, `signature_invalid`, `bounty_not_found`) forwards the typed envelope on stderr with exit 1. Full bounty CLI surface (list / get / submit / announce / status) now matches pof.
- **C4 PASS 2026-05-09 (Slices S15-S19):** `boole mine init / address / config get / config set / bounty / start` all live, dispatching directly into `boole_miner::cli::run_mine` (library call, no subprocess). `mine start` runs the full pipeline (head fetch → ticket grind → announce → per-j target/LLM/verify/canonicalize/share grind/submission PoW → submit) with pluggable trait collaborators; `--max-cycles` / `--max-shares` stop conditions; `--mock-llm-response` / `--mock-verify-accept` / `--fixed-target-{seed-hex,render}` smoke knobs; final `summary:` envelope shape `{cyclesRun, sharesAccepted, ticketsFound, networkErrors, ...}`. `mine bounty` reads an envelope file, computes `proofHash = sha256(envelope_bytes)`, POSTs to `/bounties/:id/proof` via `BountyClient`, prints typed `BountyProofResult` JSON. Live-model evidence remains a manual / non-CI gate (Gate P5 PARTIAL).
- All pof CLI subcommands have a matching `boole <cmd>` with identical JSON shape
- `boole --help` agent contract block matches pof `docs/cli-json-contract.md`
- A simple AFK-agent test script (the `boole-demo.sh` analogue, `examples/boole-demo.sh`) runs green

### Gate B — Benchmark soundness

Gate B is split into two subgates: **B-CI** (deterministic, runs in CI on every PR) and **B-Live** (live model evidence, gathered manually outside CI). Live model runs cost money, depend on external services (claude.ai, ollama daemons), and are non-deterministic — they must not block CI.

#### Gate B-CI (deterministic, hard CI gate)
- B3 unit-level coverage: synthetic CoT inputs (`<think>…</think>` block, prompt-recall echo of forbidden tokens, mixed Thinking… preamble) round-trip through `extract_proof_term_candidate()` with the expected post-fix outcome (extracted candidate clean, forbidden-token check applied to final candidate only). Test fixtures in `scripts/test_model_benchmark.py`.
- **B4 PASS 2026-05-09 (Slice S4):** a synthetic run where every row has `generatedAttempt=False` produces summary `replayPassed: null`, not `true`. Per-row `replayInvoked` distinguishes verified-pass from never-invoked. Asserted via `scripts/test_model_benchmark.py`.
- **B5 PASS 2026-05-09 (Slice S5):** `verifier_hash` is read from `fixtures/benchmarks/verifier-hashes.json` with version-keyed entries. After bumping `active` from `v0` to `v1`, a replay of a historical row that recorded `verifierHashVersion: "v0"` resolves to the v0 hash and matches the original `canon_hash`. Test asserts both branches against fixture rows.
- **B7(a) PASS 2026-05-09 (Slice S4):** `docs/benchmark-target-families.md` exists; CI lint asserts that every `targetFamily` value referenced in `scripts/boole-model-benchmark.py` has a section in the doc.
- **B6 PASS 2026-05-09 (Slice S(B6)):** `--timeout-sec` default 600 (raised from 300); `--timeout-sec 0` propagates as `timeout_s=None` (no per-attempt timeout); `--max-run-seconds <N>` cooperative wall-clock cap (default 0 = no cap, validated `>= 0`) stops launching new attempts at the deadline and finalizes the summary with rows-so-far + `runTerminationReason="max-run-seconds"` (exit 0); `summary.latencyDistribution = {p50Ms, p90Ms, p99Ms, sampleCount}` over `skipped=false ∧ elapsedMs>0` rows (linear-interp / numpy type-7); leaderboard surfaces all three percentiles + the optional `runTerminationReason` line. Asserted via 7 fixture-driven tests in `scripts/test_model_benchmark.py::B6TimeoutErgonomicsTests` plus a CLI smoke run against a 5-row × 0.6s mock spec with `--max-run-seconds 1`.
- **B1 + B7(b) PASS 2026-05-09 (Slice S8):** `boole.calibration.pow.v2` ships as a `targetFamily` selectable via `--benchmark-mode mining-v2`. The driver loads the family, generates 5 deterministic challenges from `runId="ci-v2-fixture"`/`target="ollama:test"`/`attempt_index 0..4`, and each per-attempt theorem has a non-`rfl`-equivalent canonical proof (right-folded `Eq.trans` of `length_mapAdd / length_mapMul / length_sortAsc` from `Boole.Family.V0Helpers`). Asserted via 9 fixture-driven tests in `scripts/test_model_benchmark.py::MiningV2FamilyTests` (no live LLM call), plus the byte-identical golden-fixture replay at `fixtures/benchmarks/v031-lp/golden-instances.json` and a Lean elaboration smoke (`lake exec boole_check` on the seed-0 wrapped candidate, exit 0).
- **B1 + B7(b) extended 2026-05-09 (Slice S8b):** `boole.calibration.pow.v3` ships as a `targetFamily` selectable via `--benchmark-mode mining-v3`, porting pof's full v031 5-way mixed generator (allSatisfy / sortedAsc / dedupFirst / partitionEq / lengthPreserved) at N=1. Each per-attempt theorem dispatches into one of the five invariant classes; canonical proofs cite the matching witness lemma from `Boole.Family.V0Helpers` (`all_filterByPred_self`, `pairwise_sortAsc`, `nodup_dedup`, `partition_eq_filter_filter`, or — for lengthPreserved — the v2 `Eq.trans`-composition shape). Asserted via 10 fixture-driven tests in `scripts/test_model_benchmark.py::MiningV3FamilyTests`, plus the byte-identical golden-fixture replay at `fixtures/benchmarks/v031-mixed/golden-instances.json` (10 instances spanning all 5 invariants with chainLen=1 and chainLen≥3 cases), plus a Lean elaboration smoke covering one wrapped candidate per invariant class (`lake exec boole_check` exit 0 for all 5). v1 + v2 stay byte-frozen via separate cursor-seed strings and separate target families.

#### Gate B-Live (manual / live evidence — NOT a CI gate)
Run periodically (e.g. weekly) against the latest model versions. Results recorded as artifacts under `artifacts/model-benchmarks/` and summarized in a leaderboard, **not** gating CI:
- B3 live evidence: a Gemma 26b run shows `verifier.invoked == true` for ≥ 80% of attempts (CoT no longer auto-rejected by forbidden-token regex). Run via `scripts/preflight-model-benchmark.sh --target ollama:gemma4:26b --use-node-ticket` and attach the resulting summary JSON.
- B1 + B7(b) live evidence: a 50-attempt v2 run produces non-zero `blocksProduced` for at least one model AND a per-model spread of ≥10 percentage points across opus/sonnet/gemma. Run after S8 lands. v1 stays callable for back-compat smoke runs.

Recording protocol: attach the run's `benchmark-summary.json` to the slice's PR/issue as evidence. CI does not block on this — but a slice claiming to satisfy B3 or B1 must show the live evidence before the slice is marked complete. Failures here trigger a follow-up bug, not a CI red.

---

## 8. Risks and trade-offs

- **HTTP framework swap (N1) is invasive.** Cleanly separating runtime state from transport is the win; the risk is dropping subtle behavior (1 MiB body cap, 15s socket timeout) on the floor. Add explicit regression tests for each before swapping.
- **Reward ledger persistence format.** Picking pof's NDJSON format costs nothing and unlocks shared replay tooling. Diverging here for a "better" format pays no dividend; defer "better" to Phase N6.
- **Miner crate scope creep.** The pof TS miner has accumulated bounty mode, state checkpointing, multiple LLM drivers. Port them incrementally; don't gate Gate P5 on full driver matrix. One driver (`claude_cli`) is enough for parity claim; others can land in patch releases.
- **Benchmark theme swap (B1) changes the historical record.** Past leaderboards become incomparable. Mitigation: give the new family a new `targetFamily` value (`boole.calibration.pow.v2` or similar) and keep v1 callable for back-compat smoke runs.
- **`boole-cli` calling pattern.** pof CLI shells out via `tsx` subprocess; Rust CLI can call libraries directly. Faster, harder to test in isolation. Use library calls in-process but keep an `--exec-mode subprocess` flag for the few cases where a separate process is desired (long-running mine).

---

## 9. Out of scope (tracked, not planned here)

- P2P networking (Phase F in `migration-status-and-next-steps.md`)
- State root / state hash beyond reward ledger
- Closed testnet packaging
- MCP daemon (`boole-mcp` Rust port)
- Hardware wallet / KMS
- Production telemetry beyond Prometheus `/metrics`

These are noted because the parity claim above is for the **node + CLI surface** specifically. Full L1 production readiness is a strictly larger goal.

---

## 10. Open questions

1. Should `boole-cli` consume the miner crate as a library or shell out? — Library preferred; revisit if it complicates testing.
2. Reward denomination unit: keep raw `u64` share-count or introduce a token unit now? — Defer until P3 lands; rename later via fixture migration.
3. Bounty announce signature scheme: reuse pof's? — Yes, default to parity. Re-evaluate at closed-testnet entry.
4. Should `/metrics` be Phase N1 or N6? — N1 is cheap with `axum`'s ecosystem; recommend including in N1.

---

## Appendix A — Executable slices

The phases above are the **roadmap**. Day-to-day execution ships in narrow slices. **Slice numbering reflects recommended execution order; track tags `[N]/[C]/[B]` indicate which roadmap track the slice belongs to.** B-track and node-track slices are independent — when in parallel review capacity, ship them concurrently.

**Slice queue:**

### Slice S1a [N] — `/ticket` payload contract (recommended first) — **SHIPPED 2026-05-09**
**Scope:** N0, smallest blocking unit.
- Tighten `/ticket` body shape to pof `{c, pk, n}`; reject extras with typed `{ok:false, reason:"unexpected_field", field:...}` (HTTP 400).
- Move `normalize_pow_fields` (`local_node.rs:344`) to `/submit`-only.
- New `crates/boole-node/src/http_error.rs` standardizing the JSON error envelope (`{ok:false, reason:<kebab>, detail?, ...context}`).
- New `crates/boole-node/tests/http_contract.rs` with `/ticket` golden fixtures at `fixtures/protocol/http-contract/v1/ticket-{ok,unexpected-field,bad-hex}.json`.
- **Update `scripts/boole-model-benchmark.py` `--use-node-ticket` request shape** to send `{c, pk, n}` only. Today the benchmark posts the full submit-shaped body (with `j`, `nonceS`, etc.) which will start returning 400 `unexpected_field` once the server tightens. Land both changes in the same slice so the benchmark's live runs (`live-claude-*-node-ticket-*`) keep working continuously. Add a regression test in `scripts/test_model_benchmark.py` asserting the posted body has exactly the three fields.

**Acceptance:** `/ticket` contract test PASS, `cargo test --workspace` PASS, `scripts/test_model_benchmark.py` PASS, no other route changed. **CI green and shipped before any other slice** so downstream slices (S1b, miner work, benchmark `--use-node-ticket`) build on a stable contract.

**Why first:** smallest blast radius, blocks downstream node + miner + benchmark integrations, no new endpoints to design. Benchmark co-fix prevents a self-inflicted regression on the live `--use-node-ticket` runs.

**Shipped acceptance evidence (2026-05-09):** `cargo test --workspace` 0 failures across 121 tests; `python3 scripts/test_model_benchmark.py` 18/18 OK; `crates/boole-node/tests/http_contract.rs` golden test PASS against the four ticket fixtures (`ok`, `unexpected-field`, `bad-hex`, `missing-field`). See §11 for the per-file change record.

### Slice S1b [N] — `/block/{latest,:height}` + `/health` + smoke — **SHIPPED 2026-05-09**
**Scope:** N0 second wave. Depends on S1a's golden infra.
- Add `GET /block/latest`, `GET /block/:height`, `GET /health` (pof shape).
- Extend `tests/http_contract.rs` with goldens for the three new routes (`not_found` for missing height).
- New `scripts/local-node-smoke.sh` exercising every N0 route end-to-end.

**Acceptance:** Gate P0 PASS in full. `local-node-smoke.sh` green.

**Shipped acceptance evidence (2026-05-09):**
- `cargo test -p boole-node --test http_contract` → 3/3 PASS (S1a `ticket_contract_fixtures_match` + new `read_routes_on_empty_chain_match_fixtures` + new `block_read_routes_after_first_block_match_fixtures`).
- `cargo test --workspace` → 0 failures, no regressions.
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 18/18 PASS.
- `scripts/local-node-smoke.sh` → PASS, exercises `/status`, `/health`, `/block/latest` (empty), `/block/notanumber` (400), `/block/9999` (404), `/submit`, `/block/latest` (post-submit), `/block/0`, `/head`.

### Slice S2 [B] — B3 CoT/Thinking stripping (priority-1 benchmark hardening) — **SHIPPED 2026-05-09**
**Scope:** B3, the priority-1 benchmark item. Independent of S1; can run in parallel.
- `<think>…</think>` block stripping in `normalize_model_output`.
- Move forbidden-token check after extraction (final-candidate-only validation).
- Lift general CoT preamble stripping from the `last_proof_term_line` fallback to the primary extraction path; apply to all providers, not only ollama.
- Regression test: a recorded Gemma CoT that previously tripped `candidate-forbidden-token` via prompt-recall must now reach the verifier.

**Acceptance:** Gate B's B3 bullet PASS — Gemma 26b run shows `verifier.invoked == true` for ≥80% of attempts.

**Shipped acceptance evidence (2026-05-09):**
- 4 new/updated extractor tests in `scripts/test_model_benchmark.py` PASS deterministically (Red→Green): `<think>`-with-prompt-recall reaches verifier as `rfl`, `<think>`-only-prompt-recall (forbidden token only inside think) reaches verifier as `Eq.refl 1`, final-candidate `sorry` still rejects with `candidate-forbidden-token`, and the original ollama prompt-echo case keeps working under the renamed provider-agnostic `final-proof-line` label.
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 21/21 PASS.
- `cargo test --workspace` → 0 failures (S1a/S1b regressions still green).
- `bash scripts/local-node-smoke.sh` → PASS (no regression in the N0 contract surface).
- **Gate B-Live B3** (Gemma 26b ≥80% verifier.invoked) is a live-model gate that requires an actual ollama-gemma run; the deterministic CI contract — that the extractor reaches the verifier on `<think>`-style CoT instead of pre-rejecting on prompt-recall — is now PASS.

### Slice S3 [C] — `boole node start` + `boole block latest/get` (demo-ready slice) — **SHIPPED 2026-05-09**
**Scope:** **C0 + C1 together** — `boole node start` is required for the demo, so it ships in the same slice as the block-query CLIs. Depends on S1b (block read endpoints exist) and S1a (CLI requires the standardized JSON error envelope to surface typed errors).
- `boole node start [--port N] [--data-dir DIR] [--genesis HEX32]` — wraps the existing daemon binary with env-var fan-out (`PORT`, `BLOCKSTORE_PATH`, `REWARDLEDGER_PATH`, `GENESIS_C`). Mirrors pof `booleCli.ts:116-134`.
- `boole block latest [--node URL] [--json]`.
- `boole block get --height N [--node URL]`.
- Output matches pof CLI JSON contract (`{ok:true, ...}` on stdout, typed errors on stderr).
- `examples/boole-block-demo.sh` end-to-end: `boole node start` (background) → `boole block latest` → `boole block get --height 0`.

**Acceptance:** demo-ready slice. Gate P0 + C0/C1 bullets of Gate P6 PASS. CI green. Demo artifact: `examples/boole-block-demo.sh` JSON output suitable for the frontpage/VC card.

### Slice S4 [B] — B4 (`replayInvoked`) + B7(a) (v1 family docs) — **SHIPPED 2026-05-09**
**Scope:** priority-2 benchmark hardening + first doc pass. Independent of S1-S3.
- Add `replayInvoked: bool` to per-row score and summary.
- Recompute `replayPassed` per §5 B4 (`null` when no row invoked replay).
- Update leaderboard renderer to display `null` as "—".
- New `docs/benchmark-target-families.md` documenting v1 family + known limitations (priority-4 in B-track).
- CI lint: every `targetFamily` value in `boole-model-benchmark.py` has a doc section.

**Acceptance:** Gate B's B4 and B7(a) bullets PASS.

**Parallelism note:** the Python change and the markdown are zero-coupled. Land as one PR for CI economy or split — no technical reason to block one on the other.

### Slice S5 [B] — B5 versioned `verifier_hash` file — **SHIPPED 2026-05-09**
**Scope:** priority-3 benchmark hardening.
- New `fixtures/benchmarks/verifier-hashes.json` with version-keyed entries (see §5 B5).
- Driver reads `active` for new runs; persists `verifierHashVersion` field in `benchmark-rows.ndjson`.
- Replay/validation resolves historical rows via their version key. Legacy `v0` entry preserves the original `"boole-model-benchmark-ollama-v0"` string.

**Acceptance:** Gate B's B5 bullet PASS.

### Slice S6 [C] — `boole keys new/list/show` (C2) — **SHIPPED 2026-05-09**
**Scope:** independent of node-track. Unblocks bounty/announce flows in later phases.
- Local key storage `~/.boole/keys/<id>.json`, mode 0600, atomic tmp+rename.
- pof JSON contract for success/error envelopes.

**Acceptance:** Gate P6's C2 bullet PASS.

### Slice S7 [N] — N1 axum migration — **SHIPPED 2026-05-09**
**Scope:** pure transport diff. Possible only after N0 contract is fully frozen (S1a + S1b) and exercised by S3 (CLI consumer) for at least one weekly cycle.
- Replace raw-TCP server with axum on tokio.
- All N0 routes unchanged in shape. Reuse `tests/http_contract.rs` and `local-node-smoke.sh` unchanged as the regression net.
- 16-concurrent `/submit` test added.

**Acceptance:** Gate P1 PASS. Raw-TCP path deleted.

### Slice S8 [B] — B1 + B7(b) v2 family — **SHIPPED 2026-05-09**
**Scope:** priority-5 benchmark capability. Heaviest B-track slice.
- Ported pof `v031-lp` family (narrowed to N=1 lp-only over the length-preserving op family `{mapAdd, mapMul, sortAsc}`) from `projects/pof/lean/Boole/Family/V0Helpers.lean` + `V0ProbeV031.lean`.
- New `targetFamily: "boole.calibration.pow.v2"` selected via `--benchmark-mode mining-v2`. v1 stays byte-frozen and callable.
- Appended a `## boole.calibration.pow.v2` section to `docs/benchmark-target-families.md` documenting Mode / Family origin / Theorem template / Canonical proof shape / Lean library / Why non-rfl-equivalent / Difficulty / Verifier hash / Acceptance criteria / Known limitations.

**Acceptance:** Gate B's B1+B7(b) **CI** bullet PASS — `MiningV2FamilyTests` in `scripts/test_model_benchmark.py` covers mode dispatch, attempt-context shape, per-challenge proof divergence + non-rfl evidence, `.trans` for chainLen≥2, no `.trans` for chainLen=1, wrapped Lean module + lottery-binding comments, prompt contract, argparse choice, and a 5-instance golden-fixture replay regression. Lean elaboration confirmed for fixture seed 0 (`lake exec boole_check` exit 0). Live evidence (50-attempt v2 run with non-zero `blocksProduced` and ≥10pp per-model spread) is a manual gate, deferred to a separate live-run artifact.

### Slice S8b [B] — B1 + B7(b) v3 mixed-family expansion — **SHIPPED 2026-05-09**
**Scope:** extend the v031 family to its full 5-way invariant generator without disturbing v1 or v2.
- Ported pof `v031` mixed family (N=1) from `projects/pof/lean/Boole/Family/{V0Helpers,V0ProbeV031,ListInvariantsV031}.lean`, covering `{allSatisfy, sortedAsc, dedupFirst, partitionEq, lengthPreserved}`.
- Extended `Boole.Family.V0Helpers` with `filterByPred` + `dedup` op definitions and the four pof "truthy" witness lemmas (`all_filterByPred_self`, `nodup_dedup`, `pairwise_sortAsc`, `partition_eq_filter_filter`).
- New `targetFamily: "boole.calibration.pow.v3"` selected via `--benchmark-mode mining-v3`. v1 + v2 stay byte-frozen and callable; v3 uses its own cursor seed string (`|v031-mixed-cursor`).
- Appended a `## boole.calibration.pow.v3` section to `docs/benchmark-target-families.md` documenting the 5-way invariant table, per-branch theorem template + canonical proof shape, Pred → Bool rendering, Lean library, why each branch is non-rfl-equivalent, difficulty (chainLen ∈ [1, 6], full v0.2 op set + 6 pred kinds), verifier hash, acceptance criteria, known limitations.

**Acceptance:** Gate B's B1+B7(b) **CI** bullet remains PASS with v3 added — `MiningV3FamilyTests` in `scripts/test_model_benchmark.py` covers mode dispatch, attempt-context shape, per-invariant witness-lemma matching, 5-way coverage in 50 samples, non-rfl-equivalence, `.trans` for lengthPreserved long chains, wrapped Lean module + extended lottery-binding comments (`invariantClass / chainLen / D`), prompt content covering all 5 branches and helper names, argparse choice, and a 10-instance golden-fixture replay regression at `fixtures/benchmarks/v031-mixed/golden-instances.json` covering all 5 invariants + chainLen=1 + chainLen≥3 cases. Lean elaboration confirmed for one wrapped candidate per invariant class (`lake exec boole_check` exit 0 for all 5 files). Live evidence (50-attempt v3 run) is a manual gate.

### Slice S19 [C4] — Mining loop + CLI wiring (`boole mine start` / `boole mine bounty`) — **SHIPPED 2026-05-09**
**Scope:** close out Phase C4 by orchestrating S15-S18's collaborators into the canonical mining loop and exposing it as `boole mine start`. The `boole mine bounty` driver from S16 also wires into `boole-cli` here.

- New `boole-miner::mining_loop` module — `run_mining_loop(deps: MiningLoopDeps, opts: MiningLoopOptions) -> MiningSummary`. `MiningLoopDeps` boxes every collaborator (`pk: Hex32`, `chain_head`, `emitter`, `driver`, `verifier`, `canonicalizer`, `submit_client`, optional `prompt_builder`, optional `log: LogSink`, optional `sleeper`). The loop performs `fetch_head → grind_ticket → announce_ticket → for j ∈ [0, M): emit → with_retry(driver.generate) → verify → canonicalize → grind_share → grind_submission_pow → submit`. `LogSink = Box<dyn Fn(&MiningEvent) + Send + Sync>` is extracted as a type alias to satisfy clippy `type_complexity`. Per-cycle stop conditions: `cyclesRun >= max_cycles` or `sharesAccepted >= max_shares`.
- New `boole-miner::cli::start` handler — owns `--max-cycles`, `--max-shares`, `--head-timeout-ms`, `--mock-llm-response`, `--mock-verify-accept`, and the paired `--fixed-target-seed-hex` / `--fixed-target-render` (handler enforces "must be provided together"). Resolves the configured driver/emitter/verifier from miner state, defaults to `AcceptingVerifier` when the `lake-verify` feature is OFF, and prints a final `summary: <json>` envelope for stdout-line scrapers to read.
- `boole-cli::main` adds the `Mine { Start, Bounty, ... }` subtree by re-exporting `boole_miner::cli::{MineCommand, run_mine}` — so the standalone `boole-miner` binary and the `boole mine ...` subcommand share the same code paths. `boole-miner` added to `crates/boole-cli/Cargo.toml` `[dependencies]`.

**Acceptance:** 14 lib tests in `crates/boole-miner/tests/mining_loop.rs` cover the full pipeline with stub collaborators (head-fetch failure path, ticket-grind path, per-j emit/generate/verify/canonicalize/share-grind/submission-PoW path, max-cycles termination, max-shares termination, retry-on-network-error, summary aggregation). 9 boole-cli integration tests across `mine_init_cli.rs`, `mine_bounty_cli.rs`, `mine_start_cli.rs` cover the CLI binary's argument parsing + summary envelope shape (closed-port head fetch for fail-fast smoke). `cargo test -p boole-miner -p boole-cli` PASS; clippy `--all-targets -- -D warnings` clean (4 fixes during slice: `unnecessary_cast`, `collapsible_if`, `manual_contains`, `type_complexity`). Phase C4 PASS, Gate P5 PARTIAL PASS (live-model evidence remains a manual gate — `scripts/local-mining-smoke.sh` deferred), Gate P6 C4 PASS.

**Deviations from plan §4:** `mining_loop.rs` is the single orchestration module; pof's `bounty_mode.rs` and `submission_pow_grinder.rs` (separate file) are folded into `submit_grinder.rs` and `mining_loop.rs`. Lake-toolchain integration (`lake-canon`, `lake-target`, `lake-verify`) is feature-gated and OFF by default — without features, the loop falls back to `AcceptingVerifier` and the `boole.calibration.pow.v3` fixture-only emitter (drives the deterministic CLI smoke tests).

**Follow-ups:** live-model evidence for at least one driver (`claude_cli`, `ollama_local`, or `anthropic_api`) — a 5-cycle run against a local `boole node start` producing ≥1 accepted share would flip Gate P5 from PARTIAL to PASS.

### Slice S18 [C4] — Target emitter + local verify + prover driver + canonicalizer — **SHIPPED 2026-05-09**
**Scope:** the per-attempt collaborators consumed by S19's mining loop. After S18, an attempt can emit a target, generate a candidate via a prover, verify it locally, and canonicalize it before grinding.

- New `boole-miner::target_emitter` — `TargetEmitter` trait + deterministic `boole.calibration.pow.v3` emitter (behind feature `lake-target`) that produces `(target_emit, target_render)` pairs from `(seed_hex, attempt_index)`. Without the feature, only fixture-driven `--fixed-target-{seed-hex,render}` works.
- New `boole-miner::local_verify` — `Verifier` trait + `AcceptingVerifier` (default fallback when feature `lake-verify` is OFF, every candidate passes) + `LakeVerifier` (behind `lake-verify`, runs `lake exec boole_check` for cheap-reject before sharing).
- New `boole-miner::llm_driver` — `ProverDriver` trait + HTTP and process-spawn drivers + `MockProverDriver` for tests (honors `--mock-llm-response` for CLI smoke tests).
- New `boole-miner::canonicalizer` (`mod.rs`, `structural.rs`) — `Canonicalizer` trait + structural canonicalizer; lake-backed canonicalizer engaged behind feature `lake-canon`.

**Acceptance:** 6 lib tests covering emitter determinism, accepting verifier short-circuit, lake-verify feature gating, mock prover record/replay, retry-on-network-error semantics, and feature-flag absence. `cargo test -p boole-miner --test target_emitter --test local_verify` PASS plus per-module unit tests.

### Slice S17 [C4] — Chain-head fetch + share grinder + proof-package envelope assembly — **SHIPPED 2026-05-09**
**Scope:** the read side of the mining loop, the per-attempt share PoW grind, and the submission-envelope assembly point that S16's clients consume.

- New `boole-miner::chain_head` — `ChainHeadFetcher` trait + `HttpHead` blocking-HTTP impl for `GET /head`; honors `--head-timeout-ms` for fail-fast smoke testing.
- New `boole-miner::grinder::share` — finds a share that survives `share_hash < T_share`; per-attempt nonce iteration with `Hex32` byte representation (Boole consensus deviation from pof's 8/4/8-byte nonces).
- New `boole-miner::proof_package` — assembles the submission envelope (ticket + target + canonical proof + share + submission PoW) before signing.

**Acceptance:** lib tests cover `HttpHead` timeout semantics, share-grinder round-trip against a known target, and proof-package shape. `cargo test -p boole-miner` PASS.

### Slice S16 [C4] — Ticket + submit_pow grinders + submit/bounty HTTP clients + `boole mine bounty` driver — **SHIPPED 2026-05-09**
**Scope:** the write side of the mining loop and the bounty proof submitter.

- New `boole-miner::grinder` (`mod.rs`, `ticket.rs`, `submit_pow.rs`) — ticket grinder (find `n` with `ticket_hash < T_ticket`) + submission PoW grinder (find `nonce_s` with `submission_pow_hash(submission_envelope, nonce_s) < T_submit`).
- New `boole-miner::submit_client` — `Submitter` trait + `SubmitClient` blocking-HTTP impl posting `/submit` envelopes; `with_retry` adapter for transient network errors.
- New `boole-miner::bounty_client` — `BountyClient` blocking-HTTP impl posting `/bounties/:id/proof`; computes `proofHash = sha256(envelope_bytes)`, parses typed `BountyProofResult`.
- New `boole-miner::http_client` — shared blocking-`reqwest` transport (timeouts, retries, status mapping) consumed by submit / bounty / chain-head clients.
- New `boole-miner::cli::bounty` handler — reads envelope file (1 MiB cap), drives `BountyClient`, prints typed `BountyProofResult` JSON.

**Acceptance:** lib tests cover ticket-grind / submission-PoW round-trips, retry-on-network-error semantics, and bounty client envelope round-trip against a stub `Submitter`/HTTP server. `cargo test -p boole-miner` PASS.

### Slice S15 [C4] — Miner state + `boole mine init / address / config` CLI surface — **SHIPPED 2026-05-09**
**Scope:** the persistent state foundation for the miner crate. After S15, an operator can `boole mine init` to bootstrap a miner identity + dispatcher config, query it via `boole mine address` and `boole mine config get`, and rotate non-secret fields with `boole mine config set`.

- New `crates/boole-miner` workspace member — Rust library + thin binary at `src/bin/boole-miner.rs` wrapping `cli::run_mine`. No `tokio` dep (matches the rest of the workspace's sync HTTP idiom; uses `reqwest::blocking`).
- New `boole-miner::state` — `MinerState { schema:"boole.miner.state.v1", version:1, identity:{pk}, config:{dispatcher_url, llm:{backend, ...}, ...}, createdAt, updatedAt }`. Atomic tmp+rename writes at mode 0600 to `$BOOLE_MINER_STATE` (default `~/.boole/miner-state.json`). `iso_now()` uses Howard Hinnant civil_from_days inline (no chrono/time dep).
- New `boole-miner::cli` — internal subcommands `init` (refuses to overwrite without `--force`), `address` (prints `pk` hex), `config get` (secret redaction by default with `--reveal` flag), `config set` (key=value updates with secret-key dotted-path validation). Pluggable error helper `emit_typed_error(reason, exit_code, ...)` matching the `boole keys` exit-code vocabulary (2 = bad_request, 3 = state_already_exists / state_not_found).

**Acceptance:** lib tests cover state round-trip, atomic-rename durability, secret redaction, and key-already-exists error path. `cargo test -p boole-miner` PASS. 5 boole-cli integration tests in `mine_init_cli.rs` cover `mine init` / `mine address` / `mine config get` / `mine config set` / secret redaction round-trip.

### Slice S(B6) [B] — B6 timeout ergonomics — **SHIPPED 2026-05-09**
**Scope:** make benchmark timeout handling a usable operator surface so frontier-model runs don't silently miss valid attempts on cold-start latency, and so an operator can cap overall wall-clock cost cleanly.
- `--timeout-sec` default raised 300 → 600 (CLI + `run_benchmark` signature). `--timeout-sec 0` documented as the no-per-attempt-timeout opt-out (plumbing already routed `0 → None`; this slice pins the contract via test).
- New `--max-run-seconds <N>` cooperative wall-clock cap (default `0` = no cap). Validated `>= 0`. When set, `run_benchmark` snapshots `time.monotonic()` and propagates a `deadline_monotonic` float into `run_ollama_attempts`, `run_claude_cli_attempts`, and the spec-based row loop; each attempt iteration checks the deadline at the top and breaks before launching new work. In-flight attempts run to completion (cooperative — no SIGTERM mid-flight).
- On cap-trip: `summary["runTerminationReason"] = "max-run-seconds"` (omitted otherwise so byte-frozen v1/v2/v3 runs without the cap don't carry the field). Exit code 0 (operator-designed termination); `summary.ok` reflects per-row + safety gates as before.
- New `summary.latencyDistribution = {p50Ms, p90Ms, p99Ms, sampleCount}` computed across `skipped=false ∧ elapsedMs>0` rows via closed-form linear-interp (numpy type-7 contract). Leaderboard renders the three percentiles + the optional `runTerminationReason` line under the top-level summary block.

**Acceptance:** Gate B's B6 **CI** bullet PASS — new `B6TimeoutErgonomicsTests` × 7 in `scripts/test_model_benchmark.py` covers default-600 surfaced via `--help`, `--timeout-sec 0 → timeout_s=None` propagation, `--max-run-seconds` argparse non-negative validation (negative exits 2), cap-trip stops launching + writes partial summary with `runTerminationReason`, latency-distribution math (10 rows {100..1000} → p50=550 / p90=910 / p99=991), latency-distribution filter excluding `skipped=True` and `elapsedMs==0` rows, and leaderboard rendering of all three percentile lines. Total deterministic-CI count 50 → 57. CLI smoke `--max-run-seconds 1` against a 5-row × 0.6s mock spec ends with 2 rows + `runTerminationReason="max-run-seconds"` + exit 0. v1/v2/v3 family byte-frozen tests unchanged.

### Slice S(B2) [B] — `submit-lean --admission-nonce` override — **SHIPPED 2026-05-09**
**Scope:** for benchmarks run without a live node (CI, smoke), let the operator override the fixture's admission nonce on `boole-node submit-lean` so back-to-back runs produce diverse `share_hash` values without spinning up a node. Both pof and Boole `/ticket` are observe-only — miners must grind `n` themselves — so a flag-level override is the cleanest offline diversification path.
- New flag `--admission-nonce <64-hex>` on `boole-node submit-lean`. When set, overrides `ProofSubmissionTemplate.n`; when unset, behavior is byte-frozen (fixture `n`).
- Validation (`len == 64 && all ascii_hexdigit`) runs before fixture parse + Lean spawn; malformed value emits the typed envelope `{ok:false, command:"submit-lean", accepted:false, error:"malformed-admission-nonce", shareAccepted:false, blockProduced:false, invalidAccepted:0}` on stderr and exits 1 (matches existing rejected-input precedent: `missing_checker_artifact_policy`, Lean reject, admission-rejected all use exit 1).
- Reason code `malformed-admission-nonce` parallels S9's `malformed_pk` so cross-surface tooling can pattern-match on `error`/`reason` uniformly.

**Acceptance:** Gate B's B2 §3 bullet flipped to PASS — 3 new tests in `crates/boole-node/tests/submit_lean_admission_nonce.rs`: malformed-nonce typed-stderr-and-exit-1 (no lake/lean dependency), override changes `submissionBody.n` + `shareHash` between two runs (lake/lean-gated), default uses fixture `constants.n` byte-for-byte (lake/lean-gated). Full regression `cargo test -p boole-core -p boole-node -p boole-cli` clean, clippy clean.

### Slice S14 [N+C] — `POST /bounties/:id/status` operator status transition + audit ledger status_change events + `boole bounty status` — **SHIPPED 2026-05-09**
**Scope:** close out the bounty operator surface. After S14, operators can change a bounty's lifecycle status by signing a `boole.bounty.status.v1` payload with a v2 key, POSTing the resulting `boole.signed.v1` envelope to `/bounties/:id/status`, and the new status survives node restart via the `kind:"status_change"` extension to the existing audit ledger. No new envelope schema, no new ledger file: the existing `boole.signed.v1` envelope (S13a), `BountyRegistry::update_status` + `validate_status_transition` (pre-existing), and the existing NDJSON audit ledger (S12, status_change validator added in S12 + S13b's flat-field cross-checks) are the substrate.

- `boole-node::http_error` adds 3 typed reason constructors: `bad_status_value(value)` 400 (handler-side enum check, surfaces `newStatus` in the envelope), `bounty_id_mismatch(url_id, payload_id)` 400 (URL `:id` vs payload `id` cross-check, mirrors S13b's `workId/bounty.id` cross-check), `invalid_status_transition(detail)` 400 (fallback bucket for any future `update_status` rule that doesn't match the existing terminal-state pattern). All underscores, matching the `bad_proof_hash` precedent.
- `boole-node::local_node` adds `bounty_status_handler` (`POST /bounties/:id/status`) wired into `build_router` next to `/bounties/:id/proof`. 9-step validation chain: (1) outer schema `boole.signed.v1` else `bad_envelope`, (2) hex-shape on `pk` (hex32) and `signature` (hex64) → typed `bad_pk` / `bad_signature`, (3) `boole_core::verify_signature(pk, sig, payload)` — `Err(detail)` → 400 `bad_pk`, `Ok(false)` → 401 `signature_invalid`, `Ok(true)` proceeds, (4) inner schema `boole.bounty.status.v1` else `bad_payload`, (5) field validation (`id`, `newStatus`, optional `reason`, `ts`) into `UpdateStatusInput`, (6) URL `:id` vs payload `id` cross-check → 400 `bounty_id_mismatch` on disagree, (7) `newStatus` ∈ enum check → 400 `bad_status_value` else, (8) acquire registry write lock, load existing bounty for server-derived `prevStatus`, call `BountyRegistry::update_status` — `Err("cannot transition from terminal status ...")` → 409 `bounty_terminal`, `Err("unknown bounty id: ...")` → 404 `bounty_not_found`, other `Err` → 400 `invalid_status_transition`, (9) build the `kind:"status_change"` audit event `{schemaVersion:1, kind:"status_change", workId, problemHash, verifierKind, ts, prevStatus, newStatus, announcerPk:pk, reason?}` and `FileBountyEventLedger::append`; return 200 `{ok:true, bounty:<updated>}`.
- Boot replay (`replay_bounty_audit_event`) gains a `kind:"status_change"` arm via new `replay_status_change_event` that pulls `id`, `newStatus`, `ts` from the event and calls `BountyRegistry::update_status`. On `Err(...)` (overlap with the static catalog seeding the bounty in a different state, or any future stricter transition rule), the function emits a single-line stderr warning and continues — parallel to S13b's static-wins-on-overlap policy for create events. The order remains "static seeds first, audit deltas on top" (S13b).
- `boole-cli::main::BountyCommand::Status` clap variant exposes 7 args (`--id`, `--new-status` with `value_parser = ["open","solved","expired","withdrawn"]` for clap-level enum validation, `--reason`, `--ts`, `--signing-key`, `--node`, `--json`). Handler `bounty_status` 6-step flow: (1) resolve `--signing-key` against `BOOLE_KEYS_DIR` and refuse v1 envelopes with `legacy_v1_key` exit 3, (2) decode `sk` via `SigningKeyV2::from_seed_hex`, (3) build a `boole.bounty.status.v1` payload with default `ts = unix_ms_now()` (overridable for fixture reproducibility) and the optional `reason` only emitted when present, (4) sign + POST `/bounties/:id/status`, (5) on 2xx print bare `<newStatus>` (default) or full server envelope (`--json`) — parallels `bounty submit`'s bare-`<status>` ergonomics, (6) on non-2xx forward typed envelope to stderr with exit 1.
- Reuses `unix_ms_now()` (S13b), `validate_key_id` (S6), `read_json_arg` (S12 lifted), `keys_dir` / `key_path` / `KEYS_SCHEMA_V2` (S6 + S13a), and `http_post` (existing) — zero new helpers.

**Acceptance:** 10 new tests + 0 modified existing tests — 6 in `crates/boole-node/tests/bounty_status_route.rs` (valid signed envelope flips status + appends `kind:"status_change"` audit event with server-derived `prevStatus` + `announcerPk` + `verifierKind`, unknown `newStatus` → 400 `bad_status_value`, URL/payload id mismatch → 400 `bounty_id_mismatch`, terminal-state transition → 409 `bounty_terminal`, boot replay restores post-transition status, tampered payload → 401 `signature_invalid`); 4 in `crates/boole-cli/tests/bounty_status_cli.rs` (successful status change emits bare `<newStatus>` on stdout, `--json` returns full server envelope, v1 key refused with `legacy_v1_key` exit 3, server-side terminal transition forwards `bounty_terminal` to stderr exit 1). Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions (single-threaded run to dodge a pre-existing flaky port-collision in `account_balance_route`); clippy `--all-targets -- -D warnings` clean. Phase N5 PARTIAL PASS extended (operator status surface), Gate P4 bounty status-transition flow PASS, Phase C3 PASS extended (full bounty CLI surface complete: list / get / submit / announce / status).

**Deviations from plan §Appendix A · S14:** none. Scope decisions held: `boole.bounty.status.v1` payload schema, `boole.signed.v1` envelope reuse, no nonce/replay protection (idempotence enforced by `validate_status_transition`), no allowlist, single audit ledger reused, server-derived `prevStatus` (operators never supply it on the wire), reason vocab (`bad_status_value` / `bounty_terminal` / `bounty_id_mismatch` / `invalid_status_transition`), CLI thin-client default-bare-`<newStatus>` shape, operator-supplied `ts` with CLI default `unix_ms_now()`.

**Follow-ups:** runtime side-pool wiring for promoted bounties + `FamilyManifest` activation gating (Phase N5 long tail), `--allowlist <path>` for restricting status-change pks (parallel to S13b deferral), reward credit on accepted proofs (`accepted:true → RewardLedger::credit`), schema v3 keys with sealed-box `sk` encryption, RFC 8785 full canonicalization parity hardening.

### Slice S13b [N+C+core] — `POST /bounties` announce flow + audit ledger create events + `boole bounty announce` — **SHIPPED 2026-05-09**
**Scope:** close the bounty surface. After S13b, operators can publish a new bounty by signing a `boole.bounty.announce.v1` payload with a v2 key, POSTing the resulting `boole.signed.v1` envelope to `/bounties`, and the bounty appears on `GET /bounties` and survives node restart via the audit ledger. No new envelope schema and no new ledger file: the existing `boole.signed.v1` envelope (S13a) and the existing NDJSON audit ledger (S12) both extend to carry create events.

- `boole-core::bounty_ledger::validate_event` extended to accept `kind:"create"` events with the same `{schemaVersion, kind, workId, problemHash, verifierKind, ts}` flat-field requirements as proof events. When the optional `bounty` sub-object is present (S13b durable announce events embed the full `Bounty` so a restart can rebuild a dynamically-announced registry without an external catalog), the cross-checks `bounty.id == workId`, `bounty.problemHash == problemHash`, `bounty.verifier.kind == verifierKind` run — a divergence between flat index fields and the embedded record would let a replay restore a bounty under the wrong id and silently corrupt state. Legacy pof fixtures predate the embedded sub-object and carry only the flat fields; the sub-object is therefore optional, not required, so `fixtures/protocol/bounty-ledger/v1.json` parity holds.
- `boole-node::http_error` adds 4 typed reason constructors: `bad_envelope(detail)` 400, `signature_invalid()` 401, `bad_payload(field, detail)` 400, `bounty_already_exists(id)` 409. All underscores, matching the existing `bad_proof_hash` precedent.
- `boole-node::local_node` adds `bounty_announce_handler` (`POST /bounties`) wired into `build_router` next to the existing `bounty_list_handler` via `.route("/bounties", get(bounty_list_handler).post(bounty_announce_handler))`. 8-step validation chain: (1) outer schema `boole.signed.v1` else `bad_envelope`, (2) hex-shape on `pk` (hex32) and `signature` (hex64) → typed `bad_pk` / `bad_signature`, (3) `boole_core::verify_signature(pk, sig, payload)` — `Err(detail)` → 400 `bad_pk`, `Ok(false)` → 401 `signature_invalid`, `Ok(true)` proceeds, (4) inner schema `boole.bounty.announce.v1` else `bad_payload`, (5) field validation (`id`, `domain`, `problemHash` hex32, `verifier.{kind,metadata}`, `reward` u128 string, `deadline`, `ts`) into `CreateBountyInput`, (6) `BountyRegistry::create(input)` — duplicate id → 409 `bounty_already_exists`, validate-create errors → 400 typed, (7) build the `kind:"create"` audit event `{schemaVersion:1, kind:"create", workId, problemHash, verifierKind, ts, announcerPk:pk, bounty:<full Bounty>}` and `FileBountyEventLedger::append`, (8) return 200 `{ok:true, bounty:<created>}`.
- Boot replay (`replay_bounty_audit_event`) refactored to dispatch on event kind: `kind:"proof"` keeps the existing `BountyRegistry::submit_proof` path; `kind:"create"` calls `BountyRegistry::apply_event_fixture(json!({"kind":"create", "bounty":<bounty>}))`. On `Err("duplicates id...")` (because the static `bounties_path` catalog already loaded that id), `replay_create_event` emits a single-line stderr warning and continues — static catalog wins on overlap, audit log adds new ids on top. This matches the operator workflow where bounties are dynamically announced first and later promoted into the static catalog.
- `boole-cli::main::BountyCommand::Announce` clap variant exposes 11 args (`--id`, `--domain`, `--problem-hash`, `--verifier-kind`, `--verifier-metadata`, `--reward`, `--deadline`, `--ts`, `--signing-key`, `--node`, `--json`). Handler `bounty_announce` annotated `#[allow(clippy::too_many_arguments)]` (the surface is the surface). 8-step flow: (1) local hex32 lint on `--problem-hash` → exit 2 typed `malformed-problem-hash` envelope (no network round-trip, mirrors S9 `malformed_pk`), (2) `read_json_arg` on `--verifier-metadata` (inline JSON or file path, 1 MiB cap), (3) resolve `--signing-key` against `BOOLE_KEYS_DIR` and refuse v1 envelopes with `legacy_v1_key` exit 3, (4) decode `sk` via `SigningKeyV2::from_seed_hex`, (5) build payload with default `ts = unix_ms_now()` (overridable for fixture reproducibility), (6) sign + POST `/bounties`, (7) on 2xx print bare bounty id (default) or full server envelope (`--json`), (8) on non-2xx forward typed envelope to stderr with exit 1.
- New helper `unix_ms_now() -> u64` parallels the existing `now_iso8601_utc` so the announce path has a clean millisecond timestamp source without pulling another time crate.

**Acceptance:** 16 new tests + 0 modified existing tests — 2 in `crates/boole-core/tests/bounty_event_ledger.rs` (create event round-trip via `FileBountyEventLedger`, validation rejects mismatched `workId` vs `bounty.id`); 8 in `crates/boole-node/tests/bounty_announce_route.rs` (valid signed envelope creates bounty + appears in `GET /bounties`, audit log gains `kind:"create"` line with `announcerPk`, boot replay restores announced bounty, boot replay overlap with static catalog wins without panic, tampered payload → 401 `signature_invalid`, wrong outer schema → 400 `bad_envelope`, wrong inner payload schema → 400 `bad_payload`, duplicate id → 409 `bounty_already_exists`); 6 in `crates/boole-cli/tests/bounty_announce_cli.rs` (successful announce emits bare bounty id, `--json` returns full server envelope, v1 key refused with `legacy_v1_key` exit 3, malformed `--problem-hash` exits 2 with no network round-trip, server-side duplicate forwarded to stderr exit 1, `--verifier-metadata` file path). Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions; clippy `-p boole-core -p boole-node -p boole-cli --all-targets -- -D warnings` clean. The single `cargo test --workspace` failure (`canonical_checker_artifact_hash_matches_readme_pin` in `boole-lean-runner`) is a pre-existing drift between `lean/checker/lakefile.lean` (modified ahead of S13b to add a `lean_lib «Boole»` stanza) and `lean/checker/README.md`'s pinned hash — independent of S13b which touches no Lean code.

**Deviations from plan §Appendix A · S13b:** the audit-event `bounty` sub-object is OPTIONAL rather than REQUIRED on create events. Original plan §scope-decision-5 called for `bounty` to be required; preserving pof TypeScript golden fixture parity (`fixtures/protocol/bounty-ledger/v1.json` carries flat-fields-only create events) forced making it optional. Cross-check semantics still apply when present, so newly-written events carry the stronger guarantee while legacy fixtures replay byte-equal.

**Follow-ups:** runtime side-pool wiring for promoted bounties + `FamilyManifest` activation gating (Phase N5 long tail), ✅ `POST /bounties/:id/status` operator status transitions delivered in S14, `--allowlist <path>` for restricting announce pks, reward credit on accepted proofs (`accepted:true → RewardLedger::credit`), schema v3 keys with sealed-box `sk` encryption, RFC 8785 full canonicalization parity hardening.

### Slice S13a [C+core] — ed25519 signing primitives + keys schema v2 + `boole keys sign / verify` — **SHIPPED 2026-05-09**
**Scope:** ship the cryptographic toolkit S13b's announce flow needs as a self-contained slice. After S13a, `keys new` issues `boole.keys.v2` envelopes carrying the ed25519 secret seed `sk` alongside `pk`; `keys sign` produces `boole.signed.v1` envelopes with `signature = ed25519_sign(SHA-256(canonical_json(payload)))`; `keys verify` is a stateless verifier any party can run. v1 keys remain readable but cannot sign — there is no implicit upgrade because pk rotation would change the operator's identity.

- New `boole-core::canonical_json::canonicalize(&Value) -> Vec<u8>` — RFC 8785-lite (sorted object keys, preserved arrays, default number/string emit). Full RFC 8785 is deferred until on-chain commitment lands; the divergence is documented in the module so a future swap is localized.
- New `boole-core::signed_envelope` exposing `SigningKeyV2` (ed25519 keypair wrapper with `from_random` / `from_dev_id(id)` / `from_seed_hex(hex)` / `pk_hex` / `sk_seed_hex` / `sign(&Value) -> SignedEnvelope`), `SignedEnvelope { schema, payload, pk, signature }`, and the stateless `verify_signature(pk_hex, sig_hex, &Value) -> Result<bool, String>` primitive. Error split is deliberate: `Err(detail)` for wire-malformed pk/sig hex (4xx-class), `Ok(false)` for cryptographically wrong (200-class). `--dev` keys derive their seed from `blake3("boole-keys-dev::" || id)` — same domain prefix as S6's v1 dev path, so dev keys still don't collide with anything else.
- `boole-cli::keys_new` rewritten to generate ed25519 keys via `SigningKeyV2` and emit `{schema:"boole.keys.v2", id, pk:hex32, sk:hex32, createdAt}` envelopes. Atomic write 0600 unchanged. `keys list` / `keys show` continue to surface pre-S13a v1 envelopes byte-equal (one regression test pre-stages a v1 file by hand and asserts no `sk` synthesis).
- New `KeysCommand::Sign { id, payload, json }` → `keys_sign` handler: load the envelope, refuse v1 with `{ok:false, reason:"legacy_v1_key", id, schema, detail:"key was created before S13a..."}` exit 3, decode `sk` via `SigningKeyV2::from_seed_hex`, sign, print bare hex64 by default or `{ok:true, envelope:<boole.signed.v1>}` under `--json`. `--payload` accepts inline JSON or a file path via the lifted `read_json_arg` helper (renamed from S12's `read_envelope_arg`).
- New `KeysCommand::Verify { pk, signature, payload, json }` → `keys_verify` handler: shape-check `--pk` and `--signature` locally before any crypto call (typed `bad_pk` / `bad_signature` envelopes on stderr exit 2 if wrong), call `verify_signature`, print bare `valid`/`invalid` (both exit 0 because verification ran successfully) or `{ok:true, valid:true|false}` under `--json`.
- `boole-cli::Cargo.toml` drops the now-unused `blake3` and `hex` dependencies (the dev seed lives in `boole-core::SigningKeyV2::from_dev_id`, hex encoding/decoding is internal to the core module).

**Acceptance:** 18 new tests + 2 modified existing tests — 4 in `crates/boole-core/tests/canonical_json.rs` (key sort, recursive sort, array preservation, idempotence), 4 in `crates/boole-core/tests/signed_envelope.rs` (round-trip, wrong-pk Ok(false), tampered-payload Ok(false), malformed_pk Err), 5 in `crates/boole-cli/tests/keys_sign.rs` (inline payload + verify roundtrip, v1 key `legacy_v1_key` exit 3, unknown id `key_not_found` exit 3, file-path payload, `--json` full envelope), 5 in `crates/boole-cli/tests/keys_verify.rs` (valid stdout `valid` exit 0, tampered payload stdout `invalid` exit 0, wrong pk stdout `invalid` exit 0, malformed pk stderr `bad_pk` exit 2, malformed signature stderr `bad_signature` exit 2). Existing `keys.rs` v1-schema assertions flipped to v2 + a new `keys_list_includes_legacy_v1_envelope_unchanged` regression. Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions; clippy `--all-targets -- -D warnings` clean. Manual CLI smoke verified the full surface: `keys new --dev` → on-disk v2 envelope with hex64 `sk`, `keys sign` → bare hex64 + `--json` full `boole.signed.v1` envelope, `keys verify` → `valid` for genuine and `invalid` for tampered (both exit 0), `bad_pk` / `bad_signature` exit 2 for malformed hex, `legacy_v1_key` exit 3 for v1-on-disk. Phase C2 PASS extended.

**Deviations from plan §Appendix A · S13a:** none. Scope decisions held: `ed25519-dalek 2.1`, RFC 8785-lite canonical JSON, `boole.signed.v1` envelope shape, schema v2 keys with `sk = ed25519 seed`, `--dev` blake3-derived seed, no `--legacy-v1` flag, no `keys upgrade` command, error-split between wire-malformed (Err exit 2) and cryptographically wrong (Ok(false) exit 0).

**Follow-ups (S13b):** ✅ delivered — `POST /bounties` announce route consuming `boole.signed.v1` envelopes, audit-ledger create events parallel to S12's proof events (single ledger, schema extended), and `boole bounty announce` CLI driving the signed POST landed in S13b. `keys rotate / upgrade` (operator-confirmed pk rotation) and schema v3 (sealed-box `sk` encryption) remain on the long tail.

### Slice S12 [N+C] — Phase N5 bounty proof flow + Lean verifier wiring + `boole bounty submit` — **SHIPPED 2026-05-09**
**Scope:** ship the write side of bounty: `POST /bounties/:id/proof`, a `BountyProofVerifier` trait + verifier registry on `LocalNodeState`, the Lean adapter that wraps `LeanRunner` for real Lean verification, an NDJSON audit ledger that survives restart, and the matching `boole bounty submit` CLI. POST `/bounties` (announce) remains deferred to S13+ since it needs operator-key signing on top of S12's proof plumbing.

- `boole-core::BountyProofVerifier` trait (`fn verify(&self, bounty: &Bounty, envelope: &Value) -> Result<bool, String>`) + `FileBountyEventLedger` (NDJSON, schemaVersion-validating append+recover, mirror of `FileRewardLedger`).
- `boole-node::LeanBountyVerifier` adapter — envelope shape `{leanSource: "<utf8 .lean source>"}`. Writes the source to a per-call temp dir, calls `LeanRunner::check_file`, returns `Ok(result.accepted)`. Reads `verifierHash` from `bounty.verifier.metadata` and binds it into `LeanRunnerConfig::new(verifier_hash)` so checker-artifact identity matches the bounty record. Per-call temp dirs use `pid-nanos-AtomicU64` to avoid collisions under parallel test execution.
- `LocalNodeState` swap: `bounties: Vec<Bounty>` → `bounty_registry: Arc<RwLock<BountyRegistry>>`. Boot seeds the registry from the static catalog via `apply_event_fixture` create events, then replays the audit log (`BountyEventLedger.recover()`) to call `BountyRegistry::submit_proof` for every accepted proof so `solved` status survives restart. Verifier registry is `HashMap<String, Arc<dyn BountyProofVerifier>>`; production binary registers `lean → LeanBountyVerifier` when `--lean-checker-dir` (or `LEAN_CHECKER_DIR`) is set; tests use the new `LocalNodeConfig.bounty_verifiers: Option<HashMap<...>>` injection seam.
- `LocalNodeConfig` gains `bounty_event_ledger_path: Option<PathBuf>` + `bounty_verifiers: Option<HashMap<String, Arc<dyn BountyProofVerifier>>>`. Removed `#[derive(Debug, Clone)]` (trait object Arc has no Debug). 11 existing call-sites backfilled with `bounty_event_ledger_path: None, bounty_verifiers: None`.
- `boole-node run-local` adds `--bounty-events <path>` / `BOUNTY_EVENT_LEDGER_PATH` env and `--lean-checker-dir <path>` / `LEAN_CHECKER_DIR` env. Boot log lines `bountyEvents=<path|<none>>` and lean kind registration on stderr.
- `bounty_proof_handler` (`POST /bounties/:id/proof`) implements an 8-branch validation chain that matches pof byte-frozen: 404 `bounty_not_found` → 400 `bad_proof_hash` → 400 `bad_prover` → dedup peek (200 `duplicate:true, accepted:cached`, no verifier call, no ledger event) → 501 `no_verifier` → 409 `bounty_terminal` → verifier dispatch → `Ok(true)` flips status to `solved` and appends ledger event with `accepted:true, reward, credit:reward` → `Ok(false)` keeps status `open` and appends `accepted:false, credit:"0"` → `Err(msg)` returns 502 `verifier_error`. Audit event shape: `{schemaVersion:1, kind:"proof", workId, problemHash, verifierKind, ts, proofHash, solverPk, accepted, reward, credit}`.
- New `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32> --envelope <path|inline> [--node URL] [--json]`: default prints bare `<status>` word (`solved` on accept, `open` on reject, `duplicate` when the server returns `duplicate:true`); `--json` forwards the full server envelope; 4xx/5xx forward typed error to stderr + exit 1. `--envelope` accepts inline JSON or a file path (1 MiB cap).
- New fixture `fixtures/protocol/bounties/v1-mock.json` — 4 entries (gamma-1 mock-accept, delta-1 mock-reject, epsilon-1 withdrawn, zeta-1 unknown-kind) for the test-only verifier-injection seam. Existing `fixtures/protocol/bounties/v1.json` stays byte-frozen for S11 tests.

**Acceptance:** 20 new tests — 3 in `crates/boole-core/tests/bounty_event_ledger.rs` (append+recover round-trip, NDJSON line discipline, schema validation rejection); 8 in `crates/boole-node/tests/bounty_proof_route.rs` (accept flips status, reject keeps open, dedup returns cached without revisiting verifier, unknown bounty 404, bad proofHash 400, bad prover 400, terminal bounty 409, unknown kind 501); 2 in `crates/boole-node/tests/bounty_event_ledger_recovery.rs` (recovered event byte-equal to appended, second boot replays audit log to restore solved status); 2 in `crates/boole-node/tests/bounty_lean_verifier.rs` (lake/lean-gated; valid Lean source accepts, invalid Lean source rejects); 5 in `crates/boole-cli/tests/bounty_submit_cli.rs` (default-accept prints `solved`, default-reject prints `open`, default-dedup prints `duplicate`, `--json` envelope, unknown bounty stderr+exit 1). Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions; clippy `-- -D warnings` clean. Live lake/lean toolchain confirmed via `bounty_lean_verifier` (Lean 4.29.1, lake 5.0.0). Manual CLI smoke against a `boole-node run-local --bounties .../v1-mock.json --bounty-events .../events.ndjson` confirms typed envelopes for 404 (`bounty_not_found`), 400 (`bad_proof_hash`, `bad_prover`), 501 (`no_verifier` for `wholly-unknown-kind` and for `mock-accept` since the production binary doesn't register mock kinds). Phase N5 PARTIAL PASS extended (proof-write surface), Gate P4 bounty proof-write portion PARTIAL PASS, Phase C3 PARTIAL PASS extended.

### Slice S11 [N+C] — Phase N5 partial bounty read API + `boole bounty list/get` — **SHIPPED 2026-05-09**
**Scope:** ship the read-only bounty catalog over HTTP and the matching CLI surface. Mirrors S10's bundling of N4 (work read API) + C3-work (CLI). POST `/bounties` (announce — needs operator-key signing + bounty event ledger writes) and side-pool admission + `FamilyManifest` activation gating remain explicitly deferred to S12.

- `boole-core::BountyList { version: u32, bounties: Vec<Bounty> }` envelope + `load_bounties(path) -> anyhow::Result<Vec<Bounty>>` loader. Validates `version == 1`; future format bumps must rev the version explicitly. Storage type is `Vec<Bounty>` (not `BountyRegistry`) — read-only path stays free of registry mutation invariants; S12 will swap when announce events land.
- `LocalNodeConfig.bounties_path: Option<PathBuf>`. `boole-node run-local` accepts `--bounties <path>` / `BOUNTIES_PATH` env (precedence: flag > env > absent, identical to `--work-manifests`). When `None`, both routes still serve — `/bounties` returns `{ok:true, bounties:[]}` and every id is `bounty_not_found`.
- New axum routes: `GET /bounties` returns `{ok:true, bounties:[<Bounty>...]}` (200 always); `GET /bounties/:id` returns `{ok:true, bounty:<Bounty>}` on hit and 404 + `{ok:false, reason:"bounty_not_found", id}` on miss.
- New `boole bounty list [--node URL] [--json]`: default prints one line per bounty as `<id>\t<domain>\t<status>\t<reward>` (4-column tab-separated; one column more than `work list` since reward is bounty-specific); `--json` forwards the server envelope.
- New `boole bounty get --id <ID> [--node URL] [--json]`: default prints the bare `verifier.metadata.verifierHash` (parallels `work get` so the bare-output contract is uniform across read CLIs); `--json` forwards the envelope; non-2xx (e.g. 404 `bounty_not_found`) forwards the body to stderr with exit 1.
- Fixture `fixtures/protocol/bounties/v1.json` — 2 bounties: `alpha-1` (lean.protocol-invariant, status=open, reward=42, lean verifier) + `beta-1` (code.spec-template, status=solved, reward=11, wasm verifier).

**Acceptance:** 12 new tests — 3 in `crates/boole-core/tests/bounty_loader.rs` (loads v1 fixture, rejects bad version, accepts empty list), 4 in `crates/boole-node/tests/bounty_route.rs` (empty list when no path, list returns loaded bounties, by-id returns match, by-id unknown returns 404 typed), 5 in `crates/boole-cli/tests/bounty_cli.rs` (list default terse, list json envelope, get default verifier hash, get json envelope, get unknown forwards typed error exit 1). Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS (169 tests) with no regressions; clippy clean. Manual smoke against a live node on port 8901 verifies all six scenarios (`/bounties`, `/bounties/alpha-1`, `/bounties/no-such` → 404, `bounty list`, `bounty get` hit, `bounty get` miss → exit 1) end-to-end. Phase N5 PARTIAL PASS (read surface), Gate P4 bounty read portion PARTIAL PASS, Phase C3 PARTIAL PASS extended.

### Slice S10 [N+C] — Phase N4 work API + `boole work list/get` — **SHIPPED 2026-05-09**
**Scope:** ship the read-only work-manifest catalog over HTTP and the matching CLI surface so AFK agents and operators can enumerate the work types a node accepts. Closes the work portion of Gate P4 and the work portion of Phase C3 (bounty still blocked on N5). Mirrors S9's bundling of N3 (node) + C3-account (CLI) so end-to-end tests run against a real node.

- `boole-core::WorkManifestList { version: u32, work: Vec<WorkManifest> }` envelope + `load_work_manifests(path) -> anyhow::Result<Vec<WorkManifest>>` loader. Validates `version == 1`; future format bumps must rev the version explicitly.
- `LocalNodeConfig.work_manifests_path: Option<PathBuf>`. `boole-node run-local` accepts `--work-manifests <path>` / `WORK_MANIFESTS_PATH` env. When `None`, both routes still serve — `/work` returns `{ok:true, work:[]}` and every id is `work_not_found`.
- New axum routes: `GET /work` returns `{ok:true, work:[<WorkManifest>...]}` (200 always); `GET /work/:id` returns `{ok:true, work:<WorkManifest>}` on hit and 404 + `{ok:false, reason:"work_not_found", id}` on miss.
- New `boole work list [--node URL] [--json]`: default prints one line per manifest as `<workId>\t<familyId>\t<status>`; `--json` forwards the server envelope.
- New `boole work get --id <ID> [--node URL] [--json]`: default prints the bare `verifier.metadata.verifierHash` (the obvious useful field for downstream miners); `--json` forwards the envelope; non-2xx (e.g. 404 `work_not_found`) forwards the body to stderr with exit 1 (matches `block get` precedent).
- Fixture `fixtures/protocol/work/v1.json` — 2 manifests: bounty-derived (`workId == "lean-bounty-1"`, `source == "bounty"`) + direct (`workId == "smart-contract-invariant-v01-direct"`, `source == "direct"`).

**Acceptance:** 12 new tests — 3 in `crates/boole-core/tests/work_manifest_loader.rs` (loads v1 fixture, rejects bad version, accepts empty list), 4 in `crates/boole-node/tests/work_route.rs` (list returns loaded manifests, by-id returns match, by-id unknown returns 404 typed, routes empty when no manifests path), 5 in `crates/boole-cli/tests/work_cli.rs` (list default terse, list json envelope, get default verifier hash, get json envelope, get unknown forwards typed error exit 1). Full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions; clippy clean. Manual smoke against a live node verifies all four scenarios end-to-end. Gate P4 PARTIAL PASS (work portion), Phase C3 PARTIAL PASS extended.

### Slice S9 [N+C] — Reward ledger runtime integration + `boole account balance` — **SHIPPED 2026-05-09**
**Scope:** wire the existing `FileRewardLedger` (NDJSON, append-on-commit) into `RuntimeAdmissionState`, expose the read surface through the local node, and ship the matching CLI subcommand. This closes Gate P3 (reward ledger parity) and the account-portion of Phase C3.

- `RuntimeAdmissionState` gains `reward_ledger: Option<FileRewardLedger>` + `reward_ledger_path: Option<PathBuf>`. `boot_from_store(config, block_path, reward_ledger_path)` recovers from the ledger file when present, re-derives by re-issuing `compute_block_credits` over each replayed block when absent, and bails on the first divergent pk via `verify_ledger_matches_replay` (typed prefix `reward ledger divergence at pk …`). `commit_using_cache` appends the per-block `PersistedRewardEvent { height, c, credits }` between block append and `apply_block_unchecked`, so a crash in that window leaves the ledger one event behind the block log and recovery rolls the missing event forward.
- `LocalNodeConfig.reward_ledger_path: Option<PathBuf>`. `boole-node run-local` accepts `--reward-store <path>` / `REWARDLEDGER_PATH` env (parallel to `--block-store` / `BLOCKSTORE_PATH`) defaulting to `/tmp/boole-node-rewards.ndjson` so the route always works out of the box.
- New axum route `GET /account/{pk}/balance` returning `{ok:true, pk, balance:"<u128>", asOfHeight, asOfC}`. Well-formed-hex32 (`s.len() == 64 && all ascii_hexdigit`) is the boundary check; non-conformant `pk` → HTTP 400 `{ok:false, reason:"malformed_pk"}`. Unknown but well-formed `pk` → HTTP 200 `balance:"0"` (parity with pof contract). Empty chain (`ledger_head() == None`) falls back to `asOfHeight: 0` + scenario `genesis_c`.
- New `boole account balance --pk <hex32> [--node URL] [--json]`. Default prints the bare `balance` to stdout; `--json` prints the full server envelope. Local validation rejects malformed pk before the HTTP call (stderr `{ok:false, reason:"malformed_pk", pk}`, exit 2), matching the server-side reason verbatim so callers can pattern-match `reason` regardless of where the rejection lands.

**Acceptance:** 16 new tests across four files (`crates/boole-node/tests/{reward_store_divergence,runtime_reward_ledger,account_balance_route}.rs`, `crates/boole-cli/tests/account_balance_cli.rs`); full `cargo test -p boole-core -p boole-node -p boole-cli` PASS with no regressions; manual smoke against a node booted off `fixtures/protocol/replay/v1.json` returns `PK_2 → "3"`, unknown pk → `"0"`, malformed pk → exit 2 with typed envelope. Gate P3 PASS, Phase C3 partial PASS (account; work + bounty deferred to N4 / N5).

---

**Track-priority alignment check:**
- B-track priority order (B3 → B4 → B5 → B7(a) → B1+B7(b) → B6) maps to slices S2 → S4 → S5 → S4(folded) → S8 + S8b → S(B6).
- Node-track priority (N0 → CLI demo → N1 → later) maps to S1a → S1b → S3 → S7 → later.
- B and N tracks interleave; numerical slice order is recommended ship order, not a serial dependency chain.

---

## 11. Slice ship log

Per-slice change record — what landed, where, and what to expect downstream.

### Slice S1a [N] — `/ticket` payload contract — SHIPPED 2026-05-09

**Outcome:** N0 partial — `/ticket` contract is now frozen; `/health`, `/block/latest`, `/block/:height` remain for slice S1b.

**Files added:**
- `crates/boole-node/src/http_error.rs` — new `HttpError` type with constructors for `unexpected_field`, `missing_field`, `bad_hex`, `bad_request`, `body_too_large`, `not_found`, `internal_error`. Wire shape `{ok:false, reason:<kebab>, field?:<string>, detail?:<string>, ...extra}`. Includes `with_field` / `with_detail` / `with_extra` builders and an `into_json()` serializer. Self-tested via `#[cfg(test)] mod tests`.
- `crates/boole-node/tests/http_contract.rs` — fixture-driven golden test. Boots a single `serve_local_node` instance and asserts each ticket fixture's `(request, expect)` pair using one of three matchers per fixture: `bodyEquals` (exact), `bodyContains` (subset map), `bodyTypes` (`hex64`/`bool` checks).
- `fixtures/protocol/http-contract/v1/ticket-ok.json` — valid `{c, pk, n}` returns 200 with `{ok, valid, hashHex:hex64}`.
- `fixtures/protocol/http-contract/v1/ticket-unexpected-field.json` — body containing `j` returns 400 `{ok:false, reason:"unexpected_field", field:"j"}`.
- `fixtures/protocol/http-contract/v1/ticket-bad-hex.json` — non-hex `c` returns 400 `{ok:false, reason:"bad_hex", field:"c"}`.
- `fixtures/protocol/http-contract/v1/ticket-missing-field.json` — body missing `n` returns 400 `{ok:false, reason:"missing_field", field:"n"}`.

**Files modified:**
- `crates/boole-node/src/lib.rs` — `pub mod http_error;` added.
- `crates/boole-node/src/local_node.rs` —
  - Imports `crate::http_error::HttpError`.
  - `handle_connection` now dispatches via `Result<Value, HttpError>`. 4xx/5xx flow through a new `write_error_response` helper.
  - 413 body-too-large path migrated from `{error:"body_too_large"}` to `HttpError::body_too_large(limit, actual)` envelope (now also carries `limitBytes` and `actualBytes`).
  - 404 unknown-route path migrated from `{error:"not_found"}` to `HttpError::not_found(detail)` envelope (now carries `detail` with the offending `METHOD PATH`).
  - GET `/status` and `/head` and POST `/submit` errors are wrapped via `anyhow_to_internal` as 500 `internal_error` (previously they propagated through `?` and dropped the connection).
  - `ticket_json` rewritten with strict pof TicketBody (`{c, pk, n}`) validation:
    - `TICKET_BODY_FIELDS = ["c", "pk", "n"]` is the closed allowed set.
    - Loop over body keys → first key not in the allowed set → `HttpError::unexpected_field(key)`.
    - `required_string` now returns `Result<&str, HttpError>` and emits `missing_field` directly.
    - Each of `c`, `pk`, `n` is `Hex32::from_hex` validated → `HttpError::bad_hex(field)` on failure.
    - **`normalize_pow_fields` is no longer called at the `/ticket` boundary.** It remains in `submit_json` for the wider `/submit` body shape (and is no longer dead code there).
- `crates/boole-node/tests/local_node.rs` —
  - `local_node_serves_status_and_accepts_submit_into_replayable_block` — `/ticket` POST now sends `{c, pk, n}` only, projected from the fixture body (it previously sent the full submit-shaped body that the new contract rejects with 400 `unexpected_field`).
  - `local_node_rejects_oversized_http_body_before_json_parsing` — assertion migrated from `parsed["error"] == "body_too_large"` to `parsed["reason"] == "body_too_large"` and now also asserts the new `limitBytes` / `actualBytes` keys.
- `scripts/boole-model-benchmark.py` — `post_ticket_to_node` no longer wraps the body as `{"payload": submission_body}`; it now POSTs exactly `{"c": ..., "pk": ..., "n": ...}`. Added a defensive guard that returns `{reason:"missing_ticket_field", missing:[...]}` without an HTTP call when the canonical body is missing one of the three required fields. Both the wrapper drop and the projection are required by the new server contract — keeping the wrapper would 400 every live `--use-node-ticket` benchmark run.
- `scripts/test_model_benchmark.py` — `test_node_ticket_mode_requests_ticket_before_submit_and_records_evidence` updated to assert `set(ticket_body.keys()) == {"c", "pk", "n"}` (no payload wrapper, no extras) and reads `ticket_body[k]` directly instead of `requests[0]["body"]["payload"][k]`.

**Acceptance evidence:**
- `cargo test --workspace` — 121 tests pass, 0 failures (Doc-tests included).
- `python3 scripts/test_model_benchmark.py` — 18/18 OK.
- `cargo test -p boole-node --test http_contract` — 1/1 PASS (drives 4 fixture cases).

**Deviations from plan §Appendix A · S1a:**
1. **Added a fourth fixture** `ticket-missing-field.json`. The plan listed three (`ok`, `unexpected-field`, `bad-hex`). Adding `missing_field` was needed to fully exercise the symmetric pair with `unexpected_field` and to prove the full reason vocabulary the new envelope claims to support per §2 N0 task 5. No downstream change required.
2. **Migrated 413 and 404 paths to the new envelope.** The plan's S1a bullet only mandates the new envelope for `/ticket` errors, but §2 N0 task 5 says "all 4xx/5xx responses use `{ok:false, reason:<kebab>, ...}`". Since `http_error.rs` is the central error helper, the body-too-large and unknown-route paths were migrated in the same diff to avoid leaving the codebase with two competing envelopes. The existing `local_node_rejects_oversized_http_body_before_json_parsing` test was updated to reflect this (one-line assertion change plus two new assertions for `limitBytes`/`actualBytes`).
3. **Wrapped `/status`, `/head`, `/submit` runtime errors as 500 `internal_error`.** Previously these propagated through `?` and dropped the TCP connection without a response. The new behavior produces a typed envelope on the rare runtime-error path. No existing test asserts the old behavior; this is a strict UX improvement.

**Downstream unblocked:**
- **S1b** — `tests/http_contract.rs` and `fixtures/protocol/http-contract/v1/` infrastructure is in place; S1b only needs to add `health-*.json`, `block-latest-*.json`, `block-by-height-*.json` fixtures and one new test entry per fixture.
- **S3** — CLI demo slice can now consume the typed `{ok:false, reason, ...}` envelope directly; no more parsing surprises on `/ticket` errors.
- **Live `--use-node-ticket` benchmark runs** — the next opus/sonnet 50-attempt smoke run is now expected to keep passing, confirming the co-fix prevented a self-inflicted regression.

**Open follow-ups (not blocking S1a):**
- `submit_json` still uses `{ok:false, error:..., accepted:false}` for `canon_tag_out_of_range` and `ts_out_of_range` 200-level responses. These are 200-level by design (admission decision, not contract violation). Migrating them to `reason` would unify the field name across all "ok:false" responses but is out of scope for S1a.
- `scripts/local-node-smoke.sh` does not yet exist; planned for S1b along with the `/health` and `/block/{latest,:height}` routes.

### Slice S1b [N] — `/block/{latest,:height}` + `/health` + smoke — SHIPPED 2026-05-09

**Outcome:** N0 complete — every read route in §2 N0 (`/health`, `/block/latest`, `/block/:height`) now matches a frozen golden, and `scripts/local-node-smoke.sh` exercises the entire N0 surface end-to-end via the actual `boole-node run-local` binary. **Gate P0 PASS in full.**

**Files added:**
- `fixtures/protocol/http-contract/v1/health-ok.json` — 200 with `{ok, status:"ok"}`, `sharePoolSize:uint`, `provenance:string`. Mirrors pof `dispatcher/src/httpServer.ts:269-277` minus the optional SLA snapshot (boole-node has no `SlaMetrics` provider yet).
- `fixtures/protocol/http-contract/v1/block-latest-empty.json` — empty chain returns 200 with `block:null`, `height:null`, `c:GENESIS_C`. **Intentional pof divergence** (pof returns 404 `no_blocks`); rationale lives in the fixture's `description` and §2 N0 task 3 — clients always receive a chain-head reference so `/ticket` and `/submit` against genesis work without a separate bootstrap call.
- `fixtures/protocol/http-contract/v1/block-latest-ok.json` — non-empty chain returns 200; structural-only (`block:object`, `height:uint`, `c:hex64`) since the committed block contents are dynamic.
- `fixtures/protocol/http-contract/v1/block-by-height-ok.json` — `/block/0` after one commit returns 200 with `{ok:true, height:0, block:object, c:hex64}`.
- `fixtures/protocol/http-contract/v1/block-by-height-bad-request.json` — `/block/notanumber` returns 400 `{ok:false, reason:"bad_request", detail:"height must be a non-negative integer"}`. Distinguishes parse failure (path malformed) from lookup failure (path well-formed, height beyond chain).
- `fixtures/protocol/http-contract/v1/block-by-height-not-found.json` — `/block/9999` returns 404 `{ok:false, reason:"not_found", detail:"no block at height 9999"}`. Detail echoes the queried height for diagnostics.

**Files modified:**
- `crates/boole-node/src/local_node.rs` —
  - Added route arms in `handle_connection`: `("GET", "/health")`, `("GET", "/block/latest")`, and a guarded `("GET", path) if path.starts_with("/block/")` that strips the prefix and dispatches to `block_by_height_json`. Order matters: `/block/latest` is matched before the prefix guard so it does not fall into the height parser.
  - New `health_json(state)` — returns `{ok:true, status:"ok", sharePoolSize:<runtime.pool_size()>, provenance:<report.provenance>}`.
  - New `block_latest_json(state)` — reads `runtime.cached_blocks()` (authoritative after boot per the existing `replay_matches_runtime_at_boot` invariant). On non-empty: `{ok:true, block, height:<len-1>, c:<block.c>}`. On empty: `{ok:true, block:null, height:null, c:<state.genesis_c>}` (the pof divergence above).
  - New `block_by_height_json(state, raw)` — `raw.parse::<usize>()` failure → `HttpError::bad_request("height must be a non-negative integer")` (covers both negative and non-numeric). `cached_blocks().get(height)` `None` → `HttpError::not_found(format!("no block at height {height}"))`. Hit → `{ok:true, block, height, c:<block.c>}`.
- `crates/boole-node/tests/http_contract.rs` —
  - `dispatch_fixture_request` extended to support `GET` (renamed from POST-only). GET writes `GET <path> HTTP/1.1\r\nHost: localhost\r\n\r\n` directly via raw TCP.
  - `bodyTypes` matcher vocabulary extended with `uint`, `string`, `object`. Existing `hex64`, `bool` types kept.
  - New helper `submit_first_runtime_smoke_step(addr)` — reads `fixtures/protocol/runtime-smoke/v1.json` step 0, POSTs `{body, canonTag:0}` to `/submit`, and asserts `accepted:true`. Drives the post-commit fixtures.
  - Two new test functions:
    - `read_routes_on_empty_chain_match_fixtures` — boots a fresh server, asserts `health-ok`, `block-latest-empty`, `block-by-height-bad-request`, `block-by-height-not-found`. `max_requests=4`.
    - `block_read_routes_after_first_block_match_fixtures` — boots a fresh server, calls `submit_first_runtime_smoke_step`, then asserts `block-latest-ok` and `block-by-height-ok`. `max_requests=3`.
  - Existing `ticket_contract_fixtures_match` (S1a) untouched and still passes.
- `scripts/local-node-smoke.sh` — extended end-to-end coverage:
  - `request()` now accepts `expect_status` (default `200`); raises `SystemExit` on mismatch. Lets the smoke probe error envelopes too.
  - New probes added between the existing `/status` and `/submit` calls: `GET /health`, `GET /block/latest` (asserts empty-chain envelope with `block:null`/`height:null`/`c:0×64`), `GET /block/notanumber` (asserts 400 `bad_request`), `GET /block/9999` (asserts 404 `not_found`).
  - New probes added after `/submit`: `GET /block/latest` (asserts `height:0` and `c == submit.block.c`), `GET /block/0` (asserts the same).
  - Final stdout JSON expanded to mirror every probe so the smoke is greppable for regressions.
  - `--max-requests` raised from `3` to `9` to match exactly the number of HTTP calls the script makes — sized so the server exits the moment the last probe lands, and the bash `wait $PID` returns cleanly. Mismatched values would either truncate the smoke (too low) or hang the script (too high).

**Acceptance evidence:**
- `cargo test -p boole-node --test http_contract` — 3/3 PASS (`ticket_contract_fixtures_match`, `read_routes_on_empty_chain_match_fixtures`, `block_read_routes_after_first_block_match_fixtures`).
- `cargo test --workspace` — 0 failures across the workspace.
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` — 18/18 PASS (S1a regressions still green).
- `bash scripts/local-node-smoke.sh` — `local-node-smoke: PASS`. Final JSON line confirms each probe's outcome (status height=0 → submit.block.c=`04451d…dd8d` → latest.height=0 → byZero.height=0 → head.height=1).

**Deviations from plan §Appendix A · S1b:**
1. **Empty `/block/latest` returns 200 with a sentinel, not 404.** The plan's S1b bullet says "goldens for the three new routes (`not_found` for missing height)" without specifying empty-chain `/block/latest`. The implementation matches §2 N0 task 3 (intentional pof divergence) rather than mirroring pof's 404 `no_blocks`. Documented in the `block-latest-empty.json` fixture description so the divergence is discoverable at the contract level, not buried in code.
2. **`/block/:height` parses with `usize` and rejects negatives via the parser, not a separate range check.** This makes "non-negative integer" the contract phrasing in the `bad_request` detail string (echoed verbatim in the fixture) — both negative numbers and non-numeric paths flow through the same error path with the same detail.

**Downstream unblocked:**
- **Gate P0 (full N0 contract surface frozen via raw TCP)** — now PASS. Subsequent contract changes must update fixtures rather than just code.
- **S3 (`boole node start` + `boole block latest/get`)** — every block read endpoint the CLI consumes now exists with a frozen golden. The CLI can rely on the empty-chain sentinel to render a useful "no blocks yet" UX without a special-case 404 handler.
- **S7 (CLI HTTP transport diff)** — N0 contract is fully exercised by both the in-process golden tests and the binary smoke; ready to be exercised by a CLI consumer in a future slice without another contract churn.

**Open follow-ups (not blocking S1b):**
- **`/block/:height` with leading zeros** (e.g. `/block/00`) currently parses as `0` (Rust `usize::from_str` accepts leading zeros). pof's `parseInt(..., 10)` would too, so this matches pof — but it's not asserted by a fixture. Add a fixture in a later slice if we want the canonicalization to be a contract guarantee rather than an implementation accident.
- **`provenance: string` is currently always `"calibration-final"`** from the calibration report. If runtime overrides become a thing in N1, the smoke and fixture should switch from a type assertion to an explicit value or a `bodyContains` entry.
- **`sharePoolSize` is asserted as `uint` only.** A future slice that exercises `/health` after submitting shares could tighten this to `bodyContains: {sharePoolSize: 0}` post-commit (since commit drains the pool) — currently only the structural shape is guarded.

### Slice S2 [B] — B3 CoT/Thinking stripping — SHIPPED 2026-05-09

**Outcome:** B-track priority-1 hardening lands. The benchmark extractor now strips `<think>…</think>` blocks before any forbidden-token check, lifts the existing `last_proof_term_line` walker from the sorry-fallback into the primary extraction path for all providers, and validates `\b(sorry|admit)\b` on the FINAL candidate only. Gemma-style CoT outputs that previously tripped `candidate-forbidden-token` via prompt-recall ("do not use sorry or admit" inside `<think>`) now reach the Lean verifier with the actual answer.

**Files modified:**
- `scripts/boole-model-benchmark.py` —
  - Added `THINK_BLOCK_RE = re.compile(r"<think\b[^>]*>[\s\S]*?</think>", re.IGNORECASE)`. The `<think\b[^>]*>` opening allows attribute-bearing variants (e.g. `<think id="...">`) without losing the closing-tag anchor.
  - `normalize_model_output` now runs `THINK_BLOCK_RE.subn(...)` after ANSI stripping, before the trailing `.strip()`. When at least one block is removed, `"strip-think"` is appended to the returned `normalizations` list — providers, downstream metrics and tests can detect the path.
  - `extract_proof_term_candidate` rewired:
    - Removed the early forbidden-token branch that ran on `raw` BEFORE candidate extraction. The single `\b(sorry|admit)\b` check now runs **after** the optional `last_proof_term_line` lift, on the final candidate only.
    - Lifted `last_proof_term_line` invocation from "fallback after sorry/admit" to "always-on multi-line primary extraction": when `"\n" in raw` and the walker returns a strict subset (`final_line != raw`), the candidate becomes that line. The `final_line != raw` guard avoids a no-op format-label change when the walker happens to return the same string.
    - Renamed the format label from `ollama-final-line` → `final-proof-line`. The label is now provider-agnostic — the walker is no longer ollama-specific in either invocation site or naming.
- `scripts/test_model_benchmark.py` —
  - Renamed `test_extractor_handles_ollama_thinking_prompt_echo_and_final_proof_line` → `test_extractor_handles_thinking_prompt_echo_and_final_proof_line` (provider-neutral). Assertion updated from `format == "ollama-final-line"` to `format == "final-proof-line"`, and an extra assertion that `"last-proof-line"` appears in `normalization` was added (previously only `strip-ansi` was asserted).
  - Added `test_extractor_strips_think_blocks_then_extracts_final_proof_line` — Gemma-style multi-line `<think>…</think>` block containing prompt-recall ("do not use sorry or admit") followed by a clean `rfl`. Asserts (a) reach-verifier (no rejection reason), (b) candidate == `"rfl"`, (c) `strip-think` in normalization, (d) format ≠ `ollama-final-line` (provider-agnostic).
  - Added `test_extractor_does_not_reject_when_forbidden_token_only_in_think_block` — `<think>do not use sorry; do not use admit</think>\nEq.refl 1` reaches the verifier as `Eq.refl 1`. This is the symmetric "final-candidate-only" case: prompt-recall lives entirely inside `<think>` and the final candidate is clean.
  - Added `test_extractor_still_rejects_when_final_candidate_contains_forbidden_token` — `<think>thinking...</think>\nsorry` still rejects with `candidate-forbidden-token`, locking the regression that the reorder must not turn into a wholesale sorry-acceptance.

**Acceptance evidence:**
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 21/21 PASS (4 new/updated extractor tests + 17 prior).
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy -k extractor -v` (Red→Green proof) → 4/4 PASS, all 4 fail before the implementation diff was applied.
- `cargo test --workspace` → 0 failures (S1a/S1b regressions still green; S2 is a Python-only diff).
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (N0 contract surface unchanged).

**Deviations from plan §Appendix A · S2:**
1. **Test corpus is synthetic, not a recorded Gemma 26b CoT.** The plan says "a recorded Gemma CoT that previously tripped `candidate-forbidden-token`". The committed regression tests use synthetic `<think>…</think>` inputs that are minimum reproducers of the described failure modes. A literal recorded transcript would (a) require an ollama-gemma run, which is part of Gate B-Live, and (b) bloat the deterministic CI test corpus. The deterministic minimum reproducers cover the same failure modes; the live ≥80% bar lives under Gate B-Live and is covered by the ollama-gemma smoke (`scripts/boole-miner-ollama-gemma-smoke.sh`) that this slice unblocks.
2. **`final_line != raw` guard added.** The plan says "lift `last_proof_term_line` to primary path" without specifying when the lift takes effect. Without the guard, a multi-line raw whose `last_proof_term_line` walker returns the same string would still get the format relabeled to `final-proof-line` and the normalization tagged with `last-proof-line`, even though no extraction happened. The guard restricts the label change to actual lifts, keeping the format labels truthful.
3. **Format label renamed `ollama-final-line` → `final-proof-line`** is a wire-shape change in the benchmark row schema. Any downstream analytics that grouped by format label will need to learn the new value. Since the only known consumer is `scripts/test_model_benchmark.py` (which was migrated in the same diff), this is safe to land in S2 rather than gated behind a separate rename slice.

**Downstream unblocked:**
- **B-Live Gemma 26b ≥80% verifier.invoked** can now be exercised by `scripts/boole-miner-ollama-gemma-smoke.sh` against an actual ollama daemon. Live runs are out of scope for deterministic CI but the extractor's contract is now the right shape to support the bar.
- **Future provider integrations** (claude, sonnet, hermes) that emit `<think>` CoT will share the same extraction path. No more provider-specific naming or fallback branches to add.
- **B4** (replayInvoked) and **B7(a)** (v1 family docs) — S4 — can now build on a stable extractor. Any change to the extraction contract from here onward must update fixtures rather than just code.

**Open follow-ups (not blocking S2):**
- **Inline `<think>…</think>` immediately followed by an answer on the same line** (e.g. `<think>...</think> rfl`) currently relies on `THINK_BLOCK_RE` removing the block but leaving any trailing space. Trim handles it for normal whitespace but not for tab/newline mixes. Add a fixture in a later slice if production traces show the inline pattern is common.
- **Self-closing `<think/>`** is not stripped. The regex requires a closing `</think>`. If any provider emits `<think/>`, extend the regex; not observed in current traces.
- **B-Live ≥80% bar measurement** is not enforced by deterministic CI. A follow-up slice could add a recorded-trace replay harness (record one ollama-gemma run, replay against the extractor, assert the ratio) so the bar becomes deterministic.

### Slice S3 [C] — `boole node start` + `boole block latest/get` — SHIPPED 2026-05-09

**Outcome:** Gate P6 C0+C1 PASS. The Rust CLI now drives the local node end-to-end: `boole node start` spawns boole-node with the right block-store path and an optional genesis override, while `boole block latest --node URL --json` and `boole block get --height N --node URL --json` print the typed envelope on stdout and forward the server's `{ok:false, reason:<kebab>, ...}` envelope on stderr (non-zero exit). `examples/boole-block-demo.sh` runs the full demo loop (empty-chain latest → submit → latest after → get-by-height) and prints a single-line JSON suitable for the frontpage card.

**Files modified:**
- `crates/boole-node/src/local_node.rs` —
  - Added `LocalNodeConfig.genesis_override: Option<String>`. When set, `LocalNodeState::from_config` rewrites `scenario.genesis_c` BEFORE the runtime adopts the head, so `replay_matches_runtime_at_boot` continues to match. The field's doc-comment documents that ordering invariant.
- `crates/boole-node/src/main.rs` —
  - `run_local_command` grew env-var fan-out to mirror pof `booleCli.ts:116-134`: `PORT`, `BLOCKSTORE_PATH`, `GENESIS_C`. Flags win over env vars (so explicit invocations stay debuggable), env vars win over defaults. New flags: `--port` (composes with the existing `--addr`), `--genesis`. Flag/env precedence is encoded as `flag.or(env).unwrap_or(default)` in three places (port→addr, block-store, genesis-override) so the precedence rule is obvious from the call sites.
  - `print_help` updated with the new flags and a new "environment" block listing the three env vars and their flag overrides.
- `crates/boole-node/tests/local_node.rs`, `crates/boole-node/tests/http_contract.rs` — propagated `genesis_override: None` to all three `LocalNodeConfig { ... }` literal sites that pre-existed S3. Construction-only diff.
- `crates/boole-cli/Cargo.toml` — added `boole-node = { path = "../boole-node" }` as a `[dev-dependencies]` entry. The CLI does not link boole-node at runtime; the dev-dep is purely so the integration tests can boot a server in-process via `serve_local_node` instead of having to spawn another subprocess for every block-query test.
- `crates/boole-cli/src/main.rs` —
  - New `Command::Node { command: NodeCommand::Start { port, data_dir, scenario, genesis, max_requests } }` and `Command::Block { command: BlockCommand::{Latest, Get} }` clap subcommands.
  - `node_start` resolves the boole-node binary via `BOOLE_NODE_BIN` env (tests) → sibling-of-current-exe (cargo dev profile) → bare `boole-node` (PATH). `--data-dir` becomes `<dir>/blocks.ndjson` for the block store path; `--max-requests` is passed through verbatim for smoke/test caps.
  - `block_latest` / `block_get` use a small inline raw-TCP HTTP/1.1 GET client (`http_get` + `parse_http_response` + `find_header_end`). 2xx body lands on stdout, anything else lands on stderr unchanged so the CLI surface speaks the same JSON dialect as a direct curl. The 4xx/5xx branch calls `std::process::exit(1)` so callers see a non-zero exit alongside the typed envelope.
  - Top-level error handler in `main` switched from `{ok:false, error:"runtime", message}` to `{ok:false, reason:"internal_error", detail}` so anyhow-bearing failures speak the same kebab vocabulary as the HTTP boundary.
- `crates/boole-cli/tests/replay_cli.rs` — assertion for the runtime-error-on-stderr test updated from `error == "runtime"` to `reason == "internal_error"` to match the new envelope shape. Inline comment captures the rationale (CLI and node now speak the same dialect).
- `crates/boole-cli/tests/node_block.rs` (NEW) — 6 integration tests:
  1. `block_latest_against_empty_chain_prints_genesis_envelope_to_stdout` — empty-chain `block latest` returns `{ok:true, block:null, height:null, c:"0"*64}` on stdout, stderr empty.
  2. `block_latest_after_one_commit_prints_block_envelope_to_stdout` — boots in-process server with `max_requests=2`, runs `submit_step_zero(addr)` against `fixtures/protocol/runtime-smoke/v1.json` step 0, asserts `block_latest` returns `{ok:true, height:0, block:{...}, c:<64hex>}`.
  3. `block_get_height_zero_after_commit_prints_block_envelope_to_stdout` — same setup, `block get --height 0` returns the same envelope.
  4. `block_get_height_bad_request_prints_typed_error_to_stderr` — `block get --height notanumber` exits non-zero, stdout empty, stderr is `{ok:false, reason:"bad_request", ...}`.
  5. `block_get_height_not_found_prints_typed_error_to_stderr` — `block get --height 9999` against an empty chain exits non-zero, stderr `{ok:false, reason:"not_found", ...}`.
  6. `node_start_spawns_daemon_serving_health` — runs the `boole node start` subcommand with `BOOLE_NODE_BIN` pointing at the boole-node test binary, polls TCP until the listener is up, then sends a raw `GET /health` and asserts `{ok:true, status:"ok", ...}`. Cap at `--max-requests 2` because the readiness probe consumes one slot before the actual GET (documented inline).
- `examples/boole-block-demo.sh` (NEW) — end-to-end demo that builds both binaries, spawns `boole node start` with `--max-requests 4` (latest-empty, submit, latest-after, get-zero), polls readiness via the typed `boole block latest` call (so probe doubles as the first served slot), submits scenario step 0 via raw `http.client`, then runs `boole block latest` and `boole block get --height 0` and emits a single-line JSON envelope: `{ok, demo, node, emptyLatest, submitted, latestAfter, byHeightZero}`. The block `c` value is identical across `submitted`, `latestAfter`, and `byHeightZero` — proves runtime/cache/disk all agree.

**Acceptance evidence:**
- `cargo test --workspace` → 0 failures (the 6 new tests + the rewritten replay_cli error envelope test all pass alongside the prior workspace suite).
- `cargo test -p boole-cli --test node_block` → 6/6 PASS.
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (S1a/S1b/S2 contract surface still green).
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 21/21 PASS (S2 still green).
- `bash examples/boole-block-demo.sh` → `boole-block-demo: PASS`. Sample stdout: `{"ok":true,"demo":"boole-block","node":"http://127.0.0.1:18091","emptyLatest":{"height":null,"c":"0000…"},"submitted":{"accepted":true,"height":0,"c":"04451d16…"},"latestAfter":{"height":0,"c":"04451d16…"},"byHeightZero":{"height":0,"c":"04451d16…"}}`.

**Deviations from plan §Appendix A · S3:**
1. **`REWARDLEDGER_PATH` env var is documented in the plan but not wired in S3.** The pof booleCli wrapper sets four env vars (`PORT`, `BLOCKSTORE_PATH`, `REWARDLEDGER_PATH`, `GENESIS_C`); only the first three are plumbed through here. The reward ledger runtime path is Phase N3 work — there's no reward-ledger reader on the boole-node side yet, so accepting the env var would be wiring a no-op. Captured as a follow-up: when N3 lands, fan it through `run_local_command` in the same shape as the other three (`flag.or(env).unwrap_or(default)`).
2. **`boole node start` blocks until boole-node exits, instead of fork-and-detach.** The pof CLI returns immediately after spawning the daemon under `tsx`. The Rust version uses `Command::status()` so the test harness can `wait()` and the demo can wait for `--max-requests N` to expire deterministically. Production users who want a long-running daemon use the shell `&` operator. Documented inline in `examples/boole-block-demo.sh` and re-validated by `node_start_spawns_daemon_serving_health` which exercises the spawn-and-wait shape end-to-end.
3. **`max-requests 2` cap (not 1) in `node_start_spawns_daemon_serving_health`.** The plan didn't specify the cap. The readiness loop uses `TcpStream::connect_timeout` followed by an immediate drop (no header), which the server reads as a 0-byte request and counts as one served slot before failing it as `bad HTTP request: missing header terminator`. With `max_requests=1` the server exits before the actual GET arrives. Inline comment in the test documents the contract; a future slice can replace the probe with retrying-the-real-GET if we want a tighter `max_requests=1` invariant.
4. **CLI does not re-wrap the server's typed envelope on error.** The plan says "typed errors on stderr". The implementation forwards the server's body verbatim rather than re-parsing and re-emitting. This means the CLI surface for an HTTP error and the curl surface for the same request are byte-identical, which simplifies test assertions and downstream agents. If we ever want to tag a CLI-side reason (e.g. `connection_refused` when the node is down), that lands in a follow-up — currently a connection failure surfaces as the catch-all `internal_error` from the top-level error handler.
5. **`replay_cli.rs` legacy error shape rewritten in-slice.** The S3 scope on paper is "add new subcommands"; in practice the old `chain replay` error shape (`error: "runtime"`) was inconsistent with the new typed envelope and would have needed migrating later regardless. Rewriting it in this slice means the entire CLI speaks one error dialect from S3 onward — better than leaving a one-route exception that paper-cuts every CLI consumer.

**Downstream unblocked:**
- **C2 (`boole keys new/list/show`)** — Slice S6 — can now follow the same clap subcommand pattern (`Command::Keys { command: KeysCommand::... }`) and the same typed-envelope-on-stderr contract.
- **C3 / C4 (account, work, bounty queries; mining)** — once their server-side routes land in N3-N5, the CLI helpers `http_get_print` and `node_start` are the obvious building blocks: GET routes can call `http_get_print`, mining can spawn a long-running boole-node via the same path and then issue requests against it.
- **Frontpage / VC card** — the demo's single-line JSON is now a stable artifact that can be regenerated by anyone with a checkout. The `submitted.c == latestAfter.c == byHeightZero.c` invariant is the visual proof that runtime, cache, and disk are coherent.

**Open follow-ups (not blocking S3):**
- **`REWARDLEDGER_PATH` env-var pass-through** — wire it in when Phase N3 lands the reward-ledger reader on boole-node.
- **`fork-and-detach` mode for `boole node start`** — current shape is `spawn-and-wait`. A `--detach` flag (or a separate `boole node spawn` verb) could fork the daemon and return immediately, emitting the PID on stdout. Useful for shell pipelines that want to chain `boole node start --detach` with the next command without a `&`.
- **Connection-failure typed envelope** — when the node is unreachable, the CLI currently emits `{ok:false, reason:"internal_error", detail:"…connection refused…"}`. A `connection_refused` reason would be more agent-friendly. Add to the `reasons` vocabulary in a future slice when more CLI-side error categories surface.
- **`boole block latest --node URL` without `--json`** — currently `--json` is accepted but the implementation always emits JSON. Plain-text output for human consumers is a future-S6 concern; the JSON shape is the agent contract and that's what the slice ships.

### Slice S4 [B] — B4 (`replayInvoked`) + B7(a) (v1 family docs) — SHIPPED 2026-05-09

**Outcome:** Gate B-CI's B4 and B7(a) bullets PASS. The benchmark summary can no longer claim a vacuous `replayPassed: true` when zero rows reached the verifier — that case now reports `null` (rendered as `—` on the leaderboard) and per-row `replayInvoked` records exactly which rows the verifier was actually run against. The authoritative target-family reference doc (`docs/benchmark-target-families.md`) ships with `boole.calibration.pow.v1` and `boole.smoke.true.v1` sections, and a CI lint asserts every `targetFamily` literal in `scripts/boole-model-benchmark.py` has a matching `## <family>` section in the doc — drifting one without the other now fails CI.

**Files modified:**
- `scripts/boole-model-benchmark.py` —
  - New helper `replay_invoked_from_result(parsed)` that returns `True` iff the verifier-side parsed result carries the `replayMatchesRuntime` key (under top-level, `status`, or `safety`). Presence-of-key is the signal, not value: `replayMatchesRuntime: False` still counts as "invoked" (the verifier ran and returned a verdict), while a row whose parsed result lacks the key counts as "never invoked" (rejected before extraction, timed out, skipped). Doc-comment captures the contract.
  - `score_from_result` now emits `"replayInvoked": replay_invoked_from_result(parsed)` alongside the existing `blocksProduced` / `replayPass` keys for every verified-row score.
  - `zero_score(*, replay_pass=True, replay_invoked=False)` gained the `replay_invoked` keyword (default `False`) so all rejected/skipped/timeout call sites correctly flag rows that never reached the verifier without per-call-site book-keeping.
  - The inline verified-row score path (immediately after `submit-lean` accepts a candidate) emits `"replayInvoked": True` directly, since the verifier was just invoked successfully.
  - `summarize` replaced the legacy unconditional `all(replayPass is True)` with a presence-aware computation: filter `active_rows` by `replayInvoked is True`, count, then `replayPassed = None` iff the count is 0 else `all(replayPass is True for invoked rows)`. The summary also gained `"replayInvoked": <count>` under `attemptHierarchy` so dashboards can show the verified-row denominator without re-iterating rows.
  - `render_leaderboard` switched the `replayPassed` line from `str(value).lower()` to `"—" if value is None else str(value).lower()`, mirroring the JSON `null` semantics in the human-facing markdown.
  - `replay_report.rows` now passes `replayInvoked` through per row, so downstream tools that consume the per-row replay report see the same flag the summary uses.
- `scripts/test_model_benchmark.py` —
  - New `ReplayInvokedAndTargetFamilyDocTests` class with 4 deterministic tests:
    1. `test_summary_replayPassed_is_null_when_no_row_invoked_replay` — drives a fenced-lean fake-ollama run where every attempt is rejected at extraction; asserts summary `replayPassed is None` and `attemptHierarchy.replayInvoked == 0`.
    2. `test_summary_replayPassed_is_true_when_invoked_row_passes` — drives a fake-`submit-lean` returning `replayMatchesRuntime: True` for a verified attempt; asserts summary `replayPassed is True` and `attemptHierarchy.replayInvoked == 1`.
    3. `test_leaderboard_renders_em_dash_when_replayPassed_is_null` — invokes `render_leaderboard` directly on a `replayPassed: None` summary; asserts the rendered markdown contains `- replayPassed: \`—\`` and not `false`/`true`.
    4. `test_every_target_family_value_has_a_doc_section` — scrapes `\"(boole\\.[a-z][a-z0-9.]*\\.v\\d+)\"` literals from `boole-model-benchmark.py`, then asserts each has a `## <family>` header in `docs/benchmark-target-families.md`. This is the B7(a) lint.
  - 5 legacy assertions adjusted for the new score-dict shape: 4× hardcoded score equality became `{"blocksProduced": 1, "replayPass": True, "replayInvoked": True}`; 1× timeout-row score became `{"blocksProduced": 0, "replayPass": True, "replayInvoked": False}`; 1× legacy `attemptHierarchy` dict gained `"replayInvoked": 1`. Pure key-addition diffs — the existing semantic assertions stand unchanged.
- `docs/benchmark-target-families.md` (NEW) —
  - Authoritative target-family reference (CI-linted). Two `## <family>` sections to start:
    - `## boole.calibration.pow.v1`: mode `mining`, theorem template `theorem boole_benchmark_pow_target_<n>`, canonical proof shape "any term that closes `"<challenge>" = "<challenge>"`" (in practice `rfl`), difficulty from `fixtures/protocol/admission/v1.json`, verifier hash `boole-model-benchmark-ollama-v0` (legacy naming, deferred to Slice S5). Known limitations subsection records the rfl-trivial canonical proof (Slice S8 `v2` will fix), the single-template-per-attempt constraint, and the legacy verifier-hash naming.
    - `## boole.smoke.true.v1`: mode `smoke`, theorem template `theorem boole_benchmark_true : True`, intentionally trivial (any `True.intro` term passes). Known-limitations subsection flags that smoke rows must be filtered out of any leaderboard computation that mixes mining and smoke modes.
  - Header paragraph documents the immutability invariant ("a family is immutable once a run has used it; bump the suffix to v2/v3/...") and the CI lint contract (every `targetFamily` literal in the script must have a `## <family>` section here).

**Acceptance evidence:**
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 25/25 PASS (21 pre-existing S2 tests still green + 4 new B4/B7(a) tests pass deterministically Red→Green; the lint test would have failed before `docs/benchmark-target-families.md` was added).
- `cargo test --workspace` → 0 failures (38 test groups; S1a/S1b/S2/S3 all still green — Python-only diff doesn't disturb the Rust side).
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (N0 contract surface unchanged).
- `bash examples/boole-block-demo.sh` → `boole-block-demo: PASS` (S3 demo unchanged).
- Red-state proof for B4: stashing the `summarize` patch and running test #1 returns `replayPassed: True` with `attemptHierarchy.replayInvoked == 0` (the legacy vacuous-true bug). Restoring the patch flips it to `None` / `0`. Same drill for the leaderboard test (`replayPassed: \`true\`` → `replayPassed: \`—\``).
- Red-state proof for B7(a) lint: temporarily renaming the `## boole.calibration.pow.v1` header to `## boole.calibration.pow.v1-renamed` makes test #4 fail with the missing-section assertion; reverting passes.

**Deviations from plan §Appendix A · S4:**
1. **`replayInvoked` lives on the score dict, not as a sibling of `score`.** The plan §5 B4 sketch shows it as a peer of `score`. In practice every consumer of the field (leaderboard renderer, replay report, summary aggregator) was already reading `row["score"]`, and adding it there means one fewer key to plumb through the JSON-row reading code. The semantics are identical; the field is just nested one level deeper than the plan literal. Doc-comment in `score_from_result` captures the new shape.
2. **`replayInvoked` count in `attemptHierarchy`, not a top-level summary field.** The plan didn't specify where the count lives in the summary JSON. `attemptHierarchy` already carries the row-bucket totals (`generated`, `verifierInvoked`-style counters), so adding `replayInvoked` there keeps related metrics co-located. Top-level still surfaces `replayPassed` (boolean | null), which is what the leaderboard renders.
3. **Lint scrapes literals via regex, not via importing the script.** The plan said "CI lint asserts that every `targetFamily` value referenced in `scripts/boole-model-benchmark.py` has a section in the doc." The implementation reads the script as text and matches `\"(boole\\.[a-z][a-z0-9.]*\\.v\\d+)\"` rather than importing and inspecting AST nodes. The script does not expose the literals via a single constant (they're embedded in branches keyed off `--benchmark-mode`), and a regex over the source is both simpler and gives a clearer assertion message ("targetFamily 'boole.x.v1' has no '## boole.x.v1' header in docs/benchmark-target-families.md") than a faithful AST pass. Drift risk: a future literal that doesn't match the regex (e.g. uses single quotes, or a non-`v<int>` suffix) would silently bypass the lint. Mitigation: a comment in the test documents the regex contract; if the family-naming convention ever changes, the regex bumps in lockstep.
4. **`zero_score` keyword default is `False`, not `True`.** The plan §5 B4 sketch gives `replayInvoked` no explicit default. The implementation defaults to `False` because every existing `zero_score()` call site is for a row that did not reach the verifier (rejected, skipped, timeout). Defaulting to `False` means existing call sites need no changes; the only call site that overrides is the timeout-row path (which keeps `replay_pass=True` for legacy reasons but also opts into `replay_invoked=False`). Inline rationale captured at the function signature.

**Downstream unblocked:**
- **B5 (`verifier_hash` versioning) — Slice S5** — the v1 family doc already references the legacy `boole-model-benchmark-ollama-v0` string and flags the rename to a versioned fixture as Slice S5's job. S5 can now bump `boole.calibration.pow.v1`'s `Verifier hash` line to point at the new fixture key without rewriting unrelated context.
- **B1 + B7(b) (v2 family) — Slice S8** — the doc's "Known limitations" subsection on `boole.calibration.pow.v1` already calls out the rfl-trivial limitation that v2 fixes, so S8 can land the v2 section by appending a new `## boole.calibration.pow.v2` header without restructuring the doc. The lint will automatically pick up the new literal once the script references it.
- **Leaderboard consumers** — the summary JSON contract is now stable for `replayPassed: bool | null` and `attemptHierarchy.replayInvoked: int`. Downstream dashboards / scrapers can rely on the null-vs-true distinction without having to count rows themselves.

**Open follow-ups (not blocking S4):**
- **Surface `replayInvoked` in the per-attempt latency CSV** — currently `replayInvoked` only lands in `benchmark-rows.ndjson` and the summary. The per-attempt CSV is a B6 surface; when B6 lands, it should pick up the same flag so a row's `replayInvoked: false` correlates 1:1 with its rejection reason in the latency CSV.
- **Tighter B7(a) lint** — the regex skips single-quoted literals and unconventional suffixes. If we ever introduce a `boole.calibration.pow.v1a` (alpha suffix) or single-quoted literals, the regex needs a bump. Cheap to fix when the need arises.
- **Doc-driven canonicalization of family list** — `boole-model-benchmark.py` does not currently read the doc to populate the family list at runtime. A future slice could invert the relationship — read the doc for the canonical list and assert script literals are a subset — but it adds a runtime markdown parse for limited gain. Defer until at least one third family ships.

### Slice S5 [B] — B5 versioned `verifier_hash` file — SHIPPED 2026-05-09

**Outcome:** Gate B-CI's B5 bullet PASS. `scripts/boole-model-benchmark.py` no longer hardcodes `boole-model-benchmark-ollama-v0`; the verifier hash is read from `fixtures/benchmarks/verifier-hashes.json` (`{"active": <key>, "versions": {<key>: <hash>}}`) via the new `load_verifier_hashes` + `resolve_verifier_hash` helpers, and every row records both the resolved string (`verifierHash`) and the lookup key (`verifierHashVersion`). Bumping `active` from `v0` to a future `v1` hands the new hash to fresh runs while pinning historical rows to the version they were recorded with — the original `boole-model-benchmark-ollama-v0` string is preserved byte-identically as the `v0` entry so existing `benchmark-rows.ndjson` artifacts replay against the original hash without re-derivation.

**Files modified:**
- `fixtures/benchmarks/verifier-hashes.json` (NEW) —
  - Authoritative version-keyed verifier-hash map. `active: "v0"`, `versions.v0: "boole-model-benchmark-ollama-v0"`. Future versions append entries under `versions.<key>` and bump `active`; renaming `v0` is forbidden because it would invalidate every historical row that recorded `verifierHashVersion: "v0"`.
- `scripts/boole-model-benchmark.py` —
  - New module-level constant `VERIFIER_HASHES_FIXTURE = ROOT / "fixtures" / "benchmarks" / "verifier-hashes.json"`.
  - New `load_verifier_hashes(path=VERIFIER_HASHES_FIXTURE)`: reads the JSON, validates that `active` and `versions` exist, that `versions` is a non-empty `{str: str}` map, and that `active` keys into it. Returns a normalized dict `{"active": str, "versions": {str: str}}`. Validation failures raise `ValueError` with the offending path and reason — drift in the fixture format fails loudly at the call site rather than silently masking with the legacy literal.
  - New `resolve_verifier_hash(*, version=None, hashes=None)`: with `version=None` returns `(active, versions[active])` (the new-run path); with an explicit `version` returns `(version, versions[version])` (the historical-replay path that pins to the recorded version regardless of what `active` currently is). `hashes=None` triggers a fixture read; tests pass synthetic `hashes` dicts to exercise multi-version scenarios without rewriting the fixture. Unknown versions raise `KeyError(f"unknown verifier hash version: {version}")` so callers can match on the typed message.
  - Replaced `verifier_hash = "boole-model-benchmark-ollama-v0"` (line 813) with `verifier_hash_version, verifier_hash = resolve_verifier_hash()`. The legacy literal is now reachable only via the fixture, so the literal cannot drift away from the fixture without a test failure.
  - Added `"verifierHashVersion": verifier_hash_version` to the row dict alongside the existing `"verifierHash"` field. Co-located with `verifierHash` rather than nested under a `verifier` sub-object so consumers (replay, validation, leaderboard) can read both with one access pattern.
- `scripts/test_model_benchmark.py` —
  - New `VerifierHashVersioningTests` class with 6 deterministic tests:
    1. `test_verifier_hashes_fixture_exists_with_active_and_legacy_v0_preserved` — reads the fixture file directly; asserts `active` exists, references a real entry in `versions`, and that `versions["v0"]` is byte-identical to `"boole-model-benchmark-ollama-v0"`. Catches anyone trying to "tidy up" the legacy string.
    2. `test_load_verifier_hashes_returns_normalized_shape` — calls `benchmark.load_verifier_hashes()`; asserts shape `{"active": str, "versions": {str: str}}`.
    3. `test_resolve_verifier_hash_returns_active_by_default` — synthetic `{active:"v1", versions:{v0:"hash-a", v1:"hash-b"}}`; asserts `resolve_verifier_hash(hashes=...)` returns `("v1", "hash-b")`.
    4. `test_resolve_verifier_hash_with_explicit_version_pins_to_recorded` — same fixture, `resolve_verifier_hash(version="v0", hashes=...)` returns `("v0", "hash-a")`. This is the headline B5 spec — replay of a row recorded under v0 stays on v0 even when `active` has moved to v1.
    5. `test_resolve_verifier_hash_unknown_version_raises` — explicit unknown version raises `KeyError` with message containing both the unknown version and the typed prefix `"unknown verifier hash version"`.
    6. `test_benchmark_row_carries_verifier_hash_version` — runs the driver subprocess against a fake-ollama + fake-submit-lean pair (mirrors the `test_ollama_generated_candidate_is_submitted_to_verifier_path` setup); asserts `rows[0]["verifier"]["verifierHashVersion"]` matches the fixture's `active` key, AND `rows[0]["verifier"]["verifierHash"]` matches `versions[active]`. Both fields read from the fixture, not hardcoded — the test follows future `active` bumps automatically.
  - Updated the legacy assertion in `test_ollama_generated_candidate_is_submitted_to_verifier_path` (line 320) from `assertEqual(invocation["verifierHash"], "boole-model-benchmark-ollama-v0")` to read the expected hash from the fixture: `verifier_hash_fixture["versions"][verifier_hash_fixture["active"]]`. The semantic assertion is unchanged at `active="v0"`; it just stops re-hardcoding the literal.
- `scripts/test_preflight_orchestration.py` —
  - The fake-submit-lean script's hardcoded `"verifierHash": "boole-model-benchmark-ollama-v0"` left as-is (it's a structural echo, not an assertion target). Added a 4-line comment marking it as the v0-fixture mirror and pointing at the alternative pattern (read `--verifier-hash` from argv) that downstream readers should reach for if `active` ever moves past v0.
- `docs/benchmark-target-families.md` —
  - `boole.calibration.pow.v1` "Verifier hash" line rewritten to reference `fixtures/benchmarks/verifier-hashes.json` and the resolver helper. Records the version-pinning replay invariant explicitly so doc readers don't have to chase the script.
  - "Known limitations · Verifier-hash naming" bullet flipped from "S5 will fix" (forward-looking) to "S5 moved verifier-hash resolution to the versioned fixture; renaming v0 is forbidden" (post-S5 invariant). Tooling that strips the `ollama` substring is reminded to treat the legacy string as opaque.

**Acceptance evidence:**
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 31/31 PASS (25 prior + 6 new B5 tests).
- `python3 -m pytest scripts/test_model_benchmark.py scripts/test_preflight_orchestration.py -p no:anchorpy` → 46/46 PASS (preflight orchestration unchanged after the v0 fake-echo comment).
- `cargo test --workspace` → 38 test groups, 0 failures (Python-only diff doesn't disturb the Rust side).
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (N0 contract surface unchanged).
- `bash examples/boole-block-demo.sh` → `boole-block-demo: PASS` (S3 demo unchanged).
- Red-state proof for the headline test (`test_resolve_verifier_hash_with_explicit_version_pins_to_recorded`): with the resolver removed and the legacy `verifier_hash = "boole-model-benchmark-ollama-v0"` literal restored, `pytest VerifierHashVersioningTests` fails with `AttributeError: module 'boole_model_benchmark' has no attribute 'resolve_verifier_hash'`. Restoring the resolver flips it Green. Same drill for the fixture-presence test (file missing → AssertionError on `is_file()`).

**Deviations from plan §Appendix A · S5:**
1. **Resolver returns `(version, hash)` tuple, not bare hash.** The plan §5 B5 sketch implies `resolve_verifier_hash(...) -> str`. Returning the tuple costs nothing at the call site (Python destructures inline as `v, h = resolve_verifier_hash()`) and removes a second call to recover the version when the caller also wants to record it on the row. This is the dominant case (every row carries both fields), so the tuple form is the lower-friction shape.
2. **`KeyError` (not a custom exception class) for unknown versions.** The plan said "typed error" without specifying the exception type. `KeyError` matches the lookup-failure semantics, ships zero new types, and the message still carries the typed prefix `"unknown verifier hash version: <v>"` so callers can match on the message. A custom `UnknownVerifierHashVersion` exception would be over-engineering for a single call site.
3. **`verifierHashVersion` lives at the same level as `verifierHash` (top of the row dict's `verifier` sub-object), not under a nested `verifierHash: {value, version}`.** The plan didn't specify the layout. Co-locating both fields keeps the read pattern flat (`row["verifier"]["verifierHash"]` / `row["verifier"]["verifierHashVersion"]`) and avoids breaking every downstream JSON consumer that already reads `verifierHash` flat. The cost is two top-level keys instead of one nested object — negligible JSON.
4. **No `replay_lookup_verifier_hash(row)` helper shipped.** The plan listed it as a candidate. In practice the only consumer of the recorded version is the resolver itself: `resolve_verifier_hash(version=row["verifier"]["verifierHashVersion"])`. Wrapping that one-liner adds a layer without saving any keystrokes. Will revisit when a second consumer (replay tooling) actually surfaces the need.
5. **`scripts/test_preflight_orchestration.py:417` left hardcoded with a comment, not refactored to read from argv.** The plan said "leave as-is, add comment". The fake submit-lean stub there exists to test preflight orchestration, not verifier-hash resolution. Adding `--verifier-hash` argv parsing to the fake would entangle two test concerns and risk a flake if argv plumbing changes. The comment makes the future-bump path explicit (mirror the `test_model_benchmark.py:232` fake's argv read) without doing the refactor speculatively.

**Downstream unblocked:**
- **B1 + B7(b) v2 family — Slice S8** — when v2 ships its own verifier hash, it bumps `active: "v0"` → `active: "v1"` (or further) without touching v0's value. v1 rows automatically pick up the new hash and v0 rows automatically pin to the legacy hash. No code change in the resolver — the fixture is the only edit, plus a `## boole.calibration.pow.v2` section in `docs/benchmark-target-families.md`.
- **Replay tooling beyond submit-lean** — any future tool that reads `benchmark-rows.ndjson` to re-verify historical attempts (e.g. a CI re-validation script) calls `resolve_verifier_hash(version=row["verifier"]["verifierHashVersion"])` and gets the right hash without referencing the live fixture's `active` value. The version field is the durable contract.
- **Provider rename / verifier-hash provider-agnosticism** — the legacy `ollama` substring is locked into v0 forever (renaming would invalidate historical rows). A provider-agnostic name (e.g. `"boole-model-benchmark-v1"`) lands as a new `versions.v1` entry and an `active` bump in a future slice; no changes to history.

**Open follow-ups (not blocking S5):**
- **Replay/validation entry point that reads version per row** — there's no end-to-end replay tooling shipped yet that calls `resolve_verifier_hash(version=...)` on historical rows. The resolver supports the call shape; an actual replay validator (Slice S?) will surface when CI starts re-validating historical artifacts.
- **Versioning the checker artifact hash too** — `verifierHash` and `checkerArtifactHash` move together in practice (the verifier hash names a build of the checker). A future slice could fold both into one fixture entry per version (`{"v0": {"verifierHash": ..., "checkerArtifactHash": ...}}`) so a single bump moves both. Defer until a checker rebuild actually triggers a divergence.
- **Provider-agnostic `boole-model-benchmark-v1`** — when convenient, add a `v1` entry under `versions` that drops the `ollama` substring, and bump `active`. Pure fixture edit + a sentence in `docs/benchmark-target-families.md`. Not blocking any current slice.

### Slice S6 [C] — `boole keys new/list/show` (C2) — SHIPPED 2026-05-09

**Outcome:** Gate P6's C2 bullet PASS. `boole-cli` ships local key management with the pof JSON contract: `boole keys new --id <name> [--dev] [--dry-run]` writes `<keys-dir>/<id>.json` (mode 0600, atomic tmp+rename) and emits `{ok:true, key:{id, pk, createdAt, schema:"boole.keys.v1"}}` on stdout; `boole keys list` returns `{ok:true, keys:[...]}` sorted by id (empty array when the dir is missing); `boole keys show --id <name>` returns the stored envelope. Errors flow through a single `emit_typed_error(reason, exit_code, fields...)` helper as `{ok:false, reason:<kebab>, ...}` on stderr with pof exit codes (2 = `bad_request`, 3 = `key_already_exists` / `key_not_found`, 1 = `internal_error`). `--dev` seeds `pk` deterministically from `blake3("boole-keys-dev::" + id)` so fixture tests don't need a randomness mock; `--dry-run` emits the would-be envelope without touching disk. Storage location resolves env-first via `$BOOLE_KEYS_DIR` (test isolation) then `$HOME/.boole/keys`.

**Files modified:**
- `crates/boole-cli/Cargo.toml` —
  - Added `blake3.workspace = true` and `hex.workspace = true` to `[dependencies]`. Both are already workspace members (used by `boole-core`'s blake3 leaf-hashing and the existing CLI hex output), so no `Cargo.lock` shape diff beyond a new edge from `boole-cli`.
- `crates/boole-cli/src/main.rs` —
  - New imports `std::fs::OpenOptions` and `std::os::unix::fs::OpenOptionsExt` for the mode-0600 atomic-create path.
  - New `Command::Keys { command: KeysCommand }` clap subcommand with three `KeysCommand` variants: `New { id, dev, dry_run }`, `List`, `Show { id }`. Wired into the existing `run()` dispatch alongside `block` / `node` / `genesis`.
  - `keys_dir() -> PathBuf` — env-first resolver: `$BOOLE_KEYS_DIR` overrides; default `<HOME>/.boole/keys` (falls back to `./.boole/keys` if `$HOME` is unset, which only happens in heavily-stripped environments). The env override is the seam every test uses.
  - `validate_key_id(&str) -> Result<(), String>` — accepts `[a-zA-Z0-9_-]+`, rejects empty / path separators / dots / whitespace. Inline byte-loop instead of pulling `regex` as a dep — the alphabet is tiny.
  - `random_pk_hex() -> anyhow::Result<String>` — opens `/dev/urandom`, reads 32 bytes via `read_exact`, hex-encodes. POSIX-only by construction (Boole's CI matrix is Linux + macOS); Windows support would swap in `getrandom` and is deferred until there's a Windows consumer.
  - `dev_pk_hex(id: &str) -> String` — `blake3::Hasher::new()` then update with `b"boole-keys-dev::"` and `id.as_bytes()`, hex-encode. Domain-separated so a future deterministic-seed scheme for a different surface (e.g. `boole bounty keys --dev`) can use a distinct prefix without colliding.
  - `now_iso8601_utc() -> String` + `format_iso8601_utc(secs: i64)` + `days_to_civil(days: i64) -> (i32, u32, u32)` — hand-rolled second-precision ISO 8601 UTC formatter. Howard Hinnant's civil-from-days algorithm handles leap years/months without pulling `chrono` (saves ~600KB binary size and a transitive dep tree). Range covered is well past 9999, more than sufficient for `createdAt` timestamps.
  - `atomic_write_0600(path, bytes) -> anyhow::Result<()>` — writes to `<path>.tmp.<pid>` via `OpenOptions::new().write(true).create_new(true).mode(0o600).open()` (so the perm bits are set at create time, not via a follow-up `chmod` race), `write_all` + `sync_all`, then `rename`. On rename failure the tmp file is removed in a finally-style cleanup so a partial write never lingers.
  - `emit_typed_error(reason, exit_code, fields: &[(&str, serde_json::Value)]) -> !` — single helper for every typed-error site. Builds `{ok:false, reason, ...fields}` via `serde_json::Map`, prints to stderr, calls `std::process::exit(exit_code)`. Mirrors the boole-node `http_error` envelope shape so server and CLI errors are visually interchangeable.
  - `keys_new(id, dev, dry_run)` — id validation (exit 2 `bad_request`), duplicate check (exit 3 `key_already_exists` with the conflicting path echoed), pk via `dev_pk_hex` or `random_pk_hex`, builds the envelope, skips disk write under `--dry-run` (still prints the envelope so callers can plumb-test), atomic write otherwise, `{ok:true, key:{...}}` to stdout.
  - `keys_list()` — reads the keys dir; missing dir → `{ok:true, keys:[]}` (not an error: a fresh install legitimately has no keys). Filters `*.json`, sorts by path so output is deterministic across filesystems, parses each, emits the array. Parse failures bubble as `internal_error` (exit 1) — corrupted on-disk state is the operator's problem, not a recoverable CLI condition.
  - `keys_show(id)` — id validation (exit 2), missing file → `{ok:false, reason:"key_not_found", id}` (exit 3), parse + reprint the stored envelope wrapped in `{ok:true, key:{...}}`.
- `crates/boole-cli/tests/keys.rs` (NEW) —
  - 9 deterministic process-spawn tests against `env!("CARGO_BIN_EXE_boole-cli")` with `BOOLE_KEYS_DIR=<tmp>` env override. `fresh_tmp(label)` mints a unique dir using `std::process::id()` + `SystemTime::now()` nanos so concurrent test threads don't collide.
  - Coverage: envelope shape + mode 0600 (`std::fs::metadata().permissions().mode() & 0o777 == 0o600`); `--dev` determinism across two separate dirs (byte-identical `pk`); duplicate-id `key_already_exists` exit 3 with stderr envelope and unchanged file; `--dry-run` envelope on stdout + zero-touch on disk; bad-id matrix `["", "a/b", "../oops", "white space", "dot.in.middle"]` → exit 2 `bad_request`; `keys list` sort (insert `["carol", "alice", "bob"]`, expect `["alice", "bob", "carol"]`); empty/missing-dir `keys list` → `{ok:true, keys:[]}` (not an error); `keys show` round-trips an envelope written by `keys new`; `keys show` on a missing id → exit 3 `key_not_found`.

**Acceptance evidence:**
- `cargo test -p boole-cli --test keys` → 9/9 PASS.
- `cargo test --workspace` → 38 test groups, 0 failures.
- `cargo clippy -p boole-cli --tests -- -D warnings` → clean.
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (N0 contract surface untouched by C2 work).
- `bash examples/boole-block-demo.sh` → `boole-block-demo: PASS` (S3 demo unaffected).
- Manual smoke: `BOOLE_KEYS_DIR=/tmp/boole-keys-demo target/debug/boole-cli keys new --id demo --dry-run` returns `{ok:true, key:{id:"demo", pk:<64-hex>, createdAt:"2026-05-09T...Z", schema:"boole.keys.v1"}}` on stdout and leaves `/tmp/boole-keys-demo` untouched.
- Red-state proof: with the `Keys` clap variant + helpers stashed and only `tests/keys.rs` present, all 9 tests fail with `error: unrecognized subcommand 'keys'` (clap's exit 2). Restoring the implementation flips it 9/9 Green.

**Deviations from plan §Appendix A · S6:**
1. **No `sk` field shipped.** The plan §3 C2 sketch said "key shape is `{id, pk}` for now". Locked in: only `pk` lives on disk because the protocol does not yet have a signing scheme — `pk` is an opaque 32-byte miner identity. When bounty-announce signing lands (a later C-track slice), the right move is a new `schema: "boole.keys.v2"` envelope (with `sk` or a sealed-box wrapper) rather than retrofitting `v1`. The `schema` field is on every envelope precisely so a v2 reader can refuse to load v1 without the operator opting in.
2. **`schema: "boole.keys.v1"` field added on every envelope.** The plan didn't specify a schema marker. Adding it now (zero cost — one extra string per file) is the difference between "we'll figure out migration later" and "v2 reader can branch on schema and reject ambiguous payloads at parse time". Standard versioning hygiene; matches how `boole-node`'s `/health` envelope already carries `schema: "boole.health.v1"`.
3. **ISO 8601 timestamp is hand-rolled, not via `chrono`.** The plan's helper sketch was `now_iso8601_utc()` without dictating the library. `chrono` would add ~600KB and a transitive dep tree to a CLI that otherwise has zero need for date math. Howard Hinnant's civil-from-days is ~30 lines, public domain, and exactly second-precision (the resolution `createdAt` needs). Tested via the round-trip in the keys-new envelope test (`createdAt` regex match `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$`). Will revisit if a future slice needs sub-second precision or timezone math.
4. **`atomic_write_0600` sets the mode at create time via `OpenOptions::mode(0o600)`, not via a follow-up `set_permissions`.** Closes a TOCTOU window where a concurrent reader could open the tmp file between `create` and `chmod`. Same end state, strictly safer race profile, no extra cost.
5. **`keys list` returns `{ok:true, keys:[]}` for a missing directory rather than `{ok:false, reason:"not_found"}`.** A fresh install legitimately has no keys; treating that as an error would force every consumer to special-case the first-run path. Empty array is the unambiguous shape — pof's reference CLI uses the same convention. Documented in the test (`keys_list_with_empty_or_missing_dir_returns_empty_array`).
6. **Bad-id matrix expanded beyond the plan's three cases (`""`, `"a/b"`, `"../oops"`).** Added `"white space"` and `"dot.in.middle"` because the validation regex `[a-zA-Z0-9_-]+` rejects them too — a test that didn't cover them would let a future "tidy up" regex relaxation slip through silently. All five cases share the same exit + envelope shape; the test parametrizes over them with a single helper.

**Downstream unblocked:**
- **Bounty-announce signing (future C-track slice)** — the `schema: "boole.keys.v1"` envelope is the migration anchor. A v2 envelope adds `sk` (or `sealedSk`) and a `schemaPrev` reference; existing v1 keys keep working until the operator runs a `boole keys upgrade` flow. No retroactive disk-format migration required.
- **`boole bounty post --signed-by <id>`** — the `keys show --id <name>` path already returns the parsed envelope on stdout; the bounty CLI can shell to `boole keys show` for lookup without re-implementing the dir layout. (When signing actually lands, it'll read the on-disk file directly to grab `sk`, but the read path / location contract is now frozen.)
- **CI fixture seeding** — `--dev --id <fixed>` produces byte-identical keys across machines, so test fixtures that need a stable miner identity (e.g. `boole.keys.dev.alice`) can seed without committing a real-looking pubkey to git.

**Open follow-ups (not blocking S6):**
- **Windows support** — `/dev/urandom` and Unix mode bits are POSIX-only. When a Windows CI lane appears, swap `random_pk_hex` to `getrandom::getrandom` and gate the `OpenOptionsExt::mode` call behind `cfg(unix)` (Windows ACLs would replace mode bits). Cheap to add when the demand surfaces; not worth the dep + cfg branching today.
- **`boole keys delete --id <name>`** — not in the plan and not in pof's reference, but the natural fourth op once key rotation matters. Trivial to add when needed (read → unlink, with `key_not_found` for a missing id).
- **`boole keys export --id <name>`** — once `sk` lands in v2, an export path will follow. Today there's nothing extractable beyond the JSON envelope, which `keys show` already prints, so this is a v2-era follow-up.
- **Schema-version negotiation in `keys list`** — currently `keys list` parses every file and emits the union; if a v2 file appears, today's reader still parses the shared fields (`id`, `pk`, `schema`) and emits them, which is the right "forwards-compatible" behavior. When v2 ships, decide whether `keys list` filters by schema or surfaces a `schema` field per entry. Defer until v2 is in flight.

### Slice S7 [N] — N1 axum migration — SHIPPED 2026-05-09

**Outcome:** Gate P1 PASS. `boole-node`'s local HTTP transport is now `axum 0.8` on a multi-thread `tokio` runtime. The N0 contract surface — every route, every JSON envelope, every status code — is byte-identical: `tests/http_contract.rs`, `tests/local_node.rs`, `scripts/local-node-smoke.sh`, and `examples/boole-block-demo.sh` were the regression net and all pass unchanged. The raw-TCP path (`read_http_request` / `handle_connection` / `write_json_response` / `write_error_response`) is fully deleted from the source tree. A new 16-concurrent `/submit` regression test (`tests/concurrency.rs`) proves the admission gate still serializes correctly under contention.

**Files modified:**
- `Cargo.toml` (workspace) —
  - Added four workspace deps to power the transport: `axum = "0.8"`, `tokio = { version = "1", features = ["full"] }`, `tower = "0.5"`, `tower-http = { version = "0.6", features = ["timeout"] }`. `tower` is included for the `Service`/`MakeService` traits we wrap; `tower-http` ships the request-timeout layer (deprecation-aware: uses the new `TimeoutLayer::with_status_code(StatusCode, Duration)` API, not the deprecated `TimeoutLayer::new`).
- `crates/boole-node/Cargo.toml` —
  - Added `axum.workspace = true`, `tokio.workspace = true`, `tower.workspace = true`, `tower-http.workspace = true` to `[dependencies]`. No version drift vs the workspace; everything pins via `.workspace = true`.
- `crates/boole-node/src/local_node.rs` (full rewrite of the transport layer) —
  - Public surface preserved: `pub fn serve_local_node(listener: std::net::TcpListener, config: LocalNodeConfig) -> anyhow::Result<()>` — same sync entry signature so `boole-node`'s `main.rs` (`run-local`) and every consumer (boole-cli's `boole node start`, the smoke scripts, every test) compiles unchanged. Internally `serve_local_node` builds a multi-thread tokio runtime via `tokio::runtime::Builder::new_multi_thread().enable_all().build()?` and `block_on`s an async `serve_local_node_async`.
  - State model: `LocalNodeState` retains all the boot-time business state (runtime, scenario, store path, replay-matches-runtime witness). A new `AppState { inner: Arc<RwLock<LocalNodeState>> }` is the per-request lock seam; read endpoints (`/status /head /config /health /block/latest /block/{height}`) take `read()`; the two mutating endpoints (`/ticket /submit`) take `write()`. `RuntimeAdmissionState::observe_ticket_from_body(&mut self)` and `admit_body_with_canon_tag(&mut self, ...)` both require `&mut`, so a write lock is unavoidable until a follow-up refactor pushes interior-mutability into `boole-core`. RwLock keeps reads parallel, which is the practical win.
  - Router: `Router::new().route("/status", get(...))` …, with `axum 0.8` matchit-0.8 path syntax (`/block/{height}`, not the old `/block/:height`). Layered top-down: `TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT)` (15s, preserves the old socket timeout semantics at request granularity), then a custom `body_cap_middleware` that enforces the 1 MiB cap by inspecting `Content-Length` *before* axum buffers the body (so we keep the existing 413 envelope `{ok:false, reason:"body_too_large", limitBytes, actualBytes}` exactly — axum's `DefaultBodyLimit` returns a different shape and was therefore rejected), then `connection_close_middleware` which stamps `Connection: close` on every response.
  - Why `Connection: close`: hyper's HTTP/1.1 default is keep-alive. The existing wire-level regression net (`http_contract.rs`, `local_node.rs`, the boole-cli node-block tests) issues each request as a fresh `TcpStream::connect → write → read_to_end` cycle; `read_to_end` blocks until EOF. Without `Connection: close` the server holds the connection open per HTTP/1.1 spec and the test client hangs. Forcing close per response is the lowest-risk way to keep the existing test contract *and* make our `--max-requests N` accounting deterministic (each request = exactly one TCP connection).
  - `--max-requests N` semantics — the trickiest part of the migration. The raw-TCP server counted "served" once per *connection that finished processing*, including a connection that opened and closed without sending any HTTP bytes (the readiness-probe pattern in `crates/boole-cli/tests/node_block.rs::node_start_spawns_daemon_serving_health` explicitly relies on this: `--max-requests 2` budgets 1 slot for the bare TCP probe + 1 slot for the actual `GET /health`). The first axum draft counted HTTP requests via a `request_counter_middleware` and broke that test (probe consumed no slot → server stayed alive past the timeout). The second draft counted TCP accepts via a custom `axum::serve::Listener` impl, which fixed node_start but failed on the orphan-rule wall: `axum 0.8`'s `Connected<IncomingStream<'_, TcpListener>>` impl for `SocketAddr` is keyed off the concrete `tokio::net::TcpListener`, not a generic `L: Listener<Addr = SocketAddr>`, so a custom listener can't drive `into_make_service_with_connect_info::<SocketAddr>()`. The shipping design counts events at *per-connection `tower::Service` end-of-life*: `ConnectionCountingMakeService<M>` wraps the make service; each `MakeService::call(IncomingStream)` mints a fresh `Arc<ConnectionLifetime>` and embeds it in a `ConnectionCountedService<S>` Clone-able wrapper. Hyper clones the per-connection service for every HTTP request it dispatches over that connection (HTTP/2 stream concurrency, HTTP/1.1 internal cloning), so the wrapper itself can't `Drop`-count. The shared `Arc<ConnectionLifetime>` solves it — `ConnectionLifetime`'s `Drop` runs exactly once when the last clone of the per-connection service is released, i.e. when hyper has finished tearing down the connection. That's the correct moment to bump `served`, and (crucially) it fires *after* axum has fully serviced the request, avoiding the race where notifying earlier causes `axum::serve`'s `with_graceful_shutdown` to drop `signal_tx` before the spawned hyper task starts polling and the client sees `Connection reset by peer`.
  - Shutdown: a single `tokio::sync::Notify` is shared between the connection-lifetime drop hook and `axum::serve(...).with_graceful_shutdown(async move { notify.notified().await })`. After the Nth connection finishes, `notify.notify_one()` fires; axum stops accepting and drains in-flight tasks; the runtime returns from `block_on`; `serve_local_node` returns `Ok(())`.
  - Listener handoff: the public sync entry receives a `std::net::TcpListener` (so the test harness retains its existing "bind first, then hand over" pattern). Inside `serve_local_node_async` we call `listener.set_nonblocking(true)?` and `tokio::net::TcpListener::from_std(listener)?` to convert without re-binding, preserving the ephemeral-port contract every test depends on (`TcpListener::bind("127.0.0.1:0")`).
  - Peer IP for rate limiting: `app.into_make_service_with_connect_info::<SocketAddr>()` plus `ConnectInfo<SocketAddr>` extraction in the `submit_handler` keeps `local_node_submit_uses_tcp_peer_ip_not_spoofed_body_ip_for_rate_limit` (the security-critical test that pins peer-IP source to the TCP layer, not the JSON body) green without a single line of test change.
  - 413 / 404 / 408 envelopes: kept exactly. `error_response(HttpError)` builds `(StatusCode, Json(envelope))` tuples and goes through `IntoResponse`. The custom `fallback_handler` preserves `{ok:false, reason:"not_found", detail:"no route for METHOD path"}`. The `TimeoutLayer` returns 408 with a default body — acceptable because no existing test exercises the timeout path.
  - Deleted: `read_http_request`, `header_end`, `HttpRequest`, `HttpRequestError`, `handle_connection`, `write_error_response`, `write_json_response`, `state_should_stop`, `SOCKET_READ_TIMEOUT`, `SOCKET_WRITE_TIMEOUT`. ~250 lines of bespoke HTTP/1.1 plumbing replaced by axum's vetted stack.
- `crates/boole-node/tests/concurrency.rs` (NEW) —
  - `concurrent_submits_serialize_through_admission` boots `serve_local_node` on `127.0.0.1:0` with `max_requests = 16`, then spawns 16 std::threads that each issue a fresh `POST /submit` with the `runtime-smoke/v1.json` step-0 body. Asserts: every response is HTTP 200; exactly *one* response carries `accepted: true / ok: true`, the other 15 carry `accepted: false / ok: false / decision: <non-empty>` (the `RateLimited { reason: IpQuota }` decision tag from `boole-core::AdmissionDecision`, since 127.0.0.1 hits `perIpRateLimitPer60s` after the first submit); accepted + rejected = 16; the server thread joins cleanly. This is the regression that proves N1 didn't introduce a `share_pool` race during the move from single-threaded `for stream in listener.incoming()` to multi-thread tokio.

**Acceptance evidence:**
- `cargo test -p boole-node --no-fail-fast` → 13 test groups, all PASS (unit `http_error` 3/3; `agent_proof_cli` 2/2; `concurrency` 1/1 NEW; `http_contract` 3/3 unchanged; `local_node` 3/3 unchanged; `proof_package_bridge` 7/7; `runtime_global_cap` 1/1; `runtime_policy_boot` 9/9; `runtime_smoke_cli` 11/11; `runtime_smoke_library` 2/2; `store_fixtures` 3/3; `submit_lean_cli` 5/5).
- `cargo test --workspace --no-fail-fast` → 0 failures across all crates (boole-core, boole-cli, boole-node, boole-lean-runner). Notably `crates/boole-cli/tests/node_block.rs::node_start_spawns_daemon_serving_health` PASS (the test that wedged on the request-counter and accept-counter drafts; drop-once lifetime token is what unwedged it).
- `cargo clippy -p boole-node --tests -- -D warnings` → clean.
- `bash scripts/local-node-smoke.sh` → `local-node-smoke: PASS` (unchanged script, axum-served).
- `bash examples/boole-block-demo.sh` → `boole-block-demo: PASS` (unchanged demo, axum-served).
- Source-grep proof of raw-TCP removal: `grep -rE "read_http_request|handle_connection|write_json_response|write_error_response" crates/boole-node/src/` → no hits.
- Red-state proof: with `local_node.rs` reverted to the raw-TCP source and only `tests/concurrency.rs` present, the test fails immediately at `serde_json::from_str` because the raw-TCP server's response framing differs (the test asserts on the parsed body shape under concurrent contention). Restoring the axum implementation flips it Green.

**Deviations from plan §Appendix A · S7 (and §3 N1):**
1. **Lock model is `Arc<tokio::sync::RwLock<LocalNodeState>>`, not the plan's "RwLock + interior mutability everywhere".** `RuntimeAdmissionState`'s mutation surface still requires `&mut self` — pushing interior mutability into `boole-core` is a much larger refactor (it touches the share pool, the rate limiter, and the cached block count, none of which are currently `Send + Sync` for parallel writes). The shipped design uses `read()` for the six pure-read endpoints (so they parallelize freely) and `write()` only for `/ticket` and `/submit` (the two endpoints that mutate runtime state). This is correct under the existing API, ships the transport win immediately, and leaves the interior-mutability refactor as a clean follow-up slice when the access pattern actually warrants it.
2. **Per-connection counter via `tower::Service` lifetime, not a custom `axum::serve::Listener`.** The plan didn't dictate the mechanism, but the natural-looking choice (a counting `Listener` wrapper) is *not viable* under axum 0.8 — its `Connected<IncomingStream<'_, TcpListener>> for SocketAddr` impl is concrete to `tokio::net::TcpListener`, so a custom listener loses peer-IP `ConnectInfo`. The `ConnectionCountingMakeService` + `Arc<ConnectionLifetime>` design sits at the only layer that's both (a) hookable and (b) Connection-scoped (not request-scoped, not accept-scoped). It also matches the *exact* semantic of the raw-TCP server: count an event when a connection finishes its work, regardless of whether HTTP bytes were ever sent.
3. **Forced `Connection: close` on every response, instead of relying on hyper keep-alive.** The plan didn't address keep-alive. Forcing close is the lightest possible compatibility shim with the existing wire-level test client, and aligns with the `--max-requests N` accounting model (one connection = one served event). The cost is one extra header per response and the loss of HTTP/1.1 connection reuse — neither of which any current consumer depends on (curl, the CLI's `reqwest::blocking::Client`, the rust test harness all open fresh connections per call).
4. **`TimeoutLayer::with_status_code` not `TimeoutLayer::new`.** `tower-http 0.6` deprecated `TimeoutLayer::new` in favor of an explicit-status-code constructor; the new signature is `(StatusCode, Duration)` (note: status first, duration second — easy to swap, caught by the type checker on first compile attempt). Behavior identical; just keeps clippy quiet on a `-D warnings` build.
5. **Boolean `axum::extract::ConnectInfo<SocketAddr>` chosen over a custom `Connected` impl.** Keeping `SocketAddr` as the connect-info type means we ride axum's blanket impl directly and don't have to fight orphan rules with our own type. The peer IP is exactly what the rate limiter needs; richer connect-info (e.g. TLS state) is not on the table for the local-node use case.
6. **No public API change in `LocalNodeConfig` or `serve_local_node`.** The plan's spirit was "transport diff only". `LocalNodeState::from_config` no longer carries an explicit `max_requests` field on the state (that field was an artifact of the raw-TCP loop's internal accounting); `max_requests` is now read from `config` directly at the top of `serve_local_node` and threaded into `serve_local_node_async`. Net effect on callers: zero.
7. **No `tracing` integration shipped.** Tempting and almost free with `axum`/`tower-http`, but every existing consumer (smoke scripts, CLI, test harness) parses stdout as plain text or JSON only. Adding structured logs without a consumer that reads them is engineering debt, not value. Defer to the operator-debugging slice when there's an actual observability story to ship.

**Downstream unblocked:**
- **N3 (account/balance) and beyond can ship as plain `axum` handlers.** The router skeleton (`Router::new().route(...).layer(...).with_state(state)`) is exactly the shape future endpoints want. Adding `/account/{pk}/balance` is a `route` line + a handler function; no transport plumbing required.
- **Streaming response endpoints (e.g. `GET /work/stream` for SSE)** are now possible — axum's `Sse` and `axum::response::Streaming` types are compatible with the current `AppState` pattern. Raw TCP couldn't do this without re-implementing the chunked encoder.
- **Request-level observability hooks** (request id, latency histogram, structured logs) can land via additional `tower-http` layers without touching handlers — the plumbing is now standard.
- **HTTP/2 + TLS termination** is a swap of `axum::serve(listener, ...)` for `axum_server::bind(...).serve(...)` with a `RustlsConfig`, with zero handler changes. Out of scope for S7, but the transport now permits it cheaply.

**Open follow-ups (not blocking S7):**
- **Push interior mutability into `RuntimeAdmissionState` so `/ticket` and `/submit` can take read locks** — the share pool, rate limiter, and cached block count are the three fields that need internal `Mutex`/`RwLock` cells. Worth doing once contention shows up in benchmarks; today the per-request lock duration is microseconds and the tests stress it at 16-way concurrency without observable issues.
- **Replace the forced `Connection: close` with proper hyper keep-alive once the wire-level tests migrate to a high-level HTTP client.** The shape of those tests (`TcpStream + read_to_end`) was deliberate when the server was hand-rolled raw TCP; once they're rewritten to use `reqwest` or `hyper-util` clients, the `Connection: close` shim becomes vestigial. Pure cleanup; no behavior change.
- **Add a `ServiceBuilder::layer(TraceLayer::new_for_http())` once a consumer reads the logs.** Mentioned above; literally one line when the time comes.
- **Convert `body_cap_middleware`'s 413 envelope into a typed extractor.** The current middleware pre-checks `Content-Length` and synthesizes the response by hand. A typed `Json<T>` extractor with a custom rejection would be more idiomatic, but would also require teaching the rejection how to render the existing `{ok:false, reason:"body_too_large", limitBytes, actualBytes}` shape — strictly more code than the middleware. Defer until the envelope shape becomes a teaching example for new contributors.
- **Cargo.lock impact** — the new `axum + tokio + tower + tower-http + hyper` graph adds a non-trivial transitive set (≈40 new crates, mostly in the hyper-util / tower-http / pin-project-lite / http-body-util neighborhood). All MIT/Apache-2.0 licensed; reviewed at workspace-deps add time.

### Slice S8 [B] — B1 + B7(b) v2 family — SHIPPED 2026-05-09

**Outcome:** Gate B B1+B7(b) deterministic-CI PASS. `boole.calibration.pow.v2` ships as a new `targetFamily` selectable via `--benchmark-mode mining-v2`. Each per-attempt theorem is `∀ (xs : List Int), <chain xs>.length = xs.length` where `<chain>` is a deterministic composition of length-preserving ops drawn from `{mapAdd k, mapMul k, sortAsc}` — non-`rfl`-equivalent by construction (the kernel cannot reduce `(mapAdd k xs).length` to `xs.length` without `List.length_map`). Canonical proofs are right-folded `Eq.trans` chains of `length_*` lemmas from the new `Boole.Family.V0Helpers` Lean library. The legacy `boole.calibration.pow.v1` family stays byte-frozen and callable for back-compat smoke runs (Slice S4 v1 regression tests still Green).

**Files added:**
- `lean/checker/Boole/Family/V0Helpers.lean` (NEW, 41 lines) — Lean library namespaced `Boole.Family.V0Helpers`, ported from `projects/pof/lean/Boole/Family/V0Helpers.lean` and narrowed to the lp-only subset:
  - `@[reducible] def mapAdd (k : Int) (xs : List Int) : List Int := xs.map (fun x => x + k)`
  - `@[reducible] def mapMul (k : Int) (xs : List Int) : List Int := xs.map (fun x => x * k)`
  - `@[reducible] def sortAsc (xs : List Int) : List Int := xs.mergeSort (fun a b => decide (a ≤ b))`
  - `theorem length_mapAdd / length_mapMul / length_sortAsc` — each closed by `unfold` then `List.length_map`/`List.length_mergeSort`.
  - The pof v0.2 truthy lemmas (`all_filterByPred_self`, `nodup_dedup`, `pairwise_sortAsc`, `partition_eq_filter_filter`) are intentionally omitted: N=1 lp-only does not exercise them.
- `fixtures/benchmarks/v031-lp/golden-instances.json` (NEW) — 5-instance deterministic regression fixture. Each entry stores `runId="ci-v2-fixture"`, `target="ollama:test"`, `attemptIndex 0..4`, `benchmarkMode="mining-v2"`, plus the derived `(challenge, nonce, theoremName, lpChainLen, lpD, lpChain, chainExpr, canonicalProof, wrappedCandidate)`. Replay regenerates from the stored seeds and asserts byte-identical `(chainExpr, canonicalProof, wrappedCandidate)` tuples — protects against any silent change to the cursor algorithm, op-set encoding, or canonical-proof shape.

**Files modified:**
- `lean/checker/lakefile.lean` — added `lean_lib «Boole» where globs := #[.submodules \`Boole.Family]` between the existing `package boole_check` declaration and the `lean_exe boole_check` declaration. The `globs := #[.submodules ...]` form auto-discovers every module under `Boole.Family.*`, so a future `Boole.Family.V0Helpers2` (or sibling truthy-helpers module) requires zero lakefile churn. Verified via `lake build Boole` (succeeds in 153ms cold).
- `scripts/boole-model-benchmark.py` —
  - New module-level constant `MINING_V2_TARGET_FAMILY = "boole.calibration.pow.v2"`. Sits next to the existing `MINING_TARGET_FAMILY = "boole.calibration.pow.v1"`; the constant is the single source of truth that the doc-lint asserts has a section in `docs/benchmark-target-families.md`.
  - `target_family_for_mode(benchmark_mode)` extended with a `"mining-v2"` branch returning `MINING_V2_TARGET_FAMILY`. `"smoke"` and `"mining"` paths unchanged.
  - 8 new `_v031lp_*` private helpers ported byte-for-byte from pof's `Cursor` + `genOpV031Length` + `genBranchV031Lp`:
    - `_v031lp_cursor_read(buf, pos, mod_n) -> (n, pos)` — reads one byte at `buf[pos % len(buf)]`, returns `(byte % mod_n, pos + 1)`. Matches pof `Cursor.readNat` exactly, including the wrap-on-end-of-buffer behavior (the seed is sha256, 32 bytes; chains can consume more than 32 reads at higher chainLen, so the wrap is structural, not a bug).
    - `_v031lp_choose_int(buf, pos, range_n) -> (k, pos)` — reads `mod_n = range_n*2 + 1`, maps to `k - range_n` so the result lives in `[-range_n, range_n]`. Mirrors pof `Cursor.chooseInt`.
    - `_v031lp_gen_op_length(buf, pos)` — picks one of 3 ops via `cursor_read mod 3`. For `mapAdd`/`mapMul` reads `chooseInt` with `range=9` (→ k ∈ [-9, 9]); for `sortAsc` consumes a 5-mod placeholder byte to keep the stream alignment with v0.2's full op set. The placeholder byte is critical: dropping it would yield a different chain for the same seed at chainLen≥2.
    - `_v031lp_generate(seed_bytes)` — the top-level emitter. Reads `D = (cursor_read 6) + 1` so chain length lives in `[1, 6]`, then folds `_v031lp_gen_op_length` D times to produce `chain : list[dict]`, then consumes 3 forward-compat positions (pred kind / inputLen / noiseDefs) so the cursor is byte-equivalent to the pof generator even though N=1 lp-only doesn't read them. Returns `{"chain": [...], "chainLen": D, "D": D_raw}`.
    - `_v031lp_op_apply(op, inner) -> str` — renders `(mapAdd (k : Int) inner)` / `(mapMul (k : Int) inner)` / `(sortAsc inner)`. The `(k : Int)` ascription is required because Lean infers `Nat` for non-negative literals otherwise, which would mis-elaborate against `mapAdd : Int -> List Int -> List Int`.
    - `_v031lp_render_chain_expr(chain, var="xs")` — left-fold `_v031lp_op_apply` over the chain starting from `var`. Output for chain `[mapAdd(-2), mapMul(3)]` is `(mapMul (3 : Int) (mapAdd (-2 : Int) xs))` — outer is the *last* applied op, matching the pof Lean term shape exactly.
    - `_v031lp_length_lemma_call(op, inner)` — emits `length_mapAdd k inner` / `length_mapMul k inner` / `length_sortAsc inner` (no `(k : Int)` here because the helper signatures already pin the type).
    - `_v031lp_render_canonical_proof(chain) -> str` — chainLen=1 emits `fun xs => length_<op> <args> xs` directly; chainLen≥2 emits `fun xs => (length_<lastOp> _).trans ((length_<midOp> _).trans (... (length_<firstOp> <args> xs)))` — innermost call binds `xs`, every outer call uses `_` so Lean infers the intermediate list. This matches the pof reference proof shape in `Boole.Family.V0ProbeV031.lean` exactly.
  - `attempt_context(...)` extended with a `"mining-v2"` branch: derives `cursor_seed = sha256(seed + "|v031-lp-cursor").digest()`, calls `_v031lp_generate`, builds `chainExpr` and `canonicalProof`, and returns the v2 context dict (`benchmarkMode, targetFamily, attemptIndex, challenge, nonce, theoremName, lpChain, lpChainLen, lpD, chainExpr, canonicalProof`). v1 path falls through unchanged.
  - `wrap_proof_term_candidate(...)` extended with a `"mining-v2"` branch emitting a full Lean module: a header of lottery-binding comment lines (`-- benchmarkMode: mining-v2`, `-- targetFamily: boole.calibration.pow.v2`, `-- lotteryChallenge: …`, `-- lotteryNonce: …`, `-- lpChainLen: N`, `-- lpD: D`), then `import Boole.Family.V0Helpers`, `namespace <theoremName>`, `open Boole.Family.V0Helpers`, then `theorem <theoremName> : ∀ (xs : List Int), <chainExpr>.length = xs.length := <indented proof term>`, then `end <theoremName>`. The lottery-binding comments are what bind a candidate to a specific challenge/nonce — `lake exec boole_check` ignores them, but the source-hash recorded in the benchmark row covers them, so a model's accepted submission cannot be replayed against a different challenge.
  - `model_proof_term_prompt(...)` extended with a `"mining-v2"` branch describing the v031-lp contract to the LLM: lists the helper names from `Boole.Family.V0Helpers`, gives the chain expression for the current attempt, and provides worked examples for chainLen=1 (`fun xs => length_<op> <args> xs`) and chainLen≥2 (`Eq.trans` composition). The prompt deliberately does not name pof or v031 — it is self-contained against Boole's own family doc.
  - `argparse --benchmark-mode` choices extended from `["mining", "smoke"]` to `["mining", "mining-v2", "smoke"]`. Help text updated to call out the v031-lp lengthPreserved family.
- `docs/benchmark-target-families.md` — inserted a `## boole.calibration.pow.v2` section before the existing `## boole.smoke.true.v1` section. Documents Mode (`mining-v2`), Family origin (pof `v031-lp`, narrowed to N=1 lp-only), Theorem template, Canonical proof shape (right-folded `Eq.trans` of `length_*` lemmas), Lean library reference (`Boole.Family.V0Helpers`), Why non-rfl-equivalent, Difficulty (chainLen ∈ [1, 6], op set {`mapAdd`, `mapMul`, `sortAsc`}), Verifier hash (deferred to a future `verifier-hashes.json` `active` bump), Acceptance criteria, Known limitations (no truthy/dedup branches yet, single-template-per-attempt). The CI lint at `scripts/test_model_benchmark.py::TargetFamilyDocLintTests` automatically picks up the new literal — no doc-lint code change required.
- `scripts/test_model_benchmark.py` — added `MiningV2FamilyTests` (9 tests):
  - `test_v2_mining_mode_selects_v2_target_family` — `target_family_for_mode("mining-v2") == "boole.calibration.pow.v2"`.
  - `test_v2_mining_attempt_context_carries_lp_chain_and_canonical_proof` — context shape: `benchmarkMode`, `targetFamily`, `theoremName == "boole_benchmark_pow_v2_target_1"`, `lpChain` non-empty, `lpChainLen == len(lpChain)`, every op ∈ `{mapAdd, mapMul, sortAsc}`, integer `k` for `mapAdd`/`mapMul`.
  - `test_v2_mining_canonical_proofs_differ_per_challenge_and_are_non_rfl_equivalent` — 5 distinct `attempt_index` values produce 5 distinct `chainExpr` AND 5 distinct `canonicalProof`; every canonical proof contains `length_` (non-rfl evidence); none equals an `rfl` synonym.
  - `test_v2_mining_canonical_proof_uses_eq_trans_for_long_chains` — finds an attempt with `lpChainLen ≥ 2` in 20 deterministic samples (different runId so chain distribution is independent), asserts `.trans` appears in `canonicalProof`.
  - `test_v2_mining_canonical_proof_omits_eq_trans_for_singleton_chain` — finds an attempt with `lpChainLen == 1` in 50 deterministic samples (different runId again), asserts `.trans` absent and `length_` present.
  - `test_v2_mining_wrap_proof_term_candidate_emits_lp_world_and_theorem` — wrapped candidate contains `import Boole.Family.V0Helpers`, `open Boole.Family.V0Helpers`, the theorem signature `∀ (xs : List Int)` + `.length = xs.length`, the lottery-binding comment lines, the chainExpr advertised by the context, and is free of v1 strings (`"<challenge>" = "<challenge>"`, `boole_benchmark_true`).
  - `test_v2_mining_prompt_describes_lp_chain_and_helpers` — prompt names `boole.calibration.pow.v2`, the chainExpr, `Boole.Family.V0Helpers`, the three helper names; does not leak `boole_benchmark_true` or `True.intro` from other modes.
  - `test_argparse_accepts_mining_v2_choice` — `python3 boole-model-benchmark.py --benchmark-mode mining-v2 --help` exits 0.
  - `test_v2_mining_golden_fixture_matches` — loads `fixtures/benchmarks/v031-lp/golden-instances.json`, regenerates each instance's `(chainExpr, canonicalProof, wrappedCandidate)` from its stored seeds, asserts byte-identity. This is the deterministic-replay regression that catches any cursor-algorithm or canonical-proof-emitter drift.

**Acceptance evidence:**
- `python3 -m pytest scripts/test_model_benchmark.py -p no:anchorpy` → 40/40 PASS (9 new MiningV2 + all existing v1, smoke, doc-lint, B3, B4, B5, B7(a), C0/C1/C2 surface tests).
- `lake build Boole` (in `lean/checker/`) → succeeds, builds `Boole.Family.V0Helpers` cleanly.
- Lean elaboration smoke for fixture seed 0: materialized the `wrappedCandidate` for `attemptIndex=0` (chainLen=2: `(mapMul (3 : Int) (mapAdd (-2 : Int) xs)).length = xs.length`, proof `fun xs => (length_mapMul (3 : Int) _).trans (length_mapAdd (-2 : Int) xs)`) to `/tmp/boole-v2-fixture-seed0.lean` and ran `lake exec boole_check /tmp/boole-v2-fixture-seed0.lean` → exit 0.
- Manual chainLen=1 + chainLen=3 smoke earlier in development (`/tmp/boole-v2-lib-smoke.lean`) elaborated cleanly through `lake env lean` and `lake exec boole_check` (exit 0 each).
- v1 byte-frozen: existing Slice-S4 v1 family-doc + `boole_benchmark_pow_target_<n>` shape tests are unchanged and pass.
- Red-state proof: with the v2 branches stashed and only `MiningV2FamilyTests` + the golden fixture present, all 9 v2 tests fail (mode dispatch returns the v1 family, attempt_context lacks `lpChain`, wrap returns the v1 challenge-equality template, etc.). Restoring the implementation flips them 9/9 Green.

**Deviations from plan §Appendix A · S8 (and §3 B1+B7(b)):**
1. **Narrowed to N=1 lp-only at first ship.** The pof v031 family covers N≥1 with multiple invariant kinds (lengthPreserved, all-filterByPred, nodup-dedup, pairwise-sortAsc, partition-equality). Boole S8 ships only the lengthPreserved branch over `{mapAdd, mapMul, sortAsc}`. Rationale: this branch alone is non-`rfl`-equivalent and shippable end-to-end against Lean 4.29.1 today; the four truthy branches require porting `nodup_dedup` + `pairwise_sortAsc` + `partition_eq_filter_filter` from pof's `truthy_*` lemma set, which expands the Lean-library surface significantly without changing the B1+B7(b) CI gate. A future "Slice S8b" (or fold into S9+) can add the truthy branches by appending to `Boole.Family.V0Helpers` and extending `_v031lp_generate` to dispatch on `pred` kind — no refactor required.
2. **Cursor preserves byte alignment for unused positions.** Even though N=1 lp-only never reads `pred kind` / `inputLen` / `noiseDefs`, `_v031lp_generate` still consumes those 3 byte positions from the cursor (with the same `mod_n` values pof uses: 6, 5, 5). Rationale: when the truthy branches land later, their attempts must produce byte-identical chain/proof pairs for the same seeds — otherwise the existing `boole.calibration.pow.v2` golden fixture would silently invalidate. Forward-compat alignment is cheaper to preserve up front than to retrofit.
3. **Verifier hash deferred (no `active` bump in `verifier-hashes.json`).** S5 versioned the verifier hash file, and the natural pairing was for S8 to bump `active: "v0"` → `active: "v1"` with a new `boole.calibration.pow.v2`-keyed hash. Deferred because v2 has no live-evidence run yet — the hash should pin to a real `(family, prompt, wrap, lake-toolchain)` tuple measured against a 50-attempt run, not a synthetic test-time placeholder. The doc explicitly flags this in the v2 section's "Verifier hash" line. When the live run lands, bumping `active` is a fixture-only edit (no code change in the resolver — that contract is from S5).
4. **Single-template-per-attempt limitation kept.** v1 had this limitation, v2 inherits it: each attempt advertises *one* theorem statement and the model must close *that* statement exactly. The pof reference allows multi-template (offer the model a menu and accept any closure), but Boole's `wrap_proof_term_candidate` and the node `/submit` admission both pin to a single theorem name. The B1+B7(b) "menu" semantics is a follow-up slice (likely tied to a richer `attempt_context` that emits a list of theoremStatements, not a singleton).
5. **Live evidence is not a CI gate.** The plan lists "non-zero `blocksProduced` for at least one model AND a per-model spread of ≥10pp across opus/sonnet/gemma" as B1+B7(b) acceptance. Per the plan's own §7 split, this is *Gate B-Live* (manual evidence, not CI). The deterministic-CI portion is what's gated and what S8 ships. Live evidence stays in `artifacts/model-benchmarks/` as a non-blocking record once a real 50-attempt run is performed.
6. **Library namespace is `Boole.Family.V0Helpers`, not `BooleFamily.V0Helpers` or `Boole.Bench.V031Lp`.** Matches pof's exact namespace (which `lakefile.lean`'s `globs := #[.submodules \`Boole.Family]` directly mirrors). Picking pof's namespace verbatim means a future merge of pof's truthy lemmas is a literal copy + paste — no rename pass needed. The cost is owning a shared namespace prefix with pof for the lifetime of both projects, which is acceptable because the prefix names a *family contract*, not a vendor.
7. **No `verifier.invoked` change.** The B1+B7(b) work is purely on the *target family generation* side — the verifier path (`scripts/lean-checker.sh` → `lake exec boole_check`) is unchanged. v2 candidates flow through the same wrapper path as v1 candidates, with the v2 wrap emitting the new module shape. Decoupling family-side changes from verifier-side changes was deliberate: it lets the v2 doc + tests + fixture land without touching any code that historical v1 rows replayed against.

**Downstream unblocked:**
- **Truthy branch port (Slice S8b / S9+)** — extend `Boole.Family.V0Helpers` with `all_filterByPred_self`, `nodup_dedup`, `pairwise_sortAsc`, `partition_eq_filter_filter` (direct ports from pof v0.2). Extend `_v031lp_generate` to read a `pred kind` byte and dispatch to a per-branch chain emitter. The cursor-alignment work in S8 means existing v2 chains stay byte-identical; the new branches only activate when the `pred kind` byte selects them.
- **Live-run evidence + verifier-hash bump (Slice S8c / live)** — once a 50-attempt v2 run lands, snapshot its `(family, prompt, wrap, lake-toolchain)` tuple → blake3 → publish under `versions: { "v1": "<new-hash>" }` in `fixtures/benchmarks/verifier-hashes.json` and bump `active` to `"v1"`. Resolver code from S5 picks it up automatically.
- **Multi-template menu surface (future B-track)** — when `attempt_context` returns a list of theoremStatements, `wrap_proof_term_candidate` becomes a per-statement renderer and the node `/submit` admission learns to match by theorem name from a closed set. Today's single-template path is the exact restriction this future surface will relax.
- **Calibration-difficulty knob** — `_v031lp_generate` reads `D = (cursor_read 6) + 1` so chainLen ∈ [1, 6]. A future difficulty-tuning slice can swap the `6` for a `cli --chain-max N` knob without any algorithm change. The constant lives at one site.

**Open follow-ups (not blocking S8):**
- **N≥2 chain expansion** — pof's full N≥2 emitter composes multiple branches per attempt (e.g. lengthPreserved + nodup-dedup chained over the same input). Out of scope for S8; activates when the truthy branches ship.
- **Toolchain-pin lemma surface** — `length_sortAsc` relies on `List.length_mergeSort _` from Lean core. If the Lean toolchain ever bumps and that lemma is renamed/deprecated, the proof emits would break for any historical chain that used `sortAsc`. The right defense is a `lean-toolchain` pin + a CI step that re-elaborates the golden fixture against the pinned toolchain on every PR. Today's `lake build Boole` step covers the toolchain-resolves side; the elaboration regression on golden seeds is a one-line addition to CI when the time comes.
- **Live-run cost accounting** — a 50-attempt run across opus/sonnet/gemma at v2's chainLen distribution will burn more model tokens per attempt than v1 (longer prompt, longer canonical proof to imitate). Worth measuring once before recurring weekly runs land.
- **`v2`-aware `boole submit-lean` smoke** — the existing submit-lean smoke targets v1 challenge-equality theorems. A v2-shaped smoke (single attempt, deterministic seed, single theorem) would prove the full benchmark → wrap → `lake exec boole_check` → node `/submit` path end-to-end on v2. Cheap addition once the live-run cadence is established.

### Slice S8b [B] — B1 + B7(b) v3 mixed-family expansion — SHIPPED 2026-05-09

**Outcome:** Gate B B1+B7(b) deterministic-CI extended without disturbing v1/v2. `boole.calibration.pow.v3` ships as a new `targetFamily` selectable via `--benchmark-mode mining-v3`. Each per-attempt theorem is selected from one of `{allSatisfy, sortedAsc, dedupFirst, partitionEq, lengthPreserved}` and closed by the corresponding witness lemma in `Boole.Family.V0Helpers` (`all_filterByPred_self`, `pairwise_sortAsc`, `nodup_dedup`, `partition_eq_filter_filter`) — or, for `lengthPreserved`, the v2 `Eq.trans` shape over `length_*` lemmas. v1 + v2 stay byte-frozen via separate cursor-seed strings (`|v031-mixed-cursor` for v3, `|v031-lp-cursor` for v2) and separate target families.

**Files added:**
- `fixtures/benchmarks/v031-mixed/golden-instances.json` (NEW, 10 instances) — Deterministic regression fixture under `runId="ci-v3-fixture"` / `target="ollama:test"`. Each entry stores `attemptIndex`, `benchmarkMode="mining-v3"`, derived `(challenge, nonce, theoremName, invariantClass, invariantPred, branchChain, bodyChain, witnessOp, chainLen, D, bodyExpr, resultExpr, theoremRhs, canonicalProof, wrappedCandidate)`. Greedy attempt-index walk picks the smallest set that covers all 5 invariant classes plus chainLen=1 and chainLen≥3 cases. Replay regenerates from stored seeds and asserts byte-identical `(theoremRhs, canonicalProof, wrappedCandidate)` tuples.

**Files modified:**
- `lean/checker/Boole/Family/V0Helpers.lean` — extended (verbatim port from `projects/pof/lean/Boole/Family/V0Helpers.lean`):
  - New op definitions: `@[reducible] def filterByPred (p) (xs) := xs.filter p`, `@[reducible] def dedup (xs) := xs.eraseDups`.
  - New witness lemma `all_filterByPred_self` — `(filterByPred p xs).all p = true`, closed by `unfold filterByPred; simp [List.all_filter, Bool.not_or_self]`.
  - New witness lemma `nodup_dedup` — `List.Nodup (dedup xs)`, well-founded recursion (`termination_by xs => xs.length`, `decreasing_by simp_wf; exact Nat.lt_succ_of_le (List.length_filter_le _ as)`), structural cons-step via `List.eraseDups_cons` + `List.Pairwise.cons` + filtered membership symmetry.
  - New witness lemma `pairwise_sortAsc` — `List.Pairwise (· ≤ ·) (sortAsc xs)`, applies `List.pairwise_mergeSort` with `htrans` (Int omega) + `htotal` (split on `a ≤ b`) and `.imp` to strip `decide`.
  - New witness lemma `partition_eq_filter_filter` — `xs.partition p = (xs.filter p, xs.filter (fun x => !(p x)))`, lifted from `List.partition_eq_filter_filter` and rewriting `(not ∘ p) = (fun x => !p x)` via `rfl` so the lambda shape matches the v3 generator's render.
- `scripts/boole-model-benchmark.py` —
  - New module-level constant `MINING_V3_TARGET_FAMILY = "boole.calibration.pow.v3"`.
  - `target_family_for_mode("mining-v3")` returns the v3 family. v2/v1/smoke paths unchanged.
  - 13 new `_v031mixed_*` private helpers ported from pof's `genBranchV031` + Pred/Op/InvariantClass renderers:
    - `_v031mixed_gen_pred(buf, pos)` — 6-way pred kind dispatch (even / odd / ltK / gtK / eqK / modK), each consuming the appropriate cursor reads via `_v031lp_cursor_read` + `_v031lp_choose_int` (shared with v2 — single source of truth for the cursor primitives).
    - `_v031mixed_gen_op(buf, pos)` — 5-way op dispatch (filterP / mapAdd / mapMul / dedup / sortAsc) over the full v0.2 op set.
    - `_v031mixed_gen_inv_class(buf, pos)` — 5-way invariant class selector (allSatisfy / sortedAsc / dedupFirst / partitionEq / lengthPreserved). For the three pred-bearing classes (allSatisfy, partitionEq via different shape) emits a sub-pred via `_v031mixed_gen_pred`.
    - `_v031mixed_witness_op(inv_class)` — returns the trailing op that closes the chain for truthy branches: `{op:"filterP", pred}` for allSatisfy, `{op:"sortAsc"}` for sortedAsc, `{op:"dedup"}` for dedupFirst, `None` for partitionEq + lengthPreserved.
    - `_v031mixed_gen_branch(chain_len, buf, pos)` — top-level branch constructor: read invClass, then if witness op required produce `chain_len - 1` body ops + append the witness; otherwise produce `chain_len` body ops directly. Returns `{invClass, branchChain, bodyChain, witnessOp, chainLen}`.
    - `_v031mixed_generate(seed_bytes)` — reads `D = (cursor_read 6) + 1`, calls `_v031mixed_gen_branch`, then consumes 3 forward-compat positions (legacy pred kind / inputLen / noiseDefs) so the cursor is byte-equivalent to pof's `genBranchV031`.
    - `_v031mixed_render_int(k)` — `(k : Int)` ascription for non-negative literals.
    - `_v031mixed_render_pred_to_bool(pred)` / `_v031mixed_render_pred_to_bool_not(pred)` — emits Lean lambdas for each of the 6 pred kinds (matches pof's `Pred.toBoolFn` / `toBoolNot` byte-for-byte).
    - `_v031mixed_render_op_apply(op, inner)` — renders `(filterByPred <pBool> inner)` / `(mapAdd k inner)` / `(mapMul k inner)` / `(dedup inner)` / `(sortAsc inner)`.
    - `_v031mixed_render_chain_expr(chain, var)` — left-fold `_v031mixed_render_op_apply` over a chain.
    - `_v031mixed_render_theorem_rhs(inv_class, result_expr)` — emits the invariant-specific theorem RHS: `<resultExpr>.all <pBool> = true` / `List.Pairwise (· ≤ ·) <resultExpr>` / `List.Nodup <resultExpr>` / `<resultExpr>.partition <pBool> = (<resultExpr>.filter <pBool>, <resultExpr>.filter <pNot>)` / `<resultExpr>.length = xs.length`.
    - `_v031mixed_render_canonical_proof(inv_class, body_chain, body_expr)` — emits the canonical witness application: `fun xs => <witness_lemma> <pBool>? <body_expr>` for truthy branches; for lengthPreserved falls through to the v2 `Eq.trans` chain over `length_*` lemmas.
  - `attempt_context(...)` extended with a `"mining-v3"` branch: derives `cursor_seed = sha256(seed + "|v031-mixed-cursor").digest()`, calls `_v031mixed_generate`, builds `bodyExpr / resultExpr / theoremRhs / canonicalProof`, and returns the v3 context dict (`benchmarkMode, targetFamily, attemptIndex, challenge, nonce, theoremName="boole_benchmark_pow_v3_target_<n>", invariantClass, invariantPred, branchChain, bodyChain, witnessOp, chainLen, D, bodyExpr, resultExpr, theoremRhs, canonicalProof`). v2/v1 paths unchanged.
  - `wrap_proof_term_candidate(...)` extended with a `"mining-v3"` branch emitting a full Lean module with extended lottery-binding comments (`-- benchmarkMode: mining-v3`, `-- targetFamily: boole.calibration.pow.v3`, `-- lotteryChallenge: …`, `-- lotteryNonce: …`, `-- invariantClass: <class>`, `-- chainLen: N`, `-- D: D`), then `import Boole.Family.V0Helpers`, `namespace <theoremName>`, `open Boole.Family.V0Helpers`, then `theorem <theoremName> : ∀ (xs : List Int), <theoremRhs> := <indented proof>`, then `end <theoremName>`.
  - `model_proof_term_prompt(...)` extended with a `"mining-v3"` branch describing the v031 mixed contract: lists all 5 invariant classes, the per-attempt invariant + theorem RHS, the helpers (`filterByPred`, `mapAdd`, `mapMul`, `dedup`, `sortAsc`) and the witness lemmas (`all_filterByPred_self`, `pairwise_sortAsc`, `nodup_dedup`, `partition_eq_filter_filter`, `length_*`), and a worked example per branch.
  - `argparse --benchmark-mode` choices extended from `["mining", "mining-v2", "smoke"]` to `["mining", "mining-v2", "mining-v3", "smoke"]`. Help text updated.
- `docs/benchmark-target-families.md` — inserted a `## boole.calibration.pow.v3` section before the existing `## boole.smoke.true.v1` section. Documents Mode (`mining-v3`), Family origin (pof v031 mixed N=1 5-way), Theorem template per branch, Canonical proof shape per branch, Pred → Bool table, Lean library, why each branch is non-rfl-equivalent, difficulty (chainLen ∈ [1, 6], full v0.2 op set + 6 pred kinds, 5-way invariant), Verifier hash (deferred), Acceptance criteria, Known limitations, Forward-compat cursor.
- `scripts/test_model_benchmark.py` — added `MiningV3FamilyTests` (10 tests):
  - `test_v3_mining_mode_selects_v3_target_family` — `target_family_for_mode("mining-v3") == "boole.calibration.pow.v3"`.
  - `test_v3_mining_attempt_context_carries_v3_fields` — context shape: `benchmarkMode`, `targetFamily`, `theoremName == "boole_benchmark_pow_v3_target_1"`, every v3-specific key present, `invariantClass ∈ {allSatisfy, sortedAsc, dedupFirst, partitionEq, lengthPreserved}`, `chainLen >= 1`, `chainLen == len(branchChain)`.
  - `test_v3_mining_canonical_proofs_use_invariant_specific_witness_lemma` — for each attempt in a 50-sample window the canonical proof must contain the witness lemma matching its invariant class.
  - `test_v3_mining_5_way_invariant_coverage_in_50_samples` — 50 deterministic samples cover all 5 invariant classes.
  - `test_v3_mining_canonical_proof_is_non_rfl_equivalent` — across 5 contexts no canonical proof is an rfl synonym.
  - `test_v3_mining_lengthPreserved_branch_uses_eq_trans_for_long_chains` — at least one lengthPreserved chainLen≥2 attempt in 80 samples uses `.trans`.
  - `test_v3_mining_wrap_proof_term_candidate_emits_v3_world_and_theorem` — wrapped candidate contains the v3 imports, theorem signature, the extended lottery-binding comments, the `theoremRhs`, and is free of v1/v2 strings.
  - `test_v3_mining_prompt_describes_5_branches_and_helpers` — prompt names the v3 target family, the V0Helpers library, all 4 truthy witness lemmas, the `length_` family, all 5 op helpers, the current attempt's invariant class + theorem RHS; does not leak `boole_benchmark_true` or `True.intro`.
  - `test_argparse_accepts_mining_v3_choice` — `python3 boole-model-benchmark.py --benchmark-mode mining-v3 --help` exits 0.
  - `test_v3_mining_golden_fixture_matches` — loads `fixtures/benchmarks/v031-mixed/golden-instances.json`, regenerates each instance's `(theoremRhs, canonicalProof, wrappedCandidate)` from its stored seeds, asserts byte-identity, asserts ≥1 entry per invariant class plus at least one chainLen=1 and one chainLen≥3 case.

**Acceptance evidence:**
- `python3 -m unittest scripts.test_model_benchmark` → 50/50 PASS (10 new MiningV3 + 9 existing MiningV2 + all v1/smoke/doc-lint/B3/B4/B5/B7(a)/C0/C1/C2 tests).
- `lake build Boole` (in `lean/checker/`) → succeeds, builds the extended `Boole.Family.V0Helpers` cleanly.
- Lean elaboration smoke for one wrapped candidate per invariant class: materialized `wrappedCandidate` from the first golden-fixture entry of each invariant to `/tmp/boole-v3-fixture-<inv>.lean` (5 files: dedupFirst chainLen=2, partitionEq chainLen=3, allSatisfy chainLen=6, lengthPreserved chainLen=6, sortedAsc chainLen=1) and ran `lake exec boole_check` for each → exit 0 for all 5.
- Manual chainLen=1 + chainLen=3 helper-library smoke (`/tmp/boole-v3-helpers-smoke.lean`, 8 theorems exercising every witness lemma + length composition) elaborated cleanly through `lake exec boole_check` (exit 0).
- v1 + v2 byte-frozen: existing Slice-S4 v1 family-doc + Slice-S8 v2 fixture replay tests are unchanged and pass.
- Red-state proof: with the v3 branches stashed and only `MiningV3FamilyTests` + the golden fixture present, all 10 v3 tests fail with `SystemExit: unsupported benchmark mode: mining-v3`. Restoring the implementation flips them 10/10 Green.

**Deviations from plan §Appendix A · S8b (and §3 B1+B7(b)):**
1. **v3 ships as a separate target family, not a v2 in-place extension.** The S8 plan's "Truthy branch port" sketch suggested folding the truthy branches into `_v031lp_generate` via a pred-kind sub-cursor. S8b instead introduces a separate `boole.calibration.pow.v3` family with its own cursor seed string (`|v031-mixed-cursor`) so v2's golden fixture stays byte-identical. The cost is one extra `target_family_for_mode` branch and one extra wrap/prompt branch; the gain is that historical v2 rows replay byte-for-byte without any compatibility scaffolding.
2. **Boole prepends a `D`-byte before all subsequent reads (matches v2 design).** Pof receives `D` as a parameter to `genBranchV031`; Boole derives `D = (cursor_read 6) + 1` from the seed first. Once `D` is fixed, every subsequent read (invClass, optional pred, body ops, witness op append, forward-compat 3 trailing reads) is byte-identical to pof. This convention was set in S8 and S8b inherits it.
3. **Witness op is generated alongside the invariant class, not implicitly at proof-emit time.** Pof's `ListInvariantsV031.lean` makes the witness op part of the chain dynamically (the proof inspects the invariant class). Boole splits the chain into `branchChain` (full chain incl. witness) and `bodyChain` (chain minus witness) at generation time so the canonical proof emitter can use the body chain as the implicit-arg position to the witness lemma. This is a structural simplification with no semantic divergence.
4. **`partitionEq` pred lambda is rendered as `fun x => !(p x)`, not `(not ∘ p)`.** Pof's `List.partition_eq_filter_filter` returns `(filter p l, filter (not ∘ p) l)`. The v3 theorem RHS uses the explicit lambda shape because it reads more naturally to a non-Lean-expert prompt audience. The lemma proof in `V0Helpers.lean` rewrites `(not ∘ p) = (fun x => !p x)` via `rfl` to bridge the two shapes — so the canonical proof closes the lambda-form RHS without any user-visible change.
5. **Library namespace stays `Boole.Family.V0Helpers`.** S8 picked it; S8b inherits it. The four new witness lemmas live alongside the three existing `length_*` lemmas in the same module — no separate `V0HelpersTruthy` module needed because the lemma surface is small and tightly coupled.
6. **No `verifier.invoked` change.** Same as S8: the verifier path is unchanged. v3 candidates flow through the same `lake exec boole_check` wrapper as v2 candidates.
7. **Verifier hash deferred.** Same rationale as S8: the hash should pin to a real 50-attempt live run, not a synthetic test placeholder. Doc explicitly flags this in the v3 section's "Verifier hash" line.
8. **Live evidence is not a CI gate.** Same split as S8: deterministic-CI via `MiningV3FamilyTests` + golden fixture; live 50-attempt evidence is a manual gate stored in `artifacts/model-benchmarks/`.

**Downstream unblocked:**
- **N≥2 chain expansion (Slice S9+)** — pof's full N≥2 emitter chains multiple branches per attempt over the same input. The v3 single-branch generator is the building block; multi-branch composition is a matter of running `_v031mixed_gen_branch` N times with a per-attempt N read from the cursor. The truthy lemma surface is already in place.
- **Verifier-hash bump (Slice S8c / live)** — once a 50-attempt v3 run lands, snapshot its `(family, prompt, wrap, lake-toolchain)` tuple → blake3 → publish under `versions: { "v2": "<new-hash>" }` in `fixtures/benchmarks/verifier-hashes.json` and bump `active`. v2 + v3 hashes can be added in the same edit.
- **Multi-template menu surface** — same future B-track item that S8 surfaced; v3 inherits the single-template-per-attempt limitation and benefits identically from a future menu surface.
- **Cross-family difficulty calibration** — once both v2 (lp-only) and v3 (5-way mixed) ship, a future calibration slice can measure per-family solve rates and tune `D = (cursor_read M) + 1` independently per family without algorithm changes.

**Open follow-ups (not blocking S8b):**
- **Live-run cost accounting for v3** — v3 prompts list 5 worked examples + 4 witness lemma names + helper names, so per-attempt token cost exceeds v2. Worth measuring once before recurring weekly runs land.
- **`v3`-aware `boole submit-lean` smoke** — the existing submit-lean smoke targets v1 challenge-equality theorems. A v3-shaped smoke (one attempt per invariant class) would prove the full benchmark → wrap → `lake exec boole_check` → node `/submit` path on v3.
- **Toolchain-pin lemma surface for v3** — extends the S8 concern to four more stdlib lemmas: `List.all_filter`, `Bool.not_or_self`, `List.eraseDups_cons`, `List.pairwise_mergeSort`, `List.partition_eq_filter_filter`, `List.length_filter_le`. If the Lean toolchain bumps and any of these are renamed, v3 proof emits break for any historical chain that used them. The same `lean-toolchain` pin + golden-fixture re-elaboration CI step covers v2 and v3 simultaneously.

### Slice S19 [C4] — Mining loop + CLI wiring (`boole mine start` / `boole mine bounty`) — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-miner/src/mining_loop.rs` — new orchestration module: `MiningLoopDeps { pk, chain_head, emitter, driver, verifier, canonicalizer, submit_client, prompt_builder, log, sleeper }`, `MiningLoopOptions { max_cycles, max_shares, ... }`, `run_mining_loop(deps, opts) -> MiningSummary`, `MiningEvent` enum, and `pub type LogSink = Box<dyn Fn(&MiningEvent) + Send + Sync>` (extracted to satisfy clippy `type_complexity`). The core loop performs `fetch_head → grind_ticket → announce_ticket → for j ∈ [0, M): emit → with_retry(driver.generate) → verify → canonicalize → grind_share → grind_submission_pow → submit`, with retry-on-network-error semantics and per-cycle stop conditions.
- `crates/boole-miner/src/cli.rs` — added `start` handler owning `--max-cycles`, `--max-shares`, `--head-timeout-ms`, `--mock-llm-response`, `--mock-verify-accept`, and the paired `--fixed-target-seed-hex` / `--fixed-target-render` (handler enforces "must be provided together"); resolves the configured driver/emitter/verifier from miner state (defaults to `AcceptingVerifier` without `lake-verify`); prints a final `summary: <json>` envelope on stdout for line scrapers. Clippy fixes during slice: `unnecessary_cast` at cli.rs:190 (removed redundant cast on `doy: u32`), `collapsible_if` at cli.rs:293 (collapsed `!reveal && api_key.is_some()`), `manual_contains` at cli.rs:355 (`SECRET_KEYS.contains(&key)`).
- `crates/boole-miner/tests/mining_loop.rs` — 14 lib tests covering the full pipeline with stub collaborators (`RecordingSubmitter` + `ArcSubmitter` wrapper for shared-mutability under the boxed-trait boundary): head-fetch failure path, ticket-grind path, per-j emit/generate/verify/canonicalize/share-grind/submission-PoW path, max-cycles termination, max-shares termination, retry-on-network-error, summary aggregation.
- `crates/boole-cli/Cargo.toml` — added `boole-miner = { path = "../boole-miner" }` between `boole-core` and `clap`.
- `crates/boole-cli/src/main.rs` — added `Mine { #[command(subcommand)] command: boole_miner::cli::MineCommand }` variant after the `Keys` enum and a matching `Some(Command::Mine { command }) => boole_miner::cli::run_mine(command)` arm so the standalone `boole-miner` binary and the `boole mine ...` subcommand share the same code paths (no subprocess fan-out).
- `crates/boole-cli/tests/mine_init_cli.rs` — 5 CLI tests: `mine_init_creates_state_and_prints_address`, `mine_address_prints_pk_hex`, `mine_init_refuses_to_overwrite_without_force`, `mine_config_get_set_round_trips`, `mine_config_get_redacts_secret_by_default`. Uses `fresh_state_path(label)` helper for tmpdir isolation.
- `crates/boole-cli/tests/mine_bounty_cli.rs` — 2 CLI tests: `mine_bounty_submits_envelope_and_prints_ok_envelope` (boots local node with mock-accept verifier via `serve_local_node` + `LocalNodeConfig` from `boole_node::local_node`, writes envelope to file, runs `mine bounty --node URL --id gamma-1 --prover PROVER_X --envelope-path <path>`, asserts `parsed["bounty"]["status"] == "solved"`); `mine_bounty_rejects_malformed_prover_locally` (asserts stderr contains "bad_prover" for non-hex prover).
- `crates/boole-cli/tests/mine_start_cli.rs` — 2 CLI tests: `mine_start_exits_after_max_cycles_when_head_fetch_fails` (uses `closed_port_url()` helper that binds + drops a `TcpListener` to obtain a guaranteed-closed port, runs `mine start --max-cycles 1 --head-timeout-ms 100`, asserts `summary.cyclesRun == 1`, `networkErrors >= 1`); `mine_start_rejects_unpaired_fixed_target_flags` (asserts stderr contains "must be provided together").
- `docs/boole-node-cli-parity-plan.md` — §1.2 mine rows flipped to `✓` + match-S19 notes, §1.3 miner-loop bullet flipped to DELIVERED, §1.4 dashboard miner row → PASS (cargo) / PARTIAL (live evidence), §3 Phase C4 → PASS, §4 Rust miner crate header → SHIPPED 2026-05-09 (Slices S15-S19) with full module map + feature gates + acceptance, Gate P5 PARTIAL PASS, Gate P6 C4 PASS line, Appendix A · S15-S19 entries, this §11 ship log entry.
- `tasks/todo.md` — S19 Review section populated post-Green.

**Behavior changes (operator-visible):**
- New `boole mine start` subcommand. Drives `mining_loop::run_mining_loop` end-to-end. `--max-cycles 0` (default) means "until external interrupt"; `--max-cycles N` stops after N cycles. `--max-shares 0` (default) means "no share-count cap"; `--max-shares N` stops after N accepted shares. `--head-timeout-ms <ms>` overrides the default `HttpHead` timeout for fail-fast smoke-test scenarios. Smoke knobs: `--mock-llm-response <text>` (skip live LLM, replay a fixed candidate), `--mock-verify-accept` (skip lake verifier, accept everything — same as no-`lake-verify` build), `--fixed-target-seed-hex <hex> --fixed-target-render <text>` (skip emitter, hand the pair directly to the prompt builder).
- Final stdout envelope shape: `summary: {"cyclesRun": <u64>, "sharesAccepted": <u64>, "ticketsFound": <u64>, "networkErrors": <u64>, "lastError": <opt-string>, ...}`. The literal `summary:` prefix is the line-scraper anchor.
- New `boole mine bounty` subcommand (re-exposed from S16's `cli::bounty` handler — no behavioral change vs. the standalone `boole-miner bounty` binary).

**Implementation notes:**
- `run_mining_loop` is sync (no `tokio` runtime spin-up) — every collaborator is a sync trait. The `LogSink` type alias bounds `Send + Sync` because the Box is shared across the recovery boundary inside `run_mining_loop` for retry-attempt tracing.
- The CLI smoke test for `mine start` deliberately uses a closed-port head fetch (bind + drop a `TcpListener` to obtain an addr that's almost certainly free) — this gives a deterministic fail-fast path through `with_retry → networkError → cycle exit` in <200ms without needing a stub HTTP server in-process. Full pipeline E2E coverage with stub collaborators is in `boole-miner/tests/mining_loop.rs`; the CLI smoke tests only verify argument parsing + summary envelope shape.
- `MiningLoopDeps` boxes every collaborator behind `dyn Trait` rather than monomorphizing, because the cli::start handler needs to swap real vs mock implementations at runtime based on flags. Compile-time monomorphization wasn't worth the duplicated `cli::start` handler.

**Acceptance:**
- `cargo test -p boole-miner --test target_emitter --test local_verify --test mining_loop` PASS (S18 + S19 lib coverage).
- `cargo test -p boole-cli` PASS (9 mine-CLI integration tests across `mine_init_cli.rs`, `mine_bounty_cli.rs`, `mine_start_cli.rs`).
- `cargo clippy -p boole-miner -p boole-cli --all-targets -- -D warnings` clean after the 4 fixes.
- Phase C4 PASS, Gate P5 PARTIAL PASS, Gate P6 C4 PASS.

**Deferrals:**
- Live-model evidence — `scripts/local-mining-smoke.sh` against a real `boole node start` with at least one of `claude_cli` / `ollama_local` / `anthropic_api` driver producing ≥1 accepted share. Gate P5 stays PARTIAL until that lands.
- Lake-feature CI gating — `lake-canon` / `lake-target` / `lake-verify` are OFF by default; a separate CI job that builds with all three ON and runs the suite is deferred to a follow-up slice once the feature flags are wired into the workspace `[features]`.

### Slice S18 [C4] — Target emitter + local verify + prover driver + canonicalizer — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-miner/src/target_emitter.rs` — new module: `TargetEmitter` trait + deterministic `boole.calibration.pow.v3` emitter behind feature `lake-target` (without the feature, the trait still compiles but only the fixture-driven `--fixed-target-{seed-hex,render}` path is reachable from the CLI).
- `crates/boole-miner/src/local_verify.rs` — new module: `Verifier` trait + `AcceptingVerifier` (default fallback when feature `lake-verify` is OFF — every candidate passes) + `LakeVerifier` (behind `lake-verify`, runs `lake exec boole_check` for cheap-reject before sharing).
- `crates/boole-miner/src/llm_driver.rs` — new module: `ProverDriver` trait + HTTP and process-spawn drivers + `MockProverDriver` for tests (honors `--mock-llm-response` for CLI smoke tests).
- `crates/boole-miner/src/canonicalizer/` — new sub-module (`mod.rs`, `structural.rs`): `Canonicalizer` trait + structural canonicalizer; lake-backed canonicalizer engaged behind feature `lake-canon`.

**Acceptance:** `cargo test -p boole-miner --test target_emitter --test local_verify` PASS plus per-module unit tests. Feeds S19's mining loop.

### Slice S17 [C4] — Chain-head fetch + share grinder + proof-package envelope assembly — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-miner/src/chain_head.rs` — new module: `ChainHeadFetcher` trait + `HttpHead` blocking-HTTP impl for `GET /head`; honors `--head-timeout-ms` for fail-fast smoke testing.
- `crates/boole-miner/src/grinder/share.rs` — new module under the `grinder/` sub-tree: finds a share that survives `share_hash < T_share`. Per-attempt nonce iteration uses `Hex32` byte representation (Boole consensus deviation from pof's 8/4/8-byte nonces — `Hex32` is the canonical wire format across Boole's hash surface).
- `crates/boole-miner/src/proof_package.rs` — new module: assembles the submission envelope (ticket + target + canonical proof + share + submission PoW) before the submit client signs and POSTs.

**Acceptance:** lib tests cover `HttpHead` timeout semantics, share-grinder round-trip against a known target, and proof-package shape. Feeds S19's mining loop. `cargo test -p boole-miner` PASS.

### Slice S16 [C4] — Ticket + submit_pow grinders + submit/bounty HTTP clients + `boole mine bounty` driver — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-miner/src/grinder/` — new sub-module (`mod.rs`, `ticket.rs`, `submit_pow.rs`): ticket grinder finds `n` with `ticket_hash < T_ticket`; submission-PoW grinder finds `nonce_s` with `submission_pow_hash(submission_envelope, nonce_s) < T_submit`.
- `crates/boole-miner/src/submit_client.rs` — new module: `Submitter` trait + `SubmitClient` blocking-HTTP impl posting `/submit` envelopes; `with_retry` adapter for transient network errors.
- `crates/boole-miner/src/bounty_client.rs` — new module: `BountyClient` blocking-HTTP impl posting `/bounties/:id/proof`; computes `proofHash = sha256(envelope_bytes)`, parses typed `BountyProofResult`.
- `crates/boole-miner/src/http_client.rs` — new module: shared blocking-`reqwest` transport (timeouts, retries, status mapping) consumed by submit / bounty / chain-head clients.
- `crates/boole-miner/src/cli.rs` — added `bounty` handler that reads envelope file (1 MiB cap), drives `BountyClient`, and prints typed `BountyProofResult` JSON.

**Acceptance:** lib tests cover ticket-grind / submission-PoW round-trips, retry-on-network-error semantics, and bounty client envelope round-trip against a stub `Submitter`/HTTP server. Feeds S19's mining loop and `boole mine bounty` CLI. `cargo test -p boole-miner` PASS.

### Slice S15 [C4] — Miner state + `boole mine init / address / config` CLI surface — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-miner/Cargo.toml`, `crates/boole-miner/src/lib.rs` — new workspace member (Rust library + thin binary). No `tokio` dep — matches the rest of the workspace's sync HTTP idiom (uses `reqwest::blocking`).
- `crates/boole-miner/src/state.rs` — new module: `MinerState { schema:"boole.miner.state.v1", version:1, identity:{pk}, config:{dispatcher_url, llm:{backend, ...}, ...}, createdAt, updatedAt }`. Atomic tmp+rename writes at mode 0600 to `$BOOLE_MINER_STATE` (default `~/.boole/miner-state.json`). `iso_now()` uses Howard Hinnant civil_from_days inline (no chrono/time dep).
- `crates/boole-miner/src/cli.rs` — new module: internal subcommands `init`, `address`, `config get`, `config set`. `init` refuses to overwrite without `--force`. `config get` redacts secrets by default with `--reveal` flag. `config set` validates dotted-path keys against a `SECRET_KEYS` allowlist. Pluggable error helper `emit_typed_error(reason, exit_code, ...)` matching `boole keys`'s exit-code vocabulary (2 = bad_request, 3 = state_already_exists / state_not_found).
- `crates/boole-miner/src/bin/boole-miner.rs` — thin binary wrapping `cli::run_mine`.

**Acceptance:** lib tests cover state round-trip, atomic-rename durability, secret redaction, and key-already-exists error path. `cargo test -p boole-miner` PASS. 5 boole-cli integration tests in `mine_init_cli.rs` (delivered together with S19's CLI wiring) cover `mine init` / `mine address` / `mine config get` / `mine config set` / secret redaction round-trip.

### Slice S(B6) [B] — B6 timeout ergonomics — SHIPPED 2026-05-09
**Files touched:**
- `scripts/boole-model-benchmark.py` — argparse, `run_benchmark`, `run_ollama_attempts`, `run_claude_cli_attempts`, `summarize`, `render_leaderboard`. Added private helper `_latency_quantiles(samples)`.
- `scripts/test_model_benchmark.py` — added `B6TimeoutErgonomicsTests` (7 tests).
- `docs/boole-node-cli-parity-plan.md` — §3 B6 section flipped to PASS, Appendix A · S(B6) entry, this §11 ship log entry.
- `tasks/todo.md` — Review section populated post-Green.

**Behavior changes (operator-visible):**
- `--timeout-sec` default 300 → 600. Help text rewritten to call out the rationale ("frontier-model cold-start latency frequently approaches 300s; 600s removes the silent-rejection failure mode") and the `0 = no per-attempt timeout` opt-out.
- `run_benchmark(...)` signature default `timeout_s: int | None = 300 → 600` so direct programmatic callers (tests, future automation) get the same default as the CLI.
- New `--max-run-seconds <N>` flag (int, default `0` = no cap, validated `>= 0`). Cooperative wall-clock cap on the entire run: at attempt-launch boundaries the deadline is checked; if past, the launch loop breaks and the run finalizes with rows-so-far. In-flight attempts are *not* killed (cooperative — verifier I/O state holding subprocess stdout buffers + lake-check temp files would corrupt rows under SIGTERM mid-flight).
- New `summary.runTerminationReason = "max-run-seconds"` field — emitted *only* when the cap fires (i.e. `max_run_seconds > 0` and `len(rows) < expected_count`). Otherwise omitted, so v1/v2/v3 byte-frozen runs without the cap don't carry the field. Exit code remains 0 on cap-trip (operator-designed termination, not failure); `summary.ok` reflects per-row + safety gates (unchanged).
- New `summary.latencyDistribution = {p50Ms, p90Ms, p99Ms, sampleCount}` field at the top level (between `generatedAtUnixMs` and `totals`). Computed across `skipped=false ∧ elapsedMs>0` rows via closed-form linear-interp matching numpy's type-7 quantile contract: `idx = p * (n - 1)`, linear interp between adjacent samples. When `sampleCount == 0` all three p* fields are `null`.
- Leaderboard renders three new lines under the top-level summary block:
  ```
  - latency.p50Ms: `<v>` (sampleCount: `<n>`)
  - latency.p90Ms: `<v>`
  - latency.p99Ms: `<v>`
  ```
  Plus an optional `runTerminationReason: <reason>` line when present.

**Implementation notes:**
- `run_benchmark` snapshots `start_monotonic = time.monotonic()` and computes `deadline_monotonic = start_monotonic + max_run_seconds if max_run_seconds is not None and max_run_seconds > 0 else None`, then propagates `deadline_monotonic` into both attempt-launchers and into the spec-based row loop. Each attempt iteration checks the deadline at the top of the loop body; if past, `break` before launching new work.
- `expected_count` tracking: target-based path sets `expected_count = attempts`; spec-based path sets `expected_count = len(spec_rows)`. Comparing `len(rows) < expected_count` after collection cleanly distinguishes "all rows completed before deadline (no cap-trip)" from "deadline tripped early (cap-trip → set runTerminationReason)".
- `_latency_quantiles(samples)` is a self-contained helper (no numpy dep): sorts the list, computes `idx = p * (n - 1)`, linear-interps between `xs[lo]` and `xs[hi]`, returns rounded ints. Empty-list case returns `{p50Ms: None, p90Ms: None, p99Ms: None, sampleCount: 0}`.
- `summarize()` filter: `latency_samples = [int(row["elapsedMs"]) for row in rows if not row.get("skipped") and int(row.get("elapsedMs", 0) or 0) > 0]`. The `int(... or 0)` guard tolerates both missing-key and explicit-`None` shapes.

**Acceptance evidence:**
- `python3 -m unittest scripts.test_model_benchmark` → 57/57 PASS (50 existing + 7 new in `B6TimeoutErgonomicsTests`).
- `python3 -m py_compile scripts/boole-model-benchmark.py` → exit 0.
- `python3 scripts/boole-model-benchmark.py --help` → shows `default: 600` for `--timeout-sec` and the new `--max-run-seconds` block.
- CLI smoke `--max-run-seconds 1` against a 5-row × 0.6s mock spec (`/tmp/b6-spec.json`) → 2 rows produced (not 5), `summary.runTerminationReason="max-run-seconds"`, exit code 0, `latencyDistribution.sampleCount=2`, leaderboard contains all three p* lines + `runTerminationReason: max-run-seconds`.
- `MiningV3FamilyTests` + `MiningV2FamilyTests` re-run independently → 19/19 PASS (v1/v2/v3 family byte-frozen contracts unchanged).
- Red-state proof: with the Green edits stashed and only `B6TimeoutErgonomicsTests` present, 4 failures + 2 errors out of 7 (only `test_b6_zero_timeout_propagates_as_none_to_run_benchmark` was already passing because the `0 → None` plumbing pre-existed; B6 only adds the regression pin). Restoring the implementation flips the remaining 6 to Green.

**Deviations from plan §3 B6 / §Appendix A · S(B6):**
1. **Latency-distribution percentiles for the test fixture corrected.** The plan's todo.md targeted p50=500, p90=900, p99=990 for the 10-sample {100..1000} fixture; the numpy type-7 contract actually yields p50=550, p90=910, p99=991 (linear interpolation between adjacent samples, not nearest-neighbor). The test was authored with the correct math from the start; the plan-text round numbers were a pre-implementation approximation.
2. **`runTerminationReason` field is conditionally emitted, not always-present.** The plan implied "set on cap-trip, otherwise default value"; the implementation omits the key entirely on non-cap runs to preserve v1/v2/v3 byte-frozen summary shapes that pre-date this slice. CI consumers must check key presence, not key value.
3. **Cap-trip detection uses `len(rows) < expected_count`, not a `terminated_early` flag returned from launchers.** The plan sketched threading a flag back; the simpler post-collection comparison is sufficient and avoids changing the launcher return-tuple shape.
4. **Validation is `>= 0`, not `> 0`.** `--max-run-seconds 0` is the sentinel for "no cap" (matches `--timeout-sec 0` opt-out convention). Negative values exit with code 2 via `SystemExit`.

**Downstream unblocked:**
- **Live-run cost accounting evidence (any future B-track slice).** Operators can now run frontier models with confidence that cold-start latency won't be silently rejected, and can cap overall wall-clock cost for budgeted runs. The `latencyDistribution` field surfaces the rejection failure mode visibly in CI and leaderboard outputs.
- **CI/scheduled-run integration.** Any future cron/CI scheduler that wants to bound a benchmark run to a fixed time window now has a clean knob (`--max-run-seconds`) plus a deterministic exit-0 + `runTerminationReason` shape to gate on.

**Open follow-ups (not blocking S(B6)):**
- **Pre-emptive (SIGTERM) wall-clock cap.** Cooperative is enough for the documented use case ("don't run past midnight"); a panic-button SIGTERM mode could be added later if operators ask. Out of scope per plan.
- **Per-target / per-row timeout overrides.** The spec form already supports `timeoutSec` per row; that path is unchanged. A CLI flag to override per-row timeouts uniformly could come later.
- **`--max-run-seconds` env-var fallback.** CLI-only for now; can be promoted to env if operators ask.
- **Live 50-attempt evidence with new defaults.** A 50-attempt run with the new defaults + the new latency surface should land in `artifacts/model-benchmarks/` separately as evidence; not part of this slice.

### Slice S9 [N+C] — Reward ledger runtime integration + `boole account balance` — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-node/src/reward_store.rs` — added `verify_ledger_matches_replay(ledger, replay_balances)`, promoted `apply_event` → public `apply`, added `last_event() -> Option<&PersistedRewardEvent>`.
- `crates/boole-node/src/runtime.rs` — `RuntimeAdmissionState` gains `reward_ledger: Option<FileRewardLedger>` + `reward_ledger_path: Option<PathBuf>`; `boot_from_store` takes a third `reward_ledger_path` argument and recovers / re-derives / verifies; `commit_using_cache` appends `PersistedRewardEvent` between block append and `apply_block_unchecked`; new `balance_for(pk)` and `ledger_head() -> Option<(u64, String)>` accessors.
- `crates/boole-node/src/local_node.rs` — `LocalNodeConfig.reward_ledger_path: Option<PathBuf>`; new axum route `GET /account/{pk}/balance` + `account_balance_handler` + `account_balance_json` + `is_well_formed_hex32`; explicit `set_current_c(scenario.genesis_c)` for empty-chain after boot to preserve `--genesis` override.
- `crates/boole-node/src/runtime_smoke.rs` — pass `None` to the new `boot_from_store` arg.
- `crates/boole-node/src/main.rs` — `--reward-store <path>` flag + `REWARDLEDGER_PATH` env (default `/tmp/boole-node-rewards.ndjson`); help text + boot log line.
- `crates/boole-node/src/http_error.rs` — new `HttpError::malformed_pk()` (HTTP 400, reason code `malformed_pk`).
- `crates/boole-cli/src/main.rs` — new `Account { Balance { pk, node, json } }` clap subcommand; `account_balance(pk, node, json)` issues `GET /account/{pk}/balance`, prints bare balance by default, full envelope under `--json`, rejects malformed pk locally.
- `crates/boole-node/tests/{reward_store_divergence,runtime_reward_ledger,account_balance_route}.rs` — 12 new tests.
- `crates/boole-cli/tests/account_balance_cli.rs` — 4 new tests, boots a real node + drives the binary.
- `crates/boole-node/tests/{concurrency,http_contract,local_node,runtime_policy_boot}.rs`, `crates/boole-cli/tests/node_block.rs` — added `reward_ledger_path: None` (or third `None` arg) to keep existing call sites compiling.
- `docs/boole-node-cli-parity-plan.md` — §1 surface table updated, §2 N3 flipped to PASS, §3 Gate P3 PASS + Gate P6 C3 partial PASS, Appendix A · S9 entry, this §11 ship log entry.
- `tasks/todo.md` — Review section to follow.

**Behavior changes (operator-visible):**
- `boole-node run-local` accepts `--reward-store <path>` / `REWARDLEDGER_PATH` env; default `/tmp/boole-node-rewards.ndjson`. Boot logs the resolved path (`boole-node local rewardLedger=…`).
- New route `GET /account/{pk}/balance`. 200 OK envelope `{ok:true, pk, balance:"<u128>", asOfHeight, asOfC}`. Unknown but well-formed pk → balance `"0"` (parity with pof). Malformed pk → 400 `{ok:false, reason:"malformed_pk"}`. Empty chain → `asOfHeight: 0` + scenario `genesis_c`.
- New CLI `boole account balance --pk <hex32> [--node URL] [--json]`. Default prints bare balance to stdout, `--json` prints the full server envelope. Local malformed_pk validation rejects before the node round-trip with stderr typed envelope + exit 2.
- Append-on-commit ordering: `commit_using_cache` writes the block first, then the reward event, then mutates the in-memory state. A crash between block append and ledger append is recoverable on next boot — replay re-derives the missing event from the existing block.

**Implementation notes:**
- `verify_ledger_matches_replay` does a two-way `BTreeMap<String, u128>` comparison: every replay pk must exist in the ledger with matching amount, and every ledger pk must exist in the replay map. The first divergence is surfaced as `reward ledger divergence at pk … ledger=… replay=…` so boot bails with a single, debuggable error rather than reporting the whole diff.
- The boot path always routes through `boot_from_store` (even for empty chains) so the ledger handle is initialized uniformly. For the empty-chain case, replay returns the all-zero genesis hash; we then explicitly `set_current_c(scenario.genesis_c.clone())` to preserve `--genesis` override semantics. Without that override pass, an empty-chain start would silently force `current_c = "00…00"` regardless of scenario config.
- `is_well_formed_hex32` uses `s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())` — same shape on both sides (server validation in `account_balance_json`, client validation in `account_balance`) so the typed `reason: "malformed_pk"` envelope is identical regardless of where the rejection lands.
- Tests drive everything off `fixtures/protocol/replay/v1.json` (2 blocks, 5 distinct pks: PK_A=1, PK_1=1, PK_2=3, PK_3=1, PK_B=1) instead of synthesizing chains. Existing fixture covers proposer-credit + multi-share-owner + cross-block accumulation in one shape.

**Acceptance evidence:**
- `cargo test -p boole-core -p boole-node -p boole-cli` → all green, 16 new tests pass (4 ledger-divergence + 4 runtime boot scenarios + 4 axum route + 4 CLI surface). Pre-existing test counts unchanged.
- `cargo clippy -p boole-node -p boole-cli --tests` → clean, no new warnings.
- Manual smoke against a node booted off the seeded `replay/v1.json` ledger:
  - `account balance --pk 22…22 --json` → `{"ok":true,"pk":"22…22","balance":"3","asOfHeight":1,"asOfC":"abbd…4b49"}`.
  - `account balance --pk aa…aa` (no `--json`) → bare `1`.
  - `account balance --pk cc…cc --json` (unknown well-formed) → `balance:"0"`, exit 0.
  - `account balance --pk tooshort --json` (malformed) → stderr `{"ok":false,"pk":"tooshort","reason":"malformed_pk"}`, exit 2.

**Deviations from plan §2 N3 / §3 C3 / §Appendix A · S9:**
1. **`replay_blocks` was not extended in S9 — the divergence check lives in `reward_store.rs`.** The plan's task 5 ("extend `replay_blocks` to also re-derive balances and check against the ledger file for divergence") was already half-done before this slice: `replay_blocks` already returned `balances: BTreeMap<String, u128>`. S9 keeps the verification on the ledger side (`verify_ledger_matches_replay`) so `boole-core` stays free of file I/O — replay is pure, the file vs. replay comparison lives in the consumer. Net effect on parity is identical.
2. **Multi-miner fixture is 2 blocks, not 4.** The plan's acceptance criterion mentioned a 4-block multi-miner fixture; the existing `fixtures/protocol/replay/v1.json` already covers the multi-pk + multi-block + proposer + share-owner combinatorics in 2 blocks (5 distinct pks, cross-block accumulation on PK_2). Extending to 4 blocks adds no new code path coverage; deferred to a future hardening pass when needed.
3. **Server returns `balance: "<string>"` not `balance: <number>`.** The plan didn't specify the wire shape; we follow pof's contract here — u128 amounts overflow JS `Number` precision (`2^53`), so they ship as strings. The CLI's `--json` mode forwards the server envelope verbatim; the bare-print mode prints the string directly so callers piping into shell tools see the same digits.
4. **`balance` is a `String` field on `PersistedCredit` already.** No new wire-level type was needed; `compute_block_credits` already returns `Vec<PersistedCredit { pk: String, amount: String }>`.

**Downstream unblocked:**
- **C4 mining surface (Slice S10+).** Once the rest of N4 (work) and N5 (bounty) land, `boole mine bounty --id <id> --node URL --prover <key>` can credit miners against this same ledger; `account balance` becomes the natural verification surface for "did my mining attempt produce a share that earned a credit?".
- **Snapshot endpoint (`GET /snapshot`)** — a future tarball-style bootstrap surface can include the reward ledger NDJSON next to the block log; the divergence check at receiver-side boot is the obvious sanity gate.
- **Per-miner reward attribution dashboards** — the `asOfHeight` + `asOfC` fields let UIs render "as of block N" balances without polling for head separately.

**Open follow-ups (not blocking S9):**
- **4-block fixture for explicit multi-miner-bonus parity.** Once a calibrated multi-miner scenario exists upstream, copy it into `fixtures/protocol/runtime-smoke/multiminer.v1.json` and add a runtime smoke that asserts proposer-bonus + share-owner-credit math byte-for-byte. Existing 2-block fixture is sufficient for the divergence + recovery contract.
- **CLI `--all` flag for `boole account balance`.** Today the CLI takes a single pk. Once N3/N4 surface a `GET /accounts` route that streams pks with non-zero balances, the CLI can grow a `--all` mode for operator UX.
- **Reward ledger snapshot prune / compaction.** NDJSON grows linearly with chain height. Once chains pass O(1M) blocks, consider periodic snapshotting (height-N balance snapshot + delta log from there). Not a parity requirement — pof has the same shape today.

### Slice S(B2) [B] — `submit-lean --admission-nonce` override — SHIPPED 2026-05-09
**Files touched:**
- `crates/boole-node/src/main.rs` — `run_submit_lean_command`: extract `--admission-nonce` via the existing `take_optional_flag_value` helper; validate `len == 64 && all ascii_hexdigit` *before* fixture parse + Lean spawn (typed stderr envelope + exit 1 on malformed); thread the override into `ProofSubmissionTemplate.n` (defaulting to `fixture.constants.n` when unset). Added free helper `is_well_formed_hex32`. Updated `print_help` to surface `[--admission-nonce <64-hex>]` on the `submit-lean` line.
- `crates/boole-node/tests/submit_lean_admission_nonce.rs` — new test file with 3 tests + a self-contained `TestLeanWorkspace` helper duplicated from `submit_lean_cli.rs` (matches existing per-file convention; no shared test-util crate).
- `docs/boole-node-cli-parity-plan.md` — §3 B2 section flipped to PASS, Appendix A · S(B2) entry, this §11 ship log entry.
- `tasks/todo.md` — Review section to follow.

**Behavior changes (operator-visible):**
- `boole-node submit-lean` accepts `--admission-nonce <64-hex>`. When set, overrides `ProofSubmissionTemplate.n`; when unset, behavior is byte-frozen (fixture `n`).
- Malformed value (wrong length or non-hex byte) → stderr typed envelope `{ok:false, command:"submit-lean", accepted:false, error:"malformed-admission-nonce", shareAccepted:false, blockProduced:false, invalidAccepted:0}` + exit 1. Block store untouched, fixture not parsed, Lean not spawned. Reason code parallels S9's `malformed_pk`.
- Two `submit-lean` runs with two different `--admission-nonce` values produce two different `share_hash` outputs (`submissionBody.n` mirrors the flag value byte-for-byte). This is the offline diversification path B2 was demoted to: `/ticket` is observe-only on both pof and Boole, so without the flag, fixture-driven CI runs would produce identical shares forever.

**Implementation notes:**
- Validation block sits between the `args.is_empty()` guard and `submit_lean_fixture(&fixture_path, …)?` so a typo'd `--admission-nonce` never pays for fixture I/O or `lake exec boole_check`. The block uses the existing `eprintln! + std::process::exit(1)` shape from `missing_checker_artifact_policy` rather than `anyhow::bail!`, so the stderr surface matches the rest of `submit-lean`'s rejected-input envelopes (`{ok:false, command:"submit-lean", accepted:false, error:<kebab>, …}`) instead of degrading to a generic anyhow error.
- `is_well_formed_hex32` is defined inside `boole-node/src/main.rs` despite the same predicate also living in `boole-cli/src/main.rs` and `boole-node/src/local_node.rs`. Three single-line predicates across two crates is cheaper than a new public surface in `boole-core`; the typed envelope shape (`reason`/`error`) is what cross-surface tooling pattern-matches on, not the helper name.
- The override is a `String` clone into `template.n` to keep the existing `ProofSubmissionTemplate` field types unchanged — no need to widen the type, no down-stream serialization shifts.

**Acceptance evidence:**
- `cargo test -p boole-node --test submit_lean_admission_nonce` → 3/3 PASS:
  - `submit_lean_rejects_malformed_admission_nonce_before_lean` — runs without lake/lean (validation short-circuits); asserts exit non-zero, stdout empty, stderr is JSON with `error == "malformed-admission-nonce"`, block store path absent.
  - `submit_lean_admission_nonce_changes_submission_body_n_and_share_hash` — lake/lean-gated; two runs with `0x11…11` vs `0x22…22` nonces produce matching `submissionBody.n` and *different* `shareHash` strings.
  - `submit_lean_admission_nonce_default_uses_fixture_value` — lake/lean-gated; one run without the flag confirms `submissionBody.n == fixture.constants.n` byte-for-byte.
- `cargo test -p boole-core -p boole-node -p boole-cli` → full regression clean, no pre-existing tests modified or affected.
- `cargo clippy -p boole-node --tests` → clean, no new warnings.
- Red-state proof: with the Green edit reverted (no flag extraction, no validation block, no template override), the malformed test produced `Error: unexpected args: --admission-nonce tooshort` (not the typed envelope) and the override test produced the same anyhow message instead of a successful run. After Green: 3/3.

**Deviations from plan §3 B2 / §Appendix A · S(B2):**
1. **No env-var fallback (`BOOLE_ADMISSION_NONCE`).** Plan flagged this as out-of-scope but acknowledged it as a future possibility. Today's CLI plumbing has zero consumers asking for it; deferred until a real CI matrix needs it.
2. **No `boole-cli` top-level surface.** `submit-lean` is a `boole-node` binary subcommand (it talks to Lean directly, not to a running node), so there's nothing for `boole-cli` to wrap. If a future slice grows `boole-cli submit-lean`, the flag is trivially passable — same shape as the existing `--head-c` / `--ip` precedent.
3. **Three single-line `is_well_formed_hex32` helpers, not one shared.** Same trade-off as S9: a cross-crate helper requires a `boole-core` public surface for one bool predicate. Three duplicate lines are cheaper than the indirection.

**Downstream unblocked:**
- **`scripts/boole-model-benchmark.py` offline diversification.** Benchmark runs that don't spawn a live node can now pass per-attempt `--admission-nonce` values (deterministic seed → hex32 derivation) so leaderboard runs produce diverse share hashes without network setup. Existing live-node path (`--use-node-ticket`) is unaffected.
- **Future `boole-miner` crate (§4).** When the Rust miner lands, its dry-run mode can call `submit-lean --admission-nonce <generated>` to verify the bridge end-to-end without standing up a network — the flag is the documented seam for nonce control on the offline path.

**Open follow-ups (not blocking S(B2)):**
- **`BOOLE_ADMISSION_NONCE` env-var fallback.** Promote when CI or scripted callers ask.
- **Per-attempt nonce derivation in benchmark scripts.** Out of scope for the CLI surface; a follow-up benchmark slice can wire `attempt_index → blake3 → hex32 → --admission-nonce <hex>` for deterministic-but-diverse runs.

### Slice S14 [N+C] — `POST /bounties/:id/status` operator status transition + audit ledger status_change events + `boole bounty status` — SHIPPED 2026-05-09

**Outcome:** Phase N5 operator status surface complete. The full bounty CLI surface (list / get / submit / announce / status) now matches pof. Operators can withdraw, resolve, or re-open bounties end-to-end with audit-ledger durability across restart. C3 bullet of Gate P6 PASS extended to cover the last bounty subcommand.

**Files touched:**
- `crates/boole-node/src/http_error.rs` — 3 new typed reason constructors: `bad_status_value(value)` 400 (handler-side enum check, surfaces `newStatus`), `bounty_id_mismatch(url_id, payload_id)` 400 (URL/payload id cross-check, mirrors S13b's `workId/bounty.id` cross-check), `invalid_status_transition(detail)` 400 (fallback bucket for any future `update_status` rule). All underscores, matching the `bad_proof_hash` precedent.
- `crates/boole-node/src/local_node.rs` — new `bounty_status_handler` (`POST /bounties/:id/status`) wired into `build_router` next to the existing `/bounties/:id/proof`. New `bounty_status_json` 9-step validation: (1) outer `boole.signed.v1` else `bad_envelope`, (2) hex shape on `pk` / `signature`, (3) `verify_signature` (`Err` → `bad_pk`, `Ok(false)` → 401 `signature_invalid`), (4) inner `boole.bounty.status.v1` else `bad_payload`, (5) `id` / `newStatus` / optional `reason` / `ts` field validation, (6) URL `:id` vs payload `id` cross-check → 400 `bounty_id_mismatch`, (7) `newStatus` ∈ enum → 400 `bad_status_value`, (8) load existing bounty for server-derived `prevStatus` and call `BountyRegistry::update_status` — terminal-state Err → 409 `bounty_terminal`, unknown-id Err → 404 `bounty_not_found`, other Err → 400 `invalid_status_transition`, (9) build `kind:"status_change"` audit event (`{schemaVersion:1, kind:"status_change", workId, problemHash, verifierKind, ts, prevStatus, newStatus, announcerPk:pk, reason?}`) and `FileBountyEventLedger::append`. New `replay_status_change_event` extends `replay_bounty_audit_event` to handle the new event kind on boot — pulls `id` / `newStatus` / `ts`, calls `update_status`, on Err emits a single-line stderr warning and continues (parallel to S13b's static-wins-on-overlap policy for create events). Imports updated to add `UpdateStatusInput`. The handler signature uses `AxumPath(id): AxumPath<String>` to match the file's existing `Path as AxumPath` import alias.
- `crates/boole-node/tests/bounty_status_route.rs` (NEW, 6 tests):
  - `valid_status_change_updates_bounty_and_appends_audit_event` — gamma-1 open → withdrawn flips status, audit log gains a `kind:"status_change"` line with `prevStatus="open"`, `newStatus="withdrawn"`, `announcerPk`, `verifierKind="mock-accept"`.
  - `unknown_status_value_returns_400_bad_status_value` — `newStatus="frozen"` → 400 typed envelope with `newStatus` in the body.
  - `url_id_payload_id_mismatch_returns_400_bounty_id_mismatch` — URL `/gamma-1/status` with payload `id="delta-1"` → 400 typed envelope.
  - `terminal_bounty_returns_409_bounty_terminal` — epsilon-1 (boots in `withdrawn`) → open → 409 typed envelope.
  - `boot_replay_restores_post_transition_status` — boot 1 transitions gamma-1 to withdrawn, boot 2 GET shows `status="withdrawn"` (audit replay layered on the static catalog).
  - `tampered_payload_returns_401_signature_invalid` — payload mutated after sign → 401 typed envelope, no registry change.
- `crates/boole-cli/src/main.rs` — new `BountyCommand::Status` clap variant (7 args, `--new-status` constrained via `value_parser` over the 4 enum values for clap-level rejection of typos). New handler `bounty_status` 6-step flow: (1) resolve key, refuse v1 with `legacy_v1_key` exit 3, (2) decode `sk` via `SigningKeyV2::from_seed_hex`, (3) build payload (default `ts = unix_ms_now()`, `reason` only emitted when present), (4) sign + POST `/bounties/:id/status`, (5) on 2xx print bare `<newStatus>` (default) or full envelope (`--json`), (6) on non-2xx forward typed envelope to stderr exit 1.
- `crates/boole-cli/tests/bounty_status_cli.rs` (NEW, 4 tests): successful status change emits bare `<newStatus>` on stdout, `--json` returns full server envelope (`{ok:true, bounty:{id, status:"withdrawn"}}`), v1 key refused with `legacy_v1_key` exit 3, server-side terminal transition forwards `bounty_terminal` to stderr exit 1.

**Verification:**
- `cargo build -p boole-node` and `cargo build -p boole-cli` clean.
- `cargo test -p boole-node --test bounty_status_route` → 6/6 PASS.
- `cargo test -p boole-cli --test bounty_status_cli` → 4/4 PASS.
- `cargo test -p boole-core -p boole-node -p boole-cli -- --test-threads=1` → full regression clean (parallel runs surfaced a known flaky port-collision in `account_balance_route` unrelated to S14).
- `cargo clippy --all-targets -- -D warnings` clean.

**Deviations from plan §Appendix A · S14:** none. Scope decisions held: payload schema `boole.bounty.status.v1`, envelope reuse `boole.signed.v1`, no nonce/replay protection, no allowlist, single audit ledger reused, server-derived `prevStatus`, reason vocab as planned, CLI thin-client default-bare-`<newStatus>`, operator-supplied `ts` with CLI default `unix_ms_now()`.

**Downstream unblocked:**
- Phase N5 long-tail items (runtime side-pool wiring, `FamilyManifest` activation gating) — the registry mutation invariants are now ready for promoted-bounty admission since `update_status` is exercised end-to-end through the audit ledger.
- Operator key rotation surface — once `keys rotate / upgrade` lands, the existing announce + status-change flows pick up new pks without protocol changes.

**Open follow-ups (not blocking S14):**
- Reward credit on accepted proofs (`accepted:true → RewardLedger::credit`) — a separate slice.
- `--allowlist <path>` for restricting status-change pks — same deferral as S13b.

**Doc updates:**
- `docs/boole-node-cli-parity-plan.md` — §1.1 `POST /bounties/:id/status` row → match (S14), §1.2 `boole bounty status` row → match (S14), §2 Phase N5 header → "PARTIAL PASS … (Slices S11 + S12 + S13b + S14)" with operator status portion described, §3 C3 → PASS extended through bounty status, Gate P4 status-transition flow PASS bullet, status header to include S14, Appendix A · S14 entry, S13b Follow-ups marked delivered for the operator-status item, this §11 ship log entry.
- `tasks/todo.md` — S14 plan (locked scope decisions §1–§11, file inventory, TDD plan, acceptance evidence). Review section filled in below.

### Slice S13b [N+C+core] — `POST /bounties` announce flow + audit ledger create events + `boole bounty announce` — SHIPPED 2026-05-09

**Outcome:** Phase N5 announce write surface complete + Phase C3 bounty CLI surface complete (announce was the last subcommand). The bounty surface end-to-end is now live: keys → announce → list → submit → solved status persists across restart.

**Files touched:**
- `crates/boole-core/src/bounty_ledger.rs` — `validate_event` extended for `kind:"create"`. The optional `bounty` sub-object carries the full `Bounty` shape so a restart can rebuild a dynamically-announced registry without an external catalog. When the sub-object is present, three cross-checks fire: `bounty.id == workId`, `bounty.problemHash == problemHash`, `bounty.verifier.kind == verifierKind`. Sub-object is OPTIONAL to preserve `fixtures/protocol/bounty-ledger/v1.json` byte-equal parity (legacy pof create events carry only the flat fields).
- `crates/boole-core/tests/bounty_event_ledger.rs` — 2 new tests: `append_then_recover_round_trips_create_event_byte_equal` (full create event with bounty sub-object survives append+recover byte-for-byte), `append_rejects_create_event_when_workid_disagrees_with_embedded_bounty_id` (cross-check rejection prevents id corruption).
- `crates/boole-node/src/http_error.rs` — 4 new constructors: `bad_envelope(detail)` 400, `signature_invalid()` 401, `bad_payload(field, detail)` 400, `bounty_already_exists(id)` 409. Underscore reasons match the existing `bad_proof_hash` precedent.
- `crates/boole-node/src/local_node.rs` — wired `POST /bounties` next to existing `GET /bounties` via `.route("/bounties", get(bounty_list_handler).post(bounty_announce_handler))`. New `bounty_announce_handler` (axum) + `bounty_announce_json` (impl) implement the 8-step validation chain (outer schema, hex-shape pk/sig, signature verify, inner schema, field validation, registry create, audit append, return). New const `ANNOUNCE_PAYLOAD_SCHEMA: &str = "boole.bounty.announce.v1"`. New helpers `required_payload_string` and `is_well_formed_hex64`. Refactored `replay_bounty_audit_event` to dispatch on `kind` (proof → existing path; create → new `replay_create_event`). `replay_create_event` matches `Err("duplicates id...")` and emits a single-line stderr warning on overlap (static `bounties_path` catalog wins; audit log adds new ids on top).
- `crates/boole-node/tests/bounty_announce_route.rs` — NEW, 8 tests: valid signed envelope creates bounty + appears in `GET /bounties`, audit log gains `kind:"create"` line with `announcerPk`, boot replay restores announced bounty, boot replay overlap with static catalog wins without panic, tampered payload → 401 `signature_invalid`, wrong outer schema → 400 `bad_envelope`, wrong inner payload schema → 400 `bad_payload`, duplicate id → 409 `bounty_already_exists`. Boots `serve_local_node` with `mock_bounty_fixture_path()` static catalog and `MockAccept` verifier; signs payloads via `SigningKeyV2::from_dev_id("announcer-N")` for deterministic test keys.
- `crates/boole-cli/src/main.rs` — `BountyCommand::Announce` clap variant with 11 args (`--id`, `--domain`, `--problem-hash`, `--verifier-kind`, `--verifier-metadata`, `--reward`, `--deadline`, `--ts`, `--signing-key`, `--node`, `--json`). New `bounty_announce` handler annotated `#[allow(clippy::too_many_arguments)]` runs the 8-step CLI flow (local hex32 lint → metadata read → key resolve + v1 refuse → seed decode → payload build with default `unix_ms_now()` ts → sign → POST → 2xx bare-id-or-envelope / non-2xx forward). New helper `unix_ms_now() -> u64`. Imports extended: `use boole_core::{... verify_signature, CreateBountyInput, ... SIGNED_ENVELOPE_SCHEMA};`.
- `crates/boole-cli/tests/bounty_announce_cli.rs` — NEW, 6 tests: successful announce emits bare bounty id on stdout, `--json` returns full envelope, v1 key → `legacy_v1_key` exit 3, malformed `--problem-hash` → exit 2 typed envelope (no network round-trip), server-side duplicate forwards `bounty_already_exists` to stderr exit 1, `--verifier-metadata` accepts a file path. Boots a real node via `boole_node::local_node::serve_local_node` in a spawned thread; `make_dev_key(&keys_dir, "announcer")` runs `boole keys new --dev` against an isolated `BOOLE_KEYS_DIR`.
- `docs/boole-node-cli-parity-plan.md` — §1.1 `POST /bounties` row → match (S13b), §1.2 `boole bounty announce` row → match (S13b), §2 Phase N5 header → "PARTIAL PASS … (Slices S11 + S12 + S13b)" with announce-write portion described, §3 C3 → PASS extended through bounty announce, Gate P4 announce-flow bullet → PASS (S13b), Gate P6 C3 PASS through announce, status header to include S13b, Appendix A · S13b entry, S13a Follow-ups marked delivered, this §11 ship log entry.
- `tasks/todo.md` — S13b plan (locked scope decisions §1–§11, file inventory, TDD plan, acceptance evidence). Review section filled in below.

**Behavior changes (operator-visible):**
- `POST /bounties` accepts `{schema:"boole.signed.v1", payload, pk, signature}`. Inner `payload` schema is `boole.bounty.announce.v1` carrying `{id, domain, problemHash, verifier:{kind, metadata}, reward:string-u128, deadline, ts}`. Returns 200 `{ok:true, bounty:<created>}` on success. Typed-error vocabulary: 400 `bad_envelope`, 401 `signature_invalid`, 400 `bad_payload`, 400 `bad_pk`/`bad_signature`, 409 `bounty_already_exists`, 400 field-level (`bad_problem_hash`, etc.).
- `boole bounty announce --id <id> --domain <domain> --problem-hash <hex32> --verifier-kind <kind> --verifier-metadata <path|inline> --reward <u128> --deadline <u64-ms> --signing-key <id> [--ts <u64-ms>] [--node URL] [--json]` — bare bounty id on stdout default, full server envelope under `--json`. v1 keys refused with `legacy_v1_key` exit 3. Malformed `--problem-hash` rejected locally with `malformed-problem-hash` exit 2 (no network). 4xx/5xx forwarded to stderr exit 1.
- Boot replay rehydrates dynamically-announced bounties from the audit log on top of the static `bounties_path` catalog. Duplicate id (static + dynamic announce of same id) resolves to the static version with a stderr warning — operators commonly add bounties to the static file after originally announcing them dynamically.

**Implementation notes:**
- **Single audit ledger, schema extended.** No new file. Both `kind:"proof"` (S12) and `kind:"create"` (S13b) events live in the same NDJSON stream behind `--bounty-events`. A future operator query like "show me all activity on bounty X" runs over one file.
- **`bounty` sub-object is optional, not required.** Original plan called for required, but `fixtures/protocol/bounty-ledger/v1.json` (the pof TypeScript golden) carries flat-fields-only create events. Making the sub-object optional preserves byte-equal parity while still applying the cross-check guarantee when the sub-object is present.
- **No nonce / replay protection.** Bounty `id` is the unique key; `BountyRegistry::create` rejects duplicates. Replaying a captured envelope is a 409 no-op. When allowlists land, an `allowlistEntry` field can include a per-entry nonce.
- **No allowlist in S13b.** Any well-formed ed25519 signature is accepted. The announcer's `pk` is recorded in the audit event for after-the-fact filtering. An `--allowlist <path>` server flag can land later without changing the wire contract.
- **`ts` is operator-supplied.** The CLI defaults `--ts` to `unix_ms_now()` but allows override for fixture/test reproducibility. The server validates `ts` is a non-negative integer but does not overwrite it — keeps signatures stable across server clocks.
- **Static-wins-on-overlap is a soft policy.** A `replay_create_event` overlap emits `eprintln!` and continues. Hard error would force operators to manually reconcile the catalog whenever they add a previously-announced bounty to the static file — common in practice. The audit log still records the original announce, so the history is preserved.

**Verification:**
- `cargo test -p boole-core --test bounty_event_ledger` → 5/5 PASS (3 pre-existing + 2 new).
- `cargo test -p boole-node --test bounty_announce_route` → 8/8 PASS.
- `cargo test -p boole-cli --test bounty_announce_cli` → 6/6 PASS.
- `cargo test -p boole-core -p boole-node -p boole-cli` → full regression PASS, no pre-existing tests modified.
- `cargo clippy -p boole-core -p boole-node -p boole-cli --all-targets -- -D warnings` clean.
- `cargo test --workspace` surfaces a single pre-existing failure in `boole-lean-runner` (`canonical_checker_artifact_hash_matches_readme_pin`): `lean/checker/lakefile.lean` has an in-flight unrelated `lean_lib «Boole»` stanza addition without a `lean/checker/README.md` hash bump. S13b touches no Lean code; the failure is independent of this slice and tracked as a follow-up to that lakefile change.

**Downstream unblocked:**
- **Reward credit on accepted proofs.** `accepted:true → RewardLedger::credit(solverPk, reward)` was already wired internally as part of S12's audit append; the next slice can route the credit through the runtime so balances reflect bounty payouts on `GET /account/:pk/balance`.
- **`POST /bounties/:id/status`.** Operator status transitions (open → withdrawn, etc.) can land as a separate route reusing the same `boole.signed.v1` envelope shape and `kind:"status_change"` audit events (already in the ledger validator).
- **`--allowlist <path>` for restricting announce pks.** The `pk` is already recorded in the audit event; a future `--allowlist <path>` flag adds an extra check after `verify_signature` succeeds without changing the wire contract.

**Open follow-ups (not blocking S13b):**
- **Runtime side-pool wiring + `FamilyManifest` activation gating.** Phase N5's long tail — bounty submissions enter a side-pool, NOT the main share pool, until promoted via a signed family manifest with `activation_height`. S13b leaves the registry mutation invariants ready for this.
- **Schema v3 keys (sealed-box `sk` encryption) + `keys rotate / upgrade`.** Operator key material is still on disk in cleartext; sealed-box encryption with a passphrase is a future hardening pass.
- **RFC 8785 full canonicalization.** Today's `canonicalize` is RFC 8785-lite (sorted keys, preserved arrays, default number/string emit). Number normalization and escape minimization are deferred until on-chain commitment forces parity.

### Slice S13a [C+core] — ed25519 signing primitives + keys schema v2 + `boole keys sign / verify` — SHIPPED 2026-05-09

**Files touched:**
- `Cargo.toml` (workspace) — added `ed25519-dalek = { version = "2.1", default-features = false, features = ["std", "rand_core"] }` and `rand_core = { version = "0.6", features = ["std"] }` to `[workspace.dependencies]`. Pure-Rust ed25519, audit-friendly, the de facto standard.
- `crates/boole-core/Cargo.toml` — pulled in `ed25519-dalek.workspace = true` and `rand_core.workspace = true`.
- `crates/boole-core/src/canonical_json.rs` — new module. `pub fn canonicalize(value: &Value) -> Vec<u8>` — depth-first walk, sort object keys via `BTreeMap<&str, _>`, emit through `serde_json::to_vec`. RFC 8785-lite (sorted keys, preserved arrays, default number/string emit; no whitespace, no trailing newline). Documented divergence from full RFC 8785 (number normalization, escape minimization) so the swap is localized when on-chain commitment lands.
- `crates/boole-core/src/signed_envelope.rs` — new module. `pub const SIGNED_ENVELOPE_SCHEMA: &str = "boole.signed.v1"`. `pub struct SigningKeyV2 { inner: SigningKey }` with `from_random()` (OsRng), `from_dev_id(id)` (`blake3("boole-keys-dev::" || id)` seeded — domain prefix matches v1's `dev_pk_hex`), `from_seed_hex(hex)`, `pk_hex()`, `sk_seed_hex()`, `sign(&Value) -> SignedEnvelope`. `pub struct SignedEnvelope { schema, payload, pk, signature }` with `verify()` delegating to the stateless primitive. `pub fn verify_signature(pk_hex, sig_hex, payload) -> Result<bool, String>` does the full validation chain: hex32 shape check on `pk`, hex64 shape check on `sig`, `VerifyingKey::from_bytes`, `Signature::from_bytes`, then ed25519 verify against `SHA-256(canonicalize(payload))`. Error split: `Err(detail)` for wire-malformed (4xx-class `bad_pk`/`bad_signature`), `Ok(false)` for cryptographically wrong (200-class).
- `crates/boole-core/src/lib.rs` — `pub mod canonical_json; pub mod signed_envelope;` and re-exports `canonicalize`, `verify_signature`, `SignedEnvelope`, `SigningKeyV2`, `SIGNED_ENVELOPE_SCHEMA`.
- `crates/boole-core/tests/canonical_json.rs` — new test file with 4 tests (object keys sort alphabetically regardless of input order, nested objects sort recursively, arrays preserve insertion order, idempotence across round trips).
- `crates/boole-core/tests/signed_envelope.rs` — new test file with 4 tests (sign+verify round trip Ok(true), wrong pk Ok(false) not Err, tampered payload Ok(false), malformed pk Err `bad_pk:`).
- `crates/boole-cli/src/main.rs` — `KeysCommand` enum gains `Sign { id, payload, json }` and `Verify { pk, signature, payload, json }` variants. `keys_new` rewritten to generate ed25519 keys via `SigningKeyV2::from_random()` / `from_dev_id(id)` and emit `{schema:"boole.keys.v2", id, pk:hex32, sk:hex32, createdAt}` envelopes. New `keys_sign` handler: load envelope, refuse v1 with `legacy_v1_key` exit 3, decode `sk` via `SigningKeyV2::from_seed_hex`, sign, print bare hex64 by default or `{ok:true, envelope:<boole.signed.v1>}` under `--json`. New `keys_verify` handler: shape-check `pk`/`signature` locally before any crypto call (typed `bad_pk` / `bad_signature` envelopes exit 2 if wrong), call `verify_signature`, print bare `valid`/`invalid` (both exit 0) or `{ok:true, valid:true|false}` under `--json`. Lifted S12's `read_envelope_arg` into shared `read_json_arg(arg, field)` helper. New `is_well_formed_hex64` helper alongside the existing `is_well_formed_hex32`. `KEYS_SCHEMA: "boole.keys.v1"` constant renamed to `KEYS_SCHEMA_V2: "boole.keys.v2"`.
- `crates/boole-cli/Cargo.toml` — dropped now-unused `blake3` and `hex` deps (the dev seed lives in `boole-core::SigningKeyV2::from_dev_id`, hex coding is internal to the core module). `boole-core` re-exports cover everything the CLI needs.
- `crates/boole-cli/tests/keys.rs` — existing v1-schema assertions in `keys_new_writes_file_with_envelope_and_mode_0600` and `keys_list_returns_sorted_keys_array` flipped to v2 + added `sk: hex64` shape assertions. New `keys_list_includes_legacy_v1_envelope_unchanged` regression: pre-stages a v1 envelope by hand, asserts `keys list` lists it byte-equal (no `sk` synthesis) so operators can audit pre-S13a keys without forcing migration.
- `crates/boole-cli/tests/keys_sign.rs` — new test file with 5 tests (inline payload hex64 sig that verifies via `verify_signature`, v1 key `legacy_v1_key` exit 3 with `id` + `schema` fields, unknown id `key_not_found` exit 3, file-path payload, `--json` full `boole.signed.v1` envelope).
- `crates/boole-cli/tests/keys_verify.rs` — new test file with 5 tests (valid → stdout `valid` exit 0, tampered payload → `invalid` exit 0, wrong pk → `invalid` exit 0, malformed pk → stderr `bad_pk` exit 2, malformed signature → stderr `bad_signature` exit 2).
- `docs/boole-node-cli-parity-plan.md` — §1.2 (`keys new` row note + new `keys sign` / `keys verify` rows at PASS), §3 C2 PASS extended with the S13a paragraph, Appendix A · S13a entry, this §11 ship log entry.
- `tasks/todo.md` — replaced with the S13a plan (locked scope decisions, file inventory, TDD plan, acceptance evidence) + Review section filled in.

**Behavior changes (operator-visible):**
- `boole keys new --id <id> [--dev]` now emits `boole.keys.v2` envelopes carrying `sk` (hex64 ed25519 seed). Existing v1 envelopes stay readable via `keys list` / `keys show`.
- `boole keys sign --id <id> --payload <path|inline> [--json]` — bare hex64 signature default; `--json` emits `{ok:true, envelope:{schema:"boole.signed.v1", payload, pk, signature}}`. v1 keys → stderr `{ok:false, reason:"legacy_v1_key", id, schema, detail}` exit 3. Unknown id → `key_not_found` exit 3.
- `boole keys verify --pk <hex32> --signature <hex64> --payload <path|inline> [--json]` — bare `valid`/`invalid` default (both exit 0 because verification ran successfully); `--json` emits `{ok:true, valid:true|false}`. Wire-malformed pk/sig → stderr `bad_pk`/`bad_signature` exit 2.
- Stateless: `keys verify` does NOT touch `BOOLE_KEYS_DIR`. Anyone with `pk`/`signature`/`payload` can verify without operator key material.

**Implementation notes:**
- **Error split is deliberate.** Wire-malformed inputs (`Err → exit 2 bad_pk`/`bad_signature`) signal "your call shape is wrong" — equivalent to a 4xx. Cryptographically wrong (`Ok(false) → stdout invalid exit 0`) signals "verification ran and produced a definitive answer" — equivalent to a 200 `invalid` envelope. Both paths are first-class; conflating them would force callers to special-case "did this actually run?" Prevents spurious retries on valid `invalid` outcomes.
- **No implicit v1 → v2 upgrade.** v1 has no stored `sk`, so "upgrade" means "generate a new keypair and overwrite". That changes `pk`, which is the operator's identity. We refused to do that implicitly; operators rotate manually by creating a new key under a different id and updating downstream consumers. A future `boole keys rotate --id <id> --confirm-pk-change` can land if there's user demand.
- **`--dev` keeps the same domain prefix as v1.** The `blake3("boole-keys-dev::" || id)` derivation is identical to S6's `dev_pk_hex` in spirit; only the consumer changed (was a hex-encoded blake3 digest as `pk`, now an ed25519 seed). Dev keys still don't collide with random keys for the same id (proven by the unchanged `keys_new_dev_is_deterministic_from_id` regression).
- **Canonical JSON divergence from full RFC 8785.** Number normalization and escape minimization are deferred. The current scheme defeats "object keys re-ordered by another implementation" — sufficient for today's signing flow. Documented in `canonical_json.rs` so the future swap is localized.
- **Keys-dir contract preserved.** Env override `BOOLE_KEYS_DIR`, `$HOME/.boole/keys` fallback, mode-0600 atomic tmp+rename writes, kebab `bad_request`/`key_already_exists`/`key_not_found` typed errors — all unchanged from S6.

**Verification:**
- `cargo test -p boole-core -p boole-cli` PASS — 8 new boole-core tests + 10 new + 1 modified boole-cli tests; `cargo test -p boole-node` regression PASS.
- `cargo clippy --all-targets -- -D warnings` clean across the workspace.
- Manual CLI smoke: `keys new --dev` → on-disk envelope `{schema:"boole.keys.v2", id, pk, sk, createdAt}` mode 0600; `keys sign` → bare hex64 + `--json` full `boole.signed.v1` envelope; `keys verify` valid → `valid` exit 0, tampered → `invalid` exit 0, `--json` → `{ok:true, valid:true|false}`; bad-hex `--pk` / `--signature` → `bad_pk` / `bad_signature` exit 2; pre-staged v1 file + `keys sign` → `legacy_v1_key` exit 3; mixed v1+v2 listing surfaces both byte-equal.

**Downstream unblocked:**
- **Slice S13b (bounty announce flow).** `POST /bounties` route can now consume `boole.signed.v1` envelopes via `boole-core::verify_signature`; `boole bounty announce` CLI can shell to `boole keys sign` (or call `SigningKeyV2` directly) for the operator-key signing step.
- **Future ticket-signing path.** `/ticket` POST gates can require a `boole.signed.v1` envelope to attribute submissions to a specific operator without trusting transport headers.
- **Future P2P gossip envelopes.** Any peer message that needs cryptographic provenance (block proposals, share announcements) can wrap its payload in `boole.signed.v1`.

**Open follow-ups (not blocking S13a):**
- **Slice S13b (`POST /bounties` announce + `FileBountyRegistry` + `boole bounty announce` CLI).** The signing primitives are ready; S13b only adds the route handler, registry mutation log, and CLI driver.
- **`keys rotate / upgrade` (operator-confirmed pk rotation).** Implicit retrofit is dangerous; an explicit confirm-flagged command is the safe path.
- **Schema v3 keys with sealed-box / passphrase-encrypted `sk`.** Today the seed is on disk in plaintext (mode 0600). Encryption-at-rest is a follow-up hardening.
- **Full RFC 8785 canonical JSON.** Number normalization + escape minimization — parity hardening when on-chain commitment lands.

### Slice S12 [N+C] — Phase N5 bounty proof flow + Lean verifier wiring + `boole bounty submit` — SHIPPED 2026-05-09

**Files touched:**
- `crates/boole-core/src/bounty_proof_verifier.rs` — new module. `pub trait BountyProofVerifier: Send + Sync { fn verify(&self, bounty: &Bounty, envelope: &Value) -> Result<bool, String>; }`. Trait name disambiguates from the existing `BountyVerifier { kind, metadata }` schema struct.
- `crates/boole-core/src/bounty_ledger.rs` — added `FileBountyEventLedger { path: PathBuf }` with `append(path, &event)` (validates `schemaVersion == 1` + required keys, creates parent dirs, appends one JSON line) and `recover(path) -> Vec<Value>` (empty when file absent; validates each line). NDJSON discipline matches `FileRewardLedger`.
- `crates/boole-core/src/lib.rs` — re-export `BountyProofVerifier`, `FileBountyEventLedger`, `BountyEventLedger`.
- `crates/boole-core/tests/bounty_event_ledger.rs` — 3 new tests (append+recover round-trip, NDJSON line discipline, schema validation rejection).
- `crates/boole-node/src/lean_bounty_verifier.rs` — new module. `pub struct LeanBountyVerifier { checker_dir: PathBuf }` impl `BountyProofVerifier`. Envelope shape `{leanSource: "<utf8>"}`. Per-call temp dir uses `pid-nanos-AtomicU64` (the AtomicU64 was added after parallel-execution collision was observed in the test suite). Reads `verifierHash` from `bounty.verifier.metadata`, builds `LeanRunnerConfig::new(verifier_hash).with_package_dir(checker_dir)`, calls `LeanRunner::check_file`, returns `Ok(result.accepted)`.
- `crates/boole-node/src/local_node.rs` — `LocalNodeConfig` gains `bounty_event_ledger_path: Option<PathBuf>` + `bounty_verifiers: Option<HashMap<String, Arc<dyn BountyProofVerifier>>>` (test-injection seam). Removed `#[derive(Debug, Clone)]` since trait-object Arc has no Debug. `LocalNodeState`: swapped `bounties: Vec<Bounty>` for `bounty_registry: Arc<RwLock<BountyRegistry>>`, added `bounty_event_ledger_path: Option<PathBuf>` + `bounty_verifiers: HashMap<...>`. Boot path: seeds registry from the static catalog via `apply_event_fixture` create events, then walks the audit ledger (`replay_bounty_audit_event`) to call `BountyRegistry::submit_proof` for every accepted proof. New route `POST /bounties/{id}/proof` → `bounty_proof_handler` → `bounty_proof_json` implements the 8-branch validation chain. Audit event schema `{schemaVersion:1, kind:"proof", workId, problemHash, verifierKind, ts, proofHash, solverPk, accepted, reward, credit}`.
- `crates/boole-node/src/http_error.rs` — 5 new constructors: `bad_proof_hash()` (400), `bad_prover()` (400), `no_verifier(kind)` (501), `bounty_terminal(status)` (409), `verifier_error(detail)` (502). Reason kebabs: `bad_proof_hash`, `bad_prover`, `no_verifier`, `bounty_terminal`, `verifier_error`.
- `crates/boole-node/src/main.rs` — `run_local_command`: parse `--bounty-events` / `BOUNTY_EVENT_LEDGER_PATH` and `--lean-checker-dir` / `LEAN_CHECKER_DIR`. When `--lean-checker-dir` is set, register `lean → LeanBountyVerifier` in the default verifier registry. Boot log line `bountyEvents=<path|<none>>`. `print_help` updated.
- `crates/boole-node/src/lib.rs` — `pub mod lean_bounty_verifier;`.
- `crates/boole-node/tests/bounty_proof_route.rs` — 8 integration tests (accept → status flip + ledger append, reject → status open + ledger append, dedup → cached + no second verifier call + no second ledger event, unknown id 404, bad proofHash 400, bad prover 400, terminal bounty 409, unknown kind 501).
- `crates/boole-node/tests/bounty_event_ledger_recovery.rs` — 2 integration tests (recovered event byte-equal to appended event, second boot replays audit log to restore solved status without re-running verifier).
- `crates/boole-node/tests/bounty_lean_verifier.rs` — 2 lake/lean-gated tests (valid Lean source `2 + 2 = 4 by decide` accepts; invalid Lean source `1 + 1 = 3 by decide` rejects, returns `Ok(false)` not `Err`).
- `crates/boole-cli/src/main.rs` — `BountyCommand::Submit { id, proof_hash, prover, envelope, node, json }`. New `fn bounty_submit(...)` POSTs `{proofHash, prover, envelope}` to `/bounties/{id}/proof`, default prints bare `<status>` word (`solved`/`open`/`duplicate`), `--json` forwards the server envelope. New `fn read_envelope_arg()` helper accepts inline JSON or a file path (1 MiB cap). New `fn http_post()` helper for JSON body POSTs.
- `crates/boole-cli/tests/bounty_submit_cli.rs` — 5 integration tests against a live local node with the verifier-injection seam (default-accept prints `solved`, default-reject prints `open`, default-dedup prints `duplicate`, `--json` envelope, unknown id stderr+exit 1).
- `fixtures/protocol/bounties/v1-mock.json` — new fixture with 4 entries (gamma-1 mock-accept, delta-1 mock-reject, epsilon-1 mock-accept/withdrawn, zeta-1 unknown-kind) for the test-only verifier-injection seam. `v1.json` stays byte-frozen for S11 tests.
- 11 `LocalNodeConfig` call-sites backfilled with `bounty_event_ledger_path: None, bounty_verifiers: None`: `crates/boole-node/tests/{concurrency,http_contract,work_route,bounty_route,local_node[×3],account_balance_route}.rs`, `crates/boole-cli/tests/{work_cli,account_balance_cli,bounty_cli,node_block}.rs`.
- `docs/boole-node-cli-parity-plan.md` — §1.1 (`POST /bounties/:id/proof` row added at PASS, announce moved to S13+), §1.2 (`boole bounty submit` row at PASS), §3 N5 phase header extended to "Slices S11 + S12", §3 C3 PARTIAL extended with the bounty submit line, Gate P4 bounty proof-write portion PARTIAL PASS, Appendix A · S12 entry, this §11 ship log entry.
- `tasks/todo.md` — Review section filled in.

**Behavior changes (operator-visible):**
- `boole-node run-local` accepts `--bounty-events <path>` (env `BOUNTY_EVENT_LEDGER_PATH`) and `--lean-checker-dir <path>` (env `LEAN_CHECKER_DIR`). Boot log adds `bountyEvents=<path|<none>>`. The `lean` verifier kind is registered when a checker dir is configured.
- `POST /bounties/:id/proof` accepts `{proofHash:hex32, prover:hex32, envelope:any}`. 200 on accept/reject (status flip on accept), 200 on dedup with `duplicate:true, accepted:cached`. Typed errors: 404 `bounty_not_found`, 400 `bad_proof_hash`, 400 `bad_prover`, 501 `no_verifier`, 409 `bounty_terminal`, 502 `verifier_error`.
- `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32> --envelope <path|inline>` — bare `<status>` word default, `--json` envelope, typed errors on stderr + exit 1.
- Restart-replay: a node booted with the same `--bounty-events` path replays accepted proofs into the registry so `GET /bounties/:id` returns `status:"solved"` without re-running the verifier.

**Implementation notes:**
- **Trait visibility into config.** `Arc<dyn BountyProofVerifier>` doesn't auto-impl `Debug`, so `LocalNodeConfig` lost its `#[derive(Debug, Clone)]`. Verified neither was used by any caller (only field-by-field construction).
- **Two ledgers, only one persisted in S12.** The audit ledger (`schemaVersion:1`, `kind:"proof"`) is the only NDJSON file added. The `BountyRegistry` mutation log (kind:"create"|"status"|"proof") is rehydrated at boot by re-running `create` over the static catalog and replaying the audit log's accepted proofs. S13+ can ship `FileBountyRegistryLog` parallel to pof's `FileBountyRegistry` once announce events land.
- **Validation order.** 404 → `bad_proof_hash` → `bad_prover` → dedup peek (no verifier call) → `no_verifier` 501 → `bounty_terminal` 409 → verifier dispatch → audit append. The dedup peek BEFORE the verifier call is the byte-frozen pof contract — duplicate submissions never re-run Lean.
- **Reward credit deferred.** The audit event records `reward` and `credit` fields but does NOT mutate `RewardLedger`. pof marks reward credit as a Phase 7+ placeholder; S12 mirrors that. A future slice can wire `accepted:true → RewardLedger::credit(solverPk, reward)`.
- **Parallel test temp-dir collision.** During full regression, `lean_verifier_rejects_invalid_proof_envelope` flaked once when two parallel test threads picked the same `nanos`-based temp suffix. Adding `static COUNTER: AtomicU64` to `LeanBountyVerifier` makes the suffix collision-free across any number of threads.

**Verification:**
- `cargo test -p boole-core -p boole-node -p boole-cli` PASS (20 new tests, no regressions).
- `cargo clippy -p boole-core -p boole-node -p boole-cli --all-targets -- -D warnings` clean.
- Live lake/lean run via `bounty_lean_verifier` (Lean 4.29.1, lake 5.0.0): both accept + reject paths PASS.
- Manual CLI smoke against `boole-node run-local --bounties .../v1-mock.json --bounty-events .../events.ndjson` confirms typed envelopes for unknown id (404), bad proofHash (400), bad prover (400), unknown kind (501 for both `wholly-unknown-kind` and `mock-accept` — production binary doesn't ship mock kinds, which is by design).

**Downstream unblocked:**
- **Slice S13 (bounty announce flow).** The audit ledger and `POST` plumbing now exist; S13 only adds operator-key signing and registry create-event persistence on top.
- **Reward credit on accepted proofs.** S12's audit event already carries `reward` and `credit` fields; a follow-up slice can wire those into `RewardLedger::credit`.
- **Future `boole mine bounty --id <id>`.** Miners can shell into `boole bounty get --id <id> --json` to learn `verifierHash` + envelope shape, grind a proof, then `boole bounty submit` against a live node.

**Open follow-ups (not blocking S12):**
- **`POST /bounties` announce route + signed envelopes.** Needs ed25519 signing on top of C2 keys.
- **`FileBountyRegistryLog` parallel to pof's `FileBountyRegistry`.** Hardening for when the registry holds canonical create/status events.
- **Wasm verifier kind.** Out of scope; needs a separate runtime (S14 candidate).
- **Reward credit on accept.** Wire `accepted:true → RewardLedger::credit(solverPk, reward)`.

### Slice S11 [N+C] — Phase N5 partial bounty read API + `boole bounty list/get` — SHIPPED 2026-05-09

**Files touched:**
- `crates/boole-core/src/bounty_registry.rs` — added `BountyList { version: u32, bounties: Vec<Bounty> }` envelope (camelCase Deserialize) and `pub fn load_bounties(path: &Path) -> anyhow::Result<Vec<Bounty>>` that parses the file, validates `version == 1`, and returns the inner list. The pre-existing `BountyRegistry` (mutation API: create, update_status, submit_proof) is untouched — read path uses `Vec<Bounty>` directly so the slice doesn't pull in registry invariants. S12's announce flow will swap to `BountyRegistry` at boot when event ledger writes land.
- `crates/boole-core/src/lib.rs` — re-export `load_bounties` and `BountyList`.
- `crates/boole-core/tests/bounty_loader.rs` — new test file with 3 tests (loads v1 fixture with two entries, rejects bad version, accepts empty list).
- `crates/boole-node/src/local_node.rs` — extended `LocalNodeConfig` with `pub bounties_path: Option<PathBuf>` and `LocalNodeState` with `bounties: Vec<Bounty>` (loaded once at boot via `load_bounties` when path is `Some`, else empty Vec). Added `bounty_list_handler` (GET `/bounties`) + `bounty_by_id_handler` (GET `/bounties/{id}`) wired into `build_router`. Added `bounty_list_json` and `bounty_by_id_json` JSON builders.
- `crates/boole-node/src/http_error.rs` — added `pub fn bounty_not_found(id)` constructor (404, reason `"bounty_not_found"`, extra `id` field). Mirrors `work_not_found` exactly.
- `crates/boole-node/src/main.rs` — `run_local_command`: parse `--bounties` flag + `BOUNTIES_PATH` env, plumb into `LocalNodeConfig`. Added boot log line `boole-node local bounties=<path|<none>>`. Updated `print_help` to surface the new flag and env entry.
- `crates/boole-node/tests/bounty_route.rs` — new test file with 4 integration tests (empty list when no path, list returns loaded bounties, by-id returns match, by-id unknown returns 404 typed envelope).
- `crates/boole-cli/src/main.rs` — added `Command::Bounty { command: BountyCommand }` with `BountyCommand::List/Get` variants. Added `fn bounty_list(node, json)` (terse `<id>\t<domain>\t<status>\t<reward>` 4-column table by default, server envelope under `--json`) and `fn bounty_get(id, node, json)` (bare `verifier.metadata.verifierHash` by default, server envelope under `--json`, 404 forwarded to stderr + exit 1).
- `crates/boole-cli/tests/bounty_cli.rs` — new test file with 5 integration tests against a live local node (list default + json, get default + json, get unknown forwards typed error exit 1).
- `fixtures/protocol/bounties/v1.json` — new fixture with 2 bounties (`alpha-1`: lean.protocol-invariant, status=open, reward=42, lean verifier with `verifierHash=cdcd…cdcd`; `beta-1`: code.spec-template, status=solved, reward=11, wasm verifier with `verifierHash=dddd…dddd`).
- 9 `LocalNodeConfig` call-sites updated with `bounties_path: None`: `crates/boole-node/tests/{concurrency,http_contract,local_node,account_balance_route,work_route}.rs`, `crates/boole-cli/tests/{node_block,account_balance_cli,work_cli}.rs`. (`tests/local_node.rs` had 3 sites; `replace_all` covered them.)
- `docs/boole-node-cli-parity-plan.md` — §1.1 + §1.2 rows flipped to PASS, §3 N5 phase header marked PARTIAL PASS, §3 C3 PARTIAL extended with the bounty line, Gate P4 split (bounty read portion PARTIAL PASS), Appendix A · S11 entry, this §11 ship log entry.
- `tasks/todo.md` — Review section filled in.

**Behavior changes (operator-visible):**
- `boole-node run-local` accepts `--bounties <path>` and `BOUNTIES_PATH` env var. Boot now logs `boole-node local bounties=<path|<none>>` alongside the existing `blockStore` / `rewardLedger` / `workManifests` lines.
- `GET /bounties` returns `{ok:true, bounties:[<Bounty>...]}` (200 always; empty array when no bounties configured).
- `GET /bounties/:id` returns `{ok:true, bounty:<Bounty>}` on hit, 404 + `{ok:false, reason:"bounty_not_found", id}` on miss.
- `boole bounty list [--node URL] [--json]` — terse 4-column table by default (`<id>\t<domain>\t<status>\t<reward>` per line), full server envelope under `--json`.
- `boole bounty get --id <ID> [--node URL] [--json]` — bare `verifier.metadata.verifierHash` by default, full envelope under `--json`. Unknown id forwards the server's `bounty_not_found` envelope to stderr with exit 1, matching `work get` precedent.

**Implementation notes:**
- The `Bounty` type itself pre-existed in `boole-core::bounty_registry` (added during earlier porting). S11's loader-only addition keeps the slice tight: no new public type surface beyond the bundling envelope and a single helper.
- `LocalNodeState.bounties` is loaded once at boot and read-only thereafter. There is no live mutation path — POST `/bounties` (announce) requires operator-key signing + bounty event ledger writes, which together justify S12 as a separate slice.
- `bounty_by_id_json` walks the in-memory `Vec` linearly, identical reasoning to `work_by_id_json`. Switch to indexed storage if catalogs grow past 100 entries.
- 4-column terse output (vs 3-column for `work list`) — the extra column is `reward`, which is bounty-specific and operator-relevant. The shape stays `cut -f1` / `awk '$3 == "open"'` friendly without needing a JSON parser.

**Acceptance evidence:**
- `cargo test -p boole-core --test bounty_loader -p boole-node --test bounty_route -p boole-cli --test bounty_cli` → 12/12 PASS (3 + 4 + 5).
- `cargo test -p boole-core -p boole-node -p boole-cli` → 169 PASS / 0 FAIL clean. (One transient port-collision rerun isolated PASS; subsequent rerun confirmed 169/0.)
- `cargo clippy --all-targets` → clean, no new warnings.
- Manual smoke against `boole-node run-local --bounties fixtures/protocol/bounties/v1.json --port 8901`:
  - `GET /bounties` returns the 2-bounty list.
  - `GET /bounties/alpha-1` returns the alpha-1 envelope with `verifierHash=cdcd…cdcd`.
  - `GET /bounties/no-such` returns HTTP 404 + `{"id":"no-such","ok":false,"reason":"bounty_not_found"}`.
  - `boole bounty list` prints `alpha-1\tlean.protocol-invariant\topen\t42\nbeta-1\tcode.spec-template\tsolved\t11`.
  - `boole bounty get --id alpha-1` prints `cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd`.
  - `boole bounty get --id no-such` prints `{"id":"no-such","ok":false,"reason":"bounty_not_found"}` to stderr and exits 1.

**Deviations from plan §3 N5 / §Appendix A · S11:**
1. **Storage = `Vec<Bounty>`, NOT `BountyRegistry`.** Plan §3 N5 task 1 said "wire `bounty_registry.rs` into `RuntimeAdmissionState`". S11 wires only the read surface and skips the registry. Rationale: `BountyRegistry`'s mutation API (status transitions, proof bookkeeping, deterministic `created_at`/`updated_at` from `ts`) is correct for S12's POST surface but adds invariants that read-only catalog loading doesn't need. S12 will swap to registry-backed storage when announce events land — the swap is local to `from_config` boot.
2. **POST `/bounties` deferred to S12.** Plan §3 N5 task 2 grouped GET + POST. S11 ships GET only; POST needs the operator-key signing path (C2 keys are ready) plus a `BountyEventLedger` for persistence. Bundling all three in one slice would have made it the largest slice in the project; splitting keeps each diff reviewable.
3. **`FamilyManifest` activation gating deferred to S12.** Plan §3 N5 task 3. The type is already defined in `boole-core::family_manifest`; wiring it into promotion lives with the side-pool work, not the read surface.
4. **CLI command shape `boole bounty list/get` (not `boole bounty announce/submit`).** Plan §1.2 listed the full CLI surface; S11 ships only the read commands. Write commands (`announce`, `submit`) belong with S12's POST surface — they need signed envelope construction.
5. **Bundle the CLI surface into the same slice as the node-side routes.** Same pattern as S9 (N3 + C3-account) and S10 (N4 + C3-work). Bundling lets integration tests prove the wire shape end-to-end against a real node.

**Downstream unblocked:**
- **Slice S12 (bounty announce flow).** The read surface gives the announce flow a verification target — `POST /bounties` + a fresh `boole bounty announce` CLI succeed iff a subsequent `boole bounty list` shows the new bounty. The catalog seam is ready; S12 just adds writes through it.
- **Future `boole mine bounty --id <id>` (Phase C4).** Miners can shell into `boole bounty get --id <id> --json` to learn the verifier hash + reward before grinding shares. Same pattern as `boole work get` for the work catalog.
- **Cross-node bounty catalog audit.** Operators can diff `curl http://node-a/bounties | jq -S` vs `curl http://node-b/bounties | jq -S` once a multi-node testnet stands up.

**Open follow-ups (not blocking S11):**
- **`BountyRegistry`-backed storage at boot.** Will land with S12's announce events; the read API stays unchanged (`Vec<Bounty>` is what `BountyRegistry::list_bounties()` returns anyway).
- **`boole bounty list --filter status=open`.** Server-side filter is straightforward; deferred until a real consumer asks.
- **`bounty_by_id` HashMap lookup.** Linear scan fine for <100 bounties; revisit if catalogs grow.

### Slice S10 [N+C] — Phase N4 work API + `boole work list/get` — SHIPPED 2026-05-09

**Files touched:**
- `crates/boole-core/src/work_manifest.rs` — added `WorkManifestList { version: u32, work: Vec<WorkManifest> }` envelope (camelCase Deserialize) and `pub fn load_work_manifests(path: &Path) -> anyhow::Result<Vec<WorkManifest>>` that parses the file, validates `version == 1`, and returns the inner list.
- `crates/boole-core/src/lib.rs` — re-export `load_work_manifests` and `WorkManifestList`.
- `crates/boole-core/tests/work_manifest_loader.rs` — new test file with 3 tests (loads v1 fixture, rejects bad version, accepts empty list).
- `crates/boole-node/src/local_node.rs` — extended `LocalNodeConfig` with `pub work_manifests_path: Option<PathBuf>` and `LocalNodeState` with `work_manifests: Vec<WorkManifest>` (loaded once at boot via `load_work_manifests` when path is `Some`, else empty Vec). Added `work_list_handler` (GET `/work`) + `work_by_id_handler` (GET `/work/:id`) wired into `build_router`. Added `work_list_json` and `work_by_id_json` JSON builders.
- `crates/boole-node/src/http_error.rs` — added `pub fn work_not_found(id)` constructor (404, reason `"work_not_found"`, extra `id` field).
- `crates/boole-node/src/main.rs` — `run_local_command`: parse `--work-manifests` flag + `WORK_MANIFESTS_PATH` env, plumb into `LocalNodeConfig`. Added boot log line `boole-node local workManifests=<path|<none>>`. Updated `print_help` to surface the new flag and env entry.
- `crates/boole-node/tests/work_route.rs` — new test file with 4 integration tests (list returns loaded manifests, by-id returns match, by-id unknown returns 404 typed, routes empty when no manifests path).
- `crates/boole-cli/src/main.rs` — added `Command::Work { command: WorkCommand }` with `WorkCommand::List/Get` variants. Added `fn work_list(node, json)` (terse `<workId>\t<familyId>\t<status>` table by default, server envelope under `--json`) and `fn work_get(id, node, json)` (bare `verifier.metadata.verifierHash` by default, server envelope under `--json`, 404 forwarded to stderr + exit 1).
- `crates/boole-cli/tests/work_cli.rs` — new test file with 5 integration tests against a live local node (list default + json, get default + json, get unknown forwards typed error exit 1).
- `fixtures/protocol/work/v1.json` — new fixture with 2 manifests (bounty-derived `lean-bounty-1`, direct `smart-contract-invariant-v01-direct`).
- 7 `LocalNodeConfig` call-sites updated with `work_manifests_path: None`: `crates/boole-node/tests/{concurrency,http_contract,local_node,account_balance_route}.rs`, `crates/boole-cli/tests/{node_block,account_balance_cli}.rs`. (`tests/local_node.rs` had 3 sites; `replace_all` covered them.)
- `docs/boole-node-cli-parity-plan.md` — §1.1 + §1.2 rows flipped to PASS, §3 N4 phase header marked PASS, §3 C3 PARTIAL extended with the work line, Gate P4 row split (work portion PARTIAL PASS), Appendix A · S10 entry, this §11 ship log entry.
- `tasks/todo.md` — Review section to follow.

**Behavior changes (operator-visible):**
- `boole-node run-local` accepts `--work-manifests <path>` and `WORK_MANIFESTS_PATH` env var. Boot now logs `boole-node local workManifests=<path|<none>>` alongside the existing `blockStore` / `rewardLedger` lines.
- `GET /work` returns `{ok:true, work:[<WorkManifest>...]}` (200 always; empty array when no manifests configured).
- `GET /work/:id` returns `{ok:true, work:<WorkManifest>}` on hit, 404 + `{ok:false, reason:"work_not_found", id}` on miss.
- `boole work list [--node URL] [--json]` — terse table by default (`<workId>\t<familyId>\t<status>` per line, no header — keeps it `column -t`-friendly without needing post-processing), full server envelope under `--json`.
- `boole work get --id <ID> [--node URL] [--json]` — bare `verifier.metadata.verifierHash` by default (the obvious useful field for downstream miners), full envelope under `--json`. Unknown id forwards the server's `work_not_found` envelope to stderr with exit 1, matching `block get` precedent.

**Implementation notes:**
- The `WorkManifest` type itself pre-existed in `boole-core` (added during earlier manifest fixture porting). S10's loader-only addition keeps the slice tight: no new public type surface, just a bundling envelope and a single helper.
- `LocalNodeState.work_manifests` is loaded once at boot and read-only thereafter. There is no live mutation path (no `POST /work`) — pof has none either, and bounty announce belongs to N5's `/bounties` route, not work. Refresh-on-SIGHUP could be a future improvement, but the static-catalog assumption is what every existing test boots against.
- `work_by_id_json` walks the in-memory `Vec` linearly. With <100 manifests that is faster than a `HashMap` lookup once you account for cache effects; if catalogs grow past that, switch to indexed-by-`work_id` storage. Meanwhile no API surface changes.
- The CLI `work get` default extracts `verifier.metadata.verifierHash` rather than `verifier.metadata.profile` or `familyId`; consumers that want a different "obvious field" can use `--json` and `jq`. This mirrors the `account balance` choice of "the bare balance string" as the obvious useful field.
- The 5-line `<workId>\t<familyId>\t<status>` shape was picked over a JSON-per-line shape because the latter is harder to read in a terminal. `--json` already covers structured-output consumers, and the tab-separated form survives `cut -f1` / `awk '$3 == "open"'` without needing a JSON parser.

**Acceptance evidence:**
- `cargo test -p boole-core --test work_manifest_loader -p boole-node --test work_route -p boole-cli --test work_cli` → 12/12 PASS (3 + 4 + 5).
- `cargo test -p boole-core -p boole-node -p boole-cli` → full regression PASS, no pre-existing tests modified or affected.
- `cargo clippy -p boole-core -p boole-node -p boole-cli --tests` → clean, no new warnings.
- Manual smoke against `boole-node run-local --work-manifests fixtures/protocol/work/v1.json`:
  - `GET /work` returns the 2-manifest list.
  - `GET /work/lean-bounty-1` returns the bounty-derived manifest.
  - `GET /work/no-such` returns 404 + `{"id":"no-such","ok":false,"reason":"work_not_found"}`.
  - `boole work list` prints `lean-bounty-1\tlean.protocol-invariant\topen\nsmart-contract-invariant-v01-direct\tsmart-contract-invariant-v01\topen`.
  - `boole work get --id lean-bounty-1` prints `cdcd…cdcd` (verifier hash).
  - `boole work get --id smart-contract-invariant-v01-direct --json` prints the full envelope.
  - `boole work get --id no-such` prints `{"id":"no-such","ok":false,"reason":"work_not_found"}` to stderr and exits 1.

**Deviations from plan §3 N4 / §Appendix A · S10:**
1. **`WorkManifest` type re-use, not new definition.** The plan listed "Define `WorkManifest` type in `boole-core`" as Task 1; the type already existed from earlier manifest-fixture porting work. S10 only added the loader envelope. Documented here so future readers don't duplicate.
2. **Bundle the CLI surface (`boole work list/get`) into the same slice as the node-side routes.** Plan §3 marks the CLI as Phase C3 (separate). S10 ships them together — same pattern as S9 (N3 + C3-account). Bundling lets the integration tests prove the wire shape end-to-end against a real node, not just unit-tested handler return values.
3. **No `--all` or `--filter` flags on `boole work list`.** Today's catalog is small (<10 entries); filtering is a follow-up if catalogs grow.

**Downstream unblocked:**
- **Future `boole mine bounty --id <id>` (Phase C4).** The miner can shell into `boole work get --id <id> --json` to learn the verifier hash before grinding shares. The work catalog is the single source of truth for "what work is this node willing to accept".
- **Phase N5 bounty announce flow.** Bounty mutations land via `POST /bounties` (separate route); promoted bounties surface here as additional `WorkManifest` entries with `source: "bounty"`. The split keeps mutation auth scoped to the bounties route.
- **Cross-runtime catalog audit.** Operators can diff `curl http://node-a/work | jq -S` vs `curl http://node-b/work | jq -S` to verify two nodes serve the same work catalog. Useful before standing up a multi-node testnet.

**Open follow-ups (not blocking S10):**
- **`boole work list --filter status=open`.** Server-side filter would also be straightforward; deferred until a real consumer asks.
- **HashMap-backed `work_by_id` lookup.** Linear scan is fine for <100 manifests; revisit if a deployment grows past that.
- **Refresh-on-SIGHUP for the static catalog.** Today the catalog is process-lifetime-static. A SIGHUP listener could re-load; deferred until operator UX needs it.
- **Schema validation beyond serde shape.** The loader doesn't currently dedup on `workId` or whitelist `verifier.kind`. When N5 reuses the type for bounty announce, those checks become stricter and can move into the loader (or a separate `validate_work_manifest_list` pass).
