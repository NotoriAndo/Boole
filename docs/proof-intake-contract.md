# Boole Proof Intake Contract v1

Boole does not tune the proof path per model. Every model, CLI agent, and future bridge must submit through the same proof intake contract before verifier/share/block handling.

## Goal

The contract preserves benchmark and mining integrity:

- same problem
- same prompt family
- same helper manifest
- same proof-body contract
- same canonicalizer
- same verifier
- same retry/repair policy for the selected loop class
- no model-specific overrides

A model earns credit only for producing a proof candidate that passes the shared Boole intake and verifier path.

## Versions

Current constants are emitted by `boole-miner` and recorded in runtime NDJSON `llm_outcome` events:

- proof body contract: `boole-proof-body-v1`
- canonicalizer: `boole-proof-canonicalizer-v1`
- model-specific overrides: `false`

Future public-scoring or public-mining evidence must pin these versions, plus the family manifest/helper/verifier hashes used for the run.

## Transport envelope

The proof candidate is taken only from the declared answer channel.

Allowed transport shape:

```text
answer: proof body candidate
stdout/logs/stderr/telemetry: diagnostics only, never proof body
```

If an integration cannot distinguish the answer channel from process stdout yet, that path is treated as a legacy plain-answer envelope and must still pass the same contract classifier. Runtime logs, warnings, stderr, shell banners, gateway notices, and telemetry must not be mixed into the answer string.

## Global proof submission contract

The candidate is a Lean theorem-body expression for a Boole-generated theorem, not a standalone Lean file. This global contract is stable across families; family manifests may describe verifier environment and allowed helper APIs, but Boole must not add per-instance solution hints.

Slot-level rules:

- the answer is inserted literally after the theorem's `:=`
- return one Lean theorem-body expression only
- tactic scripts must be inside a top-level `by` block
- full Lean files, declarations, Markdown, prose, `sorry`, and `admit` are rejected

Allowed shape examples:

```lean
by
  ...
```

```lean
fun xs =>
  ...
```

Rejected examples:

```lean
import Mathlib
```

```lean
theorem instance_thm : True := by trivial
```

```text
Here is the proof: by trivial
```

```lean
by
  sorry
```

Contract failures are classified before Lean verification as `contract_failed` when the candidate is clearly not a proof body.

## Family manifest boundary

A family prompt may expose only stable verifier context, such as family id/version, goal shape, opened helper module, and allowed helper API names/types. It must not include per-instance solution strategy, helper application order, canonical proof templates, or model-specific repair advice.

The concrete rendered theorem statement is still the work item. It should not be accompanied by instance-specific proof hints.

## Boundary order

The mining loop enforces this order:

```text
driver.generate → ProofIntakeV1 → Canonicalizer → Verifier
```

Driver adapters return raw answer-channel text. They do not produce a `proof_source`, unwrap fences, classify theorem/prose shapes, or repair candidate Lean. `ProofIntakeV1` is the only boundary that turns an answer into a `ProofCandidate`.

## Report terminology

Runtime reports must not collapse answer generation, proof intake, verifier success, share acceptance, or block production into one "solved" counter.

Agent/runtime counters use transport and intake terms:

- `driverCalls`: driver invocations attempted
- `driverAnswered`: driver returned a non-empty answer channel
- `driverRejected`: driver had no usable answer channel
- `driverErrored`: transport/process/API failure
- `proofIntakeAccepted`: shared ProofIntake accepted the answer as a proof candidate
- `proofIntakeRejected`: shared ProofIntake rejected the answered candidate before canonicalizer/verifier

Protocol counters remain separate, for example `verifyAccepted`, `sharesAccepted`, and block-production evidence. `driverAnswered` is not a public mining or proof success claim.

Tracked artifact fixtures freeze this report contract:

- `fixtures/protocol/mining-report/v1-summary.json` fixes the nested `agent`/`protocol` report plus flat stdout mirror.
- `fixtures/protocol/mining-report/v1-llm-outcomes.json` fixes `llm_outcome` event wording for `answered` and `intake_rejected`.

These fixtures are local controlled-smoke artifacts. They are not public mining, paid/API benchmark, or model leaderboard evidence.

## ProofIntake syntax-envelope scope

`ProofIntakeV1` may do shared syntax-envelope normalization before verifier/canonicalizer admission. It must not change proof meaning.

Allowed:

- trim surrounding whitespace
- unwrap a Lean/Lean4/unlabeled fenced block
- preserve the proof text exactly after envelope removal

Forbidden:

- model-specific regex cleanup
- theorem statement repair
- helper-name correction
- tactic rewriting
- inserting missing introductions or terms
- changing `rw`, `exact`, `apply`, theorem names, or proof structure

Semantic repair belongs only in a separately labeled repair-enabled benchmark, not first-pass strict benchmark evidence.

## Fairness rule

Model adapters may perform transport only. They may not implement model-specific proof cleanup, model-specific prompts, model-specific helper sets, model-specific retries, or parsing exceptions.

If a future run uses any model-specific override, it must be marked ineligible for public scoring/mining evidence.
