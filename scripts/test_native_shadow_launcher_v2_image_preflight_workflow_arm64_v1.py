#!/usr/bin/env python3
"""RED wiring contract for the launcher-v2 no-image arm64 preflight.

The required CI path must rebuild the exact launcher through its sealed emitter,
hand that file to the no-image wrapper on native arm64, and make ``self-test``
depend on the result.  This is deliberately a required pull-request gate rather
than a manual image-production workflow.  It may retain one JSON report; it may
not name, create, upload, or dispatch a guest image.
"""
from __future__ import annotations

import hashlib
import pathlib
import re
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
CI = REPO / ".github/workflows/ci.yml"
WRAPPER = REPO / "scripts/native-shadow-launcher-v2-image-preflight-arm64.sh"
PREFLIGHT_MODULE = REPO / "scripts/native_shadow_launcher_v2_image_preflight_arm64_v1.py"
EMITTER_MODULE = REPO / "scripts/native_shadow_launcher_emit_arm64_v2.py"
SELF_TEST = REPO / "scripts/self-test.sh"
HISTORICAL_PRODUCER = REPO / "scripts/native_shadow_successor_produce_phase_arm64_v2.py"
HISTORICAL_WORKFLOW = REPO / ".github/workflows/native-shadow-successor-produce-arm64.yml"
HISTORICAL_PRODUCER_SHA256 = (
    "1c1b99257aa5f2d3f144387f72903fc167d6ba8c8b71a74c1b9a6c845073c1a8"
)
HISTORICAL_WORKFLOW_SHA256 = (
    "a6ff2019a9e8f95580ebcb82e32d3a12f1a0397bb25912478716772683601b61"
)
JOB_ID = "native-shadow-launcher-v2-image-preflight-arm64"

IMAGE_NAMES = ("guest-kernel", "guest-initrd", "guest-root-disk")
IMAGE_TOOL_NAMES = (
    "mke2fs",
    "mkfs.ext4",
    "mkinitramfs",
    "dracut",
    "qemu-img",
    "resize2fs",
    "tune2fs",
    "debugfs",
)


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ci_text() -> str:
    return CI.read_text(encoding="utf-8")


def job_text() -> str:
    text = ci_text()
    marker = f"  {JOB_ID}:\n"
    if marker not in text:
        raise AssertionError(f"the required arm64 preflight job is absent: {JOB_ID}")
    body = text.split(marker, 1)[1]
    # Jobs are top-level keys indented exactly two spaces in this workflow.
    next_job = re.search(r"^  [a-zA-Z0-9_-]+:\n", body, re.MULTILINE)
    return body[: next_job.start()] if next_job else body


def self_test_job() -> str:
    text = ci_text()
    marker = "  self-test:\n"
    if marker not in text:
        raise AssertionError("the CI workflow has no self-test job")
    body = text.split(marker, 1)[1]
    next_job = re.search(r"^  [a-zA-Z0-9_-]+:\n", body, re.MULTILINE)
    return body[: next_job.start()] if next_job else body


class HistoricalGenerationTests(unittest.TestCase):
    def test_exhausted_producer_and_workflow_are_not_rewritten(self) -> None:
        self.assertEqual(sha256(HISTORICAL_PRODUCER), HISTORICAL_PRODUCER_SHA256)
        self.assertEqual(sha256(HISTORICAL_WORKFLOW), HISTORICAL_WORKFLOW_SHA256)


class WrapperTests(unittest.TestCase):
    def source(self) -> str:
        if not WRAPPER.is_file():
            self.fail(f"the S2-A wrapper does not exist yet: {WRAPPER}")
        return WRAPPER.read_text(encoding="utf-8")

    def test_wrapper_is_strict_and_native_linux_arm64_only(self) -> None:
        source = self.source()
        self.assertIn("set -euo pipefail", source)
        self.assertRegex(source, r"uname -s.+Linux")
        self.assertRegex(source, r"uname -m.+(aarch64|arm64)")

    def test_wrapper_accepts_only_store_launcher_and_result_inputs(self) -> None:
        source = self.source()
        for required in ("--cas", "--launcher", "--result"):
            self.assertIn(required, source)
        self.assertNotIn("--outputs)", source)
        self.assertNotIn("--produce", source)
        self.assertNotIn("--preflight-only", source)

    def test_wrapper_invokes_the_new_preflight_with_the_exact_launcher_path(self) -> None:
        source = self.source()
        self.assertIn(PREFLIGHT_MODULE.name, source)
        self.assertRegex(source, r"--launcher\s+[\"$].*launcher")
        self.assertRegex(source, r"--result\s+[\"$].*result")

    def test_wrapper_uses_the_sealed_production_isolation_for_the_only_preflight(self) -> None:
        source = self.source()
        self.assertEqual(source.count(" isolation-argv "), 1)
        self.assertEqual(source.count('--read-write-path "$scratch"'), 1)
        self.assertEqual(source.count('"${isolation[@]}"'), 1)
        self.assertLess(source.index(" isolation-argv "), source.index('"${isolation[@]}"'))
        self.assertLess(source.index('"${isolation[@]}"'), source.index('find "$scratch"'))

    def test_wrapper_publishes_the_only_result_create_once_without_overwrite(self) -> None:
        source = self.source()
        self.assertIn("tempfile.mkstemp(", source)
        self.assertIn("os.link(temporary, destination)", source)
        self.assertIn("errno.EEXIST", source)
        self.assertGreaterEqual(source.count("os.fsync("), 2)
        self.assertIn("temporary.unlink(missing_ok=True)", source)
        for forbidden in (
            "os.replace(",
            "os.rename(",
            "shutil.copy",
            "destination.write_bytes",
            "destination.write_text",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, source)

    def test_wrapper_has_no_marker_image_name_or_image_tool(self) -> None:
        source = self.source()
        self.assertNotIn("ATTEMPT-" + "CONSUMED.json", source)
        for forbidden in (*IMAGE_NAMES, *IMAGE_TOOL_NAMES):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, source)
        self.assertIn('find "$scratch"', source)
        self.assertIn('["preflight"]["forbiddenNames"]', source)

    def test_wrapper_never_imports_or_calls_the_historical_producer(self) -> None:
        source = self.source()
        self.assertNotIn(HISTORICAL_PRODUCER.name, source)
        self.assertNotIn(HISTORICAL_WORKFLOW.name, source)


class Arm64WorkflowTests(unittest.TestCase):
    def test_preflight_is_a_required_native_arm64_ci_job(self) -> None:
        body = job_text()
        self.assertIn(f"name: {JOB_ID}", body)
        self.assertIn("runs-on: ubuntu-24.04-arm", body)
        self.assertNotIn("continue-on-error", body)
        self.assertNotIn("|| true", body)

    def test_exact_launcher_v2_is_emitted_before_the_wrapper_runs(self) -> None:
        body = job_text()
        emitter = body.index(EMITTER_MODULE.name)
        wrapper = body.index(WRAPPER.name)
        self.assertLess(emitter, wrapper)
        self.assertRegex(body[emitter:wrapper], r"\bemit\b")
        self.assertIn("--out", body[emitter:wrapper])
        self.assertIn("--launcher", body[wrapper:])
        self.assertIn("$RUNNER_TEMP/launcher-v2/boole-native-shadow-launcher", body)

    def test_job_runs_the_wrapper_not_the_preflight_beside_it(self) -> None:
        body = job_text()
        self.assertIn(WRAPPER.name, body)
        # Isolation and host checks belong to the wrapper. Calling Python again
        # beside it would create a second, weaker preflight path.
        after_emitter = body.split(EMITTER_MODULE.name, 1)[1]
        self.assertNotIn(PREFLIGHT_MODULE.name, after_emitter)

    def test_job_requires_a_json_result_and_only_that_result_is_uploaded(self) -> None:
        body = job_text()
        self.assertEqual(body.count("upload-artifact@"), 1)
        self.assertRegex(
            body,
            r"path:\s*(?:\$\{\{\s*runner\.temp\s*\}\}|\$RUNNER_TEMP)/[^\n]*\.json",
        )
        self.assertIn("if-no-files-found: error", body)
        self.assertIn("find \"$RUNNER_TEMP\"", body)
        self.assertIn('["preflight"]["forbiddenNames"]', body)

    def test_job_has_no_image_tool_marker_or_production_entry_point(self) -> None:
        body = job_text()
        self.assertNotIn(HISTORICAL_PRODUCER.name, body)
        self.assertNotIn(HISTORICAL_WORKFLOW.name, body)
        for tool in IMAGE_TOOL_NAMES:
            with self.subTest(tool=tool):
                self.assertNotIn(tool, body)

    def test_every_action_reference_in_the_job_is_an_immutable_sha(self) -> None:
        uses = re.findall(r"^\s*uses:\s*(\S+)", job_text(), re.MULTILINE)
        self.assertTrue(uses)
        for reference in uses:
            self.assertRegex(reference, r"@[0-9a-f]{40}$", reference)

    def test_self_test_depends_on_and_checks_the_arm64_preflight_result(self) -> None:
        body = self_test_job()
        needs = re.search(r"^\s*needs:\s*\[([^\]]+)\]", body, re.MULTILINE)
        self.assertIsNotNone(needs)
        self.assertIn(JOB_ID, needs.group(1))
        self.assertIn(f"needs.{JOB_ID}.result", body)
        variable = "NATIVE_SHADOW_ARM64_LAUNCHER_V2_IMAGE_PREFLIGHT_RESULT"
        self.assertRegex(
            body,
            rf"{variable}:\s*\$\{{\{{\s*needs\.{re.escape(JOB_ID)}\.result\s*\}}\}}",
        )
        self.assertRegex(body, rf'"\${variable}"\s*!=\s*success')

    def test_self_test_script_runs_both_s2a_contracts(self) -> None:
        source = SELF_TEST.read_text(encoding="utf-8")
        self.assertIn(pathlib.Path(__file__).name, source)
        self.assertIn(
            "test_native_shadow_launcher_v2_image_preflight_arm64_v1.py", source
        )


if __name__ == "__main__":
    unittest.main()
