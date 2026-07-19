"""R1CS circuit model, gadget catalog, deterministic generator, and the cheap
native verifier for the ZK.0 spike.

Model: a witness vector z over F_p with z[0] == 1 (the constant wire). Each
constraint is a triple (A, B, C) of sparse linear combinations; it is satisfied
iff <A,z> * <B,z> == <C,z> (mod p). "Underconstrained" means: for a fixed public
input assignment, the constraints fail to pin down the public outputs uniquely,
so a witness exists that satisfies every constraint yet reports a public output
different from the honest circuit's output.

The generator is a pure function of a seed: same seed -> byte-identical circuit,
honest witness, and mutation choice. That determinism is what a consensus family
would need (every node re-derives the same problem); here we only exercise it.
"""
from __future__ import annotations

import hashlib
from dataclasses import dataclass, field
from typing import Optional

from zkfield import Field

# A sparse linear combination: {var_index: coefficient}.
LinComb = dict


@dataclass
class Constraint:
    a: LinComb
    b: LinComb
    c: LinComb
    kind: str            # gadget that emitted it, for mutation categorization
    targets_output: bool  # does C pin a public output wire?


@dataclass
class Circuit:
    p: int
    num_vars: int
    constraints: list
    public_inputs: list          # var indices that are public inputs
    public_outputs: list         # var indices that are public outputs
    honest_witness: list         # a full satisfying assignment
    seed_hex: str

    def field(self) -> Field:
        return Field(self.p)


class _Builder:
    def __init__(self, p: int):
        self.p = p
        self.f = Field(p)
        self.witness = [1]  # z[0] = 1
        self.constraints: list = []

    def alloc(self, value: int) -> int:
        idx = len(self.witness)
        self.witness.append(value % self.p)
        return idx

    def _lc_value(self, lc: LinComb) -> int:
        acc = 0
        for var, coeff in lc.items():
            acc = (acc + coeff * self.witness[var]) % self.p
        return acc

    def add_constraint(self, a: LinComb, b: LinComb, c: LinComb, kind: str,
                       targets_output: bool = False) -> None:
        # Self-check: the honest witness must satisfy every emitted constraint.
        lhs = (self._lc_value(a) * self._lc_value(b)) % self.p
        rhs = self._lc_value(c) % self.p
        if lhs != rhs:
            raise AssertionError(
                f"generator bug: {kind} constraint unsatisfied by honest witness")
        self.constraints.append(Constraint(a, b, c, kind, targets_output))

    # --- gadget catalog ---------------------------------------------------
    def g_mul(self, x: int, y: int) -> int:
        out = self.alloc(self.f.mul(self.witness[x], self.witness[y]))
        self.add_constraint({x: 1}, {y: 1}, {out: 1}, "mul")
        return out

    def g_square(self, x: int) -> int:
        out = self.alloc(self.f.mul(self.witness[x], self.witness[x]))
        self.add_constraint({x: 1}, {x: 1}, {out: 1}, "square")
        return out

    def g_add(self, x: int, y: int) -> int:
        out = self.alloc(self.f.add(self.witness[x], self.witness[y]))
        self.add_constraint({x: 1, y: 1}, {0: 1}, {out: 1}, "add")
        return out

    def g_mul_const(self, x: int, k: int) -> int:
        out = self.alloc(self.f.mul(k, self.witness[x]))
        self.add_constraint({x: k % self.p}, {0: 1}, {out: 1}, "mul_const")
        return out

    def g_mimc_round(self, x: int, k: int) -> int:
        # y = (x + k)^3 -- a cubic, the main source of nonlinear structure.
        t = self.alloc(self.f.add(self.witness[x], k))
        self.add_constraint({x: 1, 0: k % self.p}, {0: 1}, {t: 1}, "mimc_add")
        t2 = self.alloc(self.f.mul(self.witness[t], self.witness[t]))
        self.add_constraint({t: 1}, {t: 1}, {t2: 1}, "mimc_sq")
        y = self.alloc(self.f.mul(self.witness[t2], self.witness[t]))
        self.add_constraint({t2: 1}, {t: 1}, {y: 1}, "mimc_cube")
        return y

    def g_boolean(self, bit_value: int) -> int:
        # A wire forced to {0,1} via x*x = x. Honest value must be a bit.
        assert bit_value in (0, 1)
        x = self.alloc(bit_value)
        self.add_constraint({x: 1}, {x: 1}, {x: 1}, "boolean")
        return x

    def g_select(self, sel_bit: int, a: int, b: int) -> int:
        # out = b + sel*(a-b), with sel already boolean-constrained.
        out_val = (self.witness[b] +
                   self.witness[sel_bit] * (self.witness[a] - self.witness[b])) % self.p
        out = self.alloc(out_val)
        self.add_constraint({sel_bit: 1}, {a: 1, b: -1 % self.p},
                            {out: 1, b: -1 % self.p}, "select")
        return out


class _SeedRng:
    """Deterministic byte stream from a seed via BLAKE2b, mapped to ints.

    Mirrors how a real family would derive the whole problem from
    target_seed(prev_block_hash, pk, n) -- here the seed is just an int/bytes.
    """

    def __init__(self, seed: bytes):
        self._buf = b""
        self._counter = 0
        self._seed = seed

    def _refill(self) -> None:
        h = hashlib.blake2b(self._seed + self._counter.to_bytes(8, "big"),
                            digest_size=32)
        self._buf += h.digest()
        self._counter += 1

    def bits(self, nbytes: int) -> int:
        while len(self._buf) < nbytes:
            self._refill()
        chunk, self._buf = self._buf[:nbytes], self._buf[nbytes:]
        return int.from_bytes(chunk, "big")

    def below(self, n: int) -> int:
        return self.bits(8) % n

    def field_elem(self, p: int) -> int:
        return self.bits(8) % p


@dataclass
class GenParams:
    p: int
    width: int          # wires introduced per layer (~ constraint density)
    depth: int          # combination depth d
    mutations: int      # k
    mutation_mode: str = "mixed"  # 'terminal' | 'internal' | 'mixed'


def generate(seed_int: int, gp: GenParams) -> Optional[Circuit]:
    """Seed -> (circuit, honest witness, planted underconstraint).

    Returns None if the seed produced a degenerate circuit (no output wire to
    free); the caller resamples. Never tunes for difficulty.
    """
    seed = seed_int.to_bytes(16, "big")
    rng = _SeedRng(seed)
    b = _Builder(gp.p)

    # Public inputs: a couple of field elements + one bit input for boolean/mux.
    pub_in = [b.alloc(rng.field_elem(gp.p)) for _ in range(2)]
    bit_in = b.g_boolean(rng.below(2))
    frontier = list(pub_in) + [bit_in]

    gadget_choices = ["mul", "square", "add", "mul_const", "mimc", "select"]

    for _ in range(gp.depth):
        new_frontier = []
        for _ in range(gp.width):
            g = gadget_choices[rng.below(len(gadget_choices))]
            if g == "mul":
                x, y = _two(rng, frontier)
                new_frontier.append(b.g_mul(x, y))
            elif g == "square":
                x = frontier[rng.below(len(frontier))]
                new_frontier.append(b.g_square(x))
            elif g == "add":
                x, y = _two(rng, frontier)
                new_frontier.append(b.g_add(x, y))
            elif g == "mul_const":
                x = frontier[rng.below(len(frontier))]
                new_frontier.append(b.g_mul_const(x, 1 + rng.field_elem(gp.p)))
            elif g == "mimc":
                x = frontier[rng.below(len(frontier))]
                new_frontier.append(b.g_mimc_round(x, rng.field_elem(gp.p)))
            elif g == "select":
                a, bb = _two(rng, frontier)
                new_frontier.append(b.g_select(bit_in, a, bb))
        frontier = frontier + new_frontier

    # Public outputs: the last `width` wires produced.
    outputs = frontier[-gp.width:]
    for con in b.constraints:
        # mark output-targeting constraints for mutation categorization
        if any(o in con.c for o in outputs):
            con.targets_output = True

    circ = Circuit(
        p=gp.p, num_vars=len(b.witness), constraints=b.constraints,
        public_inputs=pub_in + [bit_in], public_outputs=outputs,
        honest_witness=list(b.witness),
        seed_hex=hashlib.blake2b(seed, digest_size=8).hexdigest(),
    )
    return _mutate(circ, rng, gp)


def _two(rng: _SeedRng, frontier: list):
    if len(frontier) == 1:
        return frontier[0], frontier[0]
    i = rng.below(len(frontier))
    j = rng.below(len(frontier))
    if j == i:
        j = (j + 1) % len(frontier)
    return frontier[i], frontier[j]


def _mutate(circ: Circuit, rng: _SeedRng, gp: GenParams) -> Optional[Circuit]:
    """Remove k constraints to plant an underconstraint. Records which were
    removed so the report can categorize difficulty by mutation site."""
    n = len(circ.constraints)
    if n == 0:
        return None

    def pick_indices(pred):
        return [i for i, con in enumerate(circ.constraints) if pred(con)]

    if gp.mutation_mode == "terminal":
        pool = pick_indices(lambda c: c.targets_output)
    elif gp.mutation_mode == "internal":
        pool = pick_indices(lambda c: not c.targets_output)
    else:
        pool = list(range(n))
    if not pool:
        pool = list(range(n))

    removed = []
    remaining_pool = list(pool)
    for _ in range(min(gp.mutations, len(remaining_pool))):
        idx = remaining_pool.pop(rng.below(len(remaining_pool)))
        removed.append(idx)

    removed_set = set(removed)
    kept = [con for i, con in enumerate(circ.constraints) if i not in removed_set]
    removed_kinds = [circ.constraints[i].kind for i in removed]
    removed_targets_output = any(circ.constraints[i].targets_output for i in removed)
    circ.constraints = kept
    # stash mutation metadata on the circuit object for the experiment layer
    circ.removed_kinds = removed_kinds                 # type: ignore[attr-defined]
    circ.removed_targets_output = removed_targets_output  # type: ignore[attr-defined]
    return circ


def satisfies(circ: Circuit, z: list) -> bool:
    """Cheap native verifier: O(#constraints) field mults."""
    p = circ.p

    def lc(d):
        acc = 0
        for var, coeff in d.items():
            acc += coeff * z[var]
        return acc % p

    for con in circ.constraints:
        if (lc(con.a) * lc(con.b)) % p != lc(con.c) % p:
            return False
    return True


def is_underconstraint_certificate(circ: Circuit, z: list) -> bool:
    """z proves underconstraint iff it satisfies all (post-mutation) constraints,
    matches the honest public inputs, yet deviates on some public output."""
    if len(z) != circ.num_vars:
        return False
    if z[0] % circ.p != 1:
        return False
    for v in circ.public_inputs:
        if z[v] % circ.p != circ.honest_witness[v] % circ.p:
            return False
    if not satisfies(circ, z):
        return False
    return any(z[o] % circ.p != circ.honest_witness[o] % circ.p
               for o in circ.public_outputs)
