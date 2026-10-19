"""Behavioral regressions for trusted statement-identity wiring."""

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

source = os.environ.get("BOOLE_TEST_SOURCE_ROOT")
if source:
    sys.path.insert(0, str(Path(source) / "scripts"))
from zk_registry import closure as D, correctness_runtime as CR, lean_runner as L
from zk_registry.sandbox import Sandbox


class ChallengeOnlyIdentity(unittest.TestCase):
    def test_accepted_solution_above_parser_record_cap_reaches_comparator(self):
        # Pilot 0015 has 4,045,979 records. The worker refusal models its real
        # MAX_RECORDS failure; the trusted Comparator accepts this export.
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = L.LeanEnv("/usr", [], "v4.34.1", {}, "", str(root))
            tools = D.Tools("/bin/true", "/bin/true", "/bin/true", "/usr", None)
            boundary = Sandbox.fixture()
            with patch.object(
                CR, "runner", return_value=lambda *args: L.RunResult(0, "accepted export", 0, False)
            ), patch.object(
                D,
                "statement_identity",
                return_value={"verdict": "ERROR", "reason": "statement-identity-unavailable"},
            ) as worker:
                exported = D.export_module(
                    env,
                    "ProofMod",
                    "Fixture.goal",
                    str(root / "sealed" / "Solution.ndjson"),
                    tools,
                    boundary,
                    str(root),
                    [],
                    10,
                    [],
                )
            self.assertEqual(exported["verdict"], "PASS")
            worker.assert_not_called()
            with patch.object(
                CR,
                "runner",
                return_value=lambda *args: L.RunResult(
                    0,
                    "Lean default kernel accepts the solution\nnanoda kernel accepts the solution",
                    0,
                    False,
                ),
            ):
                compared = D.compare_exports(
                    exported["path"],
                    exported["path"],
                    "Fixture.goal",
                    tools,
                    boundary,
                    str(root / "compare"),
                    10,
                    [],
                )
            self.assertEqual(compared["verdict"], "PASS")


class ReferenceAndSerialization(unittest.TestCase):
    def test_reference_export_keeps_full_base_timeout(self):
        from zk_registry import check as C

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "Statement.lean").write_text("theorem goal : True := by sorry\n")
            compiled = dict(compile_rc=0, post=dict(replay="ok"), compile_secs=3)
            ticks = iter([0.0, 3.0, 3.0, 3.0])
            with patch.object(C, "compile_in_sandbox", return_value=compiled), patch.object(
                D, "export_module", return_value={}
            ) as export, patch.object(D.time, "monotonic", side_effect=lambda: next(ticks, 3.0)):
                D.prepare_reference(
                    None,
                    {"checker": {"statement_file": "Statement.lean", "theorem_fqn": "goal"}},
                    temp,
                    str(root / "work"),
                    str(root / "sealed"),
                    None,
                    Sandbox.fixture(),
                    30,
                    [],
                )
            self.assertEqual(export.call_args.args[8], 30)

    def test_write_json_defaults_null_without_mutating_input(self):
        from zk_registry import package as P

        obj = {"schema_version": "zk-registry-test-problem/v1"}
        original = dict(obj)
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "problem.json"
            P.write_json(str(target), obj)
            self.assertIsNone(json.loads(target.read_text())["statement_closure_sha256"])
        self.assertEqual(obj, original)

    def test_free_true_premise_is_not_an_authenticated_stage(self):
        from zk_registry import gates as G, kernel_nonvac as K

        compiled = L.RunResult(0, "", 0, False)
        with tempfile.TemporaryDirectory() as temp, patch.object(
            L, "compile_module", return_value=(compiled, [])
        ), patch.object(
            D,
            "export_module",
            return_value=dict(verdict="PASS", path="export", statement_closure_sha256="a" * 64),
        ), patch.object(
            D, "compare_exports", return_value=dict(verdict="PASS", reason=None)
        ):
            with self.assertRaises(TypeError):
                G.g_nonvac_k(
                    None, [], "", ["True"], str(Path(temp) / "unit"), None, Sandbox.fixture()
                )


class ReferenceIdentityPropagation(unittest.TestCase):
    def test_successful_elaboration_returns_real_challenge_identity(self):
        from zk_registry import gates as G

        compiled = L.RunResult(0, "", 0, False)
        post = dict(
            replay="ok",
            target={"kind": "theorem"},
            axioms=["sorryAx"],
            type_str="True",
            type_sha256="b" * 64,
        )
        with tempfile.TemporaryDirectory() as temp, patch.object(
            L,
            "compile_module",
            side_effect=[
                (compiled, []),
                (compiled, [{"severity": "warning", "data": "declaration uses sorry"}]),
            ],
        ), patch.object(G, "run_replay", return_value=(compiled, post)), patch.object(
            D, "export_module", return_value=dict(verdict="PASS", statement_closure_sha256="a" * 64)
        ) as export:
            env = L.LeanEnv(
                "/usr",
                [],
                "v4.34.1",
                {},
                "",
                temp,
                {
                    "acceptance_tools": D.Tools(
                        "/bin/true", "/bin/true", "/bin/true", "/usr", None
                    ),
                    "acceptance_sandbox": Sandbox.fixture(),
                },
            )
            result = G.g_elab(env, temp, temp, "Fixture", temp)
        self.assertEqual(result.status, "PASS")
        self.assertEqual(result.detail.get("statement_closure_sha256"), "a" * 64)
        self.assertTrue(export.call_args.kwargs["identity"])

    def test_same_name_same_text_collision_is_separated_in_catalog(self):
        import hashlib
        from zk_registry import service as S, package as P, acceptance as A

        fixtures = Path(__file__).parent / "tests" / "statement-identity"
        entries = []
        for label in ["original", "changed"]:
            problem = dict(
                schema_version=P.CORRECTNESS_GNARK_SCHEMA_VERSION,
                status="OPEN",
                package_id="contract",
                statement={"text": "theorem PublicClosureFixture.same : payload = payload := by sorry"},
                checker={"theorem_fqn": "PublicClosureFixture.same"},
            )
            root = fixtures / label
            export = root / "Challenge.ndjson"
            binding = A.RootBinding(
                "contract",
                {
                    "challenge_export": {
                        "path": "Challenge.ndjson",
                        "sha256": hashlib.sha256(export.read_bytes()).hexdigest(),
                    }
                },
                "b" * 64,
            )
            with patch.object(P, "validate_problem", return_value=[]), patch.object(
                A, "read", return_value=binding
            ), patch.object(A, "public", return_value={}):
                entries.append(S.catalog_entry(problem, str(root)))
        self.assertEqual(entries[0]["statement_sha256"], entries[1]["statement_sha256"])
        self.assertNotEqual(
            entries[0].get("statement_closure_sha256"), entries[1].get("statement_closure_sha256")
        )


class ContractAgreement(unittest.TestCase):
    def test_real_statements_and_joint_witness_have_identical_antecedents(self):
        import re
        from zk_registry import correctness_gnark as CG
        from test_zk_registry_correctness_gnark import unit_fixture

        unit = unit_fixture()
        statements = CG.render_modules(unit)["Statements.lean"]
        for kind, title in [("soundness", "Soundness"), ("completeness", "Completeness")]:
            section = statements.split("def " + title + " : Prop :=", 1)[1].split("\n\n", 1)[0]
            section = section.split("outputValues", 1)[0]
            names = re.findall(
                r"\b(Constraints|Assumptions|CanonicalInputs|SpecDomain|CanonicalData|AcceptedData|BindInputs)\b",
                section,
            )
            premises = CG.contract_premises(unit, kind)
            actual = [term.split()[0].rsplit(".", 1)[1] for term in premises]
            self.assertEqual(actual, names)
            self.assertNotIn("OutputLimbsBounded", actual)


class CheckerIdentityWiring(unittest.TestCase):
    def test_mismatch_stops_submission_and_equal_digest_passes_through(self):
        from zk_registry import check as C
        from test_zk_registry_package import make_package
        from test_zk_registry_check import fake_env, submission

        for advertised in ["a" * 64, "b" * 64]:
            with self.subTest(advertised=advertised), tempfile.TemporaryDirectory() as temp:
                package, metadata = make_package(temp)
                metadata["statement_closure_sha256"] = advertised
                Path(package, "problem.json").write_text(json.dumps(metadata))
                proof = Path(temp) / "Solution.lean"
                proof.write_text(
                    submission(Path(package, "Statement.lean").read_text(), "  trivial\n")
                )
                env = fake_env(temp)
                with patch.object(
                    D,
                    "prepare_reference",
                    return_value=dict(verdict="PASS", statement_closure_sha256="a" * 64),
                ), patch.object(
                    C, "compile_in_sandbox", return_value={"error": "intentional-stop"}
                ) as compile:
                    result = C.check(
                        package,
                        str(proof),
                        env,
                        policy_fixture=env.pins(),
                        sandbox=Sandbox.fixture(),
                        tools=D.Tools("", "", "", "", None),
                    )
                self.assertEqual(result["statement_closure_sha256"], "a" * 64)
                if advertised == "a" * 64:
                    compile.assert_called_once()
                    self.assertNotIn("statement-closure-mismatch", result["invalid"])
                else:
                    compile.assert_not_called()
                    self.assertEqual(result["verdict"], "INVALID")
                    self.assertIn("statement-closure-mismatch", result["invalid"])

    def test_challenge_identity_failure_is_error(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            env = L.LeanEnv("/usr", [], "v4.34.1", {}, "", temp)
            with patch.object(
                CR, "runner", return_value=lambda *args: L.RunResult(0, "export", 0, False)
            ), patch.object(
                D,
                "statement_identity",
                return_value=dict(verdict="ERROR", reason="statement-identity-unavailable"),
            ):
                result = D.export_module(
                    env,
                    "ProofMod",
                    "goal",
                    str(root / "sealed" / "Challenge.ndjson"),
                    D.Tools("", "", "", "", None),
                    Sandbox.fixture(),
                    temp,
                    [],
                    10,
                    [],
                    identity=True,
                )
            self.assertEqual(
                (result["verdict"], result["reason"]), ("ERROR", "statement-identity-unavailable")
            )

    def test_exposed_identity_directory_is_trusted_failure(self):
        from zk_registry import safe_fs as FS

        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            identity = root / "sealed" / "Challenge.ndjson-identity"
            identity.mkdir(parents=True)
            exposure = {str(identity): FS.root_identity(identity)}
            with patch.object(CR, "runner") as runner:
                result = D.export_module(
                    L.LeanEnv("/usr", [], "", {}, "", temp),
                    "ProofMod",
                    "goal",
                    str(root / "sealed" / "Challenge.ndjson"),
                    D.Tools("", "", "", "", None),
                    Sandbox.fixture(),
                    temp,
                    [],
                    10,
                    [],
                    identity=True,
                    exposure=exposure,
                )
            self.assertEqual(
                (result["verdict"], result["reason"]), ("ERROR", "sandbox-unavailable")
            )
            runner.assert_not_called()


class FullCheckerRegression(unittest.TestCase):
    def test_accepted_large_solution_stays_pass_in_check(self):
        from zk_registry import check as C
        from test_zk_registry_package import make_package
        from test_zk_registry_check import fake_env, submission, VerdictMappingTests

        with tempfile.TemporaryDirectory() as temp:
            package, metadata = make_package(temp)
            proof = Path(temp) / "Solution.lean"
            proof.write_text(submission(Path(package, "Statement.lean").read_text(), "  trivial\n"))
            env = fake_env(temp)
            tools = D.Tools("/bin/true", "/bin/true", "/bin/true", "/usr", None)
            calls = []

            def semantic(export, *args, **kwargs):
                calls.append(Path(export).name)
                if Path(export).name == "Solution.ndjson":
                    # Accepted pilot 0015: 4,045,979 records > parser MAX_RECORDS.
                    return dict(verdict="ERROR", reason="statement-identity-unavailable")
                return dict(
                    verdict="PASS",
                    statement_closure_sha256="a" * 64,
                    statement_closure_version="boole-statement-closure/v1",
                )

            with patch.object(
                C, "compile_in_sandbox", return_value=VerdictMappingTests().ok_compile()
            ), patch.object(
                CR, "runner", return_value=lambda *args: L.RunResult(0, "accepted export", 0, False)
            ), patch.object(
                D, "statement_identity", side_effect=semantic
            ), patch.object(
                D, "export_module", wraps=D.export_module
            ) as export, patch.object(
                D, "compare_exports", return_value=dict(verdict="PASS", reason=None)
            ) as compare:
                result = C.check(
                    package,
                    str(proof),
                    env,
                    policy_fixture=env.pins(),
                    sandbox=Sandbox.fixture(),
                    tools=tools,
                )
            self.assertEqual(result["verdict"], "PASS", result.get("reason"))
            compare.assert_called_once()
            self.assertEqual(calls, ["Challenge.ndjson"])
            self.assertEqual(
                [call.kwargs.get("identity", False) for call in export.call_args_list],
                [True, False],
            )


class TrustedWorkerRegression(unittest.TestCase):
    def test_worker_ignores_pythonpath_injection(self):
        fixtures = Path(__file__).parent / "tests" / "statement-identity"
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            poison = root / "poison"
            poison.mkdir()
            (poison / "json.py").write_text("raise RuntimeError('submission module injection')\n")
            env = dict(os.environ, PYTHONPATH=str(poison), PYTHONDONTWRITEBYTECODE="1")
            result = D.statement_identity(
                str(fixtures / "original" / "Challenge.ndjson"),
                "PublicClosureFixture.same",
                str(root / "trusted"),
                10,
                task_env=env,
            )
            self.assertEqual(result["verdict"], "PASS", result.get("reason"))

    def test_documented_tools_root_constructs_pinned_production_boundary(self):
        tools = D.Tools("", "", "", "", "pinned-launcher")
        with tempfile.TemporaryDirectory() as temp, patch.object(
            D.Tools, "load", return_value=tools
        ), patch.object(
            Sandbox, "production", return_value=Sandbox.fixture()
        ) as production, patch.object(
            D, "export_module", return_value=dict(verdict="PASS", statement_closure_sha256="a" * 64)
        ):
            env = L.LeanEnv(
                "/usr", [], "", {}, "", temp, {"acceptance_tools_root": "provisioned-tools"}
            )
            result = D.reference_identity(env, "Statement", "goal", temp, [], 10)
        self.assertEqual(result["verdict"], "PASS")
        production.assert_called_once_with("pinned-launcher")

    def test_missing_tools_never_claims_successful_identity(self):
        env = L.LeanEnv("/usr", [], "", {}, "", ".", {})
        with patch.object(D, "export_module") as export:
            result = D.reference_identity(env, "Statement", "goal", ".", [], 10)
        self.assertEqual(result["verdict"], "ERROR")
        export.assert_not_called()


class KernelStageWiring(unittest.TestCase):
    def test_sealed_exports_preserve_exposure_and_only_challenge_identity(self):
        from zk_registry import kernel_nonvac as K

        with tempfile.TemporaryDirectory() as temp, patch.object(
            L, "compile_module", return_value=(L.RunResult(0, "", 0, False), [])
        ), patch.object(
            D,
            "export_module",
            return_value=dict(
                verdict="PASS", path="sealed-export", statement_closure_sha256="a" * 64
            ),
        ) as export, patch.object(
            D, "compare_exports", return_value=dict(verdict="unexpected", reason=None)
        ):
            sandbox = Sandbox.fixture()
            result = K.run(None, [], "", ["True"], str(Path(temp) / "unit"), None, sandbox)
            self.assertEqual(result["status"], "UNKNOWN")
            self.assertEqual(result["exception_type"], "KeyError")
            for call in export.call_args_list:
                output, writer = Path(call.args[3]), Path(call.args[6])
                self.assertFalse(output.is_relative_to(writer))
                self.assertEqual(call.kwargs["exposure"], sandbox.exposed_writers)
            self.assertEqual(
                [call.kwargs["identity"] for call in export.call_args_list], [True, False]
            )


class CompiledReferencePropagation(unittest.TestCase):
    def test_ratchet_and_correctness_references_return_digest(self):
        from zk_registry import ratchet as RT, correctness_build as CB, gates as G

        compiled = L.RunResult(0, "", 0, False)
        warning = [{"severity": "warning", "data": "declaration uses sorry"}]
        post = dict(
            replay="ok",
            target={"kind": "theorem"},
            axioms=["sorryAx"],
            type_str="True",
            type_sha256="b" * 64,
        )
        with tempfile.TemporaryDirectory() as temp, patch.object(
            L, "compile_module", return_value=(compiled, warning)
        ), patch.object(G, "run_replay", return_value=(compiled, post)), patch.object(
            D,
            "reference_identity",
            return_value=dict(verdict="PASS", statement_closure_sha256="a" * 64),
        ):
            env = L.LeanEnv("/usr", [], "", {}, "", temp)
            rt = RT._elaborate(env, temp, [], "Fixture.equiv", [], str(Path(temp) / "ratchet"), 10)
            cb = CB.reference({"id": "Fixture"}, "soundness", Path(temp), Path(temp), env, 10)
        self.assertEqual(rt["statement_closure_sha256"], "a" * 64)
        self.assertEqual(cb["statement_closure_sha256"], "a" * 64)

    def test_real_export_fixture_hashes_are_pinned(self):
        from zk_registry import statement_identity as I

        root = Path(__file__).parent / "tests" / "statement-identity"
        expected = {
            "original": "ca0ec3458a9087742bcaff6f6fa7e3e34e1124d04b0b68ecddbc6f525bf325c7",
            "changed": "2a4a5b42b27665d913ab70097de9f7589e53d29a91b01a5b2e2b2857cfb04097",
        }
        records = [
            I.identity(root / label / "Challenge.ndjson", "PublicClosureFixture.same") for label in expected
        ]
        self.assertEqual(records[0]["root_type_sha256"], records[1]["root_type_sha256"])
        for record, label in zip(records, expected):
            self.assertEqual(record["statement_closure_sha256"], expected[label])


if __name__ == "__main__":
    unittest.main()
