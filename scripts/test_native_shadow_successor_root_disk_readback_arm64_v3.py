#!/usr/bin/env python3
"""Contract tests for the launcher-v2 successor root-disk readback."""

from __future__ import annotations

import hashlib
import inspect
import json
import os
import pathlib
import stat
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_successor_root_disk_readback_arm64_v3 as readback


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]


def digest(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class FixedRepositoryBindingsTests(unittest.TestCase):
    def test_the_four_repository_bindings_are_fixed_before_any_image_effect(self) -> None:
        expected = {
            "preregistration": (
                "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-preregistration-arm64-v1.json",
                "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
            ),
            "import-closure-correction": (
                "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-import-closure-correction-arm64-v1.json",
                "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
            ),
            "source-lock-v2": (
                "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json",
                "1a1a1df9b61795a46e82f392bda82d29c0cbde0473a11efd1f1cbd7993a85a9f",
            ),
            "launcher-result-v2": (
                "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
                "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08",
            ),
        }
        self.assertEqual(readback.repository_bindings(), expected)
        for relative, sealed in expected.values():
            path = REPOSITORY_ROOT / relative
            self.assertTrue(path.is_file())
            self.assertFalse(path.is_symlink())
            self.assertEqual(digest(path), sealed)

    def test_the_fixed_documents_load_and_name_launcher_v2(self) -> None:
        sealed = readback.load_repository_bindings()
        self.assertEqual(
            sealed.launcher_sha256,
            "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd",
        )
        self.assertEqual(sealed.launcher_size_bytes, 2_025_192)
        self.assertIs(sealed.preregistration["authorisations"]["imageProductionAuthorised"], False)
        self.assertEqual(sealed.import_closure_correction["effectiveBinding"]["effectiveUniqueBindings"], 41)
        self.assertIs(sealed.source_lock["activationAllowed"], False)

    def test_all_forty_one_effective_bindings_are_live_and_validated(self) -> None:
        preregistration = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-preregistration-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        correction = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-import-closure-correction-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        bindings = preregistration["bindings"] + correction["addedBindings"]
        self.assertEqual(len(bindings), 41)
        self.assertEqual(len({row["path"] for row in bindings}), 41)
        for row in bindings:
            path = REPOSITORY_ROOT / row["path"]
            self.assertTrue(path.is_file(), row["path"])
            self.assertFalse(path.is_symlink(), row["path"])
            self.assertEqual(path.stat().st_size, row["sizeBytes"], row["path"])
            self.assertEqual(digest(path), row["sha256"], row["path"])

        tampered = json.loads(json.dumps(preregistration))
        tampered["bindings"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(readback.ReadbackV3Error, "binding"):
            readback._validate_effective_bindings(tampered, correction)

        wrong_correction = json.loads(json.dumps(correction))
        wrong_correction["addedBindings"][0]["sha256"] = "0" * 64
        with self.assertRaisesRegex(readback.ReadbackV3Error, "binding"):
            readback._validate_effective_bindings(preregistration, wrong_correction)

    def test_launcher_v2_must_name_the_fixed_guest_path(self) -> None:
        document = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
            ).read_text(encoding="utf-8")
        )
        document["launcher"]["guestLogicalPath"] = "/tmp/not-the-launcher"
        with self.assertRaisesRegex(readback.ReadbackV3Error, "guest path"):
            readback._validate_launcher_result(document)

    def test_a_digest_change_is_refused(self) -> None:
        original = readback._read_regular_bytes

        def changed(path: pathlib.Path) -> bytes:
            raw = original(path)
            if path.name.endswith("import-closure-correction-arm64-v1.json"):
                return raw + b"\n"
            return raw

        with mock.patch.object(readback, "_read_regular_bytes", side_effect=changed):
            with self.assertRaisesRegex(readback.ReadbackV3Error, "digest"):
                readback.load_repository_bindings()

    def test_v1_launcher_result_is_rejected_even_if_presented_as_json(self) -> None:
        predecessor = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        with self.assertRaisesRegex(readback.ReadbackV3Error, "launcher v2"):
            readback._validate_launcher_result(predecessor)

    def test_boolean_and_integer_contract_fields_are_not_interchangeable(self) -> None:
        document = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-preregistration-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        document["authorisations"]["imageProductionAuthorised"] = 0
        with self.assertRaisesRegex(readback.ReadbackV3Error, "boolean"):
            readback._validate_preregistration(document)

        document = json.loads(json.dumps(document))
        document["authorisations"]["imageProductionAuthorised"] = False
        document["authorisations"]["imageProductionRunsAllowed"] = False
        with self.assertRaisesRegex(readback.ReadbackV3Error, "integer"):
            readback._validate_preregistration(document)

    def test_readback_contract_boolean_keys_are_exact_and_have_no_duplicate(self) -> None:
        expected = (
            "failureCannotEnterQualifiedComparison",
            "fallbackToV1Forbidden",
            "qualificationRequiresReadbackPass",
            "v1LauncherMustBeRejected",
            "wrapperCallsOnlyReadbackV3",
        )
        self.assertEqual(readback.READBACK_CONTRACT_TRUE_KEYS, expected)
        self.assertEqual(
            len(readback.READBACK_CONTRACT_TRUE_KEYS),
            len(set(readback.READBACK_CONTRACT_TRUE_KEYS)),
        )


class SpyEffects:
    def __init__(self) -> None:
        self.calls: list[tuple[object, ...]] = []
        self.tree: dict[str, dict[str, object]] = {}
        self.fail_at: str | None = None
        self.on_read_tree = None
        self.mountpoint_modes: list[int] = []

    def unmet_requirements(self) -> list[str]:
        self.calls.append(("requirements",))
        return []

    def setup_loop(self, image: readback.PinnedFile) -> str:
        self.calls.append(("setup-loop", image.path.name))
        if self.fail_at == "setup-loop":
            raise readback.ReadbackV3Error("setup failed")
        return "/dev/loop42"

    def mount(self, device: str, mountpoint: pathlib.Path) -> None:
        self.calls.append(("mount", device, readback.MOUNT_OPTIONS))
        self.mountpoint_modes.append(mountpoint.stat().st_mode & 0o777)
        if self.fail_at == "mount":
            raise readback.ReadbackV3Error("mount failed")

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, object]]:
        self.calls.append(("read-tree", mountpoint.name))
        if self.fail_at == "read-tree":
            raise readback.ReadbackV3Error("tree failed")
        if self.on_read_tree is not None:
            self.on_read_tree()
        return self.tree

    def unmount(self, mountpoint: pathlib.Path) -> None:
        self.calls.append(("unmount", mountpoint.name))
        if self.fail_at == "unmount":
            raise readback.ReadbackV3Error("unmount failed")

    def detach_loop(self, device: str) -> None:
        self.calls.append(("detach-loop", device))
        if self.fail_at == "detach-loop":
            raise readback.ReadbackV3Error("detach failed")


class ReadOnlyLoopLifecycleTests(unittest.TestCase):
    def setUp(self) -> None:
        preregistration_patch = mock.patch.object(
            readback,
            "PREREGISTRATION_SHA256",
            "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
        )
        preregistration_patch.start()
        self.addCleanup(preregistration_patch.stop)
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)
        self.outputs = self.root / "outputs"
        self.outputs.mkdir()
        (self.outputs / "guest-kernel").write_bytes(b"kernel")
        (self.outputs / "guest-initrd").write_bytes(b"initrd")
        (self.outputs / "guest-root-disk").write_bytes(b"disk")
        self.effects = SpyEffects()
        self.passing_report = {
            "activationAllowed": False,
            "bootableClaim": False,
            "checks": [
                {"detail": "test fixture", "id": identifier, "ok": True}
                for identifier in sorted(readback.image_verify.REQUIRED_CHECKS)
            ],
            "guestBootVerified": False,
            "passed": True,
        }

    def _verify(self) -> dict[str, object]:
        with mock.patch.object(
            readback.image_verify, "verify_tree", return_value=self.passing_report
        ):
            return readback.verify(outputs=self.outputs, effects=self.effects)

    def _assert_promotion(self, document: dict[str, object]) -> None:
        readback.assert_qualified_for_replica_comparison(
            document,
            expected_image=readback._file_identity(
                self.outputs / "guest-root-disk", "root-disk"
            ),
            expected_entry_count=len(self.effects.tree),
        )

    def test_a_passing_readback_uses_a_read_only_loop_and_cleans_both_layers(self) -> None:
        document = self._verify()
        self.assertEqual(
            self.effects.calls,
            [
                ("requirements",),
                ("setup-loop", "guest-root-disk"),
                ("mount", "/dev/loop42", ("ro", "nodev", "noexec", "nosuid")),
                ("read-tree", "successor-root-disk-readback-v3"),
                ("unmount", "successor-root-disk-readback-v3"),
                ("detach-loop", "/dev/loop42"),
            ],
        )
        self.assertEqual(document["status"], readback.PASS_STATUS)
        self.assertTrue(document["qualifiedForReplicaComparison"])
        self.assertEqual(
            document["importClosureCorrection"],
            {
                "path": readback.IMPORT_CORRECTION_PATH,
                "sha256": readback.IMPORT_CORRECTION_SHA256,
            },
        )
        self.assertEqual(
            document["producerPreregistration"],
            {
                "path": readback.PREREGISTRATION_PATH,
                "sha256": readback.PREREGISTRATION_SHA256,
            },
        )

    def test_a_binding_digest_failure_happens_before_loop_setup_or_mount(self) -> None:
        original = readback._read_regular_bytes

        def changed(path: pathlib.Path) -> bytes:
            raw = original(path)
            if path.name == pathlib.Path(readback.SOURCE_LOCK_PATH).name:
                return raw + b" "
            return raw

        with mock.patch.object(readback, "_read_regular_bytes", side_effect=changed):
            with self.assertRaisesRegex(readback.ReadbackV3Error, "digest"):
                self._verify()
        self.assertEqual(self.effects.calls, [])

    def test_unmount_and_detach_run_when_tree_reading_fails(self) -> None:
        self.effects.fail_at = "read-tree"
        with self.assertRaisesRegex(readback.ReadbackV3Error, "tree failed"):
            self._verify()
        self.assertIn(("unmount", "successor-root-disk-readback-v3"), self.effects.calls)
        self.assertIn(("detach-loop", "/dev/loop42"), self.effects.calls)
        self.assertFalse(
            (self.outputs.parent / "successor-root-disk-readback-v3").exists()
        )

    def test_a_mount_failure_attempts_unmount_before_detaching(self) -> None:
        self.effects.fail_at = "mount"
        with self.assertRaisesRegex(readback.ReadbackV3Error, "mount failed"):
            self._verify()
        self.assertIn(("unmount", "successor-root-disk-readback-v3"), self.effects.calls)
        self.assertIn(("detach-loop", "/dev/loop42"), self.effects.calls)
        self.assertFalse(
            (self.outputs.parent / "successor-root-disk-readback-v3").exists()
        )

    def test_a_post_binding_mount_failure_is_also_disowned(self) -> None:
        self.effects.fail_at = "mount"
        result = self.outputs / readback.RESULT_NAME
        with mock.patch.object(
            readback.image_verify, "verify_tree", return_value=self.passing_report
        ):
            with self.assertRaisesRegex(readback.ReadbackV3Error, "mount failed"):
                readback.verify(
                    outputs=self.outputs,
                    effects=self.effects,
                )
        document = json.loads(result.read_text(encoding="utf-8"))
        self.assertEqual(document["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertEqual(document["failureStage"], "readback-effects")
        self.assertTrue((self.outputs / readback.UNQUALIFIED_NAME).is_file())

    def test_cleanup_failure_is_a_hard_stop_and_the_second_cleanup_is_still_tried(self) -> None:
        self.effects.fail_at = "unmount"
        with self.assertRaisesRegex(readback.CleanupHardStop, "unmount failed"):
            self._verify()
        self.assertIn(("detach-loop", "/dev/loop42"), self.effects.calls)
        self.assertTrue(
            (self.outputs.parent / "successor-root-disk-readback-v3").exists()
        )

    def test_loop_detach_failure_is_also_a_hard_stop(self) -> None:
        self.effects.fail_at = "detach-loop"
        with self.assertRaisesRegex(readback.CleanupHardStop, "loop detach failed"):
            self._verify()

    def test_a_failed_verification_is_disowned_and_cannot_be_promoted(self) -> None:
        failed = json.loads(json.dumps(self.passing_report))
        failed["checks"][0]["ok"] = False
        failed["passed"] = False
        result = self.outputs / readback.RESULT_NAME
        with mock.patch.object(readback.image_verify, "verify_tree", return_value=failed):
            with self.assertRaisesRegex(readback.ReadbackV3Error, "UNQUALIFIED"):
                readback.verify(
                    outputs=self.outputs,
                    effects=self.effects,
                )
        document = json.loads(result.read_text(encoding="utf-8"))
        diagnostic = json.loads(
            (self.outputs / readback.UNQUALIFIED_NAME).read_text(encoding="utf-8")
        )
        self.assertEqual(document["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertIs(document["mayEnterQualification"], False)
        self.assertIs(document["qualifiedForReplicaComparison"], False)
        self.assertEqual(diagnostic["artifactClass"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertIs(diagnostic["mayBeAdopted"], False)
        self.assertIs(diagnostic["mayBeBooted"], False)

        document["mayEnterQualification"] = True
        document["qualifiedForReplicaComparison"] = True
        with self.assertRaises(readback.ReadbackV3Error):
            self._assert_promotion(document)

    def test_a_verifier_exception_after_cleanup_is_also_disowned(self) -> None:
        result = self.outputs / readback.RESULT_NAME
        with mock.patch.object(
            readback.image_verify,
            "verify_tree",
            side_effect=readback.image_verify.ImageVerifyError("bad expectations"),
        ):
            with self.assertRaisesRegex(
                readback.image_verify.ImageVerifyError, "bad expectations"
            ):
                readback.verify(
                    outputs=self.outputs,
                    effects=self.effects,
                )
        document = json.loads(result.read_text(encoding="utf-8"))
        self.assertEqual(document["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertEqual(document["failureStage"], "tree-verification")
        self.assertTrue((self.outputs / readback.UNQUALIFIED_NAME).is_file())

    def test_replica_promotion_requires_literal_booleans_not_integer_surrogates(self) -> None:
        document = self._verify()
        document["qualifiedForReplicaComparison"] = 1
        with self.assertRaisesRegex(readback.ReadbackV3Error, "replica comparison"):
            self._assert_promotion(document)

    def test_root_disk_digests_stream_instead_of_reading_the_image_into_memory(self) -> None:
        original = pathlib.Path.read_bytes

        def guarded(path: pathlib.Path) -> bytes:
            if path.name == "guest-root-disk":
                raise AssertionError("root disk read_bytes is forbidden")
            return original(path)

        with mock.patch.object(pathlib.Path, "read_bytes", guarded):
            document = self._verify()
        self.assertEqual(
            document["image"]["sha256"], hashlib.sha256(b"disk").hexdigest()
        )
        for helper in (readback._result_document, readback._failure_document):
            self.assertNotIn("image.read_bytes", inspect.getsource(helper))

    def test_result_path_is_fixed_inside_outputs_and_cannot_be_overridden(self) -> None:
        document = self._verify()
        fixed = self.outputs / readback.RESULT_NAME
        self.assertTrue(fixed.is_file())
        self.assertEqual(json.loads(fixed.read_text(encoding="utf-8")), document)
        self.assertEqual(set(inspect.signature(readback.verify).parameters), {"outputs", "effects"})
        options = {
            option
            for action in readback._parser()._actions
            for option in action.option_strings
        }
        self.assertNotIn("--result", options)

    def test_an_existing_or_symlinked_fixed_result_is_never_overwritten(self) -> None:
        result = self.outputs / readback.RESULT_NAME
        result.write_text("operator-owned\n", encoding="utf-8")
        with self.assertRaisesRegex(readback.ReadbackV3Error, "result|exists|overwrite"):
            self._verify()
        self.assertEqual(result.read_text(encoding="utf-8"), "operator-owned\n")
        self.assertEqual(self.effects.calls, [])

        result.unlink()
        self.effects.calls.clear()
        target = self.root / "outside-result.json"
        target.write_text("outside\n", encoding="utf-8")
        result.symlink_to(target)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "result|symlink|regular"):
            self._verify()
        self.assertEqual(target.read_text(encoding="utf-8"), "outside\n")
        self.assertEqual(self.effects.calls, [])

    def test_an_existing_or_symlinked_unqualified_path_blocks_all_image_effects(self) -> None:
        unqualified = self.outputs / readback.UNQUALIFIED_NAME
        unqualified.write_text("operator-owned\n", encoding="utf-8")
        with self.assertRaisesRegex(readback.ReadbackV3Error, "unqualified|exists|overwrite"):
            self._verify()
        self.assertEqual(unqualified.read_text(encoding="utf-8"), "operator-owned\n")
        self.assertEqual(self.effects.calls, [])

        unqualified.unlink()
        target = self.root / "outside-unqualified.json"
        target.write_text("outside\n", encoding="utf-8")
        unqualified.symlink_to(target)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "unqualified|symlink|regular"):
            self._verify()
        self.assertEqual(target.read_text(encoding="utf-8"), "outside\n")
        self.assertEqual(self.effects.calls, [])

    def test_a_preexisting_qualified_pending_path_blocks_all_image_effects(self) -> None:
        pending = self.outputs / readback.QUALIFIED_PENDING_NAME
        pending.write_text("operator-owned\n", encoding="utf-8")
        with self.assertRaisesRegex(
            readback.ReadbackV3Error,
            "pending|exists|overwrite",
        ):
            self._verify()
        self.assertEqual(pending.read_text(encoding="utf-8"), "operator-owned\n")
        self.assertEqual(self.effects.calls, [])

    def test_outputs_must_be_a_real_directory_and_outputs_are_distinct_regular_files(self) -> None:
        real_outputs = self.root / "real-outputs"
        self.outputs.rename(real_outputs)
        self.outputs.symlink_to(real_outputs, target_is_directory=True)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "outputs|directory|symlink"):
            self._verify()
        self.assertFalse(any(call[0] == "setup-loop" for call in self.effects.calls))

        self.outputs.unlink()
        real_outputs.rename(self.outputs)
        self.effects.calls.clear()
        root_disk = self.outputs / "guest-root-disk"
        root_disk.unlink()
        os.link(self.outputs / "guest-kernel", root_disk)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "distinct|inode|alias"):
            self._verify()
        self.assertEqual(self.effects.calls, [])

    def test_root_disk_symlink_is_rejected_before_loop_setup(self) -> None:
        root_disk = self.outputs / "guest-root-disk"
        root_disk.unlink()
        target = self.root / "disk-target"
        target.write_bytes(b"disk")
        root_disk.symlink_to(target)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "regular file"):
            self._verify()
        self.assertFalse(any(call[0] == "setup-loop" for call in self.effects.calls))

    def test_every_pair_of_output_files_must_have_distinct_inodes(self) -> None:
        names = tuple(readback.OUTPUT_FILES.values())
        for left_index, left in enumerate(names):
            for right in names[left_index + 1 :]:
                with self.subTest(left=left, right=right):
                    isolated = self.root / f"{left}-{right}"
                    isolated.mkdir()
                    for name in names:
                        (isolated / name).write_bytes(name.encode("utf-8"))
                    (isolated / right).unlink()
                    os.link(isolated / left, isolated / right)
                    effects = SpyEffects()
                    with self.assertRaisesRegex(
                        readback.ReadbackV3Error, "distinct|inode|alias"
                    ):
                        with mock.patch.object(
                            readback.image_verify,
                            "verify_tree",
                            return_value=self.passing_report,
                        ):
                            readback.verify(outputs=isolated, effects=effects)
                    self.assertFalse(any(call[0] == "setup-loop" for call in effects.calls))

    def test_mountpoint_is_fixed_fresh_private_and_removed_after_success(self) -> None:
        mountpoint = self.outputs.parent / "successor-root-disk-readback-v3"
        document = self._verify()
        self.assertEqual(document["status"], readback.PASS_STATUS)
        self.assertEqual(self.effects.mountpoint_modes, [0o700])
        self.assertFalse(mountpoint.exists())
        self.assertEqual(set(inspect.signature(readback.verify).parameters), {"outputs", "effects"})
        options = {
            option
            for action in readback._parser()._actions
            for option in action.option_strings
        }
        self.assertNotIn("--mountpoint", options)

    def test_a_preexisting_mountpoint_is_rejected_before_loop_setup(self) -> None:
        mountpoint = self.outputs.parent / "successor-root-disk-readback-v3"
        mountpoint.mkdir()
        (mountpoint / "sentinel").write_text("do not mount over me\n", encoding="utf-8")
        with self.assertRaisesRegex(readback.ReadbackV3Error, "mountpoint|fresh|exists"):
            self._verify()
        self.assertFalse(any(call[0] == "setup-loop" for call in self.effects.calls))
        self.assertEqual((mountpoint / "sentinel").read_text(), "do not mount over me\n")

    def test_root_disk_identity_and_digest_must_not_drift_during_readback(self) -> None:
        root_disk = self.outputs / "guest-root-disk"

        def replace_image() -> None:
            replacement = self.root / "replacement"
            replacement.write_bytes(b"changed after loop setup")
            root_disk.unlink()
            replacement.rename(root_disk)

        self.effects.on_read_tree = replace_image
        with self.assertRaisesRegex(readback.ReadbackV3Error, "changed|drift|identity|digest"):
            self._verify()
        result = self.outputs / readback.RESULT_NAME
        if result.exists():
            document = json.loads(result.read_text(encoding="utf-8"))
            self.assertEqual(document["status"], readback.FAILURE_STATUS)
            self.assertIs(document["qualifiedForReplicaComparison"], False)

    def test_loop_setup_uses_the_pinned_root_disk_inode_not_a_swapped_path(self) -> None:
        root_disk = self.outputs / "guest-root-disk"
        parked_original = self.root / "parked-original-root-disk"

        class AbaSwapEffects(SpyEffects):
            def __init__(self) -> None:
                super().__init__()
                self.attached_bytes: bytes | None = None

            def setup_loop(self, image) -> str:
                self.calls.append(("setup-loop", "guest-root-disk"))
                root_disk.rename(parked_original)
                root_disk.write_bytes(b"ATTACKER-EXT4")
                descriptor = getattr(image, "descriptor", None)
                if descriptor is None:
                    self.attached_bytes = pathlib.Path(image).read_bytes()
                else:
                    self.attached_bytes = os.pread(descriptor, 1 << 20, 0)
                return "/dev/loop42"

            def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, object]]:
                self.calls.append(("read-tree", mountpoint.name))
                root_disk.unlink()
                parked_original.rename(root_disk)
                return self.tree

        effects = AbaSwapEffects()
        with mock.patch.object(
            readback.image_verify,
            "verify_tree",
            return_value=self.passing_report,
        ):
            with self.assertRaisesRegex(
                readback.ReadbackV3Error,
                "changed|identity|digest|path",
            ):
                readback.verify(outputs=self.outputs, effects=effects)
        self.assertEqual(effects.attached_bytes, b"disk")

    def test_kernel_and_initrd_identities_also_cannot_drift_during_readback(self) -> None:
        for role in ("guest-kernel", "guest-initrd"):
            with self.subTest(role=role):
                outputs = self.root / ("outputs-" + role)
                outputs.mkdir()
                for name, raw in (
                    ("guest-kernel", b"kernel"),
                    ("guest-initrd", b"initrd"),
                    ("guest-root-disk", b"disk"),
                ):
                    (outputs / name).write_bytes(raw)
                effects = SpyEffects()

                def replace_output(name: str = role) -> None:
                    target = outputs / name
                    replacement = outputs.parent / (name + "-replacement")
                    replacement.write_bytes(b"changed after loop setup")
                    target.unlink()
                    replacement.rename(target)

                effects.on_read_tree = replace_output
                with mock.patch.object(
                    readback.image_verify,
                    "verify_tree",
                    return_value=self.passing_report,
                ):
                    with self.assertRaisesRegex(
                        readback.ReadbackV3Error,
                        "changed|drift|identity|digest",
                    ):
                        readback.verify(outputs=outputs, effects=effects)

    def test_all_three_outputs_are_rechecked_after_tree_verification_before_pass(self) -> None:
        for role in ("guest-kernel", "guest-initrd", "guest-root-disk"):
            with self.subTest(role=role):
                outputs = self.root / ("post-verifier-" + role)
                outputs.mkdir()
                for name, raw in (
                    ("guest-kernel", b"kernel"),
                    ("guest-initrd", b"initrd"),
                    ("guest-root-disk", b"disk"),
                ):
                    (outputs / name).write_bytes(raw)
                effects = SpyEffects()

                def replace_during_verifier(*_args, name: str = role, **_kwargs):
                    target = outputs / name
                    replacement = outputs.parent / (name + "-after-verifier")
                    replacement.write_bytes(b"changed during tree verification")
                    target.unlink()
                    replacement.rename(target)
                    return self.passing_report

                with mock.patch.object(
                    readback.image_verify,
                    "verify_tree",
                    side_effect=replace_during_verifier,
                ):
                    with self.assertRaisesRegex(
                        readback.ReadbackV3Error,
                        "changed|identity|digest|verification",
                    ):
                        readback.verify(outputs=outputs, effects=effects)
                result = json.loads(
                    (outputs / readback.RESULT_NAME).read_text(encoding="utf-8")
                )
                self.assertEqual(result["status"], readback.FAILURE_STATUS)
                self.assertEqual(result["failureStage"], "post-verification-output-identity")
                self.assertIs(result["qualifiedForReplicaComparison"], False)

    def test_pass_is_not_published_if_an_output_changes_while_it_is_staged(self) -> None:
        original_write = readback._write_document_once
        for role in ("guest-kernel", "guest-initrd", "guest-root-disk"):
            with self.subTest(role=role):
                outputs = self.root / ("result-staging-" + role)
                outputs.mkdir()
                for name, raw in (
                    ("guest-kernel", b"kernel"),
                    ("guest-initrd", b"initrd"),
                    ("guest-root-disk", b"disk"),
                ):
                    (outputs / name).write_bytes(raw)
                effects = SpyEffects()

                def mutate_during_pass_staging(
                    path: pathlib.Path,
                    document: dict[str, object],
                    output_name: str = role,
                ) -> None:
                    original_write(path, document)
                    if document.get("status") == readback.PASS_STATUS:
                        target = outputs / output_name
                        replacement = self.root / (output_name + "-during-result-write")
                        replacement.write_bytes(b"changed while pass was staged")
                        target.unlink()
                        replacement.rename(target)

                with mock.patch.object(
                    readback.image_verify,
                    "verify_tree",
                    return_value=self.passing_report,
                ), mock.patch.object(
                    readback,
                    "_write_document_once",
                    side_effect=mutate_during_pass_staging,
                ):
                    with self.assertRaisesRegex(
                        readback.ReadbackV3Error,
                        "changed|identity|digest|publication|staging",
                    ):
                        readback.verify(outputs=outputs, effects=effects)

                result = json.loads(
                    (outputs / readback.RESULT_NAME).read_text(encoding="utf-8")
                )
                self.assertEqual(result["status"], readback.FAILURE_STATUS)
                self.assertEqual(
                    result["failureStage"],
                    "qualified-result-publication",
                )
                self.assertIs(result["qualifiedForReplicaComparison"], False)
                self.assertTrue((outputs / readback.UNQUALIFIED_NAME).is_file())
                self.assertFalse(
                    (outputs / readback.QUALIFIED_PENDING_NAME).exists()
                )

    def test_pending_cleanup_failure_rolls_back_the_fixed_qualified_name(self) -> None:
        with mock.patch.object(
            readback.image_verify,
            "verify_tree",
            return_value=self.passing_report,
        ), mock.patch.object(
            readback,
            "_remove_private_pending",
            side_effect=readback.CleanupHardStop("injected pending cleanup failure"),
        ):
            with self.assertRaisesRegex(
                readback.CleanupHardStop,
                "pending cleanup failure",
            ):
                readback.verify(outputs=self.outputs, effects=self.effects)

        self.assertFalse((self.outputs / readback.RESULT_NAME).exists())
        pending = self.outputs / readback.QUALIFIED_PENDING_NAME
        self.assertTrue(pending.is_file())
        self.assertFalse((self.outputs / readback.UNQUALIFIED_NAME).exists())

    def test_pass_requires_the_exact_verifier_schema_check_ids_and_booleans(self) -> None:
        variants = []
        missing = json.loads(json.dumps(self.passing_report))
        missing["checks"].pop()
        variants.append(("missing-check", missing))
        extra = json.loads(json.dumps(self.passing_report))
        extra["checks"].append({"detail": "extra", "id": "unknown", "ok": True})
        variants.append(("extra-check", extra))
        extra_field = json.loads(json.dumps(self.passing_report))
        extra_field["checks"][0]["unsealed"] = True
        variants.append(("extra-field", extra_field))
        extra_top_level = json.loads(json.dumps(self.passing_report))
        extra_top_level["unsealed"] = True
        variants.append(("extra-top-level", extra_top_level))
        integer_boolean = json.loads(json.dumps(self.passing_report))
        integer_boolean["activationAllowed"] = 0
        variants.append(("integer-boolean", integer_boolean))
        integer_passed = json.loads(json.dumps(self.passing_report))
        integer_passed["passed"] = 1
        variants.append(("integer-passed", integer_passed))
        integer_guest_boot = json.loads(json.dumps(self.passing_report))
        integer_guest_boot["guestBootVerified"] = 0
        variants.append(("integer-guest-boot", integer_guest_boot))
        integer_check = json.loads(json.dumps(self.passing_report))
        integer_check["checks"][0]["ok"] = 1
        variants.append(("integer-check", integer_check))
        unsafe_boolean = json.loads(json.dumps(self.passing_report))
        unsafe_boolean["bootableClaim"] = True
        variants.append(("unsafe-boolean", unsafe_boolean))

        for label, report in variants:
            with self.subTest(label=label):
                isolated = self.root / label
                isolated.mkdir()
                for name, raw in (
                    ("guest-kernel", b"kernel"),
                    ("guest-initrd", b"initrd"),
                    ("guest-root-disk", b"disk"),
                ):
                    (isolated / name).write_bytes(raw)
                effects = SpyEffects()
                with mock.patch.object(readback.image_verify, "verify_tree", return_value=report):
                    with self.assertRaisesRegex(
                        readback.ReadbackV3Error,
                        "schema|check|boolean|activation|bootable|verification",
                    ):
                        readback.verify(outputs=isolated, effects=effects)

    def test_replica_promotion_rejects_a_minimal_forged_document(self) -> None:
        forged = {
            "artifactClass": "QUALIFIED-READBACK",
            "mayEnterQualification": True,
            "qualifiedForReplicaComparison": True,
            "status": readback.PASS_STATUS,
            "verification": self.passing_report,
        }
        with self.assertRaisesRegex(
            readback.ReadbackV3Error,
            "schema|shape|binding|image|entry|promotion",
        ):
            self._assert_promotion(forged)

    def test_replica_promotion_binds_the_exact_result_shape_and_generation(self) -> None:
        document = self._verify()
        variants: list[tuple[str, dict[str, object]]] = []
        extra = json.loads(json.dumps(document))
        extra["unsealed"] = True
        variants.append(("top-level-shape", extra))
        for key in ("schema", "release"):
            changed = json.loads(json.dumps(document))
            changed[key] = "wrong"
            variants.append((key, changed))
        for key in (
            "sourceLock",
            "producerPreregistration",
            "importClosureCorrection",
            "launcherResult",
        ):
            changed = json.loads(json.dumps(document))
            changed[key]["sha256"] = "0" * 64
            variants.append((key, changed))
        changed_launcher = json.loads(json.dumps(document))
        changed_launcher["launcherResult"]["launcherSha256"] = "0" * 64
        variants.append(("launcher-executable", changed_launcher))
        for field, value in (
            ("name", "other-image"),
            ("sha256", "not-a-digest"),
            ("sizeBytes", False),
        ):
            changed = json.loads(json.dumps(document))
            changed["image"][field] = value
            variants.append((f"image-{field}", changed))
        for value in (False, -1):
            changed = json.loads(json.dumps(document))
            changed["entryCount"] = value
            variants.append((f"entry-count-{value!r}", changed))
        for key in ("activationAllowed", "bootableClaim", "guestBootVerified"):
            changed = json.loads(json.dumps(document))
            changed[key] = True
            variants.append((key, changed))

        for label, changed in variants:
            with self.subTest(label=label):
                with self.assertRaises(readback.ReadbackV3Error):
                    self._assert_promotion(changed)

    def test_binding_paths_have_no_cli_environment_or_image_override(self) -> None:
        self.assertEqual(
            set(inspect.signature(readback.verify).parameters),
            {"outputs", "effects"},
        )
        parser = readback._parser()
        option_strings = {
            option
            for action in parser._actions
            for option in action.option_strings
        }
        self.assertFalse(
            option_strings
            & {
                "--source-lock",
                "--launcher-result",
                "--preregistration",
                "--image-binding",
                "--mountpoint",
                "--result",
            }
        )


class HostCommandAndGenerationBoundaryTests(unittest.TestCase):
    def test_real_effects_spell_read_only_loop_mount_and_cleanup_commands(self) -> None:
        effects = readback.HostReadbackEffects()
        calls: list[tuple[list[str], tuple[int, ...]]] = []

        def run(argv: list[str], **kwargs) -> bytes:
            calls.append((argv, kwargs.get("pass_fds", ())))
            return (
                b"/dev/loop7\n"
                if pathlib.Path(argv[0]).name == "losetup" and "--show" in argv
                else b""
            )

        pinned = readback.PinnedFile(
            descriptor=57,
            identity=readback.FileIdentity(
                device=1,
                inode=2,
                mode=stat.S_IFREG | 0o444,
                uid=0,
                gid=0,
                mtime_ns=0,
                ctime_ns=0,
                sha256="0" * 64,
                size_bytes=1,
            ),
            path=pathlib.Path("/tmp/guest-root-disk"),
            role="root-disk",
        )
        with mock.patch.object(readback, "_run", side_effect=run):
            device = effects.setup_loop(pinned)
            effects.mount(device, pathlib.Path("/tmp/readback"))
            effects.unmount(pathlib.Path("/tmp/readback"))
            effects.detach_loop(device)

        self.assertEqual(
            calls,
            [
                (
                    [
                        "/usr/sbin/losetup",
                        "--find",
                        "--show",
                        "--read-only",
                        "/proc/self/fd/57",
                    ],
                    (57,),
                ),
                (
                    [
                        "/usr/bin/mount",
                        "-t",
                        "ext4",
                        "-o",
                        "ro,nodev,noexec,nosuid",
                        "/dev/loop7",
                        "/tmp/readback",
                    ],
                    (),
                ),
                (["/usr/bin/umount", "/tmp/readback"], ()),
                (["/usr/sbin/losetup", "--detach", "/dev/loop7"], ()),
            ],
        )

    def test_every_host_subprocess_is_absolute_and_time_bounded(self) -> None:
        completed = mock.Mock(returncode=0, stdout=b"", stderr=b"")
        with mock.patch.object(readback.subprocess, "run", return_value=completed) as run:
            readback._run(["/usr/bin/true"])
        argv = run.call_args.args[0]
        kwargs = run.call_args.kwargs
        self.assertTrue(pathlib.Path(argv[0]).is_absolute())
        self.assertIn("timeout", kwargs)
        self.assertEqual(kwargs["pass_fds"], ())
        self.assertIsInstance(kwargs["timeout"], (int, float))
        self.assertGreater(kwargs["timeout"], 0)

        source = inspect.getsource(readback.HostReadbackEffects)
        for relative in ('"losetup"', '"mount"', '"umount"'):
            self.assertNotIn(relative, source)

    def test_host_command_launch_errors_are_closed_as_readback_failures(self) -> None:
        with mock.patch.object(
            readback.subprocess,
            "run",
            side_effect=OSError("exec failed"),
        ):
            with self.assertRaisesRegex(readback.ReadbackV3Error, "exec failed|command"):
                readback._run(["/usr/bin/true"])

    def test_source_lock_v1_and_launcher_v1_are_both_rejected(self) -> None:
        source_v1 = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        launcher_v1 = json.loads(
            (
                REPOSITORY_ROOT
                / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        with self.assertRaisesRegex(readback.ReadbackV3Error, "source-lock v2"):
            readback._validate_source_lock(source_v1)
        with self.assertRaisesRegex(readback.ReadbackV3Error, "launcher v2"):
            readback._validate_launcher_result(launcher_v1)

    def test_only_the_safe_base_tree_reader_is_reused(self) -> None:
        source = pathlib.Path(readback.__file__).read_text(encoding="utf-8")
        self.assertIn("base_reader.tree_from_directory", source)
        for forbidden in (
            "native_shadow_successor_root_disk_readback_arm64_v2",
            "native_shadow_successor_produce_phase_arm64_v2",
            "base_reader.verify",
            "base_reader.sealed_launcher_sha256",
            "base_reader.output_paths",
            "base_reader.MOUNT_OPTIONS",
            "os.environ",
            "os.getenv",
        ):
            self.assertNotIn(forbidden, source)


if __name__ == "__main__":
    unittest.main()
