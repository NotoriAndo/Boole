#!/usr/bin/env python3
"""Seal GitHub transport around the raw authority-zero R3 payload."""

from __future__ import annotations

import hashlib
import json
import pathlib
import stat
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
PROVENANCE_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-artifact-provenance-arm64-v3.json"
)
RESULT_PATH = REPO / (
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "rehearsal-result-arm64-v3.json"
)

RESULT_SHA256 = "44cd7d6feea2efc62d9ab6cb809e5d66c1452c9e4d2f034fd800e6573938fe87"
RESULT_SIZE_BYTES = 6_012
PROVENANCE_SHA256 = "f1618b92cfa138370209a50743f9630e497b35ee4e05d117d1e0af369a95320d"
PROVENANCE_SIZE_BYTES = 3_288
ZIP_SHA256 = "5f0b7da657d6a56077f16757e4bc461cb968fdd2921c9cdfa11ec878453bed9a"
ZIP_SIZE_BYTES = 1_744

ZERO_AUTHORISATIONS = {
    "bootAuthorised": False,
    "consensusActivated": False,
    "imageProductionAuthorised": False,
    "imageProductionRunsAllowed": 0,
    "mac4Started": False,
    "miningActivated": False,
    "p2pActivated": False,
    "rewardActivated": False,
    "testnetStarted": False,
}
ZERO_EFFECTS = {
    "attemptMarkersCreated": 0,
    "bootAttempts": 0,
    "imageOutputsCreated": 0,
    "productionOutputsCreated": 0,
}
EXPECTED_SOURCE = {
    "event": "workflow_dispatch",
    "headSha": "f690f109ce268bc44a6b91459a373390f6bbc31f",
    "repository": "NotoriAndo/Boole",
    "runAttempt": 1,
    "runId": 33_347_946_953,
    "runUrl": "https://github.com/NotoriAndo/Boole/actions/runs/33347946953",
    "workflowName": "native-shadow-successor-produce-arm64-v5",
    "workflowPath": ".github/workflows/native-shadow-successor-produce-arm64-v5.yml",
}
EXPECTED_JOBS = {
    "freeRehearsal": {
        "conclusion": "success",
        "jobId": 99_355_609_752,
        "jobUrl": (
            "https://github.com/NotoriAndo/Boole/actions/runs/33347946953/"
            "job/99355609752"
        ),
        "name": "free-rehearsal",
    },
    "skipped": [
        {
            "conclusion": "skipped",
            "jobId": 99_355_610_641,
            "name": "production-authority-guard",
        },
        {"conclusion": "skipped", "jobId": 99_355_610_725, "name": "produce"},
        {"conclusion": "skipped", "jobId": 99_355_611_112, "name": "compare"},
    ],
}
EXPECTED_ARTIFACT = {
    "apiDigest": f"sha256:{ZIP_SHA256}",
    "createdAt": "2026-08-31T01:37:45Z",
    "expiredAtObservation": False,
    "expiresAt": "2026-09-07T01:37:45Z",
    "id": 9_742_685_578,
    "name": "launcher-v2-successor-v5-free-rehearsal",
    "runArtifactTotalCount": 1,
    "sizeInBytes": ZIP_SIZE_BYTES,
    "updatedAt": "2026-08-31T01:37:45Z",
}
EXPECTED_ZIP = {
    "apiDigestMatched": True,
    "apiSizeMatched": True,
    "retrievedAt": "2026-08-31T01:38:27Z",
    "sha256": ZIP_SHA256,
    "sizeBytes": ZIP_SIZE_BYTES,
}
EXPECTED_MEMBER = {
    "archiveMemberCount": 1,
    "bytesReadDirectly": True,
    "directory": False,
    "name": "R3-RESULT.json",
    "pathSafe": True,
    "sha256": RESULT_SHA256,
    "sizeBytes": RESULT_SIZE_BYTES,
    "symlink": False,
}


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def read_regular(path: pathlib.Path) -> bytes:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink():
        raise AssertionError(f"{path.relative_to(REPO)} is not regular")
    return path.read_bytes()


def assert_strict_equal(actual: object, expected: object, path: str = "$") -> None:
    if type(actual) is not type(expected):
        raise AssertionError(f"{path} type differs")
    if isinstance(expected, dict):
        if set(actual) != set(expected):
            raise AssertionError(f"{path} keys differ")
        for key in expected:
            assert_strict_equal(actual[key], expected[key], f"{path}.{key}")
        return
    if isinstance(expected, list):
        if len(actual) != len(expected):
            raise AssertionError(f"{path} length differs")
        for index, (actual_item, expected_item) in enumerate(zip(actual, expected)):
            assert_strict_equal(actual_item, expected_item, f"{path}[{index}]")
        return
    if actual != expected:
        raise AssertionError(f"{path} value differs: {actual!r} != {expected!r}")


class LauncherV2SuccessorR3ArtifactProvenanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.raw = read_regular(PROVENANCE_PATH)
        self.record = json.loads(self.raw.decode("utf-8"))
        self.result_raw = read_regular(RESULT_PATH)
        self.result = json.loads(self.result_raw.decode("utf-8"))

    def test_record_is_canonical_and_contains_transport_evidence_only(self) -> None:
        self.assertEqual(len(self.raw), PROVENANCE_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.raw), PROVENANCE_SHA256)
        self.assertEqual(self.raw, canonical_json(self.record))
        self.assertEqual(
            set(self.record),
            {
                "artifact",
                "authorityBoundary",
                "downloadedZip",
                "jobs",
                "schema",
                "soleMember",
                "source",
                "status",
                "subject",
                "trackedPayload",
            },
        )
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-"
            "rehearsal-artifact-provenance.arm64.v3",
        )
        self.assertEqual(
            self.record["status"],
            "SEALED-SUCCESSFUL-AUTHORITY-ZERO-R3-ARTIFACT-PROVENANCE",
        )

    def test_run_jobs_and_artifact_match_observed_github_metadata(self) -> None:
        assert_strict_equal(self.record["source"], EXPECTED_SOURCE)
        assert_strict_equal(self.record["jobs"], EXPECTED_JOBS)
        assert_strict_equal(self.record["artifact"], EXPECTED_ARTIFACT)

    def test_downloaded_zip_and_sole_member_remain_distinct(self) -> None:
        assert_strict_equal(self.record["downloadedZip"], EXPECTED_ZIP)
        assert_strict_equal(self.record["soleMember"], EXPECTED_MEMBER)
        self.assertNotEqual(ZIP_SHA256, RESULT_SHA256)
        self.assertNotEqual(ZIP_SIZE_BYTES, RESULT_SIZE_BYTES)

    def test_member_is_byte_identical_to_the_tracked_raw_payload(self) -> None:
        self.assertEqual(len(self.result_raw), RESULT_SIZE_BYTES)
        self.assertEqual(sha256_bytes(self.result_raw), RESULT_SHA256)
        self.assertEqual(self.result_raw, canonical_json(self.result))
        assert_strict_equal(
            self.record["trackedPayload"],
            {
                "byteIdenticalToSoleMember": True,
                "path": RESULT_PATH.relative_to(REPO).as_posix(),
                "sha256": RESULT_SHA256,
                "sizeBytes": RESULT_SIZE_BYTES,
            },
        )

    def test_transport_preserves_zero_authority_and_skipped_production(self) -> None:
        assert_strict_equal(self.result["authorisations"], ZERO_AUTHORISATIONS)
        assert_strict_equal(self.result["effects"], ZERO_EFFECTS)
        assert_strict_equal(
            self.record["authorityBoundary"],
            {
                "activationAllowed": False,
                "authorisations": ZERO_AUTHORISATIONS,
                "bootableClaim": False,
                "effects": ZERO_EFFECTS,
                "guestBootClaim": False,
                "imageProductionClaim": False,
                "productionGuardSkipped": True,
            },
        )
        self.assertEqual(
            {row["name"] for row in self.record["jobs"]["skipped"]},
            {"production-authority-guard", "produce", "compare"},
        )

    def test_record_avoids_circular_ephemeral_or_future_authority_bindings(self) -> None:
        encoded = self.raw.decode("utf-8")
        for forbidden in (
            "archive_download_url",
            "archiveDownloadUrl",
            "artifactDownloadUrl",
            "gateSha256",
            "selfSha256",
            "signedUrl",
            "producer-fingerprint-arm64-v7",
            "production-authority-arm64-v7",
        ):
            self.assertNotIn(forbidden, encoded)


if __name__ == "__main__":
    unittest.main()
