"""Deterministic circuit generator for `zk-circuit-uniqueness-dual-cert.v0` P0-A.

THROWAWAY offline experiment code — never wired into self-test/consensus.

P0-A model: a *relational* Boolean constraint system over n variables with a
planted reference witness. Unlike the zk_phase0 feed-forward family, the
generator does NOT decide or expose whether a second witness exists:

  * it draws a full reference assignment w* from the XOF,
  * it emits only constraints that w* satisfies (rejection sampling over a
    fixed catalog), and
  * it designates public-input variables (pinned by unit clauses to their w*
    values) and output variables (reference output = w* restricted).

The mining question is D(seed) = C(w) AND output(w) != reference_output:
BUG  <=> D is SAT  (alternative witness = counterexample to uniqueness),
SAFE <=> D is UNSAT (LRAT certificate).

The generator never computes an alternative witness, never stores an answer
label, and has no mutation/deletion step whose location an attacker could
read back. Whether the planted-witness *sampling process* still leaks the
answer is exactly what the S1 structural attackers measure — it is a claim
under test, not a design guarantee.
"""
from __future__ import annotations

from dataclasses import dataclass, field

from xof import Xof

CANDIDATE = "zk-circuit-uniqueness-dual-cert.v0"

# Gate ops usable in definition-style constraints t <-> op(a, b).
GATE_OPS = ("and", "or", "xor")


@dataclass(frozen=True)
class Params:
    """One difficulty-band configuration. All axes are independent (S3)."""

    n_vars: int
    n_pub: int
    n_out: int
    n_clause: int  # planted width-`clause_width` clauses
    n_xor: int  # planted 3-variable parity constraints
    n_gate: int  # planted t <-> op(a,b) definitions
    clause_width: int = 3

    def label(self) -> str:
        return (
            f"n{self.n_vars}-pub{self.n_pub}-out{self.n_out}"
            f"-c{self.n_clause}-x{self.n_xor}-g{self.n_gate}-k{self.clause_width}"
        )


@dataclass
class Circuit:
    seed: str
    params: Params
    n_vars: int
    public_vars: list[int]  # 1-based var ids pinned by unit clauses
    output_vars: list[int]  # 1-based var ids whose joint value must be unique
    clauses: list[tuple[int, ...]]  # every constraint, CNF form, DIMACS literals
    # structural metadata parallel to `clauses` groups — the attacker is
    # generator-omniscient, so this is intentionally public:
    #   ("pub", var, bit) / ("clause",) / ("xor", a, b, c, parity)
    #   ("gate", op, t, a, b)
    kinds: list[tuple] = field(default_factory=list)
    ref_witness: list[int] = field(default_factory=list)  # index 0 unused
    ref_output: list[int] = field(default_factory=list)
    xof_draws: int = 0
    xof_rejections: int = 0

    def diff_clause(self) -> tuple[int, ...]:
        """CNF clause asserting output(w) != reference_output."""
        lits = []
        for v in self.output_vars:
            r = self.ref_witness[v]
            lits.append(-v if r == 1 else v)
        return tuple(lits)

    def d_clauses(self) -> list[tuple[int, ...]]:
        """The full D(seed) constraint list: circuit AND output-differs."""
        return self.clauses + [self.diff_clause()]


def _clause_satisfied(clause: tuple[int, ...], w: list[int]) -> bool:
    for lit in clause:
        v = abs(lit)
        if (w[v] == 1) == (lit > 0):
            return True
    return False


def _xor_clauses(a: int, b: int, c: int, parity: int) -> list[tuple[int, ...]]:
    """CNF for a XOR b XOR c = parity (4 clauses)."""
    out = []
    for sa in (1, -1):
        for sb in (1, -1):
            for sc in (1, -1):
                ones = sum(1 for s in (sa, sb, sc) if s > 0)
                # clause (sa*a | sb*b | sc*c) forbids the single assignment
                # a=(sa<0), b=(sb<0), c=(sc<0); keep clauses that forbid
                # exactly the wrong-parity assignments.
                if (3 - ones) % 2 != parity:
                    out.append((sa * a, sb * b, sc * c))
    return out


def _gate_clauses(op: str, t: int, a: int, b: int) -> list[tuple[int, ...]]:
    if op == "and":
        return [(-t, a), (-t, b), (t, -a, -b)]
    if op == "or":
        return [(t, -a), (t, -b), (-t, a, b)]
    if op == "xor":
        return [(-t, a, b), (-t, -a, -b), (t, -a, b), (t, a, -b)]
    raise ValueError(f"unknown gate op {op}")


def _gate_holds(op: str, w: list[int], t: int, a: int, b: int) -> bool:
    if op == "and":
        val = w[a] & w[b]
    elif op == "or":
        val = w[a] | w[b]
    else:
        val = w[a] ^ w[b]
    return w[t] == val


def generate(seed: str, params: Params) -> Circuit:
    """Pure function (seed, params) -> Circuit. See module docstring."""
    if params.n_pub + params.n_out > params.n_vars:
        raise ValueError("public + output vars exceed n_vars")
    if params.clause_width < 2 or params.clause_width > params.n_vars:
        raise ValueError("bad clause width")

    x = Xof(seed, f"gen|{params.label()}")
    n = params.n_vars

    # 1. Planted reference witness (index 0 unused to match DIMACS numbering).
    w = [0] * (n + 1)
    for v in range(1, n + 1):
        w[v] = x.bit()

    # 2. Role assignment: public inputs are the first n_pub ids and outputs the
    #    last n_out ids of a XOF-shuffled ordering, so roles are not tied to
    #    variable numbering.
    order = list(range(1, n + 1))
    for i in range(n - 1, 0, -1):
        j = x.randbelow(i + 1)
        order[i], order[j] = order[j], order[i]
    public_vars = sorted(order[: params.n_pub])
    output_vars = sorted(order[n - params.n_out :])

    clauses: list[tuple[int, ...]] = []
    kinds: list[tuple] = []

    # 3. Public-input pins (satisfied by w* by construction).
    for v in public_vars:
        lit = v if w[v] == 1 else -v
        clauses.append((lit,))
        kinds.append(("pub", v, w[v]))

    # 4. Planted clauses: uniform over width-k clauses on distinct vars that
    #    w* satisfies (rejection sampling).
    for _ in range(params.n_clause):
        while True:
            vs = x.sample_distinct(n, params.clause_width)
            cl = tuple(v if x.bit() == 1 else -v for v in vs)
            if _clause_satisfied(cl, w):
                break
            x.rejections += 1
        clauses.append(cl)
        kinds.append(("clause",))

    # 5. Planted parity constraints: parity is computed FROM w*, so these are
    #    always consistent and need no rejection.
    for _ in range(params.n_xor):
        a, b, c = x.sample_distinct(n, 3)
        parity = w[a] ^ w[b] ^ w[c]
        for cl in _xor_clauses(a, b, c, parity):
            clauses.append(cl)
            kinds.append(("xor", a, b, c, parity))

    # 6. Planted gate definitions: uniform over (op, t, a, b) consistent
    #    with w* (rejection sampling).
    for _ in range(params.n_gate):
        while True:
            t, a, b = x.sample_distinct(n, 3)
            op = GATE_OPS[x.randbelow(len(GATE_OPS))]
            if _gate_holds(op, w, t, a, b):
                break
            x.rejections += 1
        for cl in _gate_clauses(op, t, a, b):
            clauses.append(cl)
            kinds.append(("gate", op, t, a, b))

    circuit = Circuit(
        seed=seed,
        params=params,
        n_vars=n,
        public_vars=public_vars,
        output_vars=output_vars,
        clauses=clauses,
        kinds=kinds,
        ref_witness=w,
        ref_output=[w[v] for v in output_vars],
        xof_draws=x.draws,
        xof_rejections=x.rejections,
    )
    return circuit
