from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "docs/development-throughput-and-evidence-policy-v1.md"


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class DevelopmentThroughputPolicyContract(unittest.TestCase):
    def test_canonical_policy_is_current_and_bounded(self) -> None:
        text = read(POLICY)
        expected_once = (
            "BOOLE-DEVELOPMENT-THROUGHPUT-AND-EVIDENCE-V1",
            "TP1-MILESTONE-SEAM",
            "TP2-BEHAVIOR-FIRST",
            "TP3-EVIDENCE-CLASS",
            "TP4-BOUNDED-RETRY",
            "TP5-HARD-STOP",
            "TP6-DOCUMENT-SYNC",
            "TP7-HISTORICAL-SUPERSESSION",
            "TP8-CURRENT-AUTHORITY-BOUNDARY",
            "TP9-PROCESS-ONLY-CI",
        )
        for marker in expected_once:
            self.assertEqual(text.count(marker), 1, marker)

        self.assertRegex(text, r"defaultMilestoneHours:\s*4-8")
        self.assertRegex(text, r"maxInfrastructureRetriesAfterInitial:\s*2")
        self.assertRegex(text, r"fullCiRunsPerMilestone:\s*1")
        self.assertRegex(text, r"processOnlyHeavyCiRunsPerMilestone:\s*0")

    def test_tracked_authority_docs_use_the_current_process_policy(self) -> None:
        for relative in (
            "docs/mac-first-hidden-linux-execution-plan-v1.md",
            "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
            "docs/native-submission-shadow-verification-v1.md",
        ):
            text = read(ROOT / relative)
            with self.subTest(relative=relative):
                self.assertIn("CURRENT-PROCESS-POLICY-V1", text)
                self.assertIn(
                    "docs/development-throughput-and-evidence-policy-v1.md", text
                )
                self.assertIn("A7", text)

    def test_tracked_authority_docs_share_the_current_cursor(self) -> None:
        for relative in (
            "docs/mac-first-hidden-linux-execution-plan-v1.md",
            "docs/node-native-shadow-binding-containment-implementation-spec-v1.md",
            "docs/native-submission-shadow-verification-v1.md",
        ):
            text = read(ROOT / relative)
            with self.subTest(relative=relative):
                normalized = " ".join(text.split())
                self.assertIn("CURRENT-CURSOR-2026-09-02", normalized)
                self.assertIn(
                    "MAC.4 RETAINED GUEST AUTHORITY + PRIVATE JOURNAL OPENER GREEN",
                    normalized,
                )
                self.assertIn("HANDLE-BOUND BOOT INPUTS NEXT", normalized)

    def test_lessons_are_advisory_until_promoted(self) -> None:
        text = read(ROOT / "tasks/lessons.md")
        self.assertIn("LESSONS-ARE-ADVISORY-NOT-BINDING", text)
        self.assertIn("AGENTS.md", text)
        self.assertIn("todo-l1-network-master.md", text)

    def test_task_journal_does_not_override_the_current_cursor(self) -> None:
        text = read(ROOT / "tasks/todo.md")
        self.assertIn("TASK-JOURNAL-NOT-CURRENT-AUTHORITY", text)
        self.assertIn("EXECUTION-ORDER.md", text)
        self.assertIn("development-throughput-and-evidence-policy-v1.md", text)

    def test_policy_gate_is_wired_once(self) -> None:
        self_test = read(ROOT / "scripts/self-test.sh")
        docs_smoke = read(ROOT / "scripts/docs-smoke.sh")
        self.assertEqual(
            self_test.count("scripts/test_development_throughput_policy.py"), 1
        )
        self.assertIn(
            "docs/development-throughput-and-evidence-policy-v1.md", docs_smoke
        )
        self.assertIn("BOOLE-DEVELOPMENT-THROUGHPUT-AND-EVIDENCE-V1", docs_smoke)

    def test_local_planning_mirror_hashes_are_not_ci_trust_roots(self) -> None:
        docs_smoke = read(ROOT / "scripts/docs-smoke.sh")
        shadow = read(ROOT / "docs/native-submission-shadow-verification-v1.md")
        retired_digests = (
            "56c5cb1a47d385319480fe3703e1cb24e2918c63f4378b128708e48e5bbef54d",
            "72f3fa9a5ebe28ffe345986ed8647c5fda900c13fa80fa0766caacab6a840c51",
            "4aed2b5eea4721446aa5249e2e2e3f96a2471b410e41ddc374e2ef6be1158817",
        )
        for digest in retired_digests:
            self.assertNotIn(digest, docs_smoke)
        self.assertIn("LOCAL-MIRROR-DIGEST-SYNC-RETIRED", shadow)
        self.assertRegex(shadow, re.compile(r"historical snapshot", re.IGNORECASE))


if __name__ == "__main__":
    unittest.main()
