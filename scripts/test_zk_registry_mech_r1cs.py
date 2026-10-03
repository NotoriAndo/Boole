#!/usr/bin/env python3
"""Battery P3 (mechanical propagation, R1CS family) tests: model parsing round trip, the propagation rules, a
brute-force soundness check over a small prime, and the submission text rules.  Lean is not needed."""
from __future__ import annotations

import itertools
import random
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import check as C          # noqa: E402
from zk_registry import lean_emit as E      # noqa: E402
from zk_registry import mech_r1cs as M      # noqa: E402
from zk_registry import r1cs as R           # noqa: E402

P = R.BN254_SCALAR
META = {"repo_id": "toy/repo", "instantiation": "Toy()", "generator": "gen v1", "repo_url": "https://example.invalid/toy",
        "commit": "0" * 40, "path": "toy.circom", "template": "Toy", "rule": "parameter-free",
        "circom_version": "2.2.3", "circom_flags": ["--r1cs", "--sym", "--wasm", "--O0"], "r1cs_sha256": "a" * 64,
        "prime_name": "bn128"}
NS = "ZkDet.toy"


def lc(*terms):
    """[(wire, coefficient)] with coefficients reduced mod P (the R1CS file representation)."""
    return [(w, k % P) for w, k in terms]


def package_texts(cons, n_wires, inputs, outputs, extra_model: str = "", theorem: str = "det"):
    r = R.R1cs(P, 32, n_wires, len(outputs), len(inputs), 0, n_wires, cons)
    names = ["one"] + [f"main.s{i}" for i in range(1, n_wires)]
    model = E.emit_model(NS, META, r, outputs, inputs, names)
    if extra_model:
        model = model.replace(f"\nend {NS}\n", "\n" + extra_model + f"\nend {NS}\n")
    stmt = E.emit_statement(NS, META)
    if theorem == "det_mod":
        stmt = stmt.replace("theorem det [Fact (Nat.Prime p)] :\n    ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →",
                            "theorem det_mod [Fact (Nat.Prime p)] :\n    ∀ f : ℕ → List F → List F, ∀ w₁ w₂ : Fin nWires → F, "
                            "Constraints w₁ → Constraints w₂ →\n      CallsRespect f w₁ → CallsRespect f w₂ →")
    return model, stmt


def build(cons, n_wires, inputs, outputs, **kw) -> tuple[M.Model, str]:
    model, stmt = package_texts(cons, n_wires, inputs, outputs, **kw)
    return M.parse_model(model, stmt), stmt


# Num2Bits(3) over wires: 1 = in, 2..4 = bits (outputs)
NUM2BITS = [(lc((0, -1), (k, 1)), lc((k, 1)), []) for k in (2, 3, 4)] + [([], [], lc((2, 1), (3, 2), (4, 4), (1, -1)))]
# IsZero: 1 = in, 2 = inv (hint), 3 = out
ISZERO = [(lc((1, -1)), lc((2, 1)), lc((0, -1), (3, 1))), (lc((1, 1)), lc((3, 1)), [])]


class ParseTests(unittest.TestCase):
    def test_linear_terms_round_trip(self) -> None:
        for text in ["w 3", "-w 3", "5 * w 3", "-(5 * w 3)", "w 1 - 2 * w 2 + w 7", "-(7 * w 0) + w 2 - w 4"]:
            parsed = M.parse_lc(text, P)
            self.assertEqual(E.render_lc([(w, k % P) for w, k in parsed], P), text)
        self.assertEqual(M.parse_lc("0", P), [])
        self.assertEqual(M.parse_lc("-(5 * w 3) + w 4", P), [(3, -5), (4, 1)])

    def test_constraint_forms(self) -> None:
        c = M.parse_constraint(0, "(2 * w 4) * w 6 = 3 * w 5", P)
        self.assertEqual((c.a, c.b, c.c, c.lhs_zero), ({4: 2}, {6: 1}, {5: 3}, False))
        c = M.parse_constraint(1, "0 = w 1 - w 2", P)
        self.assertTrue(c.lhs_zero)
        self.assertEqual(c.c, {1: 1, 2: -1})
        with self.assertRaises(M.ModelFormatError):
            M.parse_constraint(2, "(w 1) * w 2 = w 3", P)          # not the emitter's rendering

    def test_unblocked_and_blocked_models(self) -> None:
        m, _ = build(NUM2BITS, 5, [1], [2, 3, 4])
        self.assertEqual((m.inputs, m.outputs, len(m.cons), m.blocks), ([1], [2, 3, 4], 4, []))
        chain = [([], [], lc((k, 1), (k + 1, -1))) for k in range(1, 70)]
        m, _ = build(chain, 71, [1], [70])
        self.assertEqual([len(b) for b in m.blocks], [64, 5])
        self.assertEqual(m.cons[65].c, {66: 1, 67: -1})

    def test_det_mod_and_emulated_lists(self) -> None:
        calls = ("def Calls : List (ℕ × List (Fin nWires) × List (Fin nWires)) := [(0, [1, 2], [3]), (1, [3], [4, 5])]\n\n"
                 "def CallsRespect (f : ℕ → List F → List F) (w : Fin nWires → F) : Prop :=\n"
                 "  ∀ c ∈ Calls, c.2.2.map w = f c.1 (c.2.1.map w)\n")
        m, _ = build([([], [], lc((5, 1), (6, -1)))], 7, [1, 2], [6], extra_model=calls, theorem="det_mod")
        self.assertEqual((m.theorem, m.calls), ("det_mod", [(0, [1, 2], [3]), (1, [3], [4, 5])]))
        em = "def EmulatedOutputs : List (List (Fin nWires) × ℕ × ℕ) := [([2, 3], 64, 97), ([4], 8, 7)]\n"
        m, _ = build([([], [], lc((1, 1), (2, -1)))], 5, [1], [], extra_model=em)
        self.assertEqual(m.emulated, [([2, 3], 64, 97), ([4], 8, 7)])


class PropagationTests(unittest.TestCase):
    def test_linear_chain_and_products(self) -> None:
        cons = [([], [], lc((1, 1), (3, -1))), (lc((3, 1)), lc((2, 1)), lc((4, 1))), (lc((0, 2)), lc((4, 1)), lc((5, 1)))]
        m, _ = build(cons, 6, [1, 2], [5])
        res = M.propagate(m)
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual([(s.rule, s.wires, s.info["mode"]) for s in res.steps],
                         [("lin", [3], "lin0"), ("lin", [4], "prod"), ("lin", [5], "rw")])

    def test_bit_decomposition_and_width_limit(self) -> None:
        m, _ = build(NUM2BITS, 5, [1], [2, 3, 4])
        res = M.propagate(m)
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual((res.steps[0].rule, res.steps[0].wires, res.steps[0].info["n"]), ("bits", [2, 3, 4], 3))
        n = 254                                       # 2^254 > p: Num2Bits(254) aliases, not determined
        cons = [(lc((0, -1), (k, 1)), lc((k, 1)), []) for k in range(2, n + 2)]
        cons.append(([], [], lc(*[(k, 2 ** (k - 2)) for k in range(2, n + 2)], (1, -1))))
        m, _ = build(cons, n + 2, [1], list(range(2, n + 2)))
        res = M.propagate(m)
        self.assertEqual((res.status, res.stuck["primary"]), ("STUCK", "bits-too-wide"))

    def test_is_zero_gadget(self) -> None:
        m, _ = build(ISZERO, 4, [1], [3])
        self.assertEqual(M.propagate(m, M.CORE_RULES).status, "STUCK")
        res = M.propagate(m)
        self.assertEqual((res.status, [s.rule for s in res.steps]), ("DETERMINED", ["isz"]))

    def test_linear_elimination(self) -> None:
        # x + y = a, x - y = b: no single-unknown constraint; elimination determines both
        cons = [([], [], lc((3, 1), (4, 1), (1, -1))), ([], [], lc((3, 1), (4, -1), (2, -1)))]
        m, _ = build(cons, 5, [1, 2], [3, 4])
        self.assertEqual(M.propagate(m, ("lin", "bits", "call", "isz")).status, "STUCK")
        res = M.propagate(m)
        self.assertEqual(res.status, "DETERMINED")
        self.assertIn("elim", {s.rule for s in res.steps})
        # Num2Bits through accumulator signals: acc0 = b0 * b0, acc1 = acc0 + 2 b1, acc2 = acc1 + 4 b2 = in
        bits = [(lc((0, -1), (k, 1)), lc((k, 1)), []) for k in (2, 3, 4)]
        chain = [(lc((2, -1)), lc((2, 1)), lc((5, -1))), ([], [], lc((3, 2), (5, 1), (6, -1))),
                 ([], [], lc((4, 4), (6, 1), (7, -1))), ([], [], lc((7, 1), (1, -1)))]
        m, _ = build(bits + chain, 8, [1], [2, 3, 4])
        res = M.propagate(m)
        self.assertEqual(res.status, "DETERMINED")
        self.assertIn("bits", [s.info.get("kind") for s in res.steps if s.rule == "elim"])

    def test_stuck_classes(self) -> None:
        m, _ = build([(lc((1, 1)), lc((2, 1)), lc((0, 1)))], 4, [1], [2, 3])      # x * u = 1 and a free output
        res = M.propagate(m)
        self.assertEqual(res.status, "STUCK")
        self.assertEqual((res.stuck["primary"], res.stuck["free_outputs"], res.undetermined_outputs),
                         ("output-unconstrained", 1, [2, 3]))
        self.assertEqual(res.stuck["classes"], {"nonconst-coef": 1})

    def test_det_mod_calls(self) -> None:
        calls = ("def Calls : List (ℕ × List (Fin nWires) × List (Fin nWires)) := [(0, [3], [4])]\n\n"
                 "def CallsRespect (f : ℕ → List F → List F) (w : Fin nWires → F) : Prop :=\n"
                 "  ∀ c ∈ Calls, c.2.2.map w = f c.1 (c.2.1.map w)\n")
        cons = [([], [], lc((1, 1), (3, -1))), ([], [], lc((4, 1), (2, -1)))]
        m, _ = build(cons, 5, [1], [2], extra_model=calls, theorem="det_mod")
        res = M.propagate(m)
        self.assertEqual([(s.rule, s.wires) for s in res.steps], [("lin", [3]), ("call", [4]), ("lin", [2])])
        self.assertEqual(M.propagate(m, ("lin", "bits")).status, "STUCK")


class SoundnessTests(unittest.TestCase):
    """DETERMINED must imply DET: brute force over GF(11) on random small systems (2^3 < 11 for the bits rule)."""

    Q = 11

    def det_holds(self, cons, n_wires, inputs, outputs) -> bool:
        q, seen = self.Q, {}
        ev = lambda f, w: sum(k * w[i] for i, k in f.items()) % q            # noqa: E731
        for vals in itertools.product(range(q), repeat=n_wires - 1):
            w = (1,) + vals
            if all((ev(a, w) * ev(b, w) - ev(c, w)) % q == 0 for a, b, c in cons):
                key, out = tuple(w[i] for i in inputs), tuple(w[o] for o in outputs)
                if seen.setdefault(key, out) != out:
                    return False
        return True

    def random_system(self, rng: random.Random):
        n = 5
        pick = lambda: {w: rng.choice([1, -1, 2, 3]) for w in rng.sample(range(n), rng.randint(1, 2))}  # noqa: E731
        cons = []
        for _ in range(rng.randint(1, 4)):
            kind = rng.random()
            if kind < 0.3:
                cons.append(({}, {}, pick()))
            elif kind < 0.5:
                b = rng.randrange(1, n)
                cons.append(({0: -1, b: 1}, {b: 1}, {}))
            elif kind < 0.6:
                x, v, o = rng.sample(range(1, n), 3)
                cons += [({x: -1}, {v: 1}, {0: -1, o: 1}), ({x: 1}, {o: 1}, {})]
            else:
                cons.append((pick(), pick(), pick()))
        inputs = sorted(rng.sample(range(1, n), rng.randint(1, 2)))
        outputs = sorted(rng.sample([w for w in range(1, n) if w not in inputs], 1))
        return cons, n, inputs, outputs

    def test_determined_implies_det(self) -> None:
        rng = random.Random(20261003)
        checked = 0
        for _ in range(400):
            cons, n, inputs, outputs = self.random_system(rng)
            m = M.Model(NS, "det", self.Q, n, inputs, outputs,
                        [M.Con(j, a, b, c, "", not a or not b) for j, (a, b, c) in enumerate(cons)], [])
            if M.propagate(m).status == "DETERMINED":
                checked += 1
                self.assertTrue(self.det_holds(cons, n, inputs, outputs), (cons, inputs, outputs))
        self.assertGreater(checked, 40)


class EmissionTests(unittest.TestCase):
    def submission(self, cons, n_wires, inputs, outputs, **kw) -> tuple[str, str, dict]:
        m, stmt = build(cons, n_wires, inputs, outputs, **kw)
        res = M.propagate(m)
        self.assertEqual(res.status, "DETERMINED")
        text, stats = M.emit_solution(m, stmt, res)
        return text, stmt, stats

    def assert_checker_text_rules(self, stmt: str, text: str, thm: str = "det") -> str:
        probs, aux, body = C.text_check(stmt, text, thm)
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helpers", aux), ("proof", body)]), [])
        return body

    def test_bits_and_linear_proof_text(self) -> None:
        cons = NUM2BITS + [([], [], lc((5, 1), (2, -1), (3, -1)))]
        text, stmt, stats = self.submission(cons, 6, [1], [5])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("theorem mech_lsum_inj", text)
        self.assertIn("mech_lsum_inj 3 rfl rfl hn3", body)
        self.assertIn("have e2 : w₁ 2 = w₂ 2 := congrArg (fun l => List.getD l 0 0) q3", body)
        self.assertIn("have e5 : w₁ 5 = w₂ 5 := by linear_combination", body)
        self.assertTrue(body.rstrip().endswith("exact List.forall_mem_nil _"))
        self.assertNotIn("(by ", body)                # no tactic blocks nested in terms (slow elaboration)
        self.assertGreater(stats["verified_identities"], 0)

    def test_product_constraint_uses_known_factors(self) -> None:
        cons = [(lc((1, 1), (2, 1)), lc((1, 1), (2, -1)), lc((3, 1)))]
        text, stmt, _ = self.submission(cons, 4, [1, 2], [3])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("have hA0 : (w₁ 1 + w₁ 2) = (w₂ 1 + w₂ 2) := by linear_combination e1 + e2", body)
        self.assertIn("have hm0 := a0.symm.trans ((congr (congrArg HMul.hMul hA0) hB0).trans b0)", body)

    def test_is_zero_and_det_mod_text(self) -> None:
        text, stmt, _ = self.submission(ISZERO, 4, [1], [3])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("theorem mech_isz", text)
        self.assertIn("mech_isz hz0_1 hz0_2 hz0_3 hz0_4", body)
        calls = ("def Calls : List (ℕ × List (Fin nWires) × List (Fin nWires)) := [(0, [3], [4])]\n\n"
                 "def CallsRespect (f : ℕ → List F → List F) (w : Fin nWires → F) : Prop :=\n"
                 "  ∀ c ∈ Calls, c.2.2.map w = f c.1 (c.2.1.map w)\n")
        cons = [([], [], lc((1, 1), (3, -1))), ([], [], lc((4, 1), (2, -1)))]
        text, stmt, _ = self.submission(cons, 5, [1], [2], extra_model=calls, theorem="det_mod")
        body = self.assert_checker_text_rules(stmt, text, "det_mod")
        self.assertTrue(body.startswith(" by\n  intro f w₁ w₂ h₁ h₂ hc₁ hc₂ hin\n"))
        self.assertIn("have r₁ : [w₁ 4] = f 0 [w₁ 3] := hc₁ _", body)

    def test_long_input_lists_are_peeled_once(self) -> None:
        n = 40                                        # wires 1..40 inputs, 41 = their sum
        cons = [([], [], lc(*[(k, 1) for k in range(1, n + 1)], (n + 1, -1)))]
        text, stmt, _ = self.submission(cons, n + 2, list(range(1, n + 1)), [n + 1])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("unfold Inputs at hin", body)
        self.assertIn("have hi0 := List.forall_mem_cons.1 hin", body)
        self.assertIn("have e40 : w₁ 40 = w₂ 40 := hi39.1", body)
        self.assertNotIn("List.getElem_mem", body)

    def test_identity_check_rejects_a_wrong_combination(self) -> None:
        m, _ = build([([], [], lc((1, 1), (2, -1)))], 3, [1], [2])
        em = M.Emitter(m, M.propagate(m))
        em.e(1)
        with self.assertRaises(AssertionError):
            em.derive("e2", "w₁ 2 = w₂ 2", "w₁ 2", "w₂ 2", M.pE(2), [(2, "e1")], 1)

    def test_elimination_proof_text(self) -> None:
        cons = [([], [], lc((3, 1), (4, 1), (1, -1))), ([], [], lc((3, 1), (4, -1), (2, -1)))]
        text, stmt, stats = self.submission(cons, 5, [1, 2], [3, 4])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("hP", body)                     # the 1/2 of the combination goes through (p : F) = 0
        bits = [(lc((0, -1), (k, 1)), lc((k, 1)), []) for k in (2, 3, 4)]
        chain = [(lc((2, -1)), lc((2, 1)), lc((5, -1))), ([], [], lc((3, 2), (5, 1), (6, -1))),
                 ([], [], lc((4, 4), (6, 1), (7, -1))), ([], [], lc((7, 1), (1, -1)))]
        text, stmt, _ = self.submission(bits + chain, 8, [1], [2, 3, 4])
        body = self.assert_checker_text_rules(stmt, text)
        self.assertIn("mech_lsum_inj 3 rfl rfl hn3", body)

    def test_only_determined_results_are_emitted(self) -> None:
        m, stmt = build([(lc((1, 1)), lc((2, 1)), lc((0, 1)))], 3, [1], [2])
        with self.assertRaises(ValueError):
            M.emit_solution(m, stmt, M.propagate(m))


if __name__ == "__main__":
    unittest.main()
