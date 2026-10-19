"""Kernel non-vacuity proves every premise of the same concrete real witness."""

import sys, unittest
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from zk_registry import kernel_nonvac as K


class KernelNonvacTests(unittest.TestCase):
    def test_every_premise_is_in_the_kernel_target(self):
        text = K.proof_source(["Model"], "def w : Nat := 7", ["w = 7", "w < 8"])
        self.assertIn("(w = 7) ∧ (w < 8)", text)
        self.assertIn(":= by decide", text)
        self.assertNotIn("#eval", text)
        self.assertNotIn("native_decide", text)

    def test_empty_premises_are_unknown_not_a_true_witness(self):
        with self.assertRaises(ValueError):
            K.proof_source([], "def w := 0", [])

    def test_reference_and_solution_have_identical_statement(self):
        a = K.proof_source([], "def w : Nat := 0", ["w = 0"])
        b = K.proof_source([], "def w : Nat := 0", ["w = 0"], reference=True)
        self.assertEqual(a.replace("by decide", "by sorry"), b)


class CatalogWitnessPlanTests(unittest.TestCase):
    def fixture(self, root, kind="soundness"):
        import hashlib, json

        files = {
            "Model.lean": "namespace Model\nend Model\n",
            "Candidate.lean": "-- specification\n",
            "Bindings.lean": "-- bindings\n",
            "Statements.lean": "-- statements\n",
            "EvaluationSupport.lean": "-- decider\n",
        }
        rows = []
        for name, text in files.items():
            (root / name).write_text(text)
            rows.append(
                dict(path=name, module=name[:-5], sha256=hashlib.sha256(text.encode()).hexdigest())
            )
        witness = b"1\n0\n"
        (root / "witness.txt").write_bytes(witness)
        wh = hashlib.sha256(witness).hexdigest()
        binding = dict(
            id="Example", model_module="Model", model_file="Model.lean", n_wires=2, native_prime=7
        )
        bh = hashlib.sha256(
            json.dumps(binding, sort_keys=True, separators=(",", ":")).encode()
        ).hexdigest()
        evidence = dict(
            actual_solver_assignment=True,
            completed=True,
            witness_sha256=wh,
            binding_sha256=bh,
            rows=[
                dict(
                    witness_sha256=wh,
                    valid_length=True,
                    Constraints=True,
                    Assumptions=True,
                    CanonicalInputs=True,
                    SpecDomain=True,
                    AcceptedData=True,
                    BindInputs=True,
                )
            ],
        )
        data = json.dumps(evidence).encode()
        (root / "evidence.json").write_bytes(data)
        return (
            dict(
                kind="C",
                property=kind,
                binding=binding,
                checker=dict(files=rows),
                nonvacuity=dict(
                    status="NONVACUOUS",
                    file="evidence.json",
                    sha256=hashlib.sha256(data).hexdigest(),
                    witness_file="witness.txt",
                    witness_sha256=wh,
                ),
            ),
            files,
        )

    def plan(self, root, meta, files):
        from unittest.mock import patch

        fn = getattr(K, "c_plan", None)
        self.assertTrue(callable(fn), "missing authentic catalog witness adapter")
        with patch(
            "zk_registry.correctness_gnark.render_modules",
            return_value={x: files[x] for x in ["Bindings.lean", "Statements.lean"]},
        ), patch(
            "zk_registry.correctness_gnark.evaluation_support",
            return_value=files["EvaluationSupport.lean"],
        ):
            return fn(str(root), meta)

    def test_real_witness_and_every_soundness_premise(self):
        import tempfile

        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            meta, files = self.fixture(root)
            p = self.plan(root, meta, files)
            self.assertEqual(len(p["premises"]), 4)
            self.assertIn("Model.Constraints BooleNonvac.w", p["premises"])
            self.assertIn("Example.SpecDomain BooleNonvac.w", p["premises"])
            self.assertIn("#[1, 0]", p["witness_definition"])

    def test_completeness_uses_same_witness_and_input(self):
        import tempfile

        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            meta, files = self.fixture(root, "completeness")
            p = self.plan(root, meta, files)
            self.assertEqual(len(p["premises"]), 5)
            self.assertIn("Example.AcceptedData BooleNonvac.data", p["premises"])
            self.assertIn("Example.BindInputs BooleNonvac.w BooleNonvac.data", p["premises"])

    def test_changed_source_or_witness_is_rejected(self):
        import tempfile

        for name in ["witness.txt", "Model.lean", "Bindings.lean", "EvaluationSupport.lean"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as t:
                root = Path(t)
                meta, files = self.fixture(root)
                (root / name).write_text("tampered")
                with self.assertRaises(ValueError):
                    self.plan(root, meta, files)

    def test_relabelled_contract_and_non_solver_witness_rejected(self):
        import tempfile, hashlib, json

        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            meta, files = self.fixture(root)
            meta["binding"]["id"] = "Other"
            with self.assertRaises(ValueError):
                self.plan(root, meta, files)
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            meta, files = self.fixture(root)
            e = json.loads((root / "evidence.json").read_text())
            e["actual_solver_assignment"] = False
            raw = json.dumps(e).encode()
            (root / "evidence.json").write_bytes(raw)
            meta["nonvacuity"]["sha256"] = hashlib.sha256(raw).hexdigest()
            with self.assertRaises(ValueError):
                self.plan(root, meta, files)


class CatalogStageTests(unittest.TestCase):
    def test_unsupported_catalog_item_is_unknown_without_compilation(self):
        from unittest.mock import patch

        fn = getattr(K, "run_c", None)
        self.assertTrue(callable(fn), "missing bounded catalog stage")
        with patch.object(K.L, "compile_module") as compile:
            result = fn(
                None,
                ".",
                dict(kind="ratchet"),
                ".",
                None,
                type("Boundary", (), {"mode": "dev-unsandboxed"})(),
            )
        self.assertEqual(result["status"], "UNKNOWN")
        compile.assert_not_called()


class KernelStageBoundaries(unittest.TestCase):
    def test_unadmitted_axioms_cannot_produce_a_nonvacuity_pass(self):
        from unittest.mock import patch

        with patch.object(K.L, "compile_module") as compile:
            result = K.run(
                None,
                [],
                "def w := 0",
                ["w = 0"],
                ".",
                None,
                type("Boundary", (), {"mode": "dev-unsandboxed"})(),
                axioms=["oracle"],
            )
        self.assertEqual(
            (result["status"], result["reason"]), ("UNKNOWN", "measuring-stick-axioms-unsupported")
        )
        compile.assert_not_called()

    def test_cleanup_time_is_part_of_n4(self):
        from unittest.mock import patch
        from zk_registry.sandbox import Sandbox
        import tempfile

        clock = [0.0]
        compiled = K.L.RunResult(0, "", 0, False)

        def compare(*a, **k):
            clock[0] = 11.0
            return dict(verdict="PASS", reason=None)

        with tempfile.TemporaryDirectory() as t, patch.object(
            K.time, "monotonic", side_effect=lambda: clock[0]
        ), patch.object(K.L, "compile_module", return_value=(compiled, [])), patch.object(
            K.D,
            "export_module",
            return_value=dict(verdict="PASS", path="owned", statement_closure_sha256="a" * 64),
        ), patch.object(
            K.D, "compare_exports", side_effect=compare
        ):
            r = K.run(
                None,
                [],
                "def w := 0",
                ["w = 0"],
                str(Path(t) / "unit"),
                None,
                Sandbox.development(),
                timeout=10,
            )
        self.assertEqual((r["status"], r["reason"]), ("UNKNOWN", "N4-resource-limit"))
        self.assertEqual(r["seconds"], 11.0)


if __name__ == "__main__":
    unittest.main()
