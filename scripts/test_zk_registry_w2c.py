"""Candidate identity is supplementary; trusted identity disagreement is a node error."""

import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

if os.environ.get("BOOLE_TEST_SOURCE_ROOT"):
    sys.path.insert(0, str(Path(os.environ["BOOLE_TEST_SOURCE_ROOT"]) / "scripts"))
from zk_registry import (
    closure as D,
    check as C,
    gates as G,
    lean_runner as L,
    correctness_runtime as CR,
)
from zk_registry import ratchet as RT, package as P
from zk_registry.sandbox import Sandbox


class CandidateIdentity(unittest.TestCase):
    def test_identity_limit_does_not_replace_candidate_elaboration(self):
        compiled = L.RunResult(0, "", 0, False)
        post = dict(
            replay="ok",
            target={"kind": "theorem"},
            axioms=["sorryAx"],
            type_str="True",
            type_sha256="a" * 64,
        )
        with tempfile.TemporaryDirectory() as temp, patch.object(
            L,
            "compile_module",
            return_value=(compiled, [{"severity": "warning", "data": "declaration uses sorry"}]),
        ), patch.object(G, "run_replay", return_value=(compiled, post)), patch.object(
            D,
            "reference_identity",
            return_value=dict(verdict="ERROR", reason="export-record-limit"),
        ):
            result = RT._elaborate(
                L.LeanEnv("/usr", [], "", {}, "", temp), temp, [], "Fixture.equiv", [], temp, 30
            )
        self.assertEqual(result["status"], "PASS")
        identity.assert_not_called()
        self.assertIsNone(result.get("statement_closure_sha256"))
        self.assertTrue(result.get("statement_closure_reason"))

    def test_candidate_identity_caps_never_prevent_kernel_verdict(self):
        from test_zk_registry_ratchet import offline_problem, candidate_package, fake_env
        from test_zk_registry_check import VerdictMappingTests

        for advertised in [None, "a" * 64]:
            with self.subTest(advertised=advertised), tempfile.TemporaryDirectory() as temp:
                pdir, problem = offline_problem(temp)
                package, statement = candidate_package(
                    pdir, problem, "equivalent", str(Path(temp) / "candidate")
                )
                metadata = json.loads(Path(package, "problem.json").read_text())
                metadata["statement_closure_sha256"] = advertised
                P.write_json(str(Path(package, "problem.json")), metadata)
                proof = Path(temp) / "Solution.lean"
                proof.write_text(statement.replace("  sorry", "  rfl"))
                env = fake_env(temp)
                with patch.object(
                    C, "compile_in_sandbox", return_value=VerdictMappingTests().ok_compile()
                ), patch.object(
                    CR,
                    "runner",
                    return_value=lambda *args: L.RunResult(0, "accepted export", 0, False),
                ), patch.object(
                    D,
                    "statement_identity",
                    return_value=dict(verdict="ERROR", reason="export-record-limit"),
                ) as identity, patch.object(
                    D, "compare_exports", return_value=dict(verdict="PASS", reason=None)
                ) as compare:
                    result = C.check(
                        package,
                        str(proof),
                        env,
                        policy_fixture=env.pins(),
                        sandbox=Sandbox.fixture(),
                        tools=D.Tools("/bin/true", "/bin/true", "/bin/true", "/usr", None),
                    )
                self.assertEqual(result["verdict"], "PASS", result.get("error"))
                self.assertIsNone(result["statement_closure_sha256"])
                self.assertTrue(result.get("statement_closure_reason"))
                identity.assert_not_called()
                compare.assert_called_once()


class TrustedMismatch(unittest.TestCase):
    def test_trusted_digest_disagreement_is_error(self):
        from test_zk_registry_package import make_package
        from test_zk_registry_check import fake_env, submission

        with tempfile.TemporaryDirectory() as temp:
            package, problem = make_package(temp)
            problem["statement_closure_sha256"] = "b" * 64
            P.write_json(str(Path(package, "problem.json")), problem)
            proof = Path(temp) / "Solution.lean"
            proof.write_text(submission(Path(package, "Statement.lean").read_text(), "  trivial\n"))
            env = fake_env(temp)
            with patch.object(
                D,
                "prepare_reference",
                return_value=dict(verdict="PASS", statement_closure_sha256="a" * 64),
            ), patch.object(C, "compile_in_sandbox") as compile:
                result = C.check(
                    package,
                    str(proof),
                    env,
                    policy_fixture=env.pins(),
                    sandbox=Sandbox.fixture(),
                    tools=D.Tools("", "", "", "", None),
                )
            self.assertEqual(result["verdict"], "ERROR")
            self.assertIn("statement-closure-mismatch", result["error"])
            compile.assert_not_called()


class CandidateSelection(unittest.TestCase):
    def test_only_six_validated_ratchet_candidate_schemas_skip_identity(self):
        schemas = [
            P.RATCHET_SCHEMA_VERSION,
            P.RATCHET_NOIR_SCHEMA_VERSION,
            P.RATCHET_GNARK_SCHEMA_VERSION,
            P.RATCHET_AIR_SCHEMA_VERSION,
            P.RATCHET_ZOKRATES_SCHEMA_VERSION,
            P.RATCHET_HALO2_SCHEMA_VERSION,
        ]
        for schema in schemas:
            self.assertTrue(P.candidate_derived({"schema_version": schema, "status": "CANDIDATE"}))
            self.assertFalse(P.candidate_derived({"schema_version": schema, "status": "OPEN"}))
        for schema in [
            P.SCHEMA_VERSION,
            P.CORRECTNESS_GNARK_SCHEMA_VERSION,
            P.SPEC_SCHEMA_VERSION,
            "forged",
        ]:
            self.assertFalse(
                P.candidate_derived(
                    {"schema_version": schema, "status": "CANDIDATE", "kind": "ratchet"}
                )
            )

    def test_generation_and_check_agree_on_public_reference_digest(self):
        from test_zk_registry_check import VerdictMappingTests

        fixture = Path(__file__).parent / "tests/statement-identity/original"
        raw = (fixture / "Challenge.ndjson").read_text()
        original_runner = CR.runner

        def runner(*args, **kwargs):
            guarded = original_runner(*args, **kwargs)

            def execute(argv, *rest, **options):
                if any(str(arg).endswith("statement_identity.py") for arg in argv):
                    return guarded(argv, *rest, **options)
                return L.RunResult(0, raw, 0, False)

            return execute

        compiled = L.RunResult(0, "", 0, False)
        post = dict(
            replay="ok",
            target={"kind": "theorem"},
            axioms=["sorryAx"],
            type_str="True",
            type_sha256="a" * 64,
        )
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "Statement.lean").write_text((fixture / "ProofMod.lean").read_text())
            tools = D.Tools("/bin/true", "/bin/true", "/bin/true", "/usr", None)
            env = L.LeanEnv(
                "/usr",
                [],
                "",
                {},
                "",
                temp,
                {"acceptance_tools": tools, "acceptance_sandbox": Sandbox.fixture()},
            )
            with patch.object(
                L,
                "compile_module",
                side_effect=[
                    (compiled, []),
                    (compiled, [{"severity": "warning", "data": "declaration uses sorry"}]),
                ],
            ), patch.object(G, "run_replay", return_value=(compiled, post)), patch.object(
                CR, "runner", side_effect=runner
            ), patch.object(
                C, "compile_in_sandbox", return_value=VerdictMappingTests().ok_compile()
            ):
                generated = G.g_elab(
                    env,
                    temp,
                    temp,
                    "PublicClosureFixture",
                    str(root / "generation"),
                    theorem="same",
                )
                checked = D.prepare_reference(
                    env,
                    {
                        "checker": {
                            "statement_file": "Statement.lean",
                            "theorem_fqn": "PublicClosureFixture.same",
                        }
                    },
                    temp,
                    str(root / "check"),
                    str(root / "sealed/Challenge.ndjson"),
                    tools,
                    Sandbox.fixture(),
                    30,
                    [],
                )
            self.assertEqual(generated.status, "PASS")
            self.assertEqual(checked["verdict"], "PASS")
            self.assertIsNotNone(checked["statement_closure_sha256"])
            self.assertEqual(
                generated.detail["statement_closure_sha256"], checked["statement_closure_sha256"]
            )


if __name__ == "__main__":
    unittest.main()
