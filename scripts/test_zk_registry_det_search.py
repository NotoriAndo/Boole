#!/usr/bin/env python3
"""DET counterexample searches and witness sampling (pure Python; no circom, node or Lean)."""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import det_search as S   # noqa: E402
from zk_registry import r1cs as R         # noqa: E402
from zk_registry import witness as W      # noqa: E402

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "toy-circuit"
P = R.BN254_SCALAR


def bits_circuit(n: int) -> R.R1cs:
    """out[i] * (out[i] - 1) = 0 and sum out[i] 2^i = in: wires 0, out[0..n-1], in."""
    cons = [([(1 + i, 1)], [(0, P - 1), (1 + i, 1)], []) for i in range(n)]
    cons.append(([], [], [(1 + i, (P - 2**i) % P) for i in range(n)] + [(n + 1, 1)]))
    return R.R1cs(P, 32, n + 2, n, 0, 1, n + 2, cons)


def bits_witness(n: int, x: int) -> list[int]:
    return [1] + [(x >> i) & 1 for i in range(n)] + [x]


def hidden_line_circuit() -> R.R1cs:
    """Wires 0 one, 1 o (output), 2 i (input), 3 h.  (h - h) * h = 0 mentions h without constraining
    it, and 0 = -o + i + h ties the output to h: moving o needs h to move too."""
    cons = [([(3, 1), (3, P - 1)], [(3, 1)], []),
            ([], [], [(1, P - 1), (2, 1), (3, 1)])]
    return R.R1cs(P, 32, 4, 1, 0, 1, 4, cons)


def slope_circuit() -> R.R1cs:
    """Wires 0 one, 1 out, 2 x1, 3 x2, 4 y1, 5 y2 (inputs), 6 lam.  lam * (x2 - x1) = y2 - y1 and
    out = lam * lam: with x1 = x2 and y1 = y2, lam and hence out are free, but d out / d lam = 0 at
    lam = 0, so only the re-solve step sees it."""
    cons = [([(6, 1)], [(3, 1), (2, P - 1)], [(5, 1), (4, P - 1)]),
            ([(6, 1)], [(6, 1)], [(1, 1)])]
    return R.R1cs(P, 32, 7, 1, 0, 4, 7, cons)


class DetSearchTests(unittest.TestCase):
    def test_output_mutation_finds_the_unconstrained_output_of_the_fixture(self) -> None:
        r = R.read_r1cs(str(FIX / "toy.r1cs"))
        io = R.main_io_wires(r, R.read_sym(str(FIX / "toy.sym")))
        w = [int(v) for v in json.loads((FIX / "witness.json").read_text())["witness"]]
        ce, log = S.search(r, [w], io.inputs, io.outputs, "seed")
        self.assertIsNotNone(ce)
        self.assertEqual(ce.method, "output-mutation")
        self.assertEqual(ce.changed_outputs, [2])
        self.assertEqual(S.confirm(r, ce.base, ce.other, io.inputs, io.outputs), [2])
        self.assertEqual(log["output_mutation"], "found")

    def test_linear_kernel_finds_a_multi_wire_free_direction(self) -> None:
        r = hidden_line_circuit()
        w = [1, 9, 4, 5]            # o = 9 = i (4) + h (5)
        self.assertTrue(R.satisfies(r, w))
        ce, log = S.search(r, [w], [2], [1], "seed")
        self.assertIsNotNone(ce)
        self.assertEqual(ce.method, "linear-kernel")
        self.assertEqual(log["output_mutation"], "no-counterexample")

    def test_re_solve_follows_a_curved_solution_set(self) -> None:
        r = slope_circuit()
        w = [1, 0, 5, 5, 9, 9, 0]
        self.assertTrue(R.satisfies(r, w))
        ce, _ = S.search(r, [w], [2, 3, 4, 5], [1], "seed")
        self.assertIsNotNone(ce)
        self.assertEqual(ce.method, "re-solve")
        self.assertEqual(ce.other[1], ce.other[6] * ce.other[6] % P)
        generic = [1, 4, 5, 7, 9, 13, 2]        # x1 != x2: lam = 2 is forced, out = 4
        self.assertTrue(R.satisfies(r, generic))
        self.assertIsNone(S.search(r, [generic], [2, 3, 4, 5], [1], "seed")[0])

    def test_propagate_recomputes_dependent_wires(self) -> None:
        r = slope_circuit()
        w2 = S.propagate(r, [1, 0, 5, 5, 9, 9, 0], {0, 2, 3, 4, 5}, {6: 3}, deadline=float("inf"))
        self.assertEqual(w2, [1, 9, 5, 5, 9, 9, 3])
        self.assertIsNone(S.propagate(r, [1, 4, 5, 7, 9, 13, 2], {0, 2, 3, 4, 5}, {6: 3}, deadline=float("inf")))

    def test_no_counterexample_for_a_deterministic_bit_decomposition(self) -> None:
        n = 6
        r = bits_circuit(n)
        ws = [bits_witness(n, x) for x in (0, 1, 37, 63)]
        self.assertTrue(all(R.satisfies(r, w) for w in ws))
        ce, log = S.search(r, ws, [n + 1], list(range(1, n + 1)), "seed")
        self.assertIsNone(ce)
        self.assertTrue(all(s.startswith("no-counterexample") for s in log["linear_kernel"]))

    def test_confirm_requires_equal_inputs_and_two_solutions(self) -> None:
        r = bits_circuit(3)
        a, b = bits_witness(3, 5), bits_witness(3, 6)
        self.assertIsNone(S.confirm(r, a, b, [4], [1, 2, 3]))          # inputs differ
        self.assertIsNone(S.confirm(r, a, a, [4], [1, 2, 3]))          # outputs equal
        self.assertIsNone(S.confirm(r, a, [1, 2, 0, 0, 5], [4], [1, 2, 3]))  # second is not a solution

    def test_nullspace_respects_the_budget(self) -> None:
        with self.assertRaises(S.Budget):
            S.nullspace_basis([{1: 1}], [1], P, deadline=0.0)


class WitnessSamplingTests(unittest.TestCase):
    def test_input_signals_group_contiguous_wires(self) -> None:
        io = R.MainIo([1], [2, 3, 4], ["main.o"], ["main.a", "main.b[0]", "main.b[1]"])
        self.assertEqual([(s.key, s.wires) for s in W.input_signals(io)], [("a", [2]), ("b", [3, 4])])
        with self.assertRaises(ValueError):
            W.input_signals(R.MainIo([], [2, 3, 4], [], ["main.a", "main.b", "main.a"]))

    def test_sampling_is_deterministic_and_covers_boundaries(self) -> None:
        sigs = [W.InputSignal("a", [2]), W.InputSignal("b", [3, 4])]
        s1 = W.sample_inputs(sigs, P, "pkg", 60)
        self.assertEqual(s1, W.sample_inputs(sigs, P, "pkg", 60))
        self.assertNotEqual(s1, W.sample_inputs(sigs, P, "other", 60))
        values = {v for obj in s1 for vs in obj.values() for v in vs}
        self.assertTrue({"0", "1", str(P - 1)} <= values)
        self.assertTrue(any(obj["b"][0] != obj["b"][1] and obj["b"][0] in ("0", str(P - 1)) for obj in s1[20:]))
        self.assertEqual(W.sample_inputs([], P, "pkg", 1), [{}])

    def test_boundary_grid(self) -> None:
        sigs = [W.InputSignal("a", [2]), W.InputSignal("b", [3, 4])]
        grid = W.boundary_grid(sigs, P)
        self.assertEqual(len(grid), 27)
        self.assertIn({"a": ["0"], "b": ["1", str(P - 1)]}, grid)
        self.assertEqual(W.boundary_grid([W.InputSignal("x", list(range(2, 9)))], P), [])

    def test_mixed_phase_is_restricted_to_successful_uniform_strategies(self) -> None:
        sigs = [W.InputSignal("x", [2, 3, 4])]
        uniform = W.sample_inputs(sigs, P, "pkg", W.uniform_attempts())
        self.assertEqual(len(uniform), 2 * len(W.STRATEGIES))
        gen = [W.GeneratedWitness(obj, [1] if W.STRATEGIES[k % len(W.STRATEGIES)] in ("bit", "zero") else None, None)
               for k, obj in enumerate(uniform)]
        allowed = W.successful_strategies(gen)
        self.assertEqual(allowed, ["bit", "zero"])
        mixed = W.sample_inputs(sigs, P, "pkg/mixed", 50, allowed, uniform=False)
        self.assertTrue(all(v in ("0", "1") for obj in mixed for v in obj["x"]))

    def test_collect_real_deduplicates_and_mutants_carry_oracle_verdicts(self) -> None:
        r = bits_circuit(2)
        sigs = [W.InputSignal("in", [3])]
        gen = [W.GeneratedWitness({"in": ["1"]}, bits_witness(2, 1), None),
               W.GeneratedWitness({"in": ["1"]}, bits_witness(2, 1), None),
               W.GeneratedWitness({"in": ["9"]}, None, "Error: Assert Failed.\nat x"),
               W.GeneratedWitness({"in": ["2"]}, [1, 1, 1, 2], None)]
        acc, rej, errs = W.collect_real(r, sigs, gen, 10)
        self.assertEqual([w for _, w in acc], [bits_witness(2, 1)])
        self.assertEqual(len(rej), 1)
        self.assertEqual(errs, {"Error: Assert Failed.": 1})
        muts = W.mutants(r, [bits_witness(2, 1)], 5, "pkg")
        self.assertEqual(muts[0]["wire"], 0)
        self.assertTrue(all(m["oracle"] == R.satisfies(r, m["witness"]) for m in muts))


if __name__ == "__main__":
    unittest.main()
