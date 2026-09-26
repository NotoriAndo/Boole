# Native first-halving recovery qualification — 2026-09-23

Status: **PASS — actual first-halving recovery and fixed resource criteria.**
The first functional run passed but its OS memory instrumentation failed; the
single infrastructure retry below supplies the missing resource measurement.

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

## Actual-height executions — functional PASS, then complete PASS

Both executions used clean source `b38b8320ef08e4153c71448d63b4973d18de1fce`, tree
`dda76c2414cc1a303a2647c83ecca9782d4f53e4` and the exact same debug executable
SHA-256 `ee8375539c50b71fc36a085af12039425be399b0c7d2dbfb0cb13e1e914e32f4`.
Those identities were checked before and after each run. No other build or
test load was intentionally run concurrently with either measurement. Fixed
criteria from `11ec566` were unchanged. A separate checkout was used to prepare
an HTTP shutdown regression without changing this measured source/executable.

### First execution — functional PASS; OS RSS unavailable

The first run, approximately 13:59–14:13 UTC, passed every functional, accounting,
file-preservation and in-test time criterion in 871.964s (harness/OS wall 871.97s).
Recent-fork adoption took 379ms; fork/final independent reopen took 5.546/5.558s
and final independent audit 5.556s. Maximum durable append was 17ms; measured
templates and candidate append were below one millisecond.

The surrounding `/usr/bin/time -l` then failed with
`sysctl kern.clockrate: Operation not permitted`, exit 1, and supplied no RSS.
This was an instrumentation failure, not a failing assertion or a product crash.
It was **not** accepted as a complete resource qualification. A separate no-load
`time -l true` confirmed the same sandbox failure; the same command with the
approved local instrumentation permission succeeded. Source/executable identity
checks were then repeated explicitly because the failed wrapper prevented its
trailing `&&` checks. The raw failed instrumentation output is retained below.

### Infrastructure retry 1 — complete PASS

After the instrumentation permission check, the same source/executable and
unchanged scenario ran once more, approximately 14:15–14:29 UTC. It passed in
876.337s (harness/OS wall 876.34s; user 830.03s, system 3.07s), with OS peak RSS
55,443,456 bytes (**52.875MiB**), below the fixed 1GiB criterion. The test,
measurement wrapper and final identity checks all exited 0. Recent-fork adoption
was 384ms, fork/final independent reopen 5.568/5.583s and audit 5.580s. Maximum
durable append was 16ms; templates and candidate append remained below 1ms.

Both runs independently observed the actual height-9,999 reward of
5,000,000,000,000 atoms and height-10,000 reward of 2,500,000,000,000 atoms.
The old height-10,000 reward and its height-10,010 transfer disappeared on the
greater-work branch. The now-unfunded original signature was refused without
changing files or readiness. After the new height-10,014 reward matured, the
same original transfer ID was confirmed exactly once at height 10,024. Repeated
submission before and after restart made no additional debit or nonce change.

The common final head is
`0000acf86dc13c5339c34015ec3b31870ea7c72ee91d19bd676a6dc2a9bee478`,
and original/reconfirmed transfer ID is
`8880a156555e30c53c67070981308656838b886c4c6d52b333d2e667fe00e752`.
Each final audit reports 10,024 blocks, 7,847,044 history bytes, three balance
entries, one nonce entry, one confirmed one-tBOOLE transfer, 1,000 fee atoms
and empty pending. Issued/balance atoms are `50057500000000000`, locked atoms
`25000000000000`, and spendable atoms `50032500000000000`. All per-account,
maturity, history/pending digest and unchanged-manifest checks passed. Canonical
history bytes/digest match across both runs; each independently created manifest
has its own creation timestamp and is only required to remain unchanged within
its run, not to match the other run's manifest.

This low-account, mostly empty-block history is materially different from the
131,073-account/full-transfer qualifications. Its smaller RSS/replay duration is
not a general performance improvement or qualification of the complete storage
envelope. No production Rust, monetary policy, network/genesis, signature rule,
admission or fork-choice code changed. The original signed transfer was retained
by the fixture; the node does not promise to keep an unfunded orphan in its pool.

Measurement command (the second run used the verified OS instrumentation scope):

```sh
/usr/bin/time -l /Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7 --ignored --exact actual_first_halving_fork_recovery_10024_blocks --nocapture --test-threads=1
```

### First execution and instrumentation failure — raw output

```text
b38b8320ef08e4153c71448d63b4973d18de1fce
dda76c2414cc1a303a2647c83ecca9782d4f53e4
ee8375539c50b71fc36a085af12039425be399b0c7d2dbfb0cb13e1e914e32f4  /Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7

running 1 test
test actual_first_halving_fork_recovery_10024_blocks ... halving-progress {"elapsedMs":41363,"head":"000056d1a9ce410da73e80afacead5a73ecc0aab7c6c6d45f4a6b16927560b52","height":512,"issuedAtoms":"2560000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":512,"historyBytes":399584,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":85711,"head":"0000f3f9b3ce6ff7ca9faf9e5d9301c5c841ecfad535f848267518d34bb1b459","height":1024,"issuedAtoms":"5120000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":1024,"historyBytes":799525,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":129314,"head":"000074d7f33eba738df6672a37f56aca2556be60d64e061bee6c528fbad7a25e","height":1536,"issuedAtoms":"7680000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":1536,"historyBytes":1199944,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":173378,"head":"000056d803768f7971acffc4be455af462720675ce99d3f9e7154343f8f83331","height":2048,"issuedAtoms":"10240000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":2048,"historyBytes":1600767,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":220342,"head":"000068068f1a5c1912f1add19e44afc0f32dd0711ef4f46b68736c561e0a81da","height":2560,"issuedAtoms":"12800000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":2560,"historyBytes":2001714,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":261925,"head":"0000960e85d102c614b78b8c3dc7a5078126bdc68f9fbc1da36ad8d904b48695","height":3072,"issuedAtoms":"15360000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":3072,"historyBytes":2402629,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":304798,"head":"0000fa0627f7b8129c7a44cf7e936aceca14e9066550bb94db590e513dd54a00","height":3584,"issuedAtoms":"17920000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":3584,"historyBytes":2803551,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":348442,"head":"00006208abb9a7eddf3e15ed4d8a0e23a706dd0cb4da4ff4b4c4d17cce688683","height":4096,"issuedAtoms":"20480000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":4096,"historyBytes":3204478,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":390027,"head":"0000985f166670e1aa5699c33404bab76c63a565fb81ba2211e06bbeae643f49","height":4608,"issuedAtoms":"23040000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":4608,"historyBytes":3605408,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":436484,"head":"0000e32e2b88bbf25d9abfa1d9226761337013d61986564ed04eb21c6b6c981d","height":5120,"issuedAtoms":"25600000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":5120,"historyBytes":4006354,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":480366,"head":"00005c207574f0f8845304290b63c3c418bde415a3b42b2b22898fa6d2a83d0a","height":5632,"issuedAtoms":"28160000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":5632,"historyBytes":4407290,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":526629,"head":"0000578c1096ebb4f1c17ad88f7cbacf3f34253da0c038f204b15ece46862f33","height":6144,"issuedAtoms":"30720000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":6144,"historyBytes":4808233,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":569456,"head":"000017df0a75564f391a50f4e9ec5a441693e376c3dec59d383f3e5de041b752","height":6656,"issuedAtoms":"33280000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":6656,"historyBytes":5209154,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":613077,"head":"0000428307eb93536d15d2ad52c3a82b7847e04406760747cfd54e6fdc332deb","height":7168,"issuedAtoms":"35840000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":7168,"historyBytes":5610093,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":655196,"head":"0000b5626d72b6932b586f8bcdb29ff1cf68a78ccd44549b53e77a77a04dfbdb","height":7680,"issuedAtoms":"38400000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":7680,"historyBytes":6011013,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":697180,"head":"00007db6b3d371e3c973e3cba8e5cc3410bd97e588964273c027f4051327151d","height":8192,"issuedAtoms":"40960000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":8192,"historyBytes":6411930,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":740109,"head":"0000f4bc11788685964e5a67202295dc8b47255161886f41084a0b88c5aa4b9f","height":8704,"issuedAtoms":"43520000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":8704,"historyBytes":6812845,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":783857,"head":"0000332d1676d05dd5093c730df138e88fd3a79b67f51f5eafba375fe8257d30","height":9216,"issuedAtoms":"46080000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9216,"historyBytes":7213780,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":824701,"head":"00005ffe0716a3ffaa0993949015b05dbd11a5c5c0fdbb9c22de56f07a5ff936","height":9728,"issuedAtoms":"48640000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9728,"historyBytes":7614688,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":850701,"head":"00009875251bc25980a4494bc075a545fa2dc847c7f13e798e9d523e637d9cf0","height":9999,"issuedAtoms":"49995000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9999,"historyBytes":7826905,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":850729,"head":"0000afb3a78bd152f22ea48244081ee63a0f821c0075c6d2d967d242d786cb1b","height":10000,"issuedAtoms":"49997500000000000","lastRewardAtoms":"2500000000000","resources":{"balanceEntries":2,"confirmedTransfers":0,"historyBlocks":10000,"historyBytes":7827689,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-result {"audit":{"accounting":{"balanceAtoms":"50057500000000000","issuedAtoms":"50057500000000000","lockedAtoms":"25000000000000","pendingRewardEntries":10,"spendableAtoms":"50032500000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"100000000","count":1,"feeAtoms":"1000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"0000acf86dc13c5339c34015ec3b31870ea7c72ee91d19bd676a6dc2a9bee478","height":"10024","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":1,"historyBlocks":10024,"historyBytes":7847044,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"auditMs":5556,"elapsedMs":871964,"finalFilesBlake3":["25238a9a7c6169a50723106b5f84c8708a6d91ee51dd9439fb00152bd4e1da4c","af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262","e606f964cfc07ec04b930677f739f060ceedb9b1574216941cc92f270b65c346"],"finalReopenMs":5558,"forkAdoptionMs":379,"forkHead":"0000caacc5a0757203977fc1340df9f8a75ee5f09d4d14a80d88230683add58f","forkReopenMs":5546,"maxAppendMs":17,"maxCandidateAppendMs":0,"maxTemplateMs":0,"oldHead":"0000f46560351fa43046d70711b3198d985bf2a385dc6b45a9665b738eb37b63","reconfirmedHeight":10024,"specialRewardAtoms":"2500000000000","specialRewardHeight":10000,"transferId":"8880a156555e30c53c67070981308656838b886c4c6d52b333d2e667fe00e752"}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 871.97s

      871.97 real       825.39 user         3.11 sys
time: sysctl kern.clockrate: Operation not permitted
b38b8320ef08e4153c71448d63b4973d18de1fce
dda76c2414cc1a303a2647c83ecca9782d4f53e4
ee8375539c50b71fc36a085af12039425be399b0c7d2dbfb0cb13e1e914e32f4  /Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7
```

### Infrastructure retry 1 — raw output

```text
b38b8320ef08e4153c71448d63b4973d18de1fce
dda76c2414cc1a303a2647c83ecca9782d4f53e4
ee8375539c50b71fc36a085af12039425be399b0c7d2dbfb0cb13e1e914e32f4  /Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7

running 1 test
test actual_first_halving_fork_recovery_10024_blocks ... halving-progress {"elapsedMs":41768,"head":"000056d1a9ce410da73e80afacead5a73ecc0aab7c6c6d45f4a6b16927560b52","height":512,"issuedAtoms":"2560000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":512,"historyBytes":399584,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":86532,"head":"0000f3f9b3ce6ff7ca9faf9e5d9301c5c841ecfad535f848267518d34bb1b459","height":1024,"issuedAtoms":"5120000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":1024,"historyBytes":799525,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":130624,"head":"000074d7f33eba738df6672a37f56aca2556be60d64e061bee6c528fbad7a25e","height":1536,"issuedAtoms":"7680000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":1536,"historyBytes":1199944,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":175065,"head":"000056d803768f7971acffc4be455af462720675ce99d3f9e7154343f8f83331","height":2048,"issuedAtoms":"10240000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":2048,"historyBytes":1600767,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":222300,"head":"000068068f1a5c1912f1add19e44afc0f32dd0711ef4f46b68736c561e0a81da","height":2560,"issuedAtoms":"12800000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":2560,"historyBytes":2001714,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":264270,"head":"0000960e85d102c614b78b8c3dc7a5078126bdc68f9fbc1da36ad8d904b48695","height":3072,"issuedAtoms":"15360000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":3072,"historyBytes":2402629,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":307409,"head":"0000fa0627f7b8129c7a44cf7e936aceca14e9066550bb94db590e513dd54a00","height":3584,"issuedAtoms":"17920000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":3584,"historyBytes":2803551,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":351356,"head":"00006208abb9a7eddf3e15ed4d8a0e23a706dd0cb4da4ff4b4c4d17cce688683","height":4096,"issuedAtoms":"20480000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":4096,"historyBytes":3204478,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":393107,"head":"0000985f166670e1aa5699c33404bab76c63a565fb81ba2211e06bbeae643f49","height":4608,"issuedAtoms":"23040000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":4608,"historyBytes":3605408,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":439740,"head":"0000e32e2b88bbf25d9abfa1d9226761337013d61986564ed04eb21c6b6c981d","height":5120,"issuedAtoms":"25600000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":5120,"historyBytes":4006354,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":483758,"head":"00005c207574f0f8845304290b63c3c418bde415a3b42b2b22898fa6d2a83d0a","height":5632,"issuedAtoms":"28160000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":5632,"historyBytes":4407290,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":530198,"head":"0000578c1096ebb4f1c17ad88f7cbacf3f34253da0c038f204b15ece46862f33","height":6144,"issuedAtoms":"30720000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":6144,"historyBytes":4808233,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":573177,"head":"000017df0a75564f391a50f4e9ec5a441693e376c3dec59d383f3e5de041b752","height":6656,"issuedAtoms":"33280000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":6656,"historyBytes":5209154,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":616893,"head":"0000428307eb93536d15d2ad52c3a82b7847e04406760747cfd54e6fdc332deb","height":7168,"issuedAtoms":"35840000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":7168,"historyBytes":5610093,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":658983,"head":"0000b5626d72b6932b586f8bcdb29ff1cf68a78ccd44549b53e77a77a04dfbdb","height":7680,"issuedAtoms":"38400000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":7680,"historyBytes":6011013,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":701073,"head":"00007db6b3d371e3c973e3cba8e5cc3410bd97e588964273c027f4051327151d","height":8192,"issuedAtoms":"40960000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":8192,"historyBytes":6411930,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":744242,"head":"0000f4bc11788685964e5a67202295dc8b47255161886f41084a0b88c5aa4b9f","height":8704,"issuedAtoms":"43520000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":8704,"historyBytes":6812845,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":788053,"head":"0000332d1676d05dd5093c730df138e88fd3a79b67f51f5eafba375fe8257d30","height":9216,"issuedAtoms":"46080000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9216,"historyBytes":7213780,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":828942,"head":"00005ffe0716a3ffaa0993949015b05dbd11a5c5c0fdbb9c22de56f07a5ff936","height":9728,"issuedAtoms":"48640000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9728,"historyBytes":7614688,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":854989,"head":"00009875251bc25980a4494bc075a545fa2dc847c7f13e798e9d523e637d9cf0","height":9999,"issuedAtoms":"49995000000000000","lastRewardAtoms":"5000000000000","resources":{"balanceEntries":1,"confirmedTransfers":0,"historyBlocks":9999,"historyBytes":7826905,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-progress {"elapsedMs":855016,"head":"0000afb3a78bd152f22ea48244081ee63a0f821c0075c6d2d967d242d786cb1b","height":10000,"issuedAtoms":"49997500000000000","lastRewardAtoms":"2500000000000","resources":{"balanceEntries":2,"confirmedTransfers":0,"historyBlocks":10000,"historyBytes":7827689,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":0,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0}}
halving-result {"audit":{"accounting":{"balanceAtoms":"50057500000000000","issuedAtoms":"50057500000000000","lockedAtoms":"25000000000000","pendingRewardEntries":10,"spendableAtoms":"50032500000000000","supplyCapAtoms":"100000000000000000"},"confirmedTransfers":{"amountAtoms":"100000000","count":1,"feeAtoms":"1000"},"genesisHash":"933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1","headHash":"0000acf86dc13c5339c34015ec3b31870ea7c72ee91d19bd676a6dc2a9bee478","height":"10024","networkId":"boole-native-testnet-1","resources":{"balanceEntries":3,"confirmedTransfers":1,"historyBlocks":10024,"historyBytes":7847044,"historyLimitBlocks":100000,"historyLimitBytes":268435456,"nonceEntries":1,"pendingBytes":0,"pendingLimitBytes":5242880,"pendingLimitTransfers":512,"pendingTransfers":0},"schema":"boole.native.audit.v1","scope":"confirmed_canonical"},"auditMs":5580,"elapsedMs":876337,"finalFilesBlake3":["25238a9a7c6169a50723106b5f84c8708a6d91ee51dd9439fb00152bd4e1da4c","af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262","22e08820d3ec46756f3e4847eec66b0cd2d1c2495e74b8a49fbd74754370bb2a"],"finalReopenMs":5583,"forkAdoptionMs":384,"forkHead":"0000caacc5a0757203977fc1340df9f8a75ee5f09d4d14a80d88230683add58f","forkReopenMs":5568,"maxAppendMs":16,"maxCandidateAppendMs":0,"maxTemplateMs":0,"oldHead":"0000f46560351fa43046d70711b3198d985bf2a385dc6b45a9665b738eb37b63","reconfirmedHeight":10024,"specialRewardAtoms":"2500000000000","specialRewardHeight":10000,"transferId":"8880a156555e30c53c67070981308656838b886c4c6d52b333d2e667fe00e752"}
ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 876.34s

      876.34 real       830.03 user         3.07 sys
            55443456  maximum resident set size
                   0  average shared memory size
                   0  average unshared data size
                   0  average unshared stack size
                4404  page reclaims
                   0  page faults
                   0  swaps
                   0  block input operations
                   0  block output operations
                   0  messages sent
                   0  messages received
                   0  signals received
               40991  voluntary context switches
                5902  involuntary context switches
      12080266958588  instructions retired
       3629233443813  cycles elapsed
            39453128  peak memory footprint
b38b8320ef08e4153c71448d63b4973d18de1fce
dda76c2414cc1a303a2647c83ecca9782d4f53e4
ee8375539c50b71fc36a085af12039425be399b0c7d2dbfb0cb13e1e914e32f4  /Users/seoyong/projects/Boole/target/debug/deps/native_halving_recovery-e1a82046755316b7
```
