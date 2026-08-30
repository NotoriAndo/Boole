#!/usr/bin/env python3
"""Seal the real arm64 authority-zero successor-producer rehearsal payload."""
from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
RESULT_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-rehearsal-result-"
    "arm64-v1.json"
)
PREREGISTRATION_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-preregistration-"
    "arm64-v1.json"
)
CORRECTION_PATH = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-import-closure-"
    "correction-arm64-v1.json"
)
PLAN_PATH = REPO / "docs/mac-first-hidden-linux-execution-plan-v1.md"
SPEC_PATH = (
    REPO
    / "docs/node-native-shadow-binding-containment-implementation-spec-v1.md"
)
SHADOW_PATH = REPO / "docs/native-submission-shadow-verification-v1.md"
SELF_TEST_PATH = REPO / "scripts/self-test.sh"
DOCS_SMOKE_PATH = REPO / "scripts/docs-smoke.sh"

RESULT_SHA256 = "d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c"
RESULT_SIZE_BYTES = 10_168
PREREGISTRATION_SHA256 = (
    "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec"
)
PREREGISTRATION_SIZE_BYTES = 20_145
CORRECTION_SHA256 = (
    "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498"
)
CORRECTION_SIZE_BYTES = 10_971

EXPECTED_KEYS = {
    "activationAllowed",
    "authorisations",
    "bootableClaim",
    "boundInputs",
    "effects",
    "imageProduced",
    "importClosureCorrectionSha256",
    "measurement",
    "preregistrationSha256",
    "repeatable",
    "schema",
    "status",
}
EXPECTED_MEASUREMENT = {
    "byKind": {"directory": 1_737, "file": 15_102, "symlink": 837},
    "caseFoldedSiblings": 20,
    "duplicatePaths": 0,
    "entries": 17_676,
    "largestFileBytes": 160_096_808,
    "largestFilePath": (
        "opt/boole/native-checker-toolchain/lib/"
        "libLLVM.so.22.1-rust-1.99.0-nightly"
    ),
    "pathCollisions": 0,
    "pathManifestSha256": (
        "0dbc17aeaaa8ef63ddeb53ac8b7615f361c21bda95f0ba3d9677bdbdb76dcb9a"
    ),
    "payloadBytes": 1_773_475_059,
    "symlinkEscapes": 0,
}
EXPECTED_EFFECTS = {
    "allowedArtifact": "one canonical JSON result only",
    "allowedImageTools": [],
    "artifactMemberCount": 1,
    "attemptMarkersCreated": 0,
    "forbiddenOutputNames": [
        "ATTEMPT-CONSUMED.json",
        "guest-kernel",
        "guest-initrd",
        "guest-root-disk",
    ],
    "imageEffectCalls": 0,
    "imageFilesCreated": 0,
    "productionOutputDirectoriesCreated": 0,
    "productionOutputsCreated": 0,
    "scratchSnapshotSha256": (
        "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570"
    ),
    "scratchTreeUnchanged": True,
}

SECTION_BEGIN = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-"
    "ARM64-V1-SEALED:BEGIN -->"
)
SECTION_END = (
    "<!-- LAUNCHER-V2-SUCCESSOR-PRODUCER-REHEARSAL-RESULT-"
    "ARM64-V1-SEALED:END -->"
)
LINEAGE_TOKENS = (
    "sourceHeadSha=0649dbc92a228fb67350a7eef864a9c9c612fd3d",
    "sourceWorkflow=.github/workflows/native-shadow-successor-produce-arm64-v3.yml",
    "sourceEvent=workflow_dispatch",
    "sourceRunId=33281151298",
    "sourceRunAttempt=1",
    "sourceJobId=99176428509",
    "artifactId=9723056242",
    "artifactName=launcher-v2-successor-v3-free-rehearsal",
    "archiveSizeBytes=3424",
    (
        "archiveDigest=sha256:"
        "a3f6e9c5c9a79712fab1b4454b9401325f543632d5f5f632e3e34e843974b2ef"
    ),
    "artifactMemberCount=1",
    "artifactMemberName=REHEARSAL-RESULT.json",
    "payloadSizeBytes=10168",
    "payloadSha256=d21863e342b701141d6577d3b17cf0a1f26c9211b4b82fa4c8942be96c69f21c",
    "productionGuardJobConclusion=skipped",
    "evidenceClass=AUTHORITY-ZERO-STAGING-EVIDENCE",
    "offlineClaim=false",
    "runnerGlobalTransientAbsenceClaim=false",
    "imageProductionClaim=false",
    "bootClaim=false",
    "mac4Claim=false",
)


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, sort_keys=True, indent=2, ensure_ascii=False)
        + "\n"
    ).encode("utf-8")


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def read_exact_regular_file(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not a regular non-symlink")
    return path.read_bytes()


def sealed_section(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    if text.count(SECTION_BEGIN) != 1 or text.count(SECTION_END) != 1:
        raise AssertionError(f"{path.name} does not contain exactly one sealed section")
    before, remainder = text.split(SECTION_BEGIN, 1)
    section, after = remainder.split(SECTION_END, 1)
    if SECTION_END in before or SECTION_BEGIN in after:
        raise AssertionError(f"{path.name} has crossed sealed-section markers")
    return section


class LauncherV2SuccessorProducerRehearsalResultTests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_exact_regular_file(RESULT_PATH)
        self.result = json.loads(self.raw.decode("utf-8"))

        preregistration_raw = read_exact_regular_file(PREREGISTRATION_PATH)
        correction_raw = read_exact_regular_file(CORRECTION_PATH)
        self.assertEqual(len(preregistration_raw), PREREGISTRATION_SIZE_BYTES)
        self.assertEqual(sha256_bytes(preregistration_raw), PREREGISTRATION_SHA256)
        self.assertEqual(len(correction_raw), CORRECTION_SIZE_BYTES)
        self.assertEqual(sha256_bytes(correction_raw), CORRECTION_SHA256)
        self.preregistration = json.loads(preregistration_raw.decode("utf-8"))
        self.correction = json.loads(correction_raw.decode("utf-8"))

    def test_tracked_result_is_the_exact_canonical_ci_payload(self) -> None:
        self.assertEqual(len(self.raw), RESULT_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.raw), RESULT_SHA256)
        self.assertEqual(self.raw, canonical_json(self.result))

    def test_result_has_only_the_frozen_raw_payload_schema(self) -> None:
        self.assertEqual(set(self.result), EXPECTED_KEYS)
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-rehearsal."
            "arm64.v1",
        )
        self.assertEqual(self.result["status"], "PASS-NO-IMAGE-PRODUCED")
        for forbidden in (
            "artifact",
            "artifactId",
            "archiveDigest",
            "headSha",
            "jobId",
            "runAttempt",
            "runId",
            "runs",
            "selfSha256",
            "wrapper",
        ):
            self.assertNotIn(forbidden, self.result)

    def test_parent_records_are_exact_and_remain_authority_zero(self) -> None:
        self.assertEqual(
            self.result["preregistrationSha256"], PREREGISTRATION_SHA256
        )
        self.assertEqual(
            self.result["importClosureCorrectionSha256"], CORRECTION_SHA256
        )
        self.assertEqual(
            self.result["authorisations"], self.preregistration["authorisations"]
        )
        self.assertEqual(
            self.correction["authorisations"], self.preregistration["authorisations"]
        )
        for record in (self.preregistration, self.correction):
            for key, value in record["authorisations"].items():
                if key == "imageProductionRunsAllowed":
                    self.assertIs(type(value), int, key)
                    self.assertEqual(value, 0, key)
                else:
                    self.assertIs(type(value), bool, key)
                    self.assertIs(value, False, key)
            for key, value in record["runs"].items():
                self.assertIs(type(value), int, key)
                self.assertEqual(value, 0, key)

    def test_all_forty_one_unique_bound_inputs_are_independently_rederived(self) -> None:
        predecessor = self.preregistration["bindings"]
        added = self.correction["addedBindings"]
        self.assertEqual(len(predecessor), 23)
        self.assertEqual(len(added), 18)
        predecessor_paths = {item["path"] for item in predecessor}
        added_paths = {item["path"] for item in added}
        self.assertEqual(len(predecessor_paths), 23)
        self.assertEqual(len(added_paths), 18)
        self.assertTrue(predecessor_paths.isdisjoint(added_paths))

        expected = []
        for binding in predecessor + added:
            relative = pathlib.Path(binding["path"])
            self.assertFalse(relative.is_absolute())
            raw = read_exact_regular_file(REPO / relative)
            identity = {
                "path": relative.as_posix(),
                "sha256": sha256_bytes(raw),
                "sizeBytes": len(raw),
            }
            self.assertEqual(identity["sha256"], binding["sha256"], binding["path"])
            self.assertEqual(
                identity["sizeBytes"], binding["sizeBytes"], binding["path"]
            )
            expected.append(identity)

        expected.sort(key=lambda item: item["path"])
        self.assertEqual(len(expected), 41)
        self.assertEqual(len({item["path"] for item in expected}), 41)
        self.assertEqual(self.result["boundInputs"], expected)
        for item in self.result["boundInputs"]:
            self.assertEqual(set(item), {"path", "sha256", "sizeBytes"})

    def test_measurement_is_the_exact_preregistered_successor_tree(self) -> None:
        self.assertEqual(self.result["measurement"], EXPECTED_MEASUREMENT)
        self.assertEqual(
            self.result["measurement"],
            self.preregistration["expectedPreflight"]["measurement"],
        )

    def test_effects_prove_only_the_json_rehearsal_boundary(self) -> None:
        self.assertEqual(self.result["effects"], EXPECTED_EFFECTS)
        self.assertIs(self.result["repeatable"], True)
        for key in ("activationAllowed", "bootableClaim", "imageProduced"):
            self.assertIs(type(self.result[key]), bool, key)
            self.assertIs(self.result[key], False, key)

    def test_ci_lineage_is_exactly_sealed_in_all_three_authority_docs(self) -> None:
        for path in (PLAN_PATH, SPEC_PATH, SHADOW_PATH):
            section = sealed_section(path)
            with self.subTest(path=path.name):
                for token in LINEAGE_TOKENS:
                    self.assertEqual(section.count(token), 1, token)

    def test_docs_do_not_upgrade_rehearsal_evidence_into_product_claims(self) -> None:
        forbidden_true_claims = (
            "offlineClaim=true",
            "runnerGlobalTransientAbsenceClaim=true",
            "imageProductionClaim=true",
            "bootClaim=true",
            "mac4Claim=true",
        )
        for path in (PLAN_PATH, SPEC_PATH, SHADOW_PATH):
            section = sealed_section(path)
            with self.subTest(path=path.name):
                for token in forbidden_true_claims:
                    self.assertNotIn(token, section)
                self.assertNotIn("PRODUCTION-READY", section)
                self.assertNotIn("OFFLINE-REHEARSAL-PASS", section)

    def test_result_gate_and_document_pins_are_permanently_active(self) -> None:
        invocation = (
            "scripts/"
            "test_native_shadow_launcher_v2_successor_producer_rehearsal_result_"
            "arm64_v1.py"
        )
        self_test_lines = [
            line.strip()
            for line in SELF_TEST_PATH.read_text(encoding="utf-8").splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        ]
        matches = [
            line
            for line in self_test_lines
            if invocation in line and "python3 -m unittest" in line
        ]
        self.assertEqual(len(matches), 1)

        smoke = DOCS_SMOKE_PATH.read_text(encoding="utf-8")
        for token in (
            invocation,
            RESULT_PATH.relative_to(REPO).as_posix(),
            RESULT_SHA256,
            "PASS-NO-IMAGE-PRODUCED",
            "AUTHORITY-ZERO-STAGING-EVIDENCE",
            SECTION_BEGIN.removeprefix("<!-- ").removesuffix(" -->"),
        ):
            self.assertIn(token, smoke, token)


if __name__ == "__main__":
    unittest.main()
