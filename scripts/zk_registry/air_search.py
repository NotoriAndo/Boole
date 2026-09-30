"""Cheap, sound searches for AIR output-determinism counterexamples.

Every search starts from a real window ``w`` (from the zkVM's own trace generation; it satisfies the
constraints and the assumptions and has an active output message) and proposes a second window ``w'``.  A
result is reported only when :func:`confirm` accepts the pair: both windows satisfy every constraint and
every assumption, the fixed variables agree, the input messages have equal bus contributions and some
output message does not.  No search can show that DET holds.

* single-variable: change one variable that occurs in an active output message;
* linear kernel: solve ``J·δ = 0`` where ``J`` stacks the Jacobian of the constraint polynomials at ``w`` and
  the Jacobian of the active input message expressions, over the variables that are not fixed (in a first
  pass the variables occurring in assumption lookups are fixed as well), and test ``w + t·δ`` for basis
  vectors that move an output value;
* re-solve: change one first-order free variable and recompute the others by propagation (a constraint with
  one unknown variable that enters it affinely determines that variable).
"""
from __future__ import annotations

import random
import time
from dataclasses import dataclass
from typing import Callable

from . import air_ir as IR
from . import air_lean as AL
from .det_search import Budget, nullspace_basis


@dataclass
class Counterexample:
    method: str
    base: list[int]
    other: list[int]
    changed_outputs: list[int]          # indices into roles.outputs


@dataclass
class Context:
    air: IR.Air
    layout: IR.Layout
    roles: AL.Roles
    assume: Callable[[str, list[int]], bool]      # table name, values -> fact holds

    @property
    def p(self) -> int:
        return self.air.p


def assumptions_hold(ctx: Context, vals: list[int], w: list[int]) -> bool:
    for a in ctx.roles.assumptions:
        if vals[a.mult] != 0 and not ctx.assume(a.table, [vals[v] for v in a.values]):
            return False
    if ctx.roles.selectors:
        o = ctx.layout.offsets()["sel"]
        sel = {s: w[o + i] for i, s in enumerate(ctx.layout.selectors)}
        if any(v not in (0, 1) for v in sel.values()):
            return False
        if "trans" in sel and "last" in sel and sel["trans"] != (1 - sel["last"]) % ctx.p:
            return False
    return True


def msg_eq(vals1: list[int], vals2: list[int], m: AL.Message) -> bool:
    if vals1[m.mult] != vals2[m.mult]:
        return False
    return vals1[m.mult] == 0 or all(vals1[v] == vals2[v] for v in m.values)


def confirm(ctx: Context, w1: list[int], w2: list[int]) -> list[int] | None:
    """Indices of output messages whose contributions differ, if (w1, w2) is a DET counterexample."""
    v1 = IR.eval_nodes(ctx.air, ctx.layout, w1)
    v2 = IR.eval_nodes(ctx.air, ctx.layout, w2)
    if IR.failing_constraints(ctx.air, ctx.layout, w1, v1) or IR.failing_constraints(ctx.air, ctx.layout, w2, v2):
        return None
    if not (assumptions_hold(ctx, v1, w1) and assumptions_hold(ctx, v2, w2)):
        return None
    if any(w1[i] != w2[i] for i in ctx.layout.fixed()):
        return None
    if not all(msg_eq(v1, v2, m) for m in ctx.roles.inputs):
        return None
    diff = [k for k, m in enumerate(ctx.roles.outputs) if not msg_eq(v1, v2, m)]
    return diff or None


# ------------------------------------------------------------------------------------------ helpers

def node_vars(air: IR.Air, layout: IR.Layout) -> list[frozenset]:
    """Variables each node depends on."""
    out: list[frozenset] = []
    for n in air.nodes:
        op = n[0]
        if op == "const":
            out.append(frozenset())
        elif op in IR.LEAF_OPS:
            out.append(frozenset([layout.var(n)]))
        elif op == "neg":
            out.append(out[n[1]])
        else:
            out.append(out[n[1]] | out[n[2]])
    return out


def gradients(air: IR.Air, layout: IR.Layout, vals: list[int], needed: set[int], p: int) -> dict[int, dict[int, int]]:
    """Sparse gradients (variable -> derivative) at the point whose node values are ``vals``, for the nodes
    in ``needed`` and their ancestors."""
    reach = [False] * len(air.nodes)
    for k in needed:
        reach[k] = True
    for k in range(len(air.nodes) - 1, -1, -1):
        if reach[k]:
            n = air.nodes[k]
            if n[0] in IR.BIN_OPS or n[0] == "neg":
                for c in n[1:]:
                    reach[c] = True
    g: dict[int, dict[int, int]] = {}
    for k, n in enumerate(air.nodes):
        if not reach[k]:
            continue
        op = n[0]
        if op == "const":
            g[k] = {}
        elif op in IR.LEAF_OPS:
            g[k] = {layout.var(n): 1}
        elif op == "neg":
            g[k] = {v: (-c) % p for v, c in g[n[1]].items()}
        elif op in ("add", "sub"):
            s = 1 if op == "add" else p - 1
            d = dict(g[n[1]])
            for v, c in g[n[2]].items():
                nv = (d.get(v, 0) + s * c) % p
                if nv:
                    d[v] = nv
                else:
                    d.pop(v, None)
            g[k] = d
        else:
            a, b = n[1], n[2]
            d = {v: c * vals[b] % p for v, c in g[a].items() if c * vals[b] % p}
            for v, c in g[b].items():
                nv = (d.get(v, 0) + c * vals[a]) % p
                if nv:
                    d[v] = nv
                else:
                    d.pop(v, None)
            g[k] = d
    return g


def _active_outputs(ctx: Context, vals: list[int]) -> list[AL.Message]:
    return [m for m in ctx.roles.outputs if vals[m.mult] != 0]


# ------------------------------------------------------------------------------------------ searches

def single_variable(ctx: Context, w: list[int], rng: random.Random, nv: list[frozenset]) -> Counterexample | None:
    vals = IR.eval_nodes(ctx.air, ctx.layout, w)
    fixed = set(ctx.layout.fixed())
    cand: list[int] = []
    for m in _active_outputs(ctx, vals):
        for v in m.values:
            cand += sorted(nv[v] - fixed)
    p = ctx.p
    for var in dict.fromkeys(cand):
        for delta in (1, p - 1, rng.randrange(1, p)):
            w2 = list(w)
            w2[var] = (w2[var] + delta) % p
            diff = confirm(ctx, w, w2)
            if diff:
                return Counterexample("single-variable", w, w2, diff)
    return None


def _system(ctx: Context, w: list[int], vals: list[int], extra_fixed: set[int]):
    fixed = set(ctx.layout.fixed()) | extra_fixed
    rows_nodes = list(ctx.air.constraints)
    for m in ctx.roles.inputs:
        rows_nodes.append(m.mult)
        if vals[m.mult] != 0:
            rows_nodes += m.values
    out_nodes = [v for m in _active_outputs(ctx, vals) for v in m.values] + [m.mult for m in ctx.roles.outputs]
    g = gradients(ctx.air, ctx.layout, vals, set(rows_nodes) | set(out_nodes), ctx.p)
    rows = []
    for k in rows_nodes:
        row = {v: c for v, c in g[k].items() if v not in fixed}
        if row:
            rows.append(row)
    columns = [v for v in range(ctx.layout.n_vars) if v not in fixed]
    return rows, columns, [g[k] for k in out_nodes]


def linear_kernel(ctx: Context, w: list[int], rng: random.Random, extra_fixed: set[int], budget_s: float,
                  max_vectors: int = 64) -> tuple[Counterexample | None, str]:
    p = ctx.p
    vals = IR.eval_nodes(ctx.air, ctx.layout, w)
    deadline = time.time() + budget_s
    try:
        rows, columns, out_grads = _system(ctx, w, vals, extra_fixed)
        basis = nullspace_basis(rows, columns, p, deadline)

        def moves(vec: dict[int, int]) -> bool:
            return any(sum(c * vec.get(v, 0) for v, c in og.items()) % p for og in out_grads)
        moving = [(f, vec) for f, vec in basis if moves(vec)]
        for f, vec in moving[:max_vectors]:
            for t in (1, p - 1, rng.randrange(1, p)):
                w2 = list(w)
                for j, v in vec.items():
                    w2[j] = (w2[j] + t * v) % p
                diff = confirm(ctx, w, w2)
                if diff:
                    return Counterexample("linear-kernel", w, w2, diff), "found"
        ordered = moving + [(f, vec) for f, vec in basis if not moves(vec)]
        fixed = set(ctx.layout.fixed()) | extra_fixed
        for f, _ in ordered[:max_vectors]:
            for t in (1, rng.randrange(1, p)):
                w3 = propagate(ctx, w, fixed, {f: (w[f] + t) % p}, deadline)
                diff = confirm(ctx, w, w3) if w3 else None
                if diff:
                    return Counterexample("re-solve", w, w3, diff), "found"
    except Budget:
        return None, "budget-exceeded"
    return None, f"no-counterexample(free-directions={len(basis)},moving-outputs={len(moving)})"


def propagate(ctx: Context, w: list[int], fixed: set[int], seed: dict[int, int], deadline: float) -> list[int] | None:
    """Recompute non-fixed variables reachable from ``seed``: a constraint (or an active input equation) with one
    unknown variable in which it is affine determines that variable.  Returns the window if it satisfies the
    constraints."""
    air, layout, p = ctx.air, ctx.layout, ctx.p
    nv = node_vars(air, layout)
    base_vals = IR.eval_nodes(air, layout, w)
    eqs: list[tuple[int, int]] = [(c, 0) for c in air.constraints]          # (node, required value)
    for m in ctx.roles.inputs:
        eqs.append((m.mult, base_vals[m.mult]))
        if base_vals[m.mult] != 0:
            eqs += [(v, base_vals[v]) for v in m.values]
    val: dict[int, int] = {i: w[i] for i in fixed}
    val.update(seed)
    users: dict[int, list[int]] = {}
    for k, (node, _) in enumerate(eqs):
        for v in nv[node]:
            users.setdefault(v, []).append(k)

    def value_at(node: int, u: int, x: int) -> int:
        cur = [val.get(i, w[i]) for i in range(layout.n_vars)]
        cur[u] = x
        return IR.eval_nodes(air, layout, cur)[node]

    todo = list(range(len(eqs)))
    queued = set(todo)
    while todo:
        if time.time() > deadline:
            raise Budget()
        k = todo.pop()
        queued.discard(k)
        node, target = eqs[k]
        unknown = [i for i in nv[node] if i not in val]
        if len(unknown) != 1:
            continue
        u = unknown[0]
        f0, f1, f2, f3 = (value_at(node, u, x) for x in (0, 1, 2, 3))
        # affine in u iff the second and third finite differences vanish
        if (f2 - 2 * f1 + f0) % p or (f3 - 3 * f2 + 3 * f1 - f0) % p:
            continue
        lin = (f1 - f0) % p
        if not lin:
            continue
        val[u] = (target - f0) * pow(lin, p - 2, p) % p
        for k2 in users.get(u, []):
            if k2 not in queued:
                queued.add(k2)
                todo.append(k2)
    w2 = [val.get(i, w[i]) for i in range(layout.n_vars)]
    return w2 if IR.satisfies(air, layout, w2) else None


def boundary_first(ctx: Context, windows: list[list[int]]) -> list[list[int]]:
    edge = {0, 1, ctx.p - 1}
    return sorted(windows, key=lambda w: -sum(x in edge for x in w))


def search(ctx: Context, windows: list[list[int]], seed: str, budget_s: float = 20.0, single_bases: int = 16,
           kernel_bases: int = 4) -> tuple[Counterexample | None, dict]:
    """Searches from real windows with at least one active output message; stops at the first confirmed
    counterexample."""
    rng = random.Random(seed + "/air-det-search")
    active = []
    for w in windows:
        vals = IR.eval_nodes(ctx.air, ctx.layout, w)
        if _active_outputs(ctx, vals):
            active.append(w)
    ordered = boundary_first(ctx, active)
    log = {"bases": len(windows), "active_bases": len(active), "single_variable": "no-counterexample",
           "linear_kernel": []}
    nv = node_vars(ctx.air, ctx.layout)
    for w in ordered[:single_bases]:
        ce = single_variable(ctx, w, rng, nv)
        if ce:
            log["single_variable"] = "found"
            return ce, log
    asm_vars: set[int] = set()
    for a in ctx.roles.assumptions:
        for v in [a.mult] + a.values:
            asm_vars |= nv[v]
    for w in ordered[:kernel_bases]:
        for label, extra in (("assumption-vars-fixed", asm_vars), ("free", set())):
            ce, status = linear_kernel(ctx, w, rng, extra, budget_s)
            log["linear_kernel"].append(f"{label}:{status}")
            if ce:
                return ce, log
            if status == "budget-exceeded":
                return None, log
    return None, log
