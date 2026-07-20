# zk-proof-or-refute.v0 — Phase 0 harness

Throwaway offline experiment (operator prereg 2026-07-20). Tests whether
Boole-generated ZK statements that an LLM must PROVE or REFUTE form a sound
Base family. Not wired into self-test/consensus. No GPU/zkVM. Local tools only.

**Result: `NO-GO — zk-proof-or-refute.v0`** (candidate-limited). Fired at S2
(automation shortcut) in the pre-registered kill order S0→S1→**S2**. Fixed
automation (omega/simp/decide + off-chain enumeration for counterexamples, all
prereg-allowed) solves 92% of 60 problems and determines the true/false label
for 100%. Root cause: a protocol-owned *small* ZK library over bounded-domain
arithmetic (bits, small ranges, small R1CS) is the home turf of decision
procedures; escaping automation needs unbounded/nonlinear structure or nested
existentials, both excluded by the frozen v0 grammar. S3–S9 recorded as
`not_run_due_to_preregistered_early_kill`. Report:
`../../../local-docs/zk-proof-or-refute-phase0-report.md`.

## Files

| path | role |
|---|---|
| `zklib/` | protocol-owned Lean library (Fp/bits/range/boolean/poly/R1CS) + 20 kernel-checked base theorems (pinned v4.29.1; consensus `lean/checker` untouched) |
| `generator.py` | deterministic seed→statement mutation engine (no label/proof/cex embedded) |
| `oracle.py` | independent brute-force ground-truth label (MEASUREMENT ONLY) |
| `verify.py` | Lean verification under real intake token discipline + pinned budget |
| `automation.py` | fixed AUTO portfolio (prove + enumerate-refute) and S1 structural attacker |
| `run_s0_s2.py` | S0/S1/S2 runner → JSON |

## Reproduce

```bash
cd zklib && ~/.elan/bin/lake build          # kernel-check the 20 base theorems
cd .. && python3 run_s0_s2.py --prefix cal --per-band 20 --out /tmp/por.json
```

The LLM arms (LOCAL_LLM gemma4:26b, FRONTIER claude-fable-5=this CLI) and the
S3–S9 gates were prepared but not run: the S2 early kill stops the kill order.
Closed local measurement — not a public performance/mining claim.
