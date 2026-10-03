#!/usr/bin/env python3
"""Battery P3 (mechanical propagation) for AIR and halo2 packages: offline fixture tests (model reconstruction from
the IR, the propagation rules and their soundness limits — multiplicity-zero branches, guards that may be zero,
non-constant coefficients —, range facts from tables, the emitted Solution.lean against the checker's text rules,
and the run summary).  No Lean is needed: the production checker judges real proofs in the battery run."""
from __future__ import annotations

import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import air_bus as AB                  # noqa: E402
from zk_registry import air_ir as IR                   # noqa: E402
from zk_registry import air_lean as AL                 # noqa: E402
from zk_registry import check as C                     # noqa: E402
from zk_registry import halo2_ir as H                  # noqa: E402
from zk_registry import halo2_lean_emit as HL          # noqa: E402
from zk_registry import mech_air as M                  # noqa: E402

BB = 2013265921
INV256 = pow(256, -1, BB)


class Dag:
    """A hash-consed boole-air-ir/v1 node list."""

    def __init__(self) -> None:
        self.nodes: list = []
        self.ids: dict = {}

    def node(self, *n) -> int:
        key = json.dumps(n)
        if key not in self.ids:
            self.ids[key] = len(self.nodes)
            self.nodes.append(list(n))
        return self.ids[key]

    def c(self, v: int) -> int:
        return self.node("const", v % BB)

    def v(self, col: int) -> int:
        return self.node("main", 0, col)

    def add(self, a, b):
        return self.node("add", a, b)

    def sub(self, a, b):
        return self.node("sub", a, b)

    def mul(self, a, b):
        return self.node("mul", a, b)


def inter(direction: str, values: list, mult: int, bus: int) -> dict:
    return {"dir": direction, "kind": None, "kind_name": "", "bus": bus, "scope": None, "values": values,
            "mult": mult, "count_weight": None}


def air_package(root: str, dag: Dag, width: int, constraints: list, ins: list, outs: list, asms=()) -> str:
    """A package directory (IR, interface, Model.lean, Statement.lean, problem.json) for an OpenVM-style toy AIR.
    ``ins`` / ``outs``: (values, mult); ``asms``: (table, values, mult)."""
    inters, iface = [], {"inputs": [], "outputs": [], "assumptions": []}
    for vals, mult in ins:
        iface["inputs"].append({"interaction": len(inters), "direction": "receive", "bus": "Memory 1", "role": "in",
                                "reason": "r"})
        inters.append(inter("receive", vals, mult, 1))
    for vals, mult in outs:
        iface["outputs"].append({"interaction": len(inters), "direction": "send", "bus": "Memory 1", "role": "out",
                                 "reason": "r"})
        inters.append(inter("send", vals, mult, 1))
    for table, vals, mult in asms:
        iface["assumptions"].append({"interaction": len(inters), "direction": "send", "bus": "VarRange 3",
                                     "role": "assume", "reason": "r", "table": table})
        inters.append(inter("send", vals, mult, 3))
    doc = {"format": IR.FORMAT, "zkvm": "openvm", "release": "v0", "commit": "c0",
           "field": {"name": "BabyBear", "p": BB}, "air": {"name": "Toy", "rust_type": "Toy", "group": "test",
                                                          "index": 0},
           "width": width, "preprocessed_width": 0, "num_public_values": 0, "meta": {}, "nodes": dag.nodes,
           "constraints": constraints, "interactions": inters}
    air = IR.from_json(json.loads(json.dumps(doc)))
    roles = M.air_roles(air, iface)
    ns = "ZkDet.toy_air"
    meta = {"zkvm_name": "OpenVM", "release": "v0", "generator": "g v1", "repo_url": "u", "extractor": "x",
            "ir_sha256": "h"}
    model, _ = AL.emit_model(ns, meta, air, IR.Layout.of(air), roles, AB.model_for("openvm").tables)
    return _write(root, ns, model, AL.emit_statement(ns, meta, air), {
        "package_id": "toy/air", "interface": iface, "ids": {"release": "v0", "repo_url": "u"},
        "generator": {"name": "g", "version": "1"}, "air": {"extractor": {"builder": "x"}, "ir_sha256": "h"}},
        ("evidence/air.ir.json", doc))


def _write(root: str, ns: str, model: str, statement: str, problem: dict, evidence: tuple) -> str:
    rel = AL.model_relpath(ns)
    for path, text in ((rel, model), ("Statement.lean", statement)):
        os.makedirs(os.path.dirname(os.path.join(root, path)) or root, exist_ok=True)
        with open(os.path.join(root, path), "w", encoding="utf-8") as f:
            f.write(text)
    problem["checker"] = {"statement_file": "Statement.lean",
                          "files": [{"role": "statement", "path": "Statement.lean"}, {"role": "import", "path": rel}]}
    os.makedirs(os.path.join(root, "evidence"), exist_ok=True)
    with open(os.path.join(root, evidence[0]), "w", encoding="utf-8") as f:
        json.dump(evidence[1], f)
    with open(os.path.join(root, "problem.json"), "w", encoding="utf-8") as f:
        json.dump(problem, f)
    return root


def emitted_ok(test: unittest.TestCase, prob: M.Problem, res: M.Result) -> str:
    """The emitted solution satisfies the checker's text and forbidden-construct rules."""
    text = M.emit_solution(prob, res)
    probs, aux, body = C.text_check(prob.statement, text, "det")
    test.assertEqual(probs, [])
    test.assertEqual(C.forbidden_scan([("helpers", aux), ("proof", body)]), [])
    return text


class Fixture(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = self.tmp.name

    def tearDown(self) -> None:
        self.tmp.cleanup()


class ReconstructionTests(Fixture):
    def adder(self) -> str:
        g = Dag()
        a, b, out, real = g.v(0), g.v(1), g.v(2), g.v(3)
        con = g.mul(real, g.sub(out, g.add(a, b)))
        return air_package(self.dir, g, 4, [con], [([a, b], real)], [([out], real)])

    def test_model_is_regenerated_from_the_ir(self) -> None:
        prob = M.load(self.adder())
        self.assertEqual((prob.kind, prob.n_vars, len(prob.inputs), len(prob.outputs)), ("air", 4, 1, 1))

    def test_tampered_model_is_rejected(self) -> None:
        pkg = self.adder()
        rel = AL.model_relpath("ZkDet.toy_air")
        path = os.path.join(pkg, rel)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        with open(path, "w", encoding="utf-8") as f:
            f.write(text.replace("w 3 * (w 2 - (w 0 + w 1)) = 0", "w 3 * (w 2 - (w 0 + w 0)) = 0"))
        with self.assertRaises(M.MechError):
            M.load(pkg)


class RuleTests(Fixture):
    def test_guarded_linear_step_in_the_output_context(self) -> None:
        g = Dag()
        a, b, out, real = g.v(0), g.v(1), g.v(2), g.v(3)
        con = g.mul(real, g.sub(out, g.add(a, b)))
        prob = M.load(air_package(self.dir, g, 4, [con], [([a, b], real)], [([out], real)]))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules(), {"LIN": 4})      # the multiplicity, a, b (input fields) and out
        text = emitted_ok(self, prob, res)
        self.assertIn("(mul_eq_zero.mp a).resolve_left n₁", text)
        self.assertIn("have o0 : w₁ 3 ≠ 0 → ([w₁ 2] = [w₂ 2]) := by", text)

    def test_multiplicity_zero_branch_gives_no_values(self) -> None:
        # the input carries a, b only when `flag` is non-zero; the output is due whenever `real` is non-zero
        g = Dag()
        a, b, out, real, flag = g.v(0), g.v(1), g.v(2), g.v(3), g.v(4)
        con = g.mul(real, g.sub(out, g.add(a, b)))
        res = M.solve(M.load(air_package(self.dir, g, 5, [con], [([a, b], flag), ([], real)], [([out], real)])))
        self.assertEqual(res.status, "STUCK")
        self.assertEqual(res.detail["inputs_without_values"], 1)

    def test_output_copied_from_a_gated_input_is_classified(self) -> None:
        g = Dag()
        a, real, flag = g.v(0), g.v(1), g.v(2)
        res = M.solve(M.load(air_package(self.dir, g, 3, [], [([a], flag), ([], real)], [([a], real)])))
        self.assertEqual((res.status, res.detail["class"]), ("STUCK", "input-multiplicity"))

    def test_guard_that_may_be_zero_is_not_peeled(self) -> None:
        g = Dag()
        a, b, out, real, op = g.v(0), g.v(1), g.v(2), g.v(3), g.v(4)
        con = g.mul(op, g.sub(out, g.add(a, b)))
        res = M.solve(M.load(air_package(self.dir, g, 5, [con], [([a, b, op], real)], [([out], real)])))
        self.assertEqual((res.status, res.detail["class"]), ("STUCK", "guard-may-be-zero"))

    def test_division_by_a_known_cell_is_stuck(self) -> None:
        # b * out = a: out is not determined when b may be zero (the calibration item-5 shape)
        g = Dag()
        a, b, out, real = g.v(0), g.v(1), g.v(2), g.v(3)
        con = g.mul(real, g.sub(g.mul(b, out), a))
        res = M.solve(M.load(air_package(self.dir, g, 4, [con], [([a, b], real)], [([out], real)])))
        self.assertEqual((res.status, res.detail["class"]), ("STUCK", "single-unknown-nonconstant-coefficient"))

    def test_constant_coefficient_uses_the_cancel_lemma(self) -> None:
        g = Dag()
        a, out, real = g.v(0), g.v(1), g.v(2)
        con = g.mul(real, g.sub(g.mul(g.c(3), out), a))
        prob = M.load(air_package(self.dir, g, 3, [con], [([a], real)], [([out], real)]))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        text = emitted_ok(self, prob, res)
        self.assertIn("theorem mnz_0 : (3 : F) ≠ 0 := by decide", text)
        self.assertIn("exact mech_cancel mnz_0 (by linear_combination a - b)", text)

    def carry_package(self, bits: int) -> str:
        # out = a + b - 256 * t with a boolean carry t = (a + b - out) / 256 and out < 2^bits
        g = Dag()
        a, b, out, real = g.v(0), g.v(1), g.v(2), g.v(3)
        t = g.mul(g.sub(g.add(a, b), out), g.c(INV256))
        con = g.mul(real, g.mul(t, g.sub(t, g.c(1))))
        return air_package(self.dir, g, 4, [con], [([a, b], real)], [([out], real)],
                           [("ovmVarRange", [out, g.c(bits)], real)])

    def test_carry_with_a_byte_range(self) -> None:
        prob = M.load(self.carry_package(8))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules(), {"LIN": 3, "CARRY": 1})
        text = emitted_ok(self, prob, res)
        self.assertIn("refine mech_vdig 256 256 2 (by decide) (by decide) su₁ su₂ st₁ st₂ ?_", text)
        self.assertIn("linear_combination (1) * (w₁ 2 - w₂ 2) * mech_p", text)
        self.assertIn("mech_vr 8 (by decide)", text)
        self.assertIn("theorem mech_vr", text)

    def test_carry_needs_the_range_to_fit(self) -> None:
        res = M.solve(M.load(self.carry_package(9)))
        self.assertEqual(res.status, "STUCK")

    def digits_package(self, bits: int, weight: int) -> str:
        g = Dag()
        x, u0, u1, real = g.v(0), g.v(1), g.v(2), g.v(3)
        con = g.mul(real, g.sub(x, g.add(u0, g.mul(u1, g.c(weight)))))
        return air_package(self.dir, g, 4, [con], [([x], real)], [([u0, u1], real)],
                           [("ovmVarRange", [u0, g.c(bits)], real), ("ovmVarRange", [u1, g.c(bits)], real)])

    def test_limb_decomposition_is_unique(self) -> None:
        prob = M.load(self.digits_package(8, 256))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        self.assertIn("DIGITS", res.rules())
        text = emitted_ok(self, prob, res)
        self.assertIn("have key : 1 * w₁ 1 + 256 * w₁ 2 = 1 * w₂ 1 + 256 * w₂ 2", text)
        self.assertIn("obtain ⟨e1, e2⟩ := md_", text)

    def test_overlapping_limbs_are_not_unique(self) -> None:
        self.assertEqual(M.solve(M.load(self.digits_package(9, 256))).status, "STUCK")

    def test_linear_system(self) -> None:
        g = Dag()
        a, b, u, v, real = g.v(0), g.v(1), g.v(2), g.v(3), g.v(4)
        c1 = g.mul(real, g.sub(g.add(u, v), a))
        c2 = g.mul(real, g.sub(g.sub(u, v), b))
        prob = M.load(air_package(self.dir, g, 5, [c1, c2], [([a, b], real)], [([u, v], real)]))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        self.assertIn("LINSYS", res.rules())
        text = emitted_ok(self, prob, res)
        self.assertIn("mech_p", text)

    def test_functional_table_row(self) -> None:
        g = Dag()
        x, y, z, real = g.v(0), g.v(1), g.v(2), g.v(3)
        prob = M.load(air_package(self.dir, g, 4, [], [([x, y], real)], [([z], real)],
                                  [("ovmBitwise", [x, y, z, g.c(1)], real)]))
        res = M.solve(prob)
        self.assertEqual((res.status, res.rules().get("TABLE-FN")), ("DETERMINED", 1))
        self.assertIn("exact mech_bw_fn (by decide) g₁ g₂", emitted_ok(self, prob, res))

    def test_output_built_from_an_input_field(self) -> None:
        # the output (lo + 65536 * hi) + 8 is determined by the input field lo + 65536 * hi, not by its cells
        g = Dag()
        lo, hi, real = g.v(0), g.v(1), g.v(2)
        clk = g.add(lo, g.mul(hi, g.c(65536)))
        prob = M.load(air_package(self.dir, g, 3, [], [([clk], real)], [([g.add(clk, g.c(8))], real)]))
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        self.assertIn("(by rw [iv0_0])", emitted_ok(self, prob, res))


class Halo2Tests(Fixture):
    def doc(self, copy_b: bool = True) -> dict:
        """q (fixed 0, row 0) * (a0 + a1 - a2) with a0, a1 copied from the instance; a2 copied to the outputs."""
        def hx(v: int) -> str:
            return f"{v:064x}"
        p = 0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001
        gate = ["mul", ["f", 0, 0], ["add", ["add", ["a", 0, 0], ["a", 1, 0]], ["neg", ["a", 2, 0]]]]
        cycles = [[[0, 0], [1, 0]], [[3, 0], [4, 0]]] + ([[[0, 1], [2, 0]]] if copy_b else [])
        return {"format": H.FORMAT, "halo2_line": "test", "k": 3, "n": 8, "usable_rows": 6, "num_fixed": 1,
                "num_advice": 4, "num_instance": 1, "field": {"modulus": hex(p), "one": hx(1), "two": hx(2)},
                "meta": {"target": "t", "input_notes": []}, "gates": [{"name": "g", "polys": [{"name": "p", "e": gate}]}],
                "lookups": [], "perm_columns": [["i", 0], ["a", 0], ["a", 1], ["a", 2], ["a", 3]],
                "cycles": cycles, "fixed": [[0, 0, hx(1)]], "advice": [[0, 0, hx(5)], [1, 0, hx(7)], [2, 0, hx(12)],
                                                                        [3, 0, hx(12)]],
                "instance": [[0, 0, hx(5)], [0, 1, hx(7)]], "regions": [["boole-outputs", 0, 0]],
                "notes": [[0, "boole-outputs", 3, 0, "out"]], "verify": []}

    def package(self, copy_b: bool = True) -> str:
        doc = self.doc(copy_b)
        model = H.flatten(H.parse(json.loads(json.dumps(doc))))
        ns = "ZkDet.toy_halo2"
        meta = {k: "" for k in ("repo_id", "target", "generator", "repo_url", "commit", "path", "symbol", "wrapper",
                                "rule", "call", "halo2_line", "ir_sha256", "model_sha256")}
        text, _ = HL.emit_model(ns, meta, model)
        return _write(self.dir, ns, text, HL.emit_statement(ns, meta),
                      {"package_id": "toy/halo2", "circuit": {}}, ("evidence/ir_sample_000.json", doc))

    def test_copies_and_gate_determine_the_output(self) -> None:
        prob = M.load(self.package())
        self.assertEqual(prob.kind, "halo2")
        res = M.solve(prob)
        self.assertEqual(res.status, "DETERMINED")
        text = emitted_ok(self, prob, res)
        self.assertIn("List.forall_mem_cons.2 ⟨e", text)

    def test_missing_copy_is_stuck(self) -> None:
        res = M.solve(M.load(self.package(copy_b=False)))
        self.assertEqual(res.status, "STUCK")


class HelperTests(unittest.TestCase):
    def test_conjunct_paths(self) -> None:
        self.assertEqual(M._conj_path(0, 1, []), ("", ""))
        self.assertEqual(M._conj_path(2, 3, []), (".2.2", None))
        self.assertEqual(M._conj_path(1, 3, []), (".2.1", None))
        blocks = ["Block0", "Block1"]
        self.assertEqual(M._conj_path(64, 65, blocks), (".2", "Block1"))
        self.assertEqual(M._conj_path(3, 65, blocks), (".1.2.2.2.1", None))

    def test_window_substitution(self) -> None:
        self.assertEqual(M._w("t12 w * (w 3 - ovmBitwise)", 1), "t12 w₁ * (w₁ 3 - ovmBitwise)")

    def test_matrix_inverse(self) -> None:
        a = [[1, 1], [1, BB - 1]]
        inv = M._inverse(a, BB)
        for i in range(2):
            for j in range(2):
                self.assertEqual(sum(inv[i][k] * a[k][j] for k in range(2)) % BB, int(i == j))
        self.assertEqual(M._rank([[1, 2], [2, 4]], BB), 1)

    def test_summary_and_error_classes(self) -> None:
        recs = [{"family": "sp1", "outcome": "MECH-SOLVED", "rules": {"LIN": 2}, "check_s": 3.0, "check_rss_mb": 7000},
                {"family": "halo2", "outcome": "MECH-STUCK", "reason": "r"},
                {"family": "openvm", "outcome": "MECH-PROOF-FAIL", "check_s": 9.0}]
        s = M.summarize(recs)
        self.assertEqual(s["by_family"]["sp1"]["MECH-SOLVED"], 1)
        self.assertEqual(s["stuck_reasons"], {"halo2": {"r": 1}})
        self.assertEqual((s["check_s_total"], s["check_rss_mb_max"]), (12.0, 7000))
        self.assertEqual(M.error_class({"fail": ["1 error(s): 12:3: ring failed, ring expressions not equal"]}),
                         "FAIL:ring")
        self.assertEqual(M.error_class({"fail": ["compile timed out"]}), "FAIL:timeout")


if __name__ == "__main__":
    unittest.main()
