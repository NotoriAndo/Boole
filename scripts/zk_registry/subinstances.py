"""Sub-component instances of an unoptimized circom compile, located by the ``.sym`` hierarchy (wave RT-C2).

A parent instantiation compiled without simplification (circom 2 ``--O0``, circom 1 ``-f``) keeps one wire per
signal (circom 1 merges signals connected by ``<==`` into one wire with several names).  The ``.sym`` names give the
component hierarchy: ``main.c.q[1].out[0]`` is signal ``out[0]`` of component ``main.c.q[1]``.  For a component X:

* its **wires** are the wires with a name under X; the **key** of such a wire is the sorted tuple of its names under
  X, rewritten relative to X (``main.c.q[1].out[0]`` -> ``main.out[0]`` for X = ``main.c.q[1]``);
* its **constraints** S(X) are the constraints whose every wire (wire 0 aside) is a wire of X, renamed by keys
  (terms sorted, the two factors of the product unordered).

A standalone compile of a concrete instantiation ``T(args)`` (same compiler, flags and prime) **matches** X when:

1. the keys of its wires equal the keys of X's wires (a bijection of wires, names and aliasing included);
2. its constraints, renamed by keys, are a sub-multiset of S(X);
3. every constraint of S(X) beyond them mentions only input / output signals of the main component of ``T(args)``
   (constraints the parent writes over the instance's inputs and outputs, e.g. ``c.in <== 5``);
4. every internal (non-input/output) wire of X has no name outside X and occurs in no constraint outside S(X).

Then X's sub-circuit in the parent is the standalone circuit (renamed) and the parent touches it only through its
inputs and outputs: replacing it by a circuit with the same input-output relation preserves the parent's relation.
"""
from __future__ import annotations

import collections
import hashlib
from dataclasses import dataclass, field

from . import r1cs as R


def split_path(name: str) -> list[str]:
    """``main.c[2].d.out[1]`` -> [``main``, ``c[2]``, ``d``, ``out[1]``] (dots inside brackets are kept)."""
    out, depth, cur = [], 0, []
    for ch in name:
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
        if ch == "." and depth == 0:
            out.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    out.append("".join(cur))
    return out


@dataclass
class Tree:
    """Component hierarchy of one compile.  Component 0 is ``main``."""
    r: R.R1cs
    paths: list[tuple[str, ...]]                      # component -> path (("main",), ("main", "c"), ...)
    parent: list[int]
    names: dict[int, list[tuple[int, str]]]           # wire -> [(component, local signal name)]
    comp_wires: list[set[int]] = field(default_factory=list)   # component -> wires with a name under it
    comp_cons: list[list[int]] = field(default_factory=list)   # component -> constraints of S(component)
    io_wires: set[int] = field(default_factory=set)            # main inputs and outputs (header ranges)
    children: list[list[int]] = field(default_factory=list)
    wire_cons: dict[int, list[int]] = field(default_factory=dict)   # wire -> constraints mentioning it


def build_tree(r: R.R1cs, syms: list[R.SymEntry]) -> Tree:
    index: dict[tuple[str, ...], int] = {("main",): 0}
    paths, parent = [("main",)], [-1]

    def comp(path: tuple[str, ...]) -> int:
        if path not in index:
            up = comp(path[:-1])
            index[path] = len(paths)
            paths.append(path)
            parent.append(up)
        return index[path]

    names: dict[int, list[tuple[int, str]]] = collections.defaultdict(list)
    for e in syms:
        if e.wire < 1 or e.wire >= r.n_wires:
            continue
        seg = split_path(e.name)
        if seg[0] != "main" or len(seg) < 2:
            raise ValueError(f"signal {e.name!r} outside the main component")
        names[e.wire].append((comp(tuple(seg[:-1])), seg[-1]))
    n = len(paths)
    anc = [None] * n                                   # component -> its ancestors-or-self

    def ancestors(c: int) -> frozenset:
        if anc[c] is None:
            anc[c] = frozenset([c]) | (ancestors(parent[c]) if parent[c] >= 0 else frozenset())
        return anc[c]
    wire_anc: dict[int, frozenset] = {}
    comp_wires: list[set[int]] = [set() for _ in range(n)]
    for w, lst in names.items():
        a = frozenset().union(*(ancestors(c) for c, _ in lst))
        wire_anc[w] = a
        for c in a:
            comp_wires[c].add(w)
    comp_cons: list[list[int]] = [[] for _ in range(n)]
    wire_cons: dict[int, list[int]] = collections.defaultdict(list)
    for k, con in enumerate(r.constraints):
        q = None
        for w in {w for lc in con for w, coef in lc if w != 0 and coef}:
            wire_cons[w].append(k)
            q = wire_anc.get(w, frozenset()) if q is None else q & wire_anc.get(w, frozenset())
        for c in (q if q is not None else (0,)):
            comp_cons[c].append(k)
    io = set(range(1, 1 + r.n_pub_out + r.n_pub_in + r.n_prv_in))
    children: list[list[int]] = [[] for _ in range(n)]
    for c, p in enumerate(parent):
        if p >= 0:
            children[p].append(c)
    return Tree(r, paths, parent, dict(names), comp_wires, comp_cons, io, children, dict(wire_cons))


def path_text(path: tuple[str, ...]) -> str:
    return ".".join(path)


def _under(tree: Tree, x: int) -> frozenset:
    """Components in the subtree of x."""
    out, todo = set(), [x]
    while todo:
        c = todo.pop()
        out.add(c)
        todo += tree.children[c]
    return frozenset(out)


def wire_keys(tree: Tree, x: int, under: frozenset | None = None) -> dict[int, tuple[str, ...]]:
    """Wire -> key (its names under component x, relative to x) for the wires of x."""
    under = under if under is not None else _under(tree, x)
    depth = len(tree.paths[x])
    out = {}
    for w in tree.comp_wires[x]:
        rel = sorted(".".join(("main",) + tree.paths[c][depth:] + (local,)) for c, local in tree.names[w]
                     if c in under)
        out[w] = tuple(rel)
    return out


def _lc(lc, key: dict[int, tuple[str, ...]]) -> tuple:
    return tuple(sorted(((("one",) if w == 0 else key[w]), int(c)) for w, c in lc if c))


def renamed_constraints(tree: Tree, x: int, key: dict[int, tuple[str, ...]]) -> collections.Counter:
    out: collections.Counter = collections.Counter()
    for k in tree.comp_cons[x]:
        a, b, c = tree.r.constraints[k]
        la, lb = _lc(a, key), _lc(b, key)
        out[(min(la, lb), max(la, lb), _lc(c, key))] += 1
    return out


@dataclass
class Fingerprint:
    """A standalone compile of a concrete instantiation, ready for matching."""
    n_wires: int                                       # wires with a name (wire 0 aside)
    keys_sha256: str
    keys: frozenset
    io_keys: frozenset
    constraints: collections.Counter
    prime: int


def fingerprint(r: R.R1cs, syms: list[R.SymEntry]) -> Fingerprint:
    tree = build_tree(r, syms)
    key = wire_keys(tree, 0)
    keys = sorted(key.values())
    return Fingerprint(len(key), hashlib.sha256(repr(keys).encode()).hexdigest(), frozenset(keys),
                       frozenset(key[w] for w in tree.io_wires if w in key), renamed_constraints(tree, 0, key),
                       r.prime)


def matches(tree: Tree, x: int, fp: Fingerprint, under: frozenset | None = None) -> str | None:
    """None when the standalone compile ``fp`` matches component x of the parent (rules 1-4 of the module
    docstring); otherwise the first rule that fails."""
    if tree.r.prime != fp.prime:
        return "prime"
    if len(tree.comp_wires[x]) != fp.n_wires:
        return "wires"
    under = under if under is not None else _under(tree, x)
    key = wire_keys(tree, x, under)
    keys = sorted(key.values())
    if hashlib.sha256(repr(keys).encode()).hexdigest() != fp.keys_sha256:
        return "names"
    sx = renamed_constraints(tree, x, key)
    if any(sx[c] < n for c, n in fp.constraints.items()):
        return "constraints"
    extra = sx - fp.constraints
    for (la, lb, lc), _ in extra.items():
        for lc_ in (la, lb, lc):
            for k, _ in lc_:
                if k != ("one",) and k not in fp.io_keys:
                    return "extra constraint on an internal signal"
    internal = {w for w, k in key.items() if k not in fp.io_keys}
    for w in internal:
        if any(c not in under for c, _ in tree.names[w]):
            return "internal signal named outside the instance"
    own = set(tree.comp_cons[x])
    for w in internal:
        if any(k not in own for k in tree.wire_cons.get(w, ())):
            return "internal signal in a constraint outside the instance"
    return None


def candidate_components(tree: Tree, n_wires: int) -> list[int]:
    """Components (main aside) whose number of wires is ``n_wires``."""
    return [c for c in range(1, len(tree.paths)) if len(tree.comp_wires[c]) == n_wires]


def instance_groups(tree: Tree, syms: list[R.SymEntry]) -> dict[int, list[int]]:
    """circom 2: components grouped by template instance (the ``.sym`` component column), which circom assigns per
    (template, parameter values); a group's components have identical sub-circuits.  Empty for circom 1, whose
    column numbers component instances."""
    ids: dict[int, set] = collections.defaultdict(set)
    for e in syms:
        if 0 < e.wire < tree.r.n_wires:
            seg = split_path(e.name)
            ids[e.component].add(tuple(seg[:-1]))
    index = {p: i for i, p in enumerate(tree.paths)}
    out: dict[int, list[int]] = {}
    for cid, ps in ids.items():
        comps = sorted({index[p] for p in ps if p in index})
        if len(comps) != len(ps):
            return {}
        out[cid] = comps
    return out
