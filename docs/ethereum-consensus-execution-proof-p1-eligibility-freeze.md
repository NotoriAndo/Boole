# Ethereum-consensus execution-proof P1 — STF-guest gate closure freeze (v1)

Ceiling label: **ETHEREUM-CONSENSUS-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED**.

This is a **gate-closure record, not a zero and not an eligible count**. The census
that would finalize an issuable P1 problem count for SP1 consensus state-transition
execution proofs is **blocked at the guest build**, so the eligible count is **not yet
determined** — it is explicitly **not recorded as 0**. Ethereum-consensus `mineable_now`
stays **0**; this record wires **no** consensus / BF.7 path. It is closed-local,
non-consensus evidence and is **not a public-network / leaderboard / paid-API /
production claim**.

Three separated status labels apply to this domain (each measures a different thing):

* **CORPUS-MATERIALIZED** — the frozen consensus-spec-tests corpus was provisioned,
  digest-verified, and enumerated. The candidate inventory is real: **7,111** STF
  (state-transition) candidate tasks.
* **STF-NATIVE-GATE-VALIDATED-279-OF-279** — the native state-transition engine passed
  the curated conformance gate at **279 / 279** (phase0→deneb, mainnet+minimal, release).
* **STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH** — the SP1 riscv64 guest build fails
  because the non-optional C dependencies `blst` and `c-kzg` cannot compile for the
  zkVM target, and there is **no official SP1 drop-in patch** for either crate at the
  exact versions ethereum-consensus uses.

**Exactly one combination is closed — nothing wider.** This record closes only:

```
ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest
```

It does **not** state "the STF axis is closed" and does **not** state
"Ethereum-consensus is impossible." Other STF codebases, a future official
`blst`/`c-kzg` SP1 patch, or a crypto-free state-transition subset remain open and are
the subject of the successor investigation (see *Successor scope*, below). The candidate
inventory of **7,111** STF tasks is preserved as a **candidate inventory** — neither an
eligible count nor a zero.

This document is an **append-only attestation ledger**. The frozen corpus, the vendored
lockfile, the guest crate, the Gate D build log, the API inventory, and the SP1-patch
probe transcript **stay in the git-ignored sandbox**
(`local-docs/ethereum-consensus-proof-p1-2026-08-11/`); only content hashes, corpus
fingerprints, gate results, conservation identities, and lineage are tracked here so the
result survives outside the sandbox. Future waves append new dated entries below;
existing **merged** entries are never rewritten.

---

## Entry 1 — 2026-08-11 · stf-guest-gate-closure-v1 / N = NOT-YET-DETERMINED (guest build blocked)

### Result in one line

The consensus state-transition corpus is materialized (7,111 STF candidates) and the
**native** engine is gate-validated (279/279), but the **SP1 riscv64 guest build fails
(`rc=101`)** because `blst` 0.3.17 and `c-kzg` 2.1.8 are non-optional C libraries with
no zkVM-compatible compile path and **no official SP1 drop-in patch**. Per operator
instruction the domain is **not** recorded as 0: the issuable P1 count is
**NOT-YET-DETERMINED**, the 7,111 candidates are preserved as a candidate inventory, and
the integrated confirmed subtotal **10,674** with `mineable_now = 0` is **unchanged**.

### STF codebase pin (frozen before results, immutable)

| field | value |
| --- | --- |
| repo | `github.com/ralexstokes/ethereum-consensus` (R&D; correctness authority is the official v1.6.1 expected post-state) |
| commit | `5031d31e318dd861cf3373702c5d92f085d926e4` |
| declared toolchain | `1.85.0` (`rust-toolchain.toml`) |
| host build toolchain | `1.95.0` (sandbox; fresh 2026-08 lock pulls `ruint 1.20.0` MSRV 1.90 via pinned `ssz-rs`→`alloy-primitives`, so 1.85.0 cannot build; STF results are toolchain-version-independent, dependency graph unchanged) |
| guest build toolchain | `succinct` (SP1 fork), target `riscv64im-succinct-zkvm-elf`, `sp1-zkvm = 6.3.1` |
| `Cargo.lock` sha256 | `7a74a6f00ba23f3304a3ccc70873108736aef7b63269b7225c9ba0621d5df63c` |
| vendored crates | **551**; vendor aggregate sha256 `7b9e791a9ef262a1a6a60baeea7a08115032edb9d3b7bc786d3b6234ebcbcf9b` |

**core2 yank patch (operator msg 3775, option 1).** `core2` (all versions) is yanked on
crates.io while `multihash 0.16.x` requires `core2 ^0.4.0` non-optionally, blocking a
fresh resolution. The minimal fix redirects **only** `core2` to byte-identical 0.4.0
source from the allowlisted crates.io tarball
(`[patch.crates-io] core2 = { path = "../patches/core2-0.4.0" }`;
`core2-0.4.0.crate` sha256 `b49ba7ef1ad6107f8824dbe97de947cbaac53c44e7f9756a1fba0d37c1eec505`
== the crates.io sparse-index cksum, `yanked=true`). Same version, same code, non-yanked
source path only; no other dependency, version, or feature changed.

### Corpus fingerprint + task enumeration (CORPUS-MATERIALIZED)

Consensus-spec-tests, frozen and offline-re-verified (**MATCH**, 254,383 files, whole
digest identical since freeze). Leaf layout
`tests/<config>/<fork>/<runner>/<handler>/<suite>/<case>/` — each leaf = 1 expanded task.

```
whole-corpus leaf tasks               73,889
  out-of-scope (gloas + eip7805)      19,437   (operator scope: unstable/nightly forks)
  in-scope T                          54,452   (general 5,509 + mainnet 5,523 + minimal 43,420)
```

Enumeration conservation (each in-scope leaf in exactly one class):

```
54,452 = 7,111 STF-CANDIDATE + 47,341 NON-STF-NO-RUNNABLE-TASK
```

* **STF-CANDIDATE = 7,111** — state-transition runners: `operations`,
  `epoch_processing`, `sanity`, `rewards`, `transition`, `random`, `fork`, `finality`.
  This is the **candidate inventory** whose eligibility is NOT-YET-DETERMINED.
* **NON-STF-NO-RUNNABLE-TASK = 47,341** — `ssz_static`, `ssz_generic`, `kzg`, `bls`,
  `shuffling`, `merkle_proof`, `light_client`, `fork_choice`, `sync`, `genesis`,
  `networking` (not state-transition runners; out of this family's scope).

STF candidates per fork (static enumeration; final eligibility is decided empirically,
not by this table):

| fork | STF cands | core-lib support |
| --- | ---: | --- |
| phase0 | 584 | Executor-wired |
| altair | 765 | Executor-wired |
| bellatrix | 833 | Executor-wired |
| capella | 997 | Executor-wired |
| deneb | 1,052 | Executor-wired |
| electra | 1,451 | module present; Executor block-apply NOT wired (probe empirically) |
| fulu | 1,429 | absent → CLIENT-FORK-UNSUPPORTED |

Per-fork sum `584 + 765 + 833 + 997 + 1,052 + 1,451 + 1,429 = 7,111`.

### Gate results (running log; A–C established, D is the blocker)

| gate | scope | result |
| --- | --- | --- |
| A | host offline build `--locked --offline --workspace` (1.95.0) | **PASS** (29s, warnings only) |
| corpus digest re-verify | offline transition, whole-tree | **MATCH** |
| B | native conformance, curated phase0→deneb via stock harness, release | **PASS** — **279 / 279** (mainnet+minimal, 9.99s) |
| C | fork matrix | **gathered** — phase0..deneb Executor-wired; electra module-present / Executor-unwired; fulu absent → `CLIENT-FORK-UNSUPPORTED` |
| D | SP1 guest riscv64 build | **FAIL (`rc=101`)** — the STF-guest blocker below |
| E–H | proof gates | **not reached** (blocked by Gate D) |

### Gate D failure — STF-GUEST-INCOMPATIBLE (root cause)

`cargo-prove sp1` (`8252c29`, 2026-06-25), target `riscv64im-succinct-zkvm-elf`, exits
`rc=101`. The build log is pinned (git-ignored sandbox
`gate-d-guest/gate-d-build.log`, sha256
`de67682195ca04869a61eeb1a57320153fe891ff3092e2ff0b946a66dbbb99fb`).

* **Root cause:** the non-optional C dependencies **`blst` 0.3.17** and **`c-kzg`
  2.1.8** cannot compile for the zkVM target. `cc-rs` invokes the host `cc` (Apple
  clang 17.0.0) with `--target=riscv64-succinct-zkvm`, which fails:
  `unable to create target: 'No available targets are compatible with triple
  "riscv64-succinct-zkvm"'` while compiling `blst-0.3.17/blst/src/server.c`.
* **Non-optional, not feature-droppable:** `ethereum-consensus/Cargo.toml` declares
  `blst` (line 38) and `c-kzg` (line 46) as `{ workspace = true }` with **no**
  `optional = true`; no `[features]` entry drops them; `default-features = false`
  removes only `serde` / `async`.
* **No local riscv C toolchain:** only `/usr/bin/clang` (Apple clang, no riscv
  backend) is present; no Homebrew/mainline LLVM; SP1's `.sp1/bin` bundles only
  `cargo-prove` (no clang, no riscv-gcc; `rust-lld` is a linker only). Providing a
  cross-compiler is outside the operator provisioning allowlist.
* **No force-patch applied, network not reopened:** SP1's supported `blst`/`c-kzg`
  build path is a force-patch (forbidden by operator msg 3770 §6) that would also
  change BLS/KZG execution semantics and break the native==guest byte-identical gate.

### Official SP1 patch compatibility probe (operator msg 3778) — STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH

Metadata-only probe (`git ls-remote` + `curl`; no clone, no cargo fetch, no code change,
builds stayed offline) against the only authorized sources — `github.com/sp1-patches/*`,
`github.com/succinctlabs/*`, and `raw.githubusercontent.com` of those orgs. Target crate
names + versions that a qualifying drop-in must match: **`blst` 0.3.17** and **`c-kzg`
2.1.8**.

* **BLS — no official drop-in.** `sp1-patches/blst` → **404**; `succinctlabs/blst` →
  **404**. The only BLS-related fork is `sp1-patches/bls12_381`, a **different crate**
  (pure-Rust; tag `patch-0.8.0-sp1-6.2.0` = `9e4e2ae4780d3d69cecbec000f5e814df2392468`).
  ethereum-consensus imports `blst::min_pk` directly; `bls12_381` has a different
  crate/API → not a drop-in, and adopting it is a forbidden code modification +
  "similar library" substitution (operator msg 3778 §3).
* **KZG — no official drop-in for `c-kzg` 2.1.8.** `sp1-patches/c-kzg` → **404**.
  `sp1-patches/c-kzg-4844` exists but is **stale**: no tags at all; heads
  `main = 5b35b6147bf269b16c1bffd268fd03bac0c00b64`,
  `v1.0.0 = b5e3f25f48a6cbda8a1339545b81968981bfe41a`,
  `uma/remove-taiko = ba2780307b218dd36851a22708f722b03b2f23a1`; `bindings/rust/Cargo.toml`
  404 on both `main` and `v1.0.0`; the `v1.0.0` branch is the c-kzg **1.x** era and does
  not satisfy the `c-kzg = "2.1.0"` caret. The maintained SP1 KZG route is
  `succinctlabs/kzg-rs`, a **different crate** (pure-Rust; tag `v0.2.8-sp1-6.2.0` =
  `2d48f8b948746d5cfa62ce7421369278a1c2e405`) → not a drop-in.
* **Newest SP1-version tag seen** across the different-crate forks is `sp1-6.2.0`
  (no `sp1-6.3.1` tag at probe time). Moot — no `blst`/`c-kzg` fork exists to key at all.

Per operator msg 3778 §3, either BLS **or** KZG missing ⇒ immediate STOP with **no code
modification**; both are missing. No representative implementation, no equivalence gate,
and no proof were attempted.

### Conservation of status (no double-counting)

The domain sits in exactly one terminal state for the **specified combination**, and the
candidate inventory is preserved separately:

```
combination { ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest }
    → STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH   (guest build rc=101; no official drop-in)

candidate inventory (unchanged, candidate-only, NOT eligible, NOT zero):
    7,111 STF-CANDIDATE

issuable P1 eligible count for this domain:
    NOT-YET-DETERMINED   (census blocked at Gate D; successor scope below)
```

### Cross-domain running subtotal (this record's problem-count impact = 0)

Ethereum-consensus contributes **no confirmed number** to the integrated subtotal — the
same treatment as Lean (`CORPUS-NOT-MATERIALIZED`), except here the corpus **is**
materialized and only the guest issuance path is blocked (`NOT-YET-DETERMINED`). The
**integrated confirmed subtotal 10,674 is unchanged**:

```
EVM P0                            6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity-semantic P1              1,408   (docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md, successor)
Rust execution-proof P1           2,499   (docs/rust-execution-proof-p1-eligibility-freeze.md, successor)
Solidity P0                           0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0            0   (docs/zk-native-release-audit-census-p0-eligibility-freeze.md)
---------------------------------------
integrated confirmed subtotal    10,674
Ethereum-consensus P1        NOT-YET-DETERMINED   (this record; contributes no number)
Lean                    CORPUS-NOT-MATERIALIZED   (contributes no number)
```

`mineable_now = 0` unchanged. This record modifies **no** other domain's number.

### Lineage (git-ignored sandbox; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `INPUT-FREEZE.md` / `stf/freeze/stf-input-freeze.json` | frozen inputs + gate/probe machine record | `3375a9e99d387992d30e8f866bf6075d2c8d42f7d92c6dcba16e80b63efa5b71` |
| `gate-d-guest/gate-d-build.log` | Gate D `rc=101` build failure evidence | `de67682195ca04869a61eeb1a57320153fe891ff3092e2ff0b946a66dbbb99fb` |
| `stf/freeze/sp1-patch-compat-probe.md` | official SP1 patch probe transcript | `cc90c8b5d2ddcbdbdcc49dd71ba1a702b47f0189376ee10b7f50018e15da22cc` |
| `stf/freeze/blst-ckzg-api-inventory.md` | blst/c-kzg API surface a drop-in must cover | `2154ac3ca01ddb246cd0add58a845c7fbb9330bd52ca1c502eb89ed5e79454b7` |

### CI scope (what green attests)

CI (`self-test`, `supply-chain`) does **not** rebuild the guest or re-run the probe. It
verifies that **this freeze record is intact and the repository has no regression**
(`docs-smoke.sh` pins the ceiling label, the three status labels, the closed-combination
string, the corpus/enumeration conservation, the Gate D log hash, the unchanged
integrated subtotal, and the boundary). The gate results and the NOT-YET-DETERMINED
verdict are established by the git-ignored sandbox run recorded above, not by CI.

### Successor scope (open; investigation-only, no build)

Because the closure is scoped to one exact combination, the successor is limited to a
**numbers-only** classification of the 7,111 STF candidates by their **actual execution
call path** (native instrumentation or call-graph + vector metadata, not folder names)
into six mutually-exclusive buckets — `NO-CRYPTO-REACHABLE`, `BLS-REQUIRED`,
`KZG-REQUIRED`, `BLS-AND-KZG-REQUIRED`, `CLIENT-FORK-UNSUPPORTED`, `UNRESOLVED` — with
conservation `7,111 = Σ six buckets`. No code change, no guest implementation, and no
proof generation are performed in that step; a crypto-free successor guest is proposed
only if `NO-CRYPTO-REACHABLE ≥ 1`. That investigation lands in a later entry.

### Boundary / non-claims (Entry 1)

* **ETHEREUM-CONSENSUS-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED** — an
  issuance-path closure for one exact combination, **not** a zero and **not** an eligible
  count. `mineable_now` stays 0.
* The three labels **CORPUS-MATERIALIZED**, **STF-NATIVE-GATE-VALIDATED-279-OF-279**, and
  **STF-GUEST-INCOMPATIBLE-NO-OFFICIAL-PATCH** measure three different things and are
  reported side by side; none of them means "Ethereum-consensus = 0" or "STF axis closed".
* Only `ethereum-consensus@5031d31e + upstream blst/c-kzg + no-patch + SP1 riscv64 guest`
  is closed. Other STF codebases, a future official `blst`/`c-kzg` SP1 patch, and a
  crypto-free state-transition subset stay open.
* The **7,111** STF candidates are preserved as a **candidate inventory**, not demoted to
  0 and not counted eligible.
* The **integrated confirmed subtotal 10,674** and `mineable_now = 0` are **unchanged**;
  this record contributes no number and edits no other domain's count.
* This is an **issuable-problem-count investigation, not network activation**; no
  consensus / BF.7 / mining / reward / Base / paid-API change was made. Closed-local,
  non-consensus evidence only — not a public-network / leaderboard / paid-API / production claim.

---

## Entry 2 — 2026-08-11 · stf-crypto-call-path-census / N still NOT-YET-DETERMINED (2,293 NO-CRYPTO candidates, not mineable)

### Result in one line

The successor call-path census promised in Entry 1 is complete. Running the **frozen
observation-only instrumented native harness** over the 4,231 executable STF candidates
(phase0→deneb) exactly once — with electra+fulu (2,880) statically CLIENT-FORK-UNSUPPORTED
— classifies all **7,111** candidates by their **actual observed crypto-call path** into
six mutually-exclusive buckets, conserving exactly. **2,293** candidates reach **no BLS and
no KZG call** for their frozen input (`NO-CRYPTO-REACHED-FOR-FROZEN-INPUT`, the bucket
Entry 1 called `NO-CRYPTO-REACHABLE`, renamed per operator msg 3786):
`STF-CRYPTO-CALL-PATH-CENSUS-COMPLETE`. **2,293 is a candidate count, not a mineable
count.** The domain stays **NOT-YET-DETERMINED**, `mineable_now = 0`, and the integrated
confirmed subtotal **10,674** is **unchanged**.

### Measurement freeze (SHA-256 pinned before any result was inspected)

Observation-only instrumentation: a thread-local counter increments at the entry of each
real BLS/KZG function; it does not affect any return value, error, post-state, or
serialization. Audit harness + counters only — no production / guest / consensus logic
changed; `blst` 0.3.17 and `c-kzg` 2.1.8 are **not** swapped (workspace pins identical to
Entry 1). The full freeze manifest (corpus digests, candidate lists, pre/post source,
binary, classifier, execution order) is pinned in the git-ignored sandbox
`census-freeze/MEASUREMENT-FREEZE.md`.

| field | value |
| --- | --- |
| client checkout | `5031d31e318dd861cf3373702c5d92f085d926e4` (unchanged from Entry 1) |
| BLS / KZG backend | `blst` 0.3.11→lock 0.3.17 / `c-kzg` 2.1.0→lock 2.1.8 (unchanged; no substitution) |
| instrumented census binary sha256 | `3239543b979db1956d816b6abc91fc91a3a0c5d81fa0a6325dd28ddca7eb3648` |
| stock reference binary sha256 | `a102a87f9e94b0d4a460e424fdf98679e927d720bbf091a2672bfa006b931acc` |
| execution order | fixed nodeid ascending, `--test-threads=1`, exactly one pass, no retry |

**Pre-result gates (all PASS, recorded before results):**

* **MAP-COMPLETE** — exhaustive crypto-call map: 5 BLS + 6 KZG entry points instrumented;
  the only STF-path c-kzg touch is the trusted-setup load at `context.rs:205` (Context
  construction, outside the per-case counter bracket). No FFI / re-export bypass. No gap.
* **Transparency gate** — stock vs instrumented on the bundled 279 official vectors: all
  279 per-case outcomes byte-identical (digest
  `0695d93b55cfccf33d89f34af4a008c2649b44b1b8c6a716091b69a57deed977`), 279/279 pass. The
  binaries differ in content, so the instrumentation is real yet decision-transparent — no
  `INSTRUMENTATION-NONTRANSPARENT` trigger.
* **RED→GREEN fixture gate** (nodeid ascending): KZG-forced fixture → kzg counter fires
  (bls 0 / kzg 1); no-crypto fixture → 0/0; BLS-forced fixture → bls 1 / kzg 0; a
  spec-drift panic case → recorded `panic` (not mis-scored as a 0-call); the clean case
  after it → 0/0 (counters reset after a caught panic; no contamination).

### Six-bucket classification (CENSUS-COMPLETE; conservation exact)

Priority cascade, first match wins, per candidate: (1) electra/fulu →
CLIENT-FORK-UNSUPPORTED; (2) execution did not reproduce the frozen expected outcome (Trial
`err` or `panic`) → UNRESOLVED; (3) BLS>0 ∧ KZG>0 → BLS-AND-KZG-REQUIRED; (4) BLS>0 →
BLS-REQUIRED; (5) KZG>0 → KZG-REQUIRED; (6) BLS=0 ∧ KZG=0 →
NO-CRYPTO-REACHED-FOR-FROZEN-INPUT.

```
7,111 = 2,293 NO-CRYPTO-REACHED-FOR-FROZEN-INPUT + 1,830 BLS-REQUIRED + 0 KZG-REQUIRED + 0 BLS-AND-KZG-REQUIRED + 2,880 CLIENT-FORK-UNSUPPORTED + 108 UNRESOLVED
```

| bucket | count | meaning |
| --- | ---: | --- |
| NO-CRYPTO-REACHED-FOR-FROZEN-INPUT | 2,293 | no BLS/KZG call observed for the frozen input — candidate only |
| BLS-REQUIRED | 1,830 | ≥1 BLS call, 0 KZG |
| KZG-REQUIRED | 0 | — |
| BLS-AND-KZG-REQUIRED | 0 | — |
| CLIENT-FORK-UNSUPPORTED | 2,880 | electra 1,451 + fulu 1,429 (never executed) |
| UNRESOLVED | 108 | execution did not reproduce the frozen outcome (spec drift) |

Record-set integrity: 4,231 expected executable == 4,231 records emitted, 0 duplicate, 0
unexpected, 0 missing, 0 many-to-many. The harness-emitted record names equal the frozen
candidate list exactly — a proven bijection (the `heck::to_snake_case` collapse of the raw
`__` directory names, 0 collisions).

### Per-fork × bucket

| fork | NO-CRYPTO | BLS | KZG | BLS+KZG | FORK-UNSUP | UNRES | total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| phase0 | 329 | 255 | 0 | 0 | 0 | 0 | 584 |
| altair | 363 | 402 | 0 | 0 | 0 | 0 | 765 |
| bellatrix | 447 | 386 | 0 | 0 | 0 | 0 | 833 |
| capella | 562 | 435 | 0 | 0 | 0 | 0 | 997 |
| deneb | 592 | 352 | 0 | 0 | 0 | 108 | 1,052 |
| electra | 0 | 0 | 0 | 0 | 1,451 | 0 | 1,451 |
| fulu | 0 | 0 | 0 | 0 | 1,429 | 0 | 1,429 |

### Outcome + crypto-call totals (executed 4,231)

```
ok  (frozen outcome reproduced)   4,123   (success 2,595 + reject/no-post 1,528)
err (ran, disagreed with vector)     31
panic (assertion/harness abort)      77
timeout                               0    (no timeout mechanism; run completed)
total BLS calls                  62,603
total KZG calls                       0
```

**KZG = 0 confirmed by both conditions** (operator msg 3786): (1) 0 STF-map paths reach any
KZG entry point, and (2) runtime KZG-call sum over the 4,231 executed STF cases = 0. The
standalone `kzg` runner is not one of the 8 STF runners and is not mixed into this count.

**UNRESOLVED = within-deneb spec drift.** All 108 are valid deneb blocks the v1.6.1 vectors
expect to apply but the Deneb-era v1.4.0 client mis-handles (valid 2,703 − success 2,595 =
108); not one invalid / rejection case drifted. Per the cascade, execution that fails to
reproduce the frozen outcome cannot certify a crypto path, so it is UNRESOLVED — never
counted as a no-crypto or a success. By handler (all deneb): sanity/blocks 56,
transition/core 30, random 16, finality 5, block_header 1.

### Status: 2,293 is a candidate count, not a mineable count

`NO-CRYPTO-REACHED-FOR-FROZEN-INPUT = 2,293` means only "no BLS/KZG call was observed for
that frozen input on this client." It is **not** guest-buildable, **not** mineable-eligible,
and **not** a finalized number. The issuable P1 eligible count stays **NOT-YET-DETERMINED**;
this entry contributes **no** confirmed number.

### Cross-domain running subtotal (this record's problem-count impact = 0)

The **integrated confirmed subtotal 10,674 is unchanged** (byte-identical to Entry 1):

```
EVM P0                            6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity-semantic P1              1,408   (docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md, successor)
Rust execution-proof P1           2,499   (docs/rust-execution-proof-p1-eligibility-freeze.md, successor)
Solidity P0                           0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0            0   (docs/zk-native-release-audit-census-p0-eligibility-freeze.md)
---------------------------------------
integrated confirmed subtotal    10,674
Ethereum-consensus P1        NOT-YET-DETERMINED   (this record; contributes no number)
Lean                    CORPUS-NOT-MATERIALIZED   (contributes no number)
```

`mineable_now = 0` unchanged. This record modifies **no** other domain's number.

### Lineage (git-ignored sandbox; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `census-freeze/MEASUREMENT-FREEZE.md` | full pre-result freeze manifest (corpus, lists, source, binary, classifier, order) | `ecc6870931a80da094c357d2f74beb3f16cd8a40647f5e96e76155794a01f6f4` |
| `census-freeze/census-records.tsv` | per-case `name/bls/kzg/outcome`, 4,231 rows (single pass) | `d752378350bc3f0aec857ec236ab734e0b834b490ddd426fcadaa4dc6fb69a84` |
| `census-freeze/CENSUS-RESULTS.md` | six-bucket result + cross-tabs narrative | `4af09a227d4be084fcefb88cc728d0cded147a5cad39dd2656f59e02f2b26db8` |
| `census-freeze/CENSUS-RESULTS.txt` | classifier raw output (conservation + per-handler) | `706c12e1dc912d81fc672e23dcccd1e4298a983d44729b94797ae8a566742d63` |
| `census-freeze/executable-4231.canonical.txt` | frozen executable candidate list (harness-canonical) | `57f0e98d6bad290bd4d64616ae759b1cf01ff5805f198f9166213d9846b4aa06` |

### CI scope (what green attests)

CI (`self-test`, `supply-chain`) does **not** re-run the census. It verifies that this
freeze record is intact and the repo has no regression (`docs-smoke.sh` pins the census
label, the six-bucket conservation identity, the candidate-not-mineable statement, and the
census-records digest). The bucket counts are established by the git-ignored sandbox run
recorded above, not by CI.

### Successor scope (authorized; separate entry)

The census clears Entry 1's precondition (`NO-CRYPTO-REACHED-FOR-FROZEN-INPUT ≥ 1`: 2,293 ≥
1). Per operator msg 3792 a successor step is authorized: build **one shared** SP1 guest for
the 2,293 that keeps `blst`/`c-kzg` unswapped and is **fail-closed** at the BLS/KZG boundary
(any crypto call → `CRYPTO-PATH-REACHED` reject), binds full task identity into the public
values, and passes representative RED→GREEN + resource + one-proof gates before any
2,293-wide re-execution. That build, its gates, and the resulting `2,293 = MINEABLE-ELIGIBLE
+ …` conservation land in a **later entry**; no guest build, proof, or mineable
finalization is recorded here. UNRESOLVED 108 and BLS-REQUIRED 1,830 are preserved outside
the successor scope.

### Boundary / non-claims (Entry 2)

* **NOT-YET-DETERMINED unchanged.** `2,293 NO-CRYPTO-REACHED-FOR-FROZEN-INPUT` is a candidate
  count, not a mineable count, not an eligible count, not a zero. `mineable_now` stays 0.
* The census is **observation-only**: no production / guest / consensus code changed;
  `blst`/`c-kzg` not substituted; decision-transparent at 279/279.
* **KZG-REQUIRED = 0** and **BLS-AND-KZG-REQUIRED = 0** for this client on the STF path,
  confirmed by both map and runtime. This does not claim KZG is unreachable in general.
* The **integrated confirmed subtotal 10,674** and `mineable_now = 0` are **unchanged**;
  this record contributes no number and edits no other domain's count.
* This is an **issuable-problem-count investigation, not network activation**; no
  consensus / BF.7 / mining / reward / Base / paid-API change was made. Closed-local,
  non-consensus evidence only — not a public-network / leaderboard / paid-API / production claim.
* **Entry 1 and the preamble remain byte-unchanged.**

---

## Entry 3 — 2026-08-12 · stf-successor-one-proof-gate / proof-issuance feasibility PASS (N still NOT-YET-DETERMINED)

### Result in one line

The successor **one-proof gate** promised in Entry 2 (the resource + single-proof
precondition that must hold before any 2,293-wide execution) **passed**. One SP1
**compressed** proof of the frozen crypto-fail-closed v2 guest, on the pre-registered
out-of-corpus **calibration** fixture (byte-identical to the F-OP `block_header` case but
a **separate task identity**), was produced **exactly once (0 retries)** inside the
authorized envelope, and it (a) passes **real SP1 vk verification** bound to its frozen
descriptor (**ACCEPT**), (b) is **rejected** against a cross-task descriptor
(**REJECT**), and (c) is **rejected** after a 1-byte public-values tamper (**REJECT**):
`STF-SUCCESSOR-ONE-PROOF-GATE-PASS`. This establishes **proof-issuance feasibility** and
its measured resource cost; it is **not** a guest-wide build, **not** a 2,293 result, and
**not** a mineable count. The domain stays **NOT-YET-DETERMINED**, `mineable_now = 0`, and
the integrated confirmed subtotal **10,674** is **unchanged**.

### Measurement freeze (SHA-256 pinned before the proof was run)

The single authorized attempt was pre-registered and hash-frozen before firing
(`run2-PREREG-FREEZE.json`); an earlier attempt on the same frozen ELF/input (run-1) is
preserved as **consumed** (`run1_*`) and produced no proof (see run-1 note). The proof ran
offline (`CARGO_NET_OFFLINE=true`; compressed STARK needs no trusted-setup download).

| field | value |
| --- | --- |
| client checkout | `5031d31e318dd861cf3373702c5d92f085d926e4` (unchanged from Entry 1) |
| run id (frozen) | `run2-lowmem-serial-d026acb446de` |
| pre-registration freeze hash | `c79814f730e40c3a081df161a50c04fe8a4270381d57f7d7da3d9d14cdf3b880` |
| guest ELF sha256 | `9eb562bb230015230c2a24377f1ff9df32d2fedd0a9eab959ad5de8dd9e71351` |
| guest vk (bytes32) | `0x007ea5f61663a58247895f2ddf9044fd7edbb4eaf9a75a6b7979b67cd6014507` |
| calibration input sha256 | `e3311e587f5a164e2850f0c9dadd8a333787976cc00ae00ed59b74c825a1270e` |
| SP1 low-mem serial env sha256 | `c1670c2517a752b874b8996746b539f7b6d41c619ee48c5d60db9bd6ef596a66` |
| SP1 SDK | `6.3.1`, CPU prover, **compressed** mode |
| execution order | exactly one `prove`, **0 retries**, 1-second raw memory sampling |

**Pre-result gates (all PASS, recorded before the verdict):**

* **vk guard** — the guest vk re-derived at prove time equals the frozen vk
  `0x007ea5f6…`; proving was attempted on the correct guest (`setup_frozen_vk` bails
  otherwise).
* **Envelope** — hard caps wall ≤ 4 h, RSS ≤ 48 GiB, proof size ≤ 4 MiB, retries = 0; a
  watchdog sampled the prover process tree every 1 s.
* **Config under test (operator directive 3822)** — SP1 low-memory **serial** config:
  every `SP1_WORKER_NUM_*=1` and every `SP1_WORKER_*_BUFFER_SIZE=1` (≥1 required),
  `RAYON_NUM_THREADS=4`, arities 4, `VERIFY_INTERMEDIATES=true`, `SHARD_SIZE=4194304`,
  `MINIMAL_TRACE_CHUNK_THRESHOLD=8388608`. `TRACE_CHUNK_SLOTS=2` is a **NO-OP on this
  macOS / portable executor** (effective only on x86_64-linux native); this run therefore
  measures the **worker/buffer serialization + Rayon=4 effect only**.

### One-proof gate result (produced exactly once; NOT re-proved)

| field | value |
| --- | --- |
| status | **PROOF-GREEN** (`prove_rc=0`, `retries=0`, `kill_reason=none`) |
| elapsed | **4,692 s ≈ 1 h 18 m** (cap 14,400 s — held) |
| peak process-tree RSS | **35.46 GiB** (cap 48 GiB — held) |
| peak swap used | **9,732 MiB ≈ 9.5 GiB** (baseline at t=0; declined during the run — no added saturation) |
| compressed proof size | **1,272,793 B = 1.214 MiB** (cap 4 MiB — held) |

Cryptographic task-binding gate (re-runnable checks; NOT re-proving):

| check | descriptor | result | want |
| --- | --- | --- | --- |
| calibration | `…calibration_proofcost::fop_same_bytes_v2_oneshot` (tc(v2) `af0fcc73…`) | **ACCEPT** | ACCEPT |
| cross-task | real F-OP `…pyspec_tests::basic_block_header` (tc(v2) `dcaa13eb…`) | **REJECT** | REJECT |
| tampered | pv[0] `0xe6→0xe7`, verify vs calibration | **REJECT** (`Invalid public values`) | REJECT |

Public values bound in the ACCEPT proof: version_tag `e684abb4…`, task_contract(v2)
`af0fcc73…`, result_digest (post_state_root) `18df6cd7…`, outcome `0x01`,
crypto_call_free `0x01`. The proof verifies **only** under its frozen task identity — a
different task or a mutated byte fails real vk verification.

### run-1 note (preserved as consumed; cause NOT re-attributed)

An earlier attempt on the same frozen ELF/input under a non-serial config received an
**external SIGKILL (rc=137) at ~91 s** with no proof artifact; it is labelled
`EXTERNAL-SIGKILL / KILL-CAUSE-UNATTRIBUTED` (memory pressure is the leading hypothesis,
but no kernel kill line / Jetsam report confirmed the direct cause). run-2 reached a
**higher** RSS peak (35.46 vs 22.58 GiB) yet **completed** with swap at baseline; the
serial config appears to have lowered peak memory bursts. This is an **observation** —
run-1's direct kill cause is **not** retroactively re-attributed.

### Cross-domain running subtotal (this record's problem-count impact = 0)

The **integrated confirmed subtotal 10,674 is unchanged** (byte-identical to Entry 1):

```
EVM P0                            6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity-semantic P1              1,408   (docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md, successor)
Rust execution-proof P1           2,499   (docs/rust-execution-proof-p1-eligibility-freeze.md, successor)
Solidity P0                           0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0            0   (docs/zk-native-release-audit-census-p0-eligibility-freeze.md)
---------------------------------------
integrated confirmed subtotal    10,674
Ethereum-consensus P1        NOT-YET-DETERMINED   (this record; contributes no number)
Lean                    CORPUS-NOT-MATERIALIZED   (contributes no number)
```

`mineable_now = 0` unchanged. This record modifies **no** other domain's number.

### Lineage (git-ignored sandbox; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `successor-harness/run2-PREREG-FREEZE.json` | pre-registration + hash freeze (frozen before firing) | `ff2a3d949158062caf9bb0f5ee1f589992d78fa05b993ceac39e7fd2d34d3835` |
| `successor-harness/run2_proof_oneshot_result.json` | machine result record (status, caps, verify outcomes) | `365a3a1a0c507756841feac689a9a5ab5ec029c0a738f5b80b19a9393baf848b` |
| `successor-harness/run2_proof_oneshot.log` | full one-shot prove + verify transcript | `48e66a479c8dbfaf429823f69fddff7edff09e3150d26823c40722a2bd608b2a` |
| `successor-harness/run2_mem_samples.csv` | 1-second raw memory samples (4,513 rows) | `74892f24ef63eab3c6b2164ff18bada8314f0f998f8405d4492e84cbf51ce8de` |
| `successor-harness/run2_calib_proof_compressed.bin` | the compressed proof (1.214 MiB) | `50616853d5055eebed2ccadfb843d3429150f04f3f133f0af2585efa645b6b36` |
| `successor-harness/PROOF-ONESHOT-GREEN.md` | run-2 PROOF-GREEN attestation (sandbox) | `6f95537edf8f3d6cec9cfcb1fb50101fd485f61031f89be173bb434e09903b6b` |

### CI scope (what green attests)

CI (`self-test`, `supply-chain`) does **not** re-run the proof. It verifies that this
freeze record is intact and the repo has no regression (`docs-smoke.sh` pins the
one-proof-gate label, the frozen run id, the compressed-proof digest, and the unchanged
integrated subtotal). The PROOF-GREEN verdict and the resource cost are established by the
git-ignored sandbox run recorded above, not by CI.

### Successor scope (authorized; separate entry)

With proof-issuance feasibility established, the successor step (operator directive 3828)
is: build **one shared** crypto-fail-closed SP1 guest covering **all 5 STF runners and 30
handlers** for the 2,293 NO-CRYPTO candidates — **no per-problem guest, vk, or exception
patch** — pass a representative **6-positive / 2-crypto-negative RED→GREEN** gate, then
freeze ELF/vk/ledger/config and run the 2,293 **EXECUTE** exactly once. The resulting
`2,293 = MINEABLE-ELIGIBLE + …` conservation and the finalized `MINEABLE-ELIGIBLE = N`
(recorded only if it conserves) land in a **later entry**; no 2,293-wide result or mineable
finalization is recorded here. BLS-REQUIRED 1,830, UNRESOLVED 108, and
CLIENT-FORK-UNSUPPORTED 2,880 stay outside the successor scope.

### Boundary / non-claims (Entry 3)

* **NOT-YET-DETERMINED unchanged.** run-2 is a **single calibration proof** on the
  F-OP-same-bytes out-of-corpus fixture (separate identity), establishing feasibility +
  cost + task-binding soundness — **not** a 2,293 result, **not** a mineable count, **not**
  an eligible count, **not** a zero. `mineable_now` stays 0.
* The proof is **task-bound**: it accepts only its frozen descriptor and rejects cross-task
  and tampered proofs under real SP1 vk verification. `blst`/`c-kzg` remain unswapped; the
  guest is crypto-fail-closed (this proof is on a NO-CRYPTO path).
* Produced **exactly once** (0 retries) within the wall/RSS/size envelope; no same-host
  re-tune. run-1 preserved as consumed; its kill cause is not re-attributed.
* The **integrated confirmed subtotal 10,674** and `mineable_now = 0` are **unchanged**;
  this record contributes no number and edits no other domain's count.
* This is an **issuable-problem-count investigation, not network activation**; no
  consensus / BF.7 / mining / reward / Base / paid-API change was made. Closed-local,
  non-consensus evidence only — not a public-network / leaderboard / paid-API / production claim.
* **Entry 1, Entry 2, and the preamble remain byte-unchanged.**

---

## Entry 4 — 2026-08-13 · stf-successor 2,293 EXECUTE census conservation PASS (cycle-band candidates only; NO mineable determination)

### Result in one line

The full **2,293-row EXECUTE census** of the frozen shared successor guest (the run
authorized in Entry 3's successor scope) completed with a conservation-verified exact
partition: `STF-SUCCESSOR-2293-EXECUTE-CENSUS-CONSERVATION-PASS` —
**2,293 = 2,292 MONOLITHIC-EXECUTE-ELIGIBLE + 1 CHUNKING-CANDIDATE** (every executed row
ACCEPT `0x01` with `crypto_call_free = 0x01`; 0 failures, 0 decode errors, 0 crypto-path
rows, 0 duplicates, 0 missing). Applying the common resource policy (adopted **after**
measurement, on criteria **independent** of the census distribution) splits the executed
set by cycle cost only:
**2,293 = 2,206 MONOLITHIC-CYCLE-BAND-CANDIDATE + 86 CHUNKING-REQUIRED + 1 CHUNKING-CANDIDATE**.
Per operator instruction (2026-08-13, msg 3851) the 2,206 are recorded **only** as
cycle-band candidates: **NO mineable determination** is made here — the finalized
`MINEABLE-ELIGIBLE = N` anticipated by Entry 3's successor scope does **not** land in this
entry and is blocked until the proving-band verification and the chunked-execution family
complete. The domain stays **NOT-YET-DETERMINED**, `mineable_now = 0`, and the integrated
confirmed subtotal **10,674** is **unchanged**.

### Frozen census identity (identical for every authoritative row)

| field | value |
| --- | --- |
| client checkout | `5031d31e318dd861cf3373702c5d92f085d926e4` (unchanged from Entry 1) |
| shared guest ELF sha256 | `fea14d20387282beae9a979c3f090bc77205120ec75c8a703a705a177c6f7dc8` |
| guest vk (bytes32) | `0x0003a9adb0cf23bdc62009d9ab633d651c91fb59d976a0b4467efef8c912dc59` |
| manifest (2,293 rows) sha256 | `3a361b7961c4d07b6ae920b5be9876a10f88b43801d2849d3681059f1affcae0` |
| mode | SP1 6.3.1 **EXECUTE** (no proofs produced or consumed by the census) |
| retired pre-refix pair | ELF `a862aeee…` / vk `0x002606c0…` (general sanity-parser fix, no per-fixture patch; gate RED 8/10 → GREEN 10/10 re-freeze; sandbox `REP-GATE-GREEN-FREEZE.md`) |

One shared crypto-fail-closed guest covers all rows — **no per-problem guest, vk,
exception patch, or per-nodeid retry**. Every row's `task_contract(v2)` was independently
recomputed host-side from the raw input bytes and matched against the guest's committed
public values.

### Execution provenance (three passes + one operator-ordered abort; no `--resume` anywhere)

| pass | rows | disposition |
| --- | --- | --- |
| pass 1 (10-worker, retired ELF `a862aeee…`) | 451 | **STALE — discarded** (superseded by the guest re-freeze) |
| pass 2 (10-worker, ELF `fea14d20…`) | 997 | **ABORTED-HOST-WATCHDOG-PANIC — discarded** (memory-pressure Jetsam → kernel watchdog panic, host reboot); kept only as a cross-check corpus |
| run3b (hardened sequential runner, concurrency = 1) | **1,126** | **PARTIAL-FROZEN-AUTHORITATIVE** (indexes 0–1,125; outcome tally `{0x01: 1,126}`; rows aggregate `4cb7d24f…`) |
| aborted case (i = 1126) | 1 | **ABORTED-FOR-CHUNKING / CHUNKING-CANDIDATE** (operator order; see below) |
| run3c (same runner/ELF/vk/manifest, new run id, index range [1127, 2293)) | **1,166** | **SUBSET-COMPLETE** (outcome tally `{0x01: 1,166}`; rows aggregate `66891537…`) |

* **Sealed row aggregates** — run3b
  `4cb7d24fbe38ef5f84ec7f3105b688c2cff306a0e7e3bd1efe3d11f13c62726f`; run3c
  `66891537835b845ea16a1d0a4e02841e60a72fb811cfe112dfe37b4edd0909f8` (each re-verified
  row-by-row and re-matched against its sealed marker during synthesis, before the final
  census-wide marker was written).
* **Cross-check** — the 997 discarded pass-2 rows overlap run3b on all 997 indexes and are
  **byte-identical 997/997 (0 mismatches)** across
  `nodeid / outcome / crypto_free / result_digest / task_contract / cycles / gas`; the
  panic run corroborates, but does not constitute, the authoritative results.
* **Aborted case** — `mainnet::phase0::sanity::slots::pyspec_tests::historical_accumulator`
  (i = 1126): an **8,192-slot** (`SLOTS_PER_HISTORICAL_ROOT`) monolithic EXECUTE, estimated
  **~8.8e12 cycles ≈ 128×** the largest completed row (69.9e9). Terminated by operator
  order (msg 3843) via graceful SIGTERM after ~7 h 10 m CPU-active execution; **0 surviving
  guest processes, no retry**. Classified **NOT a failure, NOT mineable**; in-flight raw
  inputs hash-preserved (`451e22f5…`, `60fc4b1d…`); expected `task_contract(v2)`
  `8bde9145dd25d4ad9d7a8327aee7871360f0088a86fd8fb4bde92b13b6f83e43`.

### Hardened runner + separated completion markers

* Per-row durability `tmp → fsync → atomic rename → dir fsync`; frozen ELF/vk/manifest
  sha re-checked at startup and per row; free-memory guard between rows; **0 retries,
  0 per-problem patches**. Runner selftest **18/18**, synthesis selftest **8/8**
  (RED→GREEN before each run).
* run3c's `SUBSET-COMPLETE.json` attests **only** "1,166 continuation rows done" (operator
  correction, msg 3846). The census-wide marker `census-final-20260813/COMPLETE.json` was
  generated **separately and only after**: both row dirs re-verified row-by-row → sealed
  aggregates re-matched → the exact partition `1,126 + 1 + 1,166 = 2,293` held with
  0 duplicates / 0 missing → the conservation equation passed.

### Conservation result + measured cycle distribution

**`2,293 = 2,292 MONOLITHIC-EXECUTE-ELIGIBLE + 1 CHUNKING-CANDIDATE`** — all 2,292
executed rows returned ACCEPT with `crypto_call_free`; 0 `EXECUTE_MISMATCH`,
0 `CRYPTO_PATH_REACHED`, 0 `DECODE_ERROR`, 0 `UNSUPPORTED`.

| stat (RISC-V cycles, n = 2,292) | value |
| --- | --- |
| min / p50 / p90 | 1,483,221 / 174,008,769 / 446,657,506 |
| p99 / max | 20,204,807,813 / 69,876,598,068 |
| total | 1,758,447,705,894 (≈ 1.76e12) |

### Common resource policy → cycle-band split (NO mineable determination)

* **Adoption** — `EC-STF-P1-RESOURCE-POLICY-v1` was adopted **after** the Entry 3 run-2
  measurement, on criteria **independent of the 2,293 census distribution** (anchor: the
  430,618,868-cycle calibration proof at 4,692 s inside the authorized 14,400 s / 48 GiB
  envelope) — **not a pre-freeze** (operator correction, msg 3846). Policy sha256
  `41a79e249d8e70f9a832f1346ac7e4c9a2875b8ec4047f12869a78bfd3511f7c` (supersedes the
  wording-only `a5ffc5b2…`; threshold and rule byte-identical).
* **Threshold** — `T = floor(430,618,868 × 14,400/4,692 × 0.75) = 991,194,325 cycles`,
  applied **uniformly — no per-nodeid exceptions**.
* **Split** —
  **`2,293 = 2,206 MONOLITHIC-CYCLE-BAND-CANDIDATE + 86 CHUNKING-REQUIRED + 1 CHUNKING-CANDIDATE`**.
  Naming note: the sandbox artifact `PROVISIONAL-CLASSIFICATION.json` labels the 2,206-row
  set `MINEABLE-ELIGIBLE` with an explicit provisional / finalization-blocked marker; per
  operator instruction (msg 3851) this ledger records that set **only** as
  `MONOLITHIC-CYCLE-BAND-CANDIDATE` — membership in the cycles ≤ T band, **not** an
  eligibility or mineable determination. The band becomes decidable only via the
  calibration-covered proving-band verification (successor scope below).
* **The 86 CHUNKING-REQUIRED rows** are all **mainnet-preset** (0 minimal):
  `sanity::slots` 30 (6 per fork × phase0/altair/bellatrix/capella/deneb; top
  `double_empty_epoch` ≈ 69.9e9 cycles), `phase0::rewards` 44 (basic 19, leak 15,
  random 10), `phase0::epoch_processing::rewards_and_penalties` 12; smallest over-T row
  1,030,350,809 cycles.

### Cross-domain running subtotal (this record's problem-count impact = 0)

The **integrated confirmed subtotal 10,674 is unchanged** (byte-identical to Entry 1):

```
EVM P0                            6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity-semantic P1              1,408   (docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md, successor)
Rust execution-proof P1           2,499   (docs/rust-execution-proof-p1-eligibility-freeze.md, successor)
Solidity P0                           0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0            0   (docs/zk-native-release-audit-census-p0-eligibility-freeze.md)
---------------------------------------
integrated confirmed subtotal    10,674
Ethereum-consensus P1        NOT-YET-DETERMINED   (this record; contributes no number)
Lean                    CORPUS-NOT-MATERIALIZED   (contributes no number)
```

`mineable_now = 0` unchanged. This record modifies **no** other domain's number.

### Lineage (git-ignored sandbox; hash-pinned here)

| artifact | role | sha256 |
| --- | --- | --- |
| `successor-harness/census-run3b-20260812/FROZEN-PARTIAL.json` | partial freeze record incl. aborted-case lineage | `7617468f187bad5fd9b5dd15fad1283ce29c4985696859108a9b8a32e729e1e1` |
| `successor-harness/census-run3b-20260812/SHA256SUMS` | per-file digests: 1,126 rows + abort raw evidence + log | `0c469fba592d304e89c342e2b5ac15b714595a3275d4549498e36c2b3833e7f6` |
| `successor-harness/census-run3b-20260812.log` | run3b full runner transcript | `0b261f18d4d472e5e2636808bdc721686775ffd605a62d2f093beaedbc491858` |
| `successor-harness/census-run3c-20260813/SUBSET-COMPLETE.json` | continuation subset marker (1,166 rows only) | `ea8a0e4b56a2b580b7625d2eb187cb2a9637832887dbbca8efaa72905502520c` |
| `successor-harness/census-run3c-20260813.log` | run3c full runner transcript | `68dc1c2bffb7e2b1d35e7a9804e3a6dc7c577d0397c31b98d05ef82786f0df7f` |
| `successor-harness/census-final-20260813/COMPLETE.json` | conservation-verified census-wide final marker | `b9da6d51df6b8a6674243d07157598bb503b3dcf05aa7449fdc2e712437f095c` |
| `successor-harness/census-final-20260813/PROVISIONAL-CLASSIFICATION.json` | policy application output (2,206 / 86 / 1) | `ee864362d833d1f6e15a72f157b124d596b61ef649c23e4f8d80aadb94476241` |
| `successor-harness/resource-policy-v1.json` | common resource policy (T, uniform rule) | `41a79e249d8e70f9a832f1346ac7e4c9a2875b8ec4047f12869a78bfd3511f7c` |
| `successor-harness/census_run3.py` | hardened sequential runner (range-capable) | `3b7a135ae62e7e349852222802b98abba40e306e725f9f8533e15eadce76b855` |
| `successor-harness/census_synthesize.py` | conservation synthesis (final-marker writer) | `64b17024305bb5522673e2b62a5cfa38139e61aa9bb1f50cc62aa59add251303` |
| `successor-harness/apply_resource_policy.py` | uniform policy applier | `f48127880e4105082a49e182989ab6ef30c08dfe3599b941d4caa1cbf4a5bd03` |

### CI scope (what green attests)

CI (`self-test`, `supply-chain`) does **not** re-run the census. It verifies that this
freeze record is intact and the repo has no regression (`docs-smoke.sh` pins the census
label, both conservation equations, the threshold, and the run3b aggregate). The census
results are established by the git-ignored sandbox artifacts hashed above, not by CI.

### Successor scope (authorized; separate entries)

Operator directive (2026-08-13, msg 3851) — none of this lands in this entry:

* **Proving-band verification** — freeze the resource axes (cycles, gas, input size,
  public-values size, shard/SP1 report) **before** analysis; produce and verify a real
  compressed proof of an out-of-corpus calibration fixture byte-identical to a candidate,
  within 4 h / 48 GiB / 4 MiB; only cases whose resource vector is covered by the
  calibration vector on **every** axis may become `MONOLITHIC-MINEABLE`; cross-task and
  tampered proofs must REJECT; candidate consumption 0; no post-hoc threshold adjustment.
* **Chunked-execution family** for the 86 + 1 rows — fixed slot-interval chunks, each
  bound to parent task / sequence number / range / start & end state roots / fork /
  policy / live challenge; chain linkage (previous chunk's end root = next chunk's start
  root; final root = the official oracle); REJECT on omission, duplication, reorder, or
  cross-proof; corpus-wide materialization only after one out-of-corpus chunk proof is
  GREEN.
* **Accounting separation** — logical problem count vs actual mining work-unit count;
  parent and chunks never double-counted.

### Boundary / non-claims (Entry 4)

* **NO mineable determination.** 2,206 is a cycle-band candidate count under policy
  threshold T — **not** an eligible count, **not** a mineable count, **not** finalized,
  and **not** a zero. Entry 3's anticipated "finalized `MINEABLE-ELIGIBLE = N`" is
  explicitly deferred; `mineable_now` stays 0.
* **EXECUTE-mode observation only** — the census produced and consumed no proofs; the
  only proof remains Entry 3's single calibration proof. `blst`/`c-kzg` remain unswapped;
  the guest is crypto-fail-closed; all 2,292 executed rows committed
  `crypto_call_free = 0x01`.
* The aborted 8,192-slot case is counted **neither as a failure nor as mineable**; it
  awaits the chunked-execution family.
* The **integrated confirmed subtotal 10,674** and `mineable_now = 0` are **unchanged**;
  this record contributes no number and edits no other domain's count.
* This is an **issuable-problem-count investigation, not network activation**; no
  consensus / BF.7 / mining / reward / Base / paid-API change was made. Closed-local,
  non-consensus evidence only — not a public-network / leaderboard / paid-API / production claim.
* **Entry 1, Entry 2, Entry 3, and the preamble remain byte-unchanged.**
