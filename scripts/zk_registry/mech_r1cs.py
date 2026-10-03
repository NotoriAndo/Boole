#!/usr/bin/env python3
"""Battery P3: a deterministic mechanical solver for R1CS-family DET / DET-MOD packages.

The R1CS-family packages (Circom DET and DET-MOD, gnark, ZoKrates) keep no constraint system other than their
model module, which the generators emit verbatim from the compiled R1CS (``lean_emit.render_constraint``); this
module parses it back (every constraint is re-rendered and must equal the text it came from).

**Propagation** (:func:`propagate`).  Known wires start as the inputs and the constant wire 0 (``w 0 = 1``).  Rules,
applied until every output is known (DETERMINED) or no rule applies (STUCK):

* ``lin``: a constraint ``A * B = C`` with exactly one unknown wire ``u`` that occurs linearly with a non-zero
  constant coefficient once the known wires are substituted (``u`` only in ``C`` with both factors known; or the
  constraint is linear: an empty factor, or a factor that is a multiple of ``w 0``);
* ``bits``: a linear constraint whose unknown wires all carry a booleanity constraint (``κ (x² - x)`` with
  ``w 0 = 1``) and whose coefficients are ``s * 2^e`` with distinct ``e`` and ``2^n < p`` (``n`` = largest ``e`` + 1):
  the bit vector is unique (bit-decomposition uniqueness), so every unknown bit is determined;
* ``call`` (DET-MOD): once every input of a sub-component instance is known, its outputs are (shared ``f``,
  ``CallsRespect``);
* ``isz`` (gadget rule, reported separately from the core rules ``CORE_RULES``): the IsZero pair
  ``X * (β v) = γ o + K`` and ``(r X) * (δ o) = 0`` with ``X``, ``K`` known determines ``o`` (``-γK`` if ``X = 0``,
  else 0) through the ``mech_isz`` helper; the hint ``v`` stays undetermined;
* ``elim`` (reported separately as well): the reduced row echelon form of the difference system ``d = w₁ - w₂`` over
  the constraints that are affine in the unknowns (``b² = b`` for a boolean unknown alone in both factors); a row
  ``e_u`` determines ``u``, a row over boolean unknowns with coefficients ``s·2^e`` determines its bits.  The proof is
  the recorded combination of the original constraints (exact up to multiples of ``(p : F) = 0``).

Every rule is sound for any two satisfying assignments, and the emitted proof is checked by the production checker
anyway: only a PASS counts.  A STUCK result names the undetermined outputs and classifies the frontier.

**Proof** (:func:`emit_solution`): the statement unchanged, helpers directly above the doc comment, and a proof that
introduces the hypotheses, projects only the constraints the backward slice from the outputs needs (``h₁.2.2.1``,
block by block), proves one ``e<k> : w₁ k = w₂ k`` per step with ``linear_combination`` (exact integer identities,
checked here before emission; non-unit coefficients through ``(p : F) = 0``; ``w 0 = 1`` folded in by a polynomial
multiple of ``h₁.1``), bit vectors through the reusable ``mech_lsum_inj`` helper, and closes the output list with
``List.forall_mem_cons``.  No ``by`` block is nested inside a term (elaborating those is slow at this size) and no
``obtain``/``rw`` runs over the growing context.

Usage::

    python3 -m zk_registry.mech_r1cs solve PACKAGE_DIR [--out Solution.lean] [--json RESULT.json]
    python3 -m zk_registry.mech_r1cs batch ITEMS.jsonl --out-dir DIR --lean-env ENV.json [--jobs N] [--no-check]
"""
from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import os
import re
import statistics
import sys
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field

if __package__ in (None, ""):
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    __package__ = "zk_registry"

from zk_registry import lean_emit as E   # noqa: E402

VERSION = "1.0"
SOURCES = ["mech_r1cs.py"]
RULES = ("lin", "bits", "call", "isz", "elim")
CORE_RULES = ("lin", "bits", "call")


def generator_files() -> list[str]:
    """Files of this battery (not a package generator; it reads packages, it writes none)."""
    return list(SOURCES)


class ModelFormatError(ValueError):
    pass


# ------------------------------------------------------------------------------------------------ model IR

@dataclass
class Con:
    idx: int
    a: dict          # wire -> balanced integer coefficient (as printed)
    b: dict
    c: dict
    text: str        # constraint text with variable `w`
    lhs_zero: bool   # printed `0 = C` (an empty factor)


@dataclass
class Model:
    ns: str
    theorem: str
    p: int
    n_wires: int
    inputs: list
    outputs: list
    cons: list
    blocks: list                       # per block: list of constraint indexes; [] when unblocked
    calls: list | None = None          # DET-MOD: (kind, inputs, outputs)
    emulated: list | None = None       # gnark: (limbs, bits, modulus)
    preconditions: bool = False

    def wires(self, j: int) -> set:
        c = self.cons[j]
        return set(c.a) | set(c.b) | set(c.c)


_FIRST = re.compile(r"^(?:-\((\d+) \* w (\d+)\)|(-?)(?:(\d+) \* )?w (\d+))$")
_NEXT = re.compile(r"^(?:(\d+) \* )?w (\d+)$")


def parse_lc(text: str, p: int) -> list:
    """Inverse of ``lean_emit.render_lc``: [(wire, balanced coefficient)] in printed order."""
    if text == "0":
        return []
    parts = re.split(r" ([+-]) ", text)
    m = _FIRST.match(parts[0])
    if not m:
        raise ModelFormatError(f"bad linear term {parts[0]!r}")
    if m.group(2):
        out = [(int(m.group(2)), -int(m.group(1)))]
    else:
        out = [(int(m.group(5)), int(m.group(4) or 1) * (-1 if m.group(3) else 1))]
    for op, term in zip(parts[1::2], parts[2::2]):
        m = _NEXT.match(term)
        if not m:
            raise ModelFormatError(f"bad linear term {term!r}")
        out.append((int(m.group(2)), int(m.group(1) or 1) * (-1 if op == "-" else 1)))
    return out


def _split_top(text: str, sep: str) -> list:
    depth, parts, start, i = 0, [], 0, 0
    while i < len(text):
        ch = text[i]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == 0 and text.startswith(sep, i):
            parts.append(text[start:i])
            start = i + len(sep)
            i = start
            continue
        i += 1
    parts.append(text[start:])
    return parts


def _factor(text: str, p: int) -> list:
    if text.startswith("(") and text.endswith(")"):
        return parse_lc(text[1:-1], p)
    return parse_lc(text, p)


def _todict(lc: list) -> dict:
    d: dict = {}
    for w, k in lc:
        d[w] = d.get(w, 0) + k
    return {w: k for w, k in d.items() if k}


def parse_constraint(idx: int, text: str, p: int) -> Con:
    sides = _split_top(text, " = ")
    if len(sides) != 2:
        raise ModelFormatError(f"constraint {idx}: no single top-level `=`: {text[:120]!r}")
    lhs, rhs = sides
    c = parse_lc(rhs, p)
    if lhs == "0":
        a, b = [], []
    else:
        fs = _split_top(lhs, " * ")
        if len(fs) != 2:
            raise ModelFormatError(f"constraint {idx}: left side is not a product: {lhs[:120]!r}")
        a, b = _factor(fs[0], p), _factor(fs[1], p)
    mod = lambda lc: [(w, k % p) for w, k in lc]                                   # noqa: E731
    if E.render_constraint(mod(a), mod(b), mod(c), p) != text:
        raise ModelFormatError(f"constraint {idx} does not re-render to its text: {text[:120]!r}")
    return Con(idx, _todict(a), _todict(b), _todict(c), text, lhs == "0")


def _int_list(s: str) -> list:
    s = s.strip()
    return [int(x) for x in s.split(",")] if s else []


def parse_model(model_text: str, statement_text: str) -> Model:
    def one(pat: str, what: str, flags: int = re.M) -> re.Match:
        m = re.search(pat, model_text, flags)
        if not m:
            raise ModelFormatError(f"model has no {what}")
        return m

    ns = one(r"^namespace (\S+)$", "namespace").group(1)
    p = int(one(r"^abbrev p : ℕ := (\d+)$", "p").group(1))
    n_wires = int(one(r"^abbrev nWires : ℕ := (\d+)$", "nWires").group(1))
    outputs = _int_list(one(r"^def Outputs : List \(Fin nWires\) := \[([^\]]*)\]$", "Outputs").group(1))
    inputs = _int_list(one(r"^def Inputs : List \(Fin nWires\) := \[([^\]]*)\]$", "Inputs").group(1))
    m = re.search(r"(?m)^theorem (\w+) ", statement_text)
    if not m:
        raise ModelFormatError("statement has no theorem")
    theorem = m.group(1)
    cm = one(r"^def Constraints \(w : Fin nWires → F\) : Prop :=\n(.*?)\n\n", "Constraints", re.M | re.S)
    body = cm.group(1)
    texts, blocks = [], []
    if body.startswith("  w 0 = 1 ∧ Block0 w"):
        names = body.strip().split(" ∧ ")[1:]
        if names != [f"Block{k} w" for k in range(len(names))]:
            raise ModelFormatError("unexpected Constraints block list")
        for k in range(len(names)):
            bm = one(r"^def Block%d \(w : Fin nWires → F\) : Prop :=\n(.*?)\n\n" % k, f"Block{k}", re.M | re.S)
            chunk = [ln[2:] for ln in bm.group(1).split(" ∧\n")]
            blocks.append(list(range(len(texts), len(texts) + len(chunk))))
            texts += chunk
    else:
        conj = [ln[2:] for ln in body.split(" ∧\n")]
        if conj[0] != "w 0 = 1":
            raise ModelFormatError("Constraints does not start with `w 0 = 1`")
        texts = conj[1:]
    cons = [parse_constraint(j, t, p) for j, t in enumerate(texts)]
    calls = emulated = None
    m = re.search(r"(?m)^def Calls : List \(ℕ × List \(Fin nWires\) × List \(Fin nWires\)\) := \[(.*)\]$", model_text)
    if m:
        calls = [(int(k), _int_list(i), _int_list(o))
                 for k, i, o in re.findall(r"\((\d+), \[([^\]]*)\], \[([^\]]*)\]\)", m.group(1))]
        if theorem != "det_mod" or "def CallsRespect (f : ℕ → List F → List F)" not in model_text:
            raise ModelFormatError("Calls without the DET-MOD statement")
    m = re.search(r"(?m)^def EmulatedOutputs : List \(List \(Fin nWires\) × ℕ × ℕ\) := \[(.*)\]$", model_text)
    if m:
        emulated = [(_int_list(l), int(b), int(q)) for l, b, q in re.findall(r"\(\[([^\]]*)\], (\d+), (\d+)\)", m.group(1))]
    pre = bool(re.search(r"(?m)^def Preconditions ", model_text))
    return Model(ns, theorem, p, n_wires, inputs, outputs, cons, blocks, calls, emulated, pre)


def load_package(pkg_dir: str) -> tuple[Model, dict, str]:
    with open(os.path.join(pkg_dir, "problem.json"), encoding="utf-8") as f:
        problem = json.load(f)
    chk = problem["checker"]
    imports = [fr for fr in chk["files"] if fr["role"] == "import"]
    if len(imports) != 1:
        raise ModelFormatError(f"expected one imported model file, found {len(imports)}")
    with open(os.path.join(pkg_dir, imports[0]["path"]), encoding="utf-8") as f:
        model_text = f.read()
    with open(os.path.join(pkg_dir, chk["statement_file"]), encoding="utf-8") as f:
        statement = f.read()
    model = parse_model(model_text, statement)
    if model.theorem != chk["theorem"]:
        raise ModelFormatError("statement theorem differs from problem.json")
    return model, problem, statement


# ------------------------------------------------------------------------------------------------ propagation

@dataclass
class Step:
    rule: str            # lin | bits | call
    con: int | None      # constraint index (lin, bits) or call index (call)
    wires: list          # wires determined by this step
    deps: list           # known wires the step uses
    info: dict = field(default_factory=dict)


@dataclass
class Result:
    status: str          # DETERMINED | STUCK
    steps: list
    known: set
    undetermined_outputs: list
    stuck: dict = field(default_factory=dict)


def _is_const(f: dict) -> bool:
    return bool(f) and set(f) <= {0}


def linear_form(con: Con, p: int):
    """(mode, ℓ) when the constraint is linear in the wires: ``lin0`` (an empty factor: ``-C``, wire 0 a variable)
    or ``rw`` (one factor a multiple of ``w 0``; after ``w 0 = 1``: ``A*B - C`` with key ``None`` the constant).
    None otherwise.  Coefficients are exact integers of the printed form."""
    if con.lhs_zero or not con.a or not con.b:
        return "lin0", {w: -k for w, k in con.c.items()}
    if _is_const(con.a) or _is_const(con.b):
        k0, other = (con.a[0], con.b) if _is_const(con.a) else (con.b[0], con.a)
        form: dict = {}
        for w, k in other.items():
            key = None if w == 0 else w
            form[key] = form.get(key, 0) + k0 * k
        for w, k in con.c.items():
            key = None if w == 0 else w
            form[key] = form.get(key, 0) - k
        return "rw", {w: k for w, k in form.items() if k}
    return None


def lin_rule(con: Con, u: int, p: int):
    """(mode, κ, ℓ) when the single unknown ``u`` is determined by ``con``; else (None, reason, None)."""
    lf = linear_form(con, p)
    if lf is not None:
        mode, form = lf
        k = form.get(u, 0)
        return (mode, k, form) if k % p else (None, "zero-coef", None)
    if u in con.a and u in con.b:
        return None, "quadratic", None
    if u in con.a or u in con.b:
        return None, "nonconst-coef", None
    k = con.c.get(u, 0)
    return ("prod", k, None) if k % p else (None, "zero-coef", None)


def bool_coeffs(con: Con, u: int, p: int):
    """Exact (α2, α1, α0) of ``A*B - C`` in ``u`` with ``w 0 = 1``, when the constraint is ``κ (u² - u)``."""
    if u == 0 or not (set(con.a) | set(con.b) | set(con.c)) <= {0, u} or con.lhs_zero:
        return None
    a0, au, b0, bu = con.a.get(0, 0), con.a.get(u, 0), con.b.get(0, 0), con.b.get(u, 0)
    c0, cu = con.c.get(0, 0), con.c.get(u, 0)
    a2, a1, a0_ = au * bu, au * b0 + bu * a0 - cu, a0 * b0 - c0
    if a2 % p and (a1 + a2) % p == 0 and a0_ % p == 0:
        return a2, a1, a0_
    return None


def _pow2_table(p: int) -> dict:
    return {pow(2, e, p): e for e in range(p.bit_length() + 1)}


def bits_rule(con: Con, unknown: set, boolean: dict, p: int, pow2: dict):
    """(info, reason): the unknown bits of a linear constraint, when bit-decomposition uniqueness applies."""
    lf = linear_form(con, p)
    if lf is None:
        return None, "multi-unknown"
    mode, form = lf
    if not unknown <= set(boolean):
        return None, "multi-unknown"
    best = None
    for base in sorted(unknown):
        s = form.get(base, 0)
        if not s % p:
            return None, "bits-other"
        inv = pow(s, -1, p)
        exps = {}
        for u in unknown:
            e = pow2.get(form.get(u, 0) * inv % p)
            if e is None:
                break
            exps[u] = e
        else:
            if len(set(exps.values())) == len(exps):
                best = (s, exps)
                break
    if best is None:
        return None, "bits-other"
    s, exps = best
    n = max(exps.values()) + 1
    if 2 ** n >= p:
        return None, "bits-too-wide"
    return {"mode": mode, "s": s, "exps": exps, "n": n, "form": form}, None


def propagate(m: Model, rules=RULES) -> Result:
    p = m.p
    known = set(m.inputs) | {0}
    steps: list[Step] = []
    w2c: dict = {}
    for c in m.cons:
        for w in set(c.a) | set(c.b) | set(c.c):
            w2c.setdefault(w, []).append(c.idx)
    unk = {c.idx: len(m.wires(c.idx) - known) for c in m.cons}
    boolean = {}
    for c in m.cons:
        ws = m.wires(c.idx) - {0}
        if len(ws) == 1:
            u = next(iter(ws))
            if u not in boolean and bool_coeffs(c, u, p) is not None:
                boolean[u] = c.idx
    calls = m.calls or []
    w2call: dict = {}
    for k, (_, ins, _) in enumerate(calls):
        for w in set(ins):
            w2call.setdefault(w, []).append(k)
    call_unk = {k: len(set(ins) - known) for k, (_, ins, _) in enumerate(calls)}
    done_calls: set = set()
    failed: dict = {}
    pow2 = _pow2_table(p)
    queue = [j for j, n in unk.items() if n == 1]
    cqueue = [k for k, n in call_unk.items() if n == 0] if "call" in rules else []
    outs = set(m.outputs) | {w for g in (m.emulated or []) for w in g[0]}

    def learn(ws) -> None:
        for w in ws:
            if w in known:
                continue
            known.add(w)
            for j in w2c.get(w, []):
                unk[j] -= 1
                if unk[j] == 1:
                    queue.append(j)
            for k in w2call.get(w, []):
                call_unk[k] -= 1
                if call_unk[k] == 0:
                    cqueue.append(k)

    while not outs <= known:
        progressed = False
        while cqueue and "call" in rules:
            k = cqueue.pop()
            if k in done_calls:
                continue
            done_calls.add(k)
            new = [w for w in dict.fromkeys(calls[k][2]) if w not in known]
            if new:
                steps.append(Step("call", k, new, sorted(set(calls[k][1]))))
                learn(new)
                progressed = True
        while queue and "lin" in rules:
            j = queue.pop()
            if unk[j] != 1:
                continue
            u = next(iter(m.wires(j) - known))
            mode, kappa, form = lin_rule(m.cons[j], u, p)
            if mode is None:
                failed[j] = kappa
                continue
            steps.append(Step("lin", j, [u], sorted(m.wires(j) - {u}), {"mode": mode, "kappa": kappa}))
            learn([u])
            progressed = True
            if cqueue:
                break
        if progressed:
            continue
        if "bits" in rules:
            for c in m.cons:
                if unk[c.idx] < 2:
                    continue
                u_set = m.wires(c.idx) - known
                info, why = bits_rule(c, u_set, boolean, p, pow2)
                if info is None:
                    continue
                order = sorted(u_set, key=lambda w: info["exps"][w])
                info["bools"] = {w: boolean[w] for w in order}
                steps.append(Step("bits", c.idx, order, sorted(m.wires(c.idx) - u_set), info))
                learn(order)
                progressed = True
                break
        if not progressed and "isz" in rules:
            hit = isz_rule(m, known)
            if hit is not None:
                c1, x1 = m.cons[hit["j1"]], hit["x1"]
                deps = sorted((set(x1) | set(c1.c) | set(m.cons[hit["j2"]].a) | set(m.cons[hit["j2"]].b))
                              - {hit["v"], hit["o"]})
                steps.append(Step("isz", hit["j1"], [hit["o"]], deps, hit))
                learn([hit["o"]])
                progressed = True
        if not progressed and "elim" in rules:
            for st in elim_round(m, known, boolean, pow2):
                steps.append(st)
                learn(st.wires)
                progressed = True
        if not progressed:
            break
    und = [o for o in dict.fromkeys(m.outputs) if o not in known]
    und += [w for g in (m.emulated or []) for w in g[0] if w not in known and w not in und]
    if not und:
        return Result("DETERMINED", steps, known, [])
    return Result("STUCK", steps, known, und, classify_stuck(m, known, boolean, pow2, set(und)))


def isz_rule(m: Model, known: set):
    """The IsZero gadget: ``X * (β v) = γ o + K`` and ``(r X) * (δ o) = 0`` with ``X`` and ``K`` known, ``X`` not
    constant, ``v`` and ``o`` unknown, ``γ``, ``r δ`` ∈ {±1} (exact integers): ``o = -γK`` when ``X = 0``, else ``o = 0``,
    so ``o`` is determined (``v`` is not).  First match in constraint order, or None."""
    second: dict = {}
    for c in m.cons:
        if c.lhs_zero or c.c:
            continue
        for x, y in ((c.a, c.b), (c.b, c.a)):
            if len(y) == 1 and set(x) - {0} and set(x) <= known:
                o = next(iter(y))
                if o not in known:
                    second.setdefault(o, []).append((c.idx, x, y[o]))
    if not second:
        return None
    for c in m.cons:
        if c.lhs_zero:
            continue
        for which, (x, y) in enumerate(((c.a, c.b), (c.b, c.a))):
            if len(y) != 1 or not set(x) - {0} or not set(x) <= known:
                continue
            v = next(iter(y))
            unk_c = [w for w in c.c if w not in known]
            if v in known or len(unk_c) != 1 or unk_c[0] == v or c.c[unk_c[0]] not in (1, -1):
                continue
            o = unk_c[0]
            for j2, x2, delta in second.get(o, []):
                for r in (1, -1):
                    if set(x2) == set(x) and all(x2[w] == r * x[w] for w in x) and r * delta in (1, -1):
                        return {"j1": c.idx, "which": "AB"[which], "x1": x, "beta": y[v], "v": v, "o": o,
                                "gamma": c.c[o], "j2": j2, "r": r, "delta": delta}
    return None


INPUT_PEEL = 32      # input lists longer than this are peeled once instead of indexed per input
ELIM_MAX = 4000      # unknown wires / rows above which the elimination rule is not attempted


def _bal(x: int, p: int) -> int:
    x %= p
    return x if x <= p // 2 else x - p


def elim_rows(m: Model, known: set, boolean: dict) -> list:
    """Rows of the difference system ``d = w₁ - w₂`` over the unknown wires: every constraint that is affine in the
    unknowns with constant coefficients (a linear constraint; a product of known factors; or ``A * B`` whose factors
    hold one boolean unknown ``b`` and constants, with ``b² = b``).  (row id, description, {wire: coefficient mod p})."""
    p = m.p
    rows = []
    for c in m.cons:
        u = m.wires(c.idx) - known
        if not u:
            continue
        lf = linear_form(c, p)
        if lf is not None:
            row, desc = {w: lf[1][w] % p for w in u if lf[1].get(w, 0) % p}, {"kind": lf[0], "j": c.idx}
        elif not u & (set(c.a) | set(c.b)):
            row, desc = {w: c.c[w] % p for w in u if c.c[w] % p}, {"kind": "prod", "j": c.idx}
        else:
            ua, ub = u & set(c.a), u & set(c.b)
            if not (len(ua) == 1 and ua == ub and set(c.a) <= {0} | ua and set(c.b) <= {0} | ua):
                continue
            b = next(iter(ua))
            if b not in boolean:
                continue
            al, be, a0, b0 = c.a[b], c.b[b], c.a.get(0, 0), c.b.get(0, 0)
            row = {b: (al * be + al * b0 + be * a0) % p}
            for w, k in c.c.items():
                if w in u:
                    row[w] = (row.get(w, 0) - k) % p
            row = {w: k for w, k in row.items() if k}
            desc = {"kind": "sq", "j": c.idx, "b": b, "jb": boolean[b], "sq": al * be}
        if row:
            rows.append((len(rows), desc, row))
    return rows


def elim_round(m: Model, known: set, boolean: dict, pow2: dict) -> list:
    """``elim``: reduced row echelon form of the difference system (non-boolean unknowns pivot first).  A row ``e_u``
    determines ``u``; a row over boolean unknowns only with coefficients ``s·2^e`` (distinct ``e``, ``2^n < p``)
    determines its bits.  Each step records the combination of original rows that yields it."""
    p = m.p
    unknown = sorted({w for c in m.cons for w in m.wires(c.idx)} - known)
    if not unknown or len(unknown) > ELIM_MAX:
        return []
    rows = elim_rows(m, known, boolean)
    if not rows or len(rows) > ELIM_MAX:
        return []
    order = [w for w in unknown if w not in boolean] + [w for w in unknown if w in boolean]
    pos = {w: i for i, w in enumerate(order)}
    piv: dict = {}

    def sub(dst: dict, src: dict, f: int) -> None:
        for w, k in src.items():
            v = (dst.get(w, 0) - f * k) % p
            if v:
                dst[w] = v
            else:
                dst.pop(w, None)

    for rid, _, row in rows:
        r, comb = dict(row), {rid: 1}
        while r:
            c = min(r, key=pos.__getitem__)
            if c in piv:
                f = r[c]
                sub(r, piv[c][0], f)
                sub(comb, piv[c][1], f)
                continue
            inv = pow(r[c], -1, p)
            r = {w: k * inv % p for w, k in r.items()}
            comb = {q: k * inv % p for q, k in comb.items()}
            for r2, c2 in piv.values():
                if c in r2:
                    f = r2[c]
                    sub(r2, r, f)
                    sub(c2, comb, f)
            piv[c] = (r, comb)
            break
    desc = {rid: d for rid, d, _ in rows}
    steps, done = [], set()

    def deps_of(comb: dict, targets: set) -> list:
        ws = set()
        for q in comb:
            d = desc[q]
            ws |= m.wires(d["j"])
        return sorted((ws & known) | {0})

    for c, (r, comb) in sorted(piv.items()):
        if set(r) == {c}:
            info = {"kind": "unit", "combo": dict(comb), "rows": {q: desc[q] for q in comb}}
            steps.append(Step("elim", None, [c], deps_of(comb, {c}), info))
            done.add(c)
    for c, (r, comb) in sorted(piv.items()):
        if c in done or c not in boolean or not set(r) <= set(boolean) or set(r) & done:
            continue
        for base in sorted(r):
            inv = pow(r[base], -1, p)
            exps = {}
            for w, k in r.items():
                e = pow2.get(k * inv % p)
                if e is None:
                    break
                exps[w] = e
            else:
                if len(set(exps.values())) == len(exps) and 2 ** (max(exps.values()) + 1) < p:
                    order_w = sorted(exps, key=exps.get)
                    info = {"kind": "bits", "combo": dict(comb), "rows": {q: desc[q] for q in comb},
                            "s": _bal(r[base], p), "exps": exps, "n": max(exps.values()) + 1,
                            "bools": {w: boolean[w] for w in order_w}}
                    steps.append(Step("elim", None, order_w, deps_of(comb, set(r)), info))
                    done |= set(r)
                    break
    return steps


def classify_stuck(m: Model, known: set, boolean: dict, pow2: dict, und: set) -> dict:
    """Frontier classes of a STUCK propagation (constraints that still hold unknown wires)."""
    p = m.p
    classes: dict = {}
    for c in m.cons:
        u_set = m.wires(c.idx) - known
        if not u_set:
            continue
        if len(u_set) == 1:
            why = lin_rule(c, next(iter(u_set)), p)[1]
            cls = why if isinstance(why, str) else "lin-applicable"
        else:
            cls = bits_rule(c, u_set, boolean, p, pow2)[1] or "bits-applicable"
        classes[cls] = classes.get(cls, 0) + 1
    constrained = {w for c in m.cons for w in m.wires(c.idx)} | {w for _, _, o in (m.calls or []) for w in o}
    free_out = sorted(o for o in und if o not in constrained)
    order = ["output-unconstrained", "bits-too-wide", "nonconst-coef", "quadratic", "zero-coef", "bits-other",
             "multi-unknown"]
    present = (["output-unconstrained"] if free_out else []) + [k for k in order[1:] if classes.get(k)]
    return {"classes": dict(sorted(classes.items())), "free_outputs": len(free_out),
            "primary": present[0] if present else "no-frontier",
            "unknown_wires": m.n_wires - len(known), "undetermined_outputs": len(und)}


def needed_steps(m: Model, res: Result) -> list:
    """Backward slice from the outputs (and emulated limbs) over the steps."""
    need = set(m.outputs) | {w for g in (m.emulated or []) for w in g[0]}
    out = []
    for st in reversed(res.steps):
        if need & set(st.wires):
            out.append(st)
            need |= set(st.deps)
            if st.rule == "bits":
                need |= set(st.wires)
    return list(reversed(out))


# ------------------------------------------------------------------------------------------------ emission

# Exact polynomials over the variables (side, wire): {monomial: integer}, a monomial a sorted tuple of variables.

def padd(a: dict, b: dict, k: int = 1) -> dict:
    out = dict(a)
    for mono, c in b.items():
        v = out.get(mono, 0) + k * c
        if v:
            out[mono] = v
        else:
            out.pop(mono, None)
    return out


def pmul(a: dict, b: dict) -> dict:
    out: dict = {}
    for m1, c1 in a.items():
        for m2, c2 in b.items():
            mono = tuple(sorted(m1 + m2))
            v = out.get(mono, 0) + c1 * c2
            if v:
                out[mono] = v
            else:
                out.pop(mono, None)
    return out


def pconst(c: int) -> dict:
    return {(): c} if c else {}


def pvar(t: int, w: int) -> dict:
    return {((t, w),): 1}


def plc(d: dict, t: int) -> dict:
    return {((t, w),): k for w, k in d.items() if k}


def pE(w: int) -> dict:
    return {((1, w),): 1, ((2, w),): -1}


def constraint_poly(con: Con, t: int) -> dict:
    """``A*B - C`` on side t, wire 0 a variable (the left side minus the right side of the printed constraint)."""
    ab = {} if con.lhs_zero else pmul(plc(con.a, t), plc(con.b, t))
    return padd(ab, plc(con.c, t), -1)


def split_w0(poly: dict, t: int) -> tuple[dict, dict]:
    """(Q, P₁) with poly = (w0 - 1)·Q + P₁ and P₁ free of w0 (w0^d - 1 = (w0 - 1)(1 + … + w0^(d-1)))."""
    z = (t, 0)
    q: dict = {}
    p1: dict = {}
    for mono, c in poly.items():
        d = mono.count(z)
        rest = tuple(v for v in mono if v != z)
        p1 = padd(p1, {rest: c})
        for i in range(d):
            q = padd(q, {tuple(sorted(rest + (z,) * i)): c})
    return q, p1


def ptext(poly: dict) -> str:
    """Lean text of a polynomial (variables printed `w₁ k` / `w₂ k`, coefficients as `(c : F)`)."""
    parts = []
    for mono, c in sorted(poly.items(), key=lambda kv: (len(kv[0]), kv[0])):
        vs = " * ".join(f"w{'₁₂'[t - 1]} {w}" for t, w in mono)
        a = abs(c)
        body = vs if (a == 1 and vs) else (f"({a} : F) * {vs}" if vs else f"({a} : F)")
        parts.append(("-" if c < 0 else "+", body))
    if not parts:
        return "0"
    s = ("-" if parts[0][0] == "-" else "") + parts[0][1]
    for sign, body in parts[1:]:
        s += f" {sign} {body}"
    return s


def lc_expr(terms) -> str:
    """Σ k * h as a linear_combination argument; terms [(k, proof)] with k an integer or a polynomial."""
    out = []
    for k, h in terms:
        if isinstance(k, dict):
            if not k:
                continue
            if set(k) == {()}:
                k = k[()]
            else:
                out.append(("+", f"({ptext(k)}) * {h}"))
                continue
        if k == 0:
            continue
        a = abs(k)
        out.append(("-" if k < 0 else "+", h if a == 1 else f"({a} : F) * {h}"))
    if not out:
        return "0"
    s = ("-" if out[0][0] == "-" else "") + out[0][1]
    for sign, body in out[1:]:
        s += f" {sign} {body}"
    return s


def _scale(k, f: int):
    return {m: c * f for m, c in k.items()} if isinstance(k, dict) else k * f


def _neg(terms) -> list:
    return [({m: -c for m, c in k.items()} if isinstance(k, dict) else -k, h) for k, h in terms]


class Emitter:
    def __init__(self, m: Model, res: Result):
        self.m, self.res = m, res
        self.lines: list[str] = []
        self.need_hP = False
        self.helpers = {"bits": False, "em": False, "isz": False}
        self.factor_cache: dict = {}
        self.have: set = set()
        self.verified = 0
        self.forms: dict = {"ha0": padd(pvar(1, 0), pconst(1), -1), "hb0": padd(pvar(2, 0), pconst(1), -1),
                            "hP": pconst(m.p)}

    def ln(self, s: str, ind: int = 1) -> None:
        self.lines.append("  " * ind + s)

    def e(self, w: int) -> str:
        if f"e{w}" not in self.forms:
            self.forms[f"e{w}"] = pE(w)
        return f"e{w}"

    # ---- hypotheses
    def con_hyp(self, j: int, t: int) -> str:
        """Projection of constraint j from h_t (emitted on first use; blocks first)."""
        nm = f"{'ab'[t - 1]}{j}"
        if nm in self.have:
            return nm
        h = f"h{'₁₂'[t - 1]}"
        m = self.m
        if m.blocks:
            bi = next(k for k, blk in enumerate(m.blocks) if j in blk)
            kb = f"k{'ab'[t - 1]}{bi}"
            if kb not in self.have:
                path = ".2" * (bi + 1) + (".1" if bi + 1 < len(m.blocks) else "")
                self.ln(f"have {kb} := {h}{path}")
                self.have.add(kb)
            blk = m.blocks[bi]
            i = blk.index(j)
            if len(blk) == 1:
                self.ln(f"have {nm} : {self.side_text(j, t)} := {kb}")
            else:
                path = ".2" * i + (".1" if i + 1 < len(blk) else "")
                self.ln(f"have {nm} := {kb}{path}")
        else:
            n = len(m.cons)
            path = ".2" * (j + 1) + (".1" if j + 1 < n else "")
            self.ln(f"have {nm} := {h}{path}")
        self.have.add(nm)
        self.forms[nm] = constraint_poly(m.cons[j], t)
        return nm

    def side_text(self, j: int, t: int) -> str:
        return re.sub(r"\bw (\d+)", "w" + "₁₂"[t - 1] + r" \1", self.m.cons[j].text)

    def lin_terms(self, j: int, t: int, mode: str) -> list:
        """Terms whose combination is the constraint on side t (``lin0``) or with ``w 0 = 1`` substituted (``rw``)."""
        h = self.con_hyp(j, t)
        if mode == "lin0":
            return [(1, h)]
        q, _ = split_w0(self.forms[h], t)
        return [(1, h), ({mono: -c for mono, c in q.items()}, "ha0" if t == 1 else "hb0")]

    def form_of(self, terms) -> dict:
        acc: dict = {}
        for k, h in terms:
            acc = padd(acc, pmul(k if isinstance(k, dict) else pconst(k), self.forms[h]))
        return acc

    # ---- derived facts
    def derive(self, name: str, stmt: str, lhs: str, rhs: str, goal: dict, terms: list, kappa: int,
               pre_exp: str = "", pre_final: str = "") -> None:
        """``have name : stmt`` where ``terms`` combine to exactly κ·(lhs - rhs) (checked here); κ ∉ {±1} through
        ``(p : F) = 0``."""
        if self.form_of(terms) != {mono: kappa * c for mono, c in goal.items()}:
            diff = padd(self.form_of(terms), goal, -kappa)
            raise AssertionError(f"{name}: identity does not hold ({sorted(diff.items())[:4]})")
        self.verified += 1
        if kappa in (1, -1):
            exp = lc_expr(terms if kappa == 1 else _neg(terms))
            self.ln(f"have {name} : {stmt} := by {pre_final}{pre_exp}linear_combination {exp}")
            return
        p = self.m.p
        self.need_hP = True
        diff = f"(({lhs}) - ({rhs}))"
        mi = pow(kappa, -1, p)
        t = (mi * kappa - 1) // p
        self.ln(f"have {name}' : ({kappa} : F) * {diff} = 0 := by {pre_exp}linear_combination {lc_expr(terms)}")
        self.ln(f"have {name} : {stmt} := by {pre_final}linear_combination ({mi} : F) * {name}' - "
                f"(({t} : F) * {diff}) * hP")

    def step_lin(self, st: Step) -> None:
        m, j, u = self.m, st.con, st.wires[0]
        con = m.cons[j]
        mode, kappa = st.info["mode"], st.info["kappa"]
        if mode in ("lin0", "rw"):
            _, form = linear_form(con, m.p)
            terms = self.lin_terms(j, 1, mode) + _neg(self.lin_terms(j, 2, mode))
            terms += [(-k, self.e(w)) for w, k in sorted(form.items(), key=_key) if w not in (u, None)]
        else:
            hm = self.prod_hyp(j)
            terms = [(1, hm)] + [(-k, self.e(w)) for w, k in sorted(con.c.items()) if w != u]
        self.derive(self.e(u), f"w₁ {u} = w₂ {u}", f"w₁ {u}", f"w₂ {u}", pE(u), terms, kappa)

    def factor_eq(self, j: int, which: str) -> str:
        """The known factor A or B of constraint j is equal on both sides."""
        con = self.m.cons[j]
        f = con.a if which == "A" else con.b
        txt = _split_top(con.text.split(" = ")[0], " * ")[0 if which == "A" else 1]
        if re.fullmatch(r"w \d+", txt):
            return self.e(int(txt[2:]))
        if txt in self.factor_cache:                   # the same known factor proven equal before
            return self.factor_cache[txt]
        nm = f"h{which}{j}"
        self.factor_cache[txt] = nm
        t1 = re.sub(r"\bw (\d+)", r"w₁ \1", txt)
        t2 = re.sub(r"\bw (\d+)", r"w₂ \1", txt)
        goal = padd(plc(f, 1), plc(f, 2), -1)
        self.derive(nm, f"{t1} = {t2}", t1, t2, goal, [(k, self.e(w)) for w, k in sorted(f.items())], 1)
        return nm

    def prod_hyp(self, j: int) -> str:
        """``hm<j> : C(w₁) = C(w₂)`` for a product constraint with known factors."""
        con = self.m.cons[j]
        nm = f"hm{j}"
        a1, a2 = self.con_hyp(j, 1), self.con_hyp(j, 2)
        hA, hB = self.factor_eq(j, "A"), self.factor_eq(j, "B")
        self.ln(f"have {nm} := {a1}.symm.trans ((congr (congrArg HMul.hMul {hA}) {hB}).trans {a2})")
        self.forms[nm] = padd(plc(con.c, 1), plc(con.c, 2), -1)
        return nm

    def bool_hyp(self, w: int, j: int, t: int) -> str:
        nm = f"b{'ab'[t - 1]}{w}"
        if nm in self.have:
            return nm
        p = self.m.p
        a2, a1, a0 = bool_coeffs(self.m.cons[j], w, p)
        x = f"w{'₁₂'[t - 1]} {w}"
        terms = self.lin_terms(j, t, "rw" if 0 in self.m.wires(j) else "lin0")
        q1, q0 = (a1 + a2) // p, a0 // p
        if q1 or q0:
            self.need_hP = True
            terms.append((padd(pmul(pconst(-q1), pvar(t, w)), pconst(-q0)), "hP"))
        goal = padd(pmul(pvar(t, w), pvar(t, w)), pvar(t, w), -1)
        self.derive(nm, f"{x} = 0 ∨ {x} = 1", f"{x} * {x} - {x}", "0", goal, terms, a2, pre_final="apply mech_bit; ")
        self.have.add(nm)
        return nm

    def step_bits(self, st: Step) -> None:
        self.helpers["bits"] = True
        m, j, info = self.m, st.con, st.info
        p, s, exps, n, form, mode = m.p, info["s"], info["exps"], info["n"], info["form"], info["mode"]
        # s·(lsum l₁ - lsum l₂) = (h₁ - h₂) - Σ_known ℓ e - p·Σ_bits k_u E_u, where ℓ_u = s·2^e_u + k_u·p
        terms = self.lin_terms(j, 1, mode) + _neg(self.lin_terms(j, 2, mode))
        terms += [(-k, self.e(w)) for w, k in sorted(form.items(), key=_key) if w is not None and w not in exps]
        ks = {w: (form[w] - s * 2 ** e) // p for w, e in exps.items()}
        if any(ks.values()):
            self.need_hP = True
            corr: dict = {}
            for w, k in ks.items():
                corr = padd(corr, pE(w), -k)
            terms.append((corr, "hP"))
        goal: dict = {}
        for w, e in exps.items():
            goal = padd(goal, pE(w), 2 ** e)
        self.bits_tail(str(j), exps, n, s, goal, terms, info["bools"])

    def fix_residual(self, goal: dict, kappa: int, terms: list) -> list:
        """Append ``(R / p) * hP`` so that the terms combine to exactly κ·goal (R ≡ 0 mod p is checked)."""
        p = self.m.p
        res = padd({mono: kappa * c for mono, c in goal.items()}, self.form_of(terms), -1)
        if res:
            if any(c % p for c in res.values()):
                raise AssertionError("elimination combination is not ≡ κ·goal mod p")
            self.need_hP = True
            terms = terms + [({mono: c // p for mono, c in res.items()}, "hP")]
        return terms

    def row_terms(self, d: dict) -> list:
        """Terms of one difference row (side 1 minus side 2) of the elimination rule."""
        j, kind = d["j"], d["kind"]
        if kind in ("lin0", "rw"):
            return self.lin_terms(j, 1, kind) + _neg(self.lin_terms(j, 2, kind))
        if kind == "prod":
            return [(1, self.prod_hyp(j))]
        p = self.m.p
        a2 = bool_coeffs(self.m.cons[d["jb"]], d["b"], p)[0]
        kap = _bal(d["sq"] * pow(a2, -1, p), p)
        out = []
        for t, sg in ((1, 1), (2, -1)):
            mode = "rw" if 0 in self.m.wires(j) else "lin0"
            bmode = "rw" if 0 in self.m.wires(d["jb"]) else "lin0"
            tt = self.lin_terms(j, t, mode) + [(_scale(k, -kap), h) for k, h in self.lin_terms(d["jb"], t, bmode)]
            out += tt if sg == 1 else _neg(tt)
        return out

    def combo_terms(self, st: Step, targets: set) -> list:
        p = self.m.p
        terms: list = []
        for q, lam in sorted(st.info["combo"].items()):
            lam = _bal(lam, p)
            terms += [(_scale(k, lam), h) for k, h in self.row_terms(st.info["rows"][q])]
        f = self.form_of(terms)
        for mono, c in sorted(f.items()):
            if len(mono) == 1 and mono[0][0] == 1 and mono[0][1] not in targets and c % p:
                w = mono[0][1]
                if w in self.res.known:
                    terms.append((-_bal(c, p), self.e(w)))
        return terms

    def step_elim(self, st: Step) -> None:
        info = st.info
        if info["kind"] == "unit":
            u = st.wires[0]
            terms = self.fix_residual(pE(u), 1, self.combo_terms(st, {u}))
            self.derive(self.e(u), f"w₁ {u} = w₂ {u}", f"w₁ {u}", f"w₂ {u}", pE(u), terms, 1)
            return
        self.helpers["bits"] = True
        exps, n, s_ = info["exps"], info["n"], info["s"]
        goal: dict = {}
        for w, e in exps.items():
            goal = padd(goal, pE(w), 2 ** e)
        terms = self.fix_residual(goal, s_, self.combo_terms(st, set(exps)))
        self.bits_tail(f"x{st.wires[0]}", exps, n, s_, goal, terms, info["bools"])

    def bits_tail(self, tag: str, exps: dict, n: int, s: int, goal: dict, terms: list, bools: dict) -> None:
        at = {e: w for w, e in exps.items()}
        b1 = {w: self.bool_hyp(w, bools[w], 1) for w in exps}
        b2 = {w: self.bool_hyp(w, bools[w], 2) for w in exps}
        l1 = "[" + ", ".join(f"w₁ {at[e]}" if e in at else "0" for e in range(n)) + "]"
        l2 = "[" + ", ".join(f"w₂ {at[e]}" if e in at else "0" for e in range(n)) + "]"
        ab1 = "⟨" + ", ".join(b1[at[e]] if e in at else "Or.inl rfl" for e in range(n)) + ", trivial⟩"
        ab2 = "⟨" + ", ".join(b2[at[e]] if e in at else "Or.inl rfl" for e in range(n)) + ", trivial⟩"
        hs = f"hs{tag}"
        self.derive(hs, f"mechLsum {l1} = mechLsum {l2}", f"mechLsum {l1}", f"mechLsum {l2}", goal, terms, s,
                    pre_exp="simp only [mechLsum]; ")
        hn = f"hn{n}"
        if hn not in self.have:
            self.ln(f"have {hn} : 2 ^ {n} < p := by norm_num")
            self.have.add(hn)
        q = f"q{tag}"
        self.ln(f"have {q} : {l1} = {l2} := mech_lsum_inj {n} rfl rfl {hn} {ab1} {ab2} {hs}")
        for e in range(n):
            if e in at:
                w = at[e]
                self.ln(f"have {self.e(w)} : w₁ {w} = w₂ {w} := congrArg (fun l => List.getD l {e} 0) {q}")

    def step_call(self, st: Step) -> None:
        m, k = self.m, st.con
        kind, ins, outs = m.calls[k]
        o1 = "[" + ", ".join(f"w₁ {w}" for w in outs) + "]"
        o2 = "[" + ", ".join(f"w₂ {w}" for w in outs) + "]"
        i1 = "[" + ", ".join(f"w₁ {w}" for w in ins) + "]"
        i2 = "[" + ", ".join(f"w₂ {w}" for w in ins) + "]"
        mem = f"(List.getElem_mem (Nat.lt_of_lt_of_eq (of_decide_eq_true (p := {k} < {len(m.calls)}) rfl) hCalls.symm))"
        q = f"hr{k}"
        self.ln(f"have {q} : {o1} = {o2} := by")
        self.ln(f"have r₁ : {o1} = f {kind} {i1} := hc₁ _ {mem}", 2)
        self.ln(f"have r₂ : {o2} = f {kind} {i2} := hc₂ _ {mem}", 2)
        rws = ", ".join(["r₁", "r₂"] + [self.e(w) for w in dict.fromkeys(ins)])
        self.ln(f"rw [{rws}]", 2)
        new = set(st.wires)
        for i, w in enumerate(outs):
            if w in new:
                new.discard(w)
                self.ln(f"have {self.e(w)} : w₁ {w} = w₂ {w} := congrArg (fun l => List.getD l {i} 0) {q}")

    def step_isz(self, st: Step) -> None:
        """``mech_isz`` on the normalized pair (``X := A(w₂)``, ``iₜ := βγ·vₜ``, ``k := γ·K(w₂)``)."""
        self.helpers["isz"] = True
        m, h = self.m, st.info
        j1, j2, v, o, beta, gamma, rd = h["j1"], h["j2"], h["v"], h["o"], h["beta"], h["gamma"], h["r"] * h["delta"]
        c1 = m.cons[j1]
        x = h["x1"]
        txt = _split_top(c1.text.split(" = ")[0], " * ")[0 if h["which"] == "A" else 1]
        tx = re.sub(r"\bw (\d+)", r"w₂ \1", txt)
        kpoly = {mono: gamma * cf for mono, cf in plc({w: k for w, k in c1.c.items() if w != o}, 2).items()}
        kt = ptext(kpoly)
        bg = beta * gamma
        xa = {w: k for w, k in x.items()}
        a1, b1, a2, b2 = self.con_hyp(j1, 1), self.con_hyp(j1, 2), self.con_hyp(j2, 1), self.con_hyp(j2, 2)
        # side 1: from the constraint on w₁ and the known equalities (X(w₁) = X(w₂), K(w₁) = K(w₂))
        t1 = [(gamma, a1)] + [(pmul(pconst(-bg * k), pvar(1, v)), self.e(w)) for w, k in sorted(xa.items())]
        t1 += [(gamma * k, self.e(w)) for w, k in sorted(c1.c.items()) if w != o]
        g1 = padd(pmul(pmul(plc(xa, 2), pconst(bg)), pvar(1, v)), padd(pvar(1, o), kpoly), -1)
        n1, n2, n3, n4 = (f"hz{j1}_{k}" for k in range(1, 5))
        self.derive(n1, f"{tx} * (({bg} : F) * w₁ {v}) = w₁ {o} + ({kt})", "", "", g1, t1, 1)
        t2 = [(rd, a2)] + [(pmul(pconst(-k), pvar(1, o)), self.e(w)) for w, k in sorted(xa.items())]
        g2 = pmul(plc(xa, 2), pvar(1, o))
        self.derive(n2, f"{tx} * w₁ {o} = 0", "", "", g2, t2, 1)
        g3 = padd(pmul(pmul(plc(xa, 2), pconst(bg)), pvar(2, v)), padd(pvar(2, o), kpoly), -1)
        self.derive(n3, f"{tx} * (({bg} : F) * w₂ {v}) = w₂ {o} + ({kt})", "", "", g3, [(gamma, b1)], 1)
        self.derive(n4, f"{tx} * w₂ {o} = 0", "", "", pmul(plc(xa, 2), pvar(2, o)), [(rd, b2)], 1)
        self.ln(f"have {self.e(o)} : w₁ {o} = w₂ {o} := mech_isz {n1} {n2} {n3} {n4}")

    # ---- whole proof
    def proof(self) -> str:
        m = self.m
        steps = needed_steps(m, self.res)
        if m.calls is not None:
            self.ln("intro f w₁ w₂ h₁ h₂ hc₁ hc₂ hin")
        elif m.preconditions:
            self.ln("intro w₁ w₂ h₁ h₂ hp₁ hp₂ hin")
        else:
            self.ln("intro w₁ w₂ h₁ h₂ hin")
        self.ln("have ha0 : w₁ 0 = 1 := h₁.1")
        self.ln("have hb0 : w₂ 0 = 1 := h₂.1")
        self.ln("have e0 : w₁ 0 = w₂ 0 := ha0.trans hb0.symm")
        self.e(0)
        hp_at = len(self.lines)
        need = set(m.outputs) | {w for g in (m.emulated or []) for w in g[0]}
        for st in steps:
            need |= set(st.deps)
        used_inputs = [i for i in dict.fromkeys(m.inputs) if i in need and i != 0]
        if len(m.inputs) > INPUT_PEEL and used_inputs:
            # long input lists: peel the list once (indexing each input would walk the list every time)
            last = max(m.inputs.index(i) for i in used_inputs)
            self.ln("unfold Inputs at hin")
            prev, seen = "hin", set()
            for pos in range(last + 1):
                w = m.inputs[pos]
                cur = f"hi{pos}"
                self.ln(f"have {cur} := List.forall_mem_cons.1 {prev}")
                if w in used_inputs and w not in seen:
                    seen.add(w)
                    self.ln(f"have {self.e(w)} : w₁ {w} = w₂ {w} := {cur}.1")
                prev = f"{cur}.2"
        elif used_inputs:
            self.ln(f"have hIn : Inputs.length = {len(m.inputs)} := rfl")
            for i in used_inputs:
                pos = m.inputs.index(i)
                self.ln(f"have {self.e(i)} : w₁ {i} = w₂ {i} := hin _ (List.getElem_mem (Nat.lt_of_lt_of_eq "
                        f"(of_decide_eq_true (p := {pos} < {len(m.inputs)}) rfl) hIn.symm))")
        if any(st.rule == "call" for st in steps):
            self.ln(f"have hCalls : Calls.length = {len(m.calls)} := rfl")
        for st in steps:
            {"lin": self.step_lin, "bits": self.step_bits, "call": self.step_call, "isz": self.step_isz,
             "elim": self.step_elim}[st.rule](st)
        if self.need_hP:
            self.lines.insert(hp_at, f"  have hP : ({m.p} : F) = 0 := ZMod.natCast_self p")
        if m.emulated is not None:
            self.ln("refine ⟨?_, ?_⟩")
            self.ln("· unfold Outputs")
            self.out_chain(2)
            self.helpers["em"] = True
            self.ln("· unfold EmulatedOutputs")
            for limbs, _, _ in m.emulated:
                chain = "List.forall_mem_nil _"
                for w in reversed(limbs):
                    chain = f"List.forall_mem_cons.2 ⟨{self.e(w)}, {chain}⟩"
                self.ln(f"refine List.forall_mem_cons.2 ⟨mech_em w₁ w₂ _ _ _ ({chain}), ?_⟩", 2)
            self.ln("exact List.forall_mem_nil _", 2)
        else:
            self.ln("unfold Outputs")
            self.out_chain(1)
        return "\n".join(self.lines)

    def out_chain(self, ind: int) -> None:
        for o in self.m.outputs:
            self.ln(f"refine List.forall_mem_cons.2 ⟨{self.e(o)}, ?_⟩", ind)
        self.ln("exact List.forall_mem_nil _", ind)


def _key(item):
    w = item[0]
    return -1 if w is None else w


HELPERS_BASE = """set_option maxHeartbeats 0
set_option maxRecDepth 100000
"""

HELPERS_BITS = """
/-- Little-endian weighted sum `x₀ + 2 x₁ + 4 x₂ + …` in the field (battery P3 helper). -/
def mechLsum : List F → F
  | [] => 0
  | x :: xs => x + 2 * mechLsum xs

/-- The same weighted sum of the `val`s, as a natural number. -/
def mechNsum : List F → ℕ
  | [] => 0
  | x :: xs => x.val + 2 * mechNsum xs

/-- Every entry is 0 or 1. -/
def MechAllBit : List F → Prop
  | [] => True
  | x :: xs => (x = 0 ∨ x = 1) ∧ MechAllBit xs

theorem mech_bit [Fact (Nat.Prime p)] {x : F} (h : x * x - x = 0) : x = 0 ∨ x = 1 := by
  have h' : x * (x - 1) = 0 := by linear_combination h
  rcases mul_eq_zero.mp h' with h | h
  · exact Or.inl h
  · exact Or.inr (by linear_combination h)

theorem mech_lsum_nsum [Fact (Nat.Prime p)] : ∀ l : List F, MechAllBit l →
    mechLsum l = (mechNsum l : F) ∧ mechNsum l < 2 ^ l.length
  | [], _ => by simp [mechLsum, mechNsum]
  | x :: xs, h => by
    obtain ⟨hx, hxs⟩ := h
    obtain ⟨ih1, ih2⟩ := mech_lsum_nsum xs hxs
    have hp : 2 ^ (x :: xs).length = 2 * 2 ^ xs.length := by
      simp [List.length_cons, pow_succ, mul_comm]
    rw [hp]
    rcases hx with rfl | rfl
    · refine ⟨?_, ?_⟩
      · simp [mechLsum, mechNsum, ih1]
      · simp only [mechNsum, ZMod.val_zero]; omega
    · refine ⟨?_, ?_⟩
      · simp [mechLsum, mechNsum, ih1, ZMod.val_one]
      · simp only [mechNsum, ZMod.val_one]; omega

theorem mech_nsum_inj [Fact (Nat.Prime p)] : ∀ l₁ l₂ : List F, MechAllBit l₁ → MechAllBit l₂ →
    l₁.length = l₂.length → mechNsum l₁ = mechNsum l₂ → l₁ = l₂
  | [], [], _, _, _, _ => rfl
  | [], _ :: _, _, _, hl, _ => by simp at hl
  | _ :: _, [], _, _, hl, _ => by simp at hl
  | x :: xs, y :: ys, h₁, h₂, hl, he => by
    obtain ⟨hx, hxs⟩ := h₁
    obtain ⟨hy, hys⟩ := h₂
    simp only [List.length_cons, Nat.add_right_cancel_iff] at hl
    simp only [mechNsum] at he
    rcases hx with rfl | rfl <;> rcases hy with rfl | rfl <;>
      simp only [ZMod.val_zero, ZMod.val_one] at he
    · rw [mech_nsum_inj xs ys hxs hys hl (by omega)]
    · omega
    · omega
    · rw [mech_nsum_inj xs ys hxs hys hl (by omega)]

/-- Bit-decomposition uniqueness: two bit vectors of length `n` with `2 ^ n < p` and equal weighted sums are
equal. -/
theorem mech_lsum_inj [Fact (Nat.Prime p)] {l₁ l₂ : List F} (n : ℕ) (hl₁ : l₁.length = n)
    (hl₂ : l₂.length = n) (hn : 2 ^ n < p) (h₁ : MechAllBit l₁) (h₂ : MechAllBit l₂)
    (h : mechLsum l₁ = mechLsum l₂) : l₁ = l₂ := by
  obtain ⟨a1, a2⟩ := mech_lsum_nsum l₁ h₁
  obtain ⟨b1, b2⟩ := mech_lsum_nsum l₂ h₂
  rw [hl₁] at a2
  rw [hl₂] at b2
  rw [a1, b1, ZMod.natCast_eq_natCast_iff'] at h
  rw [Nat.mod_eq_of_lt (by omega), Nat.mod_eq_of_lt (by omega)] at h
  exact mech_nsum_inj l₁ l₂ h₁ h₂ (hl₁.trans hl₂.symm) h
"""

HELPERS_ISZ = """
/-- The IsZero gadget determines its output: `X * i = o + k` and `X * o = 0` fix `o` (`-k` if `X = 0`, else `0`). -/
theorem mech_isz [Fact (Nat.Prime p)] {X i₁ i₂ o₁ o₂ k : F} (h₁ : X * i₁ = o₁ + k) (h₂ : X * o₁ = 0)
    (h₃ : X * i₂ = o₂ + k) (h₄ : X * o₂ = 0) : o₁ = o₂ := by
  by_cases hX : X = 0
  · subst hX
    linear_combination h₃ - h₁
  · rw [(mul_eq_zero.mp h₂).resolve_left hX, (mul_eq_zero.mp h₄).resolve_left hX]
"""

HELPERS_EM = """
theorem mech_emv (w₁ w₂ : Fin nWires → F) (b : ℕ) :
    ∀ l : List (Fin nWires), (∀ i ∈ l, w₁ i = w₂ i) → emValue w₁ l b = emValue w₂ l b
  | [], _ => rfl
  | a :: l, h => by
    have ih := mech_emv w₁ w₂ b l (fun i hi => h i (List.mem_cons_of_mem a hi))
    simp only [emValue, List.foldr_cons] at ih ⊢
    rw [h a (List.mem_cons.2 (Or.inl rfl)), ih]

theorem mech_em (w₁ w₂ : Fin nWires → F) (l : List (Fin nWires)) (b m : ℕ) (h : ∀ i ∈ l, w₁ i = w₂ i) :
    emValue w₁ l b % m = emValue w₂ l b % m := by
  rw [mech_emv w₁ w₂ b l h]
"""


def emit_solution(m: Model, statement: str, res: Result) -> tuple[str, dict]:
    """The submission text for a DETERMINED propagation and emission statistics."""
    if res.status != "DETERMINED":
        raise ValueError("only a DETERMINED propagation has a proof")
    em = Emitter(m, res)
    body = em.proof()
    helpers = (HELPERS_BASE + (HELPERS_BITS if em.helpers["bits"] else "") + (HELPERS_ISZ if em.helpers["isz"] else "")
               + (HELPERS_EM if em.helpers["em"] else ""))
    k = statement.find("/-- ")
    hole = statement.find("\n  sorry\n")
    if k < 0 or hole < 0 or statement.count("\n  sorry\n") != 1:
        raise ModelFormatError("statement has no doc comment or no single `  sorry` line")
    text = statement[:k] + helpers + "\n" + statement[k:hole + 1] + body + "\n" + statement[hole + len("\n  sorry\n"):]
    stats = {"proof_lines": len(em.lines), "verified_identities": em.verified, "bits_helper": em.helpers["bits"],
             "hP": em.need_hP, "chars": len(text)}
    return text, stats


# ------------------------------------------------------------------------------------------------ solve / batch

def rule_counts(steps) -> dict:
    out: dict = {}
    for st in steps:
        out[st.rule] = out.get(st.rule, 0) + 1
    return dict(sorted(out.items()))


def solve(pkg_dir: str, out: str | None = None, rules=RULES) -> dict:
    t0 = time.time()
    m, problem, statement = load_package(pkg_dir)
    res = propagate(m, rules)
    rec = {"package_id": problem["package_id"], "theorem": m.theorem, "n_constraints": len(m.cons),
           "n_wires": m.n_wires, "n_inputs": len(m.inputs), "n_outputs": len(m.outputs),
           "emulated_groups": len(m.emulated or []), "calls": len(m.calls or []), "preconditions": m.preconditions,
           "propagation": res.status, "steps": len(res.steps), "rules": rule_counts(res.steps)}
    if res.status == "DETERMINED":
        ns = needed_steps(m, res)
        rec["needed_steps"] = len(ns)
        rec["needed_rules"] = rule_counts(ns)
        text, stats = emit_solution(m, statement, res)
        rec["emit"] = stats
        if out:
            os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
            with open(out, "w", encoding="utf-8") as f:
                f.write(text)
    else:
        rec["stuck"] = res.stuck
        rec["undetermined_outputs"] = res.undetermined_outputs[:50]
    rec["solve_secs"] = round(time.time() - t0, 3)
    return rec


def error_class(rep: dict) -> str:
    msgs = " ".join(rep.get("fail", []) + rep.get("invalid", []) + rep.get("error", []))
    for pat, cls in [(r"timed out", "timeout"), (r"linear_combination|ring failed", "linear_combination"),
                     (r"maximum recursion", "max-recursion"), (r"heartbeats", "heartbeats"),
                     (r"unknown (identifier|constant)", "unknown-identifier"), (r"type mismatch", "type-mismatch"),
                     (r"rewrite|motive|did not find", "rewrite"), (r"norm_num|decide", "decide"),
                     (r"memory|RSS|killed", "memory")]:
        if re.search(pat, msgs):
            return cls
    return "other"


_RSS = threading.local()


def _instrument_rss() -> None:
    """Record the peak RSS of every Lean process the checker runs (per worker thread)."""
    from zk_registry import lean_runner as L
    if getattr(L.run_process, "_mech_wrapped", False):
        return
    orig = L.run_process

    def wrapped(*a, **kw):
        r = orig(*a, **kw)
        lst = getattr(_RSS, "peaks", None)
        if lst is not None:
            lst.append(r.peak_rss_mb)
            if r.memkill:
                _RSS.memkill = True
        return r

    wrapped._mech_wrapped = True
    L.run_process = wrapped


def batch(items_path: str, out_dir: str, lean_env: str | None, jobs: int, check: bool, timeout: int,
          rss_mb: int | None, results: str | None = None, limit: int | None = None, work_root: str | None = None,
          substitute_manifest: bool = False, redo_stuck: bool = False) -> None:
    """Solve every item (``{"dir", "pool", "package_id", ...}`` per line); check DETERMINED proofs with the
    production checker; append one JSON row per item to ``results`` (resumable: finished items are skipped; an item
    whose check hit the RSS guard is recorded as ``MEMKILL`` and retried by the next invocation, e.g. with fewer
    jobs and a higher guard; ``redo_stuck`` re-solves MECH-STUCK items too, e.g. after a rule was added; the last row
    per item is its result).

    ``substitute_manifest``: a package whose recorded lake-manifest digest differs from the environment's while its
    Lean and Mathlib pins are equal is checked against the environment with the package's digest substituted, and
    the row says so (``check_env_note``)."""
    from zk_registry import check as C
    from zk_registry import lean_runner as L
    results = results or os.path.join(out_dir, "RESULTS.jsonl")
    with open(items_path, encoding="utf-8") as f:
        items = [json.loads(ln) for ln in f if ln.strip()]
    done = set()
    if os.path.exists(results):
        with open(results, encoding="utf-8") as f:
            last = {}
            for ln in f:
                if ln.strip():
                    r = json.loads(ln)
                    last[r["dir"]] = r["outcome"]
            done = {d for d, o in last.items() if o != "MEMKILL" and not (redo_stuck and o == "MECH-STUCK")}
    todo = [it for it in items if it["dir"] not in done][:limit]
    env = L.load_env(lean_env) if check else None
    if check:
        L.set_lean_limits(slots=jobs, rss_mb=rss_mb)
        _instrument_rss()
    lock = threading.Lock()

    def run(it: dict) -> None:
        row = dict(it)
        tag = hashlib.sha256(it["dir"].encode()).hexdigest()[:10]
        rel = os.path.join(it.get("pool", "x"), re.sub(r"[^A-Za-z0-9._-]+", "_", it["package_id"])[:150] + "-" + tag)
        sol = os.path.join(out_dir, "proofs", rel, "Solution.lean")
        try:
            rec = solve(it["dir"], sol)
            row.update(rec)
            if rec["propagation"] != "DETERMINED":
                row["outcome"] = "MECH-STUCK"
                row["stuck_reason"] = rec["stuck"]["primary"]
            elif not check:
                row["outcome"] = "DETERMINED-UNCHECKED"
                row["solution"] = sol
            else:
                row["solution"] = sol
                _RSS.peaks, _RSS.memkill = [], False
                t0 = time.time()
                cenv = env
                with open(os.path.join(it["dir"], "problem.json"), encoding="utf-8") as f:
                    pins = json.load(f)["env"]
                if (substitute_manifest and pins["lake_manifest_sha256"] != env.manifest_sha256
                        and pins["lean"] == env.lean_version and pins["mathlib"] == env.packages.get("mathlib")):
                    cenv = dataclasses.replace(env, manifest_sha256=pins["lake_manifest_sha256"])
                    row["check_env_note"] = (f"lake manifest digest {pins['lake_manifest_sha256'][:12]} substituted for "
                                             f"{env.manifest_sha256[:12]} (same Lean {env.lean_version} and Mathlib pins)")
                row["rss_guard_mb"] = rss_mb
                rep = C.check(it["dir"], sol, cenv, None, timeout, False,
                              work_root or os.path.join(out_dir, "check-work"))
                row["check_secs"] = round(time.time() - t0, 2)
                row["check_peak_rss_mb"] = max(_RSS.peaks or [0])
                row["verdict"] = rep["verdict"]
                comp = rep.get("compile") if isinstance(rep.get("compile"), dict) else {}
                row["compile_secs"] = comp.get("compile_secs")
                row["model_compile_secs"] = sum(i.get("secs", 0) for i in comp.get("imports", []))
                if getattr(_RSS, "memkill", False):
                    row["outcome"] = "MEMKILL"
                elif rep["verdict"] == "PASS":
                    row["outcome"] = "MECH-SOLVED"
                elif rep["verdict"] == "ERROR":
                    row["outcome"] = "MECH-ERROR"
                    row["error"] = (rep.get("error") or [""])[0][:500]
                else:
                    row["outcome"] = "MECH-PROOF-FAIL"
                    row["error_class"] = error_class(rep)
                    row["error"] = " | ".join(rep.get("fail", []) + rep.get("invalid", []))[:800]
                with open(sol + ".check.json", "w", encoding="utf-8") as f:
                    json.dump(rep, f, indent=1, sort_keys=True)
        except Exception as ex:                     # noqa: BLE001 - one row per item, whatever happens
            row["outcome"] = "MECH-ERROR"
            row["error"] = f"{type(ex).__name__}: {ex}"[:800]
            row["traceback"] = traceback.format_exc()[-1500:]
        row["finished_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        with lock:
            with open(results, "a", encoding="utf-8") as f:
                f.write(json.dumps(row, sort_keys=True) + "\n")
            print(f"{row['outcome']:16} {row.get('pool', '')} {it['package_id']} "
                  f"{row.get('check_secs', '')} {row.get('stuck_reason', '') or row.get('error_class', '')}", flush=True)

    os.makedirs(out_dir, exist_ok=True)
    with ThreadPoolExecutor(max_workers=max(1, jobs)) as ex:
        list(ex.map(run, todo))


def summarize(results: str) -> dict:
    with open(results, encoding="utf-8") as f:
        rows = list({r["dir"]: r for r in (json.loads(ln) for ln in f if ln.strip())}.values())
    out: dict = {}
    for r in rows:
        d = out.setdefault(r.get("pool", "?"), {})
        d[r["outcome"]] = d.get(r["outcome"], 0) + 1
    secs = [r["check_secs"] for r in rows if r.get("check_secs") is not None]
    if secs:
        out["check_secs_median"] = statistics.median(secs)
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("solve")
    s.add_argument("package")
    s.add_argument("--out")
    s.add_argument("--json")
    s.add_argument("--rules", default=",".join(RULES))
    b = sub.add_parser("batch")
    b.add_argument("items")
    b.add_argument("--out-dir", required=True)
    b.add_argument("--results")
    b.add_argument("--lean-env")
    b.add_argument("--jobs", type=int, default=1)
    b.add_argument("--timeout", type=int, default=3600)
    b.add_argument("--rss-mb", type=int)
    b.add_argument("--limit", type=int)
    b.add_argument("--no-check", action="store_true")
    b.add_argument("--work-root", help="parent directory for the checker's work directories")
    b.add_argument("--substitute-manifest", action="store_true")
    b.add_argument("--redo-stuck", action="store_true", help="also re-solve items whose last result is MECH-STUCK")
    a = ap.parse_args(argv)
    if a.cmd == "solve":
        rec = solve(a.package, a.out, tuple(a.rules.split(",")))
        text = json.dumps(rec, indent=1, sort_keys=True)
        if a.json:
            with open(a.json, "w", encoding="utf-8") as f:
                f.write(text + "\n")
        print(text)
        return 0 if rec["propagation"] == "DETERMINED" else 1
    if not a.no_check and not a.lean_env:
        ap.error("batch needs --lean-env unless --no-check")
    batch(a.items, a.out_dir, a.lean_env, a.jobs, not a.no_check, a.timeout, a.rss_mb, a.results, a.limit, a.work_root,
          a.substitute_manifest, a.redo_stuck)
    print(json.dumps(summarize(a.results or os.path.join(a.out_dir, "RESULTS.jsonl")), indent=1, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
