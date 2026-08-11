# Rust execution-proof P1 — frozen-snapshot task eligibility freeze (v1)

Ceiling label: **RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 2,461 files / 2,504 tasks**.

This is an **issuable problem count, not network activation**. Rust `mineable_now`
stays **0**; this record wires **no** consensus / BF.7 path. It is closed-local,
non-consensus evidence and is **not a public-network / leaderboard / paid-API /
production claim**.

Reported outward as **RUST-FROZEN-SNAPSHOT-MINEABLE-ELIGIBLE = 2,461 (files) /
2,504 (tasks)** — the eligible count under **this one frozen corpus commit**, not a
"Rust permanent total". A different snapshot would re-run the census and could differ.

**This does not touch the Rust Reference P0 record.**
`RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0` (the compile-only reference family that
deferred 3,293 roots as `NEEDS-SPEC`) stays **separate and unchanged**. That earlier
`0` was a property of a compile-only family; it never meant "Rust = 0". This successor
family decides eligibility by a **real end-to-end execution proof** and finds 2,461
eligible source files — the two records are kept side by side, neither overwrites the
other.

**Why public test source is not answer leakage.** The artifact a miner must produce is
a **SP1 compressed STARK proof that the guest actually executed the test program to
normal completion** (committing a fixed 20-byte sentinel) — not the fact "it exits 0".
The corpus commit is a bors-merged, CI-green revision, so every `run-pass` test passes
natively by construction; knowing that does **not** shortcut generating a valid
execution trace/proof bound to the guest ELF. A file is excluded as answer-confirmed
**only** when a real proof was already generated for it (the one representative),
never merely because its source is public.

This document is an **append-only attestation ledger**. The guest/host implementation,
the pinned toolchains, the frozen `rust-lang/rust` test corpus, the wrapped guest
crates, the census ledgers, and the representative proof **stay in the git-ignored
sandbox** (`local-docs/rust-execution-proof-p1-2026-08-10/`); only content hashes,
corpus fingerprints, conservation identities, and lineage are tracked here so the
result survives outside the sandbox. Future waves append new dated entries below;
existing **merged** entries are never rewritten.

---

## Entry 1 — 2026-08-10 · execution-proof-census-v1 / N = 2,461 files / 2,504 tasks

### Result in one line

Of the **26,235** frozen `.rs` test roots (**28,735** tasks), **2,895** were the
runnable execution-proof candidates. Each candidate's source was wrapped verbatim into
a per-edition guest crate, built for `riscv64im-succinct-zkvm-elf`, and executed under
an **8,000,000-cycle** ceiling; a build that reaches normal completion commits a fixed
20-byte sentinel. After excluding the one answer-confirmed proof fixture, guest
build/execution failures, oracle-shapes outside the proven success-completion family,
over-ceiling cases, and one content-clone, **RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE
= 2,461 files / 2,504 tasks**.

### Task unit (fixed BEFORE the census)

**1 task = 1 fully-expanded test variant.** A test file with `//@ revisions: a b`
declares two tasks; a plain test declares one. This unit was frozen before the census
ran and was not changed after seeing results.

* **File-unit `2,461` is the primary, independently-proven count:** the census built
  and executed **one** guest per `.rs` source file (the source embeds at the guest
  crate root), producing one sentinel verdict per file.
* **Task-unit `2,504` is a faithful projection of that verdict across declared
  variants**, not 2,504 independent executions. The census did **not** apply
  per-revision `cfg` flags; a multi-revision file's single verdict is attributed to
  each of its revision tasks. Because the corpus commit is CI-green, all revisions of a
  `run-pass` file pass natively, so the projection is sound — but the honest,
  independently-executed number is the file-unit 2,461. The `+43` task delta is the
  revision expansion of eligible multi-revision files.

### Pinned toolchains + SP1 proof binding

Two pinned compilers, one per role:

| role | toolchain | pin |
| --- | --- | --- |
| native judge (oracle) | `rustc 1.99.0-nightly` | commit `e7795af6d2449fb05a6393c3320ced873a999eb3` (exact corpus commit); `cargo 1.99.0-nightly` (`3efb1f477e99b42974b982d939fd100303cdf7db`); host `aarch64-apple-darwin`; official rust-lang per-commit CI artifacts |
| guest prover | succinct (SP1 fork) `rustc 1.94.0-dev` | target `riscv64im-succinct-zkvm-elf`; `sp1-zkvm = 6.3.1`; SP1 proof circuit version `v6.1.0` |

The native oracle for the built family is **normal completion / exit 0**: the corpus
commit is bors-merged and CI-green, so each `run-pass` test passes under its exact
compiler by construction.

**Uniform guest harness.** Every candidate is wrapped identically: prepend
`#![no_main]`, keep the test source **verbatim**, then append

```
extern crate sp1_zkvm;
fn __rustexec_entry() { main(); sp1_zkvm::io::commit_slice(b"RUST-EXECUTION-PROOF"); }
sp1_zkvm::entrypoint!(__rustexec_entry);
```

The guest crate edition is selected to match the test's `//@ edition` (`2015` default;
ranges → their floor). The 20-byte sentinel `RUST-EXECUTION-PROOF` (public-values
length 20) is committed **iff `main()` returns normally**; a panic, trap, or explicit
process exit halts the guest before the commit (public-values length 0), which is
exactly the discriminator between eligible and non-eligible.

**Per-task ELF and vk differ — this is not a fixed-guest design.** Because each task's
program embeds in its own guest crate, **every eligible task has its own guest ELF, its
own verifying key, and its own proof**. There is no single family-wide vk. The proof
still binds structurally to **(guest ELF + vk + SP1 verifier digest)**: a proof
accepted under a task's vk attests execution of exactly that task's ELF, and a tampered
or substituted program yields a different vk that fails verification. The end-to-end
path is demonstrated once by the representative below; the census then measures, per
candidate, whether that same path produces a sentinel-completing ELF within the ceiling.

### Representative proof (the one `ANSWERED-PROOF-FIXTURE`)

A real compressed proof was generated for `tests/ui/moves/move-4.rs` (source `sha256
fd31eab38daf9346e3dd6a524b36ee332d64089c3f1033486875213a60797fdf`), a `run-pass` test
whose internal `assert_eq` encodes its correctness. Because a proof already exists for
it, it is answer-confirmed and excluded from the eligible count — counted in **exactly
one** bucket (`ANSWERED-PROOF-FIXTURE`), never double-subtracted.

| binding component | value |
| --- | --- |
| guest ELF | `sha256 af8c179cf544f544c80eb9a23f19be2006fe2bea931e7f965f1ed77b09f7299c` |
| verifying key (`vk_bytes32`) | `0x0038de3b51dcfe81fa915df141a0b5c88e73b727b52f601f2561fe472b947c96` |
| SP1 proof circuit version | `v6.1.0` |
| SP1 package (`sp1-zkvm`) | `=6.3.1` |
| guest edition | `2015` |
| execution cycles | `4,674` (≤ 8,000,000) |
| committed public values | `RUST-EXECUTION-PROOF` (20 bytes) |
| `zkvm/rep/out/proof.bin` | `sha256 74854f93d120993c0d0e0de64052919711debb551ce981c34a939fc00e721bb1`, 1,272,589 bytes (1.213 MiB), `compressed` |
| verify | `accepted` against the ELF-derived vk |

### Oracle scoping (declared BEFORE the census)

Only **success-completion** candidates were built and executed. The Step-5
representative demonstrated exactly the success-completion sentinel; the two other
oracle shapes are therefore **out of the proven family** and are bucketed without
building, conservatively (never counted eligible):

* **run-fail / run-crash / failure-status** oracles → `ORACLE-UNREPRESENTABLE`
  (a "must-panic" answer is not the sentinel the family proves).
* **stdout-checked** oracles (`check-run-results` or a `*.run.stdout` sidecar) →
  `ORACLE-UNREPRESENTABLE` (the mined artifact is faithful execution, not a stdout
  value, and no stdout comparator is bound).

### Conservation identity (FROZEN)

**Full partition** over all **26,235** `.rs` roots / **28,735** tasks
(`final-ledger.json`; each file in exactly one terminal bucket, verified by
`tally.py` at both units — `files 26,235 == 26,235`, `tasks 28,735 == 28,735`):

```
                              file-unit   task-unit
MINEABLE-ELIGIBLE                 2,461       2,504   ← N
ANSWERED-PROOF-FIXTURE                1           1   (move-4, real proof exists)
DUPLICATE                             1           1   (one content-clone, see Step 7)
ANSWERED-DIRECT-FIXTURE               0           0
TRIVIAL-CONSTRUCTION                307         312
TOOLCHAIN-INCOMPATIBLE              978       1,159
ORACLE-UNREPRESENTABLE           14,790      16,136
HARNESS-UNSUPPORTED               6,346       7,265
DEFERRED-HIGH-COST                    2           2   (> 8,000,000 cycles)
NO-RUNNABLE-TASK                  1,345       1,350
TIMEOUT                               1           1
ERROR                                 3           4
UNRESOLVED                            0           0
-----------------------------------------------------
TOTAL                            26,235      28,735
```

**Census sub-partition** over the **2,895** `EXECUTION-PROOF-CANDIDATE` files
(`census-ledger.jsonl`, before the Step-7 dedup), each candidate in exactly one bucket:

```
2,895 candidates = 2,462 MINEABLE-ELIGIBLE      (ok + sentinel + ≤ 8M cycles)
                 +   282 TOOLCHAIN-INCOMPATIBLE  (guest build-fail, or ran but no sentinel)
                 +   145 ORACLE-UNREPRESENTABLE  (failure/stdout oracle shape, not built)
                 +     2 DEFERRED-HIGH-COST      (> 8,000,000 cycles)
                 +     3 ERROR                   (executor error)
                 +     1 TIMEOUT                 (build/exec wall-timeout)
```

**Step 7 dedup.** Content-clone key = `sha256(source) + edition + compile-flags`
(input, oracle, and execution contract are identical across all eligibles — empty
stdin, exit-0 sentinel, one uniform harness — so identical `(source, edition, flags)`
means an identical guest artifact). **One** of the 2,462 eligibles was a content-clone
of another and was demoted to `DUPLICATE`, leaving `2,462 − 1 = 2,461` file-unit
eligible.

**N is a strict lower bound.** Every file in N built for the succinct target and ran to
the sentinel within the 8M ceiling. Conservative exclusions (never counted toward N):
`ERROR`/`TIMEOUT`/`DEFERRED-HIGH-COST`; tests that legitimately call
`std::process::exit(0)` halt before the sentinel and land in `TOOLCHAIN-INCOMPATIBLE`
(guest-diverge) even though they pass natively; and any `run-fail`/stdout oracle is
excluded by scope. No case was retried or hand-fixed to raise the count.

### Cycle-ceiling boundary (no ambiguity)

The largest mineable-eligible case is **5,095,191** cycles — **2,904,809** cycles below
the 8,000,000 ceiling. The two `DEFERRED-HIGH-COST` cases
(`array-slice-vec/driftsort-off-by-one-issue-136103.rs`,
`collections/hashset-connected-border-12860.rs`) exceeded the limit outright. No
eligible case sits near the ceiling, so the 8M line is unambiguous.

The `ERROR` (3) and `TIMEOUT` (1) cases are conservatively excluded, not retried:
`test-attrs/issue-34932.rs`, `traits/vtable/lack-of-implied-bounds-for-opaque-types-unsound.rs`,
`never_type/regress/never-as-function-argument.rs` (executor error), and
`attributes/fn-align-dyn.rs` (wall-timeout).

### Corpus fingerprint (git-ignored corpus, hash-pinned here)

| corpus | files | digest |
| --- | ---: | --- |
| `rust-lang/rust` `tests/**` @ `e7795af6` | 43,082 | tests-tree object `831a95ea06a4c9158b01a559b5ec9c2399d0583b` |
| path→blobid manifest | — | `sha256 7b7225bf6279fe827e314bb75e6a754ff3c733c444aa581cdea9e46a26df74dd` |

Commit `e7795af6d2449fb05a6393c3320ced873a999eb3`; sparse `tests/**` cone (blob:none,
depth 1); checkout clean; `missing_tests_blobs = 0`; cross-verified against a second
read-only clone at the same commit and same tests-tree digest. From this frozen tree,
**26,235** `.rs` test roots (**28,735** tasks after revision expansion) form the census
denominator (`static-ledger.json`).

### Census artifact hashes (git-ignored sandbox; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `static-ledger.json` | full static partition (26,235 files → buckets + 2,895 candidates) | `c82019400ddf82f9095e206c45b0cc782f192084faa2e7b649e4bb990663fbee` |
| `candidates-kind.tsv` | 2,895 candidates × oracle kind (success/fail/stdout) | `9e927033e9daf8d4635e303882833f8c601b3a8969d770440ae5162e28c29a31` |
| `census-rules.md` | frozen Step-6 adjudication procedure (declared before execution) | `bb3092642335ad887dd953263525bc91a6f7812fa7fe8569861307df3240d50f` |
| `census-ledger.jsonl` | Step-6 census (2,895 candidates → resolved buckets) | `1ca4a9001bfee2d257ab835ba73eb2758d9ebf0da5ac47b9144b6b9a581f65af` |
| `final-ledger.json` | full resolved partition (26,235 files, post-dedup) | `614e7db0e99807878e445b71ccb551e089362ee0eac87eeabf3e6d1afd88d939` |
| `census-summary.json` | file/task-unit tallies + conservation flag | `7519581d2a5dd9ec0f4e55f38726cca87521f550f47d6b8ca831091df6974abd` |
| `corpus-freeze.json` | corpus commit + tests-tree + manifest freeze | `e4f37f81bcfb8395c49313a94c38b941113799411d8e92f08ec30f89da925061` |
| `toolchain-freeze.json` | native judge compiler freeze | `a737d74c2bf817ca8610853c88f52ad8f8477e4dd6296dfddb244fee57a24e3e` |
| `representative-gate.json` | Step-5 representative hard gate (GO-FAMILY) | `a821ee6f0e2aa5b10a5ff5038c2bbd88327e83c86d2aaebccb40a97ba8546fad` |

Representative guest ELF `sha256 af8c179c…` and proof `sha256 74854f93…` are pinned in
the binding table above.

### Resource policy (FROZEN)

Execute cycle ceiling **8,000,000** (over → `DEFERRED-HIGH-COST`); per-candidate build
wall-timeout 240 s, execute wall-timeout 300 s (over → `TIMEOUT`); **network 0**
(offline, `CARGO_NET_OFFLINE`, dependencies pre-cached); **retries 0**; the census runs
**exactly once** — no retry, no per-problem exception, no manual fix.

### CI scope (what green actually attests)

CI (`self-test`, `supply-chain`) does **not** recompute the 2,895-candidate census or
re-derive N. It verifies that **this freeze record is intact and the repository has no
regression** (`docs-smoke.sh` pins the ceiling label, the conservation identities, the
engine binding, the corpus fingerprint, and the boundary). The N = 2,461 / 2,504 result
is established by the git-ignored sandbox run recorded above, not by CI.

### Boundary / non-claims

* **RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 2,461 files / 2,504 tasks** is the
  issuable problem count under this frozen corpus commit, these pinned toolchains, and
  this uniform execution-proof harness — **not** a claim that these tasks are activated
  on any network. `mineable_now` stays 0.
* Reported as **RUST-FROZEN-SNAPSHOT-MINEABLE-ELIGIBLE = 2,461 (files) / 2,504
  (tasks)**, a snapshot count, not a "Rust permanent total".
* `RUST-REFERENCE-P0-MINEABLE-ELIGIBLE = 0` is a **separate** record and is left
  untouched; this successor family does not modify it and its `0` never meant "Rust = 0".
* Public test source is **not** answer leakage: the mined artifact is a proof of
  faithful execution bound to the guest ELF/vk, not the exit-0 fact. Only the one case
  with an actual generated proof is excluded as answer-confirmed.
* **Per-task ELF/vk differ** — each eligible task has its own guest ELF, vk, and proof;
  there is no single family-wide vk. The representative demonstrates the end-to-end path.
* **Task-unit 2,504 is a projection** of the 2,461 independently-executed file verdicts
  across declared revision variants (one compilation per file, no per-revision `cfg`);
  the file-unit 2,461 is the independently-proven count.
* The 2 `DEFERRED-HIGH-COST` cases are **follow-on scope**, not discarded: a higher
  cycle budget could admit them. `run-fail`/stdout oracles are out of this family's
  scope, not proven ineligible for a future family that binds those oracles.
* No consensus / BF.7 / mining / reward / Base / paid-API change was made; the guest is
  an offline SP1 zkVM program connected to no consensus path.
* This record is **not a public-network / leaderboard / paid-API / production claim** —
  closed-local, non-consensus evidence only.

---

## Entry 2 — 2026-08-11 · proof-statement-dedup-v1 / N = 687 distinct verifiable proof statements

### Result in one line

Re-auditing Entry 1's task unit under a **proof-statement** lens — the honest unit a miner
is actually paid for — the successor ceiling is
**RUST-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 687 distinct verifiable proof statements**
(successor to Entry 1's `2,461 files / 2,504 tasks`, which stays as the prior file/task-unit
reference; Entry 1 and the preamble are unchanged). The current guest harness commits
**only** the fixed 20-byte completion sentinel to the public values and takes empty guest
input, so a task's source / revision / compiler flags / edition-target / task identity are
**not** bound inside the verified statement. The proof statement therefore reduces to a
function of the verifying key alone, and every task sharing a vk is satisfiable by one proof.
Of the 2,499 execution-eligible tasks (2,504 minus the 5 non-execution oracles) only **688**
distinct proof statements exist; one is already answered by the representative's real proof,
leaving **687** newly mineable.

### Why the count moved (2,499 → 687)

`proof_statement_digest = SHA-256( vk ∥ input_digest ∥ expected_pv_digest ∥ verifier_contract )`

with the three non-vk components CONSTANT across the whole family:

* `input_digest       = e3b0c442…b855` — SHA-256 of the empty guest stdin
* `expected_pv_digest = 48b1336c…b2f2` — SHA-256 of the 20-byte sentinel `RUST-EXECUTION-PROOF`
* `verifier_contract  = 79febbe6…0ebc` — SHA-256 of `SP1-COMPRESSED|circuit=v6.1.0|sp1-zkvm=6.3.1|target=riscv64im-succinct-zkvm-elf`

So `proof_statement_digest` is a **bijection with vk**: the measured distinct vk count among
the eligible pool = **688** = the measured distinct proof-statement count. Distinct guest
ELFs = **698**, i.e. 10 ELFs collapse onto an already-seen vk (identical circuit ⇒ identical
statement); **0** ELFs mapped to two different vks (the vk derivation is deterministic).

### Proof-reuse binding gap (operator gate item 4) — honest finding

The audit explicitly checked whether the source digest / revision / compiler flags /
edition-target / task identity are inside the **verified public values**, not merely in
emitter metadata. They are **not**: the guest commits the constant sentinel and nothing
task-specific. Proof reuse across same-vk tasks is therefore **possible**, and the honest
mineable unit is the distinct-proof-statement count, not the file or task count. Binding the
task identity into the public values (so each task becomes its own statement) is recorded as
follow-on scope, not done here.

### Task materialization (operator gate step 2)

The 43 projected revision variants were materialized as independent tasks, each built exactly
once with its official `compiletest` settings applied verbatim (`compiletest` maps `-` → `_`
for `--cfg`, e.g. revision `randomize-layout` → `--cfg randomize_layout`; nightly `-Z` flags
pass through). 81 revision-tasks resulted: **76** reached the completion sentinel
(MINEABLE-ELIGIBLE candidates) and **5** did not, booked NON-EXECUTION-ORACLE —
`mut-ref-mut[classic2024]`, `mut-ref-mut[structural2024]`, `static-mut-refs[e2024]`,
`defaulted-never-note[e2024]`, `empty-supertrait-with-nonempty-supersupertrait[dump]`. Real
flag application was confirmed, e.g. `randomize.rs[randomize-layout]` built under
`-Zrandomize-layout -Zlayout-seed=2`. All 4 parent files keep ≥1 eligible revision, so the
Entry-1 file-unit 2,461 is unchanged.

### Representative (move-4) accounting

The representative's real compressed proof (`zkvm/rep-out/proof.bin`, SHA-256 `74854f93…`,
4,674 cycles) is bound to vk `0x0038de3b…` / ELF `af8c179c…` and is **not** part of the
2,504 denominator. Under the statement lens its vk is shared by **122** census tasks (across
2 distinct guest ELFs `bcbabb13…`, `0803ca1e…`); all 122 are therefore already answered by
that one existing proof and are booked ANSWERED-PROOF-FIXTURE, not mineable. The cross-use
test below verifies that move-4's proof genuinely accepts against those same-vk tasks.

### Cross-use verification (operator gate item 6)

One cross-use REJECT/ACCEPT test on representative collision groups, using the existing
move-4 proof and **no** new proof generation (`xtest/crossuse-result.txt`, SHA-256
`8229606a…`):

* move-4 proof vs its own ELF (vk `0x0038de3b…`) → **ACCEPT**
* move-4 proof vs a DIFFERENT ELF `bcbabb13…` deriving the SAME vk → **ACCEPT** — one proof
  satisfies a different task, i.e. the dedup criterion holds
* move-4 proof vs an ELF deriving a DIFFERENT vk `0x006a8e87…` (the largest, 1,570-task
  collision group) → **REJECT** (`sp1 vk hash mismatch`): a distinct statement; cross-use is
  cryptographically prevented

### Conservation identity (task unit, 8 buckets)

Every one of the 2,504 fixed tasks falls in exactly one bucket:

```
2,504 =   687  MINEABLE-ELIGIBLE          distinct un-answered proof statements — the ceiling
      +     5  NON-EXECUTION-ORACLE       materialized revisions that never complete
      + 1,690  DUPLICATE-PROOF-STATEMENT  same statement as a survivor; one proof covers them
      +   122  ANSWERED-PROOF-FIXTURE     share the representative's vk; answered by 74854f93…
      +     0  DEFERRED-HIGH-COST
      +     0  TIMEOUT
      +     0  ERROR
      +     0  UNRESOLVED
```

### Binding manifest

Per-task binding keys — revision-applied source digest, per-task guest ELF digest, per-task
vk digest, the common verifier contract, execute cycles, the sentinel completion oracle
(`pv_len`), the `proof_statement_digest`, and the `task_identity_digest` that is **not** bound
in-circuit — are recorded for all 2,504 tasks in `binding-manifest.jsonl` (SHA-256
`c1785e0f…`). The statement roll-up is `statement-summary.json` (SHA-256 `64217a9b…`);
per-distinct-ELF vk / cycles / sentinel telemetry is `elfvk-out.jsonl` (SHA-256 `a1da0cbc…`);
revision-task telemetry is the `mat-out-*.jsonl` set (concatenated SHA-256 `6bfc9afc…`). All
stay in the git-ignored sandbox; only these fingerprints are tracked here.

### Boundary / non-claims (Entry 2)

* Entry 1 and the preamble above are **byte-unchanged**; **687** is recorded here as the
  **successor** proof-statement count, keeping 2,461 files / 2,504 tasks as the prior
  file/task-unit reference.
* **687 is a distinct-proof-statement count** — not a file count, not a vk-activation claim;
  `mineable_now` stays **0**.
* The drop from 2,499 to 687 reflects the current harness binding only the sentinel; it is a
  faithfully-reported property of this harness, not a defect found in the corpus.
* No consensus / BF.7 / mining / reward / Base / paid-API change; closed-local, non-consensus
  evidence only — **not a public-network / leaderboard / paid-API / production claim**.
