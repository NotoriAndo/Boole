# Native submission shadow verification v1

Status: **AUTHORITATIVE DESIGN — tracked checker qualification landed; real ACCEPT parity landed (Entry 29); node path not yet landed**

Slice: **`NATIVE-SUBMISSION-SHADOW-ADMISSION-V1`**

Default: **OFF**

Consensus effect: **NONE**

## 1. Purpose

This specification closes the trust gap recorded by Entry 28 of
`docs/llm-mineable-eligibility-census-p1.md`.

The previous closed-local episode proved that a real LLM answer can pass family-specific intake,
an external frozen checker, miner-side binding and local receipt/accounting wiring. It did not
prove that the actual `boole-node` process can receive a raw answer and independently reach the
same semantic verdict.

This slice adds that missing node-owned shadow judgment. It does not activate mining, rewards,
blocks or consensus.

## 2. Trust rule

The node accepts a **raw submission**, never a miner-issued verdict or receipt as authority.

The verdict must be a node-owned pure decision over:

```
raw submitted answer
+ active task/challenge identity
+ tracked pinned family registry
+ tracked pinned checker and policy
+ pinned toolchain and deterministic resource policy
```

The miner is allowed to identify the task and provide its answer. It is not allowed to select the
checker, policy, anchor, toolchain, expected answer or verdict. The node derives those from its
own pinned registry and executes the actual checker itself.

## 3. Dedicated input contract

The dedicated route is:

```
POST /native-shadow/submissions
```

The payload schema is `boole.native-shadow.submission.v1`:

```json
{
  "schema": "boole.native-shadow.submission.v1",
  "familyVersion": "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1",
  "templateId": "<64 lowercase hex characters>",
  "challengeSha256": "<64 lowercase hex characters>",
  "epoch": 0,
  "rawAnswer": "<the complete untrusted model/miner response>"
}
```

All six fields are required. Unknown fields are rejected. In particular, the submission must not
carry an authoritative `verdict`, `receipt`, `checkerDigest`, `policyDigest`, `anchorDigest`,
expected answer or witness. The node computes the candidate digest over the exact UTF-8 bytes of
`rawAnswer` before family-specific extraction.

The existing endpoints are deliberately not reused:

* `/submit` is the existing PoW/share-admission contract and has a different identity, replay and
  accounting model.
* `/receipts` stores signed `boole.receipts.commit.v1` commitments. It is not a raw-work verifier
  and must not become one by accepting a native receipt-shaped payload.

## 4. Node-owned pinned registry

Before the route can be enabled even in shadow mode, the repository must contain a tracked,
byte-pinned registry sufficient for a clean node or CI runner to reproduce the judgment. For each
enabled family/template it binds at least:

* family and version;
* template identity and semantic locator;
* anchor bytes or an immutable tracked locator plus anchor digest;
* challenge/epoch policy and freshness rule;
* checker artifact and checker digest;
* policy digest;
* toolchain identity, binary provenance and invocation contract;
* proof-intake/extraction version;
* deterministic resource limits and containment limits; and
* allowed verdict/reason-code vocabulary.

Gitignored `local-docs` files, machine-global caches and handwritten digest constants are evidence
sources only. They are not runtime authority. The existing frozen checker, generator, fixtures and
toolchain inputs must first be migrated into a tracked fixture/registry surface. The migrated
surface must pass two distinct parity gates before the route is implemented:

* the tracked **actual checker** must reproduce the frozen real ACCEPT case and checker-owned
  negative controls with the same authority digests and normalized verdict/reason codes — **closed
  2026-08-21, see section 4.2**; and
* the node's binding and replay RED matrix must independently cover task, challenge, policy,
  registry and evidence misuse — **design frozen 2026-08-22, see
  `docs/node-native-shadow-binding-containment-design-v1.md`; operator review 2026-08-22 withheld
  approval and required six corrections, see
  `docs/node-native-shadow-binding-containment-design-v1-correction.md`; implementation still
  open and blocked until that correction itself is reviewed**.

Entry 27's `FixedVerdictChecker` reject matrix is miner-wiring evidence, not proof that the actual
checker produced those negative verdicts. Path strings, timing and telemetry need not be byte
identical; authority-bearing digests and normalized semantic outcomes must be identical.

### 4.1 Tracked qualification milestone

The first migration slice is now tracked at:

* `native/checker/rust-tuple-struct-project-v1/` — answer-free semantic checker, policy, release
  manifest and complete file digests;
* `fixtures/native-shadow/registry-v1.json` — strict qualification registry with activation
  explicitly disabled; and
* `fixtures/native-shadow/rust-tuple-struct-project-v1/` — synthetic, permanently non-issuable
  positive and negative fixtures.

`scripts/test_native_shadow_authority.py` proves from tracked files alone that the pinned checker
accepts the public positive fixture, rejects the negative controls, refuses a wrong toolchain and
detects uncoordinated registered file or digest drift. Clean CI installs and SHA-verifies the
official rust-lang per-commit artifacts for rustc `e7795af6d`; the workspace default remains Rust
1.95.0. A date-based nightly is deliberately not substituted because it resolves to a different
compiler commit.

This milestone deliberately copies no real mining answer, author witness, model transcript,
session record, census row or machine-specific compiler binary from the private experiment
archive. It also does **not**, by itself, satisfy the two route prerequisites above: at the time
this milestone landed, both the frozen real ACCEPT parity case and the node-owned binding/replay
matrix were open. A later, separate migration closed the first (see section 4.2). The registry
contains only one non-issuable fixture, no node loader or HTTP route consumes it, and
`activationAllowed` remains false.

The qualification release also makes no process-count containment claim. A clean Linux CI run
showed that `RLIMIT_NPROC` counts the shared user's existing processes and threads, so it can reject
a valid answer for reasons outside the task. That limit is removed rather than weakened or raised;
recognized process-exhaustion failures are reported as checker unavailability, and any future
activation must provide task-tree isolation with a dedicated cgroup or PID namespace. The Linux
address-space limit and the other frozen file, output, CPU and wall limits remain qualification
evidence only.

### 4.2 Real frozen-accept parity milestone (2026-08-21)

A second, independent migration slice closes the **first** of the two open route prerequisites
named in section 4: the frozen real ACCEPT case recorded by Entry 27/28 is now reproduced by the
tracked checker from Git-tracked files alone, with no dependency on the gitignored `local-docs`
experiment archive. It is tracked at:

* `fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/` — the real anchor, the real
  extracted historical candidate answer, three negative controls (empty/tampered/one-value
  mutation), task identity, provenance and a complete `SHA256SUMS`. The answer is permanently
  `nonIssuable` and `activationAllowed: false`; and
* `scripts/test_native_shadow_real_parity.py` — proves from tracked files alone, given
  `BOOLE_NATIVE_TOOLCHAIN_BIN`, that the tracked checker independently reaches ACCEPT on the real
  candidate and REJECT on every negative control and on both directions of cross-task binding,
  matching the frozen `FROZEN-PARITY.json` expectations by normalized verdict/reason code rather
  than raw string equality.

This closes `REAL-FROZEN-ACCEPT-PARITY-V1` (label: `REAL-FROZEN-ACCEPT-PARITY-GREEN`;
`docs/llm-mineable-eligibility-census-p1.md` Entry 29; PR #158; main `60814a9`). It does not
implement the `boole-node` route, an HTTP endpoint, or activation, and it does not change
`mineable_now`. The **second** prerequisite — the node's own binding and replay RED matrix — is
unaffected by this milestone and remains open.

## 5. Required decision path

For every submission, the node performs these stages in order:

1. Strictly decode and size-check the JSON payload.
2. Resolve `familyVersion` and `templateId` from the node-owned pinned registry.
3. Verify that `(challengeSha256, epoch)` is active, fresh and unused for that task.
4. Hash the exact raw answer and run the pinned family-specific intake.
5. Execute the actual pinned checker under the pinned toolchain and resource policy.
6. Convert the checker's deterministic result to a node-owned shadow verdict.
7. Atomically consume the challenge and persist node-issued shadow evidence.

Malformed input, unknown identity, stale/replayed challenge, registry drift, checker failure,
policy mismatch, resource-limit breach and semantic rejection are distinct typed outcomes. An OS
containment failure or unavailable checker is not converted to ACCEPT and is not silently treated
as a semantic REJECT; it stops the shadow adjudication with an availability/error result.

The checker executes untrusted submitted code, so the pinned execution policy must also require a
fresh temporary workspace, read-only checker/anchor/toolchain inputs, a sanitized environment with
no inherited secrets, disabled network access, a dedicated process group, bounded input/output and
file counts, and explicit CPU, memory, wall-clock and filesystem limits. It must never compile in
the repository, a shared target directory or a mutable shared dependency cache. Process-tree
termination and temporary-workspace cleanup are containment duties; they cannot change a semantic
verdict.

## 6. Node-issued shadow evidence

Success or deterministic rejection produces `boole.native-shadow.evidence.v1`, owned by the node.
It binds:

* submission schema and submission digest;
* family/version, template identity and anchor digest;
* challenge digest and epoch;
* exact raw-answer candidate digest;
* intake version;
* checker, policy and toolchain digests;
* deterministic verdict and reason code; and
* registry version.

An operational execution identifier and resource telemetry may accompany the evidence, but they
are not part of the deterministic verdict digest or any future BF.3 receipt mapping.

This object is **shadow evidence, not a consensus receipt and not a share**. It cannot alter
`SharePool`, block construction, rewards, peer state or `mineable_now`. Replaying an already-used
challenge, or presenting task-A submission/evidence as if it belonged to task B, must be rejected
before a second accepted evidence row can be written. Identical raw-answer bytes submitted afresh
for task B are not automatically invalid: the node must bind them to B and let B's actual checker
decide them independently.

## 7. Placement and dependency direction

The implementation lives in a new, isolated `boole-node` module and uses node-owned data types.

Hard dependency rules:

* `boole-node` must not depend on `boole-miner` to import `NativeReceipt`, a checker verdict or a
  task context.
* This slice must not modify `boole-core` admission, replay, hash or block-builder code.
* The native checker adapter may depend only on the smallest tracked checker/runtime surface
  needed to reproduce the judgment.
* The miner-side Entry 27 wiring remains historical evidence; it is not a node backend.

## 8. Activation and non-goals

Production activation is unavailable in v1. The route is default-OFF and, in this slice, reachable
only through loopback or an in-process test harness. A non-loopback bind is a startup/configuration
error. Remote miner transport, authentication, `network_id` binding and a signed submission
envelope belong to a separately approved BF.6 successor; they are deliberately not improvised in
this qualification slice.

This slice does not touch or authorize:

* existing PoW `/submit` admission;
* existing `/receipts` commitment storage;
* `boole-core`, `SharePool`, block or chain state;
* reward, Base, bounty settlement or accounting;
* P2P frames or propagation;
* SP1/ZK proving;
* BF.7 consensus activation; or
* a change to `mineable_now` (it remains 0).

## 9. RED gates and STOP conditions

Implementation must start with failing tests for at least:

1. a valid raw answer reaches the real checker and produces node-owned ACCEPT evidence;
2. a forged miner verdict/receipt cannot bypass checker execution;
3. a preregistered verdict-bearing raw-answer mutation changes the candidate digest and is rejected
   by the actual checker;
4. task-A submission/evidence replayed under task B and cross-challenge evidence reuse are rejected,
   while a fresh identical raw answer for B is independently adjudicated by B's checker;
5. stale/replayed challenge is rejected atomically;
6. checker, policy, anchor, registry or toolchain digest drift is rejected before execution;
7. unavailable/terminated checker never becomes ACCEPT;
8. unknown/oversized/malformed JSON and forbidden fields are rejected;
9. the feature is a byte-for-byte no-op while OFF; and
10. non-loopback exposure is refused; and
11. no accepted shadow verdict changes `SharePool`, block, reward, P2P or consensus state.

Stop without fallback if any of the following is true:

* the node cannot execute the actual checker independently;
* the only available input is a miner-created receipt or verdict;
* the frozen checker cannot be migrated to tracked inputs with digest and verdict parity;
* clean-runner toolchain or fixture reproduction fails;
* the v1 endpoint can bind or listen on a non-loopback address;
* the change requires `boole-core`, SharePool, block, reward, P2P or BF.7 modification; or
* OFF-mode behavior differs from the current node.

## 10. Relationship to BF receipts

`boole.native-shadow.evidence.v1` is a temporary qualification artifact, not a third permanent
receipt family. Before any production or BF.7 connection, a separate approved successor must map
the node-owned verdict into the **already-landed** BF.3 common `VerificationReceipt` contract and
prove the mapping preserves every binding field and reject reason required by that contract. This
is a native-adapter qualification and mapping task, not a reimplementation of BF.3 or BF.5.

The promotion rule is:

```
raw submission
  -> node-owned checker verdict
  -> native shadow evidence (this slice)
  -> native verdict mapping into the already-landed BF.3 VerificationReceipt type
  -> BF.6 remote miner commit/reveal (separate approval)
  -> BF.6a package sidecar and availability (separate approval)
  -> RP0-MD and deterministic-resource preconditions
  -> BF.7 consensus use (still HELD)
```

Once the BF.3 mapping is authoritative, shadow evidence may remain as an audit/debug record but
must not become a competing production receipt or an independently rewarded object.

## 11. Completion label

Only the full RED matrix plus one real node-process raw-answer run may earn:

```
NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN
```

That label means node-owned shadow verification works. It does not mean mining is public,
reward-bearing, peer-verified or consensus-active. It does not change
`LLM-MINEABLE-ELIGIBLE-V5 = 14,160` or `mineable_now = 0`.

## 12. Synchronized local planning mirrors

The detailed master, execution and thesis mirrors remain under gitignored `local-docs`; the
repository must not unignore that directory wholesale. Their synchronized 2026-08-21 byte
digests are recorded here so a later local edit cannot be mistaken for this reviewed state:

| local mirror | sha256 |
| --- | --- |
| `local-docs/adr/0021-native-submission-shadow-verification.md` | `49b7d2ee80c319ef1d5268855685092748d63e47c20a08d4b80f73cf1570745c` |
| `local-docs/todo/todo-l1-network-master.md` | `adcd7cc549ad80112ec727adf73b3b4fbea3bd546c0464be28b732e5ed771fc7` |
| `local-docs/todo/EXECUTION-ORDER.md` | `01243627ca10afa2bb1fdc1aa3b5f3ed320e3a9509f7afe8aecba7fc3a9bc570` (updated 2026-08-22c — correction-requested cursor sync below) |
| `local-docs/verified-reasoning-substrate-thesis-2026-06-10.md` | `255128d28961d760311680f1dfddeed01ad4f7c1509e0be7705aea6347b00f39` |
| `local-docs/todo/thesis-realization-roadmap.md` | `a0a25a0f51b39bd284f85b3a009655eaace9ca244b0edbd4c9f4e8c2d1a44f5c` |
| `local-docs/boole-thesis-value-up-verified-zk-encyclopedia-2026-07-21.md` | `d3a312acc59f73358d70820d5d5e4afd1dbec5f60bbe9e9d98e7c11f78b8b90a` |

These digests preserve synchronization evidence only. Runtime authority still requires the
tracked checker/registry migration in section 4; no node may load a `local-docs` file as a trust
root.

The 2026-08-22 update to `local-docs/todo/EXECUTION-ORDER.md` marks its execution-order step 0
complete and moves its current-position marker to node binding/replay RED-matrix design, matching
the section 4.2 closure above; it appends a new dated cursor block rather than editing the prior
one, consistent with that file's own append-only cursor convention. The other five rows remain at
their original 2026-08-21 synchronization point.

A second, same-day (2026-08-22b) update to `local-docs/todo/EXECUTION-ORDER.md` records that
`docs/node-native-shadow-binding-containment-design-v1.md` has frozen the design for the section 4
second prerequisite and moves the current-position marker to "awaiting approval of that design,"
again by appending a new dated cursor block rather than editing the prior one. The other five rows
remain at their original 2026-08-21 synchronization point.

A third, same-day (2026-08-22c) update to `local-docs/todo/EXECUTION-ORDER.md` records that
operator review of that design withheld approval and required six corrections, resolved in
`docs/node-native-shadow-binding-containment-design-v1-correction.md`, and moves the
current-position marker to "awaiting review of the correction document itself," again by appending
a new dated cursor block rather than editing the prior ones. The other five rows remain at their
original 2026-08-21 synchronization point.
