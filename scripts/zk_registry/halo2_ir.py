"""``boole-halo2-ir/v1`` documents and the flattened halo2 model.

A document is one ``MockProver`` run of a wrapper circuit (exported by ``halo2_harness/export``): the constraint
system after selector compression (gates as polynomials over column queries with rotations, lookup arguments,
permutation columns), the fixed / advice / instance values of all ``n = 2^k`` rows, the permutation cycles, the
regions and a note per advice assignment (region, annotation).  Expression forms::

    ["c", hex]  constant          ["f" | "a" | "i", column, rotation]   fixed / advice / instance query
    ["neg", e]  ["add", e, e]  ["mul", e, e]  ["scale", e, hex]         ["s", index] (virtual selector; rejected)

:func:`flatten` turns a document into a :class:`Model` over the concrete layout, with the semantics of the halo2
protocol the prover is checked against:

* every gate polynomial at every row ``r`` of the ``n`` rows (queries at ``(r + rotation) mod n``), fixed cells
  (including the compressed selectors) substituted as constants (unassigned fixed cells are 0, as keygen leaves
  them); instances whose polynomial folds to the constant 0 are dropped;
* every lookup at every usable row: the input tuple is a member of the table, the set of table tuples over the
  usable rows.  Tables over fixed columns are constants (a one-column table equal to ``{0, .., m-1}`` is the
  predicate ``x < m``); tables over advice columns are tuples of model terms (exact, no hypothesis);
* every permutation cycle as equalities of consecutive cells (fixed cells as constants);
* variables: the advice and instance cells these constraints reference.  ``Inputs`` are the instance cells and the
  advice cells the wrapper names by (region, annotation); ``Outputs`` are the cells of region ``boole-outputs``.

The evaluator here (:func:`check`) is independent of the Lean emitter; agreement on real and mutated witnesses is
the fidelity evidence (gate G-FID), together with ``MockProver::verify`` verdicts on the same mutants.
"""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, field

FORMAT = "boole-halo2-ir/v1"
FIELDS = {
    0x40000000000000000000000000000000224698fc094cf91b992d30ed00000001: "pallas-base",
    0x40000000000000000000000000000000224698fc0994a8dd8c46eb2100000001: "vesta-base",
    0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001: "bn254-fr",
}
OUTPUT_REGION = "boole-outputs"
MAX_ITEMS = 4000                 # size policy: constraints (gate instances + copies + lookups)
MAX_NODES = 100000               # and expression nodes


class IrError(ValueError):
    pass


def _int(h: str) -> int:
    return int(h, 16)


def load(path: str) -> dict:
    with open(path, encoding="utf-8") as f:
        doc = json.load(f)
    return parse(doc)


def parse(doc: dict) -> dict:
    if doc.get("format") != FORMAT:
        raise IrError(f"format {doc.get('format')!r}")
    fld = doc["field"]
    p = int(fld["modulus"], 16)
    if _int(fld["one"]) != 1 or _int(fld["two"]) != 2:
        raise IrError("field element byte order check failed (encodings of 1 and 2)")
    if p not in FIELDS:
        raise IrError(f"unknown field modulus {fld['modulus']}")
    doc["p"] = p
    doc["field_name"] = FIELDS[p]
    for key in ("fixed", "advice", "instance"):
        vals = {}
        for c, r, v in doc[key]:
            if v == "poison":
                raise IrError(f"poisoned {key} cell ({c}, {r})")
            vals[(c, r)] = _int(v)
        doc[key + "_values"] = vals
    return doc


def structure_sha256(doc: dict) -> str:
    """Hash of everything that defines the model (not the advice / instance values)."""
    body = {"k": doc["k"], "p": doc["p"], "gates": doc["gates"], "lookups": doc["lookups"],
            "perm": doc["perm_columns"], "cycles": doc["cycles"], "fixed": doc["fixed"],
            "num": [doc["num_fixed"], doc["num_advice"], doc["num_instance"]],
            "regions": doc["regions"], "cells": [[n[0], n[1], n[2], n[3]] for n in doc["notes"]],
            "inputs": doc["meta"].get("input_notes", []), "instance_rows": sorted(r for (_, r) in doc["instance_values"])}
    return hashlib.sha256(json.dumps(body, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


# ------------------------------------------------------------------------------------------ model

@dataclass
class Table:
    kind: str                      # "range" | "set" | "advice"
    width: int
    bound: int = 0                 # range: x < bound
    rows: list = field(default_factory=list)    # set: sorted tuples of ints; advice: tuples of node ids
    columns: list = field(default_factory=list)  # description of the table expressions


@dataclass
class Model:
    p: int
    field_name: str
    k: int
    n: int
    usable: int
    cells: list                    # variable -> ("a" | "i", column, row)
    nodes: list                    # DAG in the AIR IR node format (["main", 0, var], ["const", c], ...)
    polys: list                    # (node, label)
    copies: list                   # ((kind, x), (kind, y), label) with kind "v" (variable) or "c" (constant)
    lookups: list                  # (tuple of input nodes, table index, label)
    tables: list
    inputs: list
    outputs: list
    input_notes: list
    meta: dict
    regions: list
    unsat_constants: list = field(default_factory=list)   # labels of items that fold to a false constant

    @property
    def n_vars(self) -> int:
        return len(self.cells)

    @property
    def n_items(self) -> int:
        return len(self.polys) + len(self.copies) + len(self.lookups)

    def size(self) -> dict:
        return {"items": self.n_items, "gate_instances": len(self.polys), "copies": len(self.copies),
                "lookups": len(self.lookups), "nodes": len(self.nodes), "variables": self.n_vars,
                "tables": [{"kind": t.kind, "width": t.width, "rows": len(t.rows) if t.kind != "range" else t.bound}
                           for t in self.tables]}

    def within_policy(self) -> bool:
        return self.n_items <= MAX_ITEMS and len(self.nodes) <= MAX_NODES

    def cell_name(self, v: int) -> str:
        k, c, r = self.cells[v]
        return f"{'advice' if k == 'a' else 'instance'}[{c}] row {r}"

    def sha256(self) -> str:
        body = {"p": self.p, "cells": self.cells, "nodes": self.nodes, "polys": [x[0] for x in self.polys],
                "copies": [[a, b] for a, b, _ in self.copies], "lookups": [[list(i), t] for i, t, _ in self.lookups],
                "tables": [[t.kind, t.width, t.bound, t.rows] for t in self.tables], "inputs": self.inputs,
                "outputs": self.outputs}
        return hashlib.sha256(json.dumps(body, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


class _Builder:
    def __init__(self, doc: dict):
        self.doc = doc
        self.p = doc["p"]
        self.n = doc["n"]
        self.fixed = doc["fixed_values"]
        self.nodes: list = []
        self.memo: dict = {}

    def node(self, key: tuple) -> int:
        k = self.memo.get(key)
        if k is None:
            k = len(self.nodes)
            self.nodes.append(list(key))
            self.memo[key] = k
        return k

    def const(self, c: int) -> int:
        return self.node(("const", c % self.p))

    def var(self, kind: str, col: int, row: int) -> int:
        return self.node(("cell", kind, col, row))

    def cval(self, k: int):
        n = self.nodes[k]
        return n[1] if n[0] == "const" else None

    def build(self, e, row: int) -> int:
        """Node of expression ``e`` at ``row`` with fixed cells substituted and constants folded."""
        op = e[0]
        p = self.p
        if op == "c":
            return self.const(_int(e[1]))
        if op == "f":
            return self.const(self.fixed.get((e[1], (row + e[2]) % self.n), 0))
        if op in ("a", "i"):
            return self.var(op, e[1], (row + e[2]) % self.n)
        if op == "s":
            raise IrError("virtual selector in an exported expression (selectors must be compressed)")
        if op == "ch":
            raise IrError("challenge-dependent constraint (multi-phase circuit): challenges are not modelled")
        if op == "neg":
            a = self.build(e[1], row)
            ca = self.cval(a)
            return self.const(-ca) if ca is not None else self.node(("neg", a))
        if op == "scale":
            a = self.build(e[1], row)
            return self._mul(a, self.const(_int(e[2])))
        a = self.build(e[1], row)
        if op == "mul":
            if self.cval(a) == 0:
                return a
            return self._mul(a, self.build(e[2], row))
        b = self.build(e[2], row)
        if op == "add":
            ca, cb = self.cval(a), self.cval(b)
            if ca is not None and cb is not None:
                return self.const(ca + cb)
            if ca == 0:
                return b
            if cb == 0:
                return a
            return self.node(("add", a, b))
        raise IrError(f"unknown expression operator {op!r}")

    def _mul(self, a: int, b: int) -> int:
        ca, cb = self.cval(a), self.cval(b)
        if ca is not None and cb is not None:
            return self.const(ca * cb)
        if ca == 0 or cb == 0:
            return self.const(0)
        if ca == 1:
            return b
        if cb == 1:
            return a
        return self.node(("mul", a, b))


def _fixed_queries(e, out: set) -> None:
    op = e[0]
    if op == "f":
        out.add((e[1], e[2]))
    elif op in ("neg", "scale"):
        _fixed_queries(e[1], out)
    elif op in ("add", "mul"):
        _fixed_queries(e[1], out)
        _fixed_queries(e[2], out)


def _queries(e, kinds: set) -> None:
    op = e[0]
    if op in ("f", "a", "i", "c", "s"):
        kinds.add(op)
    elif op in ("neg", "scale"):
        _queries(e[1], kinds)
    elif op in ("add", "mul"):
        _queries(e[1], kinds)
        _queries(e[2], kinds)


def _zero_rows(bld: _Builder, exprs: list, rows: range) -> list[int]:
    """Rows at which some expression of ``exprs`` may be non-zero, decided once per distinct valuation of the
    fixed cells the expressions query (an expression whose folded node is the constant 0 vanishes identically)."""
    fq: set = set()
    for e in exprs:
        _fixed_queries(e, fq)
    fq_l = sorted(fq)
    cache: dict = {}
    live = []
    n = bld.n
    for r in rows:
        key = tuple(bld.fixed.get((c, (r + rot) % n), 0) for c, rot in fq_l)
        z = cache.get(key)
        if z is None:
            z = all(bld.cval(bld.build(e, r)) == 0 for e in exprs)
            cache[key] = z
        if not z:
            live.append(r)
    return live


def flatten(doc: dict) -> Model:
    bld = _Builder(doc)
    p, n, usable = doc["p"], doc["n"], doc["usable_rows"]
    polys, copies, lookups, tables, unsat = [], [], [], [], []
    # gates: every row of the domain
    for gi, g in enumerate(doc["gates"]):
        for pi, poly in enumerate(g["polys"]):
            for r in _zero_rows(bld, [poly["e"]], range(n)):
                k = bld.build(poly["e"], r)
                c = bld.cval(k)
                label = f"gate {gi} '{g['name']}' poly {pi} '{poly['name']}' row {r}"
                if c == 0:
                    continue
                if c is not None:
                    unsat.append(label)
                polys.append((k, label))
    # lookups: every usable row
    for li, lk in enumerate(doc["lookups"]):
        kinds: set = set()
        for e in lk["table"]:
            _queries(e, kinds)
        width = len(lk["table"])
        if kinds & {"a", "i"}:
            trows = sorted({tuple(bld.build(e, r) for e in lk["table"]) for r in range(usable)})
            table = Table("advice", width, rows=trows)
            consts = None
        else:
            consts = {tuple(bld.cval(bld.build(e, r)) for e in lk["table"]) for r in range(usable)}
            vals = sorted(consts)
            if width == 1 and [v[0] for v in vals] == list(range(len(vals))):
                table = Table("range", 1, bound=len(vals))
            else:
                table = Table("set", width, rows=vals)
        ti = len(tables)
        tables.append(table)
        table.columns = [json.dumps(e, separators=(",", ":")) for e in lk["table"]]
        for r in _zero_rows(bld, lk["input"], range(usable)) if consts is not None and (0,) * width in consts else range(usable):
            ins = tuple(bld.build(e, r) for e in lk["input"])
            cs = [bld.cval(k) for k in ins]
            label = f"lookup {li} row {r}"
            if all(c is not None for c in cs):
                if consts is not None:
                    if tuple(cs) in consts:
                        continue
                    unsat.append(label)
            lookups.append((ins, ti, label))
    # permutation cycles
    pcols = doc["perm_columns"]
    for ci, cyc in enumerate(doc["cycles"]):
        ops = []
        for pc, r in cyc:
            kind, col = pcols[pc]
            ops.append(("c", bld.fixed.get((col, r), 0)) if kind == "f" else ("v", (kind, col, r)))
        for a, b in zip(ops, ops[1:]):
            if a[0] == "c" and b[0] == "c":
                if a[1] == b[1]:
                    continue
                unsat.append(f"copy cycle {ci}")
            copies.append((a, b, f"copy cycle {ci}"))
    # inputs and outputs (cells)
    want = {(r, a) for r, a in doc["meta"].get("input_notes", [])}
    note_inputs = [("a", col, row) for _, rname, col, row, ann in doc["notes"] if (rname, ann) in want]
    out_cells = []
    for _, rname, col, row, ann in doc["notes"]:
        if rname == OUTPUT_REGION and ("a", col, row) not in out_cells:
            out_cells.append(("a", col, row))
    # compact: keep reachable nodes, number the referenced cells (instance cells first, then advice by column, row)
    roots = [k for k, _ in polys] + [k for ins, _, _ in lookups for k in ins]
    roots += [k for t in tables if t.kind == "advice" for row in t.rows for k in row]
    reach = [False] * len(bld.nodes)
    for k in roots:
        reach[k] = True
    for k in range(len(bld.nodes) - 1, -1, -1):
        if reach[k]:
            nd = bld.nodes[k]
            if nd[0] in ("add", "mul"):
                reach[nd[1]] = reach[nd[2]] = True
            elif nd[0] == "neg":
                reach[nd[1]] = True
    cellset = {tuple(nd[1:]) for k, nd in enumerate(bld.nodes) if reach[k] and nd[0] == "cell"}
    cellset |= {o[1] for a, b, _ in copies for o in (a, b) if o[0] == "v"}
    cellset |= set(note_inputs) | set(out_cells)
    cells = sorted(cellset, key=lambda c: (c[0] != "i", c[1], c[2]))
    var_of = {c: v for v, c in enumerate(cells)}
    remap: dict = {}
    nodes: list = []
    for k, nd in enumerate(bld.nodes):
        if not reach[k]:
            continue
        if nd[0] == "cell":
            new = ["main", 0, var_of[tuple(nd[1:])]]
        elif nd[0] == "const":
            new = ["const", nd[1]]
        elif nd[0] == "neg":
            new = ["neg", remap[nd[1]]]
        else:
            new = [nd[0], remap[nd[1]], remap[nd[2]]]
        remap[k] = len(nodes)
        nodes.append(new)
    polys = [(remap[k], label) for k, label in polys]
    lookups = [(tuple(remap[k] for k in ins), ti, label) for ins, ti, label in lookups]
    for t in tables:
        if t.kind == "advice":
            t.rows = [tuple(remap[k] for k in row) for row in t.rows]
    copies = [tuple(("v", var_of[o[1]]) if o[0] == "v" else o for o in (a, b)) + (label,) for a, b, label in copies]
    inputs = sorted({var_of[c] for c in cells if c[0] == "i"} | {var_of[c] for c in note_inputs})
    outputs = [var_of[c] for c in out_cells]
    return Model(p, doc["field_name"], doc["k"], n, usable, cells, nodes, polys, copies, lookups, tables,
                 inputs, outputs, sorted(want), doc["meta"], doc["regions"], unsat)


def witness(model: Model, doc: dict) -> list[int]:
    """Values of the model's variables in one run (unassigned cells are 0, as the prover leaves them)."""
    adv, ins = doc["advice_values"], doc["instance_values"]
    return [(adv if k == "a" else ins).get((c, r), 0) % model.p for k, c, r in model.cells]


# ------------------------------------------------------------------------------------------ evaluator

def eval_nodes(model: Model, w: list[int]) -> list[int]:
    p = model.p
    val: list[int] = []
    for nd in model.nodes:
        op = nd[0]
        if op == "const":
            val.append(nd[1])
        elif op == "main":
            val.append(w[nd[2]] % p)
        elif op == "add":
            val.append((val[nd[1]] + val[nd[2]]) % p)
        elif op == "mul":
            val.append(val[nd[1]] * val[nd[2]] % p)
        elif op == "neg":
            val.append(-val[nd[1]] % p)
        elif op == "sub":
            val.append((val[nd[1]] - val[nd[2]]) % p)
        else:
            raise IrError(f"unknown node {nd}")
    return val


def _operand(model: Model, o, w: list[int]) -> int:
    return o[1] % model.p if o[0] == "c" else w[o[1]] % model.p


def check(model: Model, w: list[int], vals: list[int] | None = None) -> list[str]:
    """Labels of the violated items (empty: the witness satisfies the model)."""
    vals = vals if vals is not None else eval_nodes(model, w)
    bad = [label for k, label in model.polys if vals[k] != 0]
    bad += [label for a, b, label in model.copies if _operand(model, a, w) != _operand(model, b, w)]
    sets = [set(map(tuple, t.rows)) if t.kind == "set" else None for t in model.tables]
    for ins, ti, label in model.lookups:
        t = model.tables[ti]
        x = tuple(vals[k] for k in ins)
        if t.kind == "range":
            ok = x[0] < t.bound
        elif t.kind == "set":
            ok = x in sets[ti]
        else:
            ok = any(x == tuple(vals[k] for k in row) for row in t.rows)
        if not ok:
            bad.append(label)
    return bad


def satisfies(model: Model, w: list[int]) -> bool:
    return not check(model, w)
