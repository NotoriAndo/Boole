#!/usr/bin/env python3
"""circom source scanner and instantiation-rule tests on a small fixture repository."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_det as D           # noqa: E402
from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import instantiation as I        # noqa: E402

REPO = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-repo"


def scan():
    files = cs.scan_repo(str(REPO), cs.list_circom_files(str(REPO)))
    scope = [p for p in files if p.startswith("circuits/")]
    return files, scope


class ScannerTests(unittest.TestCase):
    def test_comments_are_blanked_but_offsets_and_strings_survive(self) -> None:
        src = 'a // x "q"\n/* template T(n) {\n} */ include "p//q.circom";'
        clean = cs.strip_comments(src)
        self.assertEqual(len(clean), len(src))
        self.assertEqual(clean.count("\n"), src.count("\n"))
        self.assertNotIn("template", clean)
        self.assertIn('"p//q.circom"', clean)

    def test_templates_params_and_signals(self) -> None:
        files, _ = scan()
        gates = files["circuits/gates.circom"]
        names = [t.name for t in gates.templates]
        self.assertEqual(names, ["Leaf", "Bits", "Pair", "Inner", "Outer", "Table", "Orphan"])  # not `Commented`
        table = next(t for t in gates.templates if t.name == "Table")
        self.assertEqual(table.params, ["m", "T"])
        pair = next(t for t in gates.templates if t.name == "Pair")
        decls = [(d.direction, d.name, d.dims) for d in cs.signal_declarations(pair.body)]
        self.assertEqual(decls, [("input", "a", []), ("input", "b", []), ("output", "s", [])])
        outer = next(t for t in gates.templates if t.name == "Outer")
        self.assertIn(("output", "y", ["width"]), [(d.direction, d.name, d.dims) for d in cs.signal_declarations(outer.body)])

    def test_mains_includes_and_resolution(self) -> None:
        files, _ = scan()
        bits = files["test/circuits/bits_test.circom"]
        self.assertTrue(bits.is_harness)
        self.assertEqual([(m.template, m.args, m.public) for m in bits.mains], [("Bits", ["4"], "{public [x]}")])
        self.assertEqual(files["circuits/top.circom"].includes, ["circuits/gates.circom", "circuits/sub/needs_context.circom"])
        self.assertIsNotNone(cs.resolve_template(files, "circuits/top.circom", "Leaf"))
        self.assertIsNone(cs.resolve_template(files, "circuits/sub/needs_context.circom", "Leaf"))

    def test_literal_arguments(self) -> None:
        self.assertTrue(cs.is_literal_arg("250*2"))
        self.assertTrue(cs.is_literal_arg("-1"))
        self.assertFalse(cs.is_literal_arg("n+1"))
        self.assertFalse(cs.is_literal_arg("()"))
        self.assertEqual(cs.split_top_level("a, f(b, c), [d, e]"), ["a", "f(b, c)", "[d, e]"])


class InstantiationRuleTests(unittest.TestCase):
    def test_rule_tiers_for_the_fixture_repository(self) -> None:
        files, scope = scan()
        plans = {p.template.name: p for p in I.plan_templates(files, scope, "toy/repo")}
        first = {n: (p.tiers_in_order()[0][0], [c.args for c in p.tiers_in_order()[0][1]]) if p.candidates else None
                 for n, p in plans.items()}
        self.assertEqual(first, {
            "Leaf": ("repo-internal", [("2",)]),          # Leaf(3) in needs_context.circom is not resolvable
            "Bits": ("repo-main", [("4",)]),
            "Pair": ("parameter-free", [()]),
            "Inner": ("repo-derived", [("3+1",)]),       # Top -> Outer(3) -> Inner(width), width = n + 1
            "Outer": ("repo-internal", [("3",)]),
            "Table": ("repo-test", [("2", "[5,7]")]),     # wrapper var `t` substituted
            "Orphan": None,
            "UsesLeaf": ("parameter-free", [()]),
            "Top": ("parameter-free", [()]),
        })
        self.assertIn("no repository-grounded instantiation", plans["Orphan"].uninstantiable_reason)
        self.assertEqual(plans["Bits"].candidates["repo-main"][0].provenance,
                         ["test/circuits/bits_test.circom:4 component main = Bits(4)"])

    def test_constant_bindings_exclude_loop_and_reassigned_vars(self) -> None:
        body = "var a = 3; var b = a + 1; var c = 0; c += 2; for (var i = 0; i < 2; i++) { var d = i; }"
        self.assertEqual(I.constant_bindings(body), {"a": "3", "b": "a + 1"})

    def test_resolve_args_substitutes_until_closed(self) -> None:
        env = {"n": "3", "width": "n + 1", "C": "TABLE(n)"}
        self.assertEqual(I.resolve_args(["width", "C", "7"], env, {"TABLE"}), ("3+1", "TABLE(3)", "7"))
        self.assertIsNone(I.resolve_args(["i * width"], env, set()))

    def test_include_contexts_prefer_the_own_file_then_includers(self) -> None:
        files, _ = scan()
        self.assertEqual(D.include_contexts(files, "circuits/sub/needs_context.circom"),
                         ["circuits/sub/needs_context.circom", "circuits/top.circom"])

    def test_main_source(self) -> None:
        self.assertEqual(I.main_source("circuits/gates.circom", "Table", ("2", "[5,7]")),
                         'pragma circom 2.0.0;\ninclude "circuits/gates.circom";\ncomponent main = Table(2, [5,7]);\n')


if __name__ == "__main__":
    unittest.main()
