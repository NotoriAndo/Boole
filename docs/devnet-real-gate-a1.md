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

Order: authenticate release/tools/inventory/package; stream-hash the entire
bundle, including an oversized tail, and confirm the bound `export_digest`
before any size verdict; parse
all export structure; identify cached payloads; prioritize reserved declarations
and references over cached-payload mismatch; check own name grammar, literal/depth,
authenticated references, safety, profile and axiom rules; bind the trusted
reference and both statements; traverse the own axiom closure; resolve cap,
certified-state and scalar allowance; apply the own declaration-count precheck;
then report allowed library material outside the certified base as terminal S.
After the reserved declaration pass, own checks use export declaration order. A library S exit cannot
erase an independently decidable earlier own/statement/axiom violation.

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

Authenticated malformed packages alone use the nonzero SHA-256 fingerprint of
`BOOLE-M3/VOID-STATEMENT/v1`, NUL, chain u64, problem/environment/package SHA-256
bytes and reason 0801 u16, all integers big-endian. It is refused for acceptance,
rejection and every other VOID reason. It is not an ordinary proof statement id.

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
digest, bundle, package, inventory, context, reference, tool-root, counter-driver
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
This evidence does not admit Linux production, x86 production, M-FULL or a new
environment. Reset versus migration is an operator decision needed at M3d-c.
