#!/usr/bin/env python3
"""Seal and re-derive the real arm64 launcher-v2 no-image preflight result."""
from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest

from scripts import native_shadow_launcher_v2_image_preflight_arm64_v1 as preflight


REPO = pathlib.Path(__file__).resolve().parents[1]
RESULT_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-image-preflight-result-arm64-v1.json"
)
MEASUREMENT_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json"
)
PLAN_PATH = REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"
SPEC_PATH = (
    REPO
    / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md"
)
SHADOW_PATH = REPO / "docs/native-submission-shadow-verification-v1.md"
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS_SMOKE_PATH = REPO / "scripts/docs-smoke.sh"

SEALED_RESULT_SHA256 = (
    "2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d"
)
SEALED_RESULT_SIZE_BYTES = 9_409
EXPECTED_PATH_MANIFEST_SHA256 = (
    "0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a"
)
EXPECTED_TOOL_PROVENANCE = [
    {
        "basename": "gpgv",
        "role": "gpgv",
        "sha256": "a7b1bc1a88927e6f5b30101c415c311aaa9810f51642c12a6b1a824a4c1df1fa",
        "sizeBytes": 330_648,
    },
    {
        "basename": "zstd",
        "role": "zstd",
        "sha256": "53ec52050d37643356ef1d143b9abe5da76fc3b240d933c9d6f7e528f7be3ace",
        "sizeBytes": 1_122_936,
    },
]
SECTION_BEGIN = "<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:BEGIN -->"
SECTION_END = "<!-- LAUNCHER-V2-IMAGE-PREFLIGHT-ARM64-V1-SEALED:END -->"
LINEAGE_TOKENS = (
    "sourcePullRequest=#303",
    "sourceHeadSha=6e95d5a73a17dda26adb006cd2c0de5129a1921d",
    "sourceWorkflow=.github/workflows/ci.yml",
    "sourceRunId=33272680385",
    "sourceRunAttempt=1",
    "sourceJobId=99153889500",
    "artifactId=9720614194",
    "artifactName=launcher-v2-image-preflight-result",
    "archiveSizeBytes=3079",
    "archiveDigest=sha256:beb2920dcfe11ae0f827b73245a8a15bf9e7b055809ad23fac953cef4ed633c8",
    "artifactMemberCount=1",
    "artifactMemberName=PREFLIGHT-RESULT.json",
    "payloadSizeBytes=9409",
    "payloadSha256=2a2bfa93796e0ec1463e1d144250e3bc4e2f6b9c2486c35846e3b9f70071d19d",
)


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def sealed_section(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    if text.count(SECTION_BEGIN) != 1 or text.count(SECTION_END) != 1:
        raise AssertionError(f"{path.name} does not contain one sealed section")
    before, remainder = text.split(SECTION_BEGIN, 1)
    section, after = remainder.split(SECTION_END, 1)
    if SECTION_END in before or SECTION_BEGIN in after:
        raise AssertionError(f"{path.name} has crossed section markers")
    return section


class TrackedCiPreflightResultTests(unittest.TestCase):
    def setUp(self) -> None:
        info = RESULT_PATH.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(RESULT_PATH.is_symlink())
        self.raw = RESULT_PATH.read_bytes()
        self.result = json.loads(self.raw.decode("utf-8"))
        self.preregistration = preflight.load_preregistration()

    def test_tracked_report_is_the_exact_canonical_ci_payload(self) -> None:
        self.assertEqual(len(self.raw), SEALED_RESULT_SIZE_BYTES)
        self.assertEqual(hashlib.sha256(self.raw).hexdigest(), SEALED_RESULT_SHA256)
        self.assertEqual(self.raw, preflight.canonical_json(self.result))

    def test_report_uses_the_exact_consumer_schema_without_a_wrapper(self) -> None:
        self.assertEqual(set(self.result), preflight.RESULT_KEYS)
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.launcher-v2-image-preflight.arm64.v1",
        )
        self.assertEqual(self.result["status"], "PASS-NO-IMAGE-PRODUCED")
        for wrapper_key in (
            "artifact",
            "artifactId",
            "archiveDigest",
            "headSha",
            "jobId",
            "observedResult",
            "runId",
            "selfSha256",
        ):
            self.assertNotIn(wrapper_key, self.result)

    def test_every_preregistered_input_is_rederived_from_the_repository(self) -> None:
        expected = preflight.verify_bound_inputs(self.preregistration, REPO)
        self.assertTrue(preflight._strict_equal(self.result["boundInputs"], expected))
        self.assertEqual(len(expected), 22)

    def test_projection_identity_limits_and_authorities_are_exact(self) -> None:
        projection = self.preregistration["expectedProjection"]
        exact = {
            "authorisations": self.preregistration["authorisations"],
            "launcher": projection["successorLauncher"],
            "limits": projection["limits"],
            "preregistrationSha256": preflight.PREREGISTRATION_SHA256,
            "projection": {
                "baseline": projection["withoutLauncher"],
                "withLauncherV2": projection["withLauncherV2"],
            },
        }
        for key, value in exact.items():
            self.assertTrue(preflight._strict_equal(self.result[key], value), msg=key)
        self.assertIs(self.result["activationAllowed"], False)
        self.assertIs(self.result["bootableClaim"], False)
        self.assertIs(self.result["imageProduced"], False)
        self.assertIs(self.result["repeatable"], True)

    def test_two_measurements_are_strictly_equal_and_match_the_projection(self) -> None:
        preflight._verify_measurement(
            self.result, self.preregistration["expectedProjection"]
        )
        self.assertTrue(
            preflight._strict_equal(
                self.result["builderInternal"], self.result["independentTraversal"]
            )
        )
        measured = self.result["builderInternal"]
        self.assertEqual(measured["entries"], 17_676)
        self.assertEqual(
            measured["byKind"],
            {"directory": 1_737, "file": 15_102, "symlink": 837},
        )
        self.assertEqual(measured["payloadBytes"], 1_773_475_059)
        self.assertEqual(measured["largestFileBytes"], 160_096_808)
        self.assertEqual(measured["caseFoldedSiblings"], 20)
        for key in ("duplicatePaths", "pathCollisions", "symlinkEscapes"):
            self.assertEqual(measured[key], 0, key)
        self.assertEqual(
            measured["pathManifestSha256"], EXPECTED_PATH_MANIFEST_SHA256
        )

    def test_nested_manifest_and_repository_provenance_are_rederived(self) -> None:
        measurement = preflight._bound_document(
            self.preregistration, REPO, MEASUREMENT_PATH
        )
        self.assertTrue(
            preflight._strict_equal(
                self.result["nestedContentManifest"],
                measurement["nestedContentManifest"],
            )
        )
        provenance = self.result["provenance"]
        self.assertTrue(
            preflight._strict_equal(
                provenance["repositoryFiles"],
                preflight._repository_file_identities(REPO),
            )
        )
        self.assertEqual(provenance["sourceLockSha256"], preflight.SOURCE_LOCK_SHA256)
        self.assertTrue(
            preflight._strict_equal(provenance["tools"], EXPECTED_TOOL_PROVENANCE)
        )

    def test_report_cannot_claim_image_boot_or_activation_authority(self) -> None:
        authorisations = self.result["authorisations"]
        runs = authorisations["imageProductionRunsAllowed"]
        self.assertIs(type(runs), int)
        self.assertEqual(runs, 0)
        for key, value in authorisations.items():
            if key != "imageProductionRunsAllowed":
                self.assertIs(type(value), bool, key)
                self.assertIs(value, False, key)

    def test_source_ci_and_archive_lineage_is_exactly_labeled(self) -> None:
        for path in (PLAN_PATH, SPEC_PATH, SHADOW_PATH):
            section = sealed_section(path)
            with self.subTest(path=path.name):
                for token in LINEAGE_TOKENS:
                    self.assertEqual(section.count(token), 1, token)

    def test_the_result_gate_is_permanently_active(self) -> None:
        invocation = (
            "scripts/test_native_shadow_launcher_v2_image_preflight_result_arm64_v1.py"
        )
        active_lines = [
            line.strip()
            for line in SELF_TEST_PATH.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        matches = [
            line
            for line in active_lines
            if invocation in line and "python3 -m unittest" in line
        ]
        self.assertEqual(len(matches), 1)
        smoke_lines = [
            line.strip()
            for line in DOCS_SMOKE_PATH.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        for token in (
            RESULT_PATH.relative_to(REPO).as_posix(),
            SEALED_RESULT_SHA256,
            "PASS-NO-IMAGE-PRODUCED",
            '"imageProductionRunsAllowed": 0',
        ):
            self.assertTrue(any(token in line for line in smoke_lines), token)
        self.assertEqual(sha256(RESULT_PATH), SEALED_RESULT_SHA256)


if __name__ == "__main__":
    unittest.main()
