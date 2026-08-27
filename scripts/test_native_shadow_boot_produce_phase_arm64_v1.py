#!/usr/bin/env python3
"""RED tests for the offline half of the arm64 boot image producer.

Every step the produce phase needs already exists.  What does not exist is the
thing that says which order they run in and where each one's inputs come from,
and that is the part with room to differ between two jobs that are supposed to
agree byte for byte.  So the values are pulled out of the frozen records rather
than chosen here, and these tests are mostly about that: the tool paths come
from the builder authority, the image size comes from the plan's own floor, the
output names come from the producer authority, and none of the three is written
down a second time in the module.

Running the phase needs Linux, root, and the acquired closure on disk.  These
tests run on the machine that writes them, so they drive what can be settled
without any of that: the derivations, the refusals, and the boundary.
"""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_image_produce_arm64_v1 as producer
from scripts import native_shadow_boot_produce_phase_arm64_v1 as phase
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts import native_shadow_boot_writer_tree_arm64_v1 as writer_tree
from scripts.test_native_shadow_boot_initrd_arm64_v1 import SMALL, tar_bytes


REPO = pathlib.Path(__file__).resolve().parents[1]
BUILDER_AUTHORITY = REPO / (
    "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)
MODULE_SOURCE = (REPO / "scripts/native_shadow_boot_produce_phase_arm64_v1.py").read_text(
    encoding="utf-8"
)
LAYER = tar_bytes(SMALL)
# The writer set is unpacked beside the frozen tree, never inside it.
WRITER = pathlib.Path("/writer")


def builder_authority() -> dict:
    return json.loads(BUILDER_AUTHORITY.read_text(encoding="utf-8"))


class ToolPathTests(unittest.TestCase):
    """The two tools are in the frozen tree, at the paths the authority froze."""

    def test_the_inspector_path_comes_from_the_builder_authority(self) -> None:
        tree = pathlib.Path("/tree")
        found = phase.tool_paths(tree, WRITER)
        expected = {
            row["role"]: str(tree / row["memberPath"].lstrip("./"))
            for row in builder_authority()["toolBinaries"]
        }
        self.assertEqual(found["debugfs"], expected["ext4-image-inspector"])

    def test_the_writer_path_comes_from_the_writer_tree(self) -> None:
        """The authority's writer row stays; it is no longer what runs."""

        found = phase.tool_paths(pathlib.Path("/tree"), WRITER)
        self.assertEqual(found["mke2fs"], str(WRITER / writer_tree.WRITER_TREE_PATH))

    def test_the_tool_member_paths_are_not_restated_here(self) -> None:
        """A second copy of a frozen path is a second thing that can drift."""

        for row in builder_authority()["toolBinaries"]:
            self.assertNotIn(row["memberPath"].lstrip("."), MODULE_SOURCE)

    def test_every_tool_path_stays_inside_one_of_the_two_trees(self) -> None:
        tree = pathlib.Path("/tree")
        roots = (str(tree) + "/", str(WRITER) + "/")
        for value in phase.tool_paths(tree, WRITER).values():
            self.assertTrue(value.startswith(roots), value)

    def test_the_checkers_and_the_config_stay_in_the_frozen_tree(self) -> None:
        """Only the writer moved; a checker that followed it would share its blind spots."""

        tree = pathlib.Path("/tree")
        found = phase.tool_paths(tree, WRITER)
        for name in ("config", "debugfs", "e2fsck"):
            self.assertTrue(found[name].startswith(str(tree) + "/"), found[name])

    def test_the_config_the_tools_read_is_inside_the_tree_too(self) -> None:
        """`MKE2FS_CONFIG` pointing at the runner's own file would unfreeze it."""

        tree = pathlib.Path("/tree")
        self.assertEqual(
            phase.tool_paths(tree, WRITER)["config"],
            str(tree / phase.MKE2FS_CONFIG_GUEST_PATH.lstrip("/")),
        )

    def test_a_tool_role_the_authority_does_not_name_is_refused(self) -> None:
        with self.assertRaises(phase.ProducePhaseError):
            phase.tool_paths(
                pathlib.Path("/tree"), WRITER, authority={"toolBinaries": []}
            )


class PinnedSizeTests(unittest.TestCase):
    """The size is derived from the layer, because a chosen one can differ."""

    def test_the_pinned_size_is_the_plan_s_own_floor(self) -> None:
        entries = root_disk.layer_entries(LAYER)
        self.assertEqual(
            phase.pinned_size_bytes(LAYER), root_disk.required_bytes(entries)
        )

    def test_the_pinned_size_is_a_whole_number_of_blocks(self) -> None:
        self.assertEqual(phase.pinned_size_bytes(LAYER) % root_disk.BLOCK_SIZE, 0)

    def test_the_same_layer_pins_the_same_size(self) -> None:
        self.assertEqual(phase.pinned_size_bytes(LAYER), phase.pinned_size_bytes(LAYER))

    def test_no_image_size_is_written_down_in_this_module(self) -> None:
        self.assertNotIn(str(phase.pinned_size_bytes(LAYER)), MODULE_SOURCE)

    def test_the_size_the_plan_accepts_is_the_size_this_pins(self) -> None:
        plan = phase.plan_for(
            layer=LAYER,
            tree=pathlib.Path("/tree"),
            writer_tree=WRITER,
            image=pathlib.Path("/out/guest-root-disk"),
            staging=pathlib.Path("/staging/root"),
        )
        self.assertEqual(plan["sizeBytes"]["pinned"], phase.pinned_size_bytes(LAYER))
        self.assertEqual(plan["sizeBytes"]["pinned"], plan["sizeBytes"]["required"])

    def test_the_plan_this_builds_names_the_frozen_tools(self) -> None:
        plan = phase.plan_for(
            layer=LAYER,
            tree=pathlib.Path("/tree"),
            writer_tree=WRITER,
            image=pathlib.Path("/out/guest-root-disk"),
            staging=pathlib.Path("/staging/root"),
        )
        self.assertEqual(plan["tools"]["mke2fs"]["sha256"], root_disk.MKE2FS_SHA256)
        self.assertEqual(plan["tools"]["debugfs"]["sha256"], root_disk.DEBUGFS_SHA256)
        self.assertIs(plan["executed"], False)


class LayerBlobTests(unittest.TestCase):
    """The layer that is staged must be the layer the build receipt named."""

    def blob(self, root: pathlib.Path, raw: bytes) -> str:
        digest = hashlib.sha256(raw).hexdigest()
        target = root / "blobs" / "sha256" / digest
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
        return f"sha256:{digest}"

    def test_the_layer_named_by_the_receipt_is_returned(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            oci = pathlib.Path(scratch)
            digest = self.blob(oci, LAYER)
            self.assertEqual(phase.layer_bytes(oci, {"layerDigest": digest}), LAYER)

    def test_a_layer_whose_bytes_do_not_hash_to_the_receipt_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            oci = pathlib.Path(scratch)
            digest = self.blob(oci, LAYER)
            target = oci / "blobs" / "sha256" / digest.split(":", 1)[1]
            target.write_bytes(LAYER + b"tampered")
            with self.assertRaises(phase.ProducePhaseError):
                phase.layer_bytes(oci, {"layerDigest": digest})

    def test_a_receipt_naming_no_layer_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            with self.assertRaises(phase.ProducePhaseError):
                phase.layer_bytes(pathlib.Path(scratch), {})

    def test_a_layer_blob_that_is_absent_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            with self.assertRaises(phase.ProducePhaseError):
                phase.layer_bytes(pathlib.Path(scratch), {"layerDigest": "sha256:" + "0" * 64})


class OutputTests(unittest.TestCase):
    def test_the_outputs_are_the_ones_the_producer_authority_names(self) -> None:
        document = producer.load_authority(REPO)
        self.assertEqual(phase.output_names(), producer.output_names(document))

    def test_the_output_names_are_not_restated_here(self) -> None:
        for name in phase.output_names():
            self.assertNotIn(f'"{name}"', MODULE_SOURCE)


class RefusalTests(unittest.TestCase):
    def test_producing_refuses_when_the_run_is_not_root(self) -> None:
        """`mke2fs -d` copies the staged owner, so a non-root run is not the image."""

        if phase.os.geteuid() == 0:
            self.skipTest("this host is root; the refusal cannot be observed here")
        with tempfile.TemporaryDirectory() as scratch:
            with self.assertRaises(phase.ProducePhaseError) as caught:
                phase.produce(
                    scratch=pathlib.Path(scratch),
                    outputs=pathlib.Path(scratch) / "out",
                    gpgv=pathlib.Path("/nonexistent/gpgv"),
                    zstd=pathlib.Path("/nonexistent/zstd"),
                    launcher=pathlib.Path("/nonexistent/launcher"),
                )
            self.assertIn("root", str(caught.exception).lower())


class TemporaryDirectoryTests(unittest.TestCase):
    """The unit is sealed read-only and started clean, so it has no /tmp.

    `systemd-run` does not carry the caller's environment into a transient
    unit, so the `TMPDIR` the driver exports never arrives; and the unit is
    sealed with the filesystem read-only except the paths it was given, so
    Python walks /tmp, /var/tmp, /usr/tmp and / and finds none of them
    writable.  The first run on the arm64 runner died on exactly that, before
    it had written anything.

    Binding the directory inside the phase rather than passing it in leaves no
    environment variable to forget: the phase is already told where its scratch
    is, and that is the one place it puts anything.
    """

    def setUp(self) -> None:
        previous_environ = os.environ.get("TMPDIR")
        previous_tempdir = tempfile.tempdir

        def restore() -> None:
            tempfile.tempdir = previous_tempdir
            if previous_environ is None:
                os.environ.pop("TMPDIR", None)
            else:
                os.environ["TMPDIR"] = previous_environ

        self.addCleanup(restore)

    def test_temporaries_land_in_the_scratch_the_unit_can_write(self) -> None:
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            bound = phase.bind_temporary_directory(root)
            self.assertEqual(bound, root / "tmp")
            self.assertTrue(bound.is_dir())
            self.assertEqual(os.environ["TMPDIR"], str(bound))
            self.assertEqual(tempfile.gettempdir(), str(bound))

    def test_the_children_it_starts_are_told_the_same_place(self) -> None:
        """`zstd`, `mke2fs` and `debugfs` read TMPDIR, not Python's idea of it."""

        with tempfile.TemporaryDirectory() as scratch:
            bound = phase.bind_temporary_directory(pathlib.Path(scratch))
            self.assertEqual(os.environ["TMPDIR"], str(bound))

    def test_the_command_binds_it_before_the_phase_can_need_one(self) -> None:
        """Even the run that is refused had somewhere to put a temporary."""

        if os.geteuid() == 0:
            self.skipTest("this host is root; the early refusal is not observable")
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            code = phase.main(
                [
                    "produce",
                    "--scratch", str(root),
                    "--outputs", str(root / "out"),
                    "--gpgv", "/nonexistent/gpgv",
                    "--zstd", "/nonexistent/zstd",
                    "--launcher", "/nonexistent/launcher",
                ]
            )
            self.assertEqual(code, 1)
            self.assertEqual(os.environ["TMPDIR"], str(root / "tmp"))

    def test_calling_the_phase_directly_changes_nothing_process_wide(self) -> None:
        """A caller in a shared process should not have its temporaries moved."""

        if os.geteuid() == 0:
            self.skipTest("this host is root; the early refusal is not observable")
        before = tempfile.gettempdir()
        with tempfile.TemporaryDirectory() as scratch:
            root = pathlib.Path(scratch)
            with self.assertRaises(phase.ProducePhaseError):
                phase.produce(
                    scratch=root,
                    outputs=root / "out",
                    gpgv=pathlib.Path("/nonexistent/gpgv"),
                    zstd=pathlib.Path("/nonexistent/zstd"),
                    launcher=pathlib.Path("/nonexistent/launcher"),
                )
        self.assertEqual(tempfile.gettempdir(), before)


class BoundaryTests(unittest.TestCase):
    def test_producing_an_image_is_not_a_boot(self) -> None:
        self.assertIs(phase.BOOTABLE_CLAIM, False)
        self.assertIs(phase.ACTIVATION_ALLOWED, False)

    def test_nothing_in_this_module_reaches_the_network(self) -> None:
        for forbidden in ("urllib", "socket", "http", "requests"):
            self.assertNotIn(forbidden, MODULE_SOURCE)


if __name__ == "__main__":
    unittest.main()
