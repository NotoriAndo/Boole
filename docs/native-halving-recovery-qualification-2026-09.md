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

## Small companion — first run PASS

The H=16 companion passed on the existing runtime in 4.180 seconds (build 12.01s,
harness 4.18s). Recent-fork adoption took 33ms, the fork/final independent reopens
17/22ms and the independent final audit 22ms. Maximum measured durable append
was 14ms; template and candidate append were each below one millisecond.
The original transaction was rejected while locked, confirmed at height 26,
removed with its funding reward by the longer fork, rejected while unfunded,
then reconfirmed with the exact same ID at height 40 after a new reward matured.
Repeated submission before and after final reopen made no additional debit.

The final audit reported 40 blocks, 200,000,000,000,000 issued/balance atoms,
50,000,000,000,000 locked atoms, three balance entries, one nonce entry and one
confirmed one-tBOOLE transfer with the 1,000-atom fee. Canonical/pending/manifest
comparisons and all independent per-account/reward checks passed. No production
behavior was changed and no failed execution preceded this pass. This small
case does not itself cross the actual first halving.

Raw execution output:

```text
Compiling ring v0.17.14
   Compiling boole-testkit v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-testkit)
   Compiling rustls v0.23.45
   Compiling rustls-webpki v0.103.15
   Compiling boole-p2p v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-p2p)
   Compiling boole-node v0.1.0 (/private/tmp/boole-r1-integration-worktree.LGTdSb/crates/boole-node)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 12.01s
     Running tests/native_halving_recovery.rs (/Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7)

running 1 test
test small_reward_maturity_fork_recovery_preserves_original_transfer ... halving-result {"audit":{"accounting":{"balanceAtoms":"200000000000000","issuedAtoms":"200000000000000","lockedAtoms":"50000000000000","pendingRewardEntries":10,"spendableAtoms":"150000000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"100000000","count":1,"feeAtoms":"1000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"00003c4da1ff44e121a4daf4219f4b73eafcd053427307ae2eb4ba7df0ebf7ff","height":"40","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":1,"historyBlocks":40,"historyBytes":31677,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"auditMs":22,"elapsedMs":4180,"finalFilesBlake3":["18c0f2e533e0ddd5ba88fd7e5e4cf4f81597be30a63f5bef8aee7c77b44f3c61","af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262","6c47e834ab12aab1a3969a72628b5ea3d83de6e2dc2207b3cb6fa3cbf2cab270"],"finalReopenMs":22,"forkAdoptionMs":33,"forkHead":"0000ec593e904ad1a3fa0485361920582780e9bf3516412b376727e80c7dc5ce","forkReopenMs":17,"maxAppendMs":14,"maxCandidateAppendMs":0,"maxTemplateMs":0,"oldHead":"00006775a2347dbfe26ac90074fcd07427593420d90c879ac948f4b5dcd0e699","reconfirmedHeight":40,"specialRewardAtoms":"5000000000000","specialRewardHeight":16,"transferId":"cdb4efacad08ff3e0a51470961e26d5704ebfd5d09d69747caf5c8ae7f3ff7a4"}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 4.18s
```
