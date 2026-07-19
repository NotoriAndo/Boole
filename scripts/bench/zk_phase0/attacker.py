"""The attacker (== a rational miner) for the ZK.0 spike.

Threat model per L1 master §ZK R1 / ADR-0017: the attacker has FULL knowledge of
the open-source, deterministic generator. So the attacker is handed the exact
post-mutation constraint system and the honest public inputs, and uses a real
SMT solver (Z3) to search for an underconstraint certificate -- a witness that
satisfies every constraint but deviates on a public output. If Z3 finds one fast
regardless of the difficulty band, the family is just an obfuscated PoW (no-go).

We deliberately do NOT hand-roll a weak search; that would make the problem look
harder than it is, which is exactly the self-deception R1 warns against.
"""
from __future__ import annotations

import time
from dataclasses import dataclass

import z3

import circuit as C


@dataclass
class SolveResult:
    status: str        # 'sat' | 'unsat' | 'unknown'
    seconds: float
    certificate_valid: bool  # for 'sat', did the native verifier confirm it?


def _lin_expr(lc, zvars):
    terms = []
    for var, coeff in lc.items():
        if var == 0:
            terms.append(z3.IntVal(coeff))
        else:
            terms.append(z3.IntVal(coeff) * zvars[var])
    if not terms:
        return z3.IntVal(0)
    e = terms[0]
    for t in terms[1:]:
        e = e + t
    return e


def solve(circ: C.Circuit, timeout_ms: int) -> SolveResult:
    p = circ.p
    zvars = [None] * circ.num_vars
    for i in range(1, circ.num_vars):
        zvars[i] = z3.Int(f"z{i}")

    s = z3.Solver()
    s.set("timeout", timeout_ms)

    # domain: every wire in [0, p)
    for i in range(1, circ.num_vars):
        s.add(zvars[i] >= 0, zvars[i] < p)

    # constant wire and public inputs pinned to honest values
    for v in circ.public_inputs:
        s.add(zvars[v] == circ.honest_witness[v] % p)

    # constraints: (A*B - C) % p == 0
    for con in circ.constraints:
        a = _lin_expr(con.a, zvars)
        b = _lin_expr(con.b, zvars)
        c = _lin_expr(con.c, zvars)
        s.add((a * b - c) % p == 0)

    # deviation objective: differ from honest on at least one public output
    devs = [zvars[o] != circ.honest_witness[o] % p for o in circ.public_outputs]
    s.add(z3.Or(devs))

    t0 = time.perf_counter()
    res = s.check()
    elapsed = time.perf_counter() - t0

    if res == z3.sat:
        m = s.model()
        z = [1] + [0] * (circ.num_vars - 1)
        for i in range(1, circ.num_vars):
            val = m[zvars[i]]
            z[i] = (val.as_long() if val is not None else 0) % p
        valid = C.is_underconstraint_certificate(circ, z)
        return SolveResult("sat", elapsed, valid)
    if res == z3.unsat:
        return SolveResult("unsat", elapsed, False)
    return SolveResult("unknown", elapsed, False)


def _out_var(con) -> int:
    # In a feed-forward gadget, the freshly allocated output wire is the
    # highest-index variable appearing in C (inputs were allocated earlier).
    return max(con.c.keys())


def propagation_attack(circ: C.Circuit) -> SolveResult:
    """The shortcut a rational miner actually uses on a feed-forward circuit:
    find the freed wire(s), perturb them, and forward-re-evaluate every kept
    gadget so all downstream constraints stay satisfied by construction. No SMT
    solver -- O(#constraints). This is the honest measure of how easy the family
    is when the attacker knows it is a gadget chain.
    """
    p = circ.p
    t0 = time.perf_counter()

    determined = set()
    for con in circ.constraints:
        determined.add(_out_var(con))
    pub_in = set(circ.public_inputs)
    free = [v for v in range(1, circ.num_vars)
            if v not in determined and v not in pub_in]

    z = list(circ.honest_witness)
    # perturb every free wire away from its honest value
    for v in free:
        z[v] = (z[v] + 1) % p

    # forward re-evaluation in wire-allocation order (topological for a DAG)
    order = sorted(circ.constraints, key=_out_var)

    def lc(d, skip=None):
        acc = 0
        for var, coeff in d.items():
            if var == skip:
                continue
            acc += coeff * z[var]
        return acc % p

    for con in order:
        ov = _out_var(con)
        coeff_out = con.c[ov] % p
        if coeff_out != 1:
            # generator only emits coeff-1 outputs; bail rather than mis-handle
            continue
        rhs = (lc(con.a) * lc(con.b) - lc(con.c, skip=ov)) % p
        z[ov] = rhs

    elapsed = time.perf_counter() - t0
    valid = C.is_underconstraint_certificate(circ, z)
    return SolveResult("sat" if valid else "unsat", elapsed, valid)
