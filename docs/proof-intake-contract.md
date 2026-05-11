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

## Proof body contract

The candidate is a Lean proof body for a Boole-generated theorem, not a standalone Lean file.

Allowed examples:

```lean
by
  intro xs
  exact length_dedup_le _
```

```lean
```
by
  trivial
```
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

## Canonicalizer scope

The canonicalizer is syntax-envelope normalization only. It may trim whitespace and unwrap a whole Lean fenced block. It must not change proof meaning.

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
