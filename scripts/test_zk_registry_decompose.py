#!/usr/bin/env python3
"""Decomposition of TOO-LARGE instantiations: harvesting, content identity, variant selection (offline)."""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import content as CT             # noqa: E402
from zk_registry import decompose as DC           # noqa: E402
from zk_registry import instantiation as I        # noqa: E402
from zk_registry import package as P              # noqa: E402

from test_zk_registry_package import make_package   # noqa: E402

LIB = """pragma circom 2.0.0;
function twice(x) { return 2 * x; }
template X(k) { signal input a; signal output b; b <== a * k; }
template Y(m) { signal input a; signal output b; b <== a + twice(m); }
template P(n) {
    signal input a;
    signal output b[n];
    component c[n];
    for (var i = 0; i < n; i++) {
        c[i] = X(i + 1);
        c[i].a <== a;
        b[i] <== c[i].b;
    }
    component y = Y(n * 2);
    component z = Z(q);
    y.a <== a;
}
template Z(k) { signal input a; }
"""


def write(root: str, rel: str, text: str) -> None:
    p = Path(root, rel)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


class HarvestTests(unittest.TestCase):
    def test_children_of_a_concrete_instantiation(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "c/lib.circom", LIB)
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            t = next(x for x in files["c/lib.circom"].templates if x.name == "P")
            kids, unresolved = I.children_of(files, t, ("3",), I.function_names(files))
            self.assertEqual([(c.name, a) for c, a, _ in kids],
                             [("X", ("0+1",)), ("X", ("1+1",)), ("X", ("2+1",)), ("Y", ("3*2",))])
            self.assertEqual(unresolved, 1)                      # Z(q): q is not a parameter or binding
            self.assertIn("[i=1]", kids[1][2])
            self.assertIn("c/lib.circom:", kids[0][2])

    def test_content_identity_ignores_layout_but_not_dependencies(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "a/lib.circom", LIB)
            write(tmp, "b/lib.circom", LIB.replace("template Y(m) { signal input a;",
                                                   "// copy\ntemplate   Y(m) {\n  signal input a;"))
            write(tmp, "c/lib.circom", LIB.replace("return 2 * x;", "return 3 * x;"))
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            ha, hb, hc = (CT.content_hash(files, f"{d}/lib.circom", "Y") for d in "abc")
            self.assertEqual(ha, hb)
            self.assertNotEqual(ha, hc)                           # the called function differs
            self.assertEqual(CT.content_hash(files, "a/lib.circom", "X"), CT.content_hash(files, "c/lib.circom", "X"))
            self.assertIsNone(CT.content_hash(files, "a/lib.circom", "Nope"))
        self.assertEqual(CT.args_key(["3+1", "[1, 2]", "f(2)"]), ("4", "[1,2]", "f(2)"))
        self.assertEqual(CT.instance_key("bn128", "c", ["2*2"]), CT.instance_key("bn128", "c", ["4"]))

    def test_parse_call(self) -> None:
        self.assertEqual(DC.parse_call("T(a, f(b, c), [1,2])"), ("T", ["a", "f(b, c)", "[1,2]"]))
        self.assertEqual(DC.parse_call("U()"), ("U", []))


class SelectionTests(unittest.TestCase):
    def test_one_package_per_template_content(self) -> None:
        nodes = {
            "k1": {"group": ("bn128", "A"), "constraints": 10, "wires": 5, "call": "A(1)"},
            "k2": {"group": ("bn128", "A"), "constraints": 1500, "wires": 900, "call": "A(8)"},
            "k3": {"group": ("bn128", "A"), "constraints": 2500, "wires": 1000, "call": "A(16)"},
            "k4": {"group": ("bn128", "B"), "constraints": 7, "wires": 3, "call": "B()"},
            "k5": {"group": ("bn128", "B"), "constraints": None, "wires": 0, "call": "B(2)"},
        }
        chosen = DC.select_variants(nodes, 2000)
        self.assertEqual(chosen, {"k1": "k2", "k2": "k2", "k4": "k4"})
        packaged = {"k2": {"package_id": "r/a.A.8", "status": "OPEN"}, "k4": {"package_id": "r/b.B", "status": "GATE-FAIL"}}
        existing = {"k9": "w1/x.A.3"}
        res = {k: DC.resolution(k, dict(nodes, k5=dict(nodes["k5"], error="boom")), existing, chosen, packaged, 2000)
               for k in ("k1", "k2", "k3", "k5", "k9")}
        self.assertEqual(res["k1"], {"result": "variant-of", "package_id": "r/a.A.8", "status": "OPEN"})
        self.assertEqual(res["k2"]["result"], "packaged")
        self.assertEqual(res["k3"], {"result": "too-large", "constraints": 2500, "decomposed": False})
        self.assertEqual(res["k5"]["result"], "not-compiled")
        self.assertEqual(res["k9"], {"result": "existing-package", "package_id": "w1/x.A.3"})

    def test_schema_accepts_decomposition_records(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            rec["instantiation"]["rule"] = DC.RULE
            rec["instantiation"]["decomposition"] = {
                "parents": [{"package_id": "w1/p.P.3", "call": "P(3)"}, {"key": "bn128:c:4", "call": "Q(4)"}],
                "n_parents": 2, "depth": 1, "content_sha256": "ab" * 32, "variants": 3, "instance_key": "bn128:x:1"}
            self.assertEqual(P.validate_problem(rec, pkg), [])
            rec["instantiation"]["decomposition"]["parents"] = []
            self.assertTrue(P.validate_problem(rec, pkg))

    def test_decomposition_plans_size_through_the_driver_tiers(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            write(tmp, "c/lib.circom", LIB)
            files = cs.scan_repo(tmp, cs.list_circom_files(tmp))
            t = next(x for x in files["c/lib.circom"].templates if x.name == "X")
            plan = I.TemplatePlan(t, {DC.RULE: [I.Candidate(DC.RULE, ("3",), ["p"])]})
            self.assertEqual([tier for tier, _ in plan.tiers_in_order()], [DC.RULE])


if __name__ == "__main__":
    unittest.main()
