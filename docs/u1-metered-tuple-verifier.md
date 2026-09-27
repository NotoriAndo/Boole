# U1 prerequisite: non-activated metered tuple verifier

Status: implemented local verifier candidate with root-bound packages,
independent CAS re-verification and explicit offline lost-object restoration.
Adoption is gated by the owning PR's full required CI, including Linux/macOS ×
debug/release golden comparison and independent package processes.
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
refused. The root is an object and each field type is a string; positional-array
tasks and unit-enum object aliases are not alternate wire encodings. There is
no uploaded source anchor, arbitrary dependency, answer-supplied
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
| Operations / fuel | 100,000 / 150,000 |
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

Pre-merge maximum-arity review then found a separate policy-calibration defect:
the initial candidate's 100,000 fuel ceiling rejected the direct valid answer
for an admitted eight-field numeric task. The added public-consumer test failed
with `budget_exceeded:fuel`. Before adopting this new, non-activated contract,
the candidate ceiling was corrected to 150,000. Direct eight-numeric and
eight-boolean answers use 110,560 and 127,200 fuel, respectively, with 70,784
operations each. This provides bounded headroom for the advertised maximum
task shape, not acceptance of every possible correct implementation.
The original excessive-work input still rejects at the new ceiling; none of
the existing answer, parser, operation or source-limit controls was removed.
Only the five fixture policy digests change; task/answer/corpus/output digests,
verdicts and measured counters remain identical. The initial source `6b52ade`
passed its four-way corpus run `36284079396`, but that does not qualify this
correction: the updated source must pass fresh full required CI. The initial
full CI `36284079403` is retained as the superseded candidate run, not final
adoption evidence. No existing V1 or network resource policy was changed.

The subsequent input-boundary review reproduced two unintended serde aliases:
`{"u32": null}` as a field type and a top-level positional array as a task.
Both originally produced a verdict despite being outside the documented wire
schema. String-only field decoding and object-only root admission now refuse
them before producing evidence, while retaining typed duplicate-field rejection.
All existing canonical golden bytes, including the corrected policy digest,
remain unchanged. Intermediate source `d7cf622` passed its four-way corpus
`36284525397` and Linux x86_64 integration in full run `36284525472`; that run is
also superseded by the strict-input correction and is not final adoption evidence.

## Publication CI: native process fixture correction

At strict-input source `45a375f`, four-way corpus run `36285860518` and the actual
Linux x86_64/arm64 integration jobs in full run `36285860578` passed. Self-test
nevertheless failed the existing three-node native operator rehearsal's fixed
180-second total bound (181.62s including its 0.16s nested build). All preceding
functional, accounting, recovery and source-preservation assertions passed;
the 15-second phase and three-second stop bounds were not reached. The failed
full run is retained, not reported as final adoption evidence.

The unchanged fixture passed locally in 78.113s; a temporary per-command timing
probe measured 78.162s, with ordinary wallet operations around 1.17s and repeated
unlock/sign mining and transfer commands around 3.5s. It exposed two test-build
issues: the nested node/wallet build used the dev profile instead of the parent
test profile, bypassing the existing test-only curve arithmetic optimization;
and the default-strength Argon2 KDF ran unoptimized in both. This is measured
fixture cost, not proof of a particular hosted-runner slowdown or a runtime
consensus defect. An initial sandboxed local attempt could not bind loopback
and is not included among completed scenarios.

The fixture now builds its actual children with `--profile test --locked` and
checks Cargo's emitted library/binary profiles and the exact executable paths
before starting them. The real call site first rejected dev curve opt-level 0,
then rejected test Argon2 opt-level 0. Only the Argon2 test dependency receives
a new opt-level 3 override, retaining debug assertions and overflow checks;
application libraries/binaries remain at opt-level 0. The workspace profile
consumer also checks Argon2, including missing-artifact and disabled-guard
negative controls. Ordinary dev/release settings and dependency versions are
unchanged.

The same measured local scenario then passed in 10.457s; wallet backup/restore
took 79/73ms and mining commands 221–391ms. Its original signature, state,
accounting, replay, recovery and 15s/3s/180s checks remain intact.
The fixture additionally asserts the actual vault's unchanged Argon2id
65,536KiB/time-cost 3/parallelism 1 contract. No unlock is cached, no signature
is skipped and no weak test vault is substituted. Temporary timing probes are
removed before publication; the clean final fixture passed in 9.652s and the
adjacent native CLI test passed in 11.46s including its nested build.
The 13 focused vault tests, 19 actual wallet-agent tests and 37 Python
profile/self-test/workspace contract tests also pass, including wrong-password,
tamper, signature, backup/restore and fail-closed artifact controls.
This test-only correction requires fresh full CI;
earlier production/large-state measurements keep their original build settings.

## Root-bound local packages and independent re-verification

The local consumer in `boole-node::metered_tuple_package` uses the existing
BF.6a canonical sidecar and content-addressed store (CAS). It does not change
their schema, root domain, P2P frames, receipt rules or default-OFF behavior.
Exactly three files are admitted, with no extraction to the filesystem:

| File | Bound content |
|---|---|
| `task.json` | Exact supplied task bytes; the verifier still derives its semantic task identity from typed canonical JSON. |
| `answer.rs` | Exact restricted answer-body bytes, never a compiler/executable selection. |
| `verifier.json` | Byte-exact locally compiled contract: adapter/schema identifiers, policy digest, all deterministic limits, implementation identity and fixed non-issuable flags. |

The new application envelope is at most 16KiB. Maximum 4KiB task + 8KiB answer
inputs fit, including framing/contract overhead. Generic CAS reads retain their
existing 8MiB ceiling; the smaller application cap is checked before decoding a
received package. Extra/missing files, noncanonical framing, root substitution,
incompatible contracts, invalid tasks and oversized inputs produce no answer
verdict. A complete, compatible package containing a wrong answer instead
reproduces the actual `deterministic_reject` result. Producer-claimed results
are not an input, and a successful CAS read is never a verification cache.

`implementationDigest` is a length-framed SHA-256 over the interpreter and tuple
verifier source, crate/workspace Cargo manifests, workspace lockfile and pinned
Rust toolchain declaration. It identifies **source and declared build inputs**,
not the executing binary or actual compiler/linker/runtime environment. Source
or lockfile changes intentionally require a new compatible package. This is
not release signing, an executable measurement, proof of compiler equivalence
or the production executable pin required by U1. The locally trusted binary
remains an explicit premise; package bytes cannot supply a replacement for it.

### Offline command interface

Build with `cargo build --locked -p boole-node --bin boole-metered-tuple-package`.
All store commands require exclusive, **stopped-node** ownership of the CAS
directory. The inherited store does not arbitrate simultaneous processes; do
not run these commands alongside a fetch worker or another store owner.

```sh
# Choose a new output filename. pack writes binary bytes to stdout and reports
# packageRoot on stderr; replace EXPECTED_ROOT_HEX below with the retained root.
set -C
target/debug/boole-metered-tuple-package pack \
  fixtures/native-metered-tuple-v1/task.json \
  fixtures/native-metered-tuple-v1/answer.rs > package.bin
target/debug/boole-metered-tuple-package import offline-cas EXPECTED_ROOT_HEX package.bin
target/debug/boole-metered-tuple-package verify offline-cas EXPECTED_ROOT_HEX

# Only for a previously staged object that is now absent, with the original
# pending snapshot retained and all other store owners stopped:
target/debug/boole-metered-tuple-package restore offline-cas EXPECTED_ROOT_HEX package.bin
target/debug/boole-metered-tuple-package verify offline-cas EXPECTED_ROOT_HEX
```

For received material, the expected root must come from an independently trusted
context. Hashing the supplied file itself or trusting its URL does not establish
authority. `pack` output is developer metadata, not a signature or receipt.
No command starts a network listener, compiler, model or VM.

`verify` exits 0/1 only with accepted/rejected verification evidence; its stable
JSON wrapper binds `packageRoot` to the unchanged canonical verifier result.
Exit 2 has no verification result: `retryable_unavailable`, `input_error` or
`package_error`. `pack`, `import` and `restore` exit 0 for completion of their
own operation, **not answer acceptance**. They also admit correctly bound wrong
answers so an independent consumer can reproduce the rejection. Input reads
are capped, refuse symlinks/non-regular files and open nonblocking before the
regular-file check, including FIFOs.

### Missing-data and restoration boundaries

For a not-yet-received root, `reverify_stored` durably registers an idempotent
`metered-tuple:<root>` request before returning unavailable. The existing bounded
fetch queue can reload it after restart without the caller supplying the root
again. It accepts only canonical bytes matching the requested root; availability
completion means bytes were durably staged, not that the answer was accepted.
The existing count/byte caps and reject-newest policy remain (default 64 pending
references, 64MiB aggregate pending bytes, at most 64 unresolved fetch intents).
Queue/persistence failures are errors, never a false claim that retry is durable.
The offline CLI records requests but does not start a fetch worker; explicit
`import` can also satisfy a request.

For **already staged bytes deleted from disk**, ordinary store open still fails
closed. Explicit offline restoration revalidates all snapshots and present
objects, requires the exact root and size already recorded in the pending
snapshot, and republishes only the missing canonical object through the existing
atomic/fsync path. It never rewrites references or clears fetch intents. Multiple
missing objects can be restored one at a time; normal open remains refused until
all required objects are available. An existing corrupt object, symlink or
corrupt snapshot is preserved and refused, not overwritten. An uncertain durable
commit remains an error, not a verdict. Verification retains pending packages;
it does not acknowledge/delete the evidence after one successful result.

The closed-loopback integration test stops a node after an unavailable response,
reloads the original durable request, rejects a wrong-root payload, then receives
the correct bytes and starts a fresh CLI process for actual re-verification.
This reuses the legacy BF.6a test transport, **not** the separate native R1 pinned
TLS service or public P2P. No live node/consensus intake is activated by this module.

Two separate local stores and four fresh verification processes reproduce the
independent golden verdict. Direct tests cover policy/source-identity override,
extra claimed verdicts, missing/oversized input, wrong answers, queue pressure,
restart, exact offline restoration, shared references and preservation of corrupt
files/symlinks. These package/CLI cases are also wired into the required
Linux/macOS × debug/release matrix. The initial API/CLI and restoration tests
failed before implementation; the first network invocation was denied by the
local sandbox at loopback bind, before traffic, and the permitted closed-local
invocation passed. This is local engineering evidence, not independent-operator
qualification, real source supply, full useful-adapter DA qualification or BF.7.

## Remaining U1/U2 gates

This closes the local generated-task-to-meter-to-verdict and package recovery
seams, not the full useful-adapter resource qualification or BF.7 entry gate.
Before any new-rule receipt consensus:

1. Select and qualify the actual useful adapter, pin its executable/rule/resource
   contract and prove source/spec fidelity; generated tasks cannot stand in for
   real licensed source supply. The active V1 RP0-MD record is still HOLD: zero
   reward-ready stock, 197 candidate upper bound, unmeasured annual flow against
   the frozen 10,950-stock / 3,650-annual minimum alternatives. The present new
   adapter cannot inherit even those 197 candidates.
2. Qualify that adapter's complete verifier-effective package through BF.6a CAS/
   sidecar retrieval: hash/size/tamper checks, independent byte reconstruction,
   missing-data retry and bounded pending recovery. The generated local candidate
   above exercises these mechanisms but cannot qualify a different useful adapter
   or stand in for independent-operator evidence. No HTTP URL is a truth root.
3. Land the separate new network/rule/schema/hash/replay boundary with
   `no_protocol_reward`, one shared verification contract on producer/admission/
   ingest/replay/reorg, unchanged hash-only progress when useful work is empty,
   and missing data unavailable rather than permanent invalidity.

U2 still requires preregistered economic/supply/solve/attack/demand experiments,
at least four nodes and heterogeneous operators, plus an evidence-informed
Economic ADR. No model allowance was spent, buyer/LOI count was re-measured,
public operation authorized or additional useful-work reward enabled here.
