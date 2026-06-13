"""N0-pre.2 -- CI workflow supply-chain contract.

Every third-party action must be pinned to a full 40-hex commit SHA (a
mutable tag reassignment is undetected arbitrary code execution in CI),
and the workflow must declare a least-privilege top-level ``permissions``
block so the default GITHUB_TOKEN cannot write to the repository.
"""

import pathlib
import re
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"

USES_RE = re.compile(r"^\s*uses:\s*(\S+)", re.MULTILINE)
SHA_PIN_RE = re.compile(r"@[0-9a-f]{40}$")


class CiWorkflowContractTest(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def test_ci_actions_are_sha_pinned(self):
        uses = USES_RE.findall(self.text)
        self.assertTrue(uses, "ci.yml must contain at least one uses: action")
        unpinned = [ref for ref in uses if not SHA_PIN_RE.search(ref)]
        self.assertEqual(
            unpinned,
            [],
            "every action must be pinned to a 40-hex commit SHA; "
            f"mutable refs found: {unpinned}",
        )

    def test_ci_declares_least_privilege_permissions(self):
        self.assertRegex(
            self.text,
            re.compile(r"^permissions:\n\s+contents:\s*read\b", re.MULTILINE),
            "ci.yml must declare a top-level least-privilege permissions "
            "block (contents: read)",
        )


if __name__ == "__main__":
    unittest.main()
