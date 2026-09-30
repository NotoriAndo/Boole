"""Decomposition of TOO-LARGE instantiations into packageable sub-component instantiations.

For each TOO-LARGE instantiation ``T(args)`` (and each instantiation whose sizing compile was stopped by
the resource guard) the concrete sub-component instantiations it uses are harvested from the source
(:func:`instantiation.children_of`: ``T``'s parameters, bindings and loop values substituted into its
call sites).  Children are identified by (prime, template content, parameter values)
(:func:`content.instance_key`).  A child equal to an existing package is only mapped to it.  New children
are sized; a child above the size policy is decomposed in turn (up to :data:`MAX_DEPTH` levels).  Among
the harvested instantiations of one template content that fit the size policy, one is packaged: the
largest (the selection rule of the ledger tiers), and the others are recorded as its variants.  Every
parent -> child edge is recorded.  No compositional statement is made: each package states DET of the
child alone.
"""
from __future__ import annotations

import re

from . import circom_source as cs

MAX_DEPTH = 4
RULE = "decomposition"


def parse_call(call: str) -> tuple[str, list[str]]:
    """``T(a, f(b), [1,2])`` -> (``T``, [``a``, ``f(b)``, ``[1,2]``])."""
    m = re.match(r"\s*(" + cs._IDENT + r")\s*\(", call)
    if not m:
        raise ValueError(f"not a template call: {call!r}")
    close = cs.match_bracket(call, m.end() - 1)
    if close < 0:
        raise ValueError(f"unbalanced call: {call!r}")
    return m.group(1), [a for a in cs.split_top_level(call[m.end():close]) if a]


def _neg_text(s: str) -> tuple:
    return tuple(-ord(ch) for ch in s)


def select_variants(nodes: dict[str, dict], max_constraints: int) -> dict[str, str]:
    """Node key -> key of the node packaged for its template content.

    ``nodes``: key -> {"group": (prime, content), "constraints": int | None, "wires": int, "call": str}.  Only
    sized nodes within ``max_constraints`` take part; per group the largest constraint count wins (ties:
    fewer wires, then the call text), as in the driver's tier selection."""
    groups: dict[tuple, list[str]] = {}
    for key, n in nodes.items():
        c = n.get("constraints")
        if c is not None and c <= max_constraints:
            groups.setdefault(n["group"], []).append(key)
    out = {}
    for keys in groups.values():
        best = max(keys, key=lambda k: (nodes[k]["constraints"], -nodes[k]["wires"], _neg_text(nodes[k]["call"])))
        for k in keys:
            out[k] = best
    return out


def resolution(key: str, nodes: dict[str, dict], existing: dict[str, str], chosen: dict[str, str],
               packaged: dict[str, dict], max_constraints: int) -> dict:
    """How a harvested child ended: an existing package, a new package, a variant of a new package, too large
    (decomposed further when the depth allows), not compilable, or not sized (depth limit)."""
    if key in existing:
        return {"result": "existing-package", "package_id": existing[key]}
    n = nodes.get(key, {})
    if key in chosen:
        pk = packaged.get(chosen[key])
        if pk is None:
            return {"result": "not-packaged", "reason": "packaging did not produce a record"}
        res = {"result": "packaged" if chosen[key] == key else "variant-of", "package_id": pk["package_id"],
               "status": pk["status"]}
        return res
    if n.get("constraints") is not None and n["constraints"] > max_constraints:
        return {"result": "too-large", "constraints": n["constraints"], "decomposed": bool(n.get("expanded"))}
    if n.get("error"):
        return {"result": "not-compiled", "reason": n["error"][:300]}
    return {"result": "not-sized", "reason": n.get("skipped", "beyond the decomposition depth")}
