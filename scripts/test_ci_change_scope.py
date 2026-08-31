from __future__ import annotations

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CLASSIFIER = ROOT / "scripts/ci_change_scope.py"
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"
VERDICT_WORKFLOW = ROOT / ".github/workflows/verdict-corpus.yml"


def classify(paths: list[str]) -> str:
    result = subprocess.run(
        [sys.executable, str(CLASSIFIER)],
        cwd=ROOT,
        input="".join(f"{path}\n" for path in paths),
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(result.stderr or result.stdout)
    return result.stdout.strip()


class CiChangeScopeTests(unittest.TestCase):
    def test_docs_and_process_contract_files_take_the_light_lane(self) -> None:
        self.assertEqual(
            classify(
                [
                    "docs/example.md",
                    "tasks/lessons.md",
                    "README.md",
                    "scripts/docs-smoke.sh",
                    "scripts/self-test.sh",
                    "scripts/test_ci_workflow_contract.py",
                    "scripts/test_ci_change_scope.py",
                    "scripts/ci_change_scope.py",
                    ".github/workflows/ci.yml",
                ]
            ),
            "process_only=true",
        )

    def test_runtime_or_dependency_change_requires_full_lane(self) -> None:
        for path in (
            "crates/boole-node/src/main.rs",
            "native/checker/checker.py",
            "fixtures/native-shadow/registry-v1.json",
            "Cargo.toml",
            "Cargo.lock",
            "install.sh",
            "scripts/native-shadow-portable-rootfs-replay-linux.sh",
        ):
            with self.subTest(path=path):
                self.assertEqual(classify([path]), "process_only=false")

    def test_mixed_change_requires_full_lane(self) -> None:
        self.assertEqual(
            classify(["docs/example.md", "crates/boole-core/src/lib.rs"]),
            "process_only=false",
        )

    def test_empty_or_unsafe_path_fails_closed_to_full_lane(self) -> None:
        for paths in ([], ["../docs/example.md"], ["/tmp/example.md"]):
            with self.subTest(paths=paths):
                self.assertEqual(classify(paths), "process_only=false")

    def test_github_output_is_machine_readable(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "github-output"
            result = subprocess.run(
                [sys.executable, str(CLASSIFIER), "--github-output", str(output)],
                cwd=ROOT,
                input="docs/example.md\n",
                text=True,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output.read_text(encoding="utf-8"), "process_only=true\n")

    def test_ci_heavy_jobs_only_run_for_full_validation_changes(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("  change-scope:\n", workflow)
        self.assertIn(
            'python3 scripts/ci_change_scope.py --github-output "$GITHUB_OUTPUT"',
            workflow,
        )
        self.assertIn(
            'git diff --name-only --no-renames "$base" "$head"', workflow
        )
        for name in (
            "native-shadow-containment-linux",
            "native-shadow-rootfs-replay-linux",
            "native-shadow-rootfs-replay-linux-arm64",
            "native-shadow-launcher-build-arm64",
            "native-shadow-launcher-build-arm64-v2",
            "native-shadow-launcher-v2-image-preflight-arm64",
        ):
            start = workflow.index(f"  {name}:\n")
            following = re.search(
                r"^  [a-z0-9][a-z0-9-]*:\s*$",
                workflow[start + len(f"  {name}:\n") :],
                re.MULTILINE,
            )
            end = (
                start + len(f"  {name}:\n") + following.start()
                if following
                else len(workflow)
            )
            job = workflow[start:end]
            self.assertIn("needs: change-scope", job, name)
            self.assertIn(
                "if: needs.change-scope.outputs.process_only != 'true'",
                job,
                name,
            )

    def test_required_ci_jobs_keep_their_names_and_use_the_light_lane(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self_test = workflow[workflow.index("  self-test:\n") : workflow.index("  supply-chain:\n")]
        supply_chain = workflow[workflow.index("  supply-chain:\n") :]

        self.assertIn("name: self-test", self_test)
        self.assertIn("change-scope", self_test.splitlines()[2])
        self.assertIn("name: Run process-only contract gate", self_test)
        self.assertIn(
            "python3 -m unittest scripts/test_ci_change_scope.py",
            self_test,
        )
        self.assertIn("./scripts/docs-smoke.sh", self_test)
        self.assertIn(
            "if: needs.change-scope.outputs.process_only != 'true'",
            self_test,
        )

        self.assertIn("name: supply-chain", supply_chain)
        self.assertIn("needs: change-scope", supply_chain)
        self.assertIn("name: Record process-only supply-chain exemption", supply_chain)
        self.assertGreaterEqual(
            supply_chain.count(
                "if: needs.change-scope.outputs.process_only != 'true'"
            ),
            5,
        )

    def test_verdict_required_status_remains_while_matrix_takes_light_lane(self) -> None:
        workflow = VERDICT_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("  change-scope:\n", workflow)
        self.assertIn(
            'python3 scripts/ci_change_scope.py --github-output "$GITHUB_OUTPUT"',
            workflow,
        )
        self.assertIn(
            'git diff --name-only --no-renames "$base" "$head"', workflow
        )
        corpus = workflow[workflow.index("  corpus:\n") : workflow.index("  verdict-corpus:\n")]
        aggregate = workflow[workflow.index("  verdict-corpus:\n") :]
        self.assertIn("needs: change-scope", corpus)
        self.assertIn(
            "if: needs.change-scope.outputs.process_only != 'true'",
            corpus,
        )
        self.assertIn("name: verdict-corpus", aggregate)
        self.assertIn("needs: [change-scope, corpus]", aggregate)
        self.assertIn("PROCESS_ONLY", aggregate)
        self.assertIn('test "${CORPUS_RESULT}" = "skipped"', aggregate)


if __name__ == "__main__":
    unittest.main()
