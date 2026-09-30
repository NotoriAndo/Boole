#!/usr/bin/env python3
"""Parameter inference: compile-time evaluation, loops, instantiation chains, config mains, probed tier (offline)."""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_eval as V          # noqa: E402
from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import instantiation as I        # noqa: E402


def write(root: str, rel: str, text: str) -> None:
    p = Path(root, rel)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


def plan(root: str, dependencies: bool = False, lib_dirs=None) -> dict:
    files = cs.scan_repo(root, cs.list_circom_files(root), lib_dirs, dependencies=dependencies)
    scope = [p for p, sf in files.items() if not sf.dependency]
    configs = cs.scan_config_mains(root, files)
    return {p.template.name: p for p in I.plan_templates(files, scope, "t/r", config_mains=configs)}


def first(p: I.TemplatePlan):
    tiers = p.tiers_in_order()
    return (tiers[0][0], [c.args for c in tiers[0][1]]) if tiers else None


class EvalTests(unittest.TestCase):
    def test_integer_expressions(self) -> None:
        cases = {"3": 3, "0x10": 16, "2 + 3 * 4": 14, "(2 + 3) * 4": 20, "7 \\ 2": 3, "7 % 4": 3, "2 ** 10": 1024,
                 "1 << 5": 32, "256 >> 4": 16, "n + 1": 6, "n > 4 ? n : 0": 5, "n < 4 ? 1 : 2": 2, "-n + 10": 5,
                 "n == 5 && n != 4": 1, "!(n > 2)": 0, "12 / 4": 3, "2 ** 3 ** 2": 512, "6 & 3 | 8": 10}
        for expr, want in cases.items():
            self.assertEqual(V.eval_int(expr, {"n": 5}), want, expr)
        for expr in ("7 / 2", "f(3)", "m + 1", "[1, 2]", "n +", "1 ? 2", "~1", "\"s\""):
            self.assertIsNone(V.eval_int(expr, {"n": 5}), expr)

    def test_loops_and_their_values(self) -> None:
        body = ("var s = 0; for (var i = 0; i < n; i++) { c[i] = T(i + 1); }\n"
                "for (var j = 1; j <= n - 1; j += 2) s += j;\n"
                "for (var k = n; k > 0; k--) { }\n"
                "for (var q = 0; f(q); q++) { }\n")
        loops = V.parse_loops(body)
        self.assertEqual([lp.var for lp in loops], ["i", "j", "k", None])
        self.assertEqual(V.loop_values(loops[0], {"n": 3}, 100), [0, 1, 2])
        self.assertEqual(V.loop_values(loops[1], {"n": 6}, 100), [1, 3, 5])
        self.assertEqual(V.loop_values(loops[2], {"n": 3}, 100), [3, 2, 1])
        self.assertEqual(V.loop_values(loops[0], {"n": 300}, 4), [0, 1, 2, 3])
        self.assertIsNone(V.loop_values(loops[3], {}, 10))
        self.assertIsNone(V.loop_values(loops[0], {}, 10))            # unknown bound
        pos = body.index("T(")
        self.assertEqual([lp.var for lp in V.enclosing_loops(loops, pos)], ["i"])


class ResolveTests(unittest.TestCase):
    def test_indexing_a_substituted_array_literal_is_folded(self) -> None:
        env = {"N_ROUNDS_P": "[56, 57, 56, 60]", "t": "3", "nRoundsP": "N_ROUNDS_P[t - 2]"}
        self.assertEqual(I.resolve_args(["nRoundsP", "(8\\2)*t + nRoundsP"], env, set()), ("57", "(8\\2)*3+57"))
        # an index that does not evaluate is left to circom (and the argument is still closed)
        self.assertEqual(I.resolve_args(["N_ROUNDS_P[f(t)]"], env, {"f"}), ("[56,57,56,60][f(3)]",))


class ChainTests(unittest.TestCase):
    def test_chains_through_parametric_test_wrappers_and_loops(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "c/lib.circom", "pragma circom 2.0.0;\n"
                  "template T(k) { signal input a; signal output b; b <== a * k; }\n"
                  "template U(n) {\n  signal input x; signal output y[n];\n  component c[n];\n"
                  "  for (var i = 0; i < n; i++) {\n    var k = i * 3;\n    c[i] = T(k + 1);\n"
                  "    c[i].a <== x; y[i] <== c[i].b;\n  }\n}\n")
            write(tmp, "test/w.circom", 'pragma circom 2.0.0;\ninclude "../c/lib.circom";\n'
                  "template W(m) { signal input x; signal output y[m * 2]; component u = U(m * 2);\n"
                  "  u.x <== x; for (var i = 0; i < m * 2; i++) { y[i] <== u.y[i]; } }\n"
                  "component main = W(2);\n")
            plans = plan(tmp)
            self.assertEqual(first(plans["W"]), ("repo-main", [("2",)]))
            self.assertEqual(first(plans["U"]), ("repo-derived", [("2*2",)]))
            tier, cands = plans["T"].tiers_in_order()[0]
            self.assertEqual(tier, "repo-derived")
            self.assertEqual([c.args for c in cands], [("(0*3)+1",), ("(1*3)+1",), ("(2*3)+1",), ("(3*3)+1",)])
            self.assertIn("[i=2]", cands[2].provenance[0])

    def test_dependency_templates_resolve_but_stay_out_of_scope(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "node_modules/lib/l.circom", "pragma circom 2.0.0;\ntemplate L(n) { signal input a[n]; }\n")
            write(tmp, "c/x.circom", 'pragma circom 2.0.0;\ninclude "l.circom";\n'
                  "template X() { component l = L(4); }\n")
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp), ["node_modules/lib"], dependencies=True)
            self.assertTrue(files["node_modules/lib/l.circom"].dependency)
            self.assertFalse(files["c/x.circom"].dependency)
            self.assertIsNotNone(cs.resolve_template(files, "c/x.circom", "L"))
            plans = plan(tmp, dependencies=True, lib_dirs=["node_modules/lib"])
            self.assertEqual(sorted(plans), ["X"])


class ConfigMainTests(unittest.TestCase):
    def test_circomkit_configs_and_js_test_mains(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "pkg/circom/utils/sum.circom", "pragma circom 2.0.0;\ntemplate Sum(n) { signal input a[n]; }\n")
            write(tmp, "pkg/circom/tree.circom", "pragma circom 2.0.0;\ntemplate Tree(d, w) { signal input a; }\n")
            write(tmp, "src/dec.circom", "template Dec(nLevels) { signal private input a; signal output b; b <== a; }\n")
            write(tmp, "pkg/circomkit.json", json.dumps({"dirCircuits": "./circom", "circuits": "./circom/circuits.json"}))
            write(tmp, "pkg/circom/circuits.json", json.dumps({
                "tree_3": {"file": "./tree", "template": "Tree", "params": [3, [1, 2]]},
                "broken": {"file": "./missing", "template": "Nope", "params": [1]}}))
            write(tmp, "pkg/ts/__tests__/Sum.test.ts",
                  'circuit = await circomkitInstance.WitnessTester("sum", {\n  file: "./utils/sum",\n'
                  '  template: "Sum",\n  params: [6],\n});\n'
                  'other = await circomkitInstance.WitnessTester("sum", { file: "./utils/sum", template: "Sum", '
                  "params: [nums.length] });\n")
            write(tmp, "test/dec.test.js", "const NLEVELS = 32;\nconst circuitCode = `\n"
                  '  include "../src/dec.circom";\n  component main = Dec(${NLEVELS});\n`;\n')
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            got = sorted((c.kind, c.template, tuple(c.args), c.target) for c in cs.scan_config_mains(tmp, files))
            self.assertEqual(got, [("circomkit-config", "Tree", ("3", "[1,2]"), "pkg/circom/tree.circom"),
                                   ("js-test", "Dec", ("32",), "src/dec.circom"),
                                   ("js-test", "Sum", ("6",), "pkg/circom/utils/sum.circom")])
            plans = plan(tmp)
            self.assertEqual(first(plans["Tree"]), ("repo-main", [("3", "[1,2]")]))
            self.assertEqual(first(plans["Sum"]), ("repo-test", [("6",)]))
            self.assertEqual(first(plans["Dec"]), ("repo-test", [("32",)]))
            self.assertIn("pkg/circom/circuits.json", plans["Tree"].candidates["repo-main"][0].provenance[0])


class ProbedTierTests(unittest.TestCase):
    def test_probe_only_where_the_template_asserts_bound_every_parameter(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "c/p.circom", "pragma circom 2.0.0;\n"
                  "template P(n) { assert(n <= 8); signal input a[n]; }\n"
                  "template Q(n, k) { assert(n > 1 && n < 5); assert(k <= 2); signal input a; }\n"
                  "template R(n) { assert(n >= 2); signal input a; }\n"
                  "template S(n) { signal input a[n]; }\n"
                  "template G(n) { assert(n <= 8); signal input a; }\ntemplate UsesG() { component g = G(3); }\n")
            plans = plan(tmp)
            self.assertEqual(first(plans["P"]), ("probed", [("1",), ("2",), ("3",), ("4",), ("8",)]))
            self.assertEqual(first(plans["Q"]), ("probed", [("2", "1"), ("2", "2"), ("3", "1"), ("3", "2"),
                                                            ("4", "1"), ("4", "2")]))
            self.assertIn("assert", plans["P"].candidates["probed"][0].provenance[0])
            self.assertIsNone(first(plans["R"]))            # no upper bound
            self.assertIsNone(first(plans["S"]))            # no assert
            self.assertEqual(first(plans["G"]), ("repo-internal", [("3",)]))      # grounded tiers come first
            self.assertEqual(I.TIERS[-1], "probed")

    def test_probed_counterexamples_are_labelled_not_a_finding(self) -> None:
        from zk_registry import circom_det as D
        for rule, label in (("probed", True), ("repo-main", False)):
            rec = {"instantiation": {"rule": rule}, "statement": {"truth": "unknown"}}
            gates = {g: {"status": "PASS"} for g in ("G-ELAB", "G-NONVAC", "G-FID")}
            gates["G-TRIV"] = {"status": "SKIPPED"}
            gates["DET-SEARCH"] = {"status": "FAIL", "truth": "false-counterexample-found", "method": "re-solve"}
            D.set_status(rec, gates, True)
            self.assertEqual(rec["status"], "DET-FALSE-CANDIDATE")
            self.assertEqual("not-a-finding" in rec["status_reason"], label)
            self.assertEqual(gates["DET-SEARCH"].get("finding", "").startswith("not-a-finding"), label)


if __name__ == "__main__":
    unittest.main()
