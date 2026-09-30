#!/usr/bin/env python3
"""Tagged template inputs: known-tag preconditions, the untagged wrapper and DET under preconditions (offline)."""
from __future__ import annotations

import os
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import circom_source as cs       # noqa: E402
from zk_registry import det_search                # noqa: E402
from zk_registry import lean_emit as E            # noqa: E402
from zk_registry import package as P              # noqa: E402
from zk_registry import r1cs as R                 # noqa: E402
from zk_registry import tags as TG                # noqa: E402
from zk_registry import witness as W              # noqa: E402

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_zk_registry_package import make_package   # noqa: E402

SRC = """pragma circom 2.1.0;
template T(n) {
    var m = n + 1;
    signal input {binary} sel[n];
    signal input {binary} s;
    signal input x;
    signal output out[m];
    for (var i = 0; i < n; i++) { out[i] <== sel[i] * x + s; }
    out[n] <== x;
}
template U() { signal input {uint64} a; signal output b; b <== a; }
template V(k) { signal input {maxbit} a[k]; signal output b; b <== a[0]; }
template X(n) { var q; q = n; signal input {binary} a[q]; }
"""


def templates() -> dict[str, cs.Template]:
    return {t.name: t for t in cs.find_templates(cs.strip_comments(SRC), "lib.circom")}


class TagScanTests(unittest.TestCase):
    def test_signal_declarations_keep_tags(self) -> None:
        decls = cs.signal_declarations("signal input {binary, maxbit} a[2], b; signal output {binary} o; signal c;")
        self.assertEqual([(d.direction, d.name, d.tags) for d in decls],
                         [("input", "a", ["binary", "maxbit"]), ("input", "b", ["binary", "maxbit"]),
                          ("output", "o", ["binary"]), ("intermediate", "c", [])])

    def test_known_unknown_and_valued_tags(self) -> None:
        ts = templates()
        self.assertEqual([(p.signal, p.tag) for p in TG.preconditions(ts["T"])], [("sel", "binary"), ("s", "binary")])
        with self.assertRaisesRegex(TG.TagError, "uint64.*not in the known circomlib tag table"):
            TG.preconditions(ts["U"])
        with self.assertRaisesRegex(TG.TagError, "valued tag `maxbit`"):
            TG.preconditions(ts["V"])
        self.assertEqual([(p.tag, p.value) for p in TG.preconditions(ts["V"], {("a", "maxbit"): 64})],
                         [("maxbit", 64)])


class WrapperTests(unittest.TestCase):
    def test_wrapper_declares_untagged_copies(self) -> None:
        text = TG.wrapper_source("lib.circom", templates()["T"], ("3",), (2, 0, 0), set())
        self.assertEqual(text, "\n".join([
            "pragma circom 2.1.0;",
            'include "lib.circom";',
            "",
            "// untagged wrapper: DET is stated under the preconditions of the tags of T's inputs",
            "template BooleTagWrapper(n) {",
            "    var m = n + 1;",
            "    signal input sel[n];",
            "    signal input s;",
            "    signal input x;",
            "    signal output out[m];",
            "    component boole_c = T(n);",
            "    signal {binary} sel_boole_tag[n];",
            "    for (var i0 = 0; i0 < n; i0++) { sel_boole_tag[i0] <== sel[i0]; }",
            "    for (var i0 = 0; i0 < n; i0++) { boole_c.sel[i0] <== sel_boole_tag[i0]; }",
            "    signal {binary} s_boole_tag;",
            "    s_boole_tag <== s;",
            "    boole_c.s <== s_boole_tag;",
            "    boole_c.x <== x;",
            "    for (var i0 = 0; i0 < m; i0++) { out[i0] <== boole_c.out[i0]; }",
            "}",
            "",
            "component main = BooleTagWrapper(3);",
            ""]))

    def test_wrapper_needs_declarable_dimensions(self) -> None:
        with self.assertRaisesRegex(TG.TagError, "uses `q`"):
            TG.wrapper_source("lib.circom", templates()["X"], ("2",), (2, 1, 0), set())


class StatementTests(unittest.TestCase):
    PRE = [{"signal": "main.sel", "tag": "binary", "value": None, "wires": [3, 4]},
           {"signal": "main.a", "tag": "maxbit", "value": 64, "wires": [5]}]

    def test_untagged_statements_are_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, rec = make_package(tmp)
            self.assertNotIn("Preconditions", rec["statement"]["text"])
            self.assertIn("Constraints w₁ → Constraints w₂ →\n      (∀ i ∈ Inputs", rec["statement"]["text"])

    def test_model_and_statement_carry_the_preconditions(self) -> None:
        fix = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-circuit"
        r = R.read_r1cs(str(fix / "toy.r1cs"))
        syms = R.read_sym(str(fix / "toy.sym"))
        io = R.main_io_wires(r, syms)
        meta = {"repo_id": "t/r", "instantiation": "T()", "generator": "g", "repo_url": "https://x", "commit": "0" * 40,
                "path": "t.circom", "template": "T", "rule": "parameter-free", "circom_version": "2.2.3",
                "circom_flags": [], "r1cs_sha256": "0" * 64, "prime_name": "bn128"}
        model = E.emit_model("ZkDet.T", meta, r, io.outputs, io.inputs, R.wire_names(syms, r.n_wires), self.PRE)
        self.assertIn("def BinaryInputs : List (Fin nWires) := [3, 4]", model)
        self.assertIn("def MaxbitInputs64 : List (Fin nWires) := [5]", model)
        self.assertIn("def Preconditions (w : Fin nWires → F) : Prop :=\n"
                      "  (∀ i ∈ BinaryInputs, w i = 0 ∨ w i = 1) ∧ (∀ i ∈ MaxbitInputs64, (w i).val < 2 ^ 64)", model)
        stmt = E.emit_statement("ZkDet.T", meta, preconditions=True)
        self.assertIn("Constraints w₁ → Constraints w₂ → Preconditions w₁ → Preconditions w₂ →", stmt)
        self.assertEqual(E.unfold_order(3, True)[:2], ["Constraints", "Preconditions"])
        self.assertIn("MaxbitInputs64", E.unfold_order(3, True, ["BinaryInputs", "MaxbitInputs64"]))
        battery = E.emit_battery("ZkDet.T", 3, "V1", ["simp"], 1000, preconditions=True)
        self.assertIn("Preconditions w₁ → Preconditions w₂", battery)
        fid = E.emit_fid_runner("ZkDet.T", 3, [("real_000", "/w.txt")], preconditions=True)
        self.assertIn('IO.println s!"PRE real_000 {if decide (Preconditions w) then "ACCEPT" else "REJECT"}"', fid)
        self.assertIn("instance instDecPreconditions (w : Fin nWires → F) : Decidable (Preconditions w) := by\n"
                      "  unfold Preconditions; infer_instance", fid)
        self.assertNotIn("Preconditions", E.emit_fid_runner("ZkDet.T", 3, [("real_000", "/w.txt")]))

    def test_python_evaluation_and_record_schema(self) -> None:
        w = [1, 0, 0, 1, 0, 2 ** 64 - 1]
        self.assertTrue(TG.holds(self.PRE, w))
        self.assertFalse(TG.holds(self.PRE, w[:3] + [2] + w[4:]))
        self.assertFalse(TG.holds(self.PRE, w[:5] + [2 ** 64]))
        with tempfile.TemporaryDirectory() as tmp:
            pkg, rec = make_package(tmp)
            rec["statement"]["preconditions"] = [dict(g, condition="each element is 0 or 1") for g in self.PRE]
            rec["instantiation"]["tag_wrapper"] = {"wrapper": "BooleTagWrapper", "tagged_inputs": {"sel": ["binary"]}}
            self.assertEqual(P.validate_problem(rec, pkg), [])
            rec["statement"]["preconditions"][0]["tag"] = "uint64"
            self.assertTrue(P.validate_problem(rec, pkg))


class SamplingAndSearchTests(unittest.TestCase):
    def test_binary_inputs_are_sampled_inside_the_precondition(self) -> None:
        sigs = [W.InputSignal("sel", [3, 4], domain="binary"), W.InputSignal("x", [5])]
        objs = W.sample_inputs(sigs, R.BN254_SCALAR, "seed", 60)
        self.assertTrue(all(v in ("0", "1") for o in objs for v in o["sel"]))
        self.assertTrue(any(int(v) > 1 for o in objs for v in o["x"]))
        grid = W.boundary_grid(sigs, R.BN254_SCALAR)
        self.assertEqual(len(grid), 2 * 2 * 3)
        self.assertTrue(all(v in ("0", "1") for o in grid for v in o["sel"]))

    def test_counterexamples_must_satisfy_the_preconditions(self) -> None:
        p = 97
        # out = x (wire 1 output, wire 2 input), plus a free wire 3 that is not constrained
        r = R.R1cs(p, 8, 4, 1, 1, 0, 4, [([], [], [(1, 1), (2, p - 1)])])
        w1, w2 = [1, 5, 5, 0], [1, 5, 5, 1]
        self.assertIsNone(det_search.confirm(r, w1, w2, [2], [1]))
        pre = [{"signal": "main.x", "tag": "binary", "value": None, "wires": [2]}]
        base = [1, 0, 0, 0]
        other = [1, 1, 0, 0]
        r2 = R.R1cs(p, 8, 4, 1, 1, 0, 4, [([], [], [])])        # no constraint: output unconstrained
        self.assertEqual(det_search.confirm(r2, base, other, [2], [1]), [1])
        self.assertEqual(det_search.confirm(r2, base, other, [2], [1], pre=lambda w: TG.holds(pre, w)), [1])
        self.assertIsNone(det_search.confirm(r2, [1, 0, 2, 0], [1, 1, 2, 0], [2], [1],
                                             pre=lambda w: TG.holds(pre, w)))
        ce, _ = det_search.search(r2, [[1, 0, 2, 0]], [2], [1], "s", pre=lambda w: TG.holds(pre, w))
        self.assertIsNone(ce)
        ce, _ = det_search.search(r2, [[1, 0, 1, 0]], [2], [1], "s", pre=lambda w: TG.holds(pre, w))
        self.assertIsNotNone(ce)


if __name__ == "__main__":
    unittest.main()
