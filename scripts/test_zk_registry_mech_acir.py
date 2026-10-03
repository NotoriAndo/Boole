#!/usr/bin/env python3
"""Battery P3 (mechanical propagation, Noir/ACIR): offline tests of the engine, the certificates, the package
model check, the Lean emission (text-level, no Lean) and the effective-population reader."""
from __future__ import annotations

import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from zk_registry import check as C                     # noqa: E402
from zk_registry import mech_acir as M                 # noqa: E402
from zk_registry import noir_acir as A                 # noqa: E402
from zk_registry import noir_det as D                  # noqa: E402
from zk_registry import noir_lean_emit as NE           # noqa: E402

P = A.BN254
FIX = Path(__file__).resolve().parents[1] / "fixtures" / "zk-registry" / "noir"


def X(lin=(), mul=(), const=0) -> A.Expr:
    """Expression from signed integer coefficients: lin = ((q, w), ...), mul = ((q, a, b), ...)."""
    return A.Expr(tuple((q % P, a, b) for q, a, b in mul), tuple((q % P, w) for q, w in lin), const % P)


def az(**kw) -> dict:
    return {"kind": "assert_zero", "expr": X(**kw)}


def rng(w: int, bits: int) -> dict:
    return {"kind": "range", "input": ("w", w), "bits": bits}


def brillig(*outs: int) -> dict:
    return {"kind": "brillig", "id": 0, "outputs": list(outs), "predicate": None}


def mk(ops: list[dict], inputs: list[int], outputs: list[int]) -> M.Pkg:
    n = 1 + max([*inputs, *outputs] + [A._max_witness(ops)])
    flat = A.Flat(ops, inputs, outputs, n)
    ns = "ZkDet.T"
    return M.make_pkg(flat, NE.emit_statement(ns, META, bool(A.bb_keys(ops))), {}, ns, "")


META = {"repo_id": "t/t", "instantiation": "f()", "path": "src/lib.nr"}


def det(pkg: M.Pkg) -> M.Result:
    return M.solve(pkg)


class PolyTests(unittest.TestCase):
    def test_certificate_uses_hp_for_multiples_of_p(self):
        x = M.Poly.atom("w 1")
        a = x * 3 - M.Poly.const(6)                    # 3x - 6 = 0
        cert = M.lc_cert(x - M.Poly.const(2), [(M.signed(M.inv(3)), "a", a)])
        self.assertIn("* a", cert)
        self.assertIn("mech_hp", cert)

    def test_certificate_rejects_a_wrong_combination(self):
        x = M.Poly.atom("w 1")
        with self.assertRaises(AssertionError):
            M.lc_cert(x, [(1, "a", x * 2)])

    def test_render(self):
        p = M.Poly.atom("w₁ 2") * M.Poly.atom("w₂ 3") * -5 + M.Poly.const(7)
        self.assertEqual(p.render(), "7 - 5 * w₁ 2 * w₂ 3")


class EngineTests(unittest.TestCase):
    def test_linear_chain_and_constant(self):
        ops = [az(lin=((1, 0), (-1, 1))), az(lin=((-1, 2),), const=5), az(mul=((1, 2, 1),), lin=((-1, 3),))]
        res = det(mk(ops, [0], [3]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.const, {2: 5})
        self.assertEqual([s.kind for s in res.steps], ["LIN", "CONST", "LIN"])

    def test_free_brillig_output_is_stuck(self):
        res = det(mk([brillig(1)], [0], [1]))
        self.assertEqual(res.status, "STUCK")
        self.assertTrue(res.stuck["brillig_dependent"])

    def test_square_root_is_not_determined(self):
        res = det(mk([brillig(1), az(mul=((1, 1, 1),), lin=((-1, 0),))], [0], [1]))
        self.assertEqual(res.status, "STUCK")
        self.assertIn("az-nonlinear", res.stuck["blocking"])

    def test_zero_coefficient_after_substitution(self):
        # w1 * w2 - w3 = 0 with w2 = 0 constant: w3 = 0 is constant, w1 stays free
        ops = [brillig(1), az(lin=((1, 2),)), az(mul=((1, 1, 2),), lin=((-1, 3),))]
        self.assertEqual(det(mk(ops, [0], [3])).status, "DETERMINED")
        self.assertEqual(det(mk(ops, [0], [1])).status, "STUCK")

    def split_ops(self, lo_bits=8, sign=-1):
        return [brillig(1, 2), rng(1, 8), rng(2, lo_bits), az(lin=((1, 0), (-256, 1), (sign, 2)))]

    def test_split_mixed_radix(self):
        res = det(mk(self.split_ops(), [0], [1, 2]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules.get("SPLIT"), 1)

    def test_split_overlapping_widths_or_mixed_signs_are_stuck(self):
        self.assertEqual(det(mk(self.split_ops(lo_bits=9), [0], [1, 2])).status, "STUCK")
        self.assertEqual(det(mk(self.split_ops(sign=1), [0], [1, 2])).status, "STUCK")

    def iszero_ops(self):
        # x = w0 - 7; w0·y - 7y + z - 1 = 0; w0·z - 7z = 0  (z = [x == 0])
        return [brillig(1), az(mul=((1, 0, 1),), lin=((-7, 1), (1, 2)), const=-1),
                az(mul=((1, 0, 2),), lin=((-7, 2),))]

    def test_iszero_gadget(self):
        res = det(mk(self.iszero_ops(), [0], [2]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules.get("ISZERO"), 1)
        self.assertEqual(det(mk(self.iszero_ops(), [0], [1])).status, "STUCK")    # the inverse hint is free at x = 0

    def test_iszero_needs_matching_coefficients(self):
        ops = self.iszero_ops()
        ops[2] = az(mul=((1, 0, 2),), lin=((-8, 2),))           # (x - 1)·z = 0: a different factor
        self.assertEqual(det(mk(ops, [0], [2])).status, "STUCK")

    def mem_ops(self, init_known=True):
        return [{"kind": "mem_init", "block": 0, "init": [0, 1 if init_known else 5], "block_type": "\"Memory\""},
                {"kind": "mem_op", "block": 0, "write": A.const_expr(1), "index": A.witness_expr(2),
                 "value": A.witness_expr(0), "predicate": None},
                {"kind": "mem_op", "block": 0, "write": A.const_expr(0), "index": A.witness_expr(3),
                 "value": A.witness_expr(4), "predicate": None}]

    def test_memory_list_mode(self):
        res = det(mk([brillig(4)] + self.mem_ops(), [0, 1, 2, 3], [4]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules.get("MEMREAD"), 1)
        stuck = det(mk([brillig(4, 5)] + self.mem_ops(init_known=False), [0, 1, 2, 3], [4]))
        self.assertEqual(stuck.status, "STUCK")
        self.assertIn("mem-undetermined", stuck.stuck["blocking"])

    def bb_op(self, pred=None):
        name = "Poseidon2Permutation"
        return {"kind": "bb", "name": name, "key": name, "inputs": [("w", 0), ("c", 3)],
                "outputs": [1, 2], "predicate": pred, "n_inputs": 2}

    def test_black_box_congruence(self):
        self.assertEqual(det(mk([self.bb_op()], [0], [1, 2])).status, "DETERMINED")
        self.assertEqual(det(mk([self.bb_op(("w", 0))], [0], [1, 2])).status, "STUCK")    # predicate may be 0
        ops = [az(lin=((1, 5),), const=-1), self.bb_op(("w", 5))]
        self.assertEqual(det(mk(ops, [0], [1, 2])).status, "DETERMINED")                 # constant 1 predicate

    def test_logic(self):
        op = {"kind": "and", "lhs": ("w", 0), "rhs": ("c", 15), "bits": 8, "output": 1}
        self.assertEqual(det(mk([op], [0], [1])).status, "DETERMINED")

    def fieldcut_ops(self, with_check=True):
        q0 = (P - 1) >> 32                                 # floor((p-1)/2^32)
        c3 = (1 << 32) - 1 - ((P - 1) % (1 << 32))
        ops = [brillig(2, 3, 5), rng(2, 222), rng(3, 32),
               az(lin=((1, 0), (-(1 << 32), 2), (-1, 3))),
               rng(4, 222), az(lin=((-1, 2), (-1, 4)), const=q0),
               az(mul=((-1, 2, 5),), lin=((q0, 5), (1, 6)), const=-1),
               az(mul=((-1, 2, 6),), lin=((q0, 6),))]
        if with_check:
            ops += [az(mul=((1, 3, 6),), lin=((c3, 6), (-1, 7))), rng(7, 32)]
        return ops + [az(lin=((1, 1), (-1, 3)))]

    def test_fieldcut(self):
        res = det(mk(self.fieldcut_ops(), [0], [1]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules.get("FIELDCUT"), 1)
        self.assertEqual(det(mk(self.fieldcut_ops(with_check=False), [0], [1])).status, "STUCK")

    def euclid_ops(self, bits=8):
        return [brillig(3, 4, 5), az(mul=((1, 1, 3),), const=-1), rng(4, bits), rng(5, bits),
                az(lin=((1, 1), (-1, 5), (-1, 6)), const=-1), rng(6, bits),
                az(mul=((-1, 1, 4),), lin=((1, 0), (-1, 5))), az(lin=((1, 2), (-1, 4)))]

    def test_euclid(self):
        res = det(mk(self.euclid_ops(), [0, 1], [2]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.rules.get("EUCLID"), 1)
        self.assertEqual(det(mk(self.euclid_ops(bits=128), [0, 1], [2])).status, "STUCK")   # d·q may exceed p

    def test_engine_facts_hold_on_a_satisfying_assignment(self):
        """CONST values equal a satisfying assignment's, and the fixture programs run through the engine."""
        ops = [az(lin=((1, 0), (-1, 1))), az(lin=((-1, 2),), const=5), az(mul=((1, 2, 1),), lin=((-1, 3),)),
               az(mul=((3, 2, 2),), lin=((-1, 4),))]
        w = [11, 11, 5, 55, 75]
        self.assertEqual(A.check(ops, w)[0], True)
        res = det(mk(ops, [0], [3, 4]))
        self.assertEqual(res.status, "DETERMINED")
        self.assertEqual(res.const, {2: 5, 4: 75})
        for x, v in res.const.items():
            self.assertEqual(w[x], v)
        for name in ("rc2_fx_logic", "rc2_fx_mem", "rc2_fx_bb", "rc2_fx_call", "rc2_fx_under", "old_fx_logic", "old_fx_mem"):
            flat = A.flatten(A.normalize_program(json.loads((FIX / f"dec_{name}.json").read_text())))
            res = det(M.make_pkg(flat))
            self.assertIn(res.status, ("DETERMINED", "STUCK"))
            for line in (FIX / f"ex_{name}.jsonl").read_text().splitlines():
                ex = json.loads(line)
                if ex["ok"]:
                    calls = [dict(c, witness=D.to_ints(c["witness"])) for c in ex["calls"]]
                    wv = A.flat_witness(flat, D.to_ints(ex["witness"]), calls)
                    self.assertTrue(all(wv[x] % P == v for x, v in res.const.items()), name)


class EmitTests(unittest.TestCase):
    def emit(self, pkg: M.Pkg) -> str:
        res = M.solve(pkg)
        self.assertEqual(res.status, "DETERMINED")
        return M.emit_solution(pkg, res)

    def assert_valid_submission(self, pkg: M.Pkg, text: str) -> tuple[str, str]:
        probs, aux, body = C.text_check(pkg.statement, text, "det")
        self.assertEqual(probs, [])
        self.assertEqual(C.forbidden_scan([("helpers", aux), ("proof", body)]), [])
        return aux, body

    def test_every_rule_emits_a_valid_submission(self):
        t = EngineTests()
        cases = [
            mk([az(lin=((1, 0), (-1, 1))), az(lin=((-1, 2),), const=5), az(mul=((1, 2, 1),), lin=((-1, 3),))], [0], [3]),
            mk(t.split_ops(), [0], [1, 2]),
            mk(t.iszero_ops(), [0], [2]),
            mk([brillig(4)] + t.mem_ops(), [0, 1, 2, 3], [4]),
            mk([t.bb_op()], [0], [1, 2]),
            mk([{"kind": "xor", "lhs": ("w", 0), "rhs": ("w", 1), "bits": 8, "output": 2}], [0, 1], [2]),
            mk(t.fieldcut_ops(), [0], [1]),
            mk(t.euclid_ops(), [0, 1], [2]),
        ]
        for pkg in cases:
            aux, body = self.assert_valid_submission(pkg, self.emit(pkg))
            self.assertIn("theorem mech_hp", aux)
            self.assertIn("refine List.forall_mem_cons.2", body)

    def test_memory_and_black_box_helpers_only_when_used(self):
        t = EngineTests()
        aux, _ = self.assert_valid_submission(*(lambda p: (p, self.emit(p)))(mk([t.bb_op()], [0], [1, 2])))
        self.assertNotIn("memRun", aux)
        self.assertIn("(bb : BlackBox)", aux)
        pkg = mk([brillig(4)] + t.mem_ops(), [0, 1, 2, 3], [4])
        aux, _ = self.assert_valid_submission(pkg, self.emit(pkg))
        self.assertIn("theorem mech_mr0", aux)
        self.assertIn("def mech_mb0_1", aux)

    def test_projection_paths_follow_blocks(self):
        ops = [az(lin=((1, k), (-1, k + 1))) for k in range(70)]
        pkg = mk(ops, [0], [70])
        self.assertEqual(M.path(pkg, 0), ".1.1")
        self.assertEqual(M.path(pkg, 63), ".1.2" + ".2" * 62)
        self.assertEqual(M.path(pkg, 64), ".2.1")
        self.assertEqual(M.path(pkg, 69), ".2" + ".2" * 5)
        logic = mk([{"kind": "and", "lhs": ("w", 0), "rhs": ("w", 1), "bits": 8, "output": 2}, az(lin=((1, 2), (-1, 3)))],
                   [0, 1], [3])
        self.assertEqual(M.path(logic, 0, 2), ".2.2.1")
        self.assertEqual(M.path(logic, 1), ".2.2.2")


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="mech-acir-test-")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def write_pkg(self, name: str, tamper: bool = False) -> str:
        decoded = json.loads((FIX / f"dec_{name}.json").read_text())
        flat = A.flatten(A.normalize_program(decoded))
        ns = "ZkDet.fx_" + name
        meta = {"repo_id": "t/t", "instantiation": "f()", "commit": "0" * 40, "path": "src/lib.nr", "template": "f",
                "rule": "parameter-free", "generator": M.ENGINE_VERSION, "repo_url": "https://example.invalid",
                "nargo_version": "x", "nargo_command": "nargo export", "acir_sha256": "0" * 64}
        model, info = NE.emit_model(ns, meta, flat, {})
        if tamper:
            model = model.replace(" = 0 ∧", " = 1 ∧", 1)
        d = os.path.join(self.tmp, name + ("-t" if tamper else ""))
        rel = NE.model_relpath(ns)
        os.makedirs(os.path.join(d, os.path.dirname(rel)))
        os.makedirs(os.path.join(d, "evidence"))
        Path(d, rel).write_text(model)
        Path(d, "Statement.lean").write_text(NE.emit_statement(ns, meta, info["bb"]))
        Path(d, "evidence", "acir.json").write_text(json.dumps(decoded))
        Path(d, "problem.json").write_text(json.dumps({"checker": {"statement_file": "Statement.lean", "files": [
            {"role": "import", "module": NE.model_module(ns), "path": rel}]}}))
        return d

    def test_load_checks_the_model(self):
        for name in ("rc2_fx_logic", "rc2_fx_mem", "rc2_fx_bb"):
            pkg = M.load(self.write_pkg(name))
            self.assertEqual(len(pkg.entries), len(NE.opcode_conjuncts(pkg.ops, pkg.keys)))
        with self.assertRaises(M.ModelMismatch):
            M.load(self.write_pkg("rc2_fx_mem", tamper=True))

    def test_engine_record(self):
        rec, pkg, res = M.engine_record(self.write_pkg("rc2_fx_logic"))
        self.assertIn(rec["engine"], ("DETERMINED", "STUCK"))
        bad, _, _ = M.engine_record(self.write_pkg("rc2_fx_bb", tamper=True))
        self.assertEqual(bad["engine"], "ERROR")

    def test_effective_population(self):
        root = self.tmp

        def index(coll: str, rows: list[dict]) -> None:
            os.makedirs(os.path.join(root, coll), exist_ok=True)
            Path(root, coll, "INDEX.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
            for r in rows:
                pid = r["package_id"]
                os.makedirs(os.path.join(root, coll, pid), exist_ok=True)
                Path(root, coll, pid, "problem.json").write_text("{}")

        index("noir-n1", [{"package_id": "a/x", "status": "OPEN"}, {"package_id": "a/y", "status": "TOO-LARGE"}])
        index("noir-n1/decomposition", [{"package_id": "a/z", "status": "OPEN"}])
        index("recovery-r1/noir", [{"package_id": "a/y2", "status": "OPEN", "instantiation": {"rule": "probed"},
                                    "evidence": {"supersedes_record": {"package_id": "a/y"}}}])
        eff = {r["package_id"]: r for r in M.noir_effective(root)}
        self.assertEqual(sorted(eff), ["a/x", "a/y2", "a/z"])
        self.assertEqual(eff["a/y2"]["collection"], "recovery-r1/noir")
        self.assertEqual(eff["a/y2"]["rule"], "probed")
        self.assertTrue(all(r["dir"] for r in eff.values()))


if __name__ == "__main__":
    unittest.main()
