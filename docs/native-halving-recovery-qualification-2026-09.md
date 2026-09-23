# Native first-halving recovery qualification — 2026-09-23

Status: **PREREGISTERED — execution results pending.**

## Decision and scope

The compiled monetary policy already fixes the first halving at height 10,000,
ten-block reward maturity and a test-only supply ceiling. Core arithmetic tests
cover that schedule; they do not alone qualify the durable native node's recent
fork/restart path at the actual first-halving height. This qualification combines
those existing public interfaces without changing production code, parameters,
validation, signing, admission or fork choice. A small companion uses height 16
for the special reward and tests the same recovery flow without a halving.

Use disposable deterministic development spending keys and a private temporary
state directory on the same developer Mac (`Mac16,9`, arm64, 16 logical CPUs,
64GiB RAM), Rust 1.95 debug executable. Every block must have actual PoW and
producer authorization at the unchanged target, with 60-second height-based
timestamps. No preloaded ledger, synthesized checkpoint, altered schedule or
signature shortcut is permitted. No sockets, operator keys/funds, paid/VM/model
run, release, public mining or activation are involved.

## Fixed scenario and acceptance

Let H=10,000 for the qualification and H=16 for its small companion.

1. Build the durable node to H+8, rewarding producer A except for H, whose
   reward goes to owner B. Require exact independently calculated issuance and
   50,000 tBOOLE at height 9,999 / 25,000 at 10,000 in the large run. B's entire
   reward must still be locked. A signed nonce-zero transfer of one tBOOLE from
   B to C, minimum fee 1,000 atoms and expiry H+100 must be rejected unchanged.
2. At H+9, B still has zero canonical spendable balance, but the same signed
   transfer must enter the pending view for the next block, H+10, when its reward
   matures. Confirm it at H+10 and extend to H+11. B's canonical nonce is one;
   C receives exactly one tBOOLE and fees go to A.
3. Fork from the node's own verified prefix at H-2, and construct an alternative
   through H+13 with all rewards to A. Every suffix block is mined/authorized/
   validated normally. Adopt it through `adopt_recent_suffix` within ten seconds.
   The old B reward and transfer must disappear, B's nonce/balance and C's
   balance return to zero, the unfunded orphan must not remain pending, and the
   same signed retry must fail without poisoning readiness or changing files.
4. Close and independently reopen the forked state within 120 seconds. Require
   exact canonical head, issuance, balances, maturity locks, zero confirmed
   transfers and byte-identical canonical/pending journals and manifest.
5. Give B a new reward at H+14. At H+22 its same signed transfer must still be
   rejected; at H+23 it must enter the next-block pending view. Confirm the exact
   original ID at H+24. Repetition must not debit again. Require nonce one,
   exactly one confirmed transfer, one-tBOOLE recipient balance, unchanged fee
   allocation and exact independent issuance/locked/spendable accounting.
6. Close and independently reopen within 120 seconds, then stop and independently
   `audit_native_state` against the expected final head within another 120 seconds.
   Both must agree on accounting, transfer count/amount/fee and canonical/pending/
   manifest bytes. All changes stay within the private disposable state.

At H=10,000 the expected final height is 10,024 and issued atoms are
`9999 * 50000 * 100000000 + 25 * 25000 * 100000000 = 50057500000000000`.
The final ten rewards are still locked; B's height-10,014 reward has matured.
This is below the fixed one-billion-tBOOLE ceiling. Height zero pays nothing;
neither this scenario nor that ceiling promises the integer schedule exactly
reaches one billion. The existing contract's eventual issuance remains unchanged.

Each measured template, durable block append and candidate append must remain
under ten seconds. Total scenario excluding build must be under 1,800 seconds,
history under 96MiB and independently measured OS peak RSS under 1GiB. Mine with
the existing two-million-nonce per-block fixture bound. Record progress every
512 blocks, exact heads, accounting, phase timings, source/tree/executable IDs
before/after and complete OS output. Run the large executable alone under
`/usr/bin/time -l` after the small companion and focused static checks pass.
Keep all outcomes and these criteria unchanged.

This is a closed-local durable monetary-transition/recovery qualification, not
a speed claim for normal one-minute block production, public finality, a test of
all halvings, many accounts, maximum history, network reorg delivery, slower
hardware or complete R1/R2/R3 acceptance.
