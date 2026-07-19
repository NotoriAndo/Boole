"""Node-side certificate verification for the dual-cert Phase 0 harness.

THROWAWAY offline experiment code — never wired into self-test/consensus.

BUG path: an independent verifier regenerates the circuit from the seed and
checks (1) the submitted witness satisfies EVERY constraint (public-input pins
included) and (2) the submitted output actually differs from the reference
output. Cost is O(total literals) — this is the consensus-side cost model.

SAFE path: the verifier regenerates the same canonical D(seed) CNF and checks
the submitted LRAT proof against it (native checker here; the pinned Lean
`Std.Tactic.BVDecide.LRAT.Checker` wiring lives in lean_lrat.py). A Python
mock alone is never treated as acceptance — the experiment records both.
"""
from __future__ import annotations

import time
from dataclasses import dataclass

from gen import Params, generate


@dataclass
class BugVerdict:
    accepted: bool
    reason: str
    wall_s: float


def verify_bug(seed: str, params: Params, witness: list[int]) -> BugVerdict:
    """`witness` is w[0..n] with index 0 unused (DIMACS-style)."""
    t0 = time.perf_counter()
    c = generate(seed, params)
    if len(witness) != c.n_vars + 1:
        return BugVerdict(False, "bad witness length", time.perf_counter() - t0)
    if any(b not in (0, 1) for b in witness[1:]):
        return BugVerdict(False, "non-boolean witness value", time.perf_counter() - t0)
    for cl in c.clauses:
        if not any((witness[abs(l)] == 1) == (l > 0) for l in cl):
            return BugVerdict(
                False, "constraint violated", time.perf_counter() - t0
            )
    out = [witness[v] for v in c.output_vars]
    if out == c.ref_output:
        return BugVerdict(
            False, "output equals reference output", time.perf_counter() - t0
        )
    return BugVerdict(True, "ok", time.perf_counter() - t0)
