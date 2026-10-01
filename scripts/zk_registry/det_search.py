"""Cheap, sound searches for output-determinism counterexamples.

Both searches start from a real witness ``w`` (from the witness generator, accepted by the
oracle) and return a second assignment ``w'``; a result is reported only when the oracle accepts
``w'``, the inputs agree and some output differs.  Neither search can show that DET holds.

* output mutation: change one output wire of ``w``;
* linear kernel: solve ``J·δ = 0`` over the non-input wires, where ``J`` is the Jacobian of the
  constraint map at ``w``, and test ``w + t·δ`` for nullspace basis vectors that move an output.
  A basis vector whose quadratic part ``(A·δ)(B·δ)`` also vanishes gives a whole line of
  solutions, which is the usual shape of a missing constraint;
* re-solve: for the free column of each such basis vector, change that wire and recompute the
  other non-input wires by propagation (a constraint with one unknown wire that enters it
  affinely determines that wire), which follows curved solution sets the linear step misses.
"""
from __future__ import annotations

import random
import time
from dataclasses import dataclass

from . import r1cs as R


@dataclass
class Counterexample:
    method: str
    base: list[int]
    other: list[int]
    changed_outputs: list[int]


def confirm(r: R.R1cs, w1: list[int], w2: list[int], inputs: list[int], outputs: list[int],
            pre=None, differ=None) -> list[int] | None:
    """Output wires that differ, if (w1, w2) is a DET counterexample; otherwise None.  ``pre``: the input
    preconditions DET is stated under (a predicate on a full assignment); both assignments must satisfy it.
    ``differ(w1, w2)``: the output wires on which the statement's output relation fails (default: the
    output wires whose values differ)."""
    if not (R.satisfies(r, w1) and R.satisfies(r, w2)):
        return None
    if pre is not None and not (pre(w1) and pre(w2)):
        return None
    if any(w1[i] != w2[i] for i in inputs):
        return None
    diff = differ(w1, w2) if differ is not None else [o for o in outputs if w1[o] != w2[o]]
    return diff or None


def output_mutation(r: R.R1cs, w: list[int], inputs: list[int], outputs: list[int], rng: random.Random,
                    pre=None, differ=None) -> Counterexample | None:
    p = r.prime
    for o in outputs:
        for delta in (1, p - 1, rng.randrange(1, p)):
            w2 = list(w)
            w2[o] = (w2[o] + delta) % p
            diff = confirm(r, w, w2, inputs, outputs, pre, differ)
            if diff:
                return Counterexample("output-mutation", w, w2, diff)
    return None


def jacobian_rows(r: R.R1cs, w: list[int], fixed: set[int]) -> list[dict[int, int]]:
    p = r.prime
    rows = []
    for a, b, c in r.constraints:
        av, bv = R.lc_value(a, w, p), R.lc_value(b, w, p)
        row: dict[int, int] = {}
        for j, coeff in a:
            row[j] = (row.get(j, 0) + coeff * bv) % p
        for j, coeff in b:
            row[j] = (row.get(j, 0) + coeff * av) % p
        for j, coeff in c:
            row[j] = (row.get(j, 0) - coeff) % p
        row = {j: v for j, v in row.items() if v and j not in fixed}
        if row:
            rows.append(row)
    return rows


class Budget(Exception):
    pass


def nullspace_basis(rows: list[dict[int, int]], columns: list[int], p: int,
                    deadline: float) -> list[tuple[int, dict[int, int]]]:
    """Reduced row echelon form by sparse elimination; (free column, basis vector) for every free
    column, in column order."""
    pivots: dict[int, dict[int, int]] = {}          # pivot column -> reduced row (coefficient 1 at pivot)
    col_rows: dict[int, set[int]] = {}              # column -> pivot columns whose row mentions it
    for row in rows:
        if time.time() > deadline:
            raise Budget()
        row = dict(row)
        for pc in [c for c in row if c in pivots]:
            f = row.get(pc)
            if not f:
                continue
            for j, v in pivots[pc].items():
                nv = (row.get(j, 0) - f * v) % p
                if nv:
                    row[j] = nv
                else:
                    row.pop(j, None)
        if not row:
            continue
        pc = min(row, key=lambda c: (len(col_rows.get(c, ())), c))
        inv = pow(row[pc], p - 2, p)
        row = {j: v * inv % p for j, v in row.items()}
        for other in list(col_rows.get(pc, ())):
            orow = pivots[other]
            f = orow.get(pc)
            if not f:
                continue
            for j, v in row.items():
                nv = (orow.get(j, 0) - f * v) % p
                if nv:
                    if j not in orow:
                        col_rows.setdefault(j, set()).add(other)
                    orow[j] = nv
                else:
                    if j in orow:
                        del orow[j]
                        col_rows.get(j, set()).discard(other)
        pivots[pc] = row
        for j in row:
            col_rows.setdefault(j, set()).add(pc)
    free = [c for c in columns if c not in pivots]
    basis = []
    for f in free:
        vec = {f: 1}
        for pc in col_rows.get(f, ()):
            v = pivots[pc].get(f)
            if v:
                vec[pc] = (-v) % p
        basis.append((f, vec))
    return basis


def propagate(r: R.R1cs, w: list[int], fixed: set[int], seed: dict[int, int], deadline: float) -> list[int] | None:
    """Recompute every non-fixed wire reachable from ``seed`` by single-unknown affine constraints.

    Wires that no constraint determines keep their value in ``w``; the result is returned only if
    the oracle accepts it."""
    p = r.prime
    val: dict[int, int] = {i: w[i] for i in fixed}
    val.update(seed)
    wires_of = [sorted({i for lc in cons for i, _ in lc}) for cons in r.constraints]
    users: dict[int, list[int]] = {}
    for k, ws in enumerate(wires_of):
        for i in ws:
            users.setdefault(i, []).append(k)
    inv2 = pow(2, p - 2, p)

    def residual(k: int, u: int, x: int) -> int:
        def lc(terms):
            return sum(c * (x if i == u else val[i]) for i, c in terms) % p
        a, b, c = r.constraints[k]
        return (lc(a) * lc(b) - lc(c)) % p

    todo = list(range(len(r.constraints)))
    queued = set(todo)
    while todo:
        if time.time() > deadline:
            raise Budget()
        k = todo.pop()
        queued.discard(k)
        unknown = [i for i in wires_of[k] if i not in val]
        if len(unknown) != 1:
            continue
        u = unknown[0]
        f0, f1, f2 = residual(k, u, 0), residual(k, u, 1), residual(k, u, 2)
        quad = (f2 - 2 * f1 + f0) * inv2 % p
        lin = (f1 - f0 - quad) % p
        if quad or not lin:
            continue
        val[u] = (-f0) * pow(lin, p - 2, p) % p
        for k2 in users.get(u, []):
            if k2 not in queued:
                queued.add(k2)
                todo.append(k2)
    w2 = [val.get(i, w[i]) for i in range(r.n_wires)]
    return w2 if R.satisfies(r, w2) else None


def linear_kernel(r: R.R1cs, w: list[int], inputs: list[int], outputs: list[int], rng: random.Random,
                  budget_s: float = 20.0, max_vectors: int = 64, pre=None,
                  differ=None) -> tuple[Counterexample | None, str]:
    p = r.prime
    fixed = set(inputs) | {0}
    columns = [j for j in range(r.n_wires) if j not in fixed]
    deadline = time.time() + budget_s
    out_set = set(outputs)
    try:
        basis = nullspace_basis(jacobian_rows(r, w, fixed), columns, p, deadline)
        moving = [(f, vec) for f, vec in basis if any(c in out_set for c in vec)]
        for f, vec in moving[:max_vectors]:
            for t in (1, rng.randrange(1, p)):
                w2 = list(w)
                for j, v in vec.items():
                    w2[j] = (w2[j] + t * v) % p
                diff = confirm(r, w, w2, inputs, outputs, pre, differ)
                if diff:
                    return Counterexample("linear-kernel", w, w2, diff), "found"
        # re-solve from every first-order-free wire (output-moving ones first): a free wire whose
        # first-order effect on the outputs vanishes can still move them (e.g. through lam * lam)
        ordered = moving + [(f, vec) for f, vec in basis if (f, vec) not in moving]
        for f, _ in ordered[:max_vectors]:
            for t in (1, rng.randrange(1, p)):
                w3 = propagate(r, w, fixed, {f: (w[f] + t) % p}, deadline)
                diff = confirm(r, w, w3, inputs, outputs, pre, differ) if w3 else None
                if diff:
                    return Counterexample("re-solve", w, w3, diff), "found"
    except Budget:
        return None, "budget-exceeded"
    return None, f"no-counterexample(free-directions={len(basis)},moving-outputs={len(moving)})"


def boundary_first(r: R.R1cs, witnesses: list[list[int]], inputs: list[int]) -> list[list[int]]:
    """Witnesses ordered by how many inputs sit at 0, 1 or p-1 (degenerate points first), stable."""
    edge = {0, 1, r.prime - 1}
    return sorted(witnesses, key=lambda w: -sum(w[i] in edge for i in inputs))


def search(r: R.R1cs, witnesses: list[list[int]], inputs: list[int], outputs: list[int], seed: str,
           budget_s: float = 20.0, mutation_bases: int = 32, kernel_bases: int = 6,
           pre=None, differ=None) -> tuple[Counterexample | None, dict]:
    """Output mutation from up to ``mutation_bases`` real witnesses, then the linear-kernel search from
    up to ``kernel_bases``; boundary-heavy witnesses first; stop at the first confirmed counterexample.
    With input preconditions ``pre``, only witnesses satisfying them are used as bases."""
    rng = random.Random(seed + "/det-search")
    if pre is not None:
        witnesses = [w for w in witnesses if pre(w)]
    ordered = boundary_first(r, witnesses, inputs)
    log = {"bases": len(witnesses), "mutation_bases": min(len(ordered), mutation_bases),
           "output_mutation": "no-counterexample", "linear_kernel": []}
    for w in ordered[:mutation_bases]:
        ce = output_mutation(r, w, inputs, outputs, rng, pre, differ)
        if ce:
            log["output_mutation"] = "found"
            return ce, log
    for w in ordered[:kernel_bases]:
        ce, status = linear_kernel(r, w, inputs, outputs, rng, budget_s=budget_s, pre=pre, differ=differ)
        log["linear_kernel"].append(status)
        if ce:
            return ce, log
        if status == "budget-exceeded":
            break
    return None, log
