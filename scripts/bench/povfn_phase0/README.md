# PoVFN Phase 0-A — kernel-check-in-zkVM feasibility harness

Throwaway offline experiment (operator directive 2026-07-19). Core question:
*can Boole's actual pinned Lean verification be compressed into a small ZK
proof at block cadence?*

**Result (2026-07-19): `REDESIGN — PoVFN Phase 0-A` (preliminary).**
Technical execution, compatibility, and binding all succeeded; commodity-CPU
proving of the 244M-cycle full Lean kernel check exceeded the 60s block
budget by a large margin and was cancelled by operator decision (recorded as
`operator_cancelled_cpu_budget`, NOT a proof failure and NOT a cryptographic
NO-GO). Full narrative: `local-docs/povfn-phase0-a-kernel-zkvm-report.md`;
committed numbers: `result.sample.json`.

Key measured facts:
- Real fixture proof (v1-lenbound, seed from the runtime-smoke fixture):
  closure export 364 KB / 206 decls; native nanoda kernel check 8.7 ms;
  pinned `boole_check` 0.47 s; toolchain `leanchecker` 0.25 s — verdicts
  agree on every valid/tampered case (0 false accepts).
- In-zkVM (RISC Zero 3.0.6) guest runs the full kernel check + statement
  binding: 244,211,417 cycles / 276 segments / 3.09 s execution; journal
  commits 9 binding fields + statement structural hash; accepted=true.
- Large synthetic band: 3.45B cycles (14x). CPU composite proving of the
  Real band: >= 3,690 s lower bound (cancelled incomplete) vs the 60 s
  block budget with k_max=4 shares/block.
- K-stage vs P-stage: elaboration stays a host step (deterministic,
  re-computable, 0.36 s); the ZK statement covers "exported term proves the
  bound statement in the allowed-axiom environment", never "this .lean
  source was accepted".

**Not** wired into self-test/consensus. No consensus code, checker pin,
schema, or testnet changes. Local tools only; no paid APIs. Closed local
measurement — not a public performance/mining claim.

## Layout

| path | role |
|---|---|
| `PINS.md` | pinned tools, commits, local patches, build recipes |
| `derive_module/` | seed -> canonical module/statement/canon (uses boole-core, same code path as the node re-verifier) |
| `a1_differential.py` | A1 driver: 3-judge differential + P-stage binding + tamper matrix |
| `zkguest/` | RISC Zero workspace: guest (nanoda kernel check + binding journal), host driver, shared `stmt_hash` crate + CLI |
| `a2_bench.py` | full A2/A3 matrix driver — NOT run (superseded by the operator cancellation); kept for a future GPU/optimized rerun |
| `result.sample.json` | committed run record (A1 + A2 + cancellation) |
| `vendor/`, `work/` | gitignored: tool clones and run artifacts (see PINS.md to reproduce) |

## Reproduce

```bash
# 1. vendor tools per PINS.md (lean4export @ pinned commit + toolchain
#    override, nanoda_lib @ pinned commit + 2 recorded patches)
# 2. copy lean/checker -> work/checker-export (the consensus dir is never
#    modified), lake build
python3 a1_differential.py --out /tmp/a1.json
cd zkguest && cargo build --release
./target/release/host ../work/checker-export/ProofReal.ndjson \
  ../work/binding-real.json execute <expected_stmt_hash>
```
