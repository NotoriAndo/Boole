# Compensation seed admissibility in the audit comparison

This is a proof of an existing v1.3 state invariant used to select differential
test contexts. It changes neither the fold nor the M3 specification. The
[height-order and outcomes](devnet-verdicts-v1.3.md#height-order-and-outcomes),
[rewards and rechecks](devnet-verdicts-v1.3.md#rewards-admissions-and-rechecks),
and retained-state rules are the authority.

For a retained candidate key `(problem, commit)`, a compensation reward can
exist only while the candidate is ACCEPTED or STALE. A CANCELLED compensation
reward requires STALE and an existing winner for that original problem.

## Induction from genesis

1. Genesis has no compensation rewards. Ordinary settlement creates an
   ordinary reward, so it cannot introduce a compensation record.
2. Compensation is introduced only by an ACCEPT recheck of a kernel-fault
   REJECTED candidate after a winner exists. That same atomic height makes the
   candidate ACCEPTED and creates its reward and admission. Thus the invariant
   holds when compensation first appears.
3. An ACCEPT recheck on ACCEPTED preserves ACCEPTED. A REJECT recheck changes
   ACCEPTED to STALE, cancels its LOCKED rewards and retires its admissions.
   These are the only permitted subsequent result transitions from ACCEPTED.
4. STALE is absorbing for this retained key. Neither verdict kind admits a
   STALE target. Fork activation changes queues/epochs, not this status;
   quarantine targets CHECKING, settlement targets PASSED, and problem VOID
   affects CHECKING. M3 states cannot reopen, complete again or be cancelled,
   and these retained states are not pruned.
5. Vesting changes LOCKED to VESTED without changing candidate status.
   Cancellation occurs only with STALE; it never restores a reward. A re-pose
   creates a new problem identity and leaves the old problem closed, so its
   new empty winner does not clear the old key's winner.

These cases cover the operations in the specified height order. Read-only
confirmation is in `m3/fold.rs` (`apply_due`, `grant_to`, `settle`, `vest`), and
`m2.rs` (`slot_events`): the apparent winner reset belongs to the new re-pose id.
No fold helper is used to implement the auditor's invariant check.

## The fifteen preserved cases

All fifteen start with a CANCELLED compensation reward and retired admission,
but the candidate is CHECKING, PASSED or REJECTED(kernel-fault). Each prestate
already contradicts the invariant, before any word is applied. Consequently
none is reachable from genesis; none is a fold finding.

| Seed status | Winner | Prior reward | Frozen words |
|---|---|---|---|
| CHECKING | another candidate | CANCELLED compensation | `[1,4]`, `[1,4,4]`, `[0,5,4]`, `[1,5,4]`, `[8,5,4]` |
| PASSED | another candidate | CANCELLED compensation | `[5,4]`, `[5,4,4]`, `[4,5,4]`, `[5,5,4]` |
| REJECTED(kernel-fault) | another candidate | CANCELLED compensation | `[4]`, `[4,4]`, `[5,4]`, `[4,4,4]`, `[4,5,4]`, `[5,5,4]` |

Symbols are the frozen audit alphabet: 0–3 first-verdict ACCEPT/kernel-fault
REJECT/other REJECT/VOID, 4–7 corresponding rechecks, 8 quarantine RECHECK,
9 quarantine VOID. The CHECKING example `[1,4]` cannot repair an impossible
prestate by rejecting and rechecking within one height.

## Corpus rule and limits

The original 32-context grid used a CANCELLED compensation reward for every
`prior_compensation=true` cell. Apply the invariant uniformly before enumerating
words: only STALE with an existing winner survives in those cells. This excludes
15 seed contexts, or 16,665 words of lengths 0–3. The other 16 contexts plus that
one surviving prior-compensation projection retain 18,887 words and 54,570
symbols, with the same ten-symbol alphabet and maximum length three.

The exclusion is fixed by the proof, independently of comparison outcomes.
Excluded seeds must also fail the auditor's input-context check; an accepted
excluded seed is a test failure. Retained contexts remain component transition
snapshots, not a certificate that every queue/clock/history field is jointly
reachable. No observed disagreement is waived by inspecting its result.

The `already_compensated` branch of the kernel-fault reversal calculation cannot
be reached in a valid history: reversal needs REJECTED, whereas a prior
compensation record requires ACCEPTED or STALE. Repeated ACCEPT on compensated
ACCEPTED is still exercised by the ordinary recheck rules and existing ordered
reward controls. An impossible seed must not be introduced merely to execute
the defensive branch.
