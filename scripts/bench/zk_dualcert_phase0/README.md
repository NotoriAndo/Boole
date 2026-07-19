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

## Status

Experiment harness; verdict lives in the report and in the committed
`result.sample.json` gate section. Until a GO verdict exists, the candidate is
recorded as UNVERIFIED and nothing here touches consensus code, schemas,
checker pins, or rule versions.
