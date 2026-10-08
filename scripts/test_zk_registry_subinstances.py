#!/usr/bin/env python3
"""Sub-component instances located in an unoptimized parent compile (wave RT-C2 decomposition): the ``.sym``
hierarchy, wire keys (aliases included), and the four matching rules against a standalone compile.

Synthetic constraint systems in the shape circom writes (no compiler needed).  The child ``Sq()`` has input ``x``,
output ``y`` and an internal signal ``t``: ``t = x * x``, ``y = t * x``."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import r1cs as R               # noqa: E402
from zk_registry import subinstances as S       # noqa: E402

P = R.BN254_SCALAR
NEG = P - 1


def r1cs(n_wires: int, n_out: int, n_in: int, cons: list) -> R.R1cs:
    return R.R1cs(P, 32, n_wires, n_out, n_in, 0, n_wires, cons, list(range(n_wires)))


def syms(rows: list[tuple[int, int, str]]) -> list[R.SymEntry]:
    return [R.SymEntry(i + 1, w, c, nm) for i, (w, c, nm) in enumerate(rows)]


def standalone_sq():
    # wires: 1 y (out), 2 x (in), 3 t
    r = r1cs(4, 1, 1, [([(2, 1)], [(2, 1)], [(3, 1)]), ([(3, 1)], [(2, 1)], [(1, 1)])])
    return r, syms([(1, 0, "main.y"), (2, 0, "main.x"), (3, 0, "main.t")])


def parent(extra: list | None = None, alias_t: bool = False, outside_t: bool = False, child_k: int = 1):
    """Parent ``P()``: input a, output o; ``c = Sq(); c.x <== a; o <== c.y + 1`` (wires: 1 o, 2 a, 3 c.y, 4 c.x,
    5 c.t).  ``child_k``: the coefficient in the child's first constraint (a different child when != 1)."""
    cons = [([], [], [(4, 1), (2, NEG)]),                        # c.x = a (parent)
            ([(4, child_k)], [(4, 1)], [(5, 1)]),                # c.t = c.x * c.x (child)
            ([(5, 1)], [(4, 1)], [(3, 1)]),                      # c.y = c.t * c.x (child)
            ([], [], [(1, 1), (3, NEG), (0, NEG)])]              # o = c.y + 1 (parent)
    cons += extra or []
    if outside_t:
        cons.append(([(5, 1)], [(2, 1)], [(1, 1)]))              # the parent reads the internal c.t
    rows = [(1, 1, "main.o"), (2, 1, "main.a"), (3, 0, "main.c.y"), (4, 0, "main.c.x"), (5, 0, "main.c.t")]
    if alias_t:
        rows.append((5, 1, "main.u"))                            # c.t also named by the parent (circom 1 alias)
    r = r1cs(6, 1, 1, cons)
    return r, syms(rows)


class SubInstanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fp = S.fingerprint(*standalone_sq())

    def match(self, **kw) -> str | None:
        r, sy = parent(**kw)
        tree = S.build_tree(r, sy)
        comps = S.candidate_components(tree, self.fp.n_wires)
        self.assertEqual([S.path_text(tree.paths[c]) for c in comps], ["main.c"])
        return S.matches(tree, comps[0], self.fp)

    def test_split_path(self) -> None:
        self.assertEqual(S.split_path("main.c[2].d.out[1]"), ["main", "c[2]", "d", "out[1]"])
        self.assertEqual(S.split_path("main.x[a.b]"), ["main", "x[a.b]"])

    def test_tree_and_keys(self) -> None:
        r, sy = parent()
        tree = S.build_tree(r, sy)
        self.assertEqual([S.path_text(p) for p in tree.paths], ["main", "main.c"])
        self.assertEqual(sorted(S.wire_keys(tree, 1).values()), [("main.t",), ("main.x",), ("main.y",)])
        self.assertEqual(tree.comp_cons[1], [1, 2])                         # the child's own constraints
        self.assertEqual(sorted(tree.comp_cons[0]), [0, 1, 2, 3])           # main contains everything
        self.assertEqual(S.instance_groups(tree, sy), {1: [0], 0: [1]})

    def test_located(self) -> None:
        self.assertIsNone(self.match())

    def test_parent_written_constraint_on_inputs_is_allowed(self) -> None:
        self.assertIsNone(self.match(extra=[([(4, 1)], [(4, 1)], [(4, 1)])]))      # c.x boolean, written by the parent

    def test_different_child_is_not_located(self) -> None:
        self.assertEqual(self.match(child_k=2), "constraints")

    def test_extra_constraint_on_an_internal_signal(self) -> None:
        self.assertEqual(self.match(extra=[([(5, 1)], [(5, 1)], [(5, 1)])]), "extra constraint on an internal signal")

    def test_internal_signal_read_by_the_parent(self) -> None:
        self.assertEqual(self.match(outside_t=True), "internal signal in a constraint outside the instance")
        self.assertEqual(self.match(alias_t=True), "internal signal named outside the instance")

    def test_names_and_sizes(self) -> None:
        r, sy = parent()
        tree = S.build_tree(r, sy)
        other = S.fingerprint(r1cs(4, 1, 1, standalone_sq()[0].constraints),
                              syms([(1, 0, "main.y"), (2, 0, "main.x"), (3, 0, "main.s")]))
        self.assertEqual(S.matches(tree, 1, other), "names")
        self.assertEqual(S.matches(tree, 0, self.fp), "wires")

    def test_circom1_aliases_are_part_of_the_key(self) -> None:
        # circom 1 merges ``c.x <== a`` into one wire named main.a and main.c.x; the standalone compile names it x
        r = r1cs(5, 1, 1, [([(2, 1)], [(2, 1)], [(4, 1)]), ([(4, 1)], [(2, 1)], [(3, 1)]),
                           ([], [], [(1, 1), (3, NEG), (0, NEG)])])
        sy = syms([(1, 1, "main.o"), (2, 1, "main.a"), (2, 0, "main.c.x"), (3, 0, "main.c.y"), (4, 0, "main.c.t")])
        tree = S.build_tree(r, sy)
        self.assertEqual(S.wire_keys(tree, 1)[2], ("main.x",))
        self.assertIsNone(S.matches(tree, 1, self.fp))
        self.assertEqual(S.instance_groups(tree, sy), {1: [0], 0: [1]})


if __name__ == "__main__":
    unittest.main()
