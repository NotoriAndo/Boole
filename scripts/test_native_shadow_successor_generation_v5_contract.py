#!/usr/bin/env python3
"""Gate the append-only, main-only launcher-v2 successor generation v5."""

import ast
import hashlib
import json
import pathlib
import re
import stat
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
P4_RELATIVE = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-main-branch-"
    "dispatch-fence-correction-arm64-v1.json"
)
P4 = ROOT / P4_RELATIVE
P4_SHA256 = "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b"
P4_SIZE_BYTES = 13335
V5_CORE = "scripts/native_shadow_successor_produce_phase_arm64_v5.py"
V5_CORE_TEST = "scripts/test_native_shadow_successor_produce_phase_arm64_v5.py"
V5_WRAPPER = "scripts/native-shadow-successor-produce-arm64-v5.sh"
V5_WORKFLOW = ".github/workflows/native-shadow-successor-produce-arm64-v5.yml"
V5_WORKFLOW_TEST = (
    "scripts/test_native_shadow_successor_produce_workflow_arm64_v5.py"
)
V5_FILES = (V5_CORE, V5_CORE_TEST, V5_WRAPPER, V5_WORKFLOW, V5_WORKFLOW_TEST)
PRODUCTION_FILES = (V5_CORE, V5_WRAPPER, V5_WORKFLOW)
V4_CORE_TEST = ROOT / "scripts/test_native_shadow_successor_produce_phase_arm64_v4.py"
V4_WORKFLOW_TEST = (
    ROOT / "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py"
)


def identity(relative: str) -> dict[str, object]:
    path = ROOT / relative
    info = path.lstat()
    if path.is_symlink() or not stat.S_ISREG(info.st_mode):
        raise AssertionError(f"not a regular file: {relative}")
    raw = path.read_bytes()
    return {
        "path": relative,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "sizeBytes": len(raw),
    }


def test_methods(path: pathlib.Path) -> set[str]:
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    return {
        child.name
        for node in tree.body
        if isinstance(node, ast.ClassDef)
        for child in node.body
        if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef))
        and child.name.startswith("test_")
    }


class SuccessorGenerationV5ContractTests(unittest.TestCase):
    def test_p4_is_live_and_declares_exactly_the_five_v5_files(self):
        raw = P4.read_bytes()
        self.assertEqual(len(raw), P4_SIZE_BYTES)
        self.assertEqual(hashlib.sha256(raw).hexdigest(), P4_SHA256)
        p4 = json.loads(raw)
        self.assertEqual(p4["generationLabel"], "P4")
        self.assertEqual(p4["successorGeneration"]["futureFiles"], list(V5_FILES))
        self.assertIs(p4["successorGeneration"]["implementedByThisRecord"], False)
        self.assertEqual(
            p4["futureBindingRequirement"]["fieldName"],
            "mainBranchDispatchFenceCorrection",
        )

    def test_v5_files_exist_without_changing_historical_v4(self):
        p4 = json.loads(P4.read_text(encoding="utf-8"))
        for relative in V5_FILES:
            self.assertTrue((ROOT / relative).is_file(), relative)
        self.assertEqual(
            p4["historicalGeneration"]["files"],
            [identity(row["path"]) for row in p4["historicalGeneration"]["files"]],
        )

    def test_production_files_use_only_the_fresh_a7_and_v5_namespace(self):
        p4 = json.loads(P4.read_text(encoding="utf-8"))
        forbidden = [
            *p4["successorClaimNamespace"]["forbiddenLegacyTokens"],
            *p4["successorClaimNamespace"]["generation"][
                "forbiddenLegacyNamespaceTokens"
            ],
        ]
        for relative in PRODUCTION_FILES:
            source = (ROOT / relative).read_text(encoding="utf-8")
            for token in forbidden:
                self.assertNotIn(token, source, f"{relative}: {token}")
        joined = "\n".join(
            (ROOT / relative).read_text(encoding="utf-8")
            for relative in PRODUCTION_FILES
        )
        required = (
            "mainBranchDispatchFenceCorrection",
            "native-shadow-mac3-successor-production-authority-arm64-v7.json",
            "boole.native-shadow.mac3.successor-production-authority.arm64.v7",
            "native-shadow-mac3-successor-image-production-result-arm64-v7.json",
            "boole.native-shadow.mac3.successor-image-production-result.arm64.v7",
            "refs/tags/boole-native-shadow-mac3-successor-production-a7-",
            "boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v2",
            "authoritySha256",
            "eventName",
            "dispatchRef",
            "workflowRef",
            "githubRunAttempt",
            "--event-name",
            "--dispatch-ref",
            "--workflow-ref",
            "--head-authority-sha256",
            "/var/lib/boole/native-shadow-successor-v5",
            "boole-nsv5-",
            "native-shadow-successor-v5-replica-",
        )
        for token in required:
            self.assertIn(token, joined, token)

    def test_workflow_has_only_manual_dispatch_and_guards_before_checkout(self):
        source = (ROOT / V5_WORKFLOW).read_text(encoding="utf-8")
        header = source.split("permissions:", 1)[0]
        self.assertRegex(header, r"(?m)^on:\n  workflow_dispatch:\n")
        for forbidden_trigger in (
            "pull_request:",
            "pull_request_target:",
            "push:",
            "schedule:",
            "workflow_call:",
        ):
            self.assertNotIn(forbidden_trigger, header)
        first_checkout = source.index("uses: actions/checkout@")
        for expression in (
            "github.event_name == 'workflow_dispatch'",
            "github.ref == 'refs/heads/main'",
            "github.workflow_ref == 'NotoriAndo/Boole/.github/workflows/"
            "native-shadow-successor-produce-arm64-v5.yml@refs/heads/main'",
        ):
            self.assertIn(expression, source[:first_checkout])

    def test_claim_contract_is_exact_and_rechecked_by_every_consumer(self):
        p4 = json.loads(P4.read_text(encoding="utf-8"))
        namespace = p4["successorClaimNamespace"]
        self.assertEqual(
            namespace["claim"]["messageFields"],
            [
                "authoritySha256",
                "attemptId",
                "dispatchRef",
                "eventName",
                "githubRunAttempt",
                "githubRunId",
                "headSha",
                "schema",
                "workflowPath",
                "workflowRef",
            ],
        )
        core = (ROOT / V5_CORE).read_text(encoding="utf-8")
        wrapper = (ROOT / V5_WRAPPER).read_text(encoding="utf-8")
        for field in namespace["claim"]["messageFields"]:
            self.assertIn(field, core)
        for function in (
            "dispatch_claim_message",
            "verify_dispatch_claim",
            "verify_dispatch_tag_object",
            "_reverify_dispatch_capability",
            "produce",
            "publish_and_seal_replica_bundle",
            "compare_provenanced_replicas",
        ):
            self.assertRegex(core, rf"(?m)^def {re.escape(function)}\(")
        for function in (
            "prepare_dispatch_context",
            "recheck_dispatch_claim_ref",
            "snapshot_and_verify_dispatch_claim",
        ):
            self.assertRegex(wrapper, rf"(?m)^{re.escape(function)}\(\)")

    def test_v5_tests_preserve_every_v4_test_method_and_add_main_fence_tests(self):
        core_v4 = test_methods(V4_CORE_TEST)
        core_v5 = test_methods(ROOT / V5_CORE_TEST)
        workflow_v4 = test_methods(V4_WORKFLOW_TEST)
        workflow_v5 = test_methods(ROOT / V5_WORKFLOW_TEST)
        self.assertTrue(core_v4 <= core_v5, sorted(core_v4 - core_v5))
        self.assertTrue(workflow_v4 <= workflow_v5, sorted(workflow_v4 - workflow_v5))
        added = (core_v5 - core_v4) | (workflow_v5 - workflow_v4)
        for idea in (
            "event",
            "main",
            "workflow_ref",
            "a7",
            "claim",
            "legacy",
            "guard",
        ):
            self.assertTrue(any(idea in name for name in added), idea)

    def test_all_v5_gates_are_registered_in_full_self_test(self):
        source = (ROOT / "scripts/self-test.sh").read_text(encoding="utf-8")
        for relative in (V5_CORE_TEST, V5_WORKFLOW_TEST, pathlib.Path(__file__).relative_to(ROOT).as_posix()):
            self.assertIn(relative, source)


if __name__ == "__main__":
    unittest.main()
