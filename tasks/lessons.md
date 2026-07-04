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

## 2026-06-02 — Focused clippy ≠ gate clippy: always pass `-- -D warnings`

**Pattern:** P2.1's focused check `cargo clippy -p boole-mcp --all-targets`
returned exit 0 with no visible warnings, so I launched the full gate.
The gate FAILED at the `cargo-clippy` stage (GATE_EXIT=101) on a
`clippy::clone_on_copy` lint — `GrinderConfig` is `Copy`, so
`bounded.clone()` is a lint. That burned a multi-hour gate run for a
one-line fix.

**Root cause:** `scripts/self-test.sh:59-60` runs clippy as
`cargo clippy --workspace --all-targets --locked -- -D warnings` (and a
second pass with dev features). The `-D warnings` promotes EVERY lint to
an error. My focused `cargo clippy -p <crate>` without `-- -D warnings`
left lints as warnings that my grep for `error:` never caught.

**Rule:** before launching the (5-15h) full gate, reproduce the gate's
clippy exactly: `cargo clippy --workspace --all-targets --locked -- -D
warnings` (and the dev-features variant). Never trust a focused
`cargo clippy -p <crate>` without `-- -D warnings` as a gate proxy — it
silently downgrades the very lints the gate fails on. Cheap pre-gate
check that saves a whole gate cycle.

## 2026-06-01 — Gate log mtime frozen ≠ hang (never reflexively kill cargo)

**Pattern:** during the P0.5 slice-63 full gate, the main gate log
(`/tmp/p0-5-adr-gate.log`) stopped updating for 130+ minutes while stuck
on `cargo-test: RUN`. I misread the frozen mtime as a hang and assembled
a `pkill -x cargo`/`pkill -x rustc`/`pkill -f self-test.sh` batch to
"recover" it. The destructive batch only failed to execute by luck — an
earlier ports-check command in the same parallel block exited 1, which
cancelled the whole batch. The gate was in fact perfectly healthy.

**Root cause:** `scripts/self-test.sh`'s `run_logged` (lines 16-28)
redirects each stage's stdout/stderr to `$TMP_DIR/<name>.log`, NOT to the
main gate log. So the main log only ever gets the one-line
`self-test check cargo-test: RUN` marker, and its mtime stays frozen for
the entire (long, serial `RUST_TEST_THREADS=1`, host-contended)
cargo-test stage. `$TMP_DIR` is removed by an EXIT trap, so its path is
not externally observable mid-run.

**Rule:** a frozen gate-log mtime is NOT evidence of a hang. Before ever
reaching for `pkill cargo`/`pkill rustc`, prove liveness from the
PROCESS TREE instead:
- `pgrep -f self-test.sh` and `pgrep -x cargo` → both RUNNING?
- `pgrep -P <cargo-pid>` → `ps -o pid,etime,comm <child>`.
- If the child test-binary name/etime is ROTATING between probes (e.g.
  `ready_..._lean_checker_dir_path_missing` → `ready_..._ledgers_not_loaded`,
  each with a small etime), the gate is healthily stepping through the
  serial test set. Leave it alone.
Killing it discards hours of serial cargo-test work for nothing. Do not
rely on a lucky cancellation to save you next time.

## 2026-06-01 — Failed Edit + "N passed" tripwire; never batch a gate with unverified edits

**Pattern:** starting slice 64 I issued ~30 tool calls in ONE batch:
`Edit`s on `telemetry.rs`/`Cargo.toml`, then RED/GREEN `cargo test`, then
a full `self-test.sh` gate launch — together. Every Edit FAILED ("String
to replace not found") because my `old_string`s came from stale
pre-compaction memory. I didn't react (failures buried in a 30-result
batch), ran tests anyway, saw "2 passed; 16 filtered out", and launched a
5-hour gate against a tree with ZERO slice-64 changes.

**Tripwire missed:** I had ADDED 5 tests, so the focused run should show 7
(or a compile error). Seeing exactly the PRE-EXISTING count proves the
edits didn't apply.

**Rules:**
- NEVER batch an expensive/irreversible step (gate, commit, push) in the
  same tool block as the edits it depends on. Apply edits → confirm each
  landed (Edit tool reports success explicitly; or re-Read) → THEN gate.
- After "adding" tests the focused count MUST increase; if it equals the
  baseline, stop — the edit failed.
- Post-compaction, treat remembered file contents as UNTRUSTED: Read the
  real file and copy exact `old_string` from it.

## 2026-06-01 — macOS tool reality: no python tomllib, no `cat -A`; one failure cancels the batch

**Pattern:** `python3 -c "import tomllib"` and `cat -A` both errored — but
these were REAL (`python3` here is <3.11; macOS `cat` lacks `-A`), not
channel corruption. The `Exit code 1` from the python probe CANCELLED
every queued tool call after it in the same parallel batch (~20 edits +
checks lost).

**Rules:**
- Don't use `tomllib` (py<3.11 here) or GNU-only flags (`cat -A`, some
  `sed`/`grep` long opts) — they fail on this macOS host.
- A single non-zero Bash in a parallel batch can cancel the siblings
  after it. Keep edits in their OWN batch, separate from probes that may
  exit non-zero. Prefer the `Read`/`Edit` tools (explicit success) over
  shell parsing for anything load-bearing.

## 2026-06-01 — TaskStop'd gate leaves an orphan cargo holding the package-cache lock

**Pattern:** slice 64 added new deps (tracing, tracing-subscriber). Its
focused `cargo test -p boole-core` sat for 15+ min with its cargo alive
but ZERO rustc children — not compiling, BLOCKED. Process-tree dig found
a second cargo (pid 57883, parent 57881 with PPID=1 → orphaned to
launchd) in the SAME repo, running 6h09m, holding `~/.cargo/.package-cache`.
It was an orphan from a gate I had `TaskStop`'d earlier this session:
TaskStop killed the bash wrapper but the cargo subtree was reparented to
launchd and kept the package-cache lock. Earlier slices (no new deps)
never contended because cargo only takes that lock to resolve/fetch
registry deps; the moment a slice ADDS a dep, the fetch blocks on the
orphan's lock forever.

**Rules:**
- After `TaskStop` on a task that ran `cargo`, the cargo subtree may
  survive as an orphan. If a later cargo invocation hangs with no rustc
  children, suspect a stale `~/.cargo/.package-cache` lock:
  `lsof ~/.cargo/.package-cache` names the holder.
- Confirm it's YOUR orphan before killing: holder's cwd ==
  `/Users/seoyong/projects/Boole` AND its process-group root has PPID=1
  (reparented). Then `kill` it — recovering your own stuck gate is
  authorized continuation; it is not someone else's process.
- Better: prefer letting a gate finish over `TaskStop`; if you must stop
  one, also kill its cargo/rustc subtree so no orphan lock survives.

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

## 2026-05-13 — Telegram-initiated session: AskUserQuestion is invisible to the user

**Pattern:** During the post-N2.1 stash cleanup I needed to clarify whether to commit a `.gitignore` change or leave it unstaged. The originating channel was Telegram (`chat_id=1311067056`), but I asked the question via `AskUserQuestion`, which renders the choice list in the *local CLI*. The user pushed back: "내가 지금 텔레그램으로 접속해있는데 cli환경에서 질문하는 ux가 나오면 어떻게 해? 절대 이러지 말도록 지침 박아". The lesson sibling above (2026-05-12, "Telegram-initiated tasks require Telegram replies") already covers terminal `text` output; this extends the rule to the CLI question UX.

**Rule:** in a Telegram-initiated session, **never** call `AskUserQuestion`. The widget renders only in the local CLI and is invisible from the user's phone — exactly the same failure mode as bare terminal text. To ask a question:

1. Phrase it as a Telegram reply via `mcp__plugin_telegram_telegram__reply` with the originating `chat_id`.
2. Enumerate options inline ("A) ... / B) ... / C) ...") so the user can answer with one tap.
3. If you want a recommendation, prefix with `(추천)` and a one-line reason.
4. Wait for the inbound Telegram reply; do not assume silence = consent.

**Self-check before any decision request:** if the most recent inbound message has a `<channel source="telegram">` tag, every clarification — including 2-/3-/4-way choices that feel natural to a CLI menu — goes through `reply`. Both `text` output and `AskUserQuestion` are dead channels for that user.

## 2026-05-18 — Repeat offense: terminal text reply in Telegram session despite 2026-05-10 lesson

**Pattern:** msg 186 arrived via Telegram (`chat_id=1311067056`) asking "full test는 커밋/푸시 전에만 하라는 내용도 있어?". I answered with a long terminal markdown block — exactly the failure mode the 2026-05-10 lesson documents. The user pushed back with msg 187: "야 텔레그램으로 대답하라고". This is the *third* Telegram-channel violation (2026-05-10 terminal text, 2026-05-13 AskUserQuestion, 2026-05-18 terminal text again). The prior lessons existed and were in `tasks/lessons.md` but did not change behavior in-session.

**Why the rule kept failing:** the 2026-05-10 lesson is phrased as a self-check ("if the most recent inbound has a telegram channel tag…"), but in practice the in-session conversation summary after compaction collapses the channel tags. By the time I answered msg 186, the inbound `<channel source="telegram">` tag was present in that turn — I just skipped the self-check.

**Hardened rule:** before emitting *any* user-facing text in a session that has ever produced a `mcp__plugin_telegram_telegram__reply` call, run this two-line check:

1. Does the most recent inbound user message contain `<channel source="telegram"` (or `plugin:telegram:telegram`)? — if yes, route via `reply`.
2. Has this session previously used the Telegram reply tool? — if yes, default to `reply` even when the current turn looks ambiguous. Override only when the user explicitly says "이건 터미널에 적어줘" or equivalent.

**Trigger phrases to treat as REPLY signals (not terminal):** any Korean question/statement following a `<channel source="telegram">` block, including short follow-ups like "야", "어", "확인", "알겠어", and any clarification request. Short = still Telegram.

**No-exception list:** the terminal is acceptable only for (a) tool-call narration that the harness shows automatically, and (b) explicit user instruction to write to the terminal. Everything else in a Telegram session goes through `reply` — including this very lesson update would have been reported via `reply`, not text, if the user asked about it.

## 2026-05-30 — committed before confirming the full gate actually PASSED

**Pattern:** on the P1.6 cross-network matrix slice I read the self-test log with `tail`, saw the last line was `cargo-fmt: FAIL` (stage 1 bailed immediately), but registered it as PASS and committed + pushed `ea8f53c` anyway. A rustfmt-dirty file reached origin/main. Root causes: (1) I judged gate success by eyeballing a stage list instead of grepping for the explicit `self-test: PASS` terminal line; (2) my own `echo "fmt clean"` label in a background command sat just above the real rustfmt diff, and I read the label not the diff. rustfmt rewraps hand-written method chains and doc comments, so source that looks fine to the eye still fails `cargo fmt --check`.

**Rule:**
- Gate verdict is ONLY `grep -E "^self-test: (PASS|FAIL)" <log>`. Never declare a gate green by scanning the stage list. If the PASS line is absent, the gate did NOT pass — do not commit.
- self-test bails on the FIRST failing stage, so a `tail` of the log shows that stage's name as the last line, NOT `self-test: PASS`. Always grep the explicit terminal verdict.
- Run `cargo fmt` (the formatter, not `--check`) on any new/edited `.rs` file BEFORE running the gate. rustfmt reflows method chains (`opts.x.then(...)`), struct literals, and `//!`/`///` doc comments — hand-written layout is not authoritative.
- When reading background-command output, never conflate my own `echo` labels with the tool's real output; print and check the exit code explicitly.
- Recovery when a bad commit already pushed: `cargo fmt` → re-run focused tests → commit the fixup as its own NotoriAndo commit → push → verify local SHA == origin/main → re-run the FULL gate and confirm the explicit `self-test: PASS` line before considering the slice closed.

## 2026-05-30 — "tool output corrupted" was a repeated MISJUDGMENT; pausing wasted cycles

**Pattern:** twice in one long autonomous run I concluded the Bash/Read tools were "corrupted" and scheduled a 30-min pause. Both times the tools were fine. Root causes of the illusion: (1) I chased a phantom file `signed_envelope_network_rejection.rs` that never existed (the real file is `cross_network_rejection.rs`), so `wc`/`Read`/`git restore` all correctly reported "no such file" — I misread consistent absence as contamination; (2) a Bash call ran with `cwd=/Users/seoyong/projects/boole` (lowercase) instead of `…/Boole` — but macOS is case-insensitive so it is the SAME repo (git HEAD identical), harmless; (3) `grep -c` returning `0` for warning/error counts is the SUCCESS case, not a corrupted-output case.

**Rule:**
- Before declaring tools broken, run ONE deterministic sanity probe: `echo TOKEN; expr 6 \* 7; git rev-parse --short HEAD`. If it returns `TOKEN / 42 / <sha>`, the tools are fine — the problem is my query (wrong path, phantom file, or misread exit code), not the harness. Do NOT pause on suspicion.
- A file "not existing" reported consistently by ls+Read+git is GROUND TRUTH, not corruption. Verify the real filename with `git ls-files | grep <topic>` before assuming a tool fault.
- `cwd` may show `/Users/seoyong/projects/boole` (lowercase) — this is the same case-insensitive repo as `…/Boole`; not a fault. Confirm via matching `git rev-parse HEAD`.
- Reserve pausing for a REAL blocker (genuine FAIL, missing dependency, ambiguous user decision) — not for self-induced confusion. Pausing 30min on a phantom burns the autonomous budget for nothing.

## 2026-05-31 — audit the actual test file before writing a "missing" test (scout can be wrong)

**Pattern:** an Explore scout reported that the P1.5 replay-parity test (promote-some, reboot, assert exactly the unpromoted shares survive) did NOT exist and only the all-unpromoted case was covered. I was about to write it. Before authoring, I grepped the target file directly and found `boot_after_promotion_rebuilds_side_pool_to_match_pre_restart_state` already present in `crates/boole-node/tests/hard_guard_regression.rs` (line ~1100) — it does EXACTLY the mixed promoted/unpromoted reboot parity (comment: "promoted shares must not reappear, unpromoted shares must not vanish"), and it passes. The scout had scanned the 1206-line file and missed the test.

**Rule:**
- A subagent's "this test/file does not exist" claim is a HINT, not ground truth. Before writing any test a scout says is missing, run a direct `grep -n "fn .*<topic>\|<key assertion phrase>" <target_test_file>` and, for a multi-hundred-line file, grep the whole file for the invariant phrase — not just the function-name guess. Writing a duplicate test is wasted work and muddies the suite.
- This is the test-suite analogue of the older "audit existing crates before forking a shared module" lesson. Same discipline: verify absence in the real artifact, don't trust a summary.

## 2026-05-31 — very long sessions degrade bulk tool output; switch tools, don't guess

**Pattern:** deep into an enormous session, multi-section `echo;grep;grep` Bash calls and large `Read` ranges began rendering as empty / "(no output)" / abbreviated `...`, while a deterministic `echo PROBE_$(expr ...)` still returned correctly. The file contents were fine on disk (git/tests green); the harness was collapsing bulky output at the tail of a giant transcript.

**Rule:**
- When bulk output renders empty but a deterministic probe works, the tools are fine — it's output volume at session-tail. Switch to SMALLER, single-purpose calls (one grep, narrow Read window) rather than concluding corruption or pausing.
- Do NOT author code (especially tests against an exact API/schema) while you cannot reliably read that API. Guessing a `recover`/record signature risks a broken commit. Either get a clean small read first, or checkpoint and hand the precisely-scoped next step forward.
- Low-value belt-and-suspenders work (e.g. per-store torn-tail unit tests when the production path is already correct AND contract-tested) is not worth authoring blind against degraded reads at session-tail. Prefer a clean checkpoint.

## 2026-06-04 — `self-test.sh` runs cargo-test TWICE; the gate is ~14h on this host

**Pattern:** the full gate took ~14h wall-clock. Root cause: `scripts/self-test.sh` runs `cargo test --workspace` as the `cargo-test` stage AND `scripts/check-rust-parity.sh` (the `rust-parity` stage) runs `cargo test --workspace` a SECOND time (after regenerating fixtures from the pof TypeScript reference). With ~150 integration-test binaries and macOS dyld re-verifying each fresh binary's signature on first `execve` (the prewarm stage only warms the 3 production CLIs, not the test binaries), each full test run is ~6-7h, so the gate runs ~14h.

**Rules:**
- Budget ~14h for a full gate on this host; a "frozen"-looking cargo-test is almost always healthy. A test binary at `%CPU 0.0` for minutes is in dyld signature verification / page-in, NOT a deadlock. PROVE liveness by watching the cargo child rotate between binaries (`pgrep -P <cargo> → ps -o comm,etime`); the binary NAME changing between probes = healthy. The execution order is NOT alphabetical, so don't infer "near done" from the name.
- The `rust-parity` stage's cargo is a SECOND full test run, not a hang — expect the cargo-test stage to PASS, then a fresh ~6h cargo under `rust-parity`.
- Never reflexively `pkill cargo` on a slow gate; it discards the whole run.

## 2026-06-04 — worktree overlap: ship disjoint slices while a multi-hour gate runs

**Pattern:** with a ~14h P2.1 gate running in the main tree, I implemented 5 disjoint slices (P1.9/P1.7/P2.7/P2.6/P1.3b, all in boole-node/boole-lean-runner — disjoint from P2.1's boole-mcp) in a `git worktree` on a `slices-batch` branch. This converted ~14h of idle into productive slice + review + doc work; the slices merge cleanly into main after the P2.1 gate passes.

**Rules:**
- Before relying on a worktree, PROBE that the Write/Edit tools can write to its path (`git worktree add /tmp/wt HEAD`, then `Write /tmp/wt/PROBE`). Out-of-primary-dir paths may be sandboxed; confirm first.
- Warm-build the deps once (`cargo build --manifest-path <wt>/Cargo.toml --workspace --features ...`) so per-slice focused tests are incremental. Use `--manifest-path` to avoid `cd` permission prompts.
- The worktree has its OWN target dir → no build-lock conflict with the main gate. With no NEW deps, there is no `~/.cargo/.package-cache` contention either.
- Do NOT run two FULL gates concurrently: cargo-test is dyld/disk-bound and the kernel serializes it, so two gates each run ~2x slower (no net saving) and their smoke stages can collide on ports. Keep full gates serial; overlap only read-only / focused work.

## 2026-06-04 — subprocess test: a parser thread that stops reading SIGPIPEs the child

**Pattern:** `shutdown_drain.rs` spawned `boole-node`, piped stderr, and a reader thread `break`-ed after the first "listening" line. Dropping the `BufReader` closed the pipe's read end; the node's NEXT `eprintln!` (it prints several boot lines) got SIGPIPE and the process was killed before it could serve `/live` — the test failed with "node /live never returned 200". The manual run didn't repro because it redirected stderr to a FILE, not a pipe.

**Rule:** when capturing a long-lived child's piped stdout/stderr in a test, DRAIN it to EOF (keep reading after you find the line you wanted) — `let mut sent=false; for line in reader.lines() { if !sent && line.contains(..) { tx.send(line); sent=true; } }`. A reader that stops early closes the pipe and SIGPIPEs the child on its next write.

## 2026-06-04 — adversarial review BEFORE the multi-hour gate caught 6 bugs the gate could not

**Pattern:** before merging the 5 slices, an 11-agent adversarial review of the per-slice diffs (read-only, concurrent with the running gate) confirmed 6 real bugs the full gate's tests would NOT have caught, because they live in untested edge cases: (1-2) the forbidden-token scanner false-rejected SOUND proofs that mention `axiom`/`native_decide` in a `/- block comment -/` or string literal (the scan only stripped `--` line comments); (3) `is_proof_route` matched `/bounties/proof` and `/bounties/x/y/proof` via loose `starts_with`/`ends_with`; (4-5) the SIGTERM handler silently swallowed a `signal()` registration error AND raced registration vs serving; (6) HIGH — a crash between the reward append and the bounty-event append leaves the bounty-event ledger trailing and the node UNBOOTABLE (the P1.3b re-derive heal only covers the reward ledger).

**Rules:**
- A green full gate proves the tests you WROTE pass; it cannot catch a bug in a path you did not test. Run an adversarial review (a Workflow fanning a reviewer per slice + an adversarial verify per finding) on the diff BEFORE committing to a multi-hour gate — it is high-ROI and catches false-positives/false-negatives the tests miss.
- A SECURITY/soundness scanner must be lexically correct: blank comments AND strings in a SINGLE pass (a `/-` inside a `"..."` string must not start a "block comment" that blanks real code — that would be a false NEGATIVE / unsound). A naive two-pass strip is wrong.
- When fixing a found bug, prefer the SAFE direction for the domain: for a verifier scanner, over-rejection is safe (never accept an unsound proof), under-acceptance (false negative) is not — so the fix must not introduce false negatives.

## 2026-06-04 — don't ship a partial consensus-path fix; document the limitation instead

**Pattern:** the P1.3b reviewer found a HIGH unbootable-state: a crash between the reward append and the bounty-event append (on a promoted-credit block, with `--bounty-events`) leaves the bounty-event ledger trailing → boot bails on a bounty-family divergence. The "obvious" fix (re-derive only the missing `credit` events) is UNSAFE: the paired `share_promoted` events carry a `proofHash` and include zero-credit shares that are NOT recorded in `promoted_bounty_credits`, so they cannot be re-derived from the block store. `rebuild_bounty_side_pool` would then treat the already-committed share as still-pending and re-promotable → a DOUBLE-CREDIT, strictly worse than the unbootable node.

**Rule:** on a consensus/recovery path, a partial fix that trades one bug for a worse one is not a fix. When the correct closure needs an architectural change (here: staging-commit of all stores, or a block-store-aware side-pool rebuild) that you cannot validate quickly, document the exact window + why the easy fix is unsafe (code comment + ADR), narrow the slice's claim honestly, and defer — do not ship the unsafe partial.

## 2026-06-04 — avoid config-field churn for a test-injection flag

**Pattern:** `LocalNodeConfig` has no `Default` and ~50 struct-literal call sites across boole-node + boole-cli tests. P2.6 needed a disk-full injection knob; adding a `LocalNodeConfig` field would have forced a one-line edit into all ~50 sites. Instead I put the flag on `LocalNodeState` as an `Arc<AtomicBool>` (default false) + a `#[doc(hidden)] serve_local_node_with_disk_full_sentinel` test seam — zero call-site churn, and the AtomicBool is the natural home for the eventual real ENOSPC trigger. Similarly P1.9's release-refusal went on the CLI args (`--allow-insecure-verifier`), not a `LocalNodeConfig` field, keeping the library `from_config` permissive so existing node tests are untouched.

**Rule:** before adding a field to a wide, `Default`-less config struct for a TEST-only injection point, prefer a runtime flag on the state (`Arc<AtomicBool>`) + a `#[doc(hidden)]` test-seam constructor, or a CLI-arg-level guard. It avoids 50-file mechanical churn and usually models the production trigger better.

## 2026-06-09 — a green gate does not mean P0–P2 is complete; audit before claiming done

**Pattern:** I reported the codebase "P0–P2 complete" on the strength of a passing full gate. A /deep-research adversarial audit then refuted it with 24 real findings the green gate could not see: routes that AUTHENTICATED (valid signature) but never AUTHORIZED (anyone could register/revoke a session or announce/transition a bounty); boole.mine's ">0-cycle round-trip" that ran entirely through MOCK components (StubTargetEmitter + MockDriver), so no real instance was ever generated; and a heal path with no test for interleaved route+block events (the exact case that made a node unbootable).

**Rule:** a passing gate proves the tests you WROTE pass — it says nothing about authz gaps, mock-theatre, or untested edge paths. Before claiming a milestone "complete", run an adversarial audit (deep-research / a reviewer-fan-out workflow) against the actual code and reconcile every finding. Never upgrade "tests pass" to "the system is correct/complete".

## 2026-06-09 — an authorization CHECK needs a REJECT-path test, or it is unverified

**Pattern:** the P1.6 authz logic was committed with the route changes but ZERO tests asserted the 403 reject path. `grep unauthorized_signer crates/**/tests` returned nothing — the check could have been deleted and every existing (happy-path) test would still pass. The audit flagged this as a test-honesty gap.

**Rule:** when you add an authorization/security gate, the RED test MUST assert the REJECT path (non-owner → 403, unknown → 404, non-allowlisted → 403) — not merely that the happy paths still succeed. Make the happy-path tests sign AS the authorized principal (so they exercise the allow path), and add explicit deny-path tests that fail closed if the check is removed. A feature with only happy-path tests is an unverified feature.

## 2026-06-09 — run `cargo fmt --all --check` before committing hand-written Rust

**Pattern:** I hand-wrote new test functions, committed them, and launched the ~multi-hour full gate. It FAILED at stage 1 (cargo-fmt) on my unformatted code, wasting the gate cycle. cargo-fmt is the gate's first stage precisely because it is cheap and catches this.

**Rule:** any time I hand-write or hand-edit Rust, run `cargo fmt --all` (or `-p <crate>`) and then `cargo fmt --all --check` BEFORE committing and BEFORE gating. Cheap local check; a fmt-only gate restart is pure waste. (Delegated subagents already do this; the lapse was in my own inline edits.)

## 2026-06-10 — a "pinned formula" lives in N mirrors: grep scripts/ before changing it

**Pattern:** D#6 added `Boole/Family/V0Helpers.lean` to `CHECKER_PINNED_FILES`
in boole-lean-runner. The focused lean-runner tests went green, but
`runtime_smoke_cli` then failed: `scripts/proof-to-block-benchmark.sh` and
`scripts/boole-model-benchmark.py` each carry their OWN checker-workspace
writer AND their own Python reimplementation of `checker_artifact_hash`.
The strict pin made their ad-hoc workspaces unreadable (missing file) and
their mirrored hash formula stale. Same class as the canonical_checker.rs
test fallback, which had ALREADY drifted (2-file formula) before this slice.

**Rule:** before changing any consensus-adjacent formula (hash inputs, wire
shapes, pinned file lists), enumerate every reimplementation first:
`grep -rln '<key element>' crates/ scripts/ fixtures/` — test fallbacks,
shell/python mirrors, fixture writers. Fix them in the same slice, and where
possible replace mirrors with a call into the production implementation
(here: `checker_artifact_hash` was made pub so Rust tests use the real
formula; the Python mirrors remain and must be kept in lockstep manually).

## 2026-06-10 — zsh for-loop over an unquoted $(…) variable does NOT word-split

**Pattern:** `files=$(grep -rl …); for f in $files; do sed -i '' … "$f"; done`
treated the whole newline-joined list as ONE filename under zsh (no implicit
word splitting, unlike bash) — sed errored "No such file or directory" and
ZERO files were edited. The follow-up leftover-grep caught it.

**Rule:** on this host (zsh), never `for f in $var`. Use
`grep -rl … | xargs sed -i ''` or `for f in ${(f)var}`. After any bulk sed,
always run the inverse grep ("leftovers: 0") before trusting the sweep.

## 2026-06-19 — when changing a summary/JSON builder, run the WHOLE test module, not one test

**Pattern:** N1.5 added keys (difficultyMode/claimBoundary/publicMiningEvidence)
to `summary_for_log` in boole-miner cli.rs. I focused-ran only
`summary_for_log_emits_nested_agent_and_protocol_reports` (the partial-key
test) and it passed, so I committed and launched the ~14h combined gate. The
gate FAILED at cargo-test on a SIBLING test in the same module —
`mining_report_summary_matches_v1_artifact_contract_fixture` — which does an
EXACT JSON equality against `fixtures/protocol/mining-report/v1-summary.json`.
New keys → mismatch. Burned a multi-hour gate cycle. This is the THIRD time
this class bit (E#3 + D#6 were the same "fixture mirror not synced").

**Rule:** when editing any function that builds a serialized artifact
(summary JSON, /head, /status, a report, a canon package):
1. Run the ENTIRE test module / crate test target, not the one test you think
   covers it — `cargo test -p <crate> --lib <module>::tests` (or the whole
   `--lib`), not `... <module>::tests::<one_fn>`. Exact-match fixture tests
   hide as siblings of the shape test.
2. grep for fixtures the function is compared against:
   `grep -rln "<one stable key from the artifact>" fixtures/` and update every
   mirror in the SAME commit.
3. Only then launch the full gate. A fixture-only fix is cheap; a gate restart
   is ~14h.

## 2026-06-20 — Audit a "refactor/extract" task before re-implementing it

**Pattern:** RM2.3 was filed as "extract the session/submit gate into an
axum-free, directly-testable typed function." Reading the code first showed the
gate (`submit_session_gate`) was ALREADY an axum-free typed function with thin
handler delegators and 16 route tests — the only unmet part of the spec's R3
was "directly unit-testable" (it was private, covered only over HTTP). Had I
taken the title literally I'd have rewritten a working consensus-adjacent gate
and risked changing route behavior for no benefit.

**Rule:** for any "extract / split / refactor X" slice, first read X and diff it
against the spec's acceptance criteria line by line. Implement only the
*residual gap*, and write down in the decision log what was already satisfied
and why the change is the minimal closing move. The smallest behavior-preserving
edit that makes the unmet criterion true beats a faithful-to-the-title rewrite.

**Corollary (clean pure/stateful seam):** when the goal is "make validation
directly testable," look for the state-free prefix of the function — here the
envelope decode + field/format checks ran entirely before the first
`LocalNodeState` access. Extracting just that prefix (returning an owned
`ParsedSubmitSession` so the stateful suffix can re-read what it needs) gives
direct unit tests with zero change to the stateful path. Pure prefix first, then
delegate.

## 2026-06-20 — A cargo test "hung at 0% CPU" is usually a build-lock orphan, not a test bug

**Pattern:** The RM2.3 full gate appeared to "hang" at the
wallet-session-receipt-gate stage — a test binary alive 2+ min at 0.0% CPU. I
killed the named pids and re-ran, and it hung again on a *different* binary
(session_store, then session_route), also 0% CPU. Chasing the no-read-timeout
HTTP helper in those tests was a red herring: run in isolation single-threaded
they pass in ~1s. The real cause: killing a gate by its named pids left an
**orphaned `cargo test` process** (its parent script died, cargo kept running)
holding the `target/debug/.cargo-lock` build lock. Every subsequent cargo
invocation then *blocks on the flock* — which presents exactly as "process alive,
0% CPU, no rustc children, no progress."

**Rule:**
1. Before concluding "a test hangs," check for leftover cargo/rustc/`target/
   debug/deps/` processes: `ps -Ao pid,etime,%cpu,command | grep -E "cargo
   test|target/debug/deps/|rustc"`. A 0%-CPU cargo with no rustc children is
   blocked on the build lock, not computing.
2. When killing a gate, kill the WHOLE tree including the orphan-prone cargo:
   `pkill -9 -f "rustup/toolchains.*cargo test"; pkill -9 -f "target/debug/
   deps/"`. Verify the process list is empty before relaunching.
3. Do NOT run concurrent focused `cargo` commands while a full gate runs —
   build-lock contention + load (the machine sits at load ~4–5 from background
   dev servers) is what makes a gate look stuck. Launch the gate, then poll only
   with non-cargo commands (`ps`, `grep` on the log).
4. To confirm a suspected test hang is real, run that one binary in isolation
   single-threaded (`cargo test -p <crate> --test <bin> -- --test-threads=1
   --nocapture`) on a clean process table. If it passes fast, the "hang" was
   lock/load, not the test.

## 2026-06-21 — Check the precondition before starting a slice; size a default-flip's blast radius

**Pattern (precondition):** "N2.1 진행해" — but N2.1's own plan named N0-pre.10 a
*binding* precondition ("N2 전 완료 필수") on the very same submit-reject path, and
it was unimplemented. Starting N2.1 first would have meant operating on that path
twice with a conflict in between. Always grep the slice's "선행 게이트 / Conflict
risk / precondition" lines AND verify the precondition is actually in the code
(not just listed as done) before writing the first test.

**Pattern (default-flip blast radius):** N2.1 added one secure-default config
field (`allow_anonymous_submit: bool`, default false). That one field touched
**58 `LocalNodeConfig` literals** across the workspace (node tests, cli tests,
main.rs) because Rust struct literals must name every field. The flip also
silently breaks any harness that exercises the now-guarded path.

**Rule for a new required config field / default flip:**
1. Count the construction sites first: `grep -rn "<StructName> {" crates/ | wc -l`.
   If it's large, the value you choose for existing literals matters — set them
   to the *behavior-preserving* value (here `true` = old anonymous-allowed
   behavior) and flip the default ONLY in the production entrypoint + the RED
   test. Patch them mechanically (`perl -i -pe` anchored on the current last
   field, preserving indentation), then let `cargo build --workspace
   --all-targets` (BOTH feature sets) enumerate any you missed.
2. Trace every smoke/benchmark stage that exercises the guarded path. Distinguish
   HTTP-handler stages (here `local-mining-smoke` → `run-local` + POST /submit →
   AFFECTED, needs the opt-in flag) from in-process stages (here `runtime-smoke`
   = `run_runtime_smoke`, `proof-to-block-benchmark` = `submit-lean`/`agent-proof`
   subcommands → NOT through the handler → unaffected). Run the affected smoke in
   isolation before the ~12h gate.
3. Match existing wire conventions for new reason codes (snake_case here, not the
   spec's hyphenated draft) — a staff engineer matches the surrounding code.

## CI green ≠ local gate green: fresh-environment + live-advisory failures (2026-06-23)

Two CI failures on main that the local full gate could not catch, because the
gate ran in a warmer / older environment than a fresh CI runner.

**Fresh-CI vs warm-local-cache (Lean checker prebuild):** the self-test job
failed in `deep_verify_block_roundtrip` with a DeepVerifyDivergence
(`accepted: true → false`) while the same commit's local gate passed. Root
cause: the test re-runs `lake exec boole_check` on a proof that imports
`Boole.Family.V0Helpers`; the checker's `.lake/build` is gitignored, so a fresh
runner has no prebuilt olean and the import fails ("unknown module prefix
'Boole'"). A developer's already-warm `.lake/build` masked it locally.
- Fix: add a `lean-checker-build` gate stage (`lake build
  Boole.Family.V0Helpers boole_check`) BEFORE cargo-test, so local and fresh CI
  share the precondition. No runtime code performs a lake build.
- Proof technique (avoid the cache illusion): reproduce fresh by moving the
  built oleans aside (`mv lean/checker/.lake/build/lib/lean/Boole /tmp/...`),
  run the real test → it fails with the EXACT CI divergence (same work_id /
  proof_hash); run the preflight → it passes again. A worktree works too, but a
  full cargo recompile is unnecessary when only the lean state must be fresh.
- Rule: when a test shells out to an external tool whose build artifacts are
  gitignored (lean/lake, generated parsers, codegen), the gate must explicitly
  build those artifacts. A green local gate over a warm cache proves nothing
  about a fresh runner — clear the cache locally to verify.

**Live RustSec advisory (cargo audit):** the supply-chain job's `cargo audit
--deny warnings` began failing on a commit whose self-test was green, because
`cargo audit` fetches the live advisory DB: RUSTSEC-2026-0185 (quinn-proto
0.11.14 remote memory exhaustion) was published after the prior green run. The
failure is time-triggered, not change-triggered — an untouched main can go red.
- Fix: minimal Cargo.lock-only bump (`cargo update -p quinn-proto --precise
  0.11.15`); no Cargo.toml / reqwest update, no `audit ignore`. The crate was
  not even in the default build tree (`cargo tree -i quinn-proto` empty), so the
  bump is a pure lockfile change. Verify with `cargo audit --deny warnings` +
  `cargo deny check`.
- Rule: prefer the smallest lockfile change that clears the advisory; reach for
  a wider dependency update or an ignore only if the patched version genuinely
  cannot be resolved.

## Test/dev-loop friction: fmt-before-commit, macOS dyld, guard-after-admission (2026-06-24)

**`cargo fmt` a NEW file before the gate, not after.** A freshly written test
(N2.2) committed clean by focused test + `git diff --check`, but the full gate
failed at its very first stage (`cargo-fmt`) on rustfmt's multi-line `assert_eq!`
wrapping — a wasted ~full gate. Rule: run `cargo fmt --all` (or `--check`) the
moment a new `.rs` file is written, before the focused run, so the gate never
burns on formatting.

**macOS dyld: a freshly built test binary's first `execve` blocks 30-60s.**
cargo's atomic-rename-on-build invalidates the kernel signature cache (the same
reason `self-test.sh` has a `cargo-test-prewarm` stage), so the first launch
hangs inside `_dyld_start`. Wrapping a fresh binary in `timeout 15`/`timeout 60`
(or a `sleep 6` poll) kills it mid-verification — it looks like an infinite hang
with zero output (not even the test harness banner / `--list`). This burned a
long debugging detour. Rule: run new tests via `cargo test` with NO short
`timeout` and let cargo pay the dyld cost; only diagnose a "hang" as a real bug
after the binary has launched at least once (warm signature cache).

**Testing a guard that runs AFTER admission: the negative input must still pass
admission.** N2.3's dedup check fires only on an `Accepted` submission. The
multiminer fixture's proofs are valid POW only at the head they were generated
against, so the second (duplicate) submit must carry `c = the live runtime head`
after the first block — otherwise admission rejects it for `stale_c`/bad-POW
*before* the guard, and the test passes for the wrong reason (a false GREEN that
never exercised the feature). Confirm the test actually gates the feature with a
behavioral RED: temporarily disable the guard (`if false && …`) and watch the
test fail with the duplicate credited, then restore. For a consensus-path change
this disable-then-restore is worth the extra run.

**`max_requests` must equal the EXACT request count, including GETs.** The
`boot(max)` harness stops the server after `max` requests; a balance/`/account`
GET counts too. An under-count refuses a later request; an over-count hangs the
final `join()`. Count every POST and GET in the test body (N2.2 and N2.3 both
hung here first).

## 2026-07-03 — A standing user directive belongs in the constitution, not a decision-log entry

**Pattern:** the 2026-07-02 north-star directive ("optimize every proposal for a
complete L1; existing docs/conventions are reference input, not binding — state
any deviation and get confirmation") was recorded only inside one ADR entry of
EXECUTION-ORDER's decision log. A day later a recommendation defaulted back to
"consistency with the existing codebase idiom" as its primary argument, and the
user had to ask where the directive actually lived. A log entry records that a
decision happened once; only a rules-document entry changes future behavior.

**Rule:** when the user issues a standing principle (not a one-off choice),
promote it into the binding rules document (L1 master §개발 헌법 / CLAUDE.md) in
the same session it is given, and leave the decision-log entry as the origin
record. When recommending between options, argue from the north star first;
convention-consistency is a tiebreaker, never the headline reason.

## 2026-07-03 — A policy revision must reach every binding copy, or the old bottleneck survives on paper

**Pattern:** the 2026-07-02 gate revision (tiered commit gates; full verification
moved to CI) was applied to CLAUDE.md and EXECUTION-ORDER's gate-policy section,
but the L1 master's 개발 헌법 items 5/6 kept the pre-revision text ("full gate
before every commit" — the exact ~12h-per-commit bottleneck the revision
removed). The user had to ask why the time-saving policy was missing from the
constitution. Same failure shape as the north-star promotion miss caught earlier
the same day: a rule updated in one binding place and left stale in another.

**Rule:** the binding rule set lives in three places — CLAUDE.md, L1 master
§개발 헌법, EXECUTION-ORDER §게이트 정책. Any policy change lands in all three in
the same slice. Per-slice boilerplate that quotes a superseded policy gets one
superseding note in the constitution (constitution wins), not case-by-case
edits across hundreds of slice specs.

## 2026-07-03 — Reports must be written for comprehension, not for the record

**Pattern:** status reports to the user stacked project-internal jargon
(invariant numbers, slice IDs, symbol names) without plain-language glosses,
and the user had to ask for explanations in easy words. Precision belongs in
the spec/ADR documents; the report's job is that the reader understands the
decision and its consequences on first read.

**Rule:** constitution §14 / CLAUDE.md 보고 문체: lead with the conclusion in
one sentence; give every technical term an everyday-language gloss right next
to it; never cite a symbol, slice number, or invariant number alone as if it
explains itself. Write the precise version into the docs, then report the
understandable version.

## 2026-07-05 — Subagent commit gates must mirror CI's cheap first gates exactly

**Pattern:** eight parallel worktree agents ran focused tests and even the
heavy consensus smokes, but not `cargo fmt --all --check` or the two clippy
`-D warnings` invocations — the first, cheapest stages of CI's self-test.
Three PRs bounced off CI inside the first minute (fmt twice, clippy once),
each costing a diagnose-fix-push-rerun cycle that was strictly slower than
running the checks locally would have been. Separately, agents sharing one
CARGO_TARGET_DIR produced false "unresolved definition" compile errors
(cache poisoning between branches) that wasted a debugging round.

**Rule:** any agent prompt that ends in "commit and push" must include the
CI gate prefix verbatim: `cargo fmt --all --check` plus both clippy variants
from scripts/self-test.sh, before commit. Parallel worktree agents each get
a private CARGO_TARGET_DIR. After every push, confirm a CI run actually
started for the new SHA (GitHub drops triggers occasionally); the documented
`gh workflow run` dispatch is the fallback.

## 2026-07-05 — Auto-merge on a stacked PR fires instantly when the base is unprotected

**Pattern:** stacked slices (pre.2 on pre.1's branch, pre.6 on pre.2's)
enabled `gh pr merge --auto --squash`, expecting to queue behind CI. Only
`main` has required checks, so GitHub merged those PRs into the stack base
immediately, un-gated. Net effect: three consensus slices rode one landing
PR to main, and the default squash method would have collapsed them into a
single commit, destroying the per-slice commit messages the plan mandates.

**Rule:** in a stack, only the PR targeting main is a real gate; treat
intermediate PRs as instant branch merges. Switch the landing PR to rebase
merge (or rebuild the branch into one clean commit per slice before landing)
so each slice keeps its own commit message. Check `mergeStateStatus` for
CONFLICTING on the landing PR after siblings merge to main — first-landed
wins, the stack pays the rebase.

## 2026-07-05 — Gate scripts outside CI rot silently; baseline-verify before blaming the slice

**Pattern:** N3-pre.5's commit gate (`smoke-testnet-faucet-to-block.sh`)
failed with a 401 — not because of the slice, but because commit ecaa7c0
(N2.1, ownership-by-default) had broken that script months of commits
earlier. The script is not in CI's self-test, so main stayed green while a
documented gate was unusable. The agent's triage was the right protocol:
stash the slice, re-run the gate on clean origin/main, and only then declare
the failure pre-existing and stop without committing.

**Rule:** when a local gate fails, re-run it against unmodified origin/main
before attributing it to the change; a pre-existing failure becomes its own
fix-first slice (here: b4ef112) that the blocked slice then stacks on. Treat
any gate script referenced by plans but absent from CI as suspect-stale, and
prefer promoting such scripts into CI when they guard real invariants.
