"""RED for the writer tree: the sealed set is unpacked, and only it.

The approved boundary is that the selected e2fsprogs is added as an image
production tool and that the 191 frozen guest packages are neither replaced
nor deleted.  A tree built from the two sealed packages is how that boundary
becomes a fact about the filesystem rather than an intention: the writer and
its two libraries land in a directory of their own, and nothing else does.

The pins are handed in here rather than read from the sealed records, because
a test cannot manufacture bytes that hash to a pinned digest.  One test checks
that the default pins are the sealed ones, which is the part that has to be
true of the real run.
"""

from __future__ import annotations

import hashlib
import io
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))

from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk  # noqa: E402
from scripts import (  # noqa: E402
    native_shadow_boot_root_disk_execute_arm64_v1 as execute,
)
from scripts import native_shadow_boot_writer_tree_arm64_v1 as mod  # noqa: E402

LIBRARY_DIRECTORY = root_disk.LIBRARY_DIRECTORY.lstrip("/")
WRITER_BYTES = b"mke2fs, the selected build" * 8
EXT2FS_BYTES = b"libext2fs 1.47.2" * 16
E2P_BYTES = b"libe2p 1.47.2" * 16


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def ar_archive(members: dict[str, bytes]) -> bytes:
    """A .deb is an `ar` archive, and this is the smallest one that reads back."""

    out = bytearray(b"!<arch>\n")
    for name, raw in members.items():
        header = f"{name:<16}{0:<12}{0:<6}{0:<6}{100644:<8}{len(raw):<10}".encode()
        out += header + b"`\n" + raw
        if len(raw) % 2:
            out += b"\n"
    return bytes(out)


def data_tar(entries: list[tarfile.TarInfo], payloads: dict[str, bytes]) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as handle:
        for info in entries:
            body = payloads.get(info.name)
            handle.addfile(info, io.BytesIO(body) if body is not None else None)
    return buffer.getvalue()


def regular(name: str, raw: bytes) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.size = len(raw)
    info.mode = 0o755
    return info


def symlink(name: str, target: str) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.type = tarfile.SYMTYPE
    info.linkname = target
    return info


class WriterTreeCase(unittest.TestCase):
    """One fake pair of packages, and pins that describe exactly them."""

    LIBRARIES = (
        ("libext2fs.so.2", "libext2fs.so.2.4", EXT2FS_BYTES),
        ("libe2p.so.2", "libe2p.so.2.3", E2P_BYTES),
    )

    def enterContext(self, cm):  # Python 3.9 has no unittest.enterContext
        entered = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return entered

    def setUp(self) -> None:
        self.root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.store = self.root / "cas"
        (self.store / "sha256").mkdir(parents=True)
        self.tree = self.root / "writer"
        self.zstd = self.identity_zstd()

    def identity_zstd(self) -> pathlib.Path:
        """A stand-in that passes bytes through, so the tar can be stored plain.

        The point under test is which members are taken and how they are
        checked, not whether zstd decompresses; a separate test runs the real
        one when the host has it.
        """

        path = self.root / "zstd"
        path.write_text("#!/bin/sh\nexec cat\n", encoding="utf-8")
        path.chmod(0o755)
        return path

    def store_object(self, raw: bytes) -> str:
        sha256 = digest(raw)
        (self.store / "sha256" / sha256).write_bytes(raw)
        return sha256

    def writer_package(self, *, writer: bytes = WRITER_BYTES) -> str:
        tar = data_tar(
            [regular("./usr/sbin/mke2fs", writer), regular("./usr/sbin/e2fsck", b"x")],
            {"./usr/sbin/mke2fs": writer, "./usr/sbin/e2fsck": b"x"},
        )
        return self.store_object(ar_archive({"debian-binary": b"2.0\n", "data.tar.zst": tar}))

    def library_package(self, *, libraries=None, target=None) -> str:
        rows = self.LIBRARIES if libraries is None else libraries
        entries: list[tarfile.TarInfo] = []
        payloads: dict[str, bytes] = {}
        for soname, versioned, raw in rows:
            real = f"./{LIBRARY_DIRECTORY}/{versioned}"
            entries.append(regular(real, raw))
            payloads[real] = raw
            link = target if target is not None else versioned
            entries.append(symlink(f"./{LIBRARY_DIRECTORY}/{soname}", link))
        tar = data_tar(entries, payloads)
        return self.store_object(ar_archive({"debian-binary": b"2.0\n", "data.tar.zst": tar}))

    def pins(self, *, writer_package: str, library_package: str, **overrides) -> dict:
        rows = [
            {
                "logicalPath": f"/{LIBRARY_DIRECTORY}/{soname}",
                "packageSha256": library_package,
                "sha256": digest(raw),
                "sizeBytes": len(raw),
                "soname": soname,
            }
            for soname, _, raw in self.LIBRARIES
        ]
        pins = {
            "libraries": rows,
            "writer": {
                "memberPath": "./usr/sbin/mke2fs",
                "packageSha256": writer_package,
                "sha256": digest(WRITER_BYTES),
                "sizeBytes": len(WRITER_BYTES),
                "treePath": "usr/sbin/mke2fs",
            },
        }
        pins.update(overrides)
        return pins

    def materialize(self, **overrides) -> dict:
        pins = self.pins(
            writer_package=self.writer_package(),
            library_package=self.library_package(),
            **overrides,
        )
        return mod.materialize(
            cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
        )


class MaterializeTests(WriterTreeCase):
    """RED 1: the two packages become a tree holding the sealed set and no more."""

    def test_the_writer_lands_where_the_pins_say_with_the_pinned_bytes(self) -> None:
        self.materialize()
        self.assertEqual((self.tree / "usr/sbin/mke2fs").read_bytes(), WRITER_BYTES)

    def test_the_writer_is_executable_because_it_has_to_run(self) -> None:
        self.materialize()
        self.assertTrue((self.tree / "usr/sbin/mke2fs").stat().st_mode & 0o111)

    def test_nothing_else_from_the_writer_package_comes_with_it(self) -> None:
        """The package ships a second e2fsck; the frozen one stays the checker."""

        self.materialize()
        self.assertFalse((self.tree / "usr/sbin/e2fsck").exists())
        self.assertEqual(
            sorted(path.name for path in (self.tree / "usr/sbin").iterdir()), ["mke2fs"]
        )

    def test_the_two_sealed_libraries_land_under_their_sonames(self) -> None:
        self.materialize()
        directory = self.tree / LIBRARY_DIRECTORY
        self.assertEqual(
            sorted(path.name for path in directory.iterdir()),
            ["libe2p.so.2", "libext2fs.so.2"],
        )

    def test_each_soname_is_a_real_file_and_not_a_link_to_follow(self) -> None:
        """A link is one more thing that could point somewhere else later.

        The versioned name a runtime package ships is only reachable through
        the soname symlink, so the bytes are written under the soname directly
        and there is nothing left in the tree to redirect.
        """

        self.materialize()
        for soname, _, raw in self.LIBRARIES:
            path = self.tree / LIBRARY_DIRECTORY / soname
            self.assertFalse(path.is_symlink(), soname)
            self.assertEqual(path.read_bytes(), raw, soname)

    def test_the_tree_passes_the_check_the_executor_makes_of_it(self) -> None:
        """Which is the boundary itself: the writer tree holds only its set."""

        self.materialize()
        self.assertEqual(
            sorted(execute.assert_writer_tree_is_only_the_sealed_set(self.tree)),
            ["libe2p.so.2", "libext2fs.so.2"],
        )

    def test_the_receipt_names_every_file_it_wrote_with_its_digest(self) -> None:
        receipt = self.materialize()
        self.assertEqual(receipt["writer"]["sha256"], digest(WRITER_BYTES))
        self.assertEqual(
            {row["soname"]: row["sha256"] for row in receipt["libraries"]},
            {soname: digest(raw) for soname, _, raw in self.LIBRARIES},
        )
        self.assertEqual(receipt["tree"], str(self.tree))


class RefusalTests(WriterTreeCase):
    """RED 2: every way the wrong bytes could arrive is a stop, not a warning."""

    def test_a_package_the_store_does_not_hold_is_refused(self) -> None:
        pins = self.pins(writer_package="0" * 64, library_package=self.library_package())
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("0" * 64, str(caught.exception))

    def test_a_package_whose_bytes_do_not_hash_to_its_pin_is_refused(self) -> None:
        stored = self.writer_package()
        (self.store / "sha256" / stored).write_bytes(b"something else entirely")
        pins = self.pins(writer_package=stored, library_package=self.library_package())
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("hashes to", str(caught.exception))

    def test_a_writer_member_whose_digest_differs_is_refused(self) -> None:
        pins = self.pins(
            writer_package=self.writer_package(writer=b"a different mke2fs"),
            library_package=self.library_package(),
        )
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("mke2fs", str(caught.exception))

    def test_a_library_whose_digest_differs_is_refused(self) -> None:
        spoiled = (
            ("libext2fs.so.2", "libext2fs.so.2.4", b"not the measured library"),
            ("libe2p.so.2", "libe2p.so.2.3", E2P_BYTES),
        )
        pins = self.pins(
            writer_package=self.writer_package(),
            library_package=self.library_package(libraries=spoiled),
        )
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("libext2fs.so.2", str(caught.exception))

    def test_a_soname_link_that_leaves_its_directory_is_refused(self) -> None:
        """A `../` target is how a package would reach outside the sealed set."""

        pins = self.pins(
            writer_package=self.writer_package(),
            library_package=self.library_package(target="../../../sbin/mke2fs"),
        )
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("outside", str(caught.exception))

    def test_an_absolute_soname_link_is_refused(self) -> None:
        pins = self.pins(
            writer_package=self.writer_package(),
            library_package=self.library_package(target="/usr/lib/libext2fs.so.2.4"),
        )
        with self.assertRaises(mod.WriterTreeError):
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )

    def test_a_member_the_package_does_not_carry_is_refused(self) -> None:
        pins = self.pins(
            writer_package=self.writer_package(), library_package=self.library_package()
        )
        pins["writer"]["memberPath"] = "./usr/sbin/mkfs.ext4"
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("mkfs.ext4", str(caught.exception))

    def test_a_package_with_no_zstd_data_member_is_refused(self) -> None:
        stored = self.store_object(
            ar_archive({"debian-binary": b"2.0\n", "data.tar.gz": b"nope"})
        )
        pins = self.pins(writer_package=stored, library_package=self.library_package())
        with self.assertRaises(mod.WriterTreeError) as caught:
            mod.materialize(
                cas_roots=[self.store], zstd=self.zstd, writer_tree=self.tree, pins=pins
            )
        self.assertIn("data member", str(caught.exception))

    def test_a_tree_that_already_holds_something_else_is_refused(self) -> None:
        """The tree is built for this run; an inherited file is not the set."""

        (self.tree / LIBRARY_DIRECTORY).mkdir(parents=True)
        (self.tree / LIBRARY_DIRECTORY / "libc.so.6").write_bytes(b"the runner's own")
        with self.assertRaises(mod.WriterTreeError) as caught:
            self.materialize()
        self.assertIn("libc.so.6", str(caught.exception))


class SealedPinsTests(unittest.TestCase):
    """RED 3: the pins the real run uses are the sealed ones, not these."""

    def test_the_writer_pin_is_the_one_the_plan_froze(self) -> None:
        writer = mod.sealed_pins()["writer"]
        self.assertEqual(writer["sha256"], root_disk.MKE2FS_SHA256)
        self.assertEqual(writer["sizeBytes"], root_disk.MKE2FS_SIZE_BYTES)
        self.assertEqual(writer["packageSha256"], root_disk.WRITER_PACKAGE_SHA256)

    def test_the_writer_member_path_is_the_one_the_selection_record_read(self) -> None:
        """The digest was measured from that member, so the path travels with it."""

        record = mod.selection_record()
        measured = record["controls"]["positive"]["writer"]
        self.assertEqual(
            mod.sealed_pins()["writer"]["memberPath"].lstrip("."), measured["memberPath"]
        )
        self.assertEqual(mod.sealed_pins()["writer"]["sha256"], measured["sha256"])

    def test_the_libraries_are_exactly_the_ones_the_plan_calls_the_writer_set(self) -> None:
        rows = mod.sealed_pins()["libraries"]
        expected = [
            row
            for row in root_disk.WRITER_LIBRARIES
            if row["origin"] == root_disk.ORIGIN_WRITER_SET
        ]
        self.assertEqual(
            [row["soname"] for row in rows], [row["soname"] for row in expected]
        )
        for got, want in zip(rows, expected):
            self.assertEqual(got["sha256"], want["sha256"], want["soname"])
            self.assertEqual(got["sizeBytes"], want["sizeBytes"], want["soname"])
            self.assertEqual(got["packageSha256"], root_disk.WRITER_LIBRARY_PACKAGE_SHA256)

    def test_no_frozen_guest_library_is_taken_from_the_writer_package(self) -> None:
        """The five shared ones stay the guest's, which is the whole boundary."""

        taken = {row["soname"] for row in mod.sealed_pins()["libraries"]}
        guest = {
            row["soname"]
            for row in root_disk.WRITER_LIBRARIES
            if row["origin"] == root_disk.ORIGIN_FROZEN_GUEST
        }
        self.assertEqual(taken & guest, set())

    def test_the_measured_library_member_matches_the_positive_control(self) -> None:
        positive = mod.selection_record()["controls"]["positive"]["library"]
        rows = {row["soname"]: row for row in mod.sealed_pins()["libraries"]}
        self.assertEqual(rows["libext2fs.so.2"]["sha256"], positive["sha256"])
        self.assertEqual(rows["libext2fs.so.2"]["sizeBytes"], positive["sizeBytes"])

    def test_the_sealed_tree_would_satisfy_the_executor_s_own_check(self) -> None:
        """The sonames written are the ones the executor will insist on finding."""

        sealed = {
            row["soname"]
            for row in root_disk.WRITER_LIBRARIES
            if row["origin"] == root_disk.ORIGIN_WRITER_SET
        }
        self.assertEqual({row["soname"] for row in mod.sealed_pins()["libraries"]}, sealed)
        self.assertTrue(hasattr(execute, "assert_writer_tree_is_only_the_sealed_set"))


@unittest.skipUnless(shutil.which("zstd"), "no zstd on this host")
class RealZstdTests(WriterTreeCase):
    """The identity stand-in proves the wiring; this proves the compression."""

    def identity_zstd(self) -> pathlib.Path:
        return pathlib.Path(shutil.which("zstd"))

    def store_object(self, raw: bytes) -> str:
        return super().store_object(raw)

    def writer_package(self, *, writer: bytes = WRITER_BYTES) -> str:
        tar = data_tar([regular("./usr/sbin/mke2fs", writer)], {"./usr/sbin/mke2fs": writer})
        return self.store_object(
            ar_archive({"debian-binary": b"2.0\n", "data.tar.zst": self.compress(tar)})
        )

    def library_package(self, *, libraries=None, target=None) -> str:
        rows = self.LIBRARIES if libraries is None else libraries
        entries: list[tarfile.TarInfo] = []
        payloads: dict[str, bytes] = {}
        for soname, versioned, raw in rows:
            real = f"./{LIBRARY_DIRECTORY}/{versioned}"
            entries.append(regular(real, raw))
            payloads[real] = raw
            entries.append(
                symlink(
                    f"./{LIBRARY_DIRECTORY}/{soname}",
                    versioned if target is None else target,
                )
            )
        return self.store_object(
            ar_archive(
                {"debian-binary": b"2.0\n", "data.tar.zst": self.compress(data_tar(entries, payloads))}
            )
        )

    def compress(self, raw: bytes) -> bytes:
        return subprocess.run(
            [str(self.zstd), "-q", "-c"], input=raw, capture_output=True, check=True
        ).stdout

    def test_a_really_compressed_package_unpacks_the_same_way(self) -> None:
        self.materialize()
        self.assertEqual((self.tree / "usr/sbin/mke2fs").read_bytes(), WRITER_BYTES)
        for soname, _, raw in self.LIBRARIES:
            self.assertEqual((self.tree / LIBRARY_DIRECTORY / soname).read_bytes(), raw)


if __name__ == "__main__":
    unittest.main()
