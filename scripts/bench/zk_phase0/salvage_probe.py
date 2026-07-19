"""Salvage probe (the ADR-mandated 'one redesign retry' for ZK.0).

The feed-forward gadget family is broken in O(n) by propagation because every
wire is exactly one gadget's output. The only known way to make underconstraint
search hard is to force the attacker to INVERT a many-to-one map: free a wire,
require a downstream 'checkpoint' wire to stay pinned to its honest value while a
bypass path lets the public output change. If the checkpoint map is one-way, the
attacker must brute-force / algebraically invert it.

This probe builds exactly that structure with r 'mixing' rounds and asks whether
Z3's solve time (a strong attacker) grows with r. If it does not, no gadget-based
underconstraint family is salvageable and ZK.0 escalates to a family swap.
"""
from __future__ import annotations

import statistics
import time

import z3

from zkfield import MERSENNE_PRIMES


def probe(p: int, rounds: int, seeds: int, timeout_ms: int):
    """Instance: x is free; checkpoint = mix^rounds(x) pinned to honest; output
    depends on x via a separate quadratic branch. Attacker must find x' != x0
    with the SAME checkpoint but different output. Measures Z3 wall time."""
    solved = 0
    times = []
    statuses = {}
    for seed in range(seeds):
        rng = (seed * 2654435761) % p
        x0 = (rng % (p - 3)) + 2
        k = [(pow(3, r + 1, p) ^ rng) % p for r in range(rounds)]

        # honest checkpoint value via r rounds of t -> (t + k)^2  (many-to-one)
        t = x0
        for r in range(rounds):
            t = ((t + k[r]) * (t + k[r])) % p
        checkpoint = t
        # honest output via a separate branch: out = x^2 + 7x
        out0 = (x0 * x0 + 7 * x0) % p

        x = z3.Int("x")
        s = z3.Solver()
        s.set("timeout", timeout_ms)
        s.add(x >= 0, x < p, x != x0)
        tt = x
        for r in range(rounds):
            tt = ((tt + k[r]) * (tt + k[r])) % p
        s.add(tt == checkpoint)                 # checkpoint pinned
        s.add((x * x + 7 * x) % p != out0)      # output must deviate

        t0 = time.perf_counter()
        res = s.check()
        el = time.perf_counter() - t0
        times.append(el)
        statuses[str(res)] = statuses.get(str(res), 0) + 1
        if res == z3.sat:
            solved += 1

    med = statistics.median(times)
    mx = max(times)
    return med, mx, solved, seeds, statuses


def structural_attack(p: int, rounds: int, seeds: int):
    """The honest attacker for the checkpoint-inversion structure. The checkpoint
    map is iterated squaring mix(t) = (t+k)^2, which is 2-to-1. Same final
    checkpoint is reached by flipping ONE preimage sign at round 0:
        x' = -x0 - 2*k[0]   (mix(x') == mix(x0) == v_1, so mix^r(x') == checkpoint)
    O(1) per instance -- no solver, no square roots even needed for this branch.
    """
    found = 0
    times = []
    for seed in range(seeds):
        rng = (seed * 2654435761) % p
        x0 = (rng % (p - 3)) + 2
        k = [(pow(3, r + 1, p) ^ rng) % p for r in range(rounds)]
        t = x0
        for r in range(rounds):
            t = ((t + k[r]) * (t + k[r])) % p
        checkpoint = t
        out0 = (x0 * x0 + 7 * x0) % p

        t0 = time.perf_counter()
        xp = (-x0 - 2 * k[0]) % p
        # verify: same checkpoint, different output, x' != x0
        tt = xp
        for r in range(rounds):
            tt = ((tt + k[r]) * (tt + k[r])) % p
        ok = (tt == checkpoint and xp != x0 and
              (xp * xp + 7 * xp) % p != out0)
        times.append(time.perf_counter() - t0)
        if ok:
            found += 1
    return statistics.median(times), max(times), found, seeds


if __name__ == "__main__":
    p = MERSENNE_PRIMES[31]
    print("salvage probe: checkpoint-inversion structure (p=2^31-1)")
    print("-- Z3 attacker (blind to structure): looks hard --")
    for rounds in [1, 2, 4]:
        med, mx, solved, tot, st = probe(p, rounds, seeds=8, timeout_ms=6000)
        print(f"rounds={rounds:2d} | z3 median {round(med, 3)}s max {round(mx, 3)}s "
              f"| sat/total {solved}/{tot} | {st}", flush=True)
    print("-- structural attacker (knows it is iterated squaring): O(1) --")
    for rounds in [1, 2, 4, 8, 16, 32]:
        med, mx, found, tot = structural_attack(p, rounds, seeds=200)
        print(f"rounds={rounds:2d} | structural median {round(med * 1e6, 2)}us "
              f"max {round(mx * 1e6, 2)}us | certs {found}/{tot}", flush=True)
