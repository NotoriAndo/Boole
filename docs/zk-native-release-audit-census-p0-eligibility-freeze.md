# zk-native release-audit census P0 — eligibility freeze (v1)

Ceiling labels:
- **ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0**
- **LEAN-P0 = CORPUS-NOT-MATERIALIZED**

This is an **issuable problem count, not network activation**. `mineable_now`
stays **0**; this record wires **no** consensus / BF.7 path. It is closed-local,
non-consensus evidence and is **not a public-network / leaderboard / paid-API /
production claim**.

A **0 here does not mean the domain is permanently un-mineable.** It means that the
**current frozen P0 corpus** of 16,763 zk-native release-audit anchors contains **0**
anchors that materialize into an issuable, offline-checkable, non-trivial task under
the frozen inputs. Making any anchor eligible would require designing a **new** audit
statement, author oracle / deterministic checker, and resource budget — explicitly
**out of this P0 scope**.

**This is not a Lean-theorem count.** The 16,763 anchors are zk-native **source
components** (`.rs .circom .cairo .go .sol .nr .move .zok`) drawn from 54 zk
repositories; they contain **0 `.lean` files and 0 Lean theorem tasks**. This record
therefore does **not** claim a Lean problem count of 0. The separate Lean domain is
recorded as `LEAN-P0 = CORPUS-NOT-MATERIALIZED` — no materialized Lean-theorem P0
corpus exists yet, so no number is asserted for it.

This document is an **append-only attestation ledger**. The anchor ledger, the source
snapshots, the emitter, and the gate results **stay in the git-ignored sandbox**
(`local-docs/replenishment-p0-2026-07-22/` and
`local-docs/rp-a2-strict-p0-2026-07-26/`); only content hashes, lineage, and the
conservation identity are tracked here so the result survives outside the sandbox.
Future waves append new dated entries below; existing **merged** entries are never
rewritten.

---

## Entry 1 — 2026-08-10 · zk-native release-audit P0 / N = 0

### Result in one line

All 16,763 frozen zk-native release-audit anchors are `NEEDS-SPEC`: each has a genuine
binding to a real changed source component, but **none** has a fixed audit statement, an
author oracle / deterministic checker, a deterministic resource budget, or a
generality / non-vacuity basis. Zero anchors are issuable. Final
**ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0**.

### 1. Input invariance (frozen; matches prior record)

| frozen input | value / sha256 |
| --- | --- |
| audit-pool anchor count | **16,763** (of 17,548 gross candidates; 785 failed `not_deprecated`) |
| observation window (prior freeze) | `observation-window-rp2-4.json` sha256 `fab8439a4fa9b5062cebf931e155d68cc469661e3f7e2c9556b7bd07c7792bc6` |
| anchor / gate ledger | `gate-results.json` sha256 `0dd847fb98ca9b68dda3fc93640b96c110a6506e6adef5506a801e98590e8522` |
| gross source-binding ledger | `gross-candidates.json` sha256 `fd584ec0bf9d83bdd3e69a23620593ef0b8659f48c2b3d2344337629e3473cc2` |
| source universe (54 repos) | `source-universe.json` sha256 `54aa8fb166cc7732a130f68ff7878866ee02252858d2ec7c99ed360d1ef82acc` |
| release transitions (183) | `canonical-transitions.json` sha256 `14abf11c5a751fdd22bbf69b01c24a8a49356b1ec4819658d5f102e07c1ddf5f` |
| snapshot archive manifest (199) | `archive-manifest.json` sha256 `e9ea9b54d75909bbcd362b17e3972141a8c9fb94fa16d2ac5797b04130ddb89d` |
| window-end presence | `window-end-presence.json` sha256 `a1c5d620b08e6376bbf583f55f8fb04a73496651708fc390ecc2f01d27c09809` |
| checkpoint (RP.4) | `checkpoint-rp4.md` sha256 `9361be5130b42d89e96896f93b99023c93225e3b12d91c8a9d6921c7d98448c4` |
| emitter | `extract_release_tasks.py` sha256 `19058c2827877c5c20bbe607b7e4766aa812494ac26bff79354a8b2b4d13894b` |

The audit-pool count **16,763** and the observation-window digest **`fab8439a…`** are
**byte-identical to the prior frozen record** (recomputed here with `shasum -a 256`);
the STOP conditions (count mismatch / input-digest mismatch / conservation mismatch)
are **not** triggered. Source root: 54 repositories (proof-system 27, zkvm 9, circuit
9, tooling 9); each anchor is bound to a real `repo_id · source_path ·
previous_commit · target_commit · content-digest` transition inside the 12-month
release window; Lean checker toolchain pin (for the *separate* mining family, §5) is
`leanprover/lean4:v4.29.1`.

### 2. anchor→source binding is genuine (not a fake seed)

The emitter (`extract_release_tasks.py::derive_tasks`) builds each anchor's identity as
a pure hash of the **real** source identity — a file that actually changed between two
tagged releases — never from an opaque random seed. `task_kind = "AuditExisting"`
("audit this actual changed source component"), and the property is read from a frozen
enum by file extension, not invented. Re-running over the same trees is byte-identical;
recorded `task_id` collisions = 0.

Concrete, cross-verified example (anchor row 0):

```
task_id                05a3eeffbf7b0bb8d3263e877b30949aec9058cb27013f67e867b7bfebe8bac6
spec_id                boole.zk-native.release-audit.v1     variant_id  zk-native
repo_id                0xMiden/miden-vm
source_path            processor/src/host/advice/inputs.rs
property_id            functional_correctness   task_kind  AuditExisting
input_artifact_digest  2ea93dbdddb9fc21da2644866966aae72cd1ed7924b5bb25c091c23c4f226073
source_binding_digest  e75671b0566043e6260ecc3577d0fc4a307b61d5be8639738d762435f27cf0fe
target_release_digest  0e8fbe0adc945a7a5cdff11647b9f5b8a5b5e979e1ca55a594aa0dacf8b56717
```

The same `task_id` and the same `input`/`target` artifact digests appear in the anchor
gate ledger (`gate-results.json` row 0, `artifact_binding.evidence.input = 2ea93dbd…`,
`.target = 0e8fbe0a…`), proving the binding is traceable end-to-end. So the
`anchor→task` link is a real semantic binding; the reason the count is 0 is **not** a
broken/fake binding — it is the absence of a verification contract (§3).

### 3. Basis for N = 0 (each anchor's missing preconditions)

The gate ledger's own status distribution, recomputed here over all 16,763 audit-pool
anchors, shows **four** preconditions uniformly unmet (evidence strings quoted verbatim
from the ledger):

| precondition | gate | status over 16,763 | ledger evidence |
| --- | --- | --- | --- |
| fixed audit statement / spec | `spec_fixed` | **pending 16,763 / 16,763** | (no fixed spec authored) |
| author oracle / deterministic checker | — | **absent for all** | no checker binds to this family (§5) |
| deterministic resource budget | `deterministic_budget` | **pending 16,763 / 16,763** | "no adapter budget attestation yet" |
| generality / non-vacuity basis | `generic_theorem_exclusion` | **pending 16,763 / 16,763** | "semantic generality is audit work, not prefilter work" |

The gates that **do** pass (`artifact_binding`, `license`, `prior_proof_replay`,
`not_deprecated`) only establish that the anchor is a real, licensed, non-replayed,
still-present source change — i.e. a genuine binding, not a checkable task. `eligible`
(all seven gates pass) = **0**.

### 4. Conservation (FROZEN)

Primary bucket = the **first** unmet precondition only (no double-counting):

```
16,763 =     0 MINEABLE-ELIGIBLE
        + 16,763 NEEDS-SPEC
```

The other two unmet conditions are recorded as **auxiliary states of the same 16,763
rows**, NOT added into the denominator:

```
auxiliary (not summed):  NO-DETERMINISTIC-BUDGET = 16,763   (all rows)
auxiliary (not summed):  GENERALITY-UNRESOLVED   = 16,763   (all rows)
```

Each anchor occupies exactly one primary bucket (`NEEDS-SPEC`); the three missing
conditions are three facets of that single state, not three separate denominators.

Gross funnel context (outside the 16,763 P0 denominator, recorded for lineage only):
gross changed-existing candidates 17,548 = 16,763 audit pool + 785 `not_deprecated`
failures (component absent at window end); separately quarantined and **not** in the
audit pool: buildnew 3,825 + retired 2,150 + rename-only 148 + unchanged 112,337.

### 5. Why the existing Lean checker is not applied

The in-tree Lean checker (`lean/checker/`, toolchain `leanprover/lean4:v4.29.1`, release
manifest sha256 `6231366594fac2e1a46b5df9eec6255e4144ccf262428588d102b692c5056b35`,
helper surface `Boole/Family/V0Helpers.lean` sha256
`fb67fbf975a196ad38306fa5d659839316595c0b43640e55ccb6fcf80366f6ec`, family manifest
`fixtures/protocol/manifests/v1.json` sha256
`e7739e73e38ced397da73bfe7c8a5f3682dc4aeb275d3d9130574ada91b788bb`) verifies the
**`v1-lenbound` Lean proof family** (list filter/dedup/sort length-bound lemmas for the
`smart-contract-invariant-v01` family). That checker has **no binding** to a zk-native
source-audit anchor: there is no path from "an `.rs`/`.circom`/`.cairo` file changed
between two releases" to a `v1-lenbound` Lean theorem. Attaching it would be a false
binding; it is therefore **not** applied to these anchors. (Because the corpus contains
0 Lean theorem tasks, the Lean domain stays `LEAN-P0 = CORPUS-NOT-MATERIALIZED` — no
Lean number is asserted.)

The Lean-specific triviality battery (`rfl`, `simp`, `simpa`, `decide`, `native_decide`,
`aesop`, fixed trivial proofs) is likewise **inapplicable**: it targets Lean theorem
goals, and this corpus has none. No triviality pass was run because there is no
generated Lean task to run it against; this is recorded, not silently skipped.

### 6. Boundary / non-claims

* **ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0** is the issuable count under this
  frozen 16,763-anchor corpus + existing checker / manifest / helper / resource-policy
  (no paid-model or human spec authoring) — **not** a claim that the domain is
  permanently un-mineable. Reaching N > 0 requires designing a **new** audit statement,
  author oracle / deterministic offline checker, and deterministic resource budget for
  the source-audit family; that design is **out of this P0 scope**.
* **LEAN-P0 = CORPUS-NOT-MATERIALIZED**: no materialized Lean-theorem P0 corpus exists
  yet; no Lean problem count (including 0) is asserted. The forbidden phrasing
  "ZK-NATIVE/LEAN-P0 = 0" is **not** used — the two are recorded separately.
* No consensus / BF.7 / mining / reward / Base / paid-API / public-mining change was
  made. `mineable_now` stays 0. Closed-local, non-consensus.
* The anchor ledger, source snapshots (6.54 GB of archives), emitter, and gate results
  live only in the git-ignored sandbox; this record carries hashes, lineage, and the
  conservation identity so the freeze is durable in-tree.
* This record is **not a public-network / leaderboard / paid-API / production claim**.

### 7. Cross-domain running subtotal (issuable P0 counts confirmed so far)

```
EVM P0                       6,767   (docs/evm-census-p0-eligibility-freeze.md)
Solidity P0                      0   (docs/solidity-census-p0-eligibility-freeze.md)
zk-native release-audit P0       0   (this record)
------------------------------------
confirmed numeric subtotal   6,767
```

Lean is **not** summed: it is `CORPUS-NOT-MATERIALIZED`, so it contributes no number
(not even 0) to the subtotal.
