#!/usr/bin/env python3
"""Lean emitter tests: the model is a verbatim, value-preserving transcription of the R1CS."""
from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import lean_emit as E  # noqa: E402
from zk_registry import r1cs as R       # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-circuit"
P = R.BN254_SCALAR
META = {"repo_id": "toy/repo", "instantiation": "Toy()", "generator": "gen v1", "repo_url": "https://example.invalid/toy",
        "commit": "0" * 40, "path": "toy.circom", "template": "Toy", "rule": "parameter-free",
        "circom_version": "2.2.3", "circom_flags": ["--r1cs", "--sym", "--wasm", "--O0"], "r1cs_sha256": "a" * 64,
        "prime_name": "bn128"}


def eval_lean_term(text: str, w: list[int]) -> int:
    """Evaluate a rendered linear combination over ZMod p with Python (tests the rendering, not Lean)."""
    return eval(re.sub(r"w (\d+)", r"w[\1]", text), {"w": w}) % P


class RenderTests(unittest.TestCase):
    def test_balanced_coefficients_and_term_order(self) -> None:
        self.assertEqual(E.render_lc([], P), "0")
        self.assertEqual(E.render_lc([(0, P - 1), (3, 1)], P), "-w 0 + w 3")
        self.assertEqual(E.render_lc([(1, P - 2), (2, 5), (4, P - 1)], P), "-(2 * w 1) + 5 * w 2 - w 4")

    def test_rendering_preserves_values(self) -> None:
        lc = [(1, P - 2), (2, 5), (4, P - 1), (3, P // 2), (5, P // 2 + 1)]
        w = [1, 17, 2**200 + 3, P - 9, 12345, 7]
        self.assertEqual(eval_lean_term(E.render_lc(lc, P), w), R.lc_value(lc, w, P))

    def test_constraint_forms(self) -> None:
        self.assertEqual(E.render_constraint([(1, 1)], [(0, P - 1), (1, 1)], [], P), "w 1 * (-w 0 + w 1) = 0")
        self.assertEqual(E.render_constraint([], [], [(1, 1), (2, P - 3)], P), "0 = w 1 - 3 * w 2")
        self.assertEqual(E.render_constraint([(1, 2)], [(2, 1)], [(3, 1)], P), "(2 * w 1) * w 2 = w 3")

    def test_identifiers_and_name_summaries(self) -> None:
        self.assertEqual(E.lean_ident("circomlib-v2.0.5_bitify.Num2Bits.253"), "circomlib_v2_0_5_bitify_Num2Bits_253")
        self.assertEqual(E.lean_ident("9x"), "x9x")
        self.assertEqual(E.summarize_names(["main.out[0]", "main.out[1]", "main.out[2]", "main.ok"]),
                         "`main.out[0..2]`, `main.ok`")
        self.assertEqual(E.summarize_names([]), "none")


class ModuleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.r = R.read_r1cs(str(FIX / "toy.r1cs"))
        self.syms = R.read_sym(str(FIX / "toy.sym"))
        self.io = R.main_io_wires(self.r, self.syms)

    def test_toy_model_text(self) -> None:
        text = E.emit_model("ZkDet.toy", META, self.r, self.io.outputs, self.io.inputs,
                            R.wire_names(self.syms, self.r.n_wires))
        self.assertTrue(text.startswith("import Mathlib\n"))
        self.assertIn(f"abbrev p : ℕ := {P}\n", text)
        self.assertIn("abbrev nWires : ℕ := 7\n", text)
        self.assertIn("def Outputs : List (Fin nWires) := [1, 2]\n", text)
        self.assertIn("def Inputs : List (Fin nWires) := [3, 4, 5]\n", text)
        body = text.split("def Constraints (w : Fin nWires → F) : Prop :=\n")[1].split("\n\nend")[0]
        self.assertEqual(body.split(" ∧\n"), ["  w 0 = 1"] + [
            "  " + E.render_constraint(a, b, c, P) for a, b, c in self.r.constraints])
        self.assertIn("* `w 6`: `main.t`", text)
        self.assertTrue(text.rstrip().endswith("end ZkDet.toy"))
        self.assertNotIn("set_option", text)                 # short lists: emitted exactly as in wave 0

    def test_long_io_lists_raise_the_recursion_limit_before_the_doc_comment(self) -> None:
        n = E.LONG_LIST + 2
        r = R.R1cs(P, 32, n + 2, 1, n, 0, n + 2, [([(1, 1)], [(1, 1)], [(1, 1)])])
        names = ["one", "main.o"] + [f"main.i[{k}]" for k in range(n)]
        text = E.emit_model("ZkDet.wide", META, r, [1], list(range(2, n + 2)), names)
        self.assertEqual(text.count("set_option maxRecDepth 100000 in\n"), 1)
        self.assertIn("set_option maxRecDepth 100000 in\n/-- Input signals of the main component", text)
        self.assertIn("\n/-- Output signals of the main component: `main.o`. -/\ndef Outputs", text)

    def test_large_systems_are_split_into_blocks(self) -> None:
        cons = [([(1, 1)], [(1, 1)], [(2, 1)])] * 130
        r = R.R1cs(P, 32, 3, 1, 0, 1, 3, cons)
        text = E.emit_model("ZkDet.big", META, r, [1], [2], ["one", "main.o", "main.i"])
        self.assertEqual(E.block_names(130), ["Block0", "Block1", "Block2"])
        self.assertIn("def Block2 (w : Fin nWires → F) : Prop :=\n  w 1 * w 1 = w 2 ∧\n  w 1 * w 1 = w 2\n", text)
        self.assertIn("  w 0 = 1 ∧ Block0 w ∧ Block1 w ∧ Block2 w\n", text)
        self.assertEqual(text.count(" ∧\n  w 1 * w 1 = w 2"), 130 - 3)
        self.assertEqual(E.unfold_order(130), ["Constraints", "Block0", "Block1", "Block2", "Outputs", "Inputs", "F",
                                               "nWires", "p"])
        self.assertEqual(E.block_names(64), [])

    def test_statement_text(self) -> None:
        self.assertEqual(E.emit_statement("ZkDet.toy", META), "\n".join([
            "import ZkDet.toy.Model",
            "",
            "namespace ZkDet.toy",
            "",
            "/-- Output determinism of toy/repo `Toy()` (toy.circom): two",
            "assignments that satisfy the compiled constraints and agree on every input wire agree on every",
            "output wire. -/",
            "theorem det [Fact (Nat.Prime p)] :",
            "    ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →",
            "      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o := by",
            "  sorry",
            "",
            "end ZkDet.toy",
            "",
        ]))
        self.assertEqual(E.model_relpath("ZkDet.toy"), "ZkDet/toy/Model.lean")

    def test_fid_runner_and_battery(self) -> None:
        fid = E.emit_fid_runner("ZkDet.big", 130, [("real_000", "/w/a.txt")])
        self.assertEqual(fid.count("instance instDecBlock"), 3)
        self.assertEqual(fid.count("set_option synthInstance.maxSize 1000000 in\n"), 4)
        self.assertEqual(fid.count("set_option maxRecDepth 100000 in\n"), 4)
        self.assertIn('let w ← loadWitness "/w/a.txt"', fid)
        self.assertIn('FID real_000 {if decide (Constraints w) then "ACCEPT" else "REJECT"}', fid)
        bat = E.emit_battery("ZkDet.toy", 2, "V2", E.BATTERY_TACTICS, 200000)
        self.assertEqual(bat.count("\ntheorem triv_V2_"), 11)
        self.assertEqual(bat.count("#print axioms triv_V2_"), 11)
        self.assertIn("theorem triv_V2_exactQ" + E.theorem_signature() + " := by\n  intros\n  (try unfold Constraints at *)",
                      bat)
        self.assertEqual(E.battery_prefix("V0", 2), [])
        self.assertEqual(E.battery_prefix("V1", 2)[-1], "(try beta_reduce at *)")


if __name__ == "__main__":
    unittest.main()
