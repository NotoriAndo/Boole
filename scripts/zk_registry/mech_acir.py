#!/usr/bin/env python3
"""Battery P3 (mechanical propagation) for Noir DET packages: a deterministic solver over the package's ACIR.

The engine reads the package's own program (``evidence/acir.json``, normalized and flattened by ``noir_acir``
exactly as the generator did) and first re-renders the constraint conjuncts with ``noir_lean_emit``; they must equal
the conjuncts of the package's ``Model.lean`` (and ``nWires`` / ``Inputs`` / ``Outputs`` must match), otherwise the
package is MECH-ERROR.  It then propagates *determined* wires (two satisfying assignments that agree on the inputs
agree on the wire) in dependency order:

* inputs are determined; a wire is *constant* when every satisfying assignment gives it one value (CONST: it is the
  only non-constant wire of an AssertZero and occurs linearly with a non-zero coefficient);
* LIN: the only undetermined wire of an AssertZero, occurring linearly, whose coefficient is a non-zero constant
  once constant partners of its quadratic terms are substituted;
* SPLIT: an AssertZero whose undetermined wires all occur linearly, are range-checked and have coefficients of one
  sign that form a mixed-radix system below p (the natural number they encode is unique);
* ISZERO: ``X·y + c·z + D = 0`` and ``μ·X·z = 0`` with ``X``, ``D`` determined and ``c`` a non-zero constant
  determine ``z`` (``z = 0`` when ``X ≠ 0``, ``z = -D/c`` otherwise);
* FIELDCUT: the compiler's field-to-integer cut ``x = 2^k·q + r`` with ``r < 2^k`` and the comparison against
  ``p`` (``q ≤ Q`` from a range-checked complement and, through an is-zero gadget on ``Q - q``, a bound on ``r``
  when ``q = Q``): ``2^k·q + r < p``, so ``q`` and ``r`` are the quotient and remainder of ``x.val``;
* black boxes (uninterpreted, shared by both assignments) and AND / XOR: outputs are determined once the inputs are;
* memory (list mode): a block's contents are determined when its initial witnesses and every earlier write (index
  and value) are; a read at a determined index of determined contents is determined;
* Brillig outputs are free: they count only once later constraints determine them.

The run stops at DETERMINED (every output determined) or STUCK (the undetermined outputs, the blocking constraint
kinds and whether Brillig hints feed them).  For DETERMINED packages ``emit_solution`` writes ``Solution.lean``
(the statement byte-identical, helpers above its doc comment): one small theorem per step (projections of the
constraint conjunction, ``rw`` with determined / constant wires, ``linear_combination`` with an exact certificate
computed here, multiples of ``p`` entering through ``mech_hp``), state definitions and one peel theorem per memory
block, and a main proof that chains the steps.  ``run`` checks each proof with the production checker
(``check.py``) and records MECH-SOLVED (PASS) / MECH-STUCK / MECH-PROOF-FAIL / MECH-ERROR per package.

Usage::

    PYTHONDONTWRITEBYTECODE=1 python3 -m zk_registry.mech_acir solve PKG_DIR [--out DIR]
    PYTHONDONTWRITEBYTECODE=1 python3 -m zk_registry.mech_acir run --packages ROOT --out DIR --lean-env ENV.json \\
        [--jobs N] [--rss-mb MB] [--timeout S] [--only ID ...] [--engine-only]
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import re
import sys
import threading
import time
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import noir_acir as A            # noqa: E402
from zk_registry import noir_lean_emit as NE      # noqa: E402

P = A.BN254
ENGINE_VERSION = "boole-zk-mech-acir 1.0"
NOIR_COLLECTIONS = ("noir-n1", "noir-n1/decomposition")
NOIR_RECOVERY = "recovery-r1/noir"
SOLVER_SOURCES = ["mech_acir.py"]   # not generator sources: a run records their sha256 in RUN.json
MAX_SPLIT_TERMS = 24          # SPLIT over more undetermined wires is left to other rules (omega cost)


def signed(c: int) -> int:
    return NE._signed(c, P)


# ------------------------------------------------------------------------------------------ polynomials

class Poly:
    """Integer polynomial over Lean atoms (strings such as ``w₁ 5``); used for exact certificates."""

    __slots__ = ("t",)

    def __init__(self, t: dict | None = None):
        self.t = {k: v for k, v in (t or {}).items() if v}

    @staticmethod
    def const(c: int) -> "Poly":
        return Poly({(): c})

    @staticmethod
    def atom(a: str) -> "Poly":
        return Poly({(a,): 1})

    def __add__(self, o: "Poly") -> "Poly":
        t = dict(self.t)
        for k, v in o.t.items():
            t[k] = t.get(k, 0) + v
        return Poly(t)

    def __neg__(self) -> "Poly":
        return Poly({k: -v for k, v in self.t.items()})

    def __sub__(self, o: "Poly") -> "Poly":
        return self + (-o)

    def __mul__(self, o) -> "Poly":
        if isinstance(o, int):
            return Poly({k: v * o for k, v in self.t.items()})
        t: dict = {}
        for k1, v1 in self.t.items():
            for k2, v2 in o.t.items():
                k = tuple(sorted(k1 + k2))
                t[k] = t.get(k, 0) + v1 * v2
        return Poly(t)

    __rmul__ = __mul__

    def is_zero(self) -> bool:
        return not self.t

    def div_p(self) -> "Poly":
        assert all(v % P == 0 for v in self.t.values()), "certificate residual is not a multiple of p"
        return Poly({k: v // P for k, v in self.t.items()})

    def mod_p(self) -> dict:
        return {k: v % P for k, v in self.t.items() if v % P}

    def render(self) -> str:
        if not self.t:
            return "0"
        out = []
        for k in sorted(self.t, key=lambda m: (len(m), m)):
            v = self.t[k]
            body = " * ".join(k)
            mag = abs(v)
            term = (str(mag) if not k else (body if mag == 1 else f"{mag} * {body}"))
            if not out:
                out.append(("-" if v < 0 else "") + term)
            else:
                out.append((" - " if v < 0 else " + ") + term)
        return "".join(out)


def lc_cert(goal: Poly, terms: list[tuple[int | Poly, str, Poly]]) -> str:
    """``linear_combination`` argument proving ``goal = 0`` from hypotheses ``name : poly = 0``: the given
    combination plus ``(residual / p) * mech_hp`` (the residual must vanish modulo p)."""
    res = goal
    parts = []
    for coef, name, hyp in terms:
        cpoly = Poly.const(coef) if isinstance(coef, int) else coef
        if cpoly.is_zero():
            continue
        res = res - cpoly * hyp
        parts.append(f"({cpoly.render()}) * {name}")
    q = res.div_p()
    if not q.is_zero():
        parts.append(f"({q.render()}) * mech_hp")
    return " + ".join(parts) if parts else "0"


def inv(c: int) -> int:
    return pow(c % P, P - 2, P)


# ------------------------------------------------------------------------------------------ package model

@dataclass
class Entry:
    kind: str                      # az | range | logic | bb | mem
    op: int | None = None          # opcode index in the flattened program (not for memory entries)
    parts: int = 1                 # top-level conjuncts the entry renders to (AND / XOR: 3)
    block: int | None = None       # memory block id


@dataclass
class Pkg:
    dir: str
    ns: str
    flat: A.Flat
    keys: list[str]
    entries: list[Entry]
    mem: dict                      # block id -> {"init": [...], "ops": [opcode index, ...]}
    bb: bool
    statement: str
    problem: dict
    paths: list = field(default_factory=list)            # per entry: (block prefix, first part index, block parts)

    @property
    def ops(self) -> list[dict]:
        return self.flat.opcodes


class ModelMismatch(ValueError):
    """The ACIR evidence does not render to the package's Lean model."""


def memory_layout(ops: list[dict]) -> tuple[list[int], dict]:
    """Blocks in the emitter's order (``noir_lean_emit.memory_conjuncts``) with their initial witnesses and ops."""
    blocks: dict[int, dict] = {}
    order: list[int] = []
    for k, op in enumerate(ops):
        if op["kind"] == "mem_init":
            if op["block"] not in blocks:
                order.append(op["block"])
            blocks[op["block"]] = {"init": list(op["init"]), "ops": []}
        elif op["kind"] == "mem_op":
            b = blocks.setdefault(op["block"], {"init": None, "ops": []})
            if op["block"] not in order:
                order.append(op["block"])
            b["ops"].append(k)
    return order, blocks


def build_entries(ops: list[dict]) -> tuple[list[Entry], dict]:
    out: list[Entry] = []
    for k, op in enumerate(ops):
        kd = op["kind"]
        if kd == "assert_zero":
            out.append(Entry("az", k))
        elif kd == "range":
            out.append(Entry("range", k))
        elif kd in ("and", "xor"):
            out.append(Entry("logic", k, parts=3))
        elif kd == "bb":
            pred = op["predicate"]
            if pred is not None and pred[0] == "c" and pred[1] % P == 0:
                continue
            out.append(Entry("bb", k))
        elif kd in ("mem_init", "mem_op", "brillig"):
            continue
        else:
            raise A.Unsupported(f"no model for opcode kind {kd}")
    order, blocks = memory_layout(ops)
    for b in order:
        out.append(Entry("mem", None, block=b))
    return out, blocks


def model_conjuncts(text: str) -> list[str]:
    if re.search(r"(?m)^def Block0 ", text):
        bodies = re.findall(r"(?ms)^def Block\d+ [^\n]*: Prop :=\n(.*?)\n\n", text)
    else:
        m = re.search(r"(?ms)^def Constraints [^\n]*: Prop :=\n(.*?)\n\n", text)
        if not m:
            raise ModelMismatch("no Constraints definition")
        bodies = [m.group(1)]
        if bodies[0].strip() == "True":
            return []
    out = []
    for body in bodies:
        out += [c[2:] if c.startswith("  ") else c for c in body.split(" ∧\n")]
    return out


def _model_list(text: str, name: str) -> list[int]:
    m = re.search(r"(?m)^def " + name + r" : List \(Fin nWires\) := \[([^\]]*)\]", text)
    if not m:
        raise ModelMismatch(f"no {name} list")
    return [int(x) for x in m.group(1).split(",") if x.strip()]


def load(pkg_dir: str) -> Pkg:
    with open(os.path.join(pkg_dir, "problem.json"), encoding="utf-8") as f:
        problem = json.load(f)
    chk = problem["checker"]
    imp = [fr for fr in chk["files"] if fr["role"] == "import"]
    if len(imp) != 1:
        raise ModelMismatch("expected one imported model file")
    ns = imp[0]["module"].rsplit(".", 1)[0]
    with open(os.path.join(pkg_dir, imp[0]["path"]), encoding="utf-8") as f:
        model = f.read()
    with open(os.path.join(pkg_dir, chk["statement_file"]), encoding="utf-8") as f:
        statement = f.read()
    with open(os.path.join(pkg_dir, "evidence", "acir.json"), encoding="utf-8") as f:
        decoded = json.load(f)
    flat = A.flatten(A.normalize_program(decoded))
    keys = A.bb_keys(flat.opcodes)
    want = NE.opcode_conjuncts(flat.opcodes, keys)
    have = model_conjuncts(model)
    if want != have:
        k = next((i for i, (x, y) in enumerate(zip(want, have)) if x != y), min(len(want), len(have)))
        raise ModelMismatch(f"conjunct {k} differs from Model.lean ({len(want)} rendered, {len(have)} in the model)")
    m = re.search(r"(?m)^abbrev nWires : ℕ := (\d+)$", model)
    if not m or int(m.group(1)) != flat.n_witnesses:
        raise ModelMismatch("nWires differs")
    if _model_list(model, "Inputs") != list(flat.inputs) or _model_list(model, "Outputs") != list(flat.outputs):
        raise ModelMismatch("Inputs / Outputs differ")
    pkg = make_pkg(flat, statement, problem, ns, pkg_dir)
    if len(pkg.entries) != len(want):
        raise ModelMismatch("entry count differs from the conjunct count")
    return pkg


def make_pkg(flat: A.Flat, statement: str = "", problem: dict | None = None, ns: str = "", pkg_dir: str = "") -> Pkg:
    """A package view of a flattened program (``load`` adds the checks against the package's model)."""
    entries, mem = build_entries(flat.opcodes)
    keys = A.bb_keys(flat.opcodes)
    pkg = Pkg(pkg_dir, ns, flat, keys, entries, mem, bool(keys), statement, problem or {})
    pkg.paths = _paths(entries)
    return pkg


def _paths(entries: list[Entry]) -> list[tuple[str, int, int]]:
    """Per entry: (block prefix, index of its first part within the block, parts in the block)."""
    n = len(entries)
    blocked = n > NE.BLOCK
    nb = (n + NE.BLOCK - 1) // NE.BLOCK if blocked else 1
    out = []
    for b in range(nb):
        chunk = entries[b * NE.BLOCK:(b + 1) * NE.BLOCK]
        total = sum(e.parts for e in chunk)
        pre = ""
        if blocked:
            pre = ".2" * b + (".1" if b < nb - 1 else "")
        j = 0
        for e in chunk:
            out.append((pre, j, total))
            j += e.parts
    return out


def path(pkg: Pkg, g: int, part: int = 0) -> str:
    pre, j, total = pkg.paths[g]
    j += part
    return pre + ".2" * j + (".1" if j < total - 1 else "")


UNFOLD = "\x00"


def proj(pkg: Pkg, g: int, part: int = 0) -> str:
    """``path`` for a ``have`` line; when the projection is a whole definition (a block, or ``Constraints``,
    holding a single conjunct) a marker asks ``Emitter.emit`` to add ``unfold <def> at <name>``."""
    _, _, total = pkg.paths[g]
    if total > 1:
        return path(pkg, g, part)
    name = f"Block{g // NE.BLOCK}" if len(pkg.entries) > NE.BLOCK else "Constraints"
    return path(pkg, g, part) + UNFOLD + name


def resolve_unfolds(text: str) -> str:
    return re.sub(r"(?m)^(\s*)have (\S+) := (.*)\x00(\w+)$", r"\1have \2 := \3\n\1unfold \4 at \2", text)


# ------------------------------------------------------------------------------------------ engine

@dataclass
class Step:
    kind: str                      # CONST | LIN | SPLIT | ISZERO | FIELDCUT | BB | LOGIC | MEMSTATE | MEMREAD
    wires: list[int]               # wires this step determines (CONST: the constant wire)
    entries: list[int]             # entries whose conjuncts the proof uses
    eq: list[int] = field(default_factory=list)       # determined wires used through w₁ x = w₂ x
    cs: list[int] = field(default_factory=list)       # constant wires substituted
    data: dict = field(default_factory=dict)


@dataclass
class Result:
    status: str                    # DETERMINED | STUCK
    steps: list[Step]
    const: dict
    det: set
    undetermined_outputs: list[int]
    stuck: dict = field(default_factory=dict)
    rules: dict = field(default_factory=dict)


def eff_form(e: A.Expr, const: dict) -> tuple[dict, set, int]:
    """``e`` with the constant wires substituted: linear coefficients of the non-constant wires (exact sums of the
    rendered signed coefficients), the non-constant wires of products of two non-constant wires, the constant."""
    lin: dict[int, int] = {}
    nonlin: set[int] = set()
    c0 = signed(e.const) if e.const % P else 0
    for q, a, b in e.mul:
        q = signed(q)
        if a in const and b in const:
            c0 += q * const[a] * const[b]
        elif a in const:
            lin[b] = lin.get(b, 0) + q * const[a]
        elif b in const:
            lin[a] = lin.get(a, 0) + q * const[b]
        else:
            nonlin |= {a, b}
    for q, w in e.lin:
        if w in const:
            c0 += signed(q) * const[w]
        else:
            lin[w] = lin.get(w, 0) + signed(q)
    return lin, nonlin, c0


def range_bounds(pkg: Pkg) -> dict:
    """wire -> (bits, entry index) of its tightest RANGE conjunct."""
    out: dict = {}
    for g, ent in enumerate(pkg.entries):
        if ent.kind == "range":
            op = pkg.ops[ent.op]
            if op["input"][0] == "w":
                w = op["input"][1]
                if w not in out or op["bits"] < out[w][0]:
                    out[w] = (op["bits"], g)
    return out


class Engine:
    def __init__(self, pkg: Pkg):
        self.pkg = pkg
        self.const: dict[int, int] = {}
        self.det: set[int] = set(pkg.flat.inputs)
        self.steps: list[Step] = []
        self.rng = range_bounds(pkg)
        self.changed = False
        self.by_wire: dict[int, list[int]] = {}
        for g, ent in enumerate(pkg.entries):
            if ent.kind == "az":
                for x in pkg.ops[ent.op]["expr"].witnesses():
                    self.by_wire.setdefault(x, []).append(g)
        self.mem_known: dict[int, set] = {b: set() for b in pkg.mem}      # known state ids per block
        self.mem_read_done: set = set()

    # -- helpers
    def add_step(self, st: Step) -> None:
        self.steps.append(st)
        if st.kind == "CONST":
            x = st.wires[0]
            self.const[x] = st.data["value"]
            self.det.add(x)
        elif st.kind != "MEMSTATE":
            self.det.update(st.wires)
        self.changed = True

    def split_deps(self, wires) -> tuple[list[int], list[int]]:
        ws = sorted(set(wires))
        return [w for w in ws if w not in self.const], [w for w in ws if w in self.const]

    # -- rules
    def rule_az(self, g: int) -> None:
        e = self.pkg.ops[self.pkg.entries[g].op]["expr"]
        ws = sorted(e.witnesses())
        lin, nonlin, c0 = eff_form(e, self.const)
        occ = sorted({w for w, c in lin.items() if c % P} | nonlin)
        cs = [w for w in ws if w in self.const]
        if len(occ) == 1 and occ[0] not in nonlin and occ[0] not in self.const:
            x = occ[0]
            self.add_step(Step("CONST", [x], [g], [], cs, {"value": (-c0 * inv(lin[x])) % P, "coef": lin[x]}))
            return
        undet = [x for x in occ if x not in self.det]
        if not undet:
            return
        eq = [w for w in ws if w in self.det and w not in self.const]
        if len(undet) == 1:
            x = undet[0]
            if x not in nonlin:
                self.add_step(Step("LIN", [x], [g], eq, cs, {"coef": lin[x]}))
            return
        self.rule_split(g, lin, nonlin, undet, eq, cs)

    def rule_split(self, g: int, lin: dict, nonlin: set, undet: list[int], eq: list[int], cs: list[int]) -> None:
        if len(undet) > MAX_SPLIT_TERMS or any(u in nonlin or u not in self.rng for u in undet):
            return
        co = {u: signed(lin[u]) for u in undet}
        if not (all(c > 0 for c in co.values()) or all(c < 0 for c in co.values())):
            return
        tot = 0
        for u in sorted(undet, key=lambda u: abs(co[u])):
            if abs(co[u]) <= tot:
                return
            tot += abs(co[u]) * ((1 << self.rng[u][0]) - 1)
        if tot >= P:
            return
        self.add_step(Step("SPLIT", sorted(undet), [g] + [self.rng[u][1] for u in sorted(undet)], eq, cs,
                           {"coef": co}))

    def rule_iszero(self) -> None:
        ents = self.pkg.entries
        for g2, ent in enumerate(ents):
            if ent.kind != "az":
                continue
            e2 = self.pkg.ops[ent.op]["expr"]
            u2 = [x for x in e2.witnesses() if x not in self.det]
            if len(u2) != 1 or e2.const % P:
                continue
            z = u2[0]
            if any(w != z for _, w in e2.lin) or any(z not in (a, b) or a == b for _, a, b in e2.mul):
                continue
            for g1 in self.by_wire.get(z, ()):
                if g1 == g2:
                    continue
                e1 = self.pkg.ops[ents[g1].op]["expr"]
                u1 = [x for x in e1.witnesses() if x not in self.det]
                if len(u1) != 2 or z not in u1:
                    continue
                y = u1[0] if u1[1] == z else u1[1]
                if any(z in (a, b) for _, a, b in e1.mul) or any(a == b == y for _, a, b in e1.mul):
                    continue
                cz = sum(signed(q) for q, w in e1.lin if w == z)
                if cz % P == 0:
                    continue
                ws = sorted((e1.witnesses() | e2.witnesses()) - {y, z})
                eq, cs = self.split_deps(ws)
                st = Step("ISZERO", [z], [g1, g2], eq, cs, {"y": y, "cz": cz})
                if iszero_shape(self.pkg, st, self.const) is None:
                    continue
                self.add_step(st)
                break

    def rule_bb(self, g: int) -> None:
        op = self.pkg.ops[self.pkg.entries[g].op]
        if all(o in self.det for o in op["outputs"]):
            return
        ins = [x[1] for x in op["inputs"] if x[0] == "w"]
        if not all(i in self.det for i in ins):
            return
        pred = op["predicate"]
        if pred is not None and pred[0] == "w" and self.const.get(pred[1], 0) % P == 0:
            return
        cs = [pred[1]] if pred is not None and pred[0] == "w" else []
        self.add_step(Step("BB", list(op["outputs"]), [g], sorted(set(ins)), cs, {}))

    def rule_logic(self, g: int) -> None:
        op = self.pkg.ops[self.pkg.entries[g].op]
        if op["output"] in self.det:
            return
        ins = [x[1] for x in (op["lhs"], op["rhs"]) if x[0] == "w"]
        if all(i in self.det for i in ins):
            self.add_step(Step("LOGIC", [op["output"]], [g], sorted(set(ins)), [], {}))

    def mem_ops(self, b: int) -> list[dict] | None:
        out = []
        for k in self.pkg.mem[b]["ops"]:
            op = self.pkg.ops[k]
            wr = op["write"].as_const()
            iw, vw = op["index"].as_witness(), op["value"].as_witness()
            if op["predicate"] is not None or wr not in (0, 1) or iw is None or vw is None:
                return None
            out.append({"write": wr == 1, "i": iw, "v": vw})
        return out

    def rule_mem(self, g: int) -> None:
        b = self.pkg.entries[g].block
        ops = self.mem_ops(b)
        blk = self.pkg.mem[b]
        if ops is None or blk["init"] is None:
            return
        known = self.mem_known[b]
        state = 0
        if 0 not in known:
            if not all(x in self.det for x in blk["init"]):
                return
            self.add_step(Step("MEMSTATE", [], [g], sorted(set(blk["init"])), [], {"block": b, "state": 0}))
            known.add(0)
        for t, o in enumerate(ops):
            if o["write"]:
                nxt = state + 1
                if nxt not in known:
                    if state in known and o["i"] in self.det and o["v"] in self.det:
                        self.add_step(Step("MEMSTATE", [], [g], sorted({o["i"], o["v"]}), [],
                                           {"block": b, "state": nxt, "op": t}))
                        known.add(nxt)
                state = nxt
            elif (b, t) not in self.mem_read_done and state in known and o["i"] in self.det and o["v"] not in self.det:
                self.add_step(Step("MEMREAD", [o["v"]], [g], [o["i"]], [], {"block": b, "state": state, "op": t}))
                self.mem_read_done.add((b, t))

    def run(self) -> Result:
        self.changed = True
        rounds = 0
        while self.changed:
            self.changed = False
            rounds += 1
            for g, ent in enumerate(self.pkg.entries):
                if ent.kind == "az":
                    self.rule_az(g)
                elif ent.kind == "bb":
                    self.rule_bb(g)
                elif ent.kind == "logic":
                    self.rule_logic(g)
                elif ent.kind == "mem":
                    self.rule_mem(g)
            if not self.changed:
                self.rule_iszero()
            if not self.changed:
                self.rule_fieldcut()
            if not self.changed:
                self.rule_euclid()
        und = [o for o in self.pkg.flat.outputs if o not in self.det]
        rules: dict = {}
        for st in self.steps:
            rules[st.kind] = rules.get(st.kind, 0) + 1
        res = Result("DETERMINED" if not und else "STUCK", self.steps, self.const, self.det, und, rules=rules)
        res.rules["rounds"] = rounds
        if und:
            res.stuck = stuck_report(self.pkg, self)
        return res

    def rule_euclid(self) -> None:
        for g, ent in enumerate(self.pkg.entries):
            if ent.kind == "az":
                m = euclid_match(self, g)
                if m is not None:
                    self.add_step(m)

    # FIELDCUT is defined below (pattern of the compiler's field-to-integer conversion)
    def rule_fieldcut(self) -> None:
        for g, ent in enumerate(self.pkg.entries):
            if ent.kind != "az":
                continue
            m = fieldcut_match(self, g)
            if m is not None:
                self.add_step(m)


def iszero_shape(pkg: Pkg, st: Step, const: dict) -> dict | None:
    """Polynomials of the is-zero step (after substitution, side 2 atoms); None when ``Y`` is not a non-zero
    multiple of the ``z`` coefficient of the second constraint."""
    g1, g2 = st.entries
    z, y = st.wires[0], st.data["y"]
    sub = Subst(2, st.eq, {w: const[w] for w in st.cs})
    e1 = pkg.ops[pkg.entries[g1].op]["expr"]
    e2 = pkg.ops[pkg.entries[g2].op]["expr"]
    ycoef, rest = Poly(), Poly()
    for q, a, b in e1.mul:
        t = Poly.const(signed(q)) * sub.term(a) * sub.term(b)
        if y in (a, b):
            ycoef = ycoef + Poly.const(signed(q)) * sub.term(b if a == y else a)
        else:
            rest = rest + t
    for q, w in e1.lin:
        if w == y:
            ycoef = ycoef + Poly.const(signed(q))
        elif w != z:
            rest = rest + Poly.const(signed(q)) * sub.term(w)
    if e1.const % P:
        rest = rest + Poly.const(signed(e1.const))
    xcoef = Poly()
    for q, a, b in e2.mul:
        xcoef = xcoef + Poly.const(signed(q)) * sub.term(b if a == z else a)
    for q, w in e2.lin:
        xcoef = xcoef + Poly.const(signed(q))
    ym, xm = ycoef.mod_p(), xcoef.mod_p()
    if not xm or set(ym) != set(xm):
        return None
    k0 = next(iter(xm))
    lam = ym[k0] * inv(xm[k0]) % P
    if any(ym[k] != lam * xm[k] % P for k in xm):
        return None
    return {"Y": ycoef, "D": rest, "X2": xcoef, "lam": lam}


# ------------------------------------------------------------------------------------------ FIELDCUT

def _lin_only(e: A.Expr, ws) -> bool:
    return not any(a in ws or b in ws for _, a, b in e.mul)


def _expr(eng: "Engine", g: int) -> A.Expr:
    return eng.pkg.ops[eng.pkg.entries[g].op]["expr"]


def _lin_coef(e: A.Expr, x: int) -> int:
    return sum(signed(q) for q, w in e.lin if w == x)


def fieldcut_match(eng: "Engine", g0: int) -> Step | None:
    """The compiler's field-to-integer cut (truncation / cast of a Field, with the comparison against p):

    * E0: ``X ± (M·q + r) = 0`` with ``X`` determined, ``M = 2^k``, ``r`` range-checked to ``k`` bits, ``q``
      range-checked;
    * EB: ``q ± t + c = 0`` with ``t`` range-checked: ``q ≤ Q``;
    * EZ: ``λ·(Q - q)·y + c_z·z + d = 0`` (the is-zero gadget on ``Q - q``): ``q = Q`` forces ``z = z0 = -d/c_z``;
    * ER: ``c_s·s + a·z·r + b·z·z + c·z = 0`` with ``s`` range-checked to ``m`` bits: when ``z = z0`` it reads
      ``s = r + B``, so ``r ≤ 2^m - 1 - B``.

    When ``M·Q < p`` and ``M·Q + 2^m - 1 - B < p`` every satisfying assignment has ``M·q.val + r.val < p``, hence
    ``M·q.val + r.val = X.val``: ``q`` and ``r`` are determined."""
    e0 = _expr(eng, g0)
    U = [x for x in e0.witnesses() if x not in eng.det]
    if len(U) != 2 or not _lin_only(e0, U) or not all(u in eng.rng for u in U):
        return None
    co = {u: signed(_lin_coef(e0, u)) for u in U}
    r, q = sorted(U, key=lambda u: abs(co[u]))
    M = abs(co[q])
    if abs(co[r]) != 1 or M < 2 or M & (M - 1) or (1 << eng.rng[r][0]) > M or (co[r] > 0) != (co[q] > 0):
        return None
    nq = eng.rng[q][0]
    for gb in eng.by_wire.get(q, ()):
        eb = _expr(eng, gb)
        if gb == g0 or eb.mul or len(eb.witnesses()) != 2:
            continue
        t = next(x for x in eb.witnesses() if x != q)
        if t in eng.det or t not in eng.rng:
            continue
        cq, ct, c0 = signed(_lin_coef(eb, q)), signed(_lin_coef(eb, t)), signed(eb.const)
        if abs(cq) != 1 or abs(ct) != 1:
            continue
        if cq < 0:
            ct, c0 = -ct, -c0
        nt = eng.rng[t][0]
        if (1 << nq) + (1 << nt) + abs(c0) >= P:
            continue
        Q = (1 << nt) - 1 - c0 if ct < 0 else -c0
        if Q < 0 or M * Q >= P:
            continue
        rb = _fieldcut_rbound(eng, q, r, Q)
        if rb is None:
            continue
        if M * Q + rb["bound"] >= P:
            continue
        eq, cs = eng.split_deps(e0.witnesses() - {q, r})
        return Step("FIELDCUT", [q, r], [g0, gb, rb["g1"], rb["g3"]], eq, cs,
                    {"q": q, "r": r, "t": t, "M": M, "Q": Q, "coef": {q: co[q], r: co[r]}, **rb})
    return None


def _fieldcut_rbound(eng: "Engine", q: int, r: int, Q: int) -> dict | None:
    for g1 in eng.by_wire.get(q, ()):
        e1 = _expr(eng, g1)
        ws1 = e1.witnesses()
        if len(ws1) != 3 or any(w in eng.det for w in ws1):
            continue
        for z in ws1 - {q}:
            y = next(x for x in ws1 if x not in (q, z))
            if any(tuple(sorted((a, b))) != tuple(sorted((q, y))) for _, a, b in e1.mul):
                continue
            if any(w not in (y, z) for _, w in e1.lin):
                continue
            lqy = sum(signed(c) for c, _, _ in e1.mul)
            ly, cz = _lin_coef(e1, y), _lin_coef(e1, z)
            d = signed(e1.const) if e1.const % P else 0
            if lqy % P == 0 or (ly + lqy * Q) % P or cz % P == 0:
                continue
            z0 = (-d * inv(cz)) % P
            for g3 in eng.by_wire.get(z, ()):
                e3 = _expr(eng, g3)
                ws3 = e3.witnesses()
                if g3 == g1 or e3.const % P or r not in ws3 or not 2 <= len(ws3) <= 3:
                    continue
                s = next((x for x in ws3 if x not in (z, r)), None)
                if s is not None and (s not in eng.rng or s == q):
                    continue
                ok, a_r, czz = True, 0, 0
                for c, a, b in e3.mul:
                    pr = tuple(sorted((a, b)))
                    if pr == tuple(sorted((z, r))):
                        a_r += signed(c)
                    elif pr == (z, z):
                        czz += signed(c)
                    else:
                        ok = False
                if not ok or any(w not in (z, s) for _, w in e3.lin):
                    continue
                czl = _lin_coef(e3, z)
                base = {"z": z, "y": y, "z0": z0, "cz": cz, "g1": g1, "g3": g3}
                if s is None:          # z·(a·r + C) = 0: r = R0 when z = z0 ≠ 0
                    if (a_r * z0) % P == 0:
                        continue
                    r0 = (-(czz * z0 * z0 + czl * z0) * inv(a_r * z0)) % P
                    if r0 >= P // 2:
                        continue
                    return {"bound": r0, "s": None, "R0": r0, "ar": a_r, **base}
                cs_ = _lin_coef(e3, s)
                if abs(cs_) != 1:
                    continue
                if (-z0 * a_r * cs_) % P != 1:
                    continue
                B = (-(z0 * czl + z0 * z0 * czz) * cs_) % P
                m = eng.rng[s][0]
                if B >= (1 << m) or (1 << m) + (1 << eng.rng[r][0]) >= P:
                    continue
                return {"bound": (1 << m) - 1 - B, "s": s, "B": B, "cs": cs_, **base}
    return None


def euclid_match(eng: "Engine", g0: int) -> Step | None:
    """Euclidean division by a determined divisor: E0 ``X ± (d·q + r) = 0`` (``q``, ``r`` range-checked) and
    EL ``d - r - t - 1 = 0`` (``t`` range-checked, so ``r < d``).  With ``d ≤ 2^nr + 2^nt - 1`` and
    ``d·q + r < p`` for every value in range, ``q`` and ``r`` are the quotient and remainder of ``X.val``."""
    e0 = _expr(eng, g0)
    U = [x for x in e0.witnesses() if x not in eng.det]
    if len(U) != 2 or not all(u in eng.rng for u in U):
        return None
    um = [(c, a, b) for c, a, b in e0.mul if a in U or b in U]
    if len(um) != 1 or um[0][1] == um[0][2]:
        return None
    c, a, b = um[0]
    q = a if a in U else b
    d = b if q == a else a
    if d in U or d in eng.const or d not in eng.det:
        return None
    r = U[0] if U[1] == q else U[1]
    if any(w == q for _, w in e0.lin) or any(r in (x, y) for _, x, y in e0.mul):
        return None
    cr, cm = signed(_lin_coef(e0, r)), signed(c)
    if abs(cr) != 1 or cm != cr:
        return None
    nq, nr = eng.rng[q][0], eng.rng[r][0]
    for g1 in eng.by_wire.get(r, ()):
        e1 = _expr(eng, g1)
        ws = e1.witnesses()
        if g1 == g0 or e1.mul or len(ws) != 3 or d not in ws:
            continue
        t = next(x for x in ws if x not in (d, r))
        if t in eng.det or t not in eng.rng:
            continue
        cd, cr1, ct, c1 = (signed(_lin_coef(e1, d)), signed(_lin_coef(e1, r)), signed(_lin_coef(e1, t)),
                           signed(e1.const) if e1.const % P else 0)
        if cd < 0:
            cd, cr1, ct, c1 = -cd, -cr1, -ct, -c1
        if (cd, cr1, ct, c1) != (1, -1, -1, -1):
            continue
        nt = eng.rng[t][0]
        dmax = (1 << nr) - 1 + (1 << nt) - 1 + 1
        qmax = (1 << nq) - 1
        if dmax * qmax + (1 << nr) - 1 >= P or (1 << nr) + (1 << nt) + 1 >= P:
            continue
        eq, cs = eng.split_deps(e0.witnesses() - {q, r})
        return Step("EUCLID", [q, r], [g0, g1], eq, cs,
                    {"q": q, "r": r, "d": d, "t": t, "dmax": dmax, "qmax": qmax, "sign": cr})
    return None


# ------------------------------------------------------------------------------------------ stuck report

def stuck_report(pkg: Pkg, eng: Engine) -> dict:
    """Undetermined cone of the undetermined outputs: blocking constraint kinds and Brillig dependence."""
    brillig = {w for op in pkg.ops if op["kind"] == "brillig" for w in op["outputs"]}
    ent_wires: list[set] = []
    for ent in pkg.entries:
        if ent.kind == "az":
            ws = set(pkg.ops[ent.op]["expr"].witnesses())
        elif ent.kind == "range":
            x = pkg.ops[ent.op]["input"]
            ws = {x[1]} if x[0] == "w" else set()
        elif ent.kind == "logic":
            op = pkg.ops[ent.op]
            ws = {x[1] for x in (op["lhs"], op["rhs"]) if x[0] == "w"} | {op["output"]}
        elif ent.kind == "bb":
            op = pkg.ops[ent.op]
            ws = {x[1] for x in op["inputs"] if x[0] == "w"} | set(op["outputs"])
            if op["predicate"] is not None and op["predicate"][0] == "w":
                ws.add(op["predicate"][1])
        else:
            blk = pkg.mem[ent.block]
            ws = set(blk["init"] or [])
            for k in blk["ops"]:
                o = pkg.ops[k]
                ws |= o["index"].witnesses() | o["value"].witnesses() | o["write"].witnesses()
        ent_wires.append(ws)
    by_wire: dict[int, list[int]] = {}
    for g, ws in enumerate(ent_wires):
        for x in ws:
            by_wire.setdefault(x, []).append(g)
    und = [o for o in pkg.flat.outputs if o not in eng.det]
    cone, todo, ents = set(und), list(und), set()
    while todo:
        x = todo.pop()
        for g in by_wire.get(x, ()):
            if g in ents:
                continue
            ents.add(g)
            for y in ent_wires[g]:
                if y not in eng.det and y not in cone:
                    cone.add(y)
                    todo.append(y)
    kinds: dict[str, int] = {}

    def bump(k: str) -> None:
        kinds[k] = kinds.get(k, 0) + 1

    for g in sorted(ents):
        ent = pkg.entries[g]
        u = [x for x in ent_wires[g] if x not in eng.det]
        if not u or ent.kind == "range":
            continue
        if ent.kind == "az":
            e = pkg.ops[ent.op]["expr"]
            nonlin = any((a in u or b in u) and not ((a in eng.const) or (b in eng.const)) for _, a, b in e.mul)
            if nonlin:
                bump("az-nonlinear")
            elif len(u) >= 2:
                bump("az-multi-unknown-ranged" if all(x in eng.rng for x in u) else "az-multi-unknown")
            else:
                bump("az-degenerate")
        else:
            bump({"bb": "bb-undetermined-input", "logic": "logic-undetermined-operand",
                  "mem": "mem-undetermined"}[ent.kind])
    only_range = [x for x in cone if all(pkg.entries[g].kind == "range" for g in by_wire.get(x, ()))]
    if only_range:
        kinds["unconstrained-wires"] = len(only_range)
    gadgets = detect_gadgets(pkg, eng, ents)
    return {"undetermined_outputs": len(und), "cone_wires": len(cone),
            "brillig_in_cone": len(cone & brillig), "brillig_dependent": bool(cone & brillig),
            "blocking": dict(sorted(kinds.items())), "gadgets": gadgets}


def detect_gadgets(pkg: Pkg, eng: Engine, ents: set) -> list[str]:
    """Names of recognisable compiler idioms among the blocking constraints (for the report)."""
    out = set()
    for g in ents:
        ent = pkg.entries[g]
        if ent.kind != "az":
            continue
        e = pkg.ops[ent.op]["expr"]
        u = [x for x in e.witnesses() if x not in eng.det]
        if len(e.mul) == 1 and len(u) >= 2:
            _, a, b = e.mul[0]
            if (a in u) != (b in u) and any(w in u and w in eng.rng for _, w in e.lin):
                out.add("euclidean-division")
        if len(u) == 2 and not e.mul and all(x in eng.rng for x in u):
            co = sorted(abs(signed(sum(signed(q) for q, w in e.lin if w == x))) for x in u)
            if co[0] == 1 and co[1] > 1 and co[1] & (co[1] - 1) == 0:
                out.add("integer-cut")
    return sorted(out)


def solve(pkg: Pkg) -> Result:
    return Engine(pkg).run()


# ------------------------------------------------------------------------------------------ Lean emission

class Subst:
    """Atoms of one side after the step's rewrites: constants -> numerals, determined (eq) wires -> ``w₂ x``,
    other wires -> ``w₁ x`` / ``w₂ x`` (or ``w x`` for single-assignment theorems: side 0)."""

    def __init__(self, side: int, eq, const: dict):
        self.side, self.eq, self.const = side, set(eq), const

    def name(self, x: int) -> str:
        return f"w {x}" if self.side == 0 else f"w{'₁' if self.side == 1 else '₂'} {x}"

    def term(self, x: int) -> Poly:
        if x in self.const:
            return Poly.const(self.const[x])
        if x in self.eq:
            return Poly.atom(f"w₂ {x}")
        return Poly.atom(self.name(x))


def expr_poly(e: A.Expr, sub: Subst) -> Poly:
    p = Poly()
    for q, a, b in e.mul:
        p = p + Poly.const(signed(q)) * sub.term(a) * sub.term(b)
    for q, w in e.lin:
        p = p + Poly.const(signed(q)) * sub.term(w)
    if e.const % P:
        p = p + Poly.const(signed(e.const))
    return p


class Emitter:
    def __init__(self, pkg: Pkg, res: Result):
        self.pkg, self.res = pkg, res
        self.bb = pkg.bb
        self.helpers: list[str] = []
        self.body: list[str] = []
        self.eq_ready: set[int] = set()
        self.const_thm: set[int] = set()
        self.n = 0
        self.inputs_pos = {}
        for k, x in enumerate(pkg.flat.inputs):
            self.inputs_pos.setdefault(x, k)
        self.input_chain = -1
        self.mem_defs: set = set()                     # (block, state) with an emitted state definition
        self.mem_peel: dict[int, dict] = {}

    # -- names and binders
    def b1(self) -> str:
        return ("(bb : BlackBox) " if self.bb else "") + "(w : Fin nWires → F) " + \
            f"(h : Constraints {'bb ' if self.bb else ''}w)"

    def b2(self) -> str:
        c = "bb " if self.bb else ""
        return ("(bb : BlackBox) " if self.bb else "") + "(w₁ w₂ : Fin nWires → F) " + \
            f"(h₁ : Constraints {c}w₁) (h₂ : Constraints {c}w₂)"

    def a1(self, side: str) -> str:
        return ("bb " if self.bb else "") + f"w{side} h{side}"

    def a2(self) -> str:
        return ("bb " if self.bb else "") + "w₁ w₂ h₁ h₂"

    def kref(self, x: int, side: str) -> str:
        return f"mech_k{x} {self.a1(side)}"

    @staticmethod
    def eqb(xs) -> str:
        return "".join(f" (e{x} : w₁ {x} = w₂ {x})" for x in xs)

    @staticmethod
    def eqa(xs) -> str:
        return "".join(f" e{x}" for x in xs)

    # -- main-body facts
    def need_eq(self, x: int) -> None:
        if x in self.eq_ready:
            return
        if x in self.res.const and x in self.const_thm:
            self.body.append(f"  have e{x} : w₁ {x} = w₂ {x} := ({self.kref(x, '₁')}).trans ({self.kref(x, '₂')}).symm")
        elif x in self.inputs_pos:
            k = self.inputs_pos[x]
            while self.input_chain < k:
                self.input_chain += 1
                src = "hin" if self.input_chain == 0 else f"t{self.input_chain - 1}.2"
                self.body.append(f"  have t{self.input_chain} := List.forall_mem_cons.1 {src}")
                xi = self.pkg.flat.inputs[self.input_chain]
                if xi not in self.eq_ready:
                    self.body.append(f"  have e{xi} : w₁ {xi} = w₂ {xi} := t{self.input_chain}.1")
                    self.eq_ready.add(xi)
            return
        else:
            raise AssertionError(f"no equality fact for wire {x}")
        self.eq_ready.add(x)

    def rw_lines(self, hyp: str, eq: list[int], cs: list[int], side: str, wires: set) -> list[str]:
        out = []
        cw = [x for x in cs if x in wires]
        if cw:
            out.append(f"  rw [{', '.join(self.kref(x, side) for x in cw)}] at {hyp}")
        ew = [x for x in eq if x in wires] if side == "₁" else []
        if ew:
            out.append(f"  rw [{', '.join(f'e{x}' for x in ew)}] at {hyp}")
        return out

    # -- steps
    def emit(self) -> tuple[str, str]:
        self.helpers.append(HELPERS)
        if any(e.kind == "mem" for e in self.pkg.entries):
            self.helpers.append(MEM_HELPERS)
        for st in self.res.steps:
            getattr(self, "st_" + st.kind.lower())(st)
            self.n += 1
        intro = "  intro " + ("bb " if self.bb else "") + "w₁ w₂ h₁ h₂ hin"
        outs = []
        for o in self.pkg.flat.outputs:
            self.need_eq(o)
        for o in self.pkg.flat.outputs:
            outs.append(f"  refine List.forall_mem_cons.2 ⟨e{o}, ?_⟩")
        outs += ["  intro _ hnil", "  cases hnil"]
        return resolve_unfolds("\n".join(self.helpers)), "\n".join([intro] + self.body + outs)

    def st_const(self, st: Step) -> None:
        x, g = st.wires[0], st.entries[0]
        e = self.pkg.ops[self.pkg.entries[g].op]["expr"]
        sub = Subst(0, [], {w: self.res.const[w] for w in st.cs})
        a = expr_poly(e, sub)
        v = st.data["value"]
        cert = lc_cert(Poly.atom(f"w {x}") - Poly.const(v), [(signed(inv(st.data["coef"])), "a", a)])
        lines = [f"theorem mech_k{x} {self.b1()} : w {x} = (({v} : ℕ) : F) := by", f"  have a := h{proj(self.pkg, g)}"]
        cw = [w for w in st.cs]
        if cw:
            lines.append(f"  rw [{', '.join(f'mech_k{w} ' + ('bb ' if self.bb else '') + 'w h' for w in cw)}] at a")
        lines.append(f"  linear_combination {cert}")
        self.helpers.append("\n".join(lines) + "\n")
        self.const_thm.add(x)

    def _pair_hyps(self, g: int, st: Step, names=("a", "b")) -> tuple[list[str], Poly, Poly]:
        e = self.pkg.ops[self.pkg.entries[g].op]["expr"]
        wires = set(e.witnesses())
        cmap = {w: self.res.const[w] for w in st.cs}
        lines = [f"  have {names[0]} := h₁{proj(self.pkg, g)}", f"  have {names[1]} := h₂{proj(self.pkg, g)}"]
        lines += self.rw_lines(names[0], st.eq, st.cs, "₁", wires)
        lines += self.rw_lines(names[1], st.eq, st.cs, "₂", wires)
        pa = expr_poly(e, Subst(1, st.eq, cmap))
        pb = expr_poly(e, Subst(2, st.eq, cmap))
        return lines, pa, pb

    def step_head(self, st: Step, concl: str) -> str:
        return f"theorem mech_s{self.n} [Fact (Nat.Prime p)] {self.b2()}{self.eqb(st.eq)} : {concl} := by"

    def call(self, st: Step, extra: str = "") -> str:
        for x in st.eq:
            self.need_eq(x)
        return f"mech_s{self.n} {self.a2()}{self.eqa(st.eq)}{extra}"

    def bind(self, xs: list[int], term: str) -> None:
        if len(xs) == 1:
            self.body.append(f"  have e{xs[0]} : w₁ {xs[0]} = w₂ {xs[0]} := {term}")
        else:
            names = [f"e{x}" if x not in self.eq_ready else "_" for x in xs]
            self.body.append(f"  obtain ⟨{', '.join(names)}⟩ := {term}")
        self.eq_ready.update(xs)

    def st_lin(self, st: Step) -> None:
        x, g = st.wires[0], st.entries[0]
        lines, pa, pb = self._pair_hyps(g, st)
        d = signed(inv(st.data["coef"]))
        cert = lc_cert(Poly.atom(f"w₁ {x}") - Poly.atom(f"w₂ {x}"), [(d, "a", pa), (-d, "b", pb)])
        self.helpers.append("\n".join([self.step_head(st, f"w₁ {x} = w₂ {x}")] + lines +
                                      [f"  linear_combination {cert}"]) + "\n")
        self.bind([x], self.call(st))

    def st_split(self, st: Step) -> None:
        g = st.entries[0]
        us = st.wires
        co = st.data["coef"]
        lines, pa, pb = self._pair_hyps(g, st)
        rng = range_bounds(self.pkg)
        for k, u in enumerate(us):
            rp = proj(self.pkg, rng[u][1])
            lines += [f"  have r{k}a := h₁{rp}", f"  have r{k}b := h₂{rp}"]
        lines.append(f"  have hp' : p = {P} := rfl")

        def nat_sum(side: str) -> str:
            return " + ".join((f"{abs(co[u])} * " if abs(co[u]) != 1 else "") + f"(w{side} {u}).val" for u in us)

        goal = Poly()
        for u in us:
            goal = goal + Poly.const(abs(co[u])) * (Poly.atom(f"w₁ {u}") - Poly.atom(f"w₂ {u}"))
        sgn = 1 if all(c > 0 for c in co.values()) else -1
        cert = lc_cert(goal, [(sgn, "a", pa), (-sgn, "b", pb)])
        lines += [f"  have hF : (({nat_sum('₁')} : ℕ) : F) = (({nat_sum('₂')} : ℕ) : F) := by",
                  "    push_cast [mech_cv]", f"    linear_combination {cert}",
                  "  have hN := mech_lift hF (by omega) (by omega)",
                  "  exact ⟨" + ", ".join("ZMod.val_injective _ (by omega)" for _ in us) + "⟩"]
        concl = " ∧ ".join(f"w₁ {u} = w₂ {u}" for u in us)
        self.helpers.append("\n".join([self.step_head(st, concl)] + lines) + "\n")
        self.bind(us, self.call(st))

    def st_iszero(self, st: Step) -> None:
        g1, g2 = st.entries
        z = st.wires[0]
        y = st.data["y"]
        lines, pa1, pb1 = self._pair_hyps(g1, st, ("a1", "b1"))
        l2, pa2, pb2 = self._pair_hyps(g2, st, ("a2", "b2"))
        lines += l2
        sh = iszero_shape(self.pkg, st, self.res.const)
        assert sh is not None
        X, D = sh["Y"], sh["D"]
        cz = st.data["cz"]
        d = signed(inv(cz))
        rho = signed(sh["lam"])                    # Y = λ·X2 (mod p): Y·z = λ·(X2·z)
        zs = {"₁": Poly.atom(f"w₁ {z}"), "₂": Poly.atom(f"w₂ {z}")}
        ys = {"₁": Poly.atom(f"w₁ {y}"), "₂": Poly.atom(f"w₂ {y}")}
        c_hcd = lc_cert(Poly.const(cz * d - 1), [])
        h1 = lc_cert(X * ys["₁"] + Poly.const(cz) * zs["₁"] + D, [(1, "a1", pa1)])
        h2 = lc_cert(X * ys["₂"] + Poly.const(cz) * zs["₂"] + D, [(1, "b1", pb1)])
        k1 = lc_cert(X * zs["₁"], [(rho, "a2", pa2)])
        k2 = lc_cert(X * zs["₂"], [(rho, "b2", pb2)])
        lines.append(f"  exact mech_iz (X := {X.render()}) (D := {D.render()}) (y₁ := w₁ {y}) (y₂ := w₂ {y}) "
                     f"(z₁ := w₁ {z}) (z₂ := w₂ {z}) (c := {cz}) (d := {d})")
        lines += [f"    (by linear_combination {c_hcd})", f"    (by linear_combination {h1})",
                  f"    (by linear_combination {h2})", f"    (by linear_combination {k1})",
                  f"    (by linear_combination {k2})"]
        self.helpers.append("\n".join([self.step_head(st, f"w₁ {z} = w₂ {z}")] + lines) + "\n")
        self.bind([z], self.call(st))

    def st_fieldcut(self, st: Step) -> None:
        self.helpers.append(fieldcut_lean(self, st))
        self.bind([st.data["q"], st.data["r"]], self.call(st))

    def st_euclid(self, st: Step) -> None:
        self.helpers.append(euclid_lean(self, st))
        self.bind([st.data["q"], st.data["r"]], self.call(st))

    def st_bb(self, st: Step) -> None:
        g = st.entries[0]
        op = self.pkg.ops[self.pkg.entries[g].op]
        idx = self.pkg.keys.index(op["key"])
        outs = op["outputs"]
        lines = [f"  have a := h₁{proj(self.pkg, g)}", f"  have b := h₂{proj(self.pkg, g)}"]
        pred = op["predicate"]
        if pred is not None and pred[0] == "w":
            pw = pred[1]
            v = self.res.const[pw]
            for hyp, side in (("a", "₁"), ("b", "₂")):
                lines.append(f"  replace {hyp} := {hyp} (mech_ne ({self.kref(pw, side)}) "
                             f"(by linear_combination {lc_cert(Poly.const(v * inv(v) - 1), [])}) (d := {inv(v)}))")

        def lst(side: str) -> str:
            return "[" + ", ".join(f"w{side} {x[1]}" if x[0] == "w" else f"({x[1]} : F)" for x in op["inputs"]) + "]"

        ins = sorted({x[1] for x in op["inputs"] if x[0] == "w"})
        rw = f"by rw [{', '.join(f'e{x}' for x in ins)}]" if ins else "rfl"
        lines.append(f"  have c := a.trans ((congrArg (bb {idx}) (show {lst('₁')} = {lst('₂')} from {rw}))"
                     ".trans b.symm)")
        projs = []
        cur = "c"
        for _ in outs:
            projs.append(f"(List.cons.inj {cur}).1")
            cur = f"(List.cons.inj {cur}).2"
        lines.append("  exact " + (projs[0] if len(projs) == 1 else "⟨" + ", ".join(projs) + "⟩"))
        concl = " ∧ ".join(f"w₁ {o} = w₂ {o}" for o in outs)
        self.helpers.append("\n".join([self.step_head(st, concl)] + lines) + "\n")
        self.bind(list(outs), self.call(st))

    def st_logic(self, st: Step) -> None:
        g = st.entries[0]
        op = self.pkg.ops[self.pkg.entries[g].op]
        o = op["output"]
        rws = ", ".join(["a", "b"] + [f"e{x}" for x in st.eq])
        lines = [f"  have a := h₁{proj(self.pkg, g, 2)}", f"  have b := h₂{proj(self.pkg, g, 2)}",
                 "  apply ZMod.val_injective", f"  rw [{rws}]"]
        self.helpers.append("\n".join([self.step_head(st, f"w₁ {o} = w₂ {o}")] + lines) + "\n")
        self.bind([o], self.call(st))

    # -- memory
    def mem_ops(self, b: int) -> list[dict]:
        out = []
        for k in self.pkg.mem[b]["ops"]:
            op = self.pkg.ops[k]
            out.append({"write": op["write"].as_const() == 1, "i": op["index"].as_witness(),
                        "v": op["value"].as_witness()})
        return out

    def st_memstate(self, st: Step) -> None:
        b, s = st.data["block"], st.data["state"]
        if s == 0:
            self.state_def(b, 0)
            self.helpers.append(f"theorem mech_ms{b}_0 (w₁ w₂ : Fin nWires → F){self.eqb(st.eq)} : "
                                f"mech_mb{b}_0 w₁ = mech_mb{b}_0 w₂ := by\n  unfold mech_mb{b}_0\n"
                                f"  rw [{', '.join(f'e{x}' for x in st.eq)}]\n")
            for x in st.eq:
                self.need_eq(x)
            self.body.append(f"  have m{b}_0 := mech_ms{b}_0 w₁ w₂{self.eqa(st.eq)}")
            return
        self.state_def(b, s)
        self.helpers.append(f"theorem mech_ms{b}_{s} (w₁ w₂ : Fin nWires → F) "
                            f"(sp : mech_mb{b}_{s - 1} w₁ = mech_mb{b}_{s - 1} w₂){self.eqb(st.eq)} : "
                            f"mech_mb{b}_{s} w₁ = mech_mb{b}_{s} w₂ := by\n  unfold mech_mb{b}_{s}\n"
                            f"  rw [{', '.join(['sp'] + [f'e{x}' for x in st.eq])}]\n")
        for x in st.eq:
            self.need_eq(x)
        self.body.append(f"  have m{b}_{s} := mech_ms{b}_{s} w₁ w₂ m{b}_{s - 1}{self.eqa(st.eq)}")

    def state_def(self, b: int, s: int) -> None:
        """``mech_mb{b}_{s}``: the contents of block ``b`` after its ``s``-th write (0: the initial witnesses)."""
        if (b, s) in self.mem_defs:
            return
        if s == 0:
            init = ", ".join(f"w {x}" for x in self.pkg.mem[b]["init"])
            self.helpers.append(f"def mech_mb{b}_0 (w : Fin nWires → F) : List F := [{init}]\n")
        else:
            self.state_def(b, s - 1)
            o = [o for o in self.mem_ops(b) if o["write"]][s - 1]
            self.helpers.append(f"def mech_mb{b}_{s} (w : Fin nWires → F) : List F := "
                                f"(mech_mb{b}_{s - 1} w).set (w {o['i']}).val (w {o['v']})\n")
        self.mem_defs.add((b, s))

    def peel(self, b: int) -> dict:
        """Peel theorem of block ``b`` covering every read used by a MEMREAD step: position of each read."""
        if b in self.mem_peel:
            return self.mem_peel[b]
        reads = sorted(st.data["op"] for st in self.res.steps if st.kind == "MEMREAD" and st.data["block"] == b)
        last = reads[-1]
        ops = self.mem_ops(b)
        g = next(k for k, e in enumerate(self.pkg.entries) if e.kind == "mem" and e.block == b)
        lines = [f"  have M := h{proj(self.pkg, g)}"]
        concl, pos, state, facts = [], {}, 0, []
        for t, o in enumerate(ops[:last + 1]):
            if o["write"]:
                lines.append("  have M := mech_wr M")
                state += 1
            else:
                lines.append(f"  have R{t} := mech_rd M")
                lines.append(f"  have M := R{t}.2")
                if t in reads:
                    pos[t] = len(concl)
                    concl.append(f"(mech_mb{b}_{state} w).getD (w {o['i']}).val 0 = w {o['v']}")
                    facts.append(f"R{t}.1")
        lines.append("  exact ⟨" + ", ".join(facts) + "⟩" if len(facts) > 1 else f"  exact {facts[0]}")
        for k in range(state + 1):                 # every state up to the last read is defined before the peel
            self.state_def(b, k)
        self.helpers.append(f"theorem mech_mr{b} [Fact (Nat.Prime p)] {self.b1()} : {' ∧ '.join(concl)} := by\n" +
                            "\n".join(lines) + "\n")
        self.mem_peel[b] = {"pos": pos, "n": len(concl)}
        return self.mem_peel[b]

    def st_memread(self, st: Step) -> None:
        b, s, t = st.data["block"], st.data["state"], st.data["op"]
        pk = self.peel(b)
        k, n = pk["pos"][t], pk["n"]
        pj = ".2" * k + (".1" if k < n - 1 else "")
        o = self.mem_ops(b)[t]
        v, i = o["v"], o["i"]
        lines = [f"  have R₁ := (mech_mr{b} {self.a1('₁')}){pj}", f"  have R₂ := (mech_mr{b} {self.a1('₂')}){pj}",
                 f"  rw [ms, e{i}] at R₁", "  exact R₁.symm.trans R₂"]
        head = (f"theorem mech_s{self.n} [Fact (Nat.Prime p)] {self.b2()} "
                f"(ms : mech_mb{b}_{s} w₁ = mech_mb{b}_{s} w₂) (e{i} : w₁ {i} = w₂ {i}) : w₁ {v} = w₂ {v} := by")
        self.helpers.append("\n".join([head] + lines) + "\n")
        self.need_eq(i)
        self.bind([v], f"mech_s{self.n} {self.a2()} m{b}_{s} e{i}")


HELPERS = f"""set_option linter.unusedVariables false
set_option maxRecDepth 100000
set_option maxHeartbeats 10000000

theorem mech_hp : ({P} : F) = 0 := ZMod.natCast_self p

theorem mech_cv (x : F) : ((x.val : ℕ) : F) = x := ZMod.natCast_zmod_val x

theorem mech_lift {{m n : ℕ}} (h : (m : F) = (n : F)) (hm : m < p) (hn : n < p) : m = n := by
  have := congrArg ZMod.val h
  rwa [ZMod.val_natCast_of_lt hm, ZMod.val_natCast_of_lt hn] at this

theorem mech_iz [Fact (Nat.Prime p)] {{X D y₁ y₂ z₁ z₂ c d : F}} (hcd : c * d = 1)
    (h₁ : X * y₁ + c * z₁ + D = 0) (h₂ : X * y₂ + c * z₂ + D = 0)
    (k₁ : X * z₁ = 0) (k₂ : X * z₂ = 0) : z₁ = z₂ := by
  rcases eq_or_ne X 0 with hX | hX
  · rw [hX] at h₁ h₂
    linear_combination d * (h₁ - h₂) + (z₂ - z₁) * hcd
  · rw [(mul_eq_zero.1 k₁).resolve_left hX, (mul_eq_zero.1 k₂).resolve_left hX]

theorem mech_vc {{x : F}} {{c : ℕ}} (h : x = (c : F)) (hc : c < p) : x.val = c := by
  rw [h]; exact ZMod.val_natCast_of_lt hc

theorem mech_euclid {{d q₁ q₂ r₁ r₂ : ℕ}} (h : d * q₁ + r₁ = d * q₂ + r₂) (h₁ : r₁ < d) (h₂ : r₂ < d) :
    q₁ = q₂ ∧ r₁ = r₂ := by
  have hd : 0 < d := by omega
  have a := (Nat.div_mod_unique hd).2 ⟨(Nat.add_comm _ _ : r₁ + d * q₁ = d * q₁ + r₁), h₁⟩
  have b := (Nat.div_mod_unique hd).2 ⟨(Nat.add_comm _ _ : r₂ + d * q₂ = d * q₂ + r₂), h₂⟩
  rw [h] at a
  exact ⟨a.1.symm.trans b.1, a.2.symm.trans b.2⟩

theorem mech_ne [Fact (Nat.Prime p)] {{x c d : F}} (hx : x = c) (hcd : c * d = 1) : x ≠ 0 := by
  intro h0
  rw [h0] at hx
  rw [← hx, zero_mul] at hcd
  exact zero_ne_one hcd
"""

MEM_HELPERS = """theorem mech_rd [Fact (Nat.Prime p)] {m : List F} {i v : F} {ops : List (Bool × F × F × F)}
    (h : memRun m ((true, 0, i, v) :: ops) = true) : m.getD i.val 0 = v ∧ memRun m ops = true := by
  have h01 : (0 : F) ≠ 1 := zero_ne_one
  simp only [memRun, if_true, h01, if_false, Bool.and_eq_true, decide_eq_true_eq] at h
  exact ⟨h.1.2, h.2⟩

theorem mech_wr {m : List F} {i v : F} {ops : List (Bool × F × F × F)}
    (h : memRun m ((true, 1, i, v) :: ops) = true) : memRun (m.set i.val v) ops = true := by
  simp only [memRun, if_true, Bool.and_eq_true, decide_eq_true_eq] at h
  exact h.2
"""


def _lift_sides(e: A.Expr, sub: "Subst", val: set) -> tuple[str, str, Poly]:
    """ℕ sides of an AssertZero whose wires in ``val`` are lifted through ``.val`` (positive terms and a positive
    constant on the left, the negated negative ones on the right) and the field polynomial ``left - right``."""
    lt, rt, poly = [], [], Poly()
    terms = [(signed(q), w) for q, w in e.lin] + ([(signed(e.const), None)] if e.const % P else [])
    for c, w in terms:
        if w is not None and w not in val:
            raise ValueError("lifted AssertZero with a non-lifted wire")
        mag = abs(c)
        nat = str(mag) if w is None else ((f"{mag} * " if mag != 1 else "") + f"({sub.name(w)}).val")
        (lt if c > 0 else rt).append(nat)
        poly = poly + Poly.const(c) * (Poly.const(1) if w is None else sub.term(w))
    return " + ".join(lt) or "0", " + ".join(rt) or "0", poly


def fieldcut_lean(em: "Emitter", st: Step) -> str:
    pkg, d = em.pkg, st.data
    q, r, t, z, s = d["q"], d["r"], d["t"], d["z"], d["s"]
    M, Q = d["M"], d["Q"]
    g0, gb, g1, g3 = st.entries
    rng = range_bounds(pkg)
    one = Subst(0, [], {})
    eb = pkg.ops[pkg.entries[gb].op]["expr"]
    lb, rb_, pb = _lift_sides(eb, one, {q, t})
    cert_b = lc_cert(pb, [(1, "eb", expr_poly(eb, one))])
    e1 = pkg.ops[pkg.entries[g1].op]["expr"]
    a1 = expr_poly(e1, Subst(0, [], {q: Q}))
    cert_z = lc_cert(Poly.atom(f"w {z}") - Poly.const(d["z0"]), [(signed(inv(d["cz"])), "a1", a1)])
    e3 = pkg.ops[pkg.entries[g3].op]["expr"]
    a3 = expr_poly(e3, Subst(0, [], {z: d["z0"]}))
    if s is None:
        cert_r = lc_cert(Poly.atom(f"w {r}") - Poly.const(d["R0"]), [(signed(inv(d["ar"] * d["z0"])), "a3", a3)])
    else:
        cert_r = lc_cert(Poly.atom(f"w {r}") + Poly.const(d["B"]) - Poly.atom(f"w {s}"), [(-d["cs"], "a3", a3)])
    n = em.n
    lines = [f"theorem mech_fc{n} [Fact (Nat.Prime p)] {em.b1()} : {M} * (w {q}).val + (w {r}).val < p := by",
             f"  have hp' : p = {P} := rfl"]
    for nm, x in (("hq", q), ("hr", r), ("ht", t), ("hs", s)):
        if x is not None:
            lines.append(f"  have {nm} := h{proj(pkg, rng[x][1])}")
    lines += [f"  have eb := h{proj(pkg, gb)}",
              f"  have L1 : (({lb} : ℕ) : F) = (({rb_} : ℕ) : F) := by",
              "    push_cast [mech_cv]", f"    linear_combination {cert_b}",
              "  have N1 := mech_lift L1 (by omega) (by omega)",
              f"  rcases Nat.lt_or_ge (w {q}).val {Q} with hlt | hge",
              "  · omega",
              f"  · have hqv : (w {q}).val = {Q} := by omega",
              f"    have hqQ : w {q} = (({Q} : ℕ) : F) := by rw [← hqv]; exact (mech_cv (w {q})).symm",
              f"    have a1 := h{proj(pkg, g1)}",
              "    rw [hqQ] at a1",
              f"    have hz : w {z} = (({d['z0']} : ℕ) : F) := by linear_combination {cert_z}",
              f"    have a3 := h{proj(pkg, g3)}",
              "    rw [hz] at a3"]
    if s is None:
        lines += [f"    have hr0 : w {r} = (({d['R0']} : ℕ) : F) := by linear_combination {cert_r}",
                  "    have N3 := mech_vc hr0 (by omega)", "    omega"]
    else:
        lines += [f"    have L3 : (((w {r}).val + {d['B']} : ℕ) : F) = (((w {s}).val : ℕ) : F) := by",
                  "      push_cast [mech_cv]", f"      linear_combination {cert_r}",
                  "    have N3 := mech_lift L3 (by omega) (by omega)", "    omega"]
    # pair step: the cut of the determined X is unique
    co = d["coef"]
    lines2, pa, pb2 = em._pair_hyps(g0, st)
    goal = Poly.const(M) * (Poly.atom(f"w₁ {q}") - Poly.atom(f"w₂ {q}")) + (Poly.atom(f"w₁ {r}") - Poly.atom(f"w₂ {r}"))
    sgn = 1 if co[r] > 0 else -1
    cert = lc_cert(goal, [(sgn, "a", pa), (-sgn, "b", pb2)])
    lines2 += [f"  have r0a := h₁{proj(pkg, rng[r][1])}", f"  have r0b := h₂{proj(pkg, rng[r][1])}",
               f"  have hF : (({M} * (w₁ {q}).val + (w₁ {r}).val : ℕ) : F) = "
               f"(({M} * (w₂ {q}).val + (w₂ {r}).val : ℕ) : F) := by",
               "    push_cast [mech_cv]", f"    linear_combination {cert}",
               f"  have hN := mech_lift hF (mech_fc{n} {em.a1('₁')}) (mech_fc{n} {em.a1('₂')})",
               "  exact ⟨ZMod.val_injective _ (by omega), ZMod.val_injective _ (by omega)⟩"]
    return "\n".join(lines) + "\n\n" + "\n".join([em.step_head(st, f"w₁ {q} = w₂ {q} ∧ w₁ {r} = w₂ {r}")] + lines2) + "\n"


def euclid_lean(em: "Emitter", st: Step) -> str:
    pkg, d_ = em.pkg, st.data
    q, r, d, t = d_["q"], d_["r"], d_["d"], d_["t"]
    g0, g1 = st.entries
    rng = range_bounds(pkg)
    one = Subst(0, [], {})
    e1 = pkg.ops[pkg.entries[g1].op]["expr"]
    l1, r1, p1 = _lift_sides(e1, one, {d, r, t})
    cert1 = lc_cert(p1, [(1, "el", expr_poly(e1, one))])
    n = em.n
    lines = [f"theorem mech_eu{n} [Fact (Nat.Prime p)] {em.b1()} : (w {r}).val < (w {d}).val ∧ "
             f"(w {d}).val * (w {q}).val + (w {r}).val < p := by",
             f"  have hp' : p = {P} := rfl",
             f"  have hq := h{proj(pkg, rng[q][1])}", f"  have hr := h{proj(pkg, rng[r][1])}",
             f"  have ht := h{proj(pkg, rng[t][1])}", f"  have hd := ZMod.val_lt (w {d})",
             f"  have el := h{proj(pkg, g1)}",
             f"  have L1 : (({l1} : ℕ) : F) = (({r1} : ℕ) : F) := by",
             "    push_cast [mech_cv]", f"    linear_combination {cert1}",
             "  have N1 := mech_lift L1 (by omega) (by omega)",
             f"  have hm : (w {d}).val * (w {q}).val ≤ {d_['dmax']} * {d_['qmax']} := "
             "Nat.mul_le_mul (by omega) (by omega)",
             "  exact ⟨by omega, by omega⟩"]
    lines2, pa, pb = em._pair_hyps(g0, st)
    goal = (Poly.atom(f"w₂ {d}") * Poly.atom(f"w₁ {q}") + Poly.atom(f"w₁ {r}")
            - Poly.atom(f"w₂ {d}") * Poly.atom(f"w₂ {q}") - Poly.atom(f"w₂ {r}"))
    sg = d_["sign"]
    cert = lc_cert(goal, [(sg, "a", pa), (-sg, "b", pb)])
    lines2 += [f"  have c₁ := mech_eu{n} {em.a1('₁')}", f"  have c₂ := mech_eu{n} {em.a1('₂')}",
               f"  rw [e{d}] at c₁",
               f"  have hF : (((w₂ {d}).val * (w₁ {q}).val + (w₁ {r}).val : ℕ) : F) = "
               f"(((w₂ {d}).val * (w₂ {q}).val + (w₂ {r}).val : ℕ) : F) := by",
               "    push_cast [mech_cv]", f"    linear_combination {cert}",
               "  have hN := mech_lift hF c₁.2 c₂.2",
               "  have hU := mech_euclid hN c₁.1 c₂.1",
               "  exact ⟨ZMod.val_injective _ hU.1, ZMod.val_injective _ hU.2⟩"]
    return "\n".join(lines) + "\n\n" + "\n".join([em.step_head(st, f"w₁ {q} = w₂ {q} ∧ w₁ {r} = w₂ {r}")] + lines2) + "\n"


def emit_solution(pkg: Pkg, res: Result) -> str:
    assert res.status == "DETERMINED"
    helpers, proof = Emitter(pkg, res).emit()
    s = pkg.statement
    if s.count("  sorry\n") != 1 or s.count("/--") != 1:
        raise ValueError("unexpected statement layout")
    return s.replace("  sorry\n", proof + "\n", 1).replace("/--", helpers + "\n/--", 1)


# ------------------------------------------------------------------------------------------ population

def _sha256(path: str) -> str:
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def _jl(path: str) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        return [json.loads(x) for x in f if x.strip()]


def noir_effective(root: str) -> list[dict]:
    """Effective Noir records (latest per item: recovery R1 records supersede the wave-N1 record they name)."""
    eff: dict[str, tuple[str, dict]] = {}
    for coll in NOIR_COLLECTIONS:
        idx = os.path.join(root, coll, "INDEX.jsonl")
        if os.path.exists(idx):
            for r in _jl(idx):
                eff[r["package_id"]] = (coll, r)
    idx = os.path.join(root, NOIR_RECOVERY, "INDEX.jsonl")
    if os.path.exists(idx):
        for r in _jl(idx):
            eff.pop(r["evidence"]["supersedes_record"]["package_id"], None)
            eff["R:" + r["package_id"]] = (NOIR_RECOVERY, r)
    out = []
    for coll, r in eff.values():
        pid = r["package_id"]
        cands = [os.path.join(root, coll, pid), os.path.join(root, pid),
                 os.path.join(root, coll, pid.split("/", 1)[-1])]
        d = next((c for c in cands if os.path.isfile(os.path.join(c, "problem.json"))), None)
        out.append({"package_id": pid, "collection": coll, "status": r["status"], "dir": d,
                    "rule": (r.get("instantiation") or {}).get("rule")})
    return out


# ------------------------------------------------------------------------------------------ driver

def engine_record(pkg_dir: str) -> tuple[dict, Pkg | None, Result | None]:
    t0 = time.time()
    rec: dict = {}
    try:
        pkg = load(pkg_dir)
        res = solve(pkg)
    except (ModelMismatch, A.Unsupported, OverflowError, KeyError, ValueError, AssertionError) as ex:
        rec.update(engine="ERROR", reason=f"{type(ex).__name__}: {ex}"[:400], engine_secs=round(time.time() - t0, 3))
        return rec, None, None
    rec.update(engine=res.status, engine_secs=round(time.time() - t0, 3), n_opcodes=len(pkg.ops),
               n_conjuncts=len(pkg.entries), n_wires=pkg.flat.n_witnesses, n_inputs=len(pkg.flat.inputs),
               n_outputs=len(pkg.flat.outputs), brillig=sum(1 for o in pkg.ops if o["kind"] == "brillig"),
               memory=bool(pkg.mem), black_boxes=bool(pkg.keys), rules=res.rules)
    if res.status == "STUCK":
        rec["stuck"] = res.stuck
    return rec, pkg, res


_RSS = threading.local()


def _install_rss_probe() -> None:
    from zk_registry import lean_runner as L
    if getattr(L, "_mech_probe", False):
        return
    orig = L.run_process

    def probe(*a, **k):
        r = orig(*a, **k)
        peaks = getattr(_RSS, "peaks", None)
        if peaks is not None:
            peaks.append(r.peak_rss_mb)
        return r
    L.run_process = probe
    L._mech_probe = True


def check_proof(pkg_dir: str, proof: str, env, timeout: float, work_root: str) -> dict:
    from zk_registry import check as C
    _RSS.peaks = []
    rep = C.check(pkg_dir, proof, env, None, timeout, False, work_root)
    peaks, _RSS.peaks = _RSS.peaks, None
    comp = rep.get("compile") if isinstance(rep.get("compile"), dict) else {}
    return {"verdict": rep["verdict"], "check_errors": (rep["error"] + rep["invalid"] + rep["fail"])[:3],
            "compile_secs": comp.get("compile_secs"), "peak_rss_mb": max(peaks) if peaks else None}


def proof_fail_class(chk: dict) -> str:
    txt = " ".join(chk.get("check_errors") or [])
    for key, cls in (("timed out", "timeout"), ("heartbeats", "heartbeats"), ("linear_combination", "linear_combination"),
                     ("ring failed", "ring"), ("omega", "omega"), ("rewrite", "rewrite"), ("motive", "rewrite"),
                     ("did not find", "rewrite"), ("type mismatch", "type-mismatch"), ("maximum recursion", "recursion"),
                     ("memory", "memory")):
        if key in txt:
            return cls
    return "other"


def run(args) -> int:
    from zk_registry import lean_runner as L
    os.makedirs(args.out, exist_ok=True)
    pop = [r for r in noir_effective(args.packages) if r["status"] in ("OPEN", "DET-FALSE-CANDIDATE")]
    if args.only:
        pop = [r for r in pop if r["package_id"] in set(args.only)]
    pop.sort(key=lambda r: (r["status"] != "DET-FALSE-CANDIDATE", r["package_id"]))
    env = L.load_env(args.lean_env) if not args.engine_only else None
    if env is not None:
        _install_rss_probe()
        L.set_lean_limits(slots=args.jobs, rss_mb=args.rss_mb)
    res_path = os.path.join(args.out, "RESULTS.jsonl")
    done = set()
    if os.path.exists(res_path) and args.resume:
        done = {r["package_id"] for r in _jl(res_path)}
    lock = threading.Lock()
    unsound: list[str] = []

    def one(r: dict) -> dict:
        rec = {"package_id": r["package_id"], "collection": r["collection"], "status": r["status"],
               "instantiation_rule": r["rule"]}
        if r["dir"] is None:
            rec.update(outcome="MECH-ERROR", reason="package directory not found")
            return rec
        erec, pkg, res = engine_record(r["dir"])
        rec.update(erec)
        if r["status"] == "DET-FALSE-CANDIDATE":
            rec["outcome"] = "UNSOUND-DETERMINED" if erec.get("engine") == "DETERMINED" else "ENGINE-" + erec["engine"]
            return rec
        if erec["engine"] == "ERROR":
            rec["outcome"] = "MECH-ERROR"
            return rec
        if erec["engine"] == "STUCK":
            rec["outcome"] = "MECH-STUCK"
            rec["reason"] = "brillig-dependent" if res.stuck["brillig_dependent"] else "other"
            return rec
        safe = re.sub(r"[^A-Za-z0-9_.-]", "_", r["package_id"])[:180]
        pdir = os.path.join(args.out, "proofs", safe)
        os.makedirs(pdir, exist_ok=True)
        try:
            text = emit_solution(pkg, res)
        except (AssertionError, ValueError, KeyError, NotImplementedError) as ex:
            rec.update(outcome="MECH-ERROR", reason=f"emit: {type(ex).__name__}: {ex}"[:400])
            return rec
        sol = os.path.join(pdir, "Solution.lean")
        with open(sol, "w", encoding="utf-8") as f:
            f.write(text)
        rec["proof_bytes"] = len(text.encode())
        if env is None:
            rec["outcome"] = "MECH-EMITTED"
            return rec
        t0 = time.time()
        try:
            chk = check_proof(r["dir"], sol, env, args.timeout, os.path.join(args.work_root or args.out, "check"))
        except Exception as ex:          # noqa: BLE001 - recorded as an outcome
            rec.update(outcome="MECH-ERROR", reason=f"checker: {type(ex).__name__}: {ex}"[:400])
            return rec
        rec.update(check=chk, check_secs=round(time.time() - t0, 2))
        if chk["verdict"] == "PASS":
            rec["outcome"] = "MECH-SOLVED"
        elif chk["verdict"] == "ERROR":
            rec.update(outcome="MECH-ERROR", reason="checker error")
        else:
            rec.update(outcome="MECH-PROOF-FAIL", reason=proof_fail_class(chk))
        return rec

    todo = [r for r in pop if r["package_id"] not in done]
    here = os.path.dirname(os.path.abspath(__file__))
    meta = {"engine": ENGINE_VERSION, "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "sources_sha256": {f: _sha256(os.path.join(here, f)) for f in ("mech_acir.py", "check.py", "noir_acir.py",
                                                                             "noir_lean_emit.py")},
            "population": len(pop), "todo": len(todo), "jobs": args.jobs, "rss_mb": args.rss_mb,
            "timeout": args.timeout, "lean_env": args.lean_env}
    # DET-FALSE-CANDIDATE packages first, engine only: a DETERMINED one stops the run (unsound engine)
    with open(res_path, "a", encoding="utf-8") as out:
        for r in [x for x in todo if x["status"] == "DET-FALSE-CANDIDATE"]:
            rec = one(r)
            out.write(json.dumps(rec, sort_keys=True) + "\n")
            out.flush()
            if rec["outcome"] == "UNSOUND-DETERMINED":
                unsound.append(r["package_id"])
        if unsound:
            print(f"UNSOUND: {len(unsound)} DET-FALSE-CANDIDATE package(s) DETERMINED; stopping", file=sys.stderr)
            return 4
        rest = [x for x in todo if x["status"] != "DET-FALSE-CANDIDATE"]
        t0 = time.time()
        with cf.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as ex:
            for rec in ex.map(one, rest):
                with lock:
                    out.write(json.dumps(rec, sort_keys=True) + "\n")
                    out.flush()
                print(f"{rec['outcome']:<18} {rec.get('check_secs', '')!s:>8} {rec['package_id']}", flush=True)
    meta.update(finished_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), wall_secs=round(time.time() - t0, 1))
    with open(os.path.join(args.out, "RUN.json"), "w", encoding="utf-8") as f:
        json.dump(meta, f, indent=1, sort_keys=True)
    return 0


SIZE_BANDS = ((50, "≤ 50"), (200, "51–200"), (1000, "201–1,000"), (2000, "1,001–2,000"), (10 ** 9, "> 2,000"))


def stuck_class(rec: dict) -> str:
    """Primary reason of a STUCK record (heuristic over the blocking cone, for the report)."""
    st = rec.get("stuck") or {}
    g, b = st.get("gadgets") or [], st.get("blocking") or {}
    if "integer-cut" in g:
        return "integer cut, unmatched variant"
    if "euclidean-division" in g:
        return "euclidean division, unmatched variant"
    if "mem-undetermined" in b:
        return "memory with an undetermined index or contents"
    if "az-nonlinear" in b:
        return "hint used in a product with another undetermined wire"
    if "az-multi-unknown-ranged" in b:
        return "range-checked unknowns without a unique decomposition"
    if "az-multi-unknown" in b:
        return "several unknowns in one linear constraint"
    if "bb-undetermined-input" in b:
        return "black box with an undetermined input"
    return "other"


def _pct(xs: list[float], q: float) -> float:
    xs = sorted(xs)
    return xs[min(len(xs) - 1, max(0, int(round(q * (len(xs) - 1)))))] if xs else 0.0


def summarize(records: list[dict]) -> dict:
    """Counts for the P3 report from RESULTS.jsonl records."""
    from collections import Counter
    opn = [r for r in records if r["status"] == "OPEN"]
    dfc = [r for r in records if r["status"] == "DET-FALSE-CANDIDATE"]
    out: dict = {"open": len(opn), "outcomes": dict(Counter(r["outcome"] for r in opn)),
                 "dfc": len(dfc), "dfc_determined": sum(1 for r in dfc if r["outcome"] == "UNSOUND-DETERMINED")}
    out["by_collection"] = {c: dict(Counter(r["outcome"] for r in opn if r["collection"] == c))
                            for c in sorted({r["collection"] for r in opn})}
    stuck = [r for r in opn if r["outcome"] == "MECH-STUCK"]
    out["stuck_brillig"] = dict(Counter(r.get("reason") for r in stuck))
    out["stuck_classes"] = dict(Counter(stuck_class(r) for r in stuck).most_common())
    out["proof_fail"] = dict(Counter(r.get("reason") for r in opn if r["outcome"] == "MECH-PROOF-FAIL"))
    out["errors"] = dict(Counter((r.get("reason") or "")[:80] for r in opn if r["outcome"] == "MECH-ERROR"))
    rules: Counter = Counter()
    for r in opn:
        if r.get("engine") == "DETERMINED":
            rules.update(k for k in (r.get("rules") or {}) if k != "rounds")
    out["rules_packages"] = dict(rules.most_common())
    bands = {}
    for lim, name in SIZE_BANDS:
        lo = max([x for x, _ in SIZE_BANDS if x < lim], default=0)
        sel = [r for r in opn if lo < (r.get("n_opcodes") or 0) <= lim]
        bands[name] = dict(Counter(r["outcome"] for r in sel))
    out["size_bands"] = bands
    secs = [r["check_secs"] for r in opn if r.get("check_secs") is not None]
    rss = [r["check"]["peak_rss_mb"] for r in opn if (r.get("check") or {}).get("peak_rss_mb")]
    out["timing"] = {"engine_secs_total": round(sum(r.get("engine_secs") or 0 for r in records), 2),
                     "engine_secs_max": max((r.get("engine_secs") or 0 for r in records), default=0),
                     "check_secs_total": round(sum(secs), 1), "check_secs_p50": _pct(secs, 0.5),
                     "check_secs_p95": _pct(secs, 0.95), "check_secs_max": max(secs, default=0),
                     "rss_mb_p50": _pct(rss, 0.5), "rss_mb_p95": _pct(rss, 0.95), "rss_mb_max": max(rss, default=0)}
    pb = [r["proof_bytes"] for r in opn if r.get("proof_bytes")]
    out["proof_bytes"] = {"p50": _pct(pb, 0.5), "max": max(pb, default=0)}
    return out


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("solve", help="run the engine on one package (and emit Solution.lean when DETERMINED)")
    s.add_argument("package")
    s.add_argument("--out")
    r = sub.add_parser("run", help="engine + production checker over the effective OPEN Noir packages")
    r.add_argument("--packages", required=True, help="packages root (noir-n1, recovery-r1/noir, ...)")
    r.add_argument("--out", required=True)
    r.add_argument("--lean-env")
    r.add_argument("--work-root")
    r.add_argument("--jobs", type=int, default=1)
    r.add_argument("--rss-mb", type=int, default=None)
    r.add_argument("--timeout", type=int, default=3600)
    r.add_argument("--only", nargs="*")
    r.add_argument("--engine-only", action="store_true")
    r.add_argument("--resume", action="store_true")
    sm = sub.add_parser("summary", help="counts of a RESULTS.jsonl (JSON)")
    sm.add_argument("results")
    a = ap.parse_args(argv)
    if a.cmd == "summary":
        print(json.dumps(summarize(_jl(a.results)), indent=1, ensure_ascii=False))
        return 0
    if a.cmd == "solve":
        rec, pkg, res = engine_record(a.package)
        print(json.dumps(rec, sort_keys=True, indent=1))
        if res is not None and res.status == "DETERMINED" and a.out:
            os.makedirs(a.out, exist_ok=True)
            with open(os.path.join(a.out, "Solution.lean"), "w", encoding="utf-8") as f:
                f.write(emit_solution(pkg, res))
        return 0
    if not a.engine_only and not a.lean_env:
        ap.error("--lean-env is required unless --engine-only")
    return run(a)


if __name__ == "__main__":
    sys.exit(main())
