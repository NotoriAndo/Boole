#!/usr/bin/env python3
"""Tests for the deterministic cpio newc writer that produces the guest initrd."""

from __future__ import annotations

import hashlib
import io
import tarfile
import unittest

from scripts import native_shadow_boot_initrd_arm64_v1 as mod


def tar_bytes(rows: list[dict], *, mtime: int = 0, sort: bool = True) -> bytes:
    """Build a layer tar in the shape the frozen OCI builder emits."""

    order = sorted(rows, key=lambda r: r["path"].encode("utf-8")) if sort else rows
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.PAX_FORMAT) as archive:
        for row in order:
            info = tarfile.TarInfo(row["path"])
            info.mode = row.get("mode", 0o644)
            info.uid = row.get("uid", 0)
            info.gid = row.get("gid", 0)
            info.uname = ""
            info.gname = ""
            info.mtime = row.get("mtime", mtime)
            info.pax_headers = {}
            kind = row.get("kind", "file")
            if kind == "directory":
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            elif kind == "symlink":
                info.type = tarfile.SYMTYPE
                info.linkname = row["target"]
                archive.addfile(info)
            elif kind == "hardlink":
                info.type = tarfile.LNKTYPE
                info.linkname = row["target"]
                archive.addfile(info)
            else:
                raw = row.get("raw", b"")
                info.type = tarfile.REGTYPE
                info.size = len(raw)
                archive.addfile(info, io.BytesIO(raw))
    return stream.getvalue()


SMALL = [
    {"path": "etc", "kind": "directory", "mode": 0o755},
    {"path": "etc/machine-id", "kind": "file", "mode": 0o444, "raw": b""},
    {"path": "usr", "kind": "directory", "mode": 0o755},
    {"path": "usr/bin", "kind": "directory", "mode": 0o755},
    {"path": "usr/bin/true", "kind": "file", "mode": 0o755, "raw": b"\x7fELF"},
    {"path": "lib", "kind": "symlink", "mode": 0o777, "target": "usr/lib"},
    {"path": "usr/lib", "kind": "directory", "mode": 0o755},
]


def records(raw: bytes) -> list[dict]:
    return mod.parse_newc(raw)


class HeaderTests(unittest.TestCase):
    def test_every_record_uses_the_newc_magic(self) -> None:
        raw = mod.initrd_bytes(tar_bytes(SMALL))
        self.assertTrue(raw.startswith(b"070701"))
        for row in records(raw):
            self.assertEqual(row["magic"], b"070701")

    def test_the_archive_ends_with_the_trailer(self) -> None:
        rows = records(mod.initrd_bytes(tar_bytes(SMALL)))
        self.assertEqual(rows[-1]["name"], "TRAILER!!!")
        self.assertEqual(rows[-1]["filesize"], 0)

    def test_headers_and_payloads_are_padded_to_four_bytes(self) -> None:
        """An unpadded newc archive is silently misread by the kernel."""

        raw = mod.initrd_bytes(tar_bytes(SMALL))
        self.assertEqual(len(raw) % 4, 0)
        for row in records(raw):
            self.assertEqual(row["headerEnd"] % 4, 0)
            self.assertEqual(row["dataEnd"] % 4, 0)


class DeterminismTests(unittest.TestCase):
    def test_the_same_layer_produces_the_same_bytes(self) -> None:
        first = mod.initrd_bytes(tar_bytes(SMALL))
        second = mod.initrd_bytes(tar_bytes(SMALL))
        self.assertEqual(hashlib.sha256(first).digest(), hashlib.sha256(second).digest())

    def test_entries_keep_the_layer_order(self) -> None:
        names = [r["name"] for r in records(mod.initrd_bytes(tar_bytes(SMALL)))][:-1]
        self.assertEqual(names, sorted(names, key=lambda n: n.encode("utf-8")))

    def test_timestamps_and_ownership_are_pinned(self) -> None:
        for row in records(mod.initrd_bytes(tar_bytes(SMALL)))[:-1]:
            self.assertEqual(row["mtime"], 0)
            self.assertEqual(row["uid"], 0)
            self.assertEqual(row["gid"], 0)

    def test_inode_numbers_come_from_the_position_not_the_host(self) -> None:
        """A host inode number would differ between the two independent jobs."""

        rows = records(mod.initrd_bytes(tar_bytes(SMALL)))[:-1]
        self.assertEqual([r["ino"] for r in rows], list(range(1, len(rows) + 1)))


class ContentTests(unittest.TestCase):
    def test_file_type_bits_are_carried_into_the_mode(self) -> None:
        by_name = {r["name"]: r for r in records(mod.initrd_bytes(tar_bytes(SMALL)))}
        self.assertEqual(by_name["etc"]["mode"] & 0o170000, 0o040000)
        self.assertEqual(by_name["usr/bin/true"]["mode"] & 0o170000, 0o100000)
        self.assertEqual(by_name["lib"]["mode"] & 0o170000, 0o120000)

    def test_permission_bits_survive(self) -> None:
        by_name = {r["name"]: r for r in records(mod.initrd_bytes(tar_bytes(SMALL)))}
        self.assertEqual(by_name["usr/bin/true"]["mode"] & 0o7777, 0o755)
        self.assertEqual(by_name["etc/machine-id"]["mode"] & 0o7777, 0o444)

    def test_a_symlink_stores_its_target_as_the_payload(self) -> None:
        by_name = {r["name"]: r for r in records(mod.initrd_bytes(tar_bytes(SMALL)))}
        self.assertEqual(by_name["lib"]["data"], b"usr/lib")

    def test_an_empty_file_stays_empty(self) -> None:
        by_name = {r["name"]: r for r in records(mod.initrd_bytes(tar_bytes(SMALL)))}
        self.assertEqual(by_name["etc/machine-id"]["data"], b"")
        self.assertEqual(by_name["etc/machine-id"]["filesize"], 0)

    def test_file_contents_survive(self) -> None:
        by_name = {r["name"]: r for r in records(mod.initrd_bytes(tar_bytes(SMALL)))}
        self.assertEqual(by_name["usr/bin/true"]["data"], b"\x7fELF")


class RejectionTests(unittest.TestCase):
    def test_an_unsorted_layer_is_rejected(self) -> None:
        """The contract says sorted by logical path bytes -- verified, not trusted."""

        raw = tar_bytes(list(reversed(SMALL)), sort=False)
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(raw)

    def test_a_nonzero_timestamp_is_rejected(self) -> None:
        rows = [dict(row) for row in SMALL]
        rows[0]["mtime"] = 1
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes(rows))

    def test_a_nonroot_owner_is_rejected(self) -> None:
        rows = [dict(row) for row in SMALL]
        rows[1]["uid"] = 1000
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes(rows))

    def test_a_hardlink_is_rejected_because_assembly_materializes_them(self) -> None:
        rows = [dict(row) for row in SMALL]
        rows.append({"path": "usr/bin/false", "kind": "hardlink", "target": "usr/bin/true"})
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes(rows))

    def test_an_absolute_path_is_rejected(self) -> None:
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes([{"path": "/etc", "kind": "directory"}]))

    def test_a_parent_traversal_is_rejected(self) -> None:
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes([{"path": "usr/../etc", "kind": "directory"}]))

    def test_an_empty_layer_is_rejected(self) -> None:
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(tar_bytes([]))

    def test_a_device_node_is_rejected(self) -> None:
        """Nothing in the frozen layer is a device, so one appearing is a surprise."""

        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w", format=tarfile.PAX_FORMAT) as archive:
            info = tarfile.TarInfo("dev/null")
            info.type = tarfile.CHRTYPE
            info.mode = 0o666
            info.mtime = 0
            archive.addfile(info)
        with self.assertRaises(mod.InitrdBuildError):
            mod.initrd_bytes(stream.getvalue())


class BoundaryTests(unittest.TestCase):
    def test_writing_an_initrd_is_not_a_boot(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)

    def test_the_compression_stays_none(self) -> None:
        """v1 froze `initrdCompression: none`, so there is no timestamp to suppress."""

        self.assertEqual(mod.COMPRESSION, "none")
        raw = mod.initrd_bytes(tar_bytes(SMALL))
        self.assertFalse(raw.startswith(b"\x1f\x8b"))


if __name__ == "__main__":
    unittest.main()
