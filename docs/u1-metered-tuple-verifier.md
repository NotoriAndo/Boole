# U1 prerequisite: non-activated metered tuple verifier

Status: implemented local verifier candidate; adoption is gated by its owning
PR's full required CI, including Linux/macOS × debug/release golden comparison.
**BF.7 remains HOLD.** This does not activate a registry, receipt, block, reward,
public network or the existing V1 checker.

The September 27 U-first development choice advances the deterministic-resource
prerequisite without making all of R1 a dependency. The existing interpreter had
bounded parsing/evaluation but no task-owning verifier consumer. The new
`BOOLE-METERED-GENERATED-TUPLE-V1` consumer owns its task semantics, fixed budgets,
input generation, answer comparison and canonical result bytes.

## Run and interpret

```sh
cargo run --locked -p boole-native-rust-meter --bin boole-metered-tuple-check -- \
  fixtures/native-metered-tuple-v1/task.json \
  fixtures/native-metered-tuple-v1/answer.rs
```

The first argument is a typed task JSON file; the second contains only the
restricted answer body. No compiler, shell, model, VM, network or service is
started. The supplied file paths select local input bytes, not executable code
or runtime policy. The supported CLI platforms are Linux and macOS.

- Exit 0: `accepted` under the fixed 64-prefix test contract.
- Exit 1: `deterministic_reject`, such as wrong output, forbidden language,
  invalid UTF-8 or deterministic resource exhaustion.
- Exit 2: no verification evidence. Missing/unreadable/non-regular inputs are
  `retryable_unavailable`; malformed task/oversize input/usage is `input_error`.
  Neither is transformed into an answer or consensus verdict.

Reads retain at most the applicable cap plus one detection byte. Unix opens
refuse symlinks and use nonblocking descriptors before requiring regular files,
so a FIFO replacement cannot hang the read. An oversized API input is refused
before hashing; no truncated-input digest is presented as evidence for the full
input. A host crash, I/O failure or process kill is not a deterministic rejection.

## New task and answer contract

The schema `boole.metered-tuple-task.v1` accepts one to eight fields from
`u8/u16/u32/u64/i8/i16/i32/i64/bool`, a lower-case 64-hex task seed, and
`a0`, `mul`, matching `coeffs` in ±1,000,000. Unknown/duplicate fields,
floating-point or boolean coefficients, unsupported types and trailing JSON are
refused. There is no uploaded source anchor, arbitrary dependency, answer-supplied
budget or legacy task-schema fallback.

For each item, `projection` is the wrapping-i64 sum of field values multiplied
by coefficients, and `acc = acc * mul + projection` uses wrapping-i64 arithmetic.
Booleans map to 0/1. Only the existing [restricted answer language](../crates/boole-native-rust-meter/README.md)
is interpreted. Numeric fields require an immediate `as i64`; boolean values
require an explicit `if`. Comments, macros, arbitrary calls, unsafe code,
recursion and unbounded/nested loops are not in the language.

The canonical typed task JSON, including all semantic constants and the seed,
is domain-hashed with length-framed components. JSON key order/whitespace do not
change task identity. The exact answer body has its own SHA-256. Both identities
seed domain-separated SplitMix64; 64 tuples use unsigned values 0–60, signed
values −60–60 and booleans 0/1. The answer runs on all nonempty prefixes and must
match all 64 independently calculated recurrence results. Cases are reproducible
and publicly derivable, not secret or evidence of pre-solve resistance.

This is behavioral testing over a finite corpus, **not a proof for all inputs**.
It is intentionally a separate, more restricted adapter from
`RUST-TUPLE-STRUCT-PROJECT-V1`: no compiler semantic-equivalence or active-family
supply claim follows. Existing v3/schema/hash/replay/genesis and V1 release,
checker, task, receipt and runtime authority bytes are unchanged.

## Fixed resource policy and output binding

| Deterministic bound | Value |
|---|---:|
| Task specification bytes | 4,096 |
| Answer bytes | 8,192 |
| Tokens / AST nodes / AST depth | 512 / 256 / 32 |
| Operations / fuel | 100,000 / 100,000 |
| Prefix item visits | 2,080 (`1 + … + 64`) |

The adapter passes the same fixed limits into parsing and evaluation. Source,
task JSON and environment cannot override them. Input generation/type validation
are separately bounded by the fixed field/case/source/AST limits; the operation
and fuel counters count the interpreter as defined in its README, not total
machine instructions, CPU time or RSS. These are candidate engineering values,
not calibrated production capacity or network economic parameters.

`boole.metered-tuple-verification.v1` binds adapter/policy/task/answer identity,
the generated input+expected-output digest, actual-output digest, verdict/reason
and canonical resource-counter bytes. Completed evaluation, including a wrong
answer, reports all counters. A parse/runtime-budget rejection has no complete
counter result (`null`), never fabricated zero counters. A canonical result is
unsigned local evidence, not a signed receipt, checker-binary attestation or
permission to settle/activate anything. `nonIssuable=true` and
`activationAllowed=false` are fixed by the verifier.

The checked-in [synthetic corpus](../fixtures/native-metered-tuple-v1/README.md)
has explicit origin/license/regeneration instructions and an independent Python
integer/hash oracle. Rust checks exact golden bytes on all four CI combinations.
Four fresh local CLI processes also return identical bytes despite differing
irrelevant environment values. This is not independent-operator network evidence.
The contract tests cover wrong answers/task substitution, strict input schemas,
budget/forbidden-language rejection, all nine field types, wrapping overflow,
input size, missing files, symlinks, FIFOs and no host-policy override.

Development REDs established the missing verifier/CLI consumer and missing matrix
wiring; an oversize-input regression also failed before early refusal was added.
The first all-field test attempted a direct boolean cast and was rejected by the
existing language. Its fixture now uses the already-supported explicit boolean
branch; the language and budget were not widened to obtain a pass.

## Remaining U1/U2 gates

This closes a local task-to-meter-to-verdict seam, not the full adapter resource
qualification or BF.7 entry gate. Before any new-rule receipt consensus:

1. Select and qualify the actual useful adapter, pin its executable/rule/resource
   contract and prove source/spec fidelity; generated tasks cannot stand in for
   real licensed source supply. The active V1 RP0-MD record is still HOLD: zero
   reward-ready stock, 197 candidate upper bound, unmeasured annual flow against
   the frozen 10,950-stock / 3,650-annual minimum alternatives. The present new
   adapter cannot inherit even those 197 candidates.
2. Qualify that adapter's complete verifier-effective package through BF.6a CAS/
   sidecar retrieval: hash/size/tamper checks, independent byte reconstruction,
   missing-data retry and bounded pending recovery. No HTTP URL is a truth root.
3. Land the separate new network/rule/schema/hash/replay boundary with
   `no_protocol_reward`, one shared verification contract on producer/admission/
   ingest/replay/reorg, unchanged hash-only progress when useful work is empty,
   and missing data unavailable rather than permanent invalidity.

U2 still requires preregistered economic/supply/solve/attack/demand experiments,
at least four nodes and heterogeneous operators, plus an evidence-informed
Economic ADR. No model allowance was spent, buyer/LOI count was re-measured,
public operation authorized or additional useful-work reward enabled here.
