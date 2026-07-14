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
VERDICT_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "verdict-corpus.yml"

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


class VerdictCorpusWorkflowContractTest(unittest.TestCase):
    """SC.9c (ADR-0016 (a-1)) -- the cross-platform verdict corpus gate.

    Four concrete jobs (Linux/macOS x debug/release) compare one golden
    verdict digest, behind an always-created aggregate ``verdict-corpus``
    status that branch protection requires. A platform- or
    profile-divergent verdict is a fork vector; merely running the corpus
    inside the Ubuntu self-test is explicitly insufficient.
    """

    def setUp(self):
        self.assertTrue(
            VERDICT_WORKFLOW.is_file(),
            "SC.9c requires .github/workflows/verdict-corpus.yml",
        )
        self.text = VERDICT_WORKFLOW.read_text(encoding="utf-8")

    def test_actions_are_sha_pinned_and_least_privilege(self):
        uses = USES_RE.findall(self.text)
        self.assertTrue(uses, "verdict-corpus.yml must use pinned actions")
        unpinned = [ref for ref in uses if not SHA_PIN_RE.search(ref)]
        self.assertEqual(unpinned, [], f"mutable refs found: {unpinned}")
        self.assertRegex(
            self.text,
            re.compile(r"^permissions:\n\s+contents:\s*read\b", re.MULTILINE),
            "verdict-corpus.yml must declare least-privilege permissions",
        )

    def test_matrix_covers_both_platforms_and_profiles(self):
        for token in ("ubuntu-latest", "macos-latest"):
            self.assertIn(
                token,
                self.text,
                f"the corpus matrix must include {token} (ADR-0016 (a-1))",
            )
        self.assertRegex(
            self.text,
            re.compile(r"profile:\s*\[\s*debug\s*,\s*release\s*\]"),
            "the corpus matrix must cover debug AND release profiles",
        )

    def test_aggregate_verdict_corpus_status_always_runs(self):
        self.assertRegex(
            self.text,
            re.compile(r"^\s{2}verdict-corpus:\n", re.MULTILINE),
            "an aggregate job id `verdict-corpus` must exist -- it is the "
            "branch-protection required check name",
        )
        self.assertRegex(
            self.text,
            re.compile(r"if:\s*always\(\)"),
            "the aggregate must be created even when a matrix job fails, "
            "so the required status can never be silently absent",
        )

    def test_workflow_is_not_path_filtered(self):
        self.assertNotIn(
            "paths:",
            self.text,
            "a required check must run on every PR -- path filters would "
            "hang PRs that do not touch the filtered paths (contrast the "
            "non-required macos-isolation canary, ADR-0016 (a-1))",
        )

    def test_corpus_runs_the_verdict_corpus_test_in_both_profiles(self):
        self.assertIn(
            "--test verdict_corpus",
            self.text,
            "the matrix jobs must run the boole-lean-runner verdict_corpus test",
        )
        self.assertIn(
            "--release",
            self.text,
            "the release-profile job must actually test the release profile",
        )


if __name__ == "__main__":
    unittest.main()
