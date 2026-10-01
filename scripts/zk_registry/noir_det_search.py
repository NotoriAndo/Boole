"""Cheap, sound searches for output-determinism counterexamples on flattened ACIR.

Every search starts from a real witness ``w`` (an execution of the compiled program) and proposes a second
assignment ``w'`` with the same parameter witnesses; a result is reported only when :func:`confirm`
accepts both under the *real* black-box semantics (black-box calls whose inputs changed are re-solved by
the compiler's own solver through ``bbeval``) and some return witness differs.  None of the searches can
show that DET holds.

* output mutation: change one return witness;
* linear kernel: solve ``J·δ = 0`` over the non-parameter witnesses, ``J`` the Jacobian of the AssertZero
  equations at ``w`` (the other opcodes are checked by :func:`confirm`), and test ``w + t·δ`` for basis
  vectors that move a return witness;
* re-solve: change one free witness (Brillig outputs first: unconstrained hints) and recompute the other
  witnesses by propagation through AssertZero equations with a single affine unknown.
"""
from __future__ import annotations

import random
import time
from dataclasses import dataclass

from . import det_search as DS
from . import noir_acir as A


@dataclass
class Counterexample:
    method: str
    base: list[int]
    other: list[int]
    changed_outputs: list[int]


class Oracle:
    """Real black-box semantics: the known table plus ``bbeval`` (``fn(op, values) -> outputs | None``)."""

    def __init__(self, interp: A.Interp, bbeval=None):
        self.interp = interp
        self.bbeval = bbeval
        self.calls = 0

    def complete(self, ops: list[dict], w) -> bool:
        """Add real outputs for every active black-box call of ``w`` that the table lacks; False if the
        solver rejects one (the assignment then cannot satisfy that call)."""
        for k, op in enumerate(ops):
            if op["kind"] != "bb":
                continue
            pred = op["predicate"]
            if pred is not None and A.input_value(pred, w) % A.BN254 == 0:
                continue
            ins = tuple(A.input_value(x, w) for x in op["inputs"])
            if self.interp(op["key"], ins) is not None:
                continue
            if self.bbeval is None:
                return False
            self.calls += 1
            outs = self.bbeval(op, w)
            if outs is None:
                return False
            self.interp.add(op["key"], ins, outs)
        return True


def confirm(flat: A.Flat, w1: list[int], w2: list[int], oracle: Oracle) -> list[int] | None:
    if any(w1[i] != w2[i] for i in flat.inputs):
        return None
    diff = [o for o in flat.outputs if w1[o] != w2[o]]
    if not diff:
        return None
    for w in (w1, w2):
        if not oracle.complete(flat.opcodes, w):
            return None
        if not A.check(flat.opcodes, w, oracle.interp)[0]:
            return None
    return diff


def touching(flat: A.Flat) -> dict[int, list[int]]:
    """witness -> indices of the opcodes whose satisfaction can change with it: the opcodes mentioning it and,
    for a memory block it occurs in, the block's whole operation sequence (its initialization included)."""
    users: dict[int, set[int]] = {}
    blocks: dict[int, list[int]] = {}
    for k, op in enumerate(flat.opcodes):
        if op["kind"] in ("mem_init", "mem_op"):
            blocks.setdefault(op["block"], []).append(k)
    for k, op in enumerate(flat.opcodes):
        for wi in _op_witnesses(op):
            users.setdefault(wi, set()).add(k)
            if op["kind"] in ("mem_init", "mem_op"):
                users[wi].update(blocks[op["block"]])
    return {wi: sorted(ks) for wi, ks in users.items()}


def _op_witnesses(op: dict) -> set[int]:
    k = op["kind"]
    if k == "assert_zero":
        return op["expr"].witnesses()
    if k == "range":
        return {op["input"][1]} if op["input"][0] == "w" else set()
    if k in ("and", "xor"):
        return {x[1] for x in (op["lhs"], op["rhs"]) if x[0] == "w"} | {op["output"]}
    if k == "bb":
        ws = {x[1] for x in op["inputs"] if x[0] == "w"} | set(op["outputs"])
        if op["predicate"] and op["predicate"][0] == "w":
            ws.add(op["predicate"][1])
        return ws
    if k == "mem_init":
        return set(op["init"])
    if k == "mem_op":
        ws = op["write"].witnesses() | op["index"].witnesses() | op["value"].witnesses()
        return ws | (op["predicate"].witnesses() if op["predicate"] is not None else set())
    return set()


def confirm_single(flat: A.Flat, w1: list[int], w2: list[int], wire: int, oracle: Oracle,
                   touch: dict[int, list[int]]) -> list[int] | None:
    """:func:`confirm` for an assignment that differs from the real witness ``w1`` in one wire only: the
    opcodes that do not involve the wire are unchanged and hold, so only the touching ones are evaluated
    (in program order, memory blocks whole)."""
    if wire in flat.inputs or wire not in flat.outputs:
        return None
    sub = [flat.opcodes[k] for k in touch.get(wire, [])]
    if not oracle.complete(sub, w2):
        return None
    if not A.check(sub, w2, oracle.interp)[0]:
        return None
    return [wire]


def output_mutation(flat: A.Flat, w: list[int], rng: random.Random, oracle: Oracle,
                    touch: dict[int, list[int]] | None = None) -> Counterexample | None:
    p = A.BN254
    touch = touching(flat) if touch is None else touch
    for o in flat.outputs:
        for delta in (1, p - 1, rng.randrange(1, p)):
            w2 = list(w)
            w2[o] = (w2[o] + delta) % p
            if confirm_single(flat, w, w2, o, oracle, touch):
                diff = confirm(flat, w, w2, oracle)      # full confirmation of the candidate
                if diff:
                    return Counterexample("output-mutation", w, w2, diff)
    return None


def jacobian_rows(ops: list[dict], w: list[int], fixed: set[int]) -> list[dict[int, int]]:
    p = A.BN254
    rows = []
    for op in ops:
        if op["kind"] != "assert_zero":
            continue
        e = op["expr"]
        row: dict[int, int] = {}
        for q, a, b in e.mul:
            row[a] = (row.get(a, 0) + q * w[b]) % p
            row[b] = (row.get(b, 0) + q * w[a]) % p
        for q, i in e.lin:
            row[i] = (row.get(i, 0) + q) % p
        row = {j: v for j, v in row.items() if v and j not in fixed}
        if row:
            rows.append(row)
    return rows


def propagate(ops: list[dict], w: list[int], fixed: set[int], seed: dict[int, int], deadline: float) -> list[int]:
    """Recompute non-fixed witnesses reachable from ``seed`` through AssertZero equations with exactly one
    unknown witness entering affinely; the others keep their value in ``w``."""
    p = A.BN254
    val: dict[int, int] = {i: w[i] for i in fixed}
    val.update(seed)
    eqs = [op["expr"] for op in ops if op["kind"] == "assert_zero"]
    wires_of = [sorted(e.witnesses()) for e in eqs]
    users: dict[int, list[int]] = {}
    for k, ws in enumerate(wires_of):
        for i in ws:
            users.setdefault(i, []).append(k)
    todo = list(range(len(eqs)))
    queued = set(todo)
    inv2 = pow(2, p - 2, p)
    while todo:
        if time.time() > deadline:
            raise DS.Budget()
        k = todo.pop()
        queued.discard(k)
        unknown = [i for i in wires_of[k] if i not in val]
        if len(unknown) != 1:
            continue
        u = unknown[0]
        e = eqs[k]

        def f(x: int) -> int:
            look = lambda i: x if i == u else val[i]          # noqa: E731
            return (sum(q * look(a) * look(b) for q, a, b in e.mul) + sum(q * look(i) for q, i in e.lin) + e.const) % p

        f0, f1, f2 = f(0), f(1), f(2)
        quad = (f2 - 2 * f1 + f0) * inv2 % p
        lin = (f1 - f0 - quad) % p
        if quad or not lin:
            continue
        val[u] = (-f0) * pow(lin, p - 2, p) % p
        for k2 in users.get(u, []):
            if k2 not in queued:
                queued.add(k2)
                todo.append(k2)
    return [val.get(i, w[i]) for i in range(len(w))]


def brillig_outputs(ops: list[dict]) -> list[int]:
    out: list[int] = []
    for op in ops:
        if op["kind"] == "brillig":
            out += op["outputs"]
    return list(dict.fromkeys(out))


def kernel_and_resolve(flat: A.Flat, w: list[int], rng: random.Random, oracle: Oracle, budget_s: float,
                       max_vectors: int = 48) -> tuple[Counterexample | None, str]:
    p = A.BN254
    fixed = set(flat.inputs)
    columns = [j for j in range(flat.n_witnesses) if j not in fixed]
    deadline = time.time() + budget_s
    out_set = set(flat.outputs)
    try:
        basis = DS.nullspace_basis(jacobian_rows(flat.opcodes, w, fixed), columns, p, deadline)
        moving = [(f, vec) for f, vec in basis if any(c in out_set for c in vec)]
        for f, vec in moving[:max_vectors]:
            for t in (1, rng.randrange(1, p)):
                w2 = list(w)
                for j, v in vec.items():
                    w2[j] = (w2[j] + t * v) % p
                diff = confirm(flat, w, w2, oracle)
                if diff:
                    return Counterexample("linear-kernel", w, w2, diff), "found"
        hints = [h for h in brillig_outputs(flat.opcodes) if h not in fixed]
        seeds = hints + [f for f, _ in moving if f not in hints] + [f for f, _ in basis if f not in hints]
        for f in list(dict.fromkeys(seeds))[:max_vectors]:
            for t in (1, rng.randrange(1, p)):
                w3 = propagate(flat.opcodes, w, fixed, {f: (w[f] + t) % p}, deadline)
                diff = confirm(flat, w, w3, oracle)
                if diff:
                    return Counterexample("re-solve" + ("-hint" if f in hints else ""), w, w3, diff), "found"
    except DS.Budget:
        return None, "budget-exceeded"
    return None, f"no-counterexample(free-directions={len(basis)},moving-outputs={len(moving)})"


def search(flat: A.Flat, witnesses: list[list[int]], oracle: Oracle, seed: str, budget_s: float = 20.0,
           mutation_bases: int = 16, kernel_bases: int = 4) -> tuple[Counterexample | None, dict]:
    rng = random.Random(seed + "/noir-det-search")
    log = {"bases": len(witnesses), "output_mutation": "no-counterexample", "kernel": []}
    if not flat.outputs:
        log["skipped"] = "no return witnesses"
        return None, log
    touch = touching(flat)
    for w in witnesses[:mutation_bases]:
        ce = output_mutation(flat, w, rng, oracle, touch)
        if ce:
            log["output_mutation"] = "found"
            return ce, log
    for w in witnesses[:kernel_bases]:
        ce, status = kernel_and_resolve(flat, w, rng, oracle, budget_s)
        log["kernel"].append(status)
        if ce:
            return ce, log
        if status == "budget-exceeded":
            break
    log["bbeval_calls"] = oracle.calls
    return None, log
