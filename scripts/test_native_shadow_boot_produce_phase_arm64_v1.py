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
import pathlib
import tempfile
import unittest

from scripts import native_shadow_boot_image_produce_arm64_v1 as producer
from scripts import native_shadow_boot_produce_phase_arm64_v1 as phase
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts.test_native_shadow_boot_initrd_arm64_v1 import SMALL, tar_bytes


REPO = pathlib.Path(__file__).resolve().parents[1]
BUILDER_AUTHORITY = REPO / (
    "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)
MODULE_SOURCE = (REPO / "scripts/native_shadow_boot_produce_phase_arm64_v1.py").read_text(
    encoding="utf-8"
)
LAYER = tar_bytes(SMALL)


def builder_authority() -> dict:
    return json.loads(BUILDER_AUTHORITY.read_text(encoding="utf-8"))


class ToolPathTests(unittest.TestCase):
    """The two tools are in the frozen tree, at the paths the authority froze."""

    def test_the_tool_paths_come_from_the_builder_authority(self) -> None:
        tree = pathlib.Path("/tree")
        found = phase.tool_paths(tree)
        expected = {
            row["role"]: str(tree / row["memberPath"].lstrip("./"))
            for row in builder_authority()["toolBinaries"]
        }
        self.assertEqual(found["mke2fs"], expected["ext4-image-writer"])
        self.assertEqual(found["debugfs"], expected["ext4-image-inspector"])

    def test_the_tool_member_paths_are_not_restated_here(self) -> None:
        """A second copy of a frozen path is a second thing that can drift."""

        for row in builder_authority()["toolBinaries"]:
            self.assertNotIn(row["memberPath"].lstrip("."), MODULE_SOURCE)

    def test_every_tool_path_stays_inside_the_tree(self) -> None:
        tree = pathlib.Path("/tree")
        for value in phase.tool_paths(tree).values():
            self.assertTrue(value.startswith(str(tree) + "/"), value)

    def test_the_config_the_tools_read_is_inside_the_tree_too(self) -> None:
        """`MKE2FS_CONFIG` pointing at the runner's own file would unfreeze it."""

        tree = pathlib.Path("/tree")
        self.assertEqual(
            phase.tool_paths(tree)["config"],
            str(tree / phase.MKE2FS_CONFIG_GUEST_PATH.lstrip("/")),
        )

    def test_a_tool_role_the_authority_does_not_name_is_refused(self) -> None:
        with self.assertRaises(phase.ProducePhaseError):
            phase.tool_paths(pathlib.Path("/tree"), authority={"toolBinaries": []})


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
            image=pathlib.Path("/out/guest-root-disk"),
            staging=pathlib.Path("/staging/root"),
        )
        self.assertEqual(plan["sizeBytes"]["pinned"], phase.pinned_size_bytes(LAYER))
        self.assertEqual(plan["sizeBytes"]["pinned"], plan["sizeBytes"]["required"])

    def test_the_plan_this_builds_names_the_frozen_tools(self) -> None:
        plan = phase.plan_for(
            layer=LAYER,
            tree=pathlib.Path("/tree"),
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


class BoundaryTests(unittest.TestCase):
    def test_producing_an_image_is_not_a_boot(self) -> None:
        self.assertIs(phase.BOOTABLE_CLAIM, False)
        self.assertIs(phase.ACTIVATION_ALLOWED, False)

    def test_nothing_in_this_module_reaches_the_network(self) -> None:
        for forbidden in ("urllib", "socket", "http", "requests"):
            self.assertNotIn(forbidden, MODULE_SOURCE)


if __name__ == "__main__":
    unittest.main()
