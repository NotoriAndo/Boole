#!/usr/bin/env python3
"""Tests for the Linux/arm64 guest image builder input authority.

Each test here exists to make one specific way of loosening the authority fail.
A validator whose assertions never bite is worse than no validator, because it
reads like evidence, so the mutations below are deliberately the plausible ones:
a tool taken from the host, a package that is not in the source lock, a version
string that drifts from the bytes it names, a boundary quietly flipped true.
"""

from __future__ import annotations

import copy
import hashlib
import json
import pathlib
import sys
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_image_builder_authority_arm64_v1 as builder


def _authority() -> dict:
    return json.loads(builder.AUTHORITY_PATH.read_text())


class SealedDocumentTests(unittest.TestCase):
    """The committed document must be exactly what the tool pins."""

    def test_the_committed_document_matches_its_pin(self) -> None:
        raw = builder.AUTHORITY_PATH.read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), builder.AUTHORITY_SHA256)

    def test_the_committed_document_is_canonical_json(self) -> None:
        raw = builder.AUTHORITY_PATH.read_bytes()
        self.assertEqual(builder.canonical_json(json.loads(raw.decode("utf-8"))), raw)

    def test_loading_validates_and_returns_the_document(self) -> None:
        document = builder.load_authority()
        self.assertEqual(document["schema"], builder.AUTHORITY_SCHEMA)
        self.assertEqual(document["format"], builder.AUTHORITY_FORMAT)

    def test_the_declared_format_is_the_role_the_boot_plan_left_null(self) -> None:
        scaffold = json.loads(
            (
                builder.CONTAINMENT
                / "native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
            ).read_text()
        )
        slot = scaffold["inputs"]["imageBuilderToolchain"]
        self.assertEqual(slot["format"], builder.AUTHORITY_FORMAT)
        # The scaffold is audit-only, so its slot must still be empty: this
        # document is a successor input, not a retroactive edit of that plan.
        self.assertIsNone(slot["sha256"])
        self.assertIsNone(slot["sizeBytes"])


class AuthorityAcceptanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_sealed_document_validates(self) -> None:
        self.assertEqual(
            builder.validate_authority(self.authority)["release"], builder.RELEASE
        )

    def test_an_unknown_top_level_key_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["extra"] = True
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_missing_top_level_key_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        del document["toolBinaries"]
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_activation_cannot_be_switched_on(self) -> None:
        document = copy.deepcopy(self.authority)
        document["activationAllowed"] = True
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_boot_claim_cannot_be_switched_on(self) -> None:
        document = copy.deepcopy(self.authority)
        document["bootableClaim"] = True
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_declared_format_cannot_drift(self) -> None:
        document = copy.deepcopy(self.authority)
        document["format"] = "initrd-ext4-builder-authority-v2"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_platform_cannot_become_x86(self) -> None:
        document = copy.deepcopy(self.authority)
        document["platform"]["architecture"] = "x86_64"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class BoundaryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_every_boundary_is_false_in_the_sealed_document(self) -> None:
        self.assertEqual(
            set(self.authority["boundaries"]), set(builder.BOUNDARY_KEYS)
        )
        self.assertEqual(set(self.authority["boundaries"].values()), {False})

    def test_no_boundary_may_be_flipped_true(self) -> None:
        for name in sorted(builder.BOUNDARY_KEYS):
            with self.subTest(boundary=name):
                document = copy.deepcopy(self.authority)
                document["boundaries"][name] = True
                with self.assertRaises(builder.ImageBuilderAuthorityError):
                    builder.validate_authority(document)

    def test_freezing_inputs_does_not_claim_an_image_was_built(self) -> None:
        for name in ("guestImageBuilt", "initrdBuilt", "rootDiskBuilt", "kernelImageExtracted"):
            self.assertIs(self.authority["boundaries"][name], False)


class ProhibitionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_every_prohibition_is_asserted(self) -> None:
        self.assertEqual(set(self.authority["prohibitions"].values()), {True})

    def test_a_prohibition_cannot_be_relaxed(self) -> None:
        for name in sorted(builder.PROHIBITIONS):
            with self.subTest(prohibition=name):
                document = copy.deepcopy(self.authority)
                document["prohibitions"][name] = False
                with self.assertRaises(builder.ImageBuilderAuthorityError):
                    builder.validate_authority(document)

    def test_a_prohibition_cannot_be_dropped(self) -> None:
        document = copy.deepcopy(self.authority)
        del document["prohibitions"]["forbidHostPathLookup"]
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class ToolBinaryPinTests(unittest.TestCase):
    """No tool comes from `PATH`; every one is a member of a frozen package."""

    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_ext4_writer_is_mke2fs_not_the_mkfs_symlink(self) -> None:
        writer = next(
            row for row in self.authority["toolBinaries"] if row["role"] == "ext4-image-writer"
        )
        self.assertTrue(writer["memberPath"].endswith("/mke2fs"))
        self.assertNotIn("mkfs.ext4", writer["memberPath"])

    def test_an_absolute_host_path_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["memberPath"] = "/usr/sbin/mke2fs"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_bare_tool_name_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["memberPath"] = "mke2fs"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_traversing_member_path_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["memberPath"] = "./usr/sbin/../../mke2fs"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_artifact_id_must_derive_from_the_package_digest(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["packageArtifactId"] = "deb-" + "a" * 64
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_tool_binaries_cannot_be_empty(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"] = []
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_duplicated_role_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"] = [document["toolBinaries"][0]] * 2
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_tool_binaries_stay_sorted_by_role(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"] = list(reversed(document["toolBinaries"]))
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class HostToolTests(unittest.TestCase):
    """`zstd` is named and left unfrozen, and the document must not pretend otherwise."""

    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_host_tool_pair_is_null_in_the_committed_document(self) -> None:
        for row in self.authority["hostTools"]:
            self.assertIsNone(row["path"])
            self.assertIsNone(row["sha256"])
            self.assertIs(row["pinnedAtBuildTime"], True)

    def test_a_committed_host_path_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["hostTools"][0]["path"] = "/opt/homebrew/bin/zstd"
        document["hostTools"][0]["sha256"] = "a" * 64
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_half_pinned_host_tool_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["hostTools"][0]["sha256"] = "a" * 64
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_a_host_tool_cannot_opt_out_of_being_pinned(self) -> None:
        document = copy.deepcopy(self.authority)
        document["hostTools"][0]["pinnedAtBuildTime"] = False
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class BuilderScriptPinTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_frozen_legacy_builder_is_pinned(self) -> None:
        paths = {row["path"] for row in self.authority["builderScripts"]}
        self.assertIn("scripts/native_shadow_rootfs_builder.py", paths)
        self.assertIn("scripts/native_shadow_rootfs_builder_arm64_v1.py", paths)

    def test_pinned_builder_scripts_match_the_tree(self) -> None:
        self.assertEqual(builder.verify_builder_scripts(self.authority), [])

    def test_an_edited_builder_script_is_reported_as_drift(self) -> None:
        document = copy.deepcopy(self.authority)
        document["builderScripts"][0]["sha256"] = "a" * 64
        self.assertEqual(
            builder.verify_builder_scripts(document),
            [document["builderScripts"][0]["path"]],
        )

    def test_an_absent_builder_script_is_reported_as_drift(self) -> None:
        document = copy.deepcopy(self.authority)
        document["builderScripts"][0]["path"] = "scripts/does_not_exist.py"
        self.assertEqual(
            builder.verify_builder_scripts(document), ["scripts/does_not_exist.py"]
        )

    def test_builder_scripts_stay_sorted_by_role(self) -> None:
        document = copy.deepcopy(self.authority)
        document["builderScripts"] = list(reversed(document["builderScripts"]))
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class SourceLockBindingTests(unittest.TestCase):
    """Every pinned package must already be frozen by the committed source lock.

    This is the check a clean CI runner can perform, because the lock is tracked
    while the store holding the package bytes is not.
    """

    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_sealed_document_binds_to_the_committed_lock(self) -> None:
        builder.verify_source_lock_binding(self.authority)

    def test_a_package_outside_the_lock_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        digest = "b" * 64
        document["toolBinaries"][0]["packageSha256"] = digest
        document["toolBinaries"][0]["packageArtifactId"] = f"deb-{digest}"
        with self.assertRaises(builder.ImageBuilderAuthorityError) as caught:
            builder.verify_source_lock_binding(document)
        self.assertIn("absent from the source lock", str(caught.exception))

    def test_a_drifting_version_string_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["version"] = "1.47.0-9ubuntu9"
        with self.assertRaises(builder.ImageBuilderAuthorityError) as caught:
            builder.verify_source_lock_binding(document)
        self.assertIn("version differs", str(caught.exception))

    def test_a_renamed_package_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["toolBinaries"][0]["package"] = "e2fsprogs-but-not-really"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.verify_source_lock_binding(document)

    def test_a_tampered_lock_digest_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["sourceLock"]["sha256"] = "c" * 64
        with self.assertRaises(builder.ImageBuilderAuthorityError) as caught:
            builder.verify_source_lock_binding(document)
        self.assertIn("differs from its pin", str(caught.exception))

    def test_the_kernel_package_is_bound_too(self) -> None:
        document = copy.deepcopy(self.authority)
        document["kernelSource"]["version"] = "9.9.9-9.9"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.verify_source_lock_binding(document)


class KernelSourceTests(unittest.TestCase):
    """The kernel ships gzip-compressed; the decompression step is declared, not implied."""

    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_kernel_is_declared_gzip_compressed(self) -> None:
        self.assertEqual(self.authority["kernelSource"]["compression"], "gzip")
        self.assertEqual(self.authority["determinism"]["kernelDecompression"], "gzip")

    def test_both_the_compressed_and_expanded_digests_are_recorded(self) -> None:
        kernel = self.authority["kernelSource"]
        self.assertNotEqual(kernel["compressedSha256"], kernel["imageSha256"])
        self.assertGreater(kernel["imageSizeBytes"], kernel["compressedSizeBytes"])

    def test_a_compression_that_disagrees_with_the_step_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["kernelSource"]["compression"] = "zstd"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_an_expanded_image_smaller_than_its_archive_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["kernelSource"]["imageSizeBytes"] = 1
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_kernel_member_must_come_from_boot(self) -> None:
        document = copy.deepcopy(self.authority)
        document["kernelSource"]["memberPath"] = "./usr/sbin/mke2fs"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class DeterminismTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_source_date_epoch_stays_unset(self) -> None:
        self.assertIsNone(self.authority["determinism"]["sourceDateEpoch"])

    def test_a_source_date_epoch_value_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["determinism"]["sourceDateEpoch"] = 0
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_timestamp_suppression_cannot_be_permitted(self) -> None:
        document = copy.deepcopy(self.authority)
        document["determinism"]["forbidTimestampSuppression"] = False
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_two_independent_builds_are_required(self) -> None:
        self.assertEqual(self.authority["determinism"]["independentBuildCount"], 2)

    def test_a_mismatch_is_reported_never_forced(self) -> None:
        self.assertEqual(
            self.authority["determinism"]["mismatchAction"],
            "report-the-difference-never-force-a-match",
        )

    def test_a_single_build_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["determinism"]["independentBuildCount"] = 1
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_machine_id_is_an_empty_first_boot_file(self) -> None:
        self.assertEqual(self.authority["determinism"]["machineId"], "empty-file-first-boot")

    def test_ownership_and_file_order_are_fixed(self) -> None:
        determinism = self.authority["determinism"]
        self.assertEqual(determinism["ownership"], "root:root-only")
        self.assertEqual(determinism["fileOrder"], "sorted-by-logical-path-bytes")
        self.assertEqual(determinism["canonicalMtime"], 0)


class RoleTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority = _authority()

    def test_the_input_role_is_the_rootfs_stage_output(self) -> None:
        self.assertEqual(
            self.authority["roles"]["input"],
            "oci-image-layout-single-uncompressed-layer-v1",
        )

    def test_the_three_boot_outputs_are_named(self) -> None:
        names = {row["name"] for row in self.authority["roles"]["outputs"]}
        self.assertEqual(names, {"guest-kernel", "guest-initrd", "guest-root-disk"})

    def test_the_initrd_is_uncompressed_so_gzip_cannot_vary(self) -> None:
        initrd = next(
            row for row in self.authority["roles"]["outputs"] if row["name"] == "guest-initrd"
        )
        self.assertEqual(initrd["format"], "cpio-newc-uncompressed")
        self.assertEqual(self.authority["determinism"]["initrdCompression"], "none")

    def test_the_kernel_output_is_an_uncompressed_arm64_image(self) -> None:
        kernel = next(
            row for row in self.authority["roles"]["outputs"] if row["name"] == "guest-kernel"
        )
        self.assertEqual(kernel["format"], "linux-arm64-image-uncompressed")

    def test_a_changed_role_set_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["roles"]["outputs"].append({"format": "raw", "name": "guest-swap"})
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)


class GeneratorPinTests(unittest.TestCase):
    def test_the_pinned_generator_digest_tracks_the_tool_on_disk(self) -> None:
        authority = _authority()
        self.assertEqual(
            authority["generator"]["sha256"],
            builder.image_builder_authority_sha256(builder.TOOL_PATH.read_bytes()),
            "editing the authority tool must invalidate the pin, not slide past it",
        )

    def test_the_generator_path_is_this_tool(self) -> None:
        self.assertEqual(_authority()["generator"]["path"], builder.GENERATOR_PATH)

    def test_a_foreign_generator_path_is_refused(self) -> None:
        document = _authority()
        document["generator"]["path"] = "scripts/something_else.py"
        with self.assertRaises(builder.ImageBuilderAuthorityError):
            builder.validate_authority(document)

    def test_the_blanked_digest_ignores_the_authority_literal(self) -> None:
        raw = builder.TOOL_PATH.read_bytes()
        patched = raw.replace(
            builder.AUTHORITY_SHA256.encode("ascii"), b"f" * 64, 1
        )
        self.assertNotEqual(raw, patched)
        self.assertEqual(
            builder.image_builder_authority_sha256(raw),
            builder.image_builder_authority_sha256(patched),
        )


class NoProductionSigningMaterialTests(unittest.TestCase):
    """Nothing in this slice may reach for a signing identity."""

    FORBIDDEN = (
        "provisioning",
        "codesign",
        "notarytool",
        "teamid",
        "team_id",
        "-----BEGIN",
    )

    def test_the_document_names_no_signing_material(self) -> None:
        text = builder.AUTHORITY_PATH.read_text().lower()
        for needle in self.FORBIDDEN:
            self.assertNotIn(needle.lower(), text)

    def test_the_tool_names_no_signing_material(self) -> None:
        text = builder.TOOL_PATH.read_text().lower()
        for needle in self.FORBIDDEN:
            self.assertNotIn(needle.lower(), text)

    def test_signing_material_stays_prohibited(self) -> None:
        self.assertIs(_authority()["prohibitions"]["forbidProductionSigningMaterial"], True)


if __name__ == "__main__":
    unittest.main()
