#!/usr/bin/env python3
"""Extract the frozen ARM64 guest kernel from the package the authority pinned.

The image builder authority names the package, the member inside it, and the
digest of both the compressed and the expanded kernel.  This turns those pins
into an actual file and seals what it produced.

The claim this makes is deliberately small.  Every step here -- unpacking an `ar`
archive, decompressing, reading a tar member, gunzipping -- is a function of its
input bytes with no freedom to differ, so running the pipeline twice is not the
evidence that two compilations of the same source are.  What a second run in a
separate directory does rule out is state leaking between runs: a reused
temporary path, a half-written file mistaken for a finished one, a member read
from the wrong archive.  That is worth having and it is all that is claimed.

An extracted kernel is not a boot.  Nothing here starts a virtual machine,
loads the image, or establishes that the image would run anywhere.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import Any, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload
from scripts.native_shadow_boot_rootfs_payload_acquire_arm64_v1 import canonical_json


REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native" / "containment"
AUTHORITY_PATH = CONTAINMENT / "native-shadow-boot-image-builder-authority-arm64-v1.json"
RESULT_PATH = CONTAINMENT / "native-shadow-boot-kernel-extract-result-arm64-v1.json"

RELEASE = "NATIVE-SHADOW-BOOT-KERNEL-EXTRACT-ARM64-V1"
RESULT_SCHEMA = "boole.native-shadow.boot-kernel-extract-result.arm64.v1"
RESULT_STATUS = "KERNEL-IMAGE-EXTRACTED-REPRODUCIBLY-NOT-BOOT-AUTHORITY"

# The authority this reads is pinned by digest rather than by path, so a rewrite
# of that document cannot silently change which kernel this extracts.
AUTHORITY_SHA256 = "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e"

DATA_MEMBER = "data.tar.zst"
ARM64_MAGIC = b"ARM\x64"
ARM64_MAGIC_OFFSET = 0x38
MINIMUM_EXTRACTIONS = 2


class KernelExtractError(RuntimeError):
    pass


def load_authority(path: pathlib.Path = AUTHORITY_PATH) -> dict[str, Any]:
    """Read the image builder authority, refusing bytes that are not the pinned ones."""
    raw = path.read_bytes()
    digest = hashlib.sha256(raw).hexdigest()
    if digest != AUTHORITY_SHA256:
        raise KernelExtractError(
            f"image builder authority digest is {digest}, expected {AUTHORITY_SHA256}"
        )
    return payload.json.loads(raw.decode("utf-8"))


def kernel_pins(authority: dict[str, Any]) -> dict[str, Any]:
    source = authority["kernelSource"]
    if source["compression"] != "gzip":
        raise KernelExtractError(
            f"authority pins an unsupported kernel compression: {source['compression']}"
        )
    return source


def verify_bytes(
    data: bytes, *, expected_sha256: str, expected_size: int, context: str
) -> None:
    if len(data) != expected_size:
        raise KernelExtractError(
            f"{context} size is {len(data)}, expected {expected_size}"
        )
    digest = hashlib.sha256(data).hexdigest()
    if digest != expected_sha256:
        raise KernelExtractError(
            f"{context} sha256 is {digest}, expected {expected_sha256}"
        )


def assert_arm64_image(data: bytes) -> None:
    """Require the arm64 Linux image magic at the offset the header defines.

    Checking the offset rather than searching is the point: a kernel that merely
    contains those four bytes somewhere is not an arm64 image.
    """
    end = ARM64_MAGIC_OFFSET + len(ARM64_MAGIC)
    if len(data) < end:
        raise KernelExtractError(
            f"kernel is {len(data)} bytes, too short to hold an arm64 header"
        )
    found = data[ARM64_MAGIC_OFFSET:end]
    if found != ARM64_MAGIC:
        raise KernelExtractError(
            f"kernel is not an arm64 image: magic at 0x{ARM64_MAGIC_OFFSET:x} is "
            f"{found!r}, expected {ARM64_MAGIC!r}"
        )


def ar_member(archive: bytes, name: str) -> bytes:
    """Return one member of an `ar` archive, which is how a .deb is laid out.

    Parsed here rather than shelled out to `/usr/bin/ar` so the step depends on
    nothing the host happens to provide -- the same reason the authority forbids
    PATH lookups for the tools it pins.
    """
    if not archive.startswith(b"!<arch>\n"):
        raise KernelExtractError("not an ar archive: the global header is missing")
    offset = 8
    while offset + 60 <= len(archive):
        header = archive[offset : offset + 60]
        if header[58:60] != b"`\n":
            raise KernelExtractError(f"ar header at offset {offset} is malformed")
        member = header[0:16].decode("ascii", "replace").strip().rstrip("/")
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError as error:
            raise KernelExtractError(
                f"ar header at offset {offset} has an unreadable size"
            ) from error
        start = offset + 60
        end = start + size
        if end > len(archive):
            raise KernelExtractError(f"ar member {member} runs past the end of the archive")
        if member == name:
            return archive[start:end]
        offset = end + (end % 2)
    raise KernelExtractError(f"ar archive has no member named {name}")


def tar_member(archive: bytes, name: str) -> bytes:
    """Return one regular file from a tar archive.

    A symlink standing in for the kernel would extract to whatever it points at,
    which is exactly the substitution the digest pins exist to catch -- so a
    non-regular member is refused before its bytes are ever read.
    """
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as handle:
        try:
            info = handle.getmember(name)
        except KeyError as error:
            raise KernelExtractError(f"tar archive has no member named {name}") from error
        if not info.isfile():
            raise KernelExtractError(
                f"tar member {name} is not a regular file; refusing to follow it"
            )
        extracted = handle.extractfile(info)
        if extracted is None:
            raise KernelExtractError(f"tar member {name} yielded no content")
        return extracted.read()


def decompress_zstd(data: bytes, zstd_path: pathlib.Path) -> bytes:
    """Decompress with the host zstd, whose path and digest the result records."""
    completed = subprocess.run(
        [str(zstd_path), "-d", "-c"],
        input=data,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise KernelExtractError(
            f"zstd failed with status {completed.returncode}: "
            f"{completed.stderr.decode('utf-8', 'replace').strip()}"
        )
    return completed.stdout


def cas_object(cas_roots: list[pathlib.Path], sha256: str) -> pathlib.Path:
    for root in cas_roots:
        candidate = root / "sha256" / sha256
        if candidate.is_file():
            return candidate
    raise KernelExtractError(f"content-addressed store has no object {sha256}")


def extract_once(
    *,
    cas_roots: list[pathlib.Path],
    zstd_path: pathlib.Path,
    workdir: pathlib.Path,
    pins: dict[str, Any],
) -> pathlib.Path:
    """Run the whole pipeline in one directory and return the expanded kernel."""
    package_path = cas_object(cas_roots, pins["packageSha256"])
    package = package_path.read_bytes()
    verify_bytes(
        package,
        expected_sha256=pins["packageSha256"],
        expected_size=package_path.stat().st_size,
        context="kernel package",
    )

    data_tar = decompress_zstd(ar_member(package, DATA_MEMBER), zstd_path)
    compressed = tar_member(data_tar, pins["memberPath"])
    verify_bytes(
        compressed,
        expected_sha256=pins["compressedSha256"],
        expected_size=pins["compressedSizeBytes"],
        context="compressed kernel",
    )

    image = gzip.decompress(compressed)
    verify_bytes(
        image,
        expected_sha256=pins["imageSha256"],
        expected_size=pins["imageSizeBytes"],
        context="expanded kernel",
    )
    assert_arm64_image(image)

    out = workdir / "guest-kernel"
    out.write_bytes(image)
    return out


def build_result(
    *,
    kernel_sha256: str,
    kernel_size: int,
    compressed_sha256: str,
    compressed_size: int,
    host_tools: list[dict[str, Any]],
    extraction_count: int,
) -> dict[str, Any]:
    if extraction_count < MINIMUM_EXTRACTIONS:
        raise KernelExtractError(
            f"a result needs at least {MINIMUM_EXTRACTIONS} independent extractions, "
            f"got {extraction_count}"
        )
    return {
        "activationAllowed": False,
        "authoritySha256": AUTHORITY_SHA256,
        "bootableClaim": False,
        "boundaries": {
            "bootAuthority": False,
            "guestImageBuilt": False,
            "initrdBuilt": False,
            "kernelImageExtracted": True,
            "launcherDeployedIntoGuest": False,
            "rootDiskBuilt": False,
            "runtimeCompatibilityVerified": False,
        },
        "compressedKernel": {
            "sha256": compressed_sha256,
            "sizeBytes": compressed_size,
        },
        "hostTools": sorted(host_tools, key=lambda row: row["role"]),
        "independentExtractionCount": extraction_count,
        "kernel": {
            "architecture": "aarch64",
            "magicOffset": ARM64_MAGIC_OFFSET,
            "name": "guest-kernel",
            "sha256": kernel_sha256,
            "sizeBytes": kernel_size,
        },
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "status": RESULT_STATUS,
    }


def assert_kernel_matches_seal(
    result: dict[str, Any], *, sealed_path: pathlib.Path = RESULT_PATH
) -> None:
    """Require a run on another host to reproduce the sealed kernel block.

    The sealed record was written on a Mac and names that Mac's zstd by path and
    digest.  A Linux runner has a different zstd, truthfully, so comparing the
    whole document would refuse a correct extraction for a reason that has
    nothing to do with the kernel.  What has to match is the artifact, so the
    kernel block is what is compared -- and it is compared whole, key for key.
    A weaker comparison would let a differently sized or differently named image
    through on the strength of a digest that was never the only pin.
    """

    try:
        sealed = payload.json.loads(sealed_path.read_text(encoding="utf-8"))
    except OSError as error:
        raise KernelExtractError(
            f"the sealed kernel extract result is unreadable: {sealed_path}"
        ) from error
    frozen = sealed.get("kernel")
    if not isinstance(frozen, dict):
        raise KernelExtractError("the sealed kernel extract result seals no kernel")
    found = result.get("kernel")
    if not isinstance(found, dict):
        raise KernelExtractError("this extraction produced no kernel block to compare")
    if found != frozen:
        raise KernelExtractError(
            "this extraction disagrees with the sealed kernel: "
            f"{canonical_json(found)!r} against {canonical_json(frozen)!r} -- "
            "report the difference, never overwrite the seal"
        )


def seal_or_reprove(
    result: dict[str, Any], *, result_path: pathlib.Path = RESULT_PATH
) -> str:
    raw = canonical_json(result)
    if result_path.exists():
        if result_path.read_bytes() != raw:
            raise KernelExtractError(
                "this extraction disagrees with the sealed kernel extract result; "
                "report the difference, never overwrite the seal"
            )
        return "re-proved"
    payload._write_result_once(result_path, raw)
    return "sealed"


def _host_tool_row(role: str, path: pathlib.Path) -> dict[str, Any]:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return {"path": str(path), "role": role, "sha256": digest}


def extract(
    *,
    cas_roots: list[pathlib.Path],
    zstd_path: pathlib.Path,
    out_dir: pathlib.Path,
    result_path: pathlib.Path = RESULT_PATH,
) -> tuple[dict[str, Any], str]:
    pins = kernel_pins(load_authority())
    produced: list[pathlib.Path] = []
    digests: list[str] = []
    holds: list[tempfile.TemporaryDirectory] = []
    try:
        for index in range(MINIMUM_EXTRACTIONS):
            hold = tempfile.TemporaryDirectory(prefix=f"boole-kernel-{index}-")
            holds.append(hold)
            produced.append(
                extract_once(
                    cas_roots=cas_roots,
                    zstd_path=zstd_path,
                    workdir=pathlib.Path(hold.name),
                    pins=pins,
                )
            )
            digests.append(hashlib.sha256(produced[-1].read_bytes()).hexdigest())

        if len(set(digests)) != 1:
            raise KernelExtractError(
                "independent extractions produced different kernels: "
                + ", ".join(digests)
                + " -- report the difference, do not pick one"
            )

        out_dir.mkdir(parents=True, exist_ok=True)
        final = out_dir / "guest-kernel"
        shutil.copyfile(produced[0], final)

        result = build_result(
            kernel_sha256=digests[0],
            kernel_size=final.stat().st_size,
            compressed_sha256=pins["compressedSha256"],
            compressed_size=pins["compressedSizeBytes"],
            host_tools=[_host_tool_row("zstd", zstd_path)],
            extraction_count=len(produced),
        )
        if result_path != RESULT_PATH:
            # A run told to seal its own record is a run on another host, whose
            # host tool rows differ truthfully.  It still has to reproduce the
            # frozen kernel, so that is checked before its own record is written.
            assert_kernel_matches_seal(result)
            seal_or_reprove(result, result_path=result_path)
            return result, "matched-the-seal"
        return result, seal_or_reprove(result, result_path=result_path)
    finally:
        for hold in holds:
            hold.cleanup()


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("check", help="report the pinned kernel without extracting it")
    run = sub.add_parser("extract", help="extract the kernel twice and seal the result")
    run.add_argument("--cas", type=pathlib.Path, action="append", required=True)
    run.add_argument("--zstd", type=pathlib.Path, required=True)
    run.add_argument("--out", type=pathlib.Path, required=True)
    run.add_argument("--result", type=pathlib.Path, default=RESULT_PATH)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    if args.command == "check":
        pins = kernel_pins(load_authority())
        print(
            f"kernel pin: {pins['package']} {pins['version']} "
            f"member={pins['memberPath']} image={pins['imageSha256']} "
            f"size={pins['imageSizeBytes']} extracted=no"
        )
        return 0

    result, disposition = extract(
        cas_roots=list(args.cas),
        zstd_path=args.zstd,
        out_dir=args.out,
        result_path=args.result,
    )
    print(
        f"kernel extract: {result['status']} "
        f"extractions={result['independentExtractionCount']} identical=yes "
        f"{disposition} sha256={result['kernel']['sha256']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
