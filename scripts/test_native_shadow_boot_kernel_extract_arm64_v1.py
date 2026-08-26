#!/usr/bin/env python3
"""RED tests for the frozen ARM64 guest kernel extraction.

The image builder authority pins which package the kernel comes from, which
member inside it is the kernel, and what both its compressed and expanded forms
must hash to.  Nothing yet turns those pins into an actual file.  These tests
describe the extractor that does, and they are written before it exists.

What the extractor is allowed to claim is narrow.  Running the same deterministic
pipeline twice is not the same evidence as compiling twice: decompression has no
freedom to differ, so a second run mostly rules out state leaking between runs --
a reused temporary directory, a partial file mistaken for a finished one.  The
tests below therefore check the guards, not a reproducibility claim the pipeline
cannot support.
"""

from __future__ import annotations

import hashlib
import io
import json
import pathlib
import struct
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_kernel_extract_arm64_v1 as kernel


REPO = pathlib.Path(__file__).resolve().parents[1]
AUTHORITY = REPO / "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"


def _ar_archive(members: list[tuple[str, bytes]]) -> bytes:
    """Build a minimal `ar` archive the way a .deb is laid out."""
    out = bytearray(b"!<arch>\n")
    for name, payload in members:
        header = (
            name.ljust(16).encode("ascii")
            + b"0".ljust(12)
            + b"0".ljust(6)
            + b"0".ljust(6)
            + b"100644".ljust(8)
            + str(len(payload)).ljust(10).encode("ascii")
            + b"`\n"
        )
        out += header + payload
        if len(payload) % 2:
            out += b"\n"
    return bytes(out)


def _tar_with(entries: list[tuple[str, bytes]], *, symlink: str | None = None) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as archive:
        for name, payload in entries:
            info = tarfile.TarInfo(name)
            info.size = len(payload)
            archive.addfile(info, io.BytesIO(payload))
        if symlink is not None:
            info = tarfile.TarInfo(symlink)
            info.type = tarfile.SYMTYPE
            info.linkname = "/etc/passwd"
            archive.addfile(info)
    return buf.getvalue()


def _arm64_image(size: int = 4096) -> bytes:
    body = bytearray(b"\x00" * size)
    body[0x38:0x3C] = b"ARM\x64"
    struct.pack_into("<Q", body, 0x08, 0x80000)
    return bytes(body)


class ConstantTests(unittest.TestCase):
    def test_the_result_status_says_extracted_and_not_boot(self) -> None:
        self.assertEqual(
            kernel.RESULT_STATUS,
            "KERNEL-IMAGE-EXTRACTED-REPRODUCIBLY-NOT-BOOT-AUTHORITY",
        )

    def test_the_release_and_schema_are_the_arm64_kernel_extract_ones(self) -> None:
        self.assertEqual(kernel.RELEASE, "NATIVE-SHADOW-BOOT-KERNEL-EXTRACT-ARM64-V1")
        self.assertEqual(
            kernel.RESULT_SCHEMA,
            "boole.native-shadow.boot-kernel-extract-result.arm64.v1",
        )

    def test_the_pinned_authority_digest_is_the_committed_authority(self) -> None:
        digest = hashlib.sha256(AUTHORITY.read_bytes()).hexdigest()
        self.assertEqual(kernel.AUTHORITY_SHA256, digest)


class AuthorityLoadTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = pathlib.Path(self.tmp.name) / "authority.json"

    def test_the_committed_authority_loads(self) -> None:
        document = kernel.load_authority(AUTHORITY)
        self.assertEqual(document["release"], "NATIVE-SHADOW-BOOT-IMAGE-BUILDER-AUTHORITY-ARM64-V1")

    def test_an_authority_whose_bytes_moved_is_refused(self) -> None:
        document = json.loads(AUTHORITY.read_text())
        document["release"] = "SOMETHING-ELSE"
        self.path.write_text(json.dumps(document))
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.load_authority(self.path)
        self.assertIn("authority", str(caught.exception).lower())


class KernelPinTests(unittest.TestCase):
    def setUp(self) -> None:
        self.pins = kernel.kernel_pins(kernel.load_authority(AUTHORITY))

    def test_the_pins_name_the_member_and_both_forms(self) -> None:
        self.assertEqual(self.pins["memberPath"], "./boot/vmlinuz-6.8.0-31-generic")
        self.assertEqual(self.pins["compression"], "gzip")
        self.assertEqual(
            self.pins["imageSha256"],
            "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336",
        )
        self.assertEqual(self.pins["imageSizeBytes"], 57860488)

    def test_the_pins_name_the_package_by_digest(self) -> None:
        self.assertEqual(
            self.pins["packageSha256"],
            "7d0168a6ea0ebc7263fed3cce3dc45153ddd4e0e4a4b2737f74f0bd72cbaa292",
        )


class Arm64MagicTests(unittest.TestCase):
    def test_a_real_arm64_header_is_accepted(self) -> None:
        kernel.assert_arm64_image(_arm64_image())

    def test_a_buffer_without_the_magic_is_refused(self) -> None:
        body = bytearray(_arm64_image())
        body[0x38:0x3C] = b"XXXX"
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.assert_arm64_image(bytes(body))
        self.assertIn("arm64", str(caught.exception).lower())

    def test_a_buffer_too_short_to_hold_the_magic_is_refused(self) -> None:
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_arm64_image(b"\x00" * 16)

    def test_the_magic_is_read_at_the_documented_offset(self) -> None:
        # A kernel that merely contains the bytes somewhere is not an arm64
        # image; the offset is what makes it a header field.
        body = bytearray(b"\x00" * 4096)
        body[0x10:0x14] = b"ARM\x64"
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_arm64_image(bytes(body))


class ByteVerificationTests(unittest.TestCase):
    def test_matching_bytes_pass(self) -> None:
        payload = b"kernel-ish"
        kernel.verify_bytes(
            payload,
            expected_sha256=hashlib.sha256(payload).hexdigest(),
            expected_size=len(payload),
            context="test payload",
        )

    def test_a_size_mismatch_names_the_size(self) -> None:
        payload = b"kernel-ish"
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.verify_bytes(
                payload,
                expected_sha256=hashlib.sha256(payload).hexdigest(),
                expected_size=len(payload) + 1,
                context="test payload",
            )
        self.assertIn("size", str(caught.exception).lower())

    def test_a_digest_mismatch_names_the_digest(self) -> None:
        payload = b"kernel-ish"
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.verify_bytes(
                payload,
                expected_sha256="0" * 64,
                expected_size=len(payload),
                context="test payload",
            )
        self.assertIn("sha256", str(caught.exception).lower())


class ArArchiveTests(unittest.TestCase):
    def test_the_named_member_is_returned(self) -> None:
        archive = _ar_archive(
            [("debian-binary", b"2.0\n"), ("data.tar.zst", b"payload-bytes")]
        )
        self.assertEqual(kernel.ar_member(archive, "data.tar.zst"), b"payload-bytes")

    def test_odd_length_members_are_padded_not_shifted(self) -> None:
        archive = _ar_archive(
            [("debian-binary", b"2.0\n"), ("odd.bin", b"12345"), ("data.tar.zst", b"after")]
        )
        self.assertEqual(kernel.ar_member(archive, "data.tar.zst"), b"after")

    def test_a_missing_member_is_refused(self) -> None:
        archive = _ar_archive([("debian-binary", b"2.0\n")])
        with self.assertRaises(kernel.KernelExtractError):
            kernel.ar_member(archive, "data.tar.zst")

    def test_something_that_is_not_an_ar_archive_is_refused(self) -> None:
        with self.assertRaises(kernel.KernelExtractError):
            kernel.ar_member(b"not an archive at all", "data.tar.zst")


class TarMemberTests(unittest.TestCase):
    def test_a_regular_member_is_returned(self) -> None:
        blob = _tar_with([("./boot/vmlinuz-6.8.0-31-generic", b"gz-bytes")])
        self.assertEqual(
            kernel.tar_member(blob, "./boot/vmlinuz-6.8.0-31-generic"), b"gz-bytes"
        )

    def test_a_symlink_standing_in_for_the_kernel_is_refused(self) -> None:
        blob = _tar_with([], symlink="./boot/vmlinuz-6.8.0-31-generic")
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.tar_member(blob, "./boot/vmlinuz-6.8.0-31-generic")
        self.assertIn("regular", str(caught.exception).lower())

    def test_a_missing_member_is_refused(self) -> None:
        blob = _tar_with([("./boot/other", b"x")])
        with self.assertRaises(kernel.KernelExtractError):
            kernel.tar_member(blob, "./boot/vmlinuz-6.8.0-31-generic")


class ResultShapeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.result = kernel.build_result(
            kernel_sha256="d" * 64,
            kernel_size=57860488,
            compressed_sha256="f" * 64,
            compressed_size=18199471,
            host_tools=[{"role": "zstd", "path": "/opt/homebrew/bin/zstd", "sha256": "a" * 64}],
            extraction_count=2,
        )

    def test_the_only_boundary_this_flips_is_the_kernel_one(self) -> None:
        boundaries = self.result["boundaries"]
        self.assertTrue(boundaries["kernelImageExtracted"])
        for name in (
            "bootAuthority",
            "guestImageBuilt",
            "initrdBuilt",
            "launcherDeployedIntoGuest",
            "rootDiskBuilt",
            "runtimeCompatibilityVerified",
        ):
            self.assertFalse(boundaries[name], name)

    def test_the_result_never_claims_bootable_or_activatable(self) -> None:
        self.assertFalse(self.result["bootableClaim"])
        self.assertFalse(self.result["activationAllowed"])

    def test_the_result_records_how_many_independent_extractions_ran(self) -> None:
        self.assertEqual(self.result["independentExtractionCount"], 2)

    def test_the_result_records_the_host_tool_it_actually_used(self) -> None:
        # The authority leaves the zstd path and digest null on purpose: they are
        # facts about the machine that ran the build, so this is where they land.
        tool = self.result["hostTools"][0]
        self.assertEqual(tool["role"], "zstd")
        self.assertEqual(tool["sha256"], "a" * 64)

    def test_the_result_binds_itself_to_the_authority_it_read(self) -> None:
        self.assertEqual(self.result["authoritySha256"], kernel.AUTHORITY_SHA256)

    def test_a_single_extraction_is_refused(self) -> None:
        with self.assertRaises(kernel.KernelExtractError):
            kernel.build_result(
                kernel_sha256="d" * 64,
                kernel_size=57860488,
                compressed_sha256="f" * 64,
                compressed_size=18199471,
                host_tools=[],
                extraction_count=1,
            )


class SealTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        # resolve(): the write-once primitive refuses a parent reached through a
        # symlink, and the macOS temp root is one.
        self.path = pathlib.Path(self.tmp.name).resolve() / "result.json"
        self.result = kernel.build_result(
            kernel_sha256="d" * 64,
            kernel_size=57860488,
            compressed_sha256="f" * 64,
            compressed_size=18199471,
            host_tools=[{"role": "zstd", "path": "/opt/homebrew/bin/zstd", "sha256": "a" * 64}],
            extraction_count=2,
        )

    def test_the_first_seal_writes_the_document(self) -> None:
        self.assertEqual(
            kernel.seal_or_reprove(self.result, result_path=self.path), "sealed"
        )
        self.assertTrue(self.path.exists())

    def test_an_identical_rerun_reproves_instead_of_rewriting(self) -> None:
        kernel.seal_or_reprove(self.result, result_path=self.path)
        before = self.path.read_bytes()
        self.assertEqual(
            kernel.seal_or_reprove(self.result, result_path=self.path), "re-proved"
        )
        self.assertEqual(self.path.read_bytes(), before)

    def test_a_divergent_rerun_reports_and_never_overwrites(self) -> None:
        kernel.seal_or_reprove(self.result, result_path=self.path)
        before = self.path.read_bytes()
        divergent = json.loads(json.dumps(self.result))
        divergent["kernel"]["sha256"] = "e" * 64
        with self.assertRaises(kernel.KernelExtractError) as caught:
            kernel.seal_or_reprove(divergent, result_path=self.path)
        self.assertIn("never overwrite", str(caught.exception).lower())
        self.assertEqual(self.path.read_bytes(), before)


class NoOverclaimTests(unittest.TestCase):
    def test_the_module_does_not_describe_extraction_as_booting(self) -> None:
        # `bootableClaim` is a key name that says false, so the check is for
        # phrases that would assert the opposite.
        text = pathlib.Path(kernel.__file__).read_text().lower()
        for phrase in ("boots successfully", "boot verified", "bootable image", "successfully booted"):
            self.assertNotIn(phrase, text, phrase)


class ForeignHostTests(unittest.TestCase):
    """The sealed record names a Mac's zstd; the producer is a Linux runner.

    The whole result document is compared when a run re-proves a seal, and the
    host tool row inside it is genuinely different on a different machine -- so
    a Linux run would be refused for a reason that has nothing to do with the
    kernel.  The artifact is what has to match, so a run on another host seals
    its own record and answers to the frozen one on the kernel block alone.
    Loosening that to "the digests are close enough" is not on offer: the kernel
    block is compared whole.
    """

    def sealed(self) -> dict:
        return json.loads(kernel.RESULT_PATH.read_text(encoding="utf-8"))

    def test_the_sealed_kernel_block_is_accepted(self) -> None:
        kernel.assert_kernel_matches_seal(self.sealed())

    def test_a_different_kernel_digest_is_refused(self) -> None:
        result = self.sealed()
        result["kernel"]["sha256"] = "0" * 64
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_kernel_matches_seal(result)

    def test_a_different_kernel_size_is_refused(self) -> None:
        result = self.sealed()
        result["kernel"]["sizeBytes"] += 1
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_kernel_matches_seal(result)

    def test_a_different_architecture_is_refused(self) -> None:
        result = self.sealed()
        result["kernel"]["architecture"] = "x86_64"
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_kernel_matches_seal(result)

    def test_a_result_with_no_kernel_block_is_refused(self) -> None:
        result = self.sealed()
        result.pop("kernel")
        with self.assertRaises(kernel.KernelExtractError):
            kernel.assert_kernel_matches_seal(result)

    def test_the_host_tool_row_is_allowed_to_differ(self) -> None:
        result = self.sealed()
        result["hostTools"] = [
            {"path": "/usr/bin/zstd", "role": "zstd", "sha256": "1" * 64}
        ]
        kernel.assert_kernel_matches_seal(result)

    def test_a_run_can_be_told_where_to_seal_its_own_record(self) -> None:
        parsed = kernel._parser().parse_args(
            ["extract", "--cas", "/cas", "--zstd", "/zstd", "--out", "/out",
             "--result", "/scratch/kernel-extract-result.json"]
        )
        self.assertEqual(str(parsed.result), "/scratch/kernel-extract-result.json")

    def test_leaving_it_off_still_answers_to_the_frozen_record(self) -> None:
        parsed = kernel._parser().parse_args(
            ["extract", "--cas", "/cas", "--zstd", "/zstd", "--out", "/out"]
        )
        self.assertEqual(parsed.result, kernel.RESULT_PATH)


if __name__ == "__main__":
    unittest.main()
