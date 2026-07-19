# zk-circuit-uniqueness-dual-cert.v0 — Phase 0 offline experiment

Throwaway research harness for the operator-directed successor Base-family
candidate (2026-07-19, tasks/todo.md "ZK-DC" section). It answers one question
before any production code is written: *is "is the circuit's output unique for
the given input?" a sound dual-certificate mining primitive?*

- **BUG path** — submit an alternative witness: same public input, all
  constraints satisfied, different output (a counterexample proving the
  circuit underconstrained).
- **SAFE path** — submit an LRAT certificate that
  `D(seed) = Circuit(public_input, w) AND output(w) != reference_output`
  is UNSAT, checked by the pinned Lean v4.29.1
  `Std.Tactic.BVDecide.LRAT.Checker` (`check lratProof cnf = true → cnf.Unsat`)
  AND an independent native checker. A Python-mock pass alone never counts.

Design restrictions (enforced by construction, tested in `test_selfcheck.py`):
the generator plants only the reference witness; no deleted-constraint
locations, no planted alternative witness, no seed-recoverable answer label,
no mutation trace. The BUG/SAFE answer is emergent, and the threat model is a
generator-omniscient attacker.

**Not** wired into `self-test`, consensus, or any production path. No paid
API, no network at run time, no model calls. This is a "ZK circuit safety
adjudication" workload experiment — not "real ZK proof mining"; base-lane
value is limited to calibration + circuit-safety corpus + liveness.

## Run

```bash
./run.sh                  # self-check + quick S0-S7 run -> temp JSON
./run.sh --write-sample   # explicit: also refresh committed result.sample.json
```

## Files

| file | role |
|---|---|
| `xof.py` | deterministic BLAKE2b XOF randomness |
| `gen.py` | seed -> relational circuit + planted reference witness (P0-A) |
| `encode.py` | canonical circuit bytes, canonical DIMACS for D(seed), S7 canon |
| `verify.py` | BUG-certificate verifier (regenerate + check + output-differs) |
| `lrat_native.py` | independent native LRAT checker (RUP + RAT + deletions) |
| `leanchecker/` | pinned Lean v4.29.1 project wrapping `Std.Tactic.BVDecide.LRAT.check` |
| `lean_lrat.py` | build/run wiring + wall/peak-RSS measurement for the Lean checker |
| `attackers.py` | generator-omniscient structural attacks (S1) incl. free BCP LRAT |
| `solvers.py` | CaDiCaL (`--lrat`), Kissat, Z3 portfolio; timeout == UNDECIDED |
| `experiment.py` | S0–S7 gate runner -> raw JSON |
| `test_selfcheck.py` | harness self-check (determinism, totality, checker soundness) |
| `result.sample.json` | committed sample run (refresh only via `--write-sample`) |

## Gates (writeup: `../../../local-docs/zk-dualcert-phase0-report.md`)

- **S0** determinism (byte-identical circuit/CNF across processes) + totality
  (exhaustive tiny-band ground truth: exactly one of BUG/SAFE, certificates
  agree with brute force)
- **S1** shortcut resistance — portfolio attacker (structural attacks +
  solvers); all bands < 1s ⇒ no-go
- **S2** solve-or-prove / verify asymmetry ≥ 100× on each certified path
- **S3** one-axis-at-a-time difficulty control (median/p90/p95/p99 + timeouts)
- **S4** BUG:SAFE balance / liveness
- **S5** min-of-N bootstrap cherry-picking
- **S6** certificate bytes + Lean verification cost
- **S7** `canon = f(seed, outcome_tag, certificate_bytes)` binding prototype

## Headline result (2026-07-19 full run, 384 seeds / 32 bands)

**NO-GO — candidate-specific** (`zk-circuit-uniqueness-dual-cert.v0`, P0-A;
P0-B not started per the "all P0-A gates must pass first" rule).

- S0 PASS (byte-identical regeneration; exhaustive tiny-band ground truth
  58 BUG / 22 SAFE, certificates always agree with brute force).
- Scoped positive: the zk_phase0 planted-freedom collapse did NOT reproduce —
  at boundary densities structural attackers decide only 42–75% and SAT
  solvers must do real search. Emergent-answer generation fixes that leak.
- S2 FAIL: BUG search/verify 0.88x median (verify has an O(n) regeneration
  floor); SAFE prove/Lean-verify 3–10x. Target was >= 100x on each path —
  CDCL solve time and LRAT verify time are both ~linear in the same
  resolution-proof size, so "hard to solve" and "cheap to verify" cannot
  coexist in an LRAT-certified SAFE path.
- S3 FAIL: all six per-axis sweeps stay in the ms range; hardness appears
  only at the pure planted 3-SAT phase boundary (random k-SAT hardness, i.e.
  PoW-shaped, not circuit-safety reasoning) where it is fused to S6 blowup
  and timeout tails.
- S5 FAIL: bootstrap min-of-1000 seed grinding yields ~1.5 ms BUG instances
  in every band (gain up to 270x, argmin 100% BUG); emergent answers leave no
  in-family control.
- S6 FAIL: in the only hard regime, SAFE LRAT certificates reach 44–72 MB;
  pinned Lean verification costs 1.1–1.8 s wall / ~330 MB peak RSS per seed.
- S7 PASS (canon binding prototype, 379/379).

Verdict + full numbers: `../../../local-docs/zk-dualcert-phase0-report.md`
(raw JSON preserved alongside it). The candidate is retired; nothing here
touches consensus code, schemas, checker pins, or rule versions. Closed local
measurement only — not a public mining/benchmark claim.
