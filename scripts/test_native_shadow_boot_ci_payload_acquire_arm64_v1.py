#!/usr/bin/env python3
"""RED tests for putting the frozen closure into a clean arm64 runner's store.

The acquirer that filled this developer Mac's store cannot run anywhere else,
and that is not a defect in it.  It pins the plan document it will accept, the
`gpgv` and `zstd` binaries on *this* machine, the packages that were already in
the store before it started, and the exact number of requests it would make.
Every one of those is a true statement about one run on one host, which is what
a pre-registration document is for.  A clean runner satisfies none of them.

So the closure reaches CI a different way, and the way is not a new argument.
The sealed boot source lock already names all 197 artifacts by digest and size,
and the sealed candidate and acquisition documents already name where each of
them came from.  Fetching those exact URLs and refusing anything whose bytes do
not hash to the sealed digest is the same shape as the already-sealed Rust
distribution acquirer, and it trusts the server strictly less than the original
signed-metadata replay did: there, a signature decided what the digests were;
here, the digests are already decided and the server gets no vote.

What is pinned below is that this tool invents nothing.  Every URL, digest and
size is derived from a sealed document and compared back to it, the committed
plan has to equal that derivation, and the whole thing is a store full of
verified bytes -- not a build, not an image, and not a boot.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import shutil
import tempfile
import unittest
from typing import Any, Iterable

from scripts import native_shadow_boot_ci_payload_acquire_arm64_v1 as acquirer
from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
MODULE_PATH = REPO / "scripts/native_shadow_boot_ci_payload_acquire_arm64_v1.py"
MODULE_TEXT = MODULE_PATH.read_text(encoding="utf-8") if MODULE_PATH.exists() else ""
LOCAL_CAS = REPO / "local-docs/native-shadow-runtime-rootfs-source-lock-v1/cas/sha256"


def locked_artifacts() -> dict[str, dict[str, Any]]:
    lock = json.loads(LOCK_PATH.read_text(encoding="utf-8"))
    return {row["id"]: row for row in lock["artifacts"]}


def plan() -> dict[str, Any]:
    return acquirer.derive_plan()


def fixed_stream(body: bytes) -> Any:
    def factory(spec: dict[str, object]) -> Iterable[bytes]:
        yield body

    return factory


class CoverageTests(unittest.TestCase):
    """Every locked artifact has exactly one frozen way to arrive."""

    def test_the_three_routes_partition_the_locked_closure(self) -> None:
        document = plan()
        fetched = [str(row["artifactId"]) for row in document["artifacts"]]
        derived = [str(row["artifactId"]) for row in document["derivedArtifacts"]]
        reused = [str(value) for value in document["reusedArtifactIds"]]
        combined = fetched + derived + reused
        self.assertEqual(
            len(combined), len(set(combined)), "an artifact arrives by two routes"
        )
        self.assertEqual(sorted(combined), sorted(locked_artifacts()))

    def test_nothing_outside_the_lock_is_acquired(self) -> None:
        """A store that holds more than the lock is a store the lock cannot vouch for."""

        locked = set(locked_artifacts())
        document = plan()
        for row in document["artifacts"] + document["derivedArtifacts"]:
            self.assertIn(str(row["artifactId"]), locked)

    def test_the_counts_the_plan_declares_are_the_counts_it_has(self) -> None:
        document = plan()
        expected = document["expected"]
        self.assertEqual(expected["fetchCount"], len(document["artifacts"]))
        self.assertEqual(expected["derivedCount"], len(document["derivedArtifacts"]))
        self.assertEqual(expected["reusedCount"], len(document["reusedArtifactIds"]))
        self.assertEqual(expected["artifactCount"], len(locked_artifacts()))


class FrozenIdentityTests(unittest.TestCase):
    """The digest and size are the lock's; only the URL comes from elsewhere."""

    def test_every_fetched_digest_and_size_is_the_locked_one(self) -> None:
        locked = locked_artifacts()
        for row in plan()["artifacts"]:
            sealed = locked[str(row["artifactId"])]
            self.assertEqual(row["sha256"], sealed["sha256"])
            self.assertEqual(row["sizeBytes"], sealed["sizeBytes"])

    def test_every_url_passes_the_frozen_snapshot_policy(self) -> None:
        """Reusing the existing spec check means the policy has one home, not two."""

        for row in plan()["artifacts"]:
            payload._spec(row)

    def test_no_digest_size_or_url_is_restated_in_this_module(self) -> None:
        self.assertNotIn(payload.SNAPSHOT_BASE, MODULE_TEXT)
        self.assertEqual(re.findall(r"\b[0-9a-f]{64}\b", MODULE_TEXT), [])
        self.assertNotIn("pool/main", MODULE_TEXT)

    def test_the_rust_archives_are_reused_rather_than_fetched_again(self) -> None:
        """Their acquirer is already sealed, and it runs before this one."""

        document = plan()
        fetched = {str(row["artifactId"]) for row in document["artifacts"]}
        for identifier in document["reusedArtifactIds"]:
            self.assertNotIn(identifier, fetched)
            self.assertEqual(locked_artifacts()[identifier]["kind"], "rust-dist")


class DerivedArtifactTests(unittest.TestCase):
    """One artifact is not downloaded at all; it is opened out of another."""

    def row(self) -> dict[str, Any]:
        derived = plan()["derivedArtifacts"]
        self.assertEqual(len(derived), 1, "the derived set changed shape")
        return derived[0]

    def test_it_is_opened_out_of_a_package_the_lock_also_pins(self) -> None:
        row = self.row()
        self.assertIn(str(row["fromArtifactId"]), locked_artifacts())

    def test_its_expected_digest_is_the_locked_one(self) -> None:
        row = self.row()
        sealed = locked_artifacts()[str(row["artifactId"])]
        self.assertEqual(row["sha256"], sealed["sha256"])
        self.assertEqual(row["sizeBytes"], sealed["sizeBytes"])

    def test_extracting_it_reproduces_the_sealed_digest(self) -> None:
        row = self.row()
        source = LOCAL_CAS / str(locked_artifacts()[str(row["fromArtifactId"])]["sha256"])
        zstd = shutil.which("zstd")
        if not source.is_file() or zstd is None:
            self.skipTest("the source package or zstd is not on this host")
        extracted = acquirer.derive_member(
            package=source.read_bytes(),
            member_path=str(row["memberPath"]),
            zstd_path=pathlib.Path(zstd),
        )
        self.assertEqual(hashlib.sha256(extracted).hexdigest(), row["sha256"])
        self.assertEqual(len(extracted), row["sizeBytes"])

    def test_the_decompressor_is_recorded_rather_than_pinned(self) -> None:
        """Its output has to equal a sealed digest, so its identity cannot decide anything.

        The developer Mac's `zstd` and the runner's are different binaries and
        saying otherwise would be false.  Pinning one of them would only mean the
        other host is refused for a reason that has nothing to do with the bytes.
        """

        self.assertEqual(
            plan()["tools"]["zstdIdentity"], "recorded-in-the-result-never-pinned"
        )


class SealTests(unittest.TestCase):
    """The plan is frozen before any result, and it cannot be quietly edited."""

    def test_the_committed_plan_is_what_the_sealed_documents_derive(self) -> None:
        self.assertTrue(acquirer.PLAN_PATH.is_file(), f"{acquirer.PLAN_PATH} is absent")
        self.assertEqual(
            acquirer.PLAN_PATH.read_bytes(), payload.canonical_json(acquirer.derive_plan())
        )

    def test_a_plan_that_drifts_from_the_sealed_documents_is_refused(self) -> None:
        document = plan()
        document["artifacts"][0]["sha256"] = "0" * 64
        with tempfile.TemporaryDirectory() as scratch:
            path = pathlib.Path(scratch) / "plan.json"
            path.write_bytes(payload.canonical_json(document))
            with self.assertRaises(acquirer.CiPayloadAcquisitionError):
                acquirer.load_plan(path)

    def test_the_abort_conditions_are_named_before_the_run(self) -> None:
        conditions = plan()["abortConditions"]
        self.assertEqual(conditions, sorted(set(conditions)))
        self.assertTrue(conditions)

    def test_the_declared_budget_is_under_the_operator_ceiling(self) -> None:
        """Two jobs of this, and the ceiling the operator named is 2 GiB."""

        self.assertLess(plan()["expected"]["fetchBytes"] * 2, 2 * 1024**3)


class AcquisitionTests(unittest.TestCase):
    """What reaches the store, and what is refused before it can."""

    def spec(self, body: bytes) -> dict[str, object]:
        return {
            "artifactId": "deb-stand-in",
            "sha256": hashlib.sha256(body).hexdigest(),
            "sizeBytes": len(body),
            "url": f"{payload.SNAPSHOT_BASE}/pool/main/s/stand-in/stand-in.deb",
        }

    def test_verified_bytes_are_published_into_the_store(self) -> None:
        body = b"a stand-in for a package"
        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            summary = acquirer.acquire_specs(
                cas=cas, specs=[self.spec(body)], stream_factory=fixed_stream(body)
            )
            stored = cas / "sha256" / hashlib.sha256(body).hexdigest()
            self.assertEqual(stored.read_bytes(), body)
            self.assertEqual(summary["fetched"], 1)
            self.assertEqual(summary["reused"], 0)

    def test_an_artifact_already_in_the_store_is_never_requested(self) -> None:
        """A re-run costs no network, which is the whole point of a store."""

        body = b"a stand-in for a package"

        def refuse(spec: dict[str, object]) -> Iterable[bytes]:
            raise AssertionError("the store was already holding this artifact")

        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            acquirer.acquire_specs(
                cas=cas, specs=[self.spec(body)], stream_factory=fixed_stream(body)
            )
            summary = acquirer.acquire_specs(
                cas=cas, specs=[self.spec(body)], stream_factory=refuse
            )
            self.assertEqual(summary["fetched"], 0)
            self.assertEqual(summary["reused"], 1)

    def test_bytes_that_miss_the_frozen_digest_never_reach_the_store(self) -> None:
        body = b"a stand-in for a package"
        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            with self.assertRaises(payload.PayloadAcquisitionError):
                acquirer.acquire_specs(
                    cas=cas,
                    specs=[self.spec(body)],
                    stream_factory=fixed_stream(body + b"!"),
                )
            stored = cas / "sha256" / hashlib.sha256(body).hexdigest()
            self.assertFalse(stored.exists())

    def test_transport_failure_names_the_frozen_artifact(self) -> None:
        body = b"a stand-in for a package"

        def fail(_spec: dict[str, object]) -> Iterable[bytes]:
            raise payload.PayloadAcquisitionError(
                "snapshot response status is not 200"
            )

        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            with self.assertRaises(acquirer.CiPayloadAcquisitionError) as caught:
                acquirer.acquire_specs(
                    cas=cas, specs=[self.spec(body)], stream_factory=fail
                )
            message = str(caught.exception)
            self.assertIn("deb-stand-in", message)
            self.assertIn("snapshot response status is not 200", message)

    def test_a_missing_rust_archive_stops_the_run_and_says_which_tool_is_owed(
        self,
    ) -> None:
        """It is fetched by its own sealed acquirer, which is meant to run first."""

        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            cas.mkdir()
            with self.assertRaises(acquirer.CiPayloadAcquisitionError) as caught:
                acquirer.require_reused(cas=cas, plan=plan())
            self.assertIn("rustdist", str(caught.exception))


class BoundaryTests(unittest.TestCase):
    def test_holding_the_bytes_is_not_a_build_an_image_or_a_boot(self) -> None:
        self.assertFalse(acquirer.BOOTABLE_CLAIM)
        self.assertFalse(acquirer.ACTIVATION_ALLOWED)
        for value in plan()["boundaries"].values():
            self.assertFalse(value)

    def test_the_plan_says_what_it_is_not(self) -> None:
        self.assertIn("NOT-BOOT-AUTHORITY", acquirer.RESULT_STATUS)

    def test_no_maintainer_script_is_ever_run(self) -> None:
        for forbidden in ("dpkg", "apt-get", "postinst", "subprocess.run(["):
            self.assertNotIn(forbidden, MODULE_TEXT)


if __name__ == "__main__":
    unittest.main()
