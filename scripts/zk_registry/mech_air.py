"""Battery P3: a mechanical propagation solver for AIR (zkVM) and halo2 DET packages.

The solver is deterministic and reasons only over the package's own IR (the ``boole-air-ir/v1`` DAG of an AIR
package, the ``boole-halo2-ir/v1`` layout export of a halo2 package).  Before anything else the package's
``Model.lean`` is regenerated from that IR with the production emitters and compared byte for byte (AIR: from the
``namespace`` line on; halo2: from the ``set_option`` line on), so the propagation runs over exactly the statement's
hypotheses.

Propagation ("known" = equal in both windows / assignments):

* known at the start: the ``Fixed`` cells (AIR) or the ``Inputs`` cells (halo2);
* an input message (AIR) contributes ``m(w₁) = m(w₂)`` for its multiplicity ``m`` always, and ``v(w₁) = v(w₂)`` for
  each value ``v`` only in a context where ``m`` is known non-zero (a non-zero constant, the context's output
  multiplicity, or a product / negation of such); a multiplicity that may be zero contributes no values;
* a constraint ``g * e = 0`` whose factor ``g`` is known non-zero in the context is used as ``e = 0`` (guards and
  selectors are factors of the AIR polynomials); a guard that may be zero is never peeled;
* range facts, only as the model states them: ``x * (x - 1) = 0`` (``x < 2``), table lookups whose multiplicity is
  known non-zero in the context (the range a row of the table implies for one field, read off the Lean table
  definition), and halo2 range-table lookups ``x.val < m``;
* rule LIN: a fact (constraint or input equality) whose only unknown cell ``u`` occurs affinely with a non-zero
  constant coefficient (exact integer arithmetic over the printed balanced constants, as ``ring`` sees them)
  determines ``u``;
* rule DIGITS (bit / limb decomposition by uniqueness): a fact affine with constant coefficients of one sign in
  its unknown cells, each of which has a range, whose coefficients are super-increasing for those ranges and whose
  maximum stays below ``p``, determines all of them;
* rule CARRY (a ranged expression as a virtual digit): a ranged expression ``t`` whose only unknown cell ``u`` has
  a range ``B`` and occurs in ``t`` with coefficient ``α``, where ``u ± d * t`` is free of ``u`` for
  ``d = ∓1/α mod p`` and ``B ≤ d``, ``B + d * (R - 1) ≤ p``, determines ``u`` (e.g. a boolean carry
  ``t = (a + b - u) / 2^8`` of a byte ``u``);
* rule TABLE-FN: a table lookup whose multiplicity is known non-zero and whose table row states one field as a
  function of the others (bytewise AND / OR / XOR, less-than, shifts and most significant bit; the Lean table
  definition is the specification) determines that field, a cell, once the other fields are known;
* rule LINSYS: up to four facts over the same unknown cells, affine in them with constant coefficients and an
  invertible coefficient matrix mod p, determine them.

An AIR package is DETERMINED when every output multiplicity is known in the base context and, for every distinct
output multiplicity ``m``, every value of the messages carrying ``m`` is known in the context ``m ≠ 0``; a halo2
package when every output cell is known.  Otherwise it is STUCK with the undetermined cells and a reason class.

For a DETERMINED package :func:`emit_solution` writes ``Solution.lean``: the statement file byte-identical, helper
theorems (one per propagation step and per hoisted subterm used as an atom) inserted directly above the theorem's
doc comment, and a proof of ``det`` that chains them.  The production checker (``zk_registry.check``) is the only
judge of the emitted file.
"""
from __future__ import annotations

import argparse
import hashlib
import heapq
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field

from . import air_bus as AB
from . import air_ir as IR
from . import air_lean as AL
from . import halo2_ir as H
from . import halo2_lean_emit as HL
from . import lean_emit as E

VERSION = "1.0"
NAME = "boole-zk-registry-mech-p3"
SOURCES = ["mech_air.py"]          # the battery's own sources (it is a tool over packages, not a generator)
BIG = 1 << 4096                   # variable-free subterms beyond this size are not folded to a constant
MAX_DIGITS = 12                   # rule DIGITS: at most this many unknown cells in one fact
MAX_SYSTEM = 4                    # rule LINSYS: at most this many unknown cells / facts


def source_files() -> list[str]:
    return list(SOURCES)


def sources_sha256() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    h = hashlib.sha256()
    for rel in sorted(SOURCES):
        with open(os.path.join(here, rel), "rb") as f:
            h.update(rel.encode() + b"\0" + hashlib.sha256(f.read()).digest())
    return h.hexdigest()


class MechError(ValueError):
    """The package cannot be analysed (unreadable IR, model reconstruction mismatch, unsupported shape)."""


# ------------------------------------------------------------------------------------------ problem

@dataclass
class Msg:
    index: int                    # position in the In / Out list of the model
    mult: int                     # node id
    values: list[int]             # node ids


@dataclass
class Assume:
    index: int                    # conjunct position in ``Assumptions``
    table: str
    mult: int
    values: list[int]


@dataclass
class Problem:
    kind: str                     # "air" | "halo2"
    pkg: str
    package_id: str
    ns: str
    p: int
    nodes: list
    var: list[int]                # node -> window variable, -1 for non-leaf nodes
    n_vars: int
    width_name: str               # "nVars" | "nWires"
    items: list[int]              # constraint item -> polynomial node (AIR constraints; halo2 gates then copies)
    n_items: int                  # conjuncts of ``Constraints`` (halo2: also the lookups)
    blocks: list[str]             # Block names of the model (empty: one flat conjunction)
    inputs: list[Msg]             # AIR input messages
    outputs: list[Msg]            # AIR output messages
    fixed: list[int]              # AIR Fixed cells
    in_cells: list[int]           # halo2 Inputs
    out_cells: list[int]          # halo2 Outputs
    printer: AL.Printer
    statement: str
    assumes: list[Assume] = field(default_factory=list)       # AIR table lookups (conjuncts of Assumptions)
    n_asm: int = 0
    asm_blocks: list[str] = field(default_factory=list)
    lookups: list[tuple] = field(default_factory=list)        # halo2 range lookups: (item, input node, bound)
    meta: dict = field(default_factory=dict)


def _ns_of(statement: str) -> str:
    m = re.search(r"(?m)^namespace (\S+)$", statement)
    if not m:
        raise MechError("statement has no namespace line")
    return m.group(1)


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as f:
        return f.read()


def _model_path(pkg: str, problem: dict) -> str:
    for fr in problem["checker"]["files"]:
        if fr["role"] == "import":
            return os.path.join(pkg, fr["path"])
    raise MechError("no imported model file")


def _tail(text: str, marker: str) -> str:
    k = text.find(marker)
    if k < 0:
        raise MechError(f"model text has no {marker.strip()!r}")
    return text[k:]


def air_roles(air: IR.Air, iface: dict) -> AL.Roles:
    """The roles of the package's interface record, in the order ``BusModel.roles`` builds them."""
    roles = AL.Roles(selectors=bool(air.uses()["selectors"]))
    recs: dict[int, list] = {}
    for side in ("inputs", "outputs", "assumptions"):
        for rec in iface[side]:
            recs.setdefault(rec["interaction"], []).append((side, rec))
    for k, it in enumerate(air.interactions):
        got = recs.get(k)
        if not got:
            raise MechError(f"interaction {k} has no role in the interface")
        side, rec = got[0]
        note = f"#{k} {it.direction} {rec['bus']}"
        if rec["role"] == "in":
            roles.inputs.append(AL.Message(k, it.mult, list(it.values), note))
        elif rec["role"] == "out":
            roles.outputs.append(AL.Message(k, it.mult, list(it.values), note))
        elif rec["role"] == "assume":
            roles.assumptions.append(AL.Assumption(k, rec["table"], it.mult, list(it.values), f"{note}: {rec['table']}"))
        elif rec["role"] == "split":
            fi = next(r for s, r in got if s == "inputs")["fields"]
            fo = next(r for s, r in got if s == "outputs")["fields"]
            roles.inputs.append(AL.Message(k, it.mult, [it.values[i] for i in fi], f"{note} fields {list(fi)}"))
            roles.outputs.append(AL.Message(k, it.mult, [it.values[i] for i in fo], f"{note} fields {list(fo)}"))
        else:
            raise MechError(f"interaction {k}: unknown role {rec['role']!r}")
    return roles


def load_air(pkg: str) -> Problem:
    problem = json.loads(_read(os.path.join(pkg, "problem.json")))
    air = IR.load(os.path.join(pkg, "evidence", "air.ir.json"))
    layout = IR.Layout.of(air)
    roles = air_roles(air, problem["interface"])
    statement = _read(os.path.join(pkg, problem["checker"]["statement_file"]))
    ns = _ns_of(statement)
    bus = AB.model_for(air.zkvm)
    ids = problem["ids"]
    gen = problem["generator"]
    meta = {"zkvm_name": bus.display, "release": ids["release"], "generator": f"{gen['name']} v{gen['version']}",
            "repo_url": ids["repo_url"], "extractor": problem["air"]["extractor"]["builder"],
            "ir_sha256": problem["air"]["ir_sha256"]}
    text, summary = AL.emit_model(ns, meta, air, layout, roles, bus.tables)
    marker = f"\nnamespace {ns}\n"
    if _tail(text, marker) != _tail(_read(_model_path(pkg, problem)), marker):
        raise MechError("model reconstruction mismatch: the IR and interface do not regenerate Model.lean")
    pr = AL.Printer(air, layout, AL.roots_of(air, roles))
    var = [layout.var(n) if n[0] in IR.LEAF_OPS and n[0] != "const" else -1 for n in air.nodes]
    n_sel = 0
    if roles.selectors:
        n_sel = len(layout.selectors) + (1 if "trans" in layout.selectors and "last" in layout.selectors else 0)
    n_asm = len(roles.assumptions) + n_sel
    return Problem("air", pkg, problem["package_id"], ns, air.p, air.nodes, var, layout.n_vars, "nVars",
                   list(air.constraints), len(air.constraints), summary["blocks"],
                   [Msg(i, m.mult, list(m.values)) for i, m in enumerate(roles.inputs)],
                   [Msg(i, m.mult, list(m.values)) for i, m in enumerate(roles.outputs)],
                   layout.fixed(), [], [], pr, statement,
                   [Assume(i, a.table, a.mult, list(a.values)) for i, a in enumerate(roles.assumptions)],
                   n_asm, AL.asm_block_names(n_asm), [],
                   {"air_name": air.name, "zkvm": air.zkvm, "n_constraints": len(air.constraints),
                    "n_nodes": len(air.nodes), "window_rows": air.window_rows(), "p_literal": air.p})


def load_halo2(pkg: str) -> Problem:
    problem = json.loads(_read(os.path.join(pkg, "problem.json")))
    samples = sorted(f for f in os.listdir(os.path.join(pkg, "evidence")) if re.match(r"ir_sample_\d+\.json$", f))
    if not samples:
        raise MechError("no halo2 IR sample in evidence/")
    model = H.flatten(H.load(os.path.join(pkg, "evidence", samples[0])))
    statement = _read(os.path.join(pkg, problem["checker"]["statement_file"]))
    ns = _ns_of(statement)
    meta = {k: "" for k in ("repo_id", "target", "generator", "repo_url", "commit", "path", "symbol", "wrapper", "rule",
                            "call", "halo2_line", "ir_sha256", "model_sha256")}
    text, summary = HL.emit_model(ns, meta, model)
    marker = "\nset_option maxRecDepth 100000\n"
    if _tail(text, marker) != _tail(_read(_model_path(pkg, problem)), marker):
        raise MechError("model reconstruction mismatch: the IR sample does not regenerate Model.lean")
    air = HL.to_air(model)
    layout = IR.Layout.of(air)
    pr = AL.Printer(air, layout, HL._roots(model))
    var = [n[2] if n[0] == "main" else -1 for n in air.nodes]
    base = len(model.polys) + len(model.copies)
    lookups = [(base + li, ins[0], model.tables[ti].bound) for li, (ins, ti, _) in enumerate(model.lookups)
               if model.tables[ti].kind == "range"]
    return Problem("halo2", pkg, problem["package_id"], ns, model.p, air.nodes, var, max(model.n_vars, 1), "nWires",
                   list(air.constraints), summary["n_items"], summary["blocks"], [], [], [], list(model.inputs),
                   list(model.outputs), pr, statement, lookups=lookups,
                   meta={"n_items": summary["n_items"], "n_lookups": len(model.lookups), "n_nodes": len(air.nodes),
                         "p_literal": model.p})


def load(pkg: str) -> Problem:
    problem = json.loads(_read(os.path.join(pkg, "problem.json")))
    return load_halo2(pkg) if "circuit" in problem else load_air(pkg)


# ------------------------------------------------------------------------------------------ structure

def _balanced(c: int, p: int) -> int:
    """The integer the printer writes for the canonical constant ``c`` (``c > p/2`` as ``-(p - c)``)."""
    return c if c <= p // 2 else c - p


class Analysis:
    """Per-node supports, exact constant values and hoisting information of one problem."""

    def __init__(self, prob: Problem):
        self.prob = prob
        nodes, p = prob.nodes, prob.p
        self.support: list[frozenset] = []
        self.const: list[int | None] = []
        empty = frozenset()
        for k, n in enumerate(nodes):
            op = n[0]
            if op == "const":
                self.support.append(empty)
                self.const.append(_balanced(n[1], p))
            elif prob.var[k] >= 0:
                self.support.append(frozenset((prob.var[k],)))
                self.const.append(None)
            elif op == "neg":
                self.support.append(self.support[n[1]])
                c = self.const[n[1]]
                self.const.append(None if c is None else -c)
            else:
                a, b = n[1], n[2]
                sa, sb = self.support[a], self.support[b]
                self.support.append(sa if sa is sb or sb <= sa else sb if sa <= sb else sa | sb)
                ca, cb = self.const[a], self.const[b]
                if ca is None or cb is None:
                    self.const.append(None)
                else:
                    v = ca + cb if op == "add" else ca - cb if op == "sub" else ca * cb
                    self.const.append(v if -BIG < v < BIG else None)
        self.hoisted = set(prob.printer.hoisted)

    def children(self, k: int) -> tuple:
        n = self.prob.nodes[k]
        return (n[1],) if n[0] == "neg" else (n[1], n[2]) if n[0] in IR.BIN_OPS else ()

    def is_one(self, k: int) -> bool:
        return self.const[k] == 1

    def lin(self, k: int, u: int, memo: dict) -> int | None:
        """Exact integer coefficient of cell ``u`` if node ``k`` is affine in ``u`` with a constant coefficient
        (coefficient 0 when ``u`` does not occur); ``None`` otherwise (non-affine or non-constant coefficient)."""
        if u not in self.support[k]:
            return 0
        if k in memo:
            return memo[k]
        n = self.prob.nodes[k]
        op = n[0]
        if self.prob.var[k] >= 0:
            r = 1
        elif op == "neg":
            c = self.lin(n[1], u, memo)
            r = None if c is None else -c
        elif op in ("add", "sub"):
            a, b = self.lin(n[1], u, memo), self.lin(n[2], u, memo)
            r = None if a is None or b is None else (a + b if op == "add" else a - b)
        else:  # mul
            a, b = n[1], n[2]
            if u in self.support[a] and u in self.support[b]:
                r = None
            elif u in self.support[a]:
                ca, kb = self.lin(a, u, memo), self.const[b]
                r = None if ca is None or kb is None else ca * kb
            else:
                cb, ka = self.lin(b, u, memo), self.const[a]
                r = None if cb is None or ka is None else cb * ka
        memo[k] = r
        return r

    def lin_shape(self, k: int, u: int) -> str:
        """``affine-const`` | ``affine-known`` (coefficient is a non-constant expression of other cells) |
        ``nonlinear`` — for reason classification only."""
        c = self.lin(k, u, {})
        if c is not None:
            return "affine-const" if c % self.prob.p else "cancels"
        memo: dict = {}

        def deg(j: int) -> int:     # degree in u, capped at 2
            if u not in self.support[j]:
                return 0
            if j in memo:
                return memo[j]
            n = self.prob.nodes[j]
            if self.prob.var[j] >= 0:
                d = 1
            elif n[0] == "neg":
                d = deg(n[1])
            elif n[0] in ("add", "sub"):
                d = max(deg(n[1]), deg(n[2]))
            else:
                d = min(2, deg(n[1]) + deg(n[2]))
            memo[j] = d
            return d
        return "affine-known" if deg(k) == 1 else "nonlinear"

    def bool_of(self, k: int) -> tuple[int, int] | None:
        """``(x, s)`` when node ``k`` is literally ``s · x * (x - 1)`` (``x * (x - 1)``, ``(x - 1) * x``,
        ``x * (1 - x)``, ``(1 - x) * x``, or with ``x + (-1)``)."""
        n = self.prob.nodes[k]
        if n[0] != "mul":
            return None
        a, b = n[1], n[2]
        for x, y in ((a, b), (b, a)):
            m = self.prob.nodes[y]
            if m[0] == "sub" and m[1] == x and self.is_one(m[2]):
                return x, 1
            if m[0] == "sub" and m[2] == x and self.is_one(m[1]):
                return x, -1
            if m[0] == "add" and m[1] == x and self.const[m[2]] == -1:
                return x, 1
        return None

    def text(self, k: int) -> str:
        return self.prob.printer.term(k)

    def walk_atoms(self, k: int, unknown) -> tuple[list[int], list[int], list[int]]:
        """The printed term of node ``k`` with every hoisted subterm that contains an ``unknown`` cell unfolded:
        (hoisted nodes to unfold, outermost first; cell atoms not in ``unknown``; hoisted atoms kept folded)."""
        unk = frozenset(unknown)
        unfold: list[int] = []
        cells: set[int] = set()
        terms: set[int] = set()
        seen: set[int] = set()
        stack = [k]
        while stack:
            j = stack.pop()
            if j in seen:
                continue
            seen.add(j)
            if j in self.hoisted:
                if not (unk & self.support[j]):
                    terms.add(j)
                    continue
                unfold.append(j)
            v = self.prob.var[j]
            if v >= 0:
                if v not in unk:
                    cells.add(v)
                continue
            stack.extend(self.children(j))
        # a hoisted node's subterms have smaller ids: descending ids unfold the outer definitions first
        unfold.sort(reverse=True)
        return unfold, sorted(cells), sorted(terms)

    def body_atoms(self, j: int) -> tuple[list[int], list[int]]:
        """Cells and hoisted atoms of the definition body of hoisted node ``j``."""
        cells: set[int] = set()
        terms: set[int] = set()
        for c in self.children(j):
            _, cs, ts = self.walk_atoms(c, ())
            cells.update(cs)
            terms.update(ts)
        return sorted(cells), sorted(terms)


# ------------------------------------------------------------------------------------------ facts and contexts

@dataclass
class Fact:
    kind: str                     # "cons": the item's polynomial vanishes in both windows; "pair": v(w₁) = v(w₂)
    node: int                     # the polynomial (after peeling) / the value expression
    item: int                     # constraint item (cons) or input message index (pair)
    pos: int = -1                 # pair: value position (-1: the multiplicity)
    peel: list = field(default_factory=list)   # cons: [(guard node, side "L"|"R", parent node), ...] outermost first


@dataclass
class Range:
    """``node.val < bound`` in both windows, from ``src``: ("bool", fact index, sign) | ("asm", assume index,
    position, lemma call template) | ("lookup", item)."""
    node: int
    bound: int
    src: tuple


@dataclass
class Step:
    rule: str                     # LIN | DIGITS | CARRY
    cells: list[int]              # determined cells (DIGITS: in increasing coefficient order)
    fact: Fact | None = None      # LIN, DIGITS
    coef: int = 0                 # LIN: exact integer coefficient of the cell
    data: dict = field(default_factory=dict)


@dataclass
class Ctx:
    """A propagation context: the output multiplicity assumed non-zero (None: the base context)."""
    mult: int | None
    known: set
    steps: list
    facts: list = field(default_factory=list)
    ranges: list = field(default_factory=list)
    fns: list = field(default_factory=list)


def nonzero(an: Analysis, g: int, ctx_mult: int | None) -> tuple | None:
    """A derivation that node ``g`` is non-zero in both windows under the context, or None."""
    p = an.prob.p
    c = an.const[g]
    if c is not None:
        return ("const", c) if c % p else None
    if ctx_mult is not None and an.text(g) == an.text(ctx_mult):
        return ("ctx",)
    n = an.prob.nodes[g]
    if n[0] == "neg":
        d = nonzero(an, n[1], ctx_mult)
        return None if d is None else ("neg", n[1], d)
    if n[0] == "mul":
        da, db = nonzero(an, n[1], ctx_mult), nonzero(an, n[2], ctx_mult)
        return None if da is None or db is None else ("mul", n[1], n[2], da, db)
    return None


def peel(an: Analysis, k: int, ctx_mult: int | None) -> tuple[int, list]:
    """Strip known-non-zero factors off the top of constraint polynomial ``k``."""
    out = []
    while True:
        n = an.prob.nodes[k]
        if n[0] != "mul":
            return k, out
        if an.const[n[1]] is not None or an.const[n[2]] is not None:
            return k, out       # constant factors are part of the linear coefficient
        if nonzero(an, n[1], ctx_mult) is not None:
            out.append((n[1], "L", k))
            k = n[2]
        elif nonzero(an, n[2], ctx_mult) is not None:
            out.append((n[2], "R", k))
            k = n[1]
        else:
            return k, out


# Table semantics for range facts, read off the Lean table definitions in ``air_bus`` (a field of a row of the
# table is below a bound).  Each entry: (value position, bound, Lean call template; ``{h}`` is the table fact).
def table_ranges(an: Analysis, a: Assume) -> list[tuple[int, int, str]]:
    c = [an.const[v] for v in a.values]
    out: list[tuple[int, int, str]] = []
    if a.table == "ovmVarRange" and len(c) == 2 and c[1] is not None and 0 <= c[1] <= 17:
        out.append((0, 1 << c[1], f"mech_vr {c[1]} (by decide) {{h}}"))
    elif a.table == "ovmBitwise" and len(c) == 4:
        out += [(0, 256, "mech_bw0 {h}"), (1, 256, "mech_bw1 {h}")]
    elif a.table == "ovmRangeTuple" and len(c) == 2:
        out += [(0, 256, "mech_rt0 {h}"), (1, 8192, "mech_rt1 {h}")]
    elif a.table == "sp1Byte" and len(c) == 4 and c[0] is not None:
        o = c[0]
        if 0 <= o <= 5:
            out.append((2, 256, f"mech_sb_b {o} (by decide) (by decide) {{h}}"))
        if 0 <= o <= 4:
            out.append((3, 256, f"mech_sb_c {o} (by decide) (by decide) {{h}}"))
        if o == 6 and c[2] is not None and 0 <= c[2] <= 16:
            out.append((1, 1 << c[2], f"mech_sb_r {c[2]} (by decide) (by decide) {{h}}"))
    elif a.table == "picoByte" and len(c) == 5 and c[0] is not None:
        o = c[0]
        if 0 <= o <= 7:
            out.append((3, 256, f"mech_pb_b {o} (by decide) (by decide) {{h}}"))
        if 0 <= o <= 7 and o != 6:
            out.append((4, 256, f"mech_pb_c {o} (by decide) (by decide) (by decide) {{h}}"))
        if o == 8:
            out.append((1, 65536, "mech_pb_16 (by decide) {h}"))
        if o == 9 and c[3] is not None and 0 <= c[3] <= 16:
            out.append((1, 1 << c[3], f"mech_pb_r {c[3]} (by decide) (by decide) {{h}}"))
    return out


@dataclass
class Fn:
    """A functional table row: field ``result`` (a cell) is a function of the fields ``args``."""
    assume: int
    result: int
    args: list[int]
    call: str                     # Lean call template; ``{g1}`` / ``{g2}`` are the table facts of both windows


def table_fns(an: Analysis, a: Assume) -> list[tuple[int, list[int], str]]:
    c = [an.const[v] for v in a.values]
    if a.table == "sp1Byte" and len(c) == 4 and c[0] in (0, 1, 2, 4, 5):
        return [(1, [0, 2, 3], f"mech_sb_fn {c[0]} (by decide) (by decide) (by decide) {{g1}} {{g2}}")]
    if a.table == "picoByte" and len(c) == 5 and c[0] in (0, 1, 2, 3, 5, 6):
        return [(1, [0, 3, 4], f"mech_pb_fn {c[0]} (by decide) (by decide) (by decide) {{g1}} {{g2}}")]
    if a.table == "ovmBitwise" and len(c) == 4 and c[3] == 1:
        return [(2, [0, 1, 3], "mech_bw_fn (by decide) {g1} {g2}")]
    return []


def build_ctx(an: Analysis, ctx_mult: int | None) -> tuple[list[Fact], list[Range]]:
    prob = an.prob
    facts: list[Fact] = []
    ranges: list[Range] = []
    for j, k in enumerate(prob.items):
        node, pl = peel(an, k, ctx_mult)
        facts.append(Fact("cons", node, j, -1, pl))
        b = an.bool_of(node)
        if b is not None and an.support[b[0]]:
            ranges.append(Range(b[0], 2, ("bool", len(facts) - 1, b[1])))
    for m in prob.inputs:
        facts.append(Fact("pair", m.mult, m.index, -1))
        if nonzero(an, m.mult, ctx_mult) is not None:
            for i, v in enumerate(m.values):
                facts.append(Fact("pair", v, m.index, i))
    for ai, a in enumerate(prob.assumes):
        if nonzero(an, a.mult, ctx_mult) is None:
            continue
        for pos, bound, call in table_ranges(an, a):
            if an.support[a.values[pos]]:
                ranges.append(Range(a.values[pos], bound, ("asm", ai, pos, call)))
    for item, node, bound in prob.lookups:
        if an.support[node]:
            ranges.append(Range(node, bound, ("lookup", item)))
    return facts, ranges


class Propagator:
    """Fixpoint of the rules LIN (queue, lowest fact first), then DIGITS and CARRY when LIN is exhausted."""

    def __init__(self, an: Analysis, known0: set, facts: list[Fact], ranges: list[Range], fns: list[Fn] = ()):
        self.an, self.facts, self.ranges, self.fns = an, facts, ranges, list(fns)
        self.p = an.prob.p
        self.known = set(known0)
        self.steps: list[Step] = []
        self.unknown_of = [set(an.support[f.node]) - self.known for f in facts]
        self.by_var: dict[int, list[int]] = {}
        for fi, us in enumerate(self.unknown_of):
            for v in us:
                self.by_var.setdefault(v, []).append(fi)
        self.heap = [fi for fi, us in enumerate(self.unknown_of) if len(us) == 1]
        heapq.heapify(self.heap)
        self.cell_range: dict[int, tuple[int, int]] = {}
        for ri, r in enumerate(ranges):
            v = an.prob.var[r.node]
            if v >= 0 and (v not in self.cell_range or r.bound < self.cell_range[v][0]):
                self.cell_range[v] = (r.bound, ri)

    def learn(self, step: Step) -> None:
        self.steps.append(step)
        for u in step.cells:
            self.known.add(u)
            for fj in self.by_var.get(u, ()):
                self.unknown_of[fj].discard(u)
                if len(self.unknown_of[fj]) == 1:
                    heapq.heappush(self.heap, fj)

    def linear(self) -> None:
        while self.heap:
            fi = heapq.heappop(self.heap)
            us = self.unknown_of[fi]
            if len(us) != 1:
                continue
            (u,) = us
            c = self.an.lin(self.facts[fi].node, u, {})
            if c is None or c % self.p == 0:
                continue
            self.learn(Step("LIN", [u], self.facts[fi], c))

    def carry(self) -> bool:
        an, p = self.an, self.p
        for ri, r in enumerate(self.ranges):
            if an.prob.var[r.node] >= 0:
                continue
            us = an.support[r.node] - self.known
            if len(us) != 1:
                continue
            (u,) = us
            if u not in self.cell_range:
                continue
            alpha = an.lin(r.node, u, {})
            if alpha is None or alpha % p == 0:
                continue
            B, cri = self.cell_range[u]
            inv = pow(alpha % p, -1, p)
            for variant, d in (("plus", (-inv) % p), ("minus", inv)):
                if B <= d and B + d * (r.bound - 1) <= p:
                    num = 1 + d * alpha if variant == "plus" else 1 - d * alpha
                    if num % p:
                        raise MechError("internal: carry multiplier is not a multiple of p")
                    self.learn(Step("CARRY", [u], None, 0, {"range": ri, "cell_range": cri, "d": d, "B": B,
                                                             "R": r.bound, "variant": variant, "M": num // p}))
                    return True
        return False

    def digits(self) -> bool:
        an, p = self.an, self.p
        for fi, f in enumerate(self.facts):
            us = self.unknown_of[fi]
            if not 2 <= len(us) <= MAX_DIGITS or any(u not in self.cell_range for u in us):
                continue
            coefs = {}
            for u in us:
                c = an.lin(f.node, u, {})
                if c is None or c == 0:
                    break
                coefs[u] = c
            else:
                s = 1 if next(iter(coefs.values())) > 0 else -1
                if any(s * c <= 0 for c in coefs.values()):
                    continue
                order = sorted(us, key=lambda u: (s * coefs[u], u))
                acc, ok = 0, True
                for u in order:
                    if s * coefs[u] <= acc:
                        ok = False
                        break
                    acc += s * coefs[u] * (self.cell_range[u][0] - 1)
                if ok and acc < p:
                    self.learn(Step("DIGITS", order, f, 0, {"sign": s, "coefs": [s * coefs[u] for u in order],
                                                            "ranges": [self.cell_range[u][1] for u in order],
                                                            "bounds": [self.cell_range[u][0] for u in order]}))
                    return True
        return False

    def linsys(self) -> bool:
        """Rule LINSYS: ``k ≤ MAX_SYSTEM`` facts over the same ``k`` unknown cells, each affine in them with constant
        coefficients, whose coefficient matrix is invertible mod p."""
        an, p = self.an, self.p
        groups: dict[frozenset, list[int]] = {}
        for fi, us in enumerate(self.unknown_of):
            if 2 <= len(us) <= MAX_SYSTEM:
                groups.setdefault(frozenset(us), []).append(fi)
        for key, fis in sorted(groups.items(), key=lambda kv: kv[1][0]):
            us = sorted(key)
            k = len(us)
            if len(fis) < k:
                continue
            chosen: list[tuple[int, list[int]]] = []
            for fi in fis:
                row = [an.lin(self.facts[fi].node, u, {}) for u in us]
                if any(c is None for c in row):
                    continue
                if _rank([r for _, r in chosen] + [row], p) == len(chosen) + 1:
                    chosen.append((fi, row))
                    if len(chosen) == k:
                        break
            if len(chosen) < k:
                continue
            A = [r for _, r in chosen]
            lam = _inverse(A, p)
            corr = []
            for i in range(k):
                row = []
                for c in range(k):
                    v = sum(lam[i][j] * A[j][c] for j in range(k)) - (1 if i == c else 0)
                    if v % p:
                        raise MechError("internal: inverse matrix check failed")
                    row.append(v // p)
                corr.append(row)
            self.learn(Step("LINSYS", us, None, 0, {"facts": [fi for fi, _ in chosen], "lam": lam, "corr": corr}))
            return True
        return False

    def tablefn(self) -> bool:
        an = self.an
        for i, fn in enumerate(self.fns):
            a = an.prob.assumes[fn.assume]
            u = an.prob.var[a.values[fn.result]]
            if u in self.known:
                continue
            if all(not (an.support[a.values[j]] - self.known) for j in fn.args):
                self.learn(Step("TABLE-FN", [u], None, 0, {"fn": i}))
                return True
        return False

    def run(self) -> None:
        while True:
            self.linear()
            if not (self.carry() or self.digits() or self.tablefn() or self.linsys()):
                return


def _rank(rows: list[list[int]], p: int) -> int:
    m = [[x % p for x in r] for r in rows]
    rank, col, n = 0, 0, len(m[0]) if m else 0
    while rank < len(m) and col < n:
        piv = next((i for i in range(rank, len(m)) if m[i][col]), None)
        if piv is None:
            col += 1
            continue
        m[rank], m[piv] = m[piv], m[rank]
        inv = pow(m[rank][col], -1, p)
        for i in range(len(m)):
            if i != rank and m[i][col]:
                f = m[i][col] * inv % p
                m[i] = [(a - f * b) % p for a, b in zip(m[i], m[rank])]
        rank += 1
        col += 1
    return rank


def _inverse(a: list[list[int]], p: int) -> list[list[int]]:
    """Inverse of a square matrix mod p (entries in [0, p))."""
    k = len(a)
    m = [[x % p for x in r] + [1 if i == j else 0 for j in range(k)] for i, r in enumerate(a)]
    for col in range(k):
        piv = next(i for i in range(col, k) if m[i][col])
        m[col], m[piv] = m[piv], m[col]
        inv = pow(m[col][col], -1, p)
        m[col] = [x * inv % p for x in m[col]]
        for i in range(k):
            if i != col and m[i][col]:
                f = m[i][col]
                m[i] = [(x - f * y) % p for x, y in zip(m[i], m[col])]
    return [r[k:] for r in m]


def pair_source(an: Analysis, k: int, ctx_mult: int | None) -> tuple[int, int] | None:
    """An input message field whose printed term is the term of node ``k`` and which is equal in both windows
    in the context: (message index, position; -1 for the multiplicity)."""
    t = an.text(k)
    for m in an.prob.inputs:
        if an.text(m.mult) == t:
            return m.index, -1
        if nonzero(an, m.mult, ctx_mult) is not None:
            for i, v in enumerate(m.values):
                if an.text(v) == t:
                    return m.index, i
    return None


def cover(an: Analysis, k: int, known: set, ctx_mult: int | None) -> tuple[list, list[int], list[int]] | None:
    """Node ``k`` takes equal values in both windows when every path from it reaches either a subterm whose cells
    are all known or a subterm that is literally an input field equal in the context.  Returns the input fields
    used ((message, position) pairs, outermost first) and the known cells / hoisted atoms outside them, or None."""
    pairs: list = []
    cells: set[int] = set()
    terms: set[int] = set()
    seen: set[int] = set()
    stack = [k]
    while stack:
        j = stack.pop()
        if j in seen:
            continue
        seen.add(j)
        if not (an.support[j] - known):
            _, cs, ts = an.walk_atoms(j, ())
            if j in an.hoisted:
                ts = [j]
                cs = []
            cells.update(cs)
            terms.update(ts)
            continue
        src = pair_source(an, j, ctx_mult)
        if src is not None:
            if src not in pairs:
                pairs.append(src)
            continue
        if j in an.hoisted or an.prob.var[j] >= 0:
            return None
        stack.extend(an.children(j))
    return pairs, sorted(cells), sorted(terms)


def determined(an: Analysis, k: int, known: set, ctx_mult: int | None) -> bool:
    """Node ``k`` takes equal values in both windows (see :func:`cover`)."""
    return cover(an, k, known, ctx_mult) is not None


# ------------------------------------------------------------------------------------------ solve

@dataclass
class Result:
    status: str                   # DETERMINED | STUCK
    reason: str = ""
    detail: dict = field(default_factory=dict)
    base: Ctx | None = None
    ctxs: list = field(default_factory=list)     # one Ctx per distinct output multiplicity (AIR)

    def rules(self) -> dict:
        out: dict[str, int] = {}
        for c in [self.base] + list(self.ctxs):
            for s in (c.steps if c else []):
                out[s.rule] = out.get(s.rule, 0) + 1
        return out


def build_fns(an: Analysis, ctx_mult: int | None) -> list[Fn]:
    out = []
    for ai, a in enumerate(an.prob.assumes):
        if nonzero(an, a.mult, ctx_mult) is None:
            continue
        for res, args, call in table_fns(an, a):
            if an.prob.var[a.values[res]] >= 0:
                out.append(Fn(ai, res, args, call))
    return out


def run_ctx(an: Analysis, known0: set, ctx_mult: int | None) -> Ctx:
    facts, ranges = build_ctx(an, ctx_mult)
    fns = build_fns(an, ctx_mult)
    pg = Propagator(an, known0, facts, ranges, fns)
    pg.run()
    return Ctx(ctx_mult, pg.known, pg.steps, facts, ranges, fns)


def solve(prob: Problem) -> Result:
    an = Analysis(prob)
    if prob.kind == "halo2":
        base = run_ctx(an, set(prob.in_cells), None)
        missing = set(prob.out_cells) - base.known
        if not missing:
            return Result("DETERMINED", base=base)
        return _stuck(an, base, missing, "outputs", base, [])
    base = run_ctx(an, set(prob.fixed), None)
    bad_mult = [m.index for m in prob.outputs if not determined(an, m.mult, base.known, None)]
    if bad_mult:
        cells = set().union(*(an.support[prob.outputs[i].mult] - base.known for i in bad_mult))
        r = _stuck(an, base, cells, "output-multiplicity", base, [])
        r.detail["messages"] = bad_mult[:20]
        return r
    groups: dict[str, list[Msg]] = {}
    order: list[str] = []
    for m in prob.outputs:
        if an.const[m.mult] is not None and an.const[m.mult] % prob.p == 0:
            continue                 # multiplicity 0: the values never count
        key = an.text(m.mult)
        if key not in groups:
            groups[key] = []
            order.append(key)
        groups[key].append(m)
    ctxs = []
    for key in order:
        mult = groups[key][0].mult
        ctx_mult = None if an.const[mult] is not None else mult
        ctx = run_ctx(an, base.known, ctx_mult)
        ctxs.append(ctx)
        missing = set()
        for m in groups[key]:
            for v in m.values:
                if not determined(an, v, ctx.known, ctx_mult):
                    missing |= an.support[v] - ctx.known
        if missing:
            r = _stuck(an, ctx, missing, "output-values", base, ctxs)
            r.detail["context_mult"] = key[:200]
            r.detail["inputs_without_values"] = sum(1 for m in prob.inputs if nonzero(an, m.mult, ctx_mult) is None)
            return r
    return Result("DETERMINED", base=base, ctxs=ctxs)


REASONS = {
    "output-multiplicity": "output multiplicity not determined",
    "input-multiplicity": "output depends on input values whose multiplicity may be zero",
    "unconstrained": "unconstrained output cell (no fact mentions it)",
    "single-unknown-nonconstant-coefficient": "unknown with a non-constant coefficient (may be zero)",
    "single-unknown-nonlinear": "unknown occurs non-linearly",
    "multi-unknown": "coupled unknowns (no single-unknown fact)",
    "guard-may-be-zero": "constraint guarded by a factor that may be zero",
    "none": "no fact left",
}


def _classify(an: Analysis, ctx: Ctx, targets: set) -> dict:
    """Why the undetermined target cells stay unknown: the facts that mention them, by shape."""
    known = ctx.known
    counts = {"single-unknown-nonconstant-coefficient": 0, "single-unknown-nonlinear": 0,
              "multi-unknown": 0, "guard-may-be-zero": 0}
    cells_with_fact = set()
    for f in ctx.facts:
        us = an.support[f.node] - known
        if not us:
            continue
        cells_with_fact |= us
        if not (us & targets):
            continue
        if f.kind == "cons":
            n = an.prob.nodes[f.node]
            if n[0] == "mul" and an.const[n[1]] is None and an.const[n[2]] is None and \
                    (not (an.support[n[1]] - known) or not (an.support[n[2]] - known)):
                counts["guard-may-be-zero"] += 1
                continue
        if len(us) == 1:
            shape = an.lin_shape(f.node, next(iter(us)))
            counts["single-unknown-nonconstant-coefficient" if shape == "affine-known"
                   else "single-unknown-nonlinear"] += 1
        else:
            counts["multi-unknown"] += 1
    free = sorted(targets - cells_with_fact)
    return {"fact_shapes": counts, "unconstrained_cells": free[:20], "n_unconstrained": len(free)}


def _stuck(an: Analysis, ctx: Ctx, missing: set, where: str, base, ctxs) -> Result:
    cls = _classify(an, ctx, missing)
    shapes = cls["fact_shapes"]
    gated = set()
    for m in an.prob.inputs:
        if nonzero(an, m.mult, ctx.mult) is None:
            for v in m.values:
                gated |= an.support[v]
    if where == "output-multiplicity":
        cls_key = "output-multiplicity"
    elif missing & gated - ctx.known:
        cls_key = "input-multiplicity"
    elif cls["n_unconstrained"]:
        cls_key = "unconstrained"
    elif any(shapes.values()):
        cls_key = max(shapes, key=lambda k: (shapes[k], k))
    else:
        cls_key = "none"
    detail = {"where": where, "class": cls_key, "n_missing": len(missing), "missing": sorted(missing)[:20], **cls,
              "known": len(ctx.known), "n_vars": an.prob.n_vars}
    return Result("STUCK", REASONS[cls_key], detail, base, ctxs)


# ------------------------------------------------------------------------------------------ Lean emission

PRELUDE_COMMON = """/-! Battery P3 (mechanical propagation, {name} v{version}): helper lemmas, one theorem per hoisted
subterm used as an atom and one per propagation step; the proof of `det` below chains them. -/

set_option maxHeartbeats 4000000

theorem mech_p : ({p} : F) = 0 := by decide

theorem mech_cancel [Fact (Nat.Prime p)] {{x y c : F}} (hc : c ≠ 0) (h : c * x = c * y) : x = y :=
  mul_left_cancel₀ hc h

theorem mech_ne {{a b : F}} (h : a = b) (n : a ≠ 0) : b ≠ 0 := h ▸ n

theorem mech_bool [Fact (Nat.Prime p)] (x : F) (h : x * (x - 1) = 0) : x.val < 2 := by
  rcases mul_eq_zero.mp h with h | h
  · rw [h, ZMod.val_zero]; norm_num
  · rw [sub_eq_zero.mp h, ZMod.val_one]; norm_num

theorem mech_vdig [Fact (Nat.Prime p)] {{u₁ u₂ t₁ t₂ : F}} (D B R : ℕ) (hB : B ≤ D)
    (hR : B + D * (R - 1) ≤ p) (r₁ : u₁.val < B) (r₂ : u₂.val < B) (s₁ : t₁.val < R) (s₂ : t₂.val < R)
    (key : u₁ + (D : F) * t₁ = u₂ + (D : F) * t₂) : u₁ = u₂ := by
  have hk : ((u₁.val + D * t₁.val : ℕ) : F) = ((u₂.val + D * t₂.val : ℕ) : F) := by
    push_cast [ZMod.natCast_zmod_val]; exact key
  have m₁ : D * t₁.val ≤ D * (R - 1) := Nat.mul_le_mul_left _ (by omega)
  have m₂ : D * t₂.val ≤ D * (R - 1) := Nat.mul_le_mul_left _ (by omega)
  rw [ZMod.natCast_eq_natCast_iff', Nat.mod_eq_of_lt (by omega), Nat.mod_eq_of_lt (by omega)] at hk
  have e₁ : (u₁.val + D * t₁.val) % D = u₁.val := by
    rw [Nat.add_mul_mod_self_left, Nat.mod_eq_of_lt (by omega)]
  have e₂ : (u₂.val + D * t₂.val) % D = u₂.val := by
    rw [Nat.add_mul_mod_self_left, Nat.mod_eq_of_lt (by omega)]
  exact ZMod.val_injective _ (by rw [← e₁, ← e₂, hk])

theorem mech_vdig' [Fact (Nat.Prime p)] {{u₁ u₂ t₁ t₂ : F}} (D B R : ℕ) (hB : B ≤ D)
    (hR : B + D * (R - 1) ≤ p) (r₁ : u₁.val < B) (r₂ : u₂.val < B) (s₁ : t₁.val < R) (s₂ : t₂.val < R)
    (key : u₁ + (D : F) * t₂ = u₂ + (D : F) * t₁) : u₁ = u₂ :=
  mech_vdig D B R hB hR r₁ r₂ s₂ s₁ key
"""

PRELUDE_AIR = """
theorem mech_cons {a b : F} {l₁ l₂ : List F} (h : a = b) (t : l₁ = l₂) : a :: l₁ = b :: l₂ := by rw [h, t]

theorem mech_lh {a b : F} {l₁ l₂ : List F} (h : a :: l₁ = b :: l₂) : a = b := (List.cons.inj h).1

theorem mech_lt {a b : F} {l₁ l₂ : List F} (h : a :: l₁ = b :: l₂) : l₁ = l₂ := (List.cons.inj h).2

theorem mech_fh {a b : Msg} {l₁ l₂ : List Msg} (h : List.Forall₂ MsgEq (a :: l₁) (b :: l₂)) : MsgEq a b :=
  (List.forall₂_cons.mp h).1

theorem mech_ft {a b : Msg} {l₁ l₂ : List Msg} (h : List.Forall₂ MsgEq (a :: l₁) (b :: l₂)) :
    List.Forall₂ MsgEq l₁ l₂ :=
  (List.forall₂_cons.mp h).2
"""

# range lemmas per table (only the tables the model defines are referenced)
PRELUDE_TABLES = {
    "ovmVarRange": """
theorem mech_vr {x b : F} (n : ℕ) (hb : b.val = n) (h : ovmVarRange [x, b] = true) : x.val < 2 ^ n := by
  unfold ovmVarRange at h; simp only [decide_eq_true_eq] at h; rw [← hb]; exact h.2
""",
    "ovmBitwise": """
theorem mech_bw0 {x y z o : F} (h : ovmBitwise [x, y, z, o] = true) : x.val < 256 := by
  unfold ovmBitwise at h; simp only [decide_eq_true_eq] at h; exact h.1

theorem mech_bw1 {x y z o : F} (h : ovmBitwise [x, y, z, o] = true) : y.val < 256 := by
  unfold ovmBitwise at h; simp only [decide_eq_true_eq] at h; exact h.2.1

theorem mech_bw_fn {x y z₁ z₂ o : F} (ho : o.val = 1) (h₁ : ovmBitwise [x, y, z₁, o] = true)
    (h₂ : ovmBitwise [x, y, z₂, o] = true) : z₁ = z₂ := by
  unfold ovmBitwise at h₁ h₂
  simp only [decide_eq_true_eq, ho] at h₁ h₂
  apply ZMod.val_injective
  omega
""",
    "ovmRangeTuple": """
theorem mech_rt0 {x y : F} (h : ovmRangeTuple [x, y] = true) : x.val < 256 := by
  unfold ovmRangeTuple at h; simp only [decide_eq_true_eq] at h; exact h.1

theorem mech_rt1 {x y : F} (h : ovmRangeTuple [x, y] = true) : y.val < 8192 := by
  unfold ovmRangeTuple at h; simp only [decide_eq_true_eq] at h; exact h.2
""",
    "sp1Byte": """
theorem mech_sb_b {o a b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 5) (h : sp1Byte [o, a, b, c] = true) :
    b.val < 256 := by
  unfold sp1Byte at h
  simp only [ho] at h
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 3 ∨ k = 4 ∨ k = 5) with rfl | rfl | rfl | rfl | rfl | rfl <;>
    simp at h <;> omega

theorem mech_sb_c {o a b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 4) (h : sp1Byte [o, a, b, c] = true) :
    c.val < 256 := by
  unfold sp1Byte at h
  simp only [ho] at h
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 3 ∨ k = 4) with rfl | rfl | rfl | rfl | rfl <;>
    simp at h <;> omega

theorem mech_sb_r {o a b c : F} (n : ℕ) (ho : o.val = 6) (hb : b.val = n) (h : sp1Byte [o, a, b, c] = true) :
    a.val < 2 ^ n := by
  unfold sp1Byte at h
  simp only [ho, hb] at h
  simp at h
  exact h.2.1

theorem mech_sb_fn {o a₁ a₂ b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 5) (hk3 : k ≠ 3)
    (h₁ : sp1Byte [o, a₁, b, c] = true) (h₂ : sp1Byte [o, a₂, b, c] = true) : a₁ = a₂ := by
  unfold sp1Byte at h₁ h₂
  simp only [ho] at h₁ h₂
  apply ZMod.val_injective
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 4 ∨ k = 5) with rfl | rfl | rfl | rfl | rfl <;>
    simp at h₁ h₂ <;> (try split_ifs at h₁ h₂) <;> omega
""",
    "picoByte": """
theorem mech_pb_b {o a1 a2 b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 7) (h : picoByte [o, a1, a2, b, c] = true) :
    b.val < 256 := by
  unfold picoByte at h
  simp only [ho] at h
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 3 ∨ k = 4 ∨ k = 5 ∨ k = 6 ∨ k = 7) with
    rfl | rfl | rfl | rfl | rfl | rfl | rfl | rfl <;> simp at h <;> omega

theorem mech_pb_c {o a1 a2 b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 7) (hk6 : k ≠ 6)
    (h : picoByte [o, a1, a2, b, c] = true) : c.val < 256 := by
  unfold picoByte at h
  simp only [ho] at h
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 3 ∨ k = 4 ∨ k = 5 ∨ k = 7) with
    rfl | rfl | rfl | rfl | rfl | rfl | rfl <;> simp at h <;> omega

theorem mech_pb_16 {o a1 a2 b c : F} (ho : o.val = 8) (h : picoByte [o, a1, a2, b, c] = true) : a1.val < 65536 := by
  unfold picoByte at h
  simp only [ho] at h
  simp at h
  omega

theorem mech_pb_r {o a1 a2 b c : F} (n : ℕ) (ho : o.val = 9) (hb : b.val = n) (h : picoByte [o, a1, a2, b, c] = true) :
    a1.val < 2 ^ n := by
  unfold picoByte at h
  simp only [ho, hb] at h
  simp at h
  exact h.2.1

theorem mech_pb_fn {o a₁ a₂ y₁ y₂ b c : F} (k : ℕ) (ho : o.val = k) (hk : k ≤ 6) (hk4 : k ≠ 4)
    (h₁ : picoByte [o, a₁, y₁, b, c] = true) (h₂ : picoByte [o, a₂, y₂, b, c] = true) : a₁ = a₂ := by
  unfold picoByte at h₁ h₂
  simp only [ho] at h₁ h₂
  apply ZMod.val_injective
  rcases (by omega : k = 0 ∨ k = 1 ∨ k = 2 ∨ k = 3 ∨ k = 5 ∨ k = 6) with rfl | rfl | rfl | rfl | rfl | rfl <;>
    simp at h₁ h₂ <;> (try split_ifs at h₁ h₂) <;> omega
""",
}

_W_RE = re.compile(r"(?<![\w.'₀-₉])w(?![\w'₀-₉])")


def _w(text: str, side: int) -> str:
    """The printed term over ``w`` instantiated at window ``w₁`` / ``w₂``."""
    return _W_RE.sub("w₁" if side == 1 else "w₂", text)


def _conj_path(j: int, n: int, blocks: list[str]) -> tuple[str, str | None]:
    """Projection of conjunct ``j`` out of a conjunction of ``n`` items (in blocks of ``E.BLOCK`` when ``blocks``)
    and the definition to unfold when the projection lands on a one-item block (or a one-item conjunction)."""
    def flat(i: int, size: int) -> str:
        return ".2" * i + (".1" if i < size - 1 else "")
    if not blocks:
        return flat(j, n), (None if n > 1 else "")
    bi, i = divmod(j, E.BLOCK)
    size = min(E.BLOCK, n - bi * E.BLOCK)
    return flat(bi, len(blocks)) + flat(i, size), (blocks[bi] if size == 1 else None)


@dataclass
class Scope:
    ctx_mult: int | None
    cells: set = field(default_factory=set)       # e<x> available
    terms: set = field(default_factory=set)       # q<j> available
    pairs: dict = field(default_factory=dict)     # (message, position) -> hypothesis name
    lines: list = field(default_factory=list)

    def child(self, ctx_mult: int | None) -> "Scope":
        return Scope(ctx_mult, set(self.cells), set(self.terms), dict(self.pairs), [])


class Emitter:
    def __init__(self, prob: Problem, res: Result):
        if res.status != "DETERMINED":
            raise MechError("only DETERMINED problems have a proof")
        self.prob, self.res = prob, res
        self.an = Analysis(prob)
        self.W = prob.width_name
        self.helpers: list[str] = []
        self.consts: dict[str, str] = {}
        self.q_emitted: set[int] = set()
        self.n_steps = 0
        self.tables: set[str] = set()

    # -------------------------------------------------------------- texts
    def t(self, k: int, side: int) -> str:
        return _w(self.an.text(k), side)

    def const_ne(self, text: str) -> str:
        """A top-level lemma ``(text : F) ≠ 0`` (proved by ``decide`` outside any ``Fact`` instance binder, so the
        numeral elaborates without it)."""
        if text not in self.consts:
            name = f"mnz_{len(self.consts)}"
            self.consts[text] = name
            self.helpers.append(f"theorem {name} : ({text} : F) ≠ 0 := by decide\n")
        return self.consts[text]

    def nz(self, g: int, d: tuple, side: int) -> str:
        """Proof term of ``g(w_side) ≠ 0`` from a :func:`nonzero` derivation."""
        if d[0] == "ctx":
            return "n₁" if side == 1 else "n₂"
        if d[0] == "const":
            return self.const_ne(self.an.text(g))
        if d[0] == "neg":
            return f"(neg_ne_zero.mpr {self.nz(d[1], d[2], side)})"
        return f"(mul_ne_zero {self.nz(d[1], d[3], side)} {self.nz(d[2], d[4], side)})"

    @staticmethod
    def uses_ctx(d: tuple) -> bool:
        if d[0] == "ctx":
            return True
        if d[0] == "neg":
            return Emitter.uses_ctx(d[2])
        if d[0] == "mul":
            return Emitter.uses_ctx(d[3]) or Emitter.uses_ctx(d[4])
        return False

    # -------------------------------------------------------------- hoisted subterms
    def ensure_q(self, j: int, sc: Scope, ind: str) -> None:
        if j in sc.terms:
            return
        cells, terms = self.an.body_atoms(j)
        for c in terms:
            self.ensure_q(c, sc, ind)
        missing = [x for x in cells if x not in sc.cells]
        if missing:
            raise MechError(f"internal: hoisted term t{j} used before cells {missing[:5]} are known")
        if j not in self.q_emitted:
            self.q_emitted.add(j)
            args = " ".join([f"(e{x} : w₁ {x} = w₂ {x})" for x in cells] + [f"(q{c} : t{c} w₁ = t{c} w₂)" for c in terms])
            rws = [f"e{x}" for x in cells] + [f"q{c}" for c in terms]
            body = f"  unfold t{j}\n  rw [{', '.join(rws)}]" if rws else "  rfl"
            self.helpers.append(f"theorem mq_{j} (w₁ w₂ : Fin {self.W} → F) {args} : t{j} w₁ = t{j} w₂ := by\n{body}\n")
        call = " ".join([f"e{x}" for x in cells] + [f"q{c}" for c in terms])
        sc.lines.append(f"{ind}have q{j} : t{j} w₁ = t{j} w₂ := mq_{j} w₁ w₂ {call}".rstrip())
        sc.terms.add(j)

    def value_proof(self, k: int, sc: Scope, ind: str) -> str:
        """Proof of ``k(w₁) = k(w₂)`` in the scope (see :func:`cover`): the input-field equalities are rewritten
        first (their subterms still mention the first window), then the known cells and hoisted atoms."""
        an = self.an
        if self.prob.var[k] >= 0 and self.prob.var[k] in sc.cells:
            return f"e{self.prob.var[k]}"
        cov = cover(an, k, sc.cells, sc.ctx_mult)
        if cov is None:
            raise MechError(f"internal: value node {k} is not determined")
        pairs, cells, terms = cov
        if pairs == [pair_source(an, k, sc.ctx_mult)] and not cells and not terms:
            return self.ensure_pair(pairs[0][0], pairs[0][1], sc, ind)
        names = [self.ensure_pair(m, i, sc, ind) for m, i in pairs]
        for j in terms:
            self.ensure_q(j, sc, ind)
        rws = names + [f"e{x}" for x in cells] + [f"q{j}" for j in terms]
        return f"(by rw [{', '.join(rws)}])" if rws else "rfl"

    # -------------------------------------------------------------- input messages
    def ensure_pair(self, k: int, i: int, sc: Scope, ind: str) -> str:
        if (k, i) in sc.pairs:
            return sc.pairs[(k, i)]
        m = self.prob.inputs[k]
        if i < 0:
            name = f"im{k}"
            sc.lines.append(f"{ind}have {name} : {self.t(m.mult, 1)} = {self.t(m.mult, 2)} := i{k}.1")
        else:
            d = nonzero(self.an, m.mult, sc.ctx_mult)
            if d is None:
                raise MechError(f"internal: input {k} has no non-zero multiplicity in the context")
            name = f"iv{k}_{i}"
            inner = f"i{k}.2 {self.nz(m.mult, d, 1)}"
            for _ in range(i):
                inner = f"mech_lt ({inner})"
            v = m.values[i]
            sc.lines.append(f"{ind}have {name} : {self.t(v, 1)} = {self.t(v, 2)} := mech_lh ({inner})")
        sc.pairs[(k, i)] = name
        return name

    # -------------------------------------------------------------- hypotheses inside a step theorem
    def fact_lines(self, f: Fact, ctx_mult, tag: str, need: dict) -> tuple[list[str], str]:
        """Lines binding ``a{tag}`` / ``b{tag}`` to the (peeled) fact in both windows; returns (lines, name of the
        top node's Lean form: the fact node after peeling)."""
        an, prob = self.an, self.prob
        path, unf = _conj_path(f.item, prob.n_items, prob.blocks)
        a, b = f"a{tag}", f"b{tag}"
        need["cons"] = True
        lines = [f"have {a} := h₁{path}", f"have {b} := h₂{path}"]
        if unf is not None:
            lines.append(f"unfold {unf or 'Constraints'} at {a} {b}")
        cur = prob.items[f.item]
        for g, side, parent in f.peel:
            if parent != cur:
                raise MechError("internal: peel chain broken")
            if cur in an.hoisted:
                lines.append(f"unfold t{cur} at {a} {b}")
            d = nonzero(an, g, ctx_mult)
            if self.uses_ctx(d):
                need["ctx"] = True
            res = "resolve_left" if side == "L" else "resolve_right"
            lines.append(f"have {a} := (mul_eq_zero.mp {a}).{res} {self.nz(g, d, 1)}")
            lines.append(f"have {b} := (mul_eq_zero.mp {b}).{res} {self.nz(g, d, 2)}")
            n = prob.nodes[cur]
            cur = n[2] if side == "L" else n[1]
        if cur != f.node:
            raise MechError("internal: peeled node mismatch")
        return lines, cur

    def range_lines(self, r: Range, facts: list[Fact], ctx_mult, tag: str, need: dict) -> list[str]:
        """Lines proving ``s{tag}₁ : (x(w₁)).val < B`` and ``s{tag}₂`` for range fact ``r``."""
        an, prob = self.an, self.prob
        x1, x2 = self.t(r.node, 1), self.t(r.node, 2)
        n1, n2 = f"s{tag}₁", f"s{tag}₂"
        kind = r.src[0]
        if kind == "bool":
            f = facts[r.src[1]]
            lines, top = self.fact_lines(f, ctx_mult, f"r{tag}", need)
            if top in an.hoisted:
                lines.append(f"unfold t{top} at ar{tag} br{tag}")
            sg = r.src[2]
            lines += [f"have {n1} : ({x1}).val < 2 := mech_bool _ (by linear_combination ({sg}) * ar{tag})",
                      f"have {n2} : ({x2}).val < 2 := mech_bool _ (by linear_combination ({sg}) * br{tag})"]
            return lines
        if kind == "lookup":
            need["cons"] = True
            path, unf = _conj_path(r.src[1], prob.n_items, prob.blocks)
            lines = [f"have {n1} : ({x1}).val < {r.bound} := h₁{path}", f"have {n2} : ({x2}).val < {r.bound} := h₂{path}"]
            return lines
        lines, g1, g2 = self.asm_hyps(r.src[1], ctx_mult, tag, need)
        call = r.src[3]
        lines += [f"have {n1} : ({x1}).val < {r.bound} := {call.format(h=g1)}",
                  f"have {n2} : ({x2}).val < {r.bound} := {call.format(h=g2)}"]
        return lines

    def asm_hyps(self, ai: int, ctx_mult, tag: str, need: dict) -> tuple[list[str], str, str]:
        """The table fact of assumption ``ai`` in both windows (its multiplicity discharged): (lines, terms)."""
        an, prob = self.an, self.prob
        a = prob.assumes[ai]
        self.tables.add(a.table)
        need["asm"] = True
        d = nonzero(an, a.mult, ctx_mult)
        if self.uses_ctx(d):
            need["ctx"] = True
        path, unf = _conj_path(a.index, prob.n_asm, prob.asm_blocks)
        lines = []
        hyp1, hyp2 = f"ha₁{path}", f"ha₂{path}"
        if unf is not None:
            lines += [f"have z{tag}₁ := ha₁{path}", f"have z{tag}₂ := ha₂{path}",
                      f"unfold {unf or 'Assumptions'} at z{tag}₁ z{tag}₂"]
            hyp1, hyp2 = f"z{tag}₁", f"z{tag}₂"
        return lines, f"({hyp1} {self.nz(a.mult, d, 1)})", f"({hyp2} {self.nz(a.mult, d, 2)})"

    def emit_tablefn(self, s: Step, ctx: Ctx, sc: Scope, ind: str) -> None:
        an, u = self.an, s.cells[0]
        fn = ctx.fns[s.data["fn"]]
        a = self.prob.assumes[fn.assume]
        need: dict = {}
        lines, g1, g2 = self.asm_hyps(fn.assume, sc.ctx_mult, "f", need)
        body = lines + [f"have g₁ := {g1}", f"have g₂ := {g2}"]
        cells: set[int] = set()
        terms: set[int] = set()
        for j in fn.args:
            _, cs, ts = an.walk_atoms(a.values[j], ())
            cells.update(cs)
            terms.update(ts)
        for j in sorted(terms):
            self.ensure_q(j, sc, ind)
        rws = [f"e{x}" for x in sorted(cells)] + [f"q{j}" for j in sorted(terms)]
        if rws:
            body.append(f"rw [{', '.join(rws)}] at g₁")
        body.append("exact " + fn.call.format(g1="g₁", g2="g₂"))
        self.n_steps += 1
        name = f"ms_{self.n_steps}"
        binders, call = self.binders(need, sorted(cells), sorted(terms), sc.ctx_mult)
        self._theorem(name, binders, f"w₁ {u} = w₂ {u}", body)
        sc.lines.append(f"{ind}have e{u} : w₁ {u} = w₂ {u} := {name} {' '.join(call)}")
        sc.cells.add(u)

    def binders(self, need: dict, cells, terms, ctx_mult) -> tuple[list[str], list[str]]:
        b = [f"(w₁ w₂ : Fin {self.W} → F)"]
        c = ["w₁", "w₂"]
        if need.get("cons"):
            b += ["(h₁ : Constraints w₁)", "(h₂ : Constraints w₂)"]
            c += ["h₁", "h₂"]
        if need.get("asm"):
            b += ["(ha₁ : Assumptions w₁)", "(ha₂ : Assumptions w₂)"]
            c += ["ha₁", "ha₂"]
        for binder, arg in need.get("pairs", []):
            b.append(binder)
            c.append(arg)
        b += [f"(e{x} : w₁ {x} = w₂ {x})" for x in cells] + [f"(q{j} : t{j} w₁ = t{j} w₂)" for j in terms]
        c += [f"e{x}" for x in cells] + [f"q{j}" for j in terms]
        if need.get("ctx"):
            b.append(f"(n₁ : {self.t(ctx_mult, 1)} ≠ 0) (n₂ : {self.t(ctx_mult, 2)} ≠ 0)")
            c += ["n₁", "n₂"]
        return b, c

    # -------------------------------------------------------------- steps
    def emit_step(self, s: Step, ctx: Ctx, sc: Scope, ind: str) -> None:
        if s.rule == "LIN":
            self.emit_lin(s, sc, ind)
        elif s.rule == "CARRY":
            self.emit_carry(s, ctx, sc, ind)
        elif s.rule == "LINSYS":
            self.emit_linsys(s, ctx, sc, ind)
        elif s.rule == "TABLE-FN":
            self.emit_tablefn(s, ctx, sc, ind)
        else:
            self.emit_digits(s, sc, ind)

    def emit_linsys(self, s: Step, ctx: Ctx, sc: Scope, ind: str) -> None:
        us, dt = s.cells, s.data
        need: dict = {}
        body: list[str] = []
        diffs, all_cells, all_terms = [], set(), set()
        for j, fi in enumerate(dt["facts"]):
            lines, diff, _, (cells, terms) = self._fact_source(ctx.facts[fi], set(us), sc, ind, need, str(j))
            body += lines
            diffs.append(diff)
            all_cells.update(cells)
            all_terms.update(terms)
        body.append("refine ⟨" + ", ".join("?_" for _ in us) + "⟩")
        for i, u in enumerate(us):
            parts = [f"({dt['lam'][i][j]}) * ({diffs[j]})" for j in range(len(us)) if dt["lam"][i][j]]
            parts += [f"({-c}) * (w₁ {us[k]} - w₂ {us[k]}) * mech_p" for k, c in enumerate(dt["corr"][i]) if c]
            body.append(f"· linear_combination {' + '.join(parts)}")
        self.n_steps += 1
        name = f"ml_{self.n_steps}"
        binders, call = self.binders(need, sorted(all_cells), sorted(all_terms), sc.ctx_mult)
        self._theorem(name, binders, " ∧ ".join(f"w₁ {u} = w₂ {u}" for u in us), body)
        sc.lines.append(f"{ind}obtain ⟨{', '.join(f'e{u}' for u in us)}⟩ := {name} {' '.join(call)}")
        sc.cells.update(us)

    def _fact_source(self, f: Fact, unknown, sc: Scope, ind: str, need: dict,
                     tag: str = "") -> tuple[list[str], str, str, list]:
        """Lines exposing the fact with the unknown-containing hoisted terms unfolded and the known atoms of the
        first window rewritten; returns (lines, combination of the two windows, location, (cells, terms))."""
        an = self.an
        unfold, cells, terms = an.walk_atoms(f.node, unknown)
        for j in terms:
            self.ensure_q(j, sc, ind)
        if f.kind == "cons":
            lines, _ = self.fact_lines(f, sc.ctx_mult, tag, need)
            at, rw_at, diff = f"a{tag} b{tag}", f"a{tag}", f"a{tag} - b{tag}"
        else:
            hyp = self.ensure_pair(f.item, f.pos, sc, ind)
            v = f"v{tag}"
            need.setdefault("pairs", []).append((f"({v} : {self.t(f.node, 1)} = {self.t(f.node, 2)})", hyp))
            lines, at, rw_at, diff = [], v, v, v
        if unfold:
            lines.append(f"unfold {' '.join(f't{j}' for j in unfold)} at {at}")
        rws = [f"e{x}" for x in cells] + [f"q{j}" for j in terms]
        if rws:
            lines.append(f"rw [{', '.join(rws)}] at {rw_at}")
        return lines, diff, rw_at, [cells, terms]

    def _theorem(self, name: str, binders: list[str], concl: str, body: list[str]) -> None:
        self.helpers.append(f"theorem {name} [Fact (Nat.Prime p)] {' '.join(binders)} :\n    {concl} := by\n"
                            + "\n".join(f"  {x}" for x in body) + "\n")

    def emit_lin(self, s: Step, sc: Scope, ind: str) -> None:
        f, u = s.fact, s.cells[0]
        if f.kind == "pair" and self.prob.var[f.node] == u:          # the field is the cell itself
            hyp = self.ensure_pair(f.item, f.pos, sc, ind)
            sc.lines.append(f"{ind}have e{u} : w₁ {u} = w₂ {u} := {hyp}")
            sc.cells.add(u)
            return
        need: dict = {}
        body, diff, _, (cells, terms) = self._fact_source(f, {u}, sc, ind, need)
        c = s.coef
        if c == 1:
            body.append(f"linear_combination {diff}")
        elif c == -1:
            body.append(f"linear_combination -1 * ({diff})")
        else:
            body.append(f"exact mech_cancel {self.const_ne(str(c) if c > 0 else f'({c})')} (by linear_combination {diff})")
        self.n_steps += 1
        name = f"ms_{self.n_steps}"
        binders, call = self.binders(need, cells, terms, sc.ctx_mult)
        self._theorem(name, binders, f"w₁ {u} = w₂ {u}", body)
        sc.lines.append(f"{ind}have e{u} : w₁ {u} = w₂ {u} := {name} {' '.join(call)}")
        sc.cells.add(u)

    def emit_carry(self, s: Step, ctx: Ctx, sc: Scope, ind: str) -> None:
        an, u, dt = self.an, s.cells[0], s.data
        r, cr = ctx.ranges[dt["range"]], ctx.ranges[dt["cell_range"]]
        need: dict = {}
        body = self.range_lines(r, ctx.facts, sc.ctx_mult, "t", need)
        body += self.range_lines(cr, ctx.facts, sc.ctx_mult, "u", need)
        unfold, cells, terms = an.walk_atoms(r.node, {u})
        for j in terms:
            self.ensure_q(j, sc, ind)
        lemma = "mech_vdig" if dt["variant"] == "plus" else "mech_vdig'"
        body.append(f"refine {lemma} {dt['d']} {dt['B']} {dt['R']} (by decide) (by decide) su₁ su₂ st₁ st₂ ?_")
        if unfold:
            body.append(f"unfold {' '.join(f't{j}' for j in unfold)}")
        rws = [f"e{x}" for x in cells] + [f"q{j}" for j in terms]
        if rws:
            body.append(f"rw [{', '.join(rws)}]")
        body += ["push_cast", f"linear_combination ({dt['M']}) * (w₁ {u} - w₂ {u}) * mech_p"]
        self.n_steps += 1
        name = f"ms_{self.n_steps}"
        binders, call = self.binders(need, cells, terms, sc.ctx_mult)
        self._theorem(name, binders, f"w₁ {u} = w₂ {u}", body)
        sc.lines.append(f"{ind}have e{u} : w₁ {u} = w₂ {u} := {name} {' '.join(call)}")
        sc.cells.add(u)

    def emit_digits(self, s: Step, sc: Scope, ind: str) -> None:
        dt, us = s.data, s.cells
        ctx = self._ctx_of(s)
        need: dict = {}
        body, diff, _, (cells, terms) = self._fact_source(s.fact, set(us), sc, ind, need)
        lhs1 = " + ".join(f"{c} * w₁ {u}" for c, u in zip(dt["coefs"], us))
        lhs2 = " + ".join(f"{c} * w₂ {u}" for c, u in zip(dt["coefs"], us))
        body.append(f"have key : {lhs1} = {lhs2} := by linear_combination ({dt['sign']}) * ({diff})")
        for i, ri in enumerate(dt["ranges"]):
            body += self.range_lines(ctx.ranges[ri], ctx.facts, sc.ctx_mult, f"d{i}", need)
        n1 = " + ".join(f"{c} * (w₁ {u}).val" for c, u in zip(dt["coefs"], us))
        n2 = " + ".join(f"{c} * (w₂ {u}).val" for c, u in zip(dt["coefs"], us))
        body += [f"have hp : p = {self.prob.meta['p_literal']} := rfl",
                 f"have hk : (({n1} : ℕ) : F) = (({n2} : ℕ) : F) := by",
                 "  push_cast [ZMod.natCast_zmod_val]; linear_combination key",
                 "rw [ZMod.natCast_eq_natCast_iff', Nat.mod_eq_of_lt (by omega), Nat.mod_eq_of_lt (by omega)] at hk",
                 "exact ⟨" + ", ".join("ZMod.val_injective _ (by omega)" for _ in us) + "⟩"]
        self.n_steps += 1
        name = f"md_{self.n_steps}"
        binders, call = self.binders(need, cells, terms, sc.ctx_mult)
        concl = " ∧ ".join(f"w₁ {u} = w₂ {u}" for u in us)
        self._theorem(name, binders, concl, body)
        sc.lines.append(f"{ind}obtain ⟨{', '.join(f'e{u}' for u in us)}⟩ := {name} {' '.join(call)}")
        sc.cells.update(us)

    def _ctx_of(self, s: Step) -> Ctx:
        for c in [self.res.base] + list(self.res.ctxs):
            if any(x is s for x in c.steps):
                return c
        raise MechError("internal: step without a context")

    # -------------------------------------------------------------- proof
    def proof(self) -> list[str]:
        prob, res = self.prob, self.res
        ind = "  "
        base = Scope(None)
        if prob.kind == "halo2":
            base.lines.append(f"{ind}intro w₁ w₂ h₁ h₂ hin")
            self._destructure("hin", prob.in_cells, base, ind)
            for s in res.base.steps:
                self.emit_step(s, res.base, base, ind)
            tail = "List.forall_mem_nil _"
            for o in reversed(prob.out_cells):
                tail = f"List.forall_mem_cons.2 ⟨e{o}, {tail}⟩"
            return base.lines + [f"{ind}exact {tail}"]
        base.lines.append(f"{ind}intro w₁ w₂ h₁ h₂ ha₁ ha₂ hfix hin")
        self._destructure("hfix", prob.fixed, base, ind)
        prev = "hin"
        for m in prob.inputs:
            base.lines.append(f"{ind}have i{m.index} := mech_fh {prev}")
            if m.index + 1 < len(prob.inputs):
                base.lines.append(f"{ind}have r{m.index} := mech_ft {prev}")
                prev = f"r{m.index}"
        for s in res.base.steps:
            self.emit_step(s, res.base, base, ind)
        an = self.an
        mult_proof: dict[str, str] = {}
        groups: dict[str, list[Msg]] = {}
        order: list[str] = []
        for m in prob.outputs:
            key = an.text(m.mult)
            if key not in mult_proof:
                pr = self.value_proof(m.mult, base, ind)
                if not re.fullmatch(r"[\w₁₂]+", pr):
                    name = f"om{len(mult_proof)}"
                    base.lines.append(f"{ind}have {name} : {self.t(m.mult, 1)} = {self.t(m.mult, 2)} := {pr}")
                    pr = name
                mult_proof[key] = pr
            if an.const[m.mult] is not None and an.const[m.mult] % prob.p == 0:
                continue
            if key not in groups:
                groups[key] = []
                order.append(key)
            groups[key].append(m)
        lines = list(base.lines)
        conj_of: dict[int, tuple[str, int, int, bool]] = {}
        for g, key in enumerate(order):
            msgs = groups[key]
            ctx = res.ctxs[g]
            mult = msgs[0].mult
            const = ctx.mult is None
            sc = base.child(ctx.mult)
            ind2 = ind if const else ind + "  "
            if not const:
                sc.lines.append(f"{ind2}intro n₁")
                sc.lines.append(f"{ind2}have n₂ : {self.t(mult, 2)} ≠ 0 := mech_ne {mult_proof[key]} n₁")
            for s in ctx.steps:
                self.emit_step(s, ctx, sc, ind2)
            eqs, proofs = [], []
            for m in msgs:
                tail = "rfl"
                for v in reversed(m.values):
                    vp = self.value_proof(v, sc, ind2)
                    tail = f"mech_cons {vp} ({tail})" if tail != "rfl" else f"mech_cons {vp} rfl"
                eqs.append(f"[{', '.join(self.t(v, 1) for v in m.values)}] = [{', '.join(self.t(v, 2) for v in m.values)}]")
                proofs.append(tail)
            for i, m in enumerate(msgs):
                conj_of[m.index] = (f"o{g}", i, len(msgs), const)
            stmt = " ∧ ".join(f"({e})" for e in eqs)
            if const:
                lines += sc.lines
                lines.append(f"{ind}have o{g} : {stmt} := ⟨{', '.join(proofs)}⟩" if len(proofs) > 1 else
                             f"{ind}have o{g} : {stmt} := {proofs[0]}")
            else:
                lines.append(f"{ind}have o{g} : {self.t(mult, 1)} ≠ 0 → {stmt} := by")
                lines += sc.lines
                lines.append(f"{ind2}exact ⟨{', '.join(proofs)}⟩" if len(proofs) > 1 else f"{ind2}exact {proofs[0]}")
        term = "List.Forall₂.nil"
        for m in reversed(prob.outputs):
            mp = mult_proof[an.text(m.mult)]
            if m.index not in conj_of:
                vals = "(fun z => (z rfl).elim)"
            else:
                name, i, n, const = conj_of[m.index]
                proj = ".2" * i + (".1" if i < n - 1 else "")
                vals = f"(fun _ => {name}{proj})" if const else f"(fun z => ({name} z){proj})"
            term = f"List.Forall₂.cons (And.intro {mp} {vals}) ({term})" if term != "List.Forall₂.nil" else \
                f"List.Forall₂.cons (And.intro {mp} {vals}) List.Forall₂.nil"
        lines.append(f"{ind}exact {term}")
        return lines

    def _destructure(self, hyp: str, cells: list[int], sc: Scope, ind: str) -> None:
        cur = hyp
        for n, x in enumerate(cells):
            if x not in sc.cells:
                sc.lines.append(f"{ind}have e{x} : w₁ {x} = w₂ {x} := (List.forall_mem_cons.mp {cur}).1")
                sc.cells.add(x)
            if n + 1 < len(cells):
                sc.lines.append(f"{ind}have hl{n} := (List.forall_mem_cons.mp {cur}).2")
                cur = f"hl{n}"


def emit_solution(prob: Problem, res: Result) -> str:
    """``Solution.lean``: the statement with helpers above the theorem's doc comment and the proof for ``sorry``."""
    from .check import parse_statement
    em = Emitter(prob, res)
    proof = em.proof()
    prelude = PRELUDE_COMMON.format(name=NAME, version=VERSION, p=prob.meta["p_literal"])
    if prob.kind == "air":
        prelude += PRELUDE_AIR + "".join(PRELUDE_TABLES[t] for t in sorted(em.tables))
    helpers = prelude + "\n" + "\n".join(em.helpers)
    st = parse_statement(prob.statement, E.STATEMENT_THEOREM)
    if st["body"] != " by\n  sorry":
        raise MechError("statement proof is not `by sorry`")
    return st["pre"] + helpers + "\n" + st["decl"] + " by\n" + "\n".join(proof) + st["post"]


# ------------------------------------------------------------------------------------------ battery run

OUTCOMES = ("MECH-SOLVED", "MECH-STUCK", "MECH-PROOF-FAIL", "MECH-ERROR")


def _jsonl(path: str) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f if line.strip()]


def collect(packages: str, statuses=("OPEN",)) -> list[dict]:
    """The packages of the zkVM wave (SP1, Pico, OpenVM) and the halo2 wave with the given statuses, in a fixed
    order.  The ``INDEX.jsonl`` files carry the effective (merged) status of every record."""
    out = []
    for z in ("sp1", "pico", "openvm"):
        for r in _jsonl(os.path.join(packages, "zkvm-z0", z, "INDEX.jsonl")):
            if r["status"] in statuses:
                out.append({"family": z, "collection": f"zkvm-z0/{z}", "package_id": r["package_id"],
                            "status": r["status"],
                            "dir": os.path.join(packages, "zkvm-z0", z, r["package_id"].split("/", 1)[1])})
    for r in _jsonl(os.path.join(packages, "halo2-h1", "INDEX.jsonl")):
        if r["status"] in statuses:
            out.append({"family": "halo2", "collection": "halo2-h1", "package_id": r["package_id"],
                        "status": r["status"], "dir": os.path.join(packages, "halo2-h1", r["package_id"])})
    return out


def error_class(report: dict) -> str:
    """A short class of a checker failure (first reason)."""
    for key in ("error", "invalid", "fail"):
        for x in report.get(key, []):
            low = x.lower()
            for pat, cls in (("timed out", "timeout"), ("heartbeats", "heartbeats"), ("ring failed", "ring"),
                             ("linear_combination", "ring"), ("unknown identifier", "unknown-identifier"),
                             ("type mismatch", "type-mismatch"), ("omega", "omega"), ("decide", "decide"),
                             ("rewrite", "rewrite"), ("did not find", "rewrite"), ("unfold", "unfold")):
                if pat in low:
                    return f"{key.upper()}:{cls}"
            return f"{key.upper()}:{x[:80]}"
    return "unknown"


def _max_rss_mb(stderr: str) -> int | None:
    m = re.search(r"(\d+)\s+maximum resident set size", stderr)
    return int(m.group(1)) // (1 << 20) if m else None


def process(item: dict, out: str, lean_env: str | None, work_root: str, timeout: int) -> dict:
    """Solve one package and, when DETERMINED, check the emitted proof with the production checker."""
    rec = {k: item[k] for k in ("family", "collection", "package_id", "status")}
    t0 = time.time()
    try:
        prob = load(item["dir"])
        res = solve(prob)
    except Exception as exc:                       # recorded, never silently dropped
        rec.update(outcome="MECH-ERROR", error=f"{type(exc).__name__}: {exc}"[:400], solve_s=round(time.time() - t0, 2))
        return rec
    rec.update(solve_s=round(time.time() - t0, 2), engine=res.status, rules=res.rules(),
               meta={k: v for k, v in prob.meta.items() if k != "p_literal"})
    if res.status == "STUCK":
        rec.update(outcome="MECH-STUCK", reason=res.reason, detail=res.detail)
        return rec
    t1 = time.time()
    try:
        text = emit_solution(prob, res)
    except Exception as exc:
        rec.update(outcome="MECH-ERROR", error=f"emit: {type(exc).__name__}: {exc}"[:400])
        return rec
    pdir = os.path.join(out, "proofs", item["collection"], item["package_id"].split("/", 1)[1]
                        if item["family"] != "halo2" else item["package_id"])
    os.makedirs(pdir, exist_ok=True)
    sol = os.path.join(pdir, "Solution.lean")
    with open(sol, "w", encoding="utf-8") as f:
        f.write(text)
    rec.update(emit_s=round(time.time() - t1, 2), proof=os.path.relpath(sol, out), proof_bytes=len(text.encode()),
               proof_sha256=hashlib.sha256(text.encode()).hexdigest())
    if not lean_env:
        rec["outcome"] = "MECH-EMITTED"
        return rec
    report = os.path.join(pdir, "check.json")
    cmd = [sys.executable, "-m", "zk_registry.check", item["dir"], sol, "--lean-env", lean_env, "--work-root",
           work_root, "--report", report, "--timeout", str(timeout)]
    if sys.platform == "darwin" and os.path.exists("/usr/bin/time"):
        cmd = ["/usr/bin/time", "-l"] + cmd
    t2 = time.time()
    scripts = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    r = subprocess.run(cmd, cwd=scripts, capture_output=True, text=True,
                       env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
    rec.update(check_s=round(time.time() - t2, 1), check_rss_mb=_max_rss_mb(r.stderr))
    try:
        with open(report, encoding="utf-8") as f:
            rep = json.load(f)
    except (OSError, ValueError):
        rec.update(outcome="MECH-PROOF-FAIL", verdict="ERROR", error_class="ERROR:no checker report",
                   checker_tail=(r.stdout + r.stderr)[-400:])
        return rec
    comp = rep.get("compile") if isinstance(rep.get("compile"), dict) else {}
    rec.update(verdict=rep["verdict"], compile_s=comp.get("compile_secs"))
    if rep["verdict"] == "PASS":
        rec["outcome"] = "MECH-SOLVED"
    else:
        rec.update(outcome="MECH-PROOF-FAIL", error_class=error_class(rep),
                   reasons=[x[:300] for k in ("error", "invalid", "fail") for x in rep.get(k, [])][:3])
    return rec


def summarize(records: list[dict]) -> dict:
    fams = ["sp1", "pico", "openvm", "halo2"]
    by = {f: {o: 0 for o in OUTCOMES} for f in fams}
    reasons: dict[str, dict[str, int]] = {}
    rules: dict[str, int] = {}
    for r in records:
        by[r["family"]][r["outcome"]] = by[r["family"]].get(r["outcome"], 0) + 1
        if r["outcome"] == "MECH-STUCK":
            side = "halo2" if r["family"] == "halo2" else "zkvm"
            reasons.setdefault(side, {})
            reasons[side][r["reason"]] = reasons[side].get(r["reason"], 0) + 1
        if r["outcome"] == "MECH-SOLVED":
            for k, v in r.get("rules", {}).items():
                rules[k] = rules.get(k, 0) + v
    checks = [r["check_s"] for r in records if "check_s" in r]
    rss = sorted(r["check_rss_mb"] for r in records if r.get("check_rss_mb"))
    return {"by_family": by, "stuck_reasons": reasons, "rules_in_solved": rules,
            "solve_s_total": round(sum(r.get("solve_s", 0) for r in records), 1),
            "check_s_total": round(sum(checks), 1), "check_s_max": max(checks, default=0),
            "check_rss_mb_max": rss[-1] if rss else None,
            "check_rss_mb_p95": rss[min(len(rss) - 1, int(0.95 * len(rss)))] if rss else None}


def run(packages: str, out: str, lean_env: str | None, work_root: str, jobs: int, timeout: int,
        only: str = "", statuses=("OPEN",)) -> list[dict]:
    from concurrent.futures import ThreadPoolExecutor
    items = [it for it in collect(packages, statuses) if only in it["dir"]]
    os.makedirs(out, exist_ok=True)
    t0 = time.time()
    with ThreadPoolExecutor(max_workers=max(1, jobs)) as ex:
        futs = [ex.submit(process, it, out, lean_env, work_root, timeout) for it in items]
        records = []
        for it, fu in zip(items, futs):
            rec = fu.result()
            records.append(rec)
            print(f"{rec['outcome']:16} {rec['family']:6} {rec['package_id'][:90]} "
                  f"{rec.get('reason') or rec.get('error_class') or ''}", flush=True)
    with open(os.path.join(out, "RESULTS.jsonl"), "w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r, sort_keys=True) + "\n")
    summary = summarize(records)
    summary.update(wall_s=round(time.time() - t0, 1), jobs=jobs, n=len(records),
                   tool={"name": NAME, "version": VERSION, "sources_sha256": sources_sha256()})
    with open(os.path.join(out, "SUMMARY.json"), "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=1, sort_keys=True)
    return records


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Battery P3: mechanical propagation over AIR and halo2 DET packages")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("run", help="solve every OPEN package and check the emitted proofs")
    a.add_argument("--packages", required=True, help="the packages/ directory of the production waves")
    a.add_argument("--out", required=True)
    a.add_argument("--lean-env", help="environment JSON for the production checker (omit: emit only)")
    a.add_argument("--work-root", default=None, help="parent of the checker's work directories")
    a.add_argument("--jobs", type=int, default=1)
    a.add_argument("--timeout", type=int, default=1800)
    a.add_argument("--only", default="", help="substring filter on package directories")
    a.add_argument("--status", action="append", help="package statuses to take (default OPEN)")
    s = sub.add_parser("solve", help="analyse one package; optionally write Solution.lean")
    s.add_argument("package")
    s.add_argument("--emit", help="write the solution here when DETERMINED")
    args = ap.parse_args(argv)
    if args.cmd == "run":
        run(args.packages, args.out, args.lean_env, args.work_root or os.path.join(args.out, "work"), args.jobs,
            args.timeout, args.only, tuple(args.status or ["OPEN"]))
        return 0
    prob = load(args.package)
    res = solve(prob)
    print(json.dumps({"status": res.status, "reason": res.reason, "rules": res.rules(), "detail": res.detail},
                     sort_keys=True))
    if args.emit and res.status == "DETERMINED":
        with open(args.emit, "w", encoding="utf-8") as f:
            f.write(emit_solution(prob, res))
    return 0


if __name__ == "__main__":
    sys.exit(main())
