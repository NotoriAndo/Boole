"""Instantiation: grounding a ZoKrates function's generic constants and building its wrapper program.

Every ledger row names one function declaration: either the file's own ``main`` (a complete ZoKrates
program already, compiled as-is when it has no generics) or a named declaration, possibly one of
several overloads of the same name disambiguated by its source line (``name@L<line>``, e.g. the
stdlib's ``cast`` conversions).

A ZoKrates function's only *shape* parameters are its generic constants (``def f<N, P>(...)``);
everything else (the actual field/bool/uN/array/struct values ``compute-witness`` is given) is sampled
at witness time (:mod:`zokrates_witness`), not fixed here.  So instantiation only has to ground the
generics, in this tier order (first tier with a compiling candidate wins):

``parameter-free``  the function has no generics.
``repo-call``       a call site of the function elsewhere in the same file grounds every generic,
                    either an explicit turbofish (``name::<1, 2>(...)``) or the length of an
                    array-literal argument at a parameter position whose type names the generic
                    (:func:`zokrates_source.generics_from_call`).
``probed``          no call site exists: small values (:data:`PROBE_VALUES`), the same value for every
                    generic first, respecting the function's own ``assert`` bounds when it states any.

Within a tier every candidate is compiled and the largest within the size policy is kept (the smallest
is kept, and the record is TOO-LARGE, if none fits).  A counterexample on a probed record is not a
finding.  The wrapper imports the target declaration under a fixed alias and any struct type its
(generic-substituted) signature names, copying the exact import line the original file used for it, and
calls it with the chosen generics; both ZoKrates syntax eras are supported (the ``from "path" import
Name;`` / ``name::<...>`` syntax of >= 0.8.0, and the older ``import "path" as Name;`` syntax with no
generics that every pre-0.8 population row happens to need, since none of them are themselves generic).
"""
from __future__ import annotations

import itertools
import re
from dataclasses import dataclass, field

from . import zokrates_source as S

PROBE_VALUES = (1, 2, 3, 4, 8, 16, 32, 64)
MAX_CANDIDATES_PER_TIER = 8
TIERS = ["parameter-free", "repo-call", "probed"]


class InstantiationError(ValueError):
    pass


def find_symbol(text: str, symbol: str) -> S.FuncDecl:
    """The function a ledger ``symbol`` names (``name`` or ``name@L<line>`` for an overload)."""
    if "@L" in symbol:
        name, line_s = symbol.rsplit("@L", 1)
        decl = S.find_def(text, name, line=int(line_s))
    else:
        name, decl = symbol, S.find_def(text, symbol)
    if decl is None:
        raise InstantiationError(f"no (unique) declaration of {symbol!r}")
    return decl


@dataclass
class Candidate:
    tier: str
    values: dict[str, int]          # generic name -> concrete value ({} when the function has none)
    provenance: str


def candidates_for(decl: S.FuncDecl, text: str) -> list[tuple[str, list[Candidate]]]:
    """``[(tier, candidates)]`` for every non-empty tier, in :data:`TIERS` order."""
    if not decl.generics:
        return [("parameter-free", [Candidate("parameter-free", {}, f"{decl.name} has no generic parameters")])]
    out: dict[str, list[Candidate]] = {"repo-call": [], "probed": []}
    seen = set()
    for call in S.find_calls(text, decl.name):
        vals = S.generics_from_call(decl, call)
        if vals is None:
            continue
        key = tuple(vals[g] for g in decl.generics)
        if key in seen:
            continue
        seen.add(key)
        src = (f"turbofish {decl.name}::<{', '.join(call.turbofish)}>(...)" if call.turbofish is not None
               else f"call site {decl.name}(...) with an array-literal argument")
        out["repo-call"].append(Candidate("repo-call", vals, src))
        if len(out["repo-call"]) >= MAX_CANDIDATES_PER_TIER:
            break
    if not out["repo-call"]:
        for vals, note in _linear_relation_candidates(decl):
            out["probed"].append(Candidate("probed", vals, note))
            if len(out["probed"]) >= MAX_CANDIDATES_PER_TIER:
                break
    if not out["repo-call"] and not out["probed"]:
        bounds = S.assert_bounds(decl.body, decl.generics)
        for v in PROBE_VALUES:
            if all(v >= 1 and (bounds[g][0] is None or v >= bounds[g][0]) and (bounds[g][1] is None or v <= bounds[g][1])
                   for g in decl.generics):
                note = f"probe values {list(PROBE_VALUES)}"
                if any(lo is not None or hi is not None for lo, hi in bounds.values()):
                    note += f" within the function's own assert bounds {bounds}"
                out["probed"].append(Candidate("probed", {g: v for g in decl.generics}, note))
            if len(out["probed"]) >= MAX_CANDIDATES_PER_TIER:
                break
    return [(t, out[t]) for t in ("repo-call", "probed") if out[t]]


_EQ_MUL_RE = re.compile(r"assert\s*\(\s*([A-Za-z_]\w*)\s*==\s*(\d+)\s*\*\s*([A-Za-z_]\w*)\s*\)")


def _linear_relation_candidates(decl: S.FuncDecl) -> list[tuple[dict[str, int], str]]:
    """When the function's own ``assert(G1 == K * G2)`` states an exact multiplicative relation
    between two generics (the stdlib's bit-width casts, e.g. ``assert(N == 8 * P)``), ground the free
    one (``G2``) at small values and derive the other, instead of probing every generic independently
    (which would never by chance land on the exact relation)."""
    out = []
    for m in _EQ_MUL_RE.finditer(decl.body[:4000]):
        g1, k, g2 = m.group(1), int(m.group(2)), m.group(3)
        if g1 not in decl.generics or g2 not in decl.generics or k < 1:
            continue
        for v in PROBE_VALUES:
            vals = {g2: v, g1: v * k}
            if len(vals) == len(decl.generics):
                out.append((vals, f"the function's own assert({g1} == {k} * {g2}): {g2} probed, {g1} derived"))
            if len(out) >= MAX_CANDIDATES_PER_TIER:
                return out
    return out


# ------------------------------------------------------------------------------------------ wrapper source

MODIFIERS = {"private", "public", "mut"}
_TYPE_IDENT_RE = re.compile(r"\b([A-Z]\w*)\b")


def _substituted_params(decl: S.FuncDecl, env: dict[str, str]) -> list[tuple[str, str]]:
    return [(f"a{i}", S.substitute_identifiers(p.type, env)) for i, p in enumerate(decl.params)]


def wrapper_source(decl: S.FuncDecl, candidate: Candidate, module_path: str, imports: list[S.Import],
                   style: str) -> str:
    """A wrapper ``.zok`` program whose ``main`` calls ``decl`` with ``candidate``'s generics, in
    ``style`` (``"brace"``: >= 0.8.0 syntax; ``"colon"``: the pre-0.8 syntax every legacy-pinned
    population row needs, which has no generics and no ``mut``, so only the ``parameter-free`` tier
    reaches it)."""
    env = {g: str(v) for g, v in candidate.values.items()}
    params = _substituted_params(decl, env)
    ret = S.substitute_identifiers(decl.ret, env)
    alias = "target"
    needed_types = {m.group(1) for _n, ty in params for m in _TYPE_IDENT_RE.finditer(ty)}
    needed_types |= set(_TYPE_IDENT_RE.findall(ret))
    by_name = {orig: imp for imp in imports for orig, _alias in imp.names}
    extra_import_lines = []
    for t in sorted(needed_types):
        imp = by_name.get(t)
        if imp is not None:
            extra_import_lines.append(_render_import(imp, style))
    args = ", ".join(n for n, _ty in params)
    if style == "brace":
        lines = [f'from "{module_path}" import {decl.name} as {alias};', *extra_import_lines, ""]
        sig = ", ".join(f"{ty} {n}" for n, ty in params)
        call_generics = f"::<{', '.join(env[g] for g in decl.generics)}>" if candidate.values else ""
        lines += [f"def main({sig}) -> {ret} {{", f"    return {alias}{call_generics}({args});", "}", ""]
    else:
        lines = [f'import "{module_path}" as {alias};', *extra_import_lines, ""]
        sig = ", ".join(f"{ty} {n}" for n, ty in params)
        lines += [f"def main({sig}) -> {ret or '()'}:", f"    return {alias}({args})", ""]
    return "\n".join(lines)


def _render_import(imp: S.Import, style: str) -> str:
    if len(imp.names) == 1 and imp.names[0][0] == "main":
        return f'import "{imp.module}" as {imp.names[0][1]};'
    items = ", ".join(n if n == a else f"{n} as {a}" for n, a in imp.names)
    return f'from "{imp.module}" import {items};'
