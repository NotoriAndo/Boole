# Real gate release contract, M3d-a

`scripts/zk_registry/m3_gate` defines the data-only boundary for a future M3
gate release. This slice does not activate a release or change the fold, wire,
supervisor, native tools, certified state, fitted caps or existing registry.
The executable is a Mac development adapter over the existing DET3 tools.
Production isolation, artifact fetching, retry scheduling and activation belong
to the subsequent supervisor integration. No real EENV5 identity is issued here.

## Descriptor and authenticated inputs

`codec.py` is the normative schema and field order. The compact ASCII JSON
descriptor has version 1, adapter `boole-m3-real/v1`, source and kernel-set pins,
reference interpreter `CPython 3.9.6`, JSON nesting limit 127, v2 error-policy
binding, cap/authority/inventory pins, the full-file SHA-256 of
`acceptance_environment_v2.json` and `acceptance_tools_v2.json`, grammar version, fuel
weights and a sorted architecture map. Each profile binds Comparator, nanoda,
Lean replay, counter replay, exporter, Lean runtime, imports, sandbox and codec.
Only integer numeric values are accepted; duplicate, unknown and reordered
fields, alternate JSON encodings, floats, booleans as integers and host paths or
clocks are refused. The source pin covers the complete Python registry package
with relative names and byte lengths. The complete sorted profile map contains
exactly `Darwin-arm64` development and `Linux-aarch64` production; x86-64 is refused.
The kernel-set digest is checked against `BOOLE-M3/KERNEL-SET/v1`, NUL, and the
canonical object containing both full-file pins and that complete profile map.

The descriptor identity is SHA-256 of `BOOLE-M3/REAL-GATE-DESCRIPTOR/v1` followed
by a NUL and canonical descriptor bytes. Its ordinary byte SHA-256 remains a
separate content address. Selecting local profile pins preserves the complete
descriptor identity: a worker must not hash a host-filtered descriptor.
Missing production pins cannot be replaced with a development profile.

The trusted caller supplies the chain/problem/commit/solver, environment,
bundle and package content addresses, kind, lane and slot allowance. These
inputs are bound into the reply input digest. Filesystem paths are local tool
bindings, never miner-controlled bundle fields. An inventory's canonical bytes
and SHA-256 are bound by the descriptor; the authenticated package binds its
environment, target, statement, reference export, inventory and optional cap.
Fixtures exercise the schema without claiming a released EENV5 artifact table.

## Bundle, partition and first failures

The bundle is `BOOLE-M3/EXPORT-BUNDLE/v1`, NUL, an unsigned 64-bit big-endian
export length and exactly that many NDJSON export bytes, within 128 MiB including
the frame. The front parser checks every record, intern reference, DAG and scalar
before invoking a kernel. Bytes are strict UTF-8 without a BOM or unpaired
surrogates. A lexical, quote-aware limit of 127 JSON containers applies to every
record, including metadata, before JSON parsing. This matches nanoda
`parser.rs` (strict `BufRead::read_line` and `serde_json::from_str`) and pinned
serde_json 1.0.145's default 128-level recursion budget. Paired surrogate escapes
are allowed. There is no Lean source elaboration or miner program.

`front.py` derives base and library membership by structured declaration name
and complete normalized payload digest from authenticated inventories. Cached
theorem digests include proof terms, reducibility hints and safety flags. Names
absent from both inventories are own declarations; miner labels have no effect.
The certified-own adapter passes that explicit checked-own set to DET3. An empty set or a target
that is not an own theorem is rejected before a kernel; there is no challenge-name fallback.

Order is shared by decoded and typed-decoder transport paths:

1. Authenticate release/tools/inventory and the complete package, before any
   transport verdict or package-dependent reference/state fetch.
2. For certified-own packages, resolve cap identity, measured-reference equality
   and scalar slot allowance from the package plus pinned authority alone. An
   invalid published binding is 0803 before local reference access or any
   submission-phase REJECT; a local authority load failure stays D-node.
3. Kind 2 is VOID/0804, then lane 2 is D-node, before reading decoder evidence or
   submission transport. When both apply, kind2 wins. Compression cannot change
   either route's first failure.
4. Validate any private decoder channel and its release/tool/transport bindings.
   A typed invalid-data result can reject only with explicit no decoded output;
   a present bound output contradicts that result and is D-node. Real output
   address corruption remains STOP. Otherwise stream-hash the entire bundle,
   including an oversized tail, and confirm `export_digest` before any size
   verdict; compare OPEN/package environment, then apply the bundle byte limit.
5. Parse all export structure; identify cached payloads; prioritize reserved
   declarations/references over payload mismatch; check own names, literal/depth,
   authenticated references, safety, profile and axioms. Bind local reference
   bytes and both statements, then traverse the own axiom closure.
6. Bind certified state (or the explicit public-counter qualification), check
   scalar allowance and own declaration count, then report uncertified library
   material as terminal S. Own checks use declaration order after reservation;
   a library S exit cannot erase an earlier independent own/statement/axiom fault.

Package authentication is `front.package`, not a partial allowance lookup. First
confirm the caller-bound SHA-256 of the carried package, then its 1 MiB limit,
strict scalar JSON, exact ordered `PACKAGE_KEYS`, canonical bytes, version 1,
`full` or `certified-own` mode, nonempty problem label of at most 1,024 UTF-8
bytes, four nonzero SHA-256 fields, and the complete structured target name.
The target is a nonempty sequence of at most 256 nonempty string or u64 numeric
components; `false` is not a target. Inventory/environment membership must match
the descriptor. Unsupported local inventory/environment is D-node. Published
package grammar failure is VOID/0801 even if submission transport is broken or
the reference file is absent. Its authenticated carried bytes determine the
0801 fingerprint. Only a valid package can proceed to a transport verdict;
malformed submission transport then uses `front_invalid`/0106. The certified-own `fuel_cap`
identity and reference binding are resolved at step 2, before local reference
bytes, decoder evidence and submission-phase REJECTs. The CLI streams package
bytes through EOF with at most 1 MiB retained; a correct oversized address is 0801,
and a corrupt tail remains an integrity STOP.

The M3-A3 amendment reserves `Leaf.*`, `Boole.Leaf.*` and `BooleCS.*` against new declarations
only. Their references must resolve to complete payloads authenticated by the
environment or package inventory. Missing declarations are a typed
`unknown_constant` rejection (0106), not a reserved-name violation. The
`Boole.R<k>.*` declaration and reference ban remains in force. Reserved own
declarations are checked before reference policy, independently of row order.
Private-name conversion follows Lean 4.34.1 `Lean/PrivateName.lean:38-64`: strip
the root `_private` string prefix through its first numeric component only when
that component is zero, including the anonymous module. Thus
`_private.M.0.Foo.1.BooleCS.X` converts to `Foo.1.BooleCS.X`. Reservation also
conservatively checks numeric suffixes of private wrappers, so that spelling and
`_private.0.BooleCS.X` cannot hide a reserved declaration.
`Boole.R<digits>` prefixes remain reserved, while `Boole.Ratchet.*` is permitted.
An identical authenticated payload is cached input; a changed reserved payload
is a reserved redeclaration (0201), before the generic payload mismatch (0106).
An own axiom is never allowed by its spelling alone: only an exact authenticated
inventory payload can supply a permitted base axiom.

All submission-derived computation after `read_export` and before the first
kernel runs in one explicit guarded boundary. Its exact exception-type allowlist
maps `IdentityError` and Unicode encoding/decoding errors to malformed 0106,
`IdentityCap` to size/work 0108, and explicit `SubmissionTypeError` to type 0301.
Every other exception remains DEFER(D-node); generic KeyError, ValueError and
RuntimeError are not miner rejections. Trusted reference computation retains
node provenance inside that phase.

Malformed submission data is a cheap typed rejection. A missing or malformed
trusted reference, generated export, tool or inventory is DEFER(D-node).
Content-address mismatch of a bound job input is a sticky STOP, not a proof
verdict; missing external input is a resumable STALL at the integration boundary.
After package authentication and whole-bundle address confirmation, an OPEN
environment differing from the package is the submission row `front_invalid`,
REJECT/0106/apply, both kernels `not-run` and fuel zero. This rule is identical
on first execution, retry and replay. It is not an integrity STOP or an uncounted
D-node retry. Genuine address corruption and unknown local release bindings keep
their existing integrity/availability classes.
Unsupported or empty own names reject with 0103, without aliasing another name.
Natural literals use the fixed 4,096-byte cap: decimal length is bounded at
9,865 digits before conversion, then converted in nine-digit chunks and checked
by bit length. JSON integer fields are u64 and bounded before conversion. These
rules do not depend on the interpreter's configurable integer-string limit.
Only digest-matching bundles over 128 MiB reject with 0108, including at the CLI.
Mismatched bytes cause a D-input integrity STOP, regardless of size; missing
bundle bytes remain D-input availability failure. Confirmed pre-kernel
rc 2 resource caps reject with 0108 and zero kernel work; the front's declaration
fuel precheck rejects with 0303. Physical interruptions remain availability failures.

## Outcome contract

`mapping.ROWS` is the single closed outcome/reason/action table. Its independent
test vectors cover every row on first execution, retry and replay. Every path
loads the descriptor-bound v2 rules. Physical failure takes transient precedence;
submission-specific faults on trusted inputs become D-node. Native text is not
searched for semantic causes. Submission `output-cap` is terminal S/0109;
capture failure and physical interruption remain transient. Neither S outcome
retries or quarantines. The actual closure output-cap record has no
`comparator_status`; it is terminal 0109 with unknown actual kernel work and a
zero terminal intake charge under the explicit v2 charge rule below. Other
missing status/counter evidence remains a tool protocol failure and never
fabricates zero work or a completed-kernel rejection.
Unknown or inconsistent replies defer; local timeouts never authorize VOID.
VOID/0802 requires an explicit published-fork authorization from the caller.
Comparator statement, base or axiom disagreement after front validation is
DEFER class I, not a miner rejection. Silent exit also maps to I. A package's
unknown inventory/environment or a local authority load failure is D-node;
only a published cap binding invalid against the pinned authority is VOID/0803.
For certified-own packages, the M3 caller additionally requires
`FC.resolve(...).reference_export == package.reference_sha256`. Mismatch is the
published `fuel_binding_invalid`/0803 row at step 2, before decoder/submission
verdicts and before any reference or certified-state file access. Missing or corrupt local bytes of the correctly bound
reference are D-node. This M3 rule does not change frozen `fuel_caps.py`, registry
releases, reference debits or the exact fourfold allowances. A candidate may
still choose a different safe own delta and allowed library material.

Authenticated malformed packages alone use the nonzero SHA-256 fingerprint of
`BOOLE-M3/VOID-STATEMENT/v1`, NUL, chain u64, problem/environment/package SHA-256
bytes and reason 0801 u16, all integers big-endian. It is refused for acceptance,
rejection and every other VOID reason. It is not an ordinary proof statement id.

Authorized quarantine keeps a valid authenticated package statement unchanged.
If the authenticated package cannot provide a valid statement, its candidate-only
fallback is SHA-256 of `BOOLE-M3/QUARANTINE-STATEMENT/v1`, NUL, chain u64be,
problem32, commit32, environment32, package-SHA32 and reason 0802 u16be, in that
order. This separate fingerprint is valid only for authorized VOID/0802; it is
refused for ACCEPT, REJECT, 0801 and other VOID reasons. Zero and the 0801
fingerprint are never substitutes. The problem stays open, and the candidate
retains its original queue index. No timeout or local retry authorizes this row.
`mapping.quarantine_statement` and `check_statement`, and the adapter reply
encoder/decoder, take explicit `fork_authorized` and `authenticated_statement`
arguments. The required `mapping.NO_VALID_STATEMENT` sentinel alone asserts
completed authentication found no valid package statement. Omitting the argument
or passing `None` cannot select the fallback. Missing local package/inventory
bytes must defer instead. Every statement validation also requires `commit_hash`,
including ACCEPT, REJECT and other VOID reasons. The consumer must pass a valid statement whenever one is available;
it cannot select the fallback in its place. This is a new gate contract, with no
fold/wire change or reinterpretation of historical descriptors.

## Typed transport decoder channel

The real transport controller must separate decoder data errors from host I/O,
resource and output-write faults. The private channel consumed by `decoder.py`
is at most 8,192 bytes of canonical JSON, with exactly these ordered fields:
`version`, `context_sha256`, `gate_rule_digest`, `descriptor_sha256`,
`decoder_sha256`, `transport_sha256`, `status`, `cause`, `raw_sha256`, `raw_bytes`.
Version is integer 2. Bind the full canonical job context, both descriptor
identities, the pinned decoder artifact and the complete authenticated transport
bytes before decompression. Expected decoder/transport digests are explicit
trusted node arguments, not values copied from the status itself. The controller
must verify the decoder artifact against its authenticated release configuration
and hash the reconstructed transport. No new operational tool pin is invented by
this Python contract. The channel is private node evidence, never miner metadata;
it is read after package/cap and kind/lane checks. Version 1 is refused.

| status | cause | decoded fields and result |
| --- | --- | --- |
| `ok` | `none` | Nonzero output SHA-256 and positive u64 byte count; both must equal the actual fully address-checked bundle before any size verdict. |
| `invalid-data` | `frame`, `checksum`, `digest` | Both output fields null and explicit no decoded output; then `front_invalid` REJECT/0106, not-run/fuel0. Present matching output is contradictory D-node; corrupt output address is STOP. |
| `unavailable` | `read`, `write`, `resource`, `tool`, `protocol` | Both output fields null; local DEFER(D-node), no submitted attempt or proof verdict. |

Missing, foreign-job, noncanonical, inconsistent or unknown channel evidence is
D-node. An exit code, signal or stderr string is not a typed cause: in particular,
the pinned external decoder's exit 1 also covers a host write error. A real-only
decoder must obtain data-error versus I/O/resource provenance from a typed API,
not relabel that exit code or search diagnostics. The supervisor controller
must follow this rule; the eight historical stub source files remain frozen.
Outer process wall/RSS/capture interruption keeps its existing transient policy.

The Python bundle path is uncompressed data: `front.unpack` and the strict
NDJSON parser invoke no external decoder and do not classify any exit code.
Python export/reference/output I/O and MemoryError stay D-node; missing external
bundle bytes keep their D-input availability class. Malformed submitted
grammar is 0106. `run(..., decoder_status=..., decoder_pin=...,
transport_digest=...)` consumes the channel with both required node bindings.
Pass `bundle=None` only to explicitly report no decoded output; the CLI uses
`--no-decoded-output` instead of `--bundle`. CLI `--decoder-status` is paired with
`--decoder-pin` and `--transport-digest`; all are consumed only after package/cap
and kind/lane decisions, so a sidecar cannot mask 0801, 0803 or 0804.
Omitting it means the caller already supplies decoded bytes under the original
binding contract; it does not authorize an untyped compressed-transport failure.

Current native follow-ups are explicit depth attribution, explicit kernel-panic
attribution, typed runtime and pre-kernel cap provenance, and general structured own/target
selection. Those missing causes remain DEFER(D-node); 0306 is never inferred from
generic replay tool fault or a silent exit. Every current `tool-fault:replay`,
including a non-Stop nanoda panic, maps to `kernel_panic_cause_missing`,
DEFER(D-node), never 0307. No producible 0307 row exists, and the strict reply
decoder refuses it. Native changes require a separate release and fitted remeasurement.
An undifferentiated native cap with no exceeded aggregate counter defers, because
the existing marker cannot distinguish per-declaration fuel from depth.

## Debits and final channel

`bounded-debit/v2` fixes nanoda-before-Lean execution and positive integer scalar
weights. Executed kernels report the exact raw debit. Only a confirmed typed
logical cap charges `min(raw debit, kernel allowance)`; other completed work
charges its exact debit. Scalar fuel is the checked u64 weighted sum. Front-only
failures and the declaration-count precheck run neither kernel and charge zero.
An early typed nanoda cap reports Lean `not-run`, null debit and zero charge.
A missing executed counter is `unknown`, with null debit, charge and total fuel,
and forces DEFER. The sole terminal output-cap exception keeps both states
`unknown` and raw debits null, while charging zero intake fuel. This is not an
assertion that kernels did not run. The strict decoder refuses other state or
charge combinations for 0109. Positive debits, weights and all fitted numerical
caps retain their previous rule and values. Other unknown work never defaults
to zero. Physical interruptions cannot earn
a logical cap verdict. A1's fixture weights do not calibrate a production clock.

Invoke `python3 -B -m zk_registry.m3_gate` with the required descriptor, gate
digest, bundle (or explicit no decoded output), package, inventory, context, reference, tool-root, counter-driver
and exclusive work paths; use `PYTHONPATH=scripts`. Certified-own packages also
need the certified-root binding. The final channel is one
`BOOLE-M3/GATE/v1 ` marker followed by canonical reply JSON. An undecodable
request/release exits 70 without inventing a reply identity. The strict consumer
checks release/input/mapping/fingerprint, kernel-state and scalar consistency.

The explicit `--public-counter` qualification mode admits only committed corpus
content addresses and `PublicCounter.target`; it is not an uncapped intake path.
Lane 2 defers and kind 2 remains VOID/0804 until their separate qualifications.
The 32 public vectors and seven fitted packages exercise existing native tools;
Public-counter mode checks the complete committed corpus and explicitly refuses
`candidate_declarations`; certified-own mode alone supplies a selected delta.
A public-counter corpus mismatch is DEFER(D-node), never package VOID.
The legacy generator/executor source identity is unchanged.
The amended adapter rotates `source_sha256`, the ordinary descriptor byte SHA-256
and its domain-separated gate identity. With the same artifact cohort, all other
descriptor fields, including profiles, native pins, grammar, fuel weights, caps,
authority and kernel-set digest stay unchanged. The new decoder module is covered
by the complete registry Python source pin. No EENV5 release or activation is
issued by recording development descriptor pins.
This evidence does not admit Linux production, x86 production, M-FULL or a new
environment. Reset versus migration is an operator decision needed at M3d-c.
