# Real gate release contract, A1

`scripts/zk_registry/m3_gate` defines the data-only boundary for a future M3
gate release. This slice does not activate a release or change the fold, wire,
supervisor, native tools, certified state, fitted caps or existing registry.
The executable is a Mac development adapter over the existing DET3 tools.
Production isolation, artifact fetching, retry scheduling and activation belong
to the subsequent supervisor integration. No real EENV5 identity is issued here.

## Descriptor and authenticated inputs

`codec.py` is the normative schema and field order. The compact ASCII JSON
descriptor has version 1, adapter `boole-m3-real/v1`, source and kernel-set pins,
v2 error-policy binding, cap/authority/inventory pins, grammar version, fuel
weights and a sorted architecture map. Each profile binds Comparator, nanoda,
Lean replay, counter replay, exporter, Lean runtime, imports, sandbox and codec.
Only integer numeric values are accepted; duplicate, unknown and reordered
fields, alternate JSON encodings, floats, booleans as integers and host paths or
clocks are refused. The source pin covers the complete Python registry package
with relative names and byte lengths. Mac profiles require development assurance.

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
before invoking a kernel. There is no Lean source elaboration or miner program.

`front.py` derives base and library membership by structured declaration name
and complete normalized payload digest from authenticated inventories. Cached
theorem digests include proof terms, reducibility hints and safety flags. Names
absent from both inventories are own declarations; miner labels have no effect.
The adapter passes that explicit checked-own set to DET3. An empty set or a target
that is not an own theorem is rejected before a kernel; there is no challenge-name fallback.

Order: authenticate release/tools/inventory/package and input bindings; parse
all export structure; authenticate cached payloads; check own literal/depth,
reserved-name, safety, declaration-profile and axiom rules; bind the trusted
reference and both statements; traverse the own axiom closure; resolve cap,
certified-state and scalar allowance; apply the own declaration-count precheck;
then report allowed library material outside the certified base as terminal S.
Own declaration checks use export declaration order. A library S exit cannot
erase an independently decidable earlier own/statement/axiom violation.

Malformed submission data is a cheap typed rejection. A missing or malformed
trusted reference, generated export, tool or inventory is DEFER(D-node).
Content-address mismatch of a bound job input is a sticky STOP, not a proof
verdict; missing external input is a resumable STALL at the integration boundary.
Valid structured targets or own names unrepresentable by the existing native
String selector conservatively defer rather than aliasing a different name.

## Outcome contract

`mapping.ROWS` is the single closed outcome/reason/action table. Its independent
test vectors cover every row on first execution, retry and replay. Every path
loads the descriptor-bound v2 rules. Physical failure takes transient precedence;
submission-specific faults on trusted inputs become D-node. Native text is not
searched for semantic causes. Outer capture truncation is transient; structured
submission output overflow is terminal S. Neither S outcome retries or quarantines.
Unknown or inconsistent replies defer; local timeouts never authorize VOID.
VOID/0802 requires an explicit published-fork authorization from the caller.

Authenticated malformed packages alone use the nonzero SHA-256 fingerprint of
`BOOLE-M3/VOID-STATEMENT/v1`, NUL, chain u64, problem/environment/package SHA-256
bytes and reason 0801 u16, all integers big-endian. It is refused for acceptance,
rejection and every other VOID reason. It is not an ordinary proof statement id.

Current native follow-ups are explicit depth attribution, explicit kernel-panic
attribution, typed runtime and pre-kernel cap provenance, and general structured own/target
selection. Those missing causes remain DEFER(D-node); 0306 is never inferred from
generic replay tool fault or a silent exit. Existing closed typed replay faults
can use 0307. Native changes require a separate release and fitted remeasurement.
An undifferentiated native cap with no exceeded aggregate counter defers, because
the existing marker cannot distinguish per-declaration fuel from depth.

## Debits and final channel

`bounded-debit/v1` fixes nanoda-before-Lean execution and positive integer scalar
weights. Executed kernels report the exact raw debit. Only a confirmed typed
logical cap charges `min(raw debit, kernel allowance)`; other completed work
charges its exact debit. Scalar fuel is the checked u64 weighted sum. Front-only
failures and the declaration-count precheck run neither kernel and charge zero.
An early typed nanoda cap reports Lean `not-run`, null debit and zero charge.
A missing executed counter is `unknown`, with null debit, charge and total fuel,
and forces DEFER. It never defaults to zero. Physical interruptions cannot earn
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
this evidence does not admit Linux production, x86 production, M-FULL or a new
environment. Reset versus migration is an operator decision needed at M3d-c.
