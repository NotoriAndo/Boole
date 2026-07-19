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

## 2026-07-05 — Platform code you can't run locally lands in log-mode + iterates through CI

**Pattern:** the ADR-0008 isolation slice was developed on macOS, but its
Linux seccomp/Landlock path can't execute on a macOS dev box (no Docker
either). The agent cross-compiled it clean and it passed macOS tests, but the
three Linux enforce guards failed on the ubuntu CI runner: Landlock's Execute
right also governs the ELF interpreter open, so a dynamically-linked child's
execve was denied (EACCES) until the loader + system lib dirs were added to
the exec allowlist — a production-relevant bug (lake/lean are dynamically
linked), invisible on macOS.

**Rule:** for OS-specific code the dev machine can't execute, treat CI as the
only validation and plan for round-trips: land enforcement in log/permissive
mode first (so a mis-tuned filter logs instead of breaking the checker), prove
the enforcing path via guards that CI runs, and brief the fixer that iteration
is CI-paced (cap the rounds). When restricting execve via Landlock/seccomp,
remember the dynamic loader + shared-lib dirs need read+exec, or every
dynamically-linked child fails to start. Check the crate's own reference
example for the canonical allowlist.

## 2026-07-06 — Extracting a helper from a contract-pinned function body breaks scripts/ mirrors

**Pattern:** N3.3 extracted submit_json's bounty-event appends into a shared
`append_block_bounty_events` helper (for reuse by the p2p block ingest). All
Rust gates passed locally, but CI failed in `python-script-tests`:
`scripts/test_multi_store_commit_ordering_contract.py` statically pins the
SOURCE-LINE ORDER of `submit_json`'s body (nonce burn → block commit →
`FileBountyEventLedger::append(` → receipt), and the literal moved out of the
scanned span. Semantically the write order was unchanged — only the static
mirror broke. This is the 2026-06-10 "pinned formula lives in N mirrors"
lesson recurring in a new form: the mirror pins a function's BODY STRUCTURE,
not a formula.

**Rule:** before refactoring (extracting/inlining/renaming inside) any
consensus-adjacent function in boole-node/boole-core — especially
`submit_json`, commit/boot paths, ledger appends — run
`grep -rln '<fn name or moved literal>' scripts/*.py` and update the contract
tests in the same commit, following the extraction (pin the helper CALL's
position in the outer body + the moved literals inside the helper's own
span). Then run the FULL python-script-tests stage locally
(`python3 -m unittest scripts/test_*.py` per self-test.sh line 50) before
pushing — it is seconds-cheap and one CI round-trip expensive.

## 2026-07-07 — A new consensus rule invalidates every test/fixture that "cheaply" reuses one artifact

**Pattern:** N4-pre.1 (chain-wide proof dedup) made "the same proof credited
twice" invalid. The exploration sweep caught the FIXTURES doing this (all 5
runtime-smoke scenarios reused one proof across steps) but missed a TEST
doing the same thing internally: `bounty_event_crash_heal` cloned one body
across two block commits, and the new builder pre-filter correctly refused
the second block — CI bounce. The failing pattern (`body1 = body0.clone()`
then a second commit) was greppable in advance; my pre-PR sweep grepped
fixtures and scripts but not `crates/*/tests/` for body-reuse.

**Rule:** when a slice tightens a consensus invariant, enumerate EVERY
artifact class that could embody the now-forbidden shape before pushing:
fixtures (grep the literal bytes), scripts (static contract mirrors),
AND tests (`grep -rn "\.clone()" crates/*/tests/` near multi-commit
setups; any test committing 2+ blocks from one template body). Fix them in
the same commit. Also: a "full local sweep" of boole-node must use the
gate's feature flags (`--features boole-node/dev-mock-payment,...`) — a
plain `cargo test -p boole-node` fails the two verify-answer tests by
design and wastes a diagnosis round.

**Recurrence (same PR #35, round 2):** after the test fixes I amended the
commit and force-pushed WITHOUT re-running `cargo fmt --all --check` — my
one-line `.collect()` payload chains exceeded rustfmt width, so CI's stage-1
cargo-fmt bounced in 14s. This is the 2026-07-05 "mirror CI's cheap first
gates" rule firing a second time: it applies to EVERY push, including a
one-file amend that "obviously" can't have broken formatting. rustfmt
rewraps method chains you hand-write on one line. Rule reinforced: no push —
initial or amended — without `cargo fmt --all --check` + both clippy
variants green locally first. Manual single-file edits are the trap because
they feel too small to gate.

## 2026-07-09 — N5-pre.1 (PR #52): fail-fast가 진짜 회귀를 로컬에서 숨김 + fmt-after-amend 3회차

**What happened:** (1) `cargo test -p boole-node`가 Lean 부하 플레이크
(agent_proof_cli, 10s wall-clock timeout under load)로 한 타겟이 실패하자
**나머지 테스트 바이너리 실행을 중단**(cargo의 타겟 간 fail-fast 기본값) —
그 뒤에 있던 `hard_guard_regression`의 진짜 회귀(preimage v2가 promoted
credit을 커밋하므로 c-동일성 단언이 무효)가 로컬에서 아예 실행되지 않았고
CI에서야 잡혔다. (2) 마지막 unused-import 정리 후 fmt 재실행 없이 push —
2026-07-05/07-07에 이어 3번째 같은 바운스.

**Rule:** ① 스위트 스윕은 `cargo test --no-fail-fast`로 — 특히 플레이크
가능 타겟(Lean/부하 의존)이 앞에 있을 때, 한 타겟의 flake가 뒤 타겟의
실제 회귀를 가릴 수 있다. "N개 타겟 중 몇 개가 실행됐는가"를 결과에서
확인할 것(test result 줄 수 ≈ 타겟 수). ② 합의 불변량을 바꾸는 slice는
"그 불변량이 성립함을 단언하는 기존 테스트"를 사전 grep으로 찾는다 —
이번엔 `block_hash_is_unchanged_by_promoted_bounty_shares_field`는 지도에
있었지만 hard_guard의 c-동일성 단언은 이름에 hash가 없어 놓쳤다. 검색은
심볼명이 아니라 **불변량 표면**(여기선 `"c"` 필드 비교) 기준으로.
③ fmt-after-amend: 예외 없음 — 어떤 크기의 수정이든 push 전
`cargo fmt --check` (이 규칙의 3회째 강화).

## 2026-07-10 — 로컬 Lean 전체 스위트 재발 (2회째): 정책화

**What happened:** N5.2 마무리에서 `cargo test -p boole-node`(전체)를 또
로컬로 돌렸고, 어제(N5-pre.1)와 동일하게 Lean 의존 타겟에서 수 시간
정지했다(벽시계 timeout↔재시도 아님, 프로세스가 그냥 안 끝남). 어제
교훈("--no-fail-fast로 스윕")은 지켰지만 더 근본 문제는 **이 머신에서
Lean 스폰 타겟이 포함된 전체 스위트 자체가 신뢰 불가**라는 것.

**Rule:** 로컬에서는 node 전체 스위트를 돌리지 않는다. 로컬 게이트 =
blast-radius focused 타겟 열거(+ 관련 smoke) + fmt/clippy까지. Lean 스폰
타겟(agent_proof_cli, deep_verify 계열, submit-lean e2e)은 CI 클린 러너
전용으로 취급. 전체 스위트가 필요하면 CI가 한다(2026-07-02 게이트 개정의
원래 취지). 로컬 실행이 20분을 넘기면 기다리지 말고 죽이고 CI로 넘긴다.

**Recurrence 참고 (N5.2, PR #55 1차 CI 반송):** blast-radius 열거에서
boole-cli 테스트를 빠뜨렸다 — cli 테스트들도 노드를 부팅한다(`network_id:
Some("boole-testnet")` 라벨). 노드 부팅 경로를 바꾸는 slice의 focused
목록에는 **노드를 부팅하는 타 크레이트 테스트**(boole-cli의 bounty/faucet
계열)를 반드시 포함할 것. 또한 기존에 라벨로 쓰이던 이름에 새 의미(컴파일
preset)를 부여할 때는 사용처 grep을 구현 전에 — 이름 충돌은 컴파일 에러가
아니라 런타임 거부로 나타난다.

## L8 — 커밋 게이트에 python 계약 테스트 포함 (2026-07-12, CI 반송)
리셋 창 PR에서 rust focused+smoke+clippy만 로컬 확인하고 push → CI self-test의
python-script-tests 단계(스크립트 계약 테스트, scripts/test_*.py)에서 2건 FAIL
반송. 원인: 소스-계약 테스트(코드 본문 grep)와 fixture-계약 테스트는 rust 게이트
밖에 있는데, production 코드 구조 변경(함수 이동)과 fixture 값 변경이 이들을
직접 깨뜨림. 규칙: **production 코드의 함수 시그니처/구조 변경 또는 fixture 값
변경이 있는 slice는 커밋 전 `python3 -m unittest scripts.test_*` (또는 최소한
변경 파일을 grep하는 계약 테스트 모듈)를 focused 게이트에 포함**한다.

## L8 — 커밋 전 로컬 게이트에 CI 조건을 그대로 포함 (2026-07-13, SC.5 CI 반송 2회)

증상: SC.5 PR이 CI에서 2회 반송 — ① 테스트 전용 상수 재수출이 lib 빌드에서
dead-code(-D warnings) ② 수정한 crate(boole-node)만 전체 테스트하고 인접 crate
(boole-cli)의 같은 패턴(legacy 시딩 부팅) 잔존을 못 봄.

규칙:
1. 합의/production slice 커밋 전 `cargo clippy --workspace --all-targets
   --features boole-node/dev-mock-payment,boole-miner/dev-tools`(CI와 동일
   feature)를 focused 게이트에 포함한다. -D warnings는 CI가 적용하므로 경고 0을
   확인한다.
2. ~~노드 boot/verify 같은 공유 표면을 바꾸면 "그 표면을 소비하는 crate 전체"
   (최소 boole-node + boole-cli)를 --no-fail-fast로 돌린다.~~
   **개정 (2026-07-14, 운영자 승인 "추천조합 진행해")**: 인접 crate 전체
   테스트는 로컬이 아니라 **CI가 수행**한다(클린 러너 ~8분 vs 로컬 단일
   스레드 30~60분 — SC.9에서 실측). 로컬은 focused + fmt + CI-동일 clippy
   2종(반송 원인 대부분을 잡는 싼 게이트) + 합의 티어 smoke만. 가끔의 CI
   반송 1회(+8분)가 항상 로컬 full을 내는 것보다 기대값이 싸다. "full green
   없이 main 금지" 불변량은 무변경(CI required checks가 강제) — 2026-07-02
   full-gate CI 이관과 같은 방향. 공유 표면을 바꿨을 때는 그 표면의 직접
   소비자 **테스트 파일 몇 개를 focused로 골라 돌리는 것**으로 대체한다
   (전체 스위트가 아니라).
3. cfg(test)에서만 쓰는 항목은 재수출하지 말고 테스트가 원 경로를 직접 참조한다.

## L9 — 게이트 소요시간 단축 로드맵 (2026-07-14 운영자 승인)

승인된 추천 조합:
1. L8 규칙 2 개정 (위) — 즉시 적용.
2. **cargo-nextest 도입**을 독립 유지보수 슬라이스로 등록 — 테스트별 프로세스
   격리로 RUST_TEST_THREADS=1 포괄 제약을 안전하게 병렬화. 도입 시 검증 필요:
   포트/임시폴더 충돌 전수 확인(대부분 이미 임의 포트 + 고유 임시폴더),
   P0.3 결정성 계약(test_self_test_contract.py의 RUST_TEST_THREADS pin)과의
   정합 — 계약 자체를 nextest 격리 모델로 개정해야 함.
3. 남은 독립 슬라이스(SC.4/SC.8 등)는 worktree 멀티에이전트 병렬로
   (N3-pre wave 전례).

## L10 — `generate_from_hex`는 hex 디코드 실패에만 Err (SC.10-ii-a RED에서 실증)

`family_v1_lenbound::generate_from_hex`는 `hex::decode` 결과를 그대로
`generate_v1_lenbound(&bytes)`에 넘긴다. `generate_v1_lenbound`는 임의 길이
바이트를 수용하므로, "짧은/긴 seed"는 유효한 인스턴스로 재파생된다. Err는
오직 hex 디코드 실패(홀수 길이·비-hex 문자)일 때만 난다. 따라서 verifier
entry의 `SourceRederiveFailed`를 RED로 유발하려면 4바이트 같은 짧은 seed가
아니라 **디코드 불가능한 hex 문자열**("not-a-hex-seed" 등)을 써야 한다.
ii-b/c/d에서 손상 seed 케이스를 쓸 때 동일하게 적용.

## L11 — 공유 함수의 "호출 교체"는 새 테스트만이 아니라 기존 소비자 테스트도 돌려라 (SC.10-ii-d-1 CI 반송에서 실증)

SC.10-ii-d-1에서 replay 경로의 `validate_proof_package_shape` 호출을
`validate_proof_package_with_limits`로 바꿨다. 새 테스트 파일
(`replay_resource_limits.rs`)만 로컬 focused로 돌리고 커밋·push했는데, CI
`self-test`가 반송됐다. 원인: 같은 코드 경로를 쓰는 **기존** 테스트
`replay_fixtures::replay_rejects_selected_share_evidence_invalid_proof_package_shape`가
shape 오류 메시지 "proofPackage invalid"를 기대하는데, 새 호출이 Decode(shape)
오류까지 "exceeds resource limit" 메시지로 뭉갰다. 즉 하나의 검증 호출을
교체하면서 그 함수가 원래 내던 **오류 범주별 메시지 계약**을 깼다.

규칙:
1. 공유/합의 함수의 호출을 **교체**할 때(새 함수 추가가 아니라), 그 함수가
   내던 오류·반환 계약이 바뀌는지 먼저 확인하고, 바뀐다면 **기존 소비자 테스트
   파일**을 focused 게이트에 반드시 포함한다. 이는 L8 규칙 2(2026-07-14 개정)의
   "공유 표면 바꾸면 직접 소비자 테스트 파일 몇 개 focused로" 지침의 구체 사례다
   — "새 기능=새 테스트"로 프레이밍하면 이 지침을 놓치기 쉽다.
2. runtime-smoke-all/proof-to-block-benchmark는 **happy-path**라 거절 메시지
   회귀를 못 잡는다. 거절·오류 경로를 바꿨으면 그 경로를 직접 찍는 focused
   테스트(기존+신규)로 커버해야 하며, 합의 smoke green을 "거절 경로도 검증됨"으로
   착각하지 않는다.
3. 검증 호출을 자원한도용으로 바꿔도 **오류 범주는 보존**한다: shape/decode
   실패와 자원한도 초과(TooLarge/TooManyDecls)는 서로 다른 거절이므로 메시지도
   분리 유지한다(수용 경계는 동일하게 강화하되).

---

# 2026-07-16 — 로컬 다중노드 스모크의 rate-limit 함정 (loopback = 단일 IP + 티켓 dedup)

SC.10-iv-c(3-노드 Lean-invalid 주입 스모크) 개발 중, 정직 대조군 share
제출이 두 번 연속 rate-limit으로 거절됐다. 원인 2단계:

1. **IpQuota**: p2p ingress 재admit은 admission rate limiter를 peer IP로
   재사용한다(N3.2/ADR-0009 (c)). loopback에선 모든 노드가 127.0.0.1이라,
   gossip으로 들어온 주입 share가 정직 노드의 per-IP 쿼터(scenario
   `perIpRateLimitPer60s: 1`)를 소진 → 이후 정직 HTTP 제출이 IpQuota 거절.
   해결: `perIpRateLimitPer60s`는 GenesisParams가 아니라 Tier-3 노드-로컬
   knob이므로(genesis hash 불변), 이 값만 올린 전용 시나리오 fixture
   (`testnet2-pinned-highrate.v1.json`)로 교체. 이름 붙은 망의 genesis 게이트는
   그대로 통과(t_block/t_share/k_max/retarget 동일).

2. **PkQuota**: per-PK 쿼터는 티켓 `(pk, c, n)`에 걸린다 —
   `ceiling = tickets * M`, 티켓은 `(pk,c,n)` distinct일 때만 증가하고
   같은 `(pk,c,n)`은 dedup으로 재관측 안 됨(rate_limiter.rs). 주입 fixture와
   정직 fixture가 **같은 nonce `n`**을 쓰면, 먼저 들어온 주입 share가 유일한
   티켓 쿼터를 소진하고 정직 share는 새 티켓을 못 얻어 PkQuota 거절.
   해결: 주입 fixture에 **다른 nonce `n`** 부여(seed도 함께 재유도) → 각
   share가 자기 티켓을 들고 옴.

규칙:
- 로컬 loopback 다중노드 스모크를 설계할 때, gossip 재admit이 **admission
  rate limiter(IP·PK 둘 다)**를 소비한다는 걸 전제한다. 같은 IP(127.0.0.1)와
  같은 `(pk,c,n)` 티켓을 공유하는 노드/share는 서로의 쿼터를 잠식한다.
- 대조군과 주입 share는 **서로 다른 nonce**로 티켓을 분리한다. 시나리오의
  rate-limit knob이 부족하면 그 knob만 올린 전용 fixture를 만들되, 그 값이
  genesis hash(GenesisParams)에 안 들어가는 Tier-3 knob인지 먼저 확인한다.
- 증상이 "1차 IpQuota → 고치니 2차 PkQuota"처럼 순차로 드러날 수 있으니,
  rate-limit 거절은 IP·PK·티켓 세 축을 모두 점검한다.

---

# 2026-07-16 — macOS syspolicyd 스톨: cargo test EXEC는 막혀도 컴파일은 된다

SC.10-iii-b 개발 중 이 머신의 macOS Gatekeeper(syspolicyd)가 심하게 thrash(20~87% CPU)
하며 `cargo test`가 20분+ 멈춤. 원인 분리: **컴파일은 되는데 갓 빌드된 테스트
바이너리의 EXEC 시점에 syspolicyd 서명 검증이 무한정 지연**된다(clippy도 갓 빌드된
proc-macro dylib 로드에서 같은 스톨). `cargo test -p X --lib --no-run`(컴파일만,
실행 안 함)은 **정상 완료**로 확인 — 즉 타입/borrow 검증은 로컬에서 가능하고,
테스트 실행만 막힌다.

규칙 (이 머신이 이 상태일 때):
- 로컬 게이트를 **컴파일 검증**으로 대체: `cargo test --lib --no-run` 또는
  `cargo check`로 타입/borrow 오류를 잡는다(빠름, exec 없음). `cargo fmt --check`,
  `bash -n`, python 계약 테스트, self-test 집계 python mock 실행은 전부 로컬에서
  정상(파이썬/rustfmt는 안정 서명 바이너리라 스톨 없음).
- **테스트 실행(RED/GREEN)·smoke·clippy 2종은 CI(ubuntu, Gatekeeper 없음)로
  강제**. CI self-test가 lib 테스트 + smoke를 돌리므로 CI green이 실행 검증을 담당.
  이는 L8 "CI 반송 1회가 항상-로컬-full보다 싸다"의 연장 — 여기선 로컬 exec가
  아예 불가라 CI가 유일 실행 경로.
- 스톨 진단: `ps aux | grep syspolicyd`(CPU%) + `find target/debug -newermt '-60 seconds'`
  (아티팩트 0이면 컴파일 스톨, 아티팩트 있는데 test result 없으면 exec 스톨).
  killed 후 `--no-run` 재시도로 컴파일 완료 여부를 먼저 가른다.
- 커밋 전제: 로컬에서 **컴파일 clean + fmt + 계약 + 집계 mock**을 확인하고,
  실행 검증은 CI에 위임한다고 보고에 명시(맹목 push 아님 — 검증 가능한 건 다 검증).

---

# 2026-07-16 — 텔레그램-발 대화는 반드시 reply 툴로 답한다 (터미널 출력 금지)

사용자가 텔레그램으로 질문("스모크 돌리려면 재부팅?")했는데, 내가 답을
**터미널 텍스트로만** 출력하고 `reply` 툴로 안 보냄 → 사용자는 못 봄 →
"텔레그램으로 대답해" 지적받음.

규칙 (전역 CLAUDE.md 재확인):
- 텔레그램-발 세션에서는 **모든** 답(질문 응답·중간보고·완료보고·질문)을
  `mcp__plugin_telegram_telegram__reply`로 originating chat_id에 보낸다.
  터미널 최종 텍스트는 사용자에게 안 보인다.
- 특히 **clarifying 질문에 대한 답**을 터미널에만 쓰기 쉬움 — 결정 대기 중
  갈림길 응답도 반드시 텔레그램으로.
- 매 턴 마지막에 체크: "이 답이 텔레그램으로 나갔나?" 안 나갔으면 reply 호출.

## 2026-07-18 — Local gate cap exists BECAUSE of this machine: never run whole-crate tests locally

**Pattern:** during SC.1-a I ran `cargo test -p boole-core` (the full crate suite, ~50 test binaries) as a "thorough" pre-commit check. It stalled for 3.5 hours with 0.28s of CPU time — the documented macOS syspolicyd first-exec stall on fresh test binaries (recorded in tasks/todo.md SC.10-iii-c-2 notes: "로컬 test-binary exec는 여전히 syspolicyd 스톨"). The user had to ping twice before I noticed. The 2026-07-14 gate revision ("로컬 게이트는 focused test까지만, 인접 crate 전체 테스트 금지") exists precisely because of this machine behavior — I treated it as bureaucracy instead of a hard operational constraint.

**Rule:**
- Local verification = the focused `--test` binaries named in the slice gate (plus directly affected ones), passed as multiple `--test` flags in ONE cargo invocation. Never bare `cargo test -p <crate>` on this machine, even for the crate being changed. Full coverage is CI's job (required checks).
- Any local cargo run that will exec NEW test binaries: launch with run_in_background and check within ~2 minutes; if `ps` shows near-zero CPU time on the cargo process, it is the syspolicyd stall — kill it immediately and narrow the target set. Do not wait for a timeout.
- When a background verification is the only thing between "work done" and "report", check its liveness before going idle — a stalled gate discovered by a user ping is a reporting failure, not just a tooling hiccup.

## 2026-07-19 — The max_requests lesson fired AGAIN (SC.1-b): re-read lessons before writing tests, not after CI burns 30 minutes

**Pattern:** `session_submit_block_evidence_carries_signed_work` booted with `boot_with(&paths, 3)` but made only 2 HTTP connections (register + submit — the third "read" was a DISK read of blocks.ndjson, not HTTP). Exactly the documented 2026-05-10 lesson: the server waits for connection #3 forever, `boot.handle.join()` hangs, and with RUST_TEST_THREADS=1 the whole CI self-test job hung 29 minutes until job cancel. The RED phase masked it: the assertion panicked BEFORE join, so the hang only appeared post-implementation (GREEN path reaches join).

**Rule:**
- When adding ANY test that uses `max_requests`-style connection-counted boots, count the literal HTTP call sites in the test body at WRITE time — and re-count when an assertion is served from disk/file instead of HTTP (those don't consume a connection).
- A RED test that panics before teardown does NOT validate the teardown path; the GREEN run exercises join/shutdown for the first time. For connection-counted servers, verify the count independently of the RED run.
- Session-start lessons review must include a keyword scan of the test helpers being touched (`grep max_requests tasks/lessons.md`) — the lesson existed and was not consulted.

---

# 2026-07-19 — §ZK 플랜 리뷰에서 받은 정정 5건 (플랜 작성 규칙)

운영자가 §ZK base-family 교체 플랜 초판에서 결함 5건을 잡았다. 각각을 재발
방지 규칙으로 승격한다.

1. **새 표면의 활성화와 그 강제는 같은 원자 창에 있어야 한다.** "스키마
   선행 → enforcement 후속" 분리는 기존 표면의 확장(새 공격 표면이 열리지
   않음)에만 유효하다(§SC 리셋 창 선례). 새 family/새 검증 대상을 활성화하는
   창이라면, 활성화 커밋 시점에 모든 거절 경로가 이미 green이어야 한다 —
   "활성화 = 최후 커밋" 원칙. 플랜에 "X는 다음 slice에서 강제"라고 쓰는 순간
   그 사이 구간의 체인 상태를 한 문장으로 서술해 보라: "위조가 통과한다"면
   그 분리는 틀린 것이다.
2. **채굴/출제 경제 실험에는 반드시 선택 편향(골라잡기) 축을 넣는다.**
   dedup·티켓 규칙은 "같은 문제에 여러 답"만 막는다 — "여러 문제 중 쉬운
   것 선택"(best-of-N seed grinding)은 별개 공격축이고, 난이도 목표는
   평균이 아니라 **순서통계(min-of-N)** 위에서 유지되어야 한다. 실험
   설계에 N=1/10/100/1000 스윕과 추첨 비용(티켓 PoW) 경제 균형을 포함.
3. **합의 고정물(pin/hash)을 바꾸는 slice는 파생 배포물을 전수 조사한다.**
   artifact hash 하나를 바꾸면 하드코딩 파일 목록 스크립트
   (`make-checker-release-sums.sh` FILES), SHA256SUMS, README 지문,
   release 계약 테스트까지 연쇄된다. "hash 재계산"이라고 쓰기 전에
   `grep -rn <구 지문/파일명> scripts/ lean/ tests/`로 소비자 전수 목록을
   Target files에 박는다. 경로 관행도 실물 확인(helper는 `Boole/Family/`,
   `BooleCheck/`가 아님 — ls로 확인 없이 쓴 경로가 틀렸다).
   **2차 검수 재발 (같은 날)**: "지문은 디렉토리 전체 해시"라는 가정도
   틀렸다 — 실제는 `CHECKER_PINNED_FILES` 목록 + `BooleCheck/**`만 해시
   (`checker_artifact_hash`, lib.rs:1543). 고정물의 **계산 코드 자체**를
   열어 대상 집합을 확인하기 전에는 커버리지 서술("전체를 해시" 등)을
   플랜에 쓰지 않는다. 목록 밖 파일 추가는 "지문에 안 잡히는 합의 코드"
   구멍이 된다.
4. **보안 클레임은 능력 서술로 쓴다.** "위조 불가" 같은 절대 표현 금지 —
   정확한 형태는 "잘못된 X를 모든 노드가 싸고 결정적으로 거절할 수 있다".
   또한 부정형 속성(underconstraint 등)을 채굴 대상으로 삼으려면 그 속성의
   정의역(공개 입력/출력, 무엇이 유일해야 하는가)을 ADR에서 먼저 고정해야
   "반례"가 정의된다.
5. **"동일 해상도" 요구는 문서 끝까지 적용된다.** 플랜 후반부 slice에서
   9-필드(Target/RED/Expected/Minimal/Focused/게이트/Non-goals/Conflict/
   Commit)를 생략하는 압축은 해상도 위반 — 앞 slice와 같은 템플릿을 끝까지
   채우거나, 못 채우면 "미정"을 명시하고 사유를 적는다.

---

# 2026-07-19 — 채굴 문제 난이도 실측: "SMT-timeout ≠ hardness" (ZK.0 스파이크)

ZK.0 스파이크에서 hash-generated ZK underconstraint family를 실측했더니
NO-GO. 여기서 얻은, 앞으로 **모든 채굴 문제 family 후보**에 적용할 규칙:

1. **최강 공격자를 모델링하라 — 자작 순진 탐색기는 문제를 실제보다
   어렵게 보이게 해 family를 잘못 통과시킨다.** ZK.0에서 Z3 SMT조차
   최강이 아니었다: feed-forward 회로의 진짜 공격은 솔버-불요 O(n)
   propagation(자유 wire 교란 + forward 재평가). "우리 문제 어렵다"를
   증명하려면 문제 구조를 **가장 잘 아는** 공격을 구현해 재봐야 한다.
2. **SMT 솔버의 `unknown`(timeout)을 hardness로 읽지 마라.** 큰 비선형
   인스턴스에서 Z3는 그냥 포기한다 — 이를 "어렵다"로 오독하면 위장 PoW를
   green-lit한다. 구조를 아는 공격자는 즉시 뚫는다(ZK.0: checkpoint
   반복제곱을 O(1) 부호뒤집기로 역산, ~1µs, Z3는 6s timeout).
3. **생성기가 직접 심은 구조(삭제한 제약 위치·역산 경로)는 생성기-전지식
   공격자에게 그대로 회수된다 — 단, ZK.0에서 실측 확인된 범위는 ① 제약
   삭제형 feed-forward 회로 ② checkpoint-squaring 재설계 두 설계뿐이다.**
   "모든 공개·결정적 underconstraint family가 원리적으로 불가능하다"는
   일반화는 **미검증 가설**이며 실측된 바 없다 (2026-07-19 운영자 정정으로
   범위 한정). 생성기가 정답을 심지 않고 문제 성질(BUG/SAFE)이 창발하는
   설계는 별도 실측 대상이다. "생성기를 다 알아도 여전히 어려운가"는 설계로
   장담 못 하고 후보마다 **실측**해야 한다(ZK.0 하네스 = 그 실측틀).
4. **채굴 family 트릴레마로 후보를 사전 체질하라**: (a)전지식-hard
   (b)Lean `decide` 저렴·결정적 검증 (c)LLM 채굴가능 — 셋 동시 필요.
   underconstraint=(b)(c)o(a)x / correctness-proof=(a)o(c)x /
   preimage-hard=(a)o지만 PoW로 붕괴. 새 후보는 세 코너 중 어디 떨어지는지
   먼저 따지고 ZK.0-형 스파이크로 (a)를 실측한다.
5. **오프체인 스파이크 도구 설치는 정당하다.** 스펙이 지정한 실험 도구
   (Z3 등)가 없으면 로컬 pip 설치로 진짜 도구를 쓴다(오프체인·paid 없음).
   자작 대체 탐색기로 때우면 규칙 1 위반.
6. **긴 Z3 스윕은 block-buffered stdout + timeout으로 백그라운드에서 안
   보인다.** `python3 -u`(unbuffered) + per-call timeout(솔버 `timeout` 옵션)
   + 밴드/시드 수를 작게 잡아 먼저 shape를 보고 확대한다. Z3 교차검증은
   솔버가 실제로 끝나는 tractable 크기(≤~40제약)로 제한 — 큰 인스턴스는
   unknown만 쏟아내 시간만 먹는다.

---

# 2026-07-19 — dual-cert 스파이크 (zk-circuit-uniqueness-dual-cert.v0 NO-GO)에서 얻은 추가 규칙

1. **UNSAT-측 증서(LRAT/resolution형)로는 "어렵고 검증 싼" family를 만들 수
   없다 — 후보 체질 단계에서 증명계부터 봐라.** CDCL 풀이 시간 ≈ 생성되는
   resolution 증명 크기이고 LRAT 검증도 같은 크기에 선형이라, 난이도를 올리는
   순간 증서와 검증 비용이 같이 폭발한다(실측: solve 수 초 구간에서 LRAT
   44~72MB·Lean 1.1~1.8s·RSS 330MB; 랜덤 k-SAT resolution 지수 하한이 이론
   뒷받침). ≥100× 비대칭이 필요하면 증명 크기가 탐색 노력과 분리되는 증명계
   (succinct proof)가 전제인데, 그건 Phase 0 스코프가 아니라 장기 로드맵이다.
2. **답이 창발(emergent)인 family는 골라잡기(min-of-N)가 1급 파괴 축이다.**
   생성기가 정답을 모르면 easy-tail 제거도 못 한다 — 실측 min-of-1000 이득
   최대 270×, 골라잡힌 문제 100%가 ms급 BUG. 골라잡기 실측은 반드시 실측
   분포의 bootstrap 재표집으로 하고(단순 1/N-quantile 금지), 통제책이
   family 밖(티켓 과금)뿐이면 그 과금이 실질 작업이 되는지(위장 PoW 회귀)를
   같이 판정하라.
3. **BUG-형(반례 제출) 경로의 검증에는 O(인스턴스 크기) 재생성 바닥이 있다.**
   verify가 seed에서 회로를 다시 만들어 전 제약을 검사하는 구조면
   search/verify 비대칭은 그 바닥 위로만 올라간다 — 비대칭 목표를 잡을 때
   verify의 하한부터 계산하고 시작하라(실측 0.88×).
4. **범위 한정 긍정도 기록하라: planted-freedom 누출은 창발 설계로 고쳐진다.**
   기준 witness를 rejection-sampling으로만 심고 답 존재를 결정하지 않는
   생성기는 구조-전지식 공격 5종이 즉시 못 뚫었다(경계 밴드 구조공격 판정률
   42%). 실패 요인은 지름길이 아니라 증서 경제성(위 1)과 골라잡기(위 2)였다
   — "결정적 공개 생성기 = 자동 붕괴"로 일반화하지 말 것(ZK.0 범위 한정
   정정과 일관).
5. **컴파일된 Lean 함수 실행은 heartbeats/maxRecDepth 예산이 안 잡힌다.**
   `lake` 빌드 실행파일 안의 `LRAT.check`는 elaboration이 아니라 네이티브
   실행이라 합의 budget 아날로그가 없다 — 이런 경로를 합의에 넣으려면 step
   경계를 별도로 신설해야 한다는 것을 산정 단계에서 확인하라. 또한 순수 함수
   호출 전후로 시계를 읽을 때 두 번째 읽기가 결과에 의존하게 만들지 않으면
   컴파일러가 계산을 지연시켜 0ns로 측정된다(실측 재현 — 분기 삽입으로 해결).

---

# 2026-07-19 — family 후보 평가 리뷰 정정 3건 (transform.v0 평가에 대한 제3자 리뷰)

1. **일반 메커니즘을 세웠으면 발행 전에 기존 주장 전부를 그 메커니즘으로
   재소사하라.** 체인 자기증명 탐색에서 "succinct 랩핑이 인증서 크기와
   작업량을 분리하는 유일한 문"이라고 직접 주장해 놓고, 바로 다음 평가에서
   동등성 증명 경로에 같은 논리를 적용하지 않아 "반드시 LRAT 벽"이라는
   과장을 만들었다(네이티브 UNSAT 탐색 + trace 검사만 zkVM 증명이면 검증
   측 벽은 해소 가능). 결론은 안 바뀌어도 근거의 과장은 신뢰를 깎는다.
2. **요구사항을 하드 탈락 사유로 인용하기 전에 개정 여부를 확인하고,
   요구사항 개정에도 살아남는 가장 강한 형태의 반론을 앞세워라.** R4(corpus
   불인정)를 인용한 탈락 논거는 운영자가 기준을 바꾸자 무효가 됐다. 살아남는
   형태는 "오프체인 대체 테스트"(같은 산출물을 오프체인에서 더 싸게 만들 수
   있으면 채굴 생산 정당성 없음 — 합성 corpus는 원리상 항상 대체 가능, L1
   추가분 = 인센티브 조달 + 검증된 출처 = 블록 보상으로 corpus 구매와 경제
   동치)였다. 규칙 인용은 부러지기 쉽고 경제 논증은 개정을 견딘다.
3. **"X만으로 충분"을 추천할 때는 X가 운영자의 가치 축 중 무엇을 덮는지
   축별로 명시하라.** "Base = PoVFN 단독 충분"은 인프라 축(가용성·succinct
   검증·상태 계산)에서만 참이고 AI corpus 생산 축은 덮지 않는다 — 유입
   실작업 없으면 체인 검증 기록의 반복일 뿐이며 이를 corpus로 과대주장하면
   정직 라벨 위반. 축 명시 없는 "충분"은 과대주장으로 읽힌다.
