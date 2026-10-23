# Devnet verdicts v1.3 draft: staged pure M3 fold

This is an inactive development profile in `boole-derivation`. The devnet still
uses [M2 v1.2](devnet-commit-reveal-v1.2.md). This change adds no gate process,
driver activation, reset, live proof certification or executed reward mint.
The node supplies verdicts to a pure fold; the fold never reads files, clocks,
RPC, peer results or host resources. M3b must supply and independently verify
those results before enabling the profile.

Fork activation uses amendment R-4's canonical kind and disposition bytes,
list digest and 69-byte body. Pure tests qualify activation, quarantine release
and replay from genesis. The profile remains inactive on the devnet.

## Staging, configuration and startup

`Config.m3` is optional, appended after the existing fields, and omitted entirely
when absent. Every ordinary constructor still returns `None`. With it absent,
config JSON, config digest v1, root v2, events, attributes and v1.2 fixtures stay
unchanged. M3 needs explicit `State::step_with_verdicts` or `derive_with_m3`;
the old entrypoint refuses an enabled profile instead of omitting M3 inputs.

With M3 enabled, the config digest is SHA-256 of `BOOLE-M2/CONFIG/v2` followed by
compact JSON in Rust declaration order. Ordered maps and sets retain their order.
The nested M3 fields, in order, are:

```text
k, rate, cost_fixed, fuel_slot, verdict_sink, q_reward, reward_gas,
maturity, reward, mint_enabled, notice, open_licenses, bridge,
native_is_reward_unit, genesis_gate_rule_digest
```

The staged devnet uses K=8, RATE=1,000, COST_FIXED=100, FUEL_SLOT=10,000 per
slot, Q_REWARD=16, REWARD_GAS=100,000, MATURITY=64 and REWARD=1,000×10^18 per
slot. Its sink is `0x42000000000000000000000000000000000000b3`. It permits
minting only with no bridge. The staged testnet parameters use K=300,
MATURITY=50,400, NOTICE=7,200 and minting disabled. Production-shaped golden
configs use these same provisional M3 parameters; they are not calibrated
production parameters. The stub genesis gate identity is
`SHA256("BOOLE-M2/BODY/v1" || "BOOLE-M3/STUB/v1")`.

S1–S3 and S5–S8 remain. M3 replaces S4 and adds:

| Check | Refusal condition |
|---|---|
| S9 | `21,000 + 40*(24 + 80*V_PER_DEPOSIT) > VERDICT_GAS`, or REWARD_GAS < 21,000 |
| S10 | L1-info gas + M2ROOT gas + Q_MAX*VERDICT_GAS + Q_REWARD*REWARD_GAS + 16,777,216 exceeds the L2 gas limit |
| S11 | K, RATE or COST_FIXED is zero; missing/zero slot fuel; COST_FIXED + maximum slot fuel exceeds 2^63 |
| S12 | MATURITY < 2*K or MATURITY < NOTICE+K |
| S13 | REWARD has the wrong slot count, or a zero amount while minting is enabled |
| S14 | Missing/config-inconsistent genesis row, nonconsecutive epoch, nonincreasing height, later GENESIS or empty QUARANTINE list |
| S15 | A TERMS_TABLE id lacks an entry in `open_licenses` |
| S17 | Minting with a bridge while the native unit is not the reward unit |

Arithmetic is checked. S16 concerns node binaries/artifacts and belongs to M3b.

## Queue, retained positions and scheduling

A completing accepted PART gives a CHECKING candidate the next global u64 queue
index, starting at zero, with `avail = h_r`. Completing logs are ordered by
`(transaction_index, log_index)` across every slot and problem. The queue is
retained. A completion after settlement becomes SUPERSEDED immediately; after
problem VOID it becomes VOID. Neither enters the queue. Previously queued jobs
still run and can produce informational records after those transitions.

OPEN records retain `open_pos=(height,txIndex,logIndex)`, `done_pos`, the parsed
export digest and original queue index. Problems retain their original reveal
position, winner and voided flag separately from the commit-window close height.
Re-poses preserve the reveal position, pool, leaf and seed. No B, part or package
bytes are added to state. M3b rebuilds them from the frozen L1 positions.

For each supplied result, in queue order:

```text
F_i   = max(F_previous, avail_i*RATE) + COST_FIXED + fuel_i    # checked u128
d_i   = max(avail_i+K, ceil(F_i/RATE))
due_i = first height >= max(d_i, previous_due) with fewer than
        CAP_H = Q_MAX*V_PER_DEPOSIT records already assigned
```

The selected gate epoch is active at `avail+K`, including equality with a fork
height. Recheck jobs can bind the activating epoch explicitly. Fuel cannot exceed
the problem slot's cap. Results must identify the next contiguous queue prefix,
with the expected kind, commit and epoch. Unknown, reordered, extra, overdue or
invalid-target results are input errors. Missing results stall at `avail+K`.
Supplied future results can establish later due heights without being applied;
only the applied prefix advances the retained fuel clock. DEFER is never a state
or a verdict. Any error leaves the caller's state unchanged.

## Height order and outcomes

The order is fork activation and recheck enqueueing, ties for the previous height,
due verdicts, slot events, expiry, M2 pruning, inbox logs and new completions,
settlement in ascending problem id, vesting, then root.

Statuses 1–5 remain COMMITTED, OPEN, CHECKING, EXPIRED and CANCELLED. New bytes:
6 PASSED, 7 ACCEPTED, 8 REJECTED, 9 VOID, 10 STALE, 11 SUPERSEDED. These retained
M3 statuses cannot be reopened, completed again or cancelled. No M3 status is
pruned; accepted, stale and kernel-fault records retain their frozen metadata.
Per-solver counts must be derived from this state, never used as admission limits.

Kind-1 ACCEPT/REJECT/VOID targets CHECKING and yields PASSED/REJECTED/VOID.
On queued SUPERSEDED or VOID targets it is informational. Kind-2 ACCEPT on
ACCEPTED/PASSED leaves status unchanged and clears the pending recheck. REJECT
changes ACCEPTED to STALE or PASSED to REJECTED. Only a REJECTED `kernel-fault`
may be reversed: ACCEPT becomes PASSED at its original priority if no winner
exists, otherwise ACCEPTED with compensation. A further REJECT replaces its
reason. Other targets or kind-2 VOID are errors.

The unresolved barrier consists of COMMITTED, OPEN, CHECKING and PASSED, ordered
by `(h_c,t,commit_hash)`. Its earliest candidate can settle only when PASSED and
not awaiting a recheck. It becomes ACCEPTED; other CHECKING/PASSED candidates
become SUPERSEDED. COMMITTED/OPEN candidates keep their lifecycle. A settled
problem refuses new COMMITs. Settlement emits round event C5, assigns a deferred
reward and an admission index. An open window closes and schedules its next draw
at `h+GAP`. A previously deadline-closed problem only gains its winner: its slot's
new round is left alone.

VOID reasons 0801 (package), 0803 (fuel cap) and 0804 (generator) void the problem
once, emit C6, close its still-open window and mark other CHECKING candidates
VOID. Later completions are VOID. Reason 0802 (quarantine) only voids its candidate
and releases the barrier. Unsolved problems create no reward.

## Rewards, admissions and rechecks

Rewards have consecutive u64 ids from zero and retain problem, commit, B's
solver, the slot's u128 amount, `vest_h=h+MATURITY` and status LOCKED. They mint
nothing while locked. At most Q_REWARD entries vest each height in
`(vest_h,reward_id)` order; overflow waits. With minting disabled, entries still
vest and emit state records without REWARD deposits. There is no claim call.

STALE cancels LOCKED rewards only; VESTED rewards remain. It retires the
candidate's admission, emits C7 and feeds the private re-pose queue. The next
available draw opens epoch+1 with the original leaf, seed and reveal position.
The old id stays closed. Reversing a kernel-fault reject after another winner
creates one compensation entry and one admission; the earlier winner and window
stay unchanged. Repeated ACCEPT rechecks create no further compensation.
There are no reimbursement or deduction payments or consensus entries. M3b's
L1 rescan logs the evidence-only R21 formula
`(21,000 + 48*item_length)*item_L1_base_fee` for senders of record.

Admission indices start at one, in settlement/compensation order; retirement
never reuses an index. The reserved-name policy reason has stub descriptor
assignment 0201 inside the policy range. Real gates must reject declarations or
references under Boole.R, Leaf and BooleCS; this fold does not parse Lean names.
The manifest helper takes already alpha-v1-normalized declaration hashes,
rejects duplicate name bytes, sorts by those bytes and hashes:

```text
BOOLE-M3/MANIFEST/v1 || u32be(count) ||
    (u16be(name_length) || name || kind_u8 || decl_hash_32)*count
```

FORK_TABLE is an explicit input outside immutable Config. Genesis is epoch zero,
height zero and the configured gate digest. Epochs are consecutive, activation
heights strictly increase, and past rows are pinned in state: they cannot be
inserted, removed or changed retroactively. A future row can be added.

At a KERNEL-FIX activation H, older-epoch ACCEPTED, PASSED and
REJECTED(kernel-fault) candidates enter kind-2 rechecks in original queue order.
Each new job has avail=H and the activating epoch. PASSED candidates become
pending immediately, before verdicts and same-height logs can release a barrier.
POLICY only selects epochs for new jobs and never enqueues rechecks or makes
candidates STALE.

QUARANTINE names unapplied CHECKING first-verdict jobs. It changes their epoch
without appending a job or changing their original queue index, avail or frozen
positions. RECHECK supplies their first verdict under the fixed gate. VOID
requires outcome VOID and reason 0802, including when a supplied future result
moves the due height later. It releases only that candidate's barrier, leaves
the problem open and grants it no reward. List order never reorders the queue.

R-4 assigns kind bytes GENESIS=0, KERNEL-FIX=1, QUARANTINE=2, POLICY=3 and
dispositions RECHECK=1, VOID=2. Only QUARANTINE carries entries, at least one;
the other kinds require an empty list. Entries are in strictly increasing
commit-hash byte order: duplicates and unsorted lists fail S14. The digest is:

```text
SHA256("BOOLE-M3/FORK-LIST/v1" || u32be(count) ||
       (commit_hash32 || disposition_u8)*count)
```

The empty list uses the same domain and count zero. The activation body is
`epoch_u32be || gate_rule_digest32 || kind_u8 || list_digest32`, exactly 69 bytes.
E_h prefixes it with the record tag 08 (70 bytes including that tag). No dependency
taint or nonempty deps is enabled.

## Deposits, events and root bytes

The system prefix is L1-info, M2ROOT, all VERDICT batches, all REWARD deposits,
then users. M2ROOT keeps its v1.2 envelope but commits the final post-fold root.
Before F6 forwarding, the user budget subtracts both fixed deposits plus the
**actual** batch count times VERDICT_GAS and payment count times REWARD_GAS.

Both new deposits are type 7e with eight canonical RLP leaf fields:
sourceHash, from, to, mint, value, gas, false system flag, data. From is DEPOSITOR.
VERDICT has zero mint/value and the configured sink/gas. Its source is
`SHA256("BOOLE-S1/VERDICT" || origin_hash || uint256(batch_index))`. Its data is:

```text
"m3vd" || u64be(origin_number) || batch_index_u8 || batch_count_u8 ||
u64be(first_queue_index) || u16be(record_count) || record_80*record_count
```

Records are kind(1), commit(32), outcome(1), reason(2), gate_epoch(4), fuel(8),
vstmt_digest(32), in that order, big endian. Kinds are verdict=1/recheck=2;
outcomes ACCEPT=1/REJECT=2/VOID=3. ACCEPT reason is zero. REJECT categories are
01xx cheap, 02xx policy, 03xx kernel; 0306 is kernel-fault. VOID codes are
0801–0804 above. Individual labels are closed by the future pinned descriptor.
Every nonlast batch is full, indices are contiguous and batch counts are bounded.
The decoder checks every envelope field and exact canonical re-encoding; import
must also compare with the locally computed records and complete system prefix.

REWARD's source is `SHA256("BOOLE-S1/REWARD" || origin_hash || uint256(reward_id))`.
To is the solver, mint=value=u128 amount, gas is REWARD_GAS and data is empty.
The strict decoder binds the expected reward id; the local-payment checker also
binds solver and amount. This draft only constructs attributes, not execution.

The verdict statement preimage is:

```text
BOOLE-M3/VSTMT/v1 || u64be(chain_id) || problem_id || commit_hash || solver20 ||
env_id || export_digest || statement_id || gate_rule_digest || kernel_set_digest ||
outcome_u8 || reason_u16be || ext_length_u16be || ext
```

Only REJECT may use a zero statement id. Extension is empty except lane-2 ACCEPT:
schema_u8, metric_count_u8, u64be metrics, candidate_digest32, source_digest32.
The statement digest is SHA-256; kind, queue index and fuel are outside it.

M2 event formats stay unchanged. Additions, including the leading type byte:

| Type | Following bytes |
|---|---|
| 05 | queue_index u64be, record80 |
| 02 | problem32, commit32, new status u8 |
| 03 | existing round record, with C5 settlement/C6 void/C7 stale |
| 06 | reward_id u64be, problem32, commit32, solver20, amount u128be, vest_h u64be, event u8 |
| 07 | admission k u64be, problem32, commit32, event u8 |
| 08 | epoch u32be, gate_digest32, kind u8, list_digest32 (69-byte body, 70 with tag) |

Reward events are 1 LOCKED, 2 VESTED, 3 CANCELLED, 4 COMPENSATION-created.
Admission events are 1 ADMITTED, 2 RETIRED. Events follow the height order above.
Root zero is config digest v2. Subsequent roots are:

```text
SHA256("BOOLE-M2/ROOT/v3" || previous_root || u64be(height) ||
       L1_hash || SHA256(event_bytes))
```

The independent stdlib [generator](../scripts/devnet/generate_m3_vectors.py)
reproduces [wire/config/event goldens](../fixtures/devnet-m3/system-v1.3.json)
with `--check`. A separate [Registry generator](../scripts/devnet/generate_m3_service_vectors.py)
reproduces the [VOID parity checkpoint](../fixtures/devnet-m3/service-void-v1.3.json)
through the real commit/reveal/recheck/void APIs with injected verifier outcomes.
The pure fold applies an actual QUARANTINE activation for the VOID parity checkpoint.
Existing v1.2 recordings are not regenerated. Pure tests cover
the queue, fuel, transitions, settlement, vesting, activation-driven rechecks,
service parity and 51,000 decoder mutations. Fork qualification compares every
height's event bytes, root and VERDICT/REWARD attributes between independent
folds from genesis, restarts at an activated checkpoint, refuses changed past
rows and observes root divergence at a changed activation. The independent
Python vectors cover all kind/list bytes and three-profile activation roots.
These tests do not qualify supervisor behavior, L2 balances or live activation
of the inactive devnet profile.
