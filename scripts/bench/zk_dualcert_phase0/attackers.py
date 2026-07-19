"""Generator-omniscient structural attackers for the dual-cert harness (S1).

THROWAWAY offline experiment code — never wired into self-test/consensus.

Threat model: the attacker knows the generator source, the seed, and the full
generation transcript — it can (and does) regenerate the circuit, the planted
reference witness w*, and all structural metadata for free. Each attack is a
purpose-built shortcut that a generic SAT/SMT solver does not know:

  * propagation      — BCP on D; a BCP conflict proves SAFE outright.
  * gauss            — GF(2) elimination over the parity/unit subsystem; if the
                       linear part alone forces every output bit to its
                       reference value, D is UNSAT (SAFE) without search.
  * free-output      — dependency scan: an output variable that appears in no
                       circuit constraint flips into an instant BUG witness.
  * local-flip       — start FROM w* (the attacker has it), pin one output bit
                       to the flipped value, and repair with deterministic
                       WalkSAT; exploits that instances are planted around w*.
  * warm-start cache — reuse of previously found BUG witnesses from other
                       seeds of the same band as repair starting points
                       (certificate-reuse/cache attack).

A SAFE decision returned here is *sound* (conflict or forced-output proof);
a BUG decision always carries a witness that the caller re-verifies.
"""
from __future__ import annotations

import time
from dataclasses import dataclass

from gen import Circuit
from xof import Xof


@dataclass
class AttackResult:
    name: str
    decided: str | None  # "BUG" | "SAFE" | None
    wall_s: float
    witness: list[int] | None = None
    note: str = ""


def _bcp(clauses: list[tuple[int, ...]]) -> tuple[str, dict[int, int]]:
    """Unit propagation. Returns ("conflict"|"open", forced-assignment)."""
    assign: dict[int, int] = {}
    changed = True
    while changed:
        changed = False
        for cl in clauses:
            unassigned = []
            satisfied = False
            for lit in cl:
                v = abs(lit)
                if v in assign:
                    if (assign[v] == 1) == (lit > 0):
                        satisfied = True
                        break
                else:
                    unassigned.append(lit)
            if satisfied:
                continue
            if not unassigned:
                return "conflict", assign
            if len(unassigned) == 1:
                lit = unassigned[0]
                assign[abs(lit)] = 1 if lit > 0 else 0
                changed = True
    return "open", assign


def attack_propagation(c: Circuit) -> AttackResult:
    t0 = time.perf_counter()
    status, assign = _bcp(c.d_clauses())
    wall = time.perf_counter() - t0
    if status == "conflict":
        return AttackResult("propagation", "SAFE", wall, note="BCP conflict")
    if len(assign) == c.n_vars:
        w = [0] * (c.n_vars + 1)
        for v, b in assign.items():
            w[v] = b
        return AttackResult("propagation", "BUG", wall, witness=w,
                            note="BCP fixed all variables")
    return AttackResult("propagation", None, wall,
                        note=f"open, {len(assign)}/{c.n_vars} forced")


def propagation_safe_lrat(c: Circuit) -> str | None:
    """If BCP alone refutes D(seed), emit the LRAT certificate directly.

    This measures whether a structural attacker gets the SAFE certificate for
    (near) free: the firing order of unit clauses is itself a valid LRAT hint
    chain for a single empty-clause addition. Returns LRAT text or None.
    """
    clauses = c.d_clauses()
    assign: dict[int, int] = {}
    trace: list[int] = []
    changed = True
    while changed:
        changed = False
        for idx, cl in enumerate(clauses):
            unassigned = []
            satisfied = False
            for lit in cl:
                v = abs(lit)
                if v in assign:
                    if (assign[v] == 1) == (lit > 0):
                        satisfied = True
                        break
                else:
                    unassigned.append(lit)
            if satisfied:
                continue
            if not unassigned:
                trace.append(idx + 1)  # conflict clause, 1-based DIMACS id
                step_id = len(clauses) + 1
                hints = " ".join(str(h) for h in trace)
                return f"{step_id} 0 {hints} 0\n"
            if len(unassigned) == 1:
                lit = unassigned[0]
                assign[abs(lit)] = 1 if lit > 0 else 0
                trace.append(idx + 1)
                changed = True
    return None


def attack_gauss(c: Circuit) -> AttackResult:
    """GF(2) elimination over parity constraints + BCP-forced units."""
    t0 = time.perf_counter()
    _, forced = _bcp(c.clauses)  # circuit-only units (public pins etc.)

    # Rows: bitmask over vars plus rhs. Parity groups are read from the
    # generator metadata (kinds) — the attacker is allowed to.
    rows: list[tuple[int, int]] = []
    seen_xor = set()
    for kind in c.kinds:
        if kind[0] == "xor":
            key = kind[1:]
            if key in seen_xor:
                continue
            seen_xor.add(key)
            a, b, x3, parity = key
            rows.append(((1 << a) | (1 << b) | (1 << x3), parity))
    for v, bit in forced.items():
        rows.append((1 << v, bit))

    # Gaussian elimination (row = bitmask over var ids, rhs bit).
    pivots: dict[int, tuple[int, int]] = {}
    contradiction = False
    for mask, rhs in rows:
        m, r = mask, rhs
        while m:
            top = m.bit_length() - 1
            if top in pivots:
                pm, pr = pivots[top]
                m ^= pm
                r ^= pr
            else:
                pivots[top] = (m, r)
                m, r = 0, 0
        if r == 1:
            contradiction = True
            break
    if contradiction:
        # Circuit constraints alone contradictory — impossible for a planted
        # instance; record as anomaly rather than a verdict.
        return AttackResult(
            "gauss", None, time.perf_counter() - t0, note="anomaly: circuit UNSAT"
        )

    # Back-substitute to reduced row echelon form (ascending pivot order:
    # lower-pivot rows are already fully reduced when a higher row uses them).
    for t in sorted(pivots):
        m, r = pivots[t]
        while True:
            mm = m & ~(1 << t)
            eliminated = False
            while mm:
                bt = mm.bit_length() - 1
                if bt in pivots:
                    pm, pr = pivots[bt]
                    m ^= pm
                    r ^= pr
                    eliminated = True
                    break
                mm &= (1 << bt) - 1
            if not eliminated:
                break
        pivots[t] = (m, r)

    # A variable is linearly forced iff its reduced pivot row is {v} = r.
    forced_lin: dict[int, int] = {}
    for top, (m, r) in pivots.items():
        if bin(m).count("1") == 1:  # not int.bit_count(): needs py>=3.10
            forced_lin[top] = r

    outs_forced_ref = all(
        forced_lin.get(v) == c.ref_witness[v] for v in c.output_vars
    )
    wall = time.perf_counter() - t0
    if c.output_vars and outs_forced_ref:
        return AttackResult(
            "gauss", "SAFE", wall,
            note="linear subsystem forces output to reference",
        )
    n_forced_out = sum(1 for v in c.output_vars if v in forced_lin)
    return AttackResult(
        "gauss", None, wall,
        note=f"{len(forced_lin)} vars linearly forced ({n_forced_out} outputs)",
    )


def attack_free_output(c: Circuit) -> AttackResult:
    t0 = time.perf_counter()
    occurs: set[int] = set()
    for cl in c.clauses:
        for lit in cl:
            occurs.add(abs(lit))
    for v in c.output_vars:
        if v not in occurs:
            w = list(c.ref_witness)
            w[v] ^= 1
            return AttackResult(
                "free-output", "BUG", time.perf_counter() - t0, witness=w,
                note=f"output var {v} unconstrained",
            )
    return AttackResult("free-output", None, time.perf_counter() - t0,
                        note="all outputs constrained")


def _occ_index(c: Circuit) -> dict[int, list[int]]:
    occ: dict[int, list[int]] = {}
    for idx, cl in enumerate(c.clauses):
        for lit in cl:
            occ.setdefault(abs(lit), []).append(idx)
    return occ


def _repair(
    c: Circuit,
    occ: dict[int, list[int]],
    start: list[int],
    pinned: set[int],
    max_flips: int,
    rng: Xof,
    deadline: float,
) -> list[int] | None:
    """Deterministic WalkSAT restricted to non-pinned variables."""
    w = list(start)
    clauses = c.clauses

    def sat(idx: int) -> bool:
        return any((w[abs(l)] == 1) == (l > 0) for l in clauses[idx])

    violated = {i for i in range(len(clauses)) if not sat(i)}
    for step in range(max_flips):
        if not violated:
            return w
        if step % 64 == 0 and time.perf_counter() > deadline:
            return None
        cl_idx = min(violated)  # deterministic pick
        candidates = [
            abs(l) for l in clauses[cl_idx] if abs(l) not in pinned
        ]
        if not candidates:
            return None  # clause only over pinned vars — dead end
        # Greedy break-count with an epsilon of noise (deterministic XOF).
        if rng.randbelow(10) == 0:
            v = candidates[rng.randbelow(len(candidates))]
        else:
            best_v, best_break = None, None
            for v in candidates:
                w[v] ^= 1
                brk = sum(1 for i in occ.get(v, ()) if not sat(i))
                w[v] ^= 1
                if best_break is None or brk < best_break:
                    best_v, best_break = v, brk
            v = best_v
        w[v] ^= 1
        for i in occ.get(v, ()):
            if sat(i):
                violated.discard(i)
            else:
                violated.add(i)
    return None


def attack_local_flip(
    c: Circuit, max_flips: int = 3000, budget_s: float = 15.0
) -> AttackResult:
    """Start from the planted witness, force one output bit off-reference,
    repair the rest. The strongest 'knows-the-generator' BUG search."""
    t0 = time.perf_counter()
    deadline = t0 + budget_s
    rng = Xof(c.seed, "attack-local-flip")
    occ = _occ_index(c)
    pinned = set(c.public_vars)
    for v in c.output_vars:
        if time.perf_counter() > deadline:
            break
        start = list(c.ref_witness)
        start[v] ^= 1
        got = _repair(c, occ, start, pinned | {v}, max_flips, rng, deadline)
        if got is not None:
            return AttackResult(
                "local-flip", "BUG", time.perf_counter() - t0, witness=got,
                note=f"flipped output {v}",
            )
    return AttackResult("local-flip", None, time.perf_counter() - t0,
                        note="repair budget exhausted")


def attack_warm_cache(
    c: Circuit,
    cached_witnesses: list[list[int]],
    max_flips: int = 800,
    budget_s: float = 10.0,
) -> AttackResult:
    """Certificate-reuse attack: replay BUG witnesses from other seeds of the
    same band, directly and as repair starting points."""
    t0 = time.perf_counter()
    deadline = t0 + budget_s
    rng = Xof(c.seed, "attack-warm-cache")
    occ = _occ_index(c)
    pinned = set(c.public_vars)
    diff = c.diff_clause()
    for w0 in cached_witnesses:
        if len(w0) != c.n_vars + 1 or time.perf_counter() > deadline:
            continue
        # direct replay
        if all(
            any((w0[abs(l)] == 1) == (l > 0) for l in cl) for cl in c.clauses
        ) and any((w0[abs(l)] == 1) == (l > 0) for l in diff):
            return AttackResult(
                "warm-cache", "BUG", time.perf_counter() - t0, witness=list(w0),
                note="foreign witness replayed verbatim",
            )
        # repair from the foreign witness with public pins restored
        start = list(w0)
        for v in c.public_vars:
            start[v] = c.ref_witness[v]
        got = _repair(c, occ, start, pinned, max_flips, rng, deadline)
        if got is not None and any(
            (got[abs(l)] == 1) == (l > 0) for l in diff
        ):
            return AttackResult(
                "warm-cache", "BUG", time.perf_counter() - t0, witness=got,
                note="foreign witness repaired",
            )
    return AttackResult("warm-cache", None, time.perf_counter() - t0,
                        note=f"{len(cached_witnesses)} cached tried")
