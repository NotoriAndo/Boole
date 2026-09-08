# Development tuple-task admission

Status: implemented development path; real Linux integration is checked by the
containing PR's protected CI. No additional model session or public activation.

This extends the [completed direct-client canary](boole-direct-model-canary-2026-09-08.md)
to separately admitted problems in the existing
`TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1` family. The historical replay
grant, its task, and the historical fresh-answer canary remain unchanged.

## Scope and public input

The default-disabled `development-task-admission` feature adds the explicit
Linux-only binaries `boole-development-task-node` and
`boole-development-task-launcher`. Neither takes command-line configuration or
has a host-execution fallback. The existing MCP `boole.verify_native` submission
shape is unchanged. Problems are admitted out of band by an operator, never by
the model or an HTTP request.

The first profile accepts a bounded generated tuple-projection specification:

```json
{
  "typeName": "DevelopmentPoint",
  "fieldTypes": ["i32", "bool"],
  "taskSeed": "3333333333333333333333333333333333333333333333333333333333333333",
  "a0": 17,
  "mul": 3,
  "coeffs": [2, -5]
}
```

There are one to eight fields, each `bool` or an 8/16/32/64-bit signed or unsigned
integer. Names are bounded Rust-safe uppercase identifiers; coefficients,
initial accumulator and multiplier are integers in ±1,000,000. No arbitrary
anchor source, uploaded executable, dependency, path, compiler option or resource
policy is accepted. The operator tool generates the anchor and scaffold; the
fixed checker validates submitted patches and hidden-test behavior.

The recurrence starts at `a0`, then for each item computes the wrapping-i64 sum
of each field's integer value times its coefficient, and sets
`acc = acc * mul + projection` with wrapping-i64 arithmetic. Boolean fields map
to 0 or 1. The caller's seed and **all** semantic specification fields determine
the effective checker seed and challenge; changing constants with the same input
seed cannot retain the same public problem identity.

The BF.3 `taskId` remains the existing stable family/template/anchor identity.
Challenge instances of the same template are distinguished by `submissionId`
and `artifactRoot`, not by redefining the receipt's stable task identity.

## Authority and installation

Build the relevant packages with their `development-task-admission` features
(and `linux-arm64-authority` for the ARM64 installation). The offline operator
tool uses an existing private, singly-linked, operator-owned 32-byte development
seed file with mode 0600 and a precreated mode-0700 output directory:

```text
boole-canary-authority create-task-grant KEY SPEC_JSON RUN_ID JOURNAL_ID EPOCH OUTPUT_DIR
```

RUN_ID/JOURNAL_ID are fresh nonzero lower-case SHA-256-shaped identifiers; EPOCH
is at least 4. This writes new, read-only `grant.json`, `grant.sig`,
`operator-public-key.bin`, `task.json` and `anchor.rs`; existing files are never
overwritten. It does not generate keys, install files or start services.

The signature domain is `BOOLE-DEVELOPMENT-TUPLE-TASK-ADMISSION-V1`, with schema
`boole.development.tuple-task-admission.v1`. Exact task, anchor, checker release,
toolchain and execution-policy bindings form a separate development registry
identity. This never asserts membership in the frozen production registry.
The runtime regenerates materials from the signed typed spec, not from the
operator's exported task/anchor files or request-selected paths.

Explicit installation provisions the out-of-band public key and signed grant in
root-owned mode-0555 `/usr/share/boole/native-shadow/development-task-v1/`, with
mode-0444 regular files. It also provisions separate mode-0700 per-journal
directories below `/var/lib/boole/native-shadow/development-task-node/` and
`development-task-launcher/`, respectively owned by the fixed node identity and
root. The existing qualified checker, rootfs, toolchain and containment authority
must still pass all their original checks. Before execution, the launcher
re-verifies the signed task against its retained startup identity.

One installation selects **one task at a time**, one candidate and at most one
checker execution. Multiple-task service concurrency, permissionless admission,
retry-until-success and general source-package import are not implemented.
Another problem requires another explicitly signed grant and separate durable
state, with services stopped for operator reprovisioning. Reusing spent state
under another grant fails closed.

## Durability and result boundary

Node and launcher independently spend their one-shot budgets before execution.
Task mismatch is rejected before spending the candidate; intake rejection still
consumes it. A different candidate is refused. An ambiguous in-flight crash does
not become permission to execute again. A terminal receipt can be returned once
after a separately signed identical redelivery permission:

```text
boole-canary-authority create-task-redelivery KEY GRANT_DIR CANDIDATE_DIGEST SUBMISSION_DIGEST OUTPUT_DIR
```

The retained receipt/evidence remains byte-identical apart from the delivery
indicator; redelivery spends no checker budget. This is at-most-once execution,
not a claim of guaranteed completion after arbitrary crashes.

Protocol tests cover signature/scope/material substitution, unsafe specs,
semantic-identity collision, exact exported materials, restart and cross-task
state refusal. Installed-authority tests cover the separate path and replacement
after startup. The Linux manager gate runs actual MCP → contained checker for
generated accepted, incorrect and scaffold-tampered answers, cross-task rejection,
terminal crash/restart/redelivery and ambiguous in-flight crash refusal. Synthetic
CI answers are generated from each public spec; they are not model results.

The first PR CI run (`34245174844`) exposed a harness mistake before reaching
the new-task matrix: the cross-task negative probe reused the historical
canary's same problem with only its epoch changed. That is a valid historical
candidate, not a different task. The corrected probe compares actual problem
identity first, with a regression test for same-problem and different-problem
cases. The failed run remains preserved; no checker acceptance rule or budget
was relaxed.

In the next run (`34246937751`), x86_64 passed the complete new-task matrix.
ARM64 passed the historical replay/canary matrix, then the existing 2,100-second
outer orchestration limit expired after the new-task binaries built (exit 124),
before any new-task result was established. The outer ARM64 matrix budget now
includes the added gate's bounded 900 seconds: 3,000 seconds plus 600 seconds of
workflow reserve. Individual checker, HTTP, containment and recovery limits are
unchanged; the ARM64 new-task result still requires a successful complete run.

Run `34250957584` subsequently passed both architectures' complete actual MCP
and contained-checker matrices, including the new-task cases. Its final self-test
then rejected the changed outer timeout because a mirror-transport test pinned
the entire orchestration script. That test now checks the actual offline phase
wiring and kernel/network restrictions instead of freezing unrelated CI budgets.
Frozen package, builder, checker and output identities are unchanged and retain
their independent authority checks. Full required CI still gates adoption.

Run `34255846637` again passed both complete Linux matrices, then exposed one
additional historical/current-script comparison in the 3,058-test Python batch.
The old serving-gap measurement's script digest remains unchanged as evidence;
its test now distinguishes that historical digest from the current CI script's
continued use of the sealed output expectation and independent receipt check.
The measured record and all artifact authority bytes remain unchanged.

Every admitted task remains `nonIssuable=true`; `activationAllowed=false`.
This is a generated development problem family, not new real-world-source supply,
a formal proof corpus, a benchmark result or reward-ready mining. The fixed
checker's existing assurance limits remain. Additional real-model execution,
general-user installation, production release, public P2P, payments and rewards
are separate scopes. Verified paid-validation buyers/LOIs were not checked.
