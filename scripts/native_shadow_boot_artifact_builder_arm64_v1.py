#!/usr/bin/env python3
"""Fail-closed preflight for deterministic Linux/arm64 boot artifacts.

This slice deliberately produces no kernel, initrd or root disk.  It proves
that the frozen rootfs closure and the separately pinned boot inputs are
complete before a later builder is allowed to create an output directory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import stat
import struct
import sys
from typing import Any, Optional


PLAN_SCHEMA = "boole.native-shadow.boot-artifact-build-plan.arm64.v1"
LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-artifact-preflight-result.arm64.v1"
ARM64_IMAGE_MAGIC = b"ARM\x64"
ELF_MACHINE_AARCH64 = 183
ET_EXEC = 2
ET_DYN = 3
PT_LOAD = 1
PT_DYNAMIC = 2
PT_INTERP = 3
PF_X = 1
DT_NULL = 0
DT_NEEDED = 1


class BootArtifactPreflightError(RuntimeError):
    """The preflight authority or one of its present inputs is invalid."""


def canonical_json(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        indent=2,
        allow_nan=False,
    ).encode("utf-8") + b"\n"


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BootArtifactPreflightError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _normalized_absolute(path: pathlib.Path) -> pathlib.Path:
    absolute = pathlib.Path(os.path.abspath(os.fspath(path)))
    if sys.platform == "darwin" and len(absolute.parts) > 1 and absolute.parts[1] in {
        "etc",
        "tmp",
        "var",
    }:
        absolute = pathlib.Path("/private").joinpath(*absolute.parts[1:])
    return absolute


def _open_directory_nofollow(path: pathlib.Path, context: str) -> int:
    flags = (
        os.O_RDONLY
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    absolute = _normalized_absolute(path)
    try:
        descriptor = os.open(os.path.sep, flags)
    except OSError as exc:
        raise BootArtifactPreflightError(f"cannot open filesystem root for {context}") from exc
    try:
        for component in absolute.parts[1:]:
            if component in {"", ".", "..", os.path.sep}:
                raise BootArtifactPreflightError(f"{context} path is not normalized")
            try:
                child = os.open(component, flags, dir_fd=descriptor)
            except OSError as exc:
                raise BootArtifactPreflightError(
                    f"{context} directory is absent, unsafe, or contains a symlink: {path}"
                ) from exc
            os.close(descriptor)
            descriptor = child
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            raise BootArtifactPreflightError(f"{context} is not a directory")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _read_regular_at(
    directory_descriptor: int,
    name: str,
    context: str,
    *,
    executable: bool = False,
    missing_ok: bool = False,
) -> Optional[bytes]:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(name, flags, dir_fd=directory_descriptor)
    except FileNotFoundError:
        if missing_ok:
            return None
        raise BootArtifactPreflightError(f"{context} is absent")
    except OSError as exc:
        raise BootArtifactPreflightError(f"{context} is unsafe or a symlink") from exc
    try:
        metadata_before = os.fstat(descriptor)
        if not stat.S_ISREG(metadata_before.st_mode):
            raise BootArtifactPreflightError(f"{context} is not a regular file")
        if executable and not (metadata_before.st_mode & 0o111):
            raise BootArtifactPreflightError(f"{context} is not executable")
        chunks: list[bytes] = []
        remaining = metadata_before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise BootArtifactPreflightError(f"{context} was truncated")
            chunks.append(chunk)
            remaining -= len(chunk)
        metadata_after = os.fstat(descriptor)
        identity_before = (
            metadata_before.st_dev,
            metadata_before.st_ino,
            metadata_before.st_size,
            metadata_before.st_mtime_ns,
            metadata_before.st_ctime_ns,
        )
        identity_after = (
            metadata_after.st_dev,
            metadata_after.st_ino,
            metadata_after.st_size,
            metadata_after.st_mtime_ns,
            metadata_after.st_ctime_ns,
        )
        if identity_before != identity_after:
            raise BootArtifactPreflightError(f"{context} changed while reading")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _read_regular_nofollow(
    path: pathlib.Path, context: str, *, executable: bool = False
) -> bytes:
    absolute = _normalized_absolute(path)
    if absolute.name in {"", ".", ".."}:
        raise BootArtifactPreflightError(f"{context} path is invalid")
    parent_descriptor = _open_directory_nofollow(absolute.parent, f"{context} parent")
    try:
        raw = _read_regular_at(
            parent_descriptor,
            absolute.name,
            context,
            executable=executable,
        )
        assert raw is not None
        return raw
    finally:
        os.close(parent_descriptor)


def _load_canonical(path: pathlib.Path, context: str) -> tuple[dict[str, Any], bytes]:
    raw = _read_regular_nofollow(path, context)
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise BootArtifactPreflightError(f"{context} is not JSON") from exc
    if not isinstance(value, dict) or canonical_json(value) != raw:
        raise BootArtifactPreflightError(f"{context} is not canonical exact JSON")
    return value, raw


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise BootArtifactPreflightError(f"{context} keys differ")
    return value


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise BootArtifactPreflightError(f"{context} must be a lowercase SHA-256")
    return value


def _size(value: Any, context: str, *, allow_null: bool = False) -> Optional[int]:
    if allow_null and value is None:
        return None
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise BootArtifactPreflightError(f"{context} must be a positive integer")
    return value


def _pin(value: Any, name: str, expected_format: str) -> dict[str, Any]:
    pin = _exact(value, {"format", "sha256", "sizeBytes"}, f"inputs.{name}")
    if pin["format"] != expected_format:
        raise BootArtifactPreflightError(f"inputs.{name}.format differs")
    digest = pin["sha256"]
    size = _size(pin["sizeBytes"], f"inputs.{name}.sizeBytes", allow_null=True)
    if digest is None:
        if size is not None:
            raise BootArtifactPreflightError(f"inputs.{name} is only partly pinned")
    else:
        _digest(digest, f"inputs.{name}.sha256")
        if size is None:
            raise BootArtifactPreflightError(f"inputs.{name} is only partly pinned")
    return pin


def _validate_plan(value: Any) -> dict[str, Any]:
    plan = _exact(
        value,
        {
            "activationAllowed",
            "bootFormatVersion",
            "guestDownloadMaxBytes",
            "inputs",
            "release",
            "rootfsSourceLock",
            "schema",
        },
        "boot artifact plan",
    )
    if plan["schema"] != PLAN_SCHEMA:
        raise BootArtifactPreflightError("boot artifact plan schema differs")
    if plan["activationAllowed"] is not False:
        raise BootArtifactPreflightError("boot artifact plan must not allow activation")
    if plan["bootFormatVersion"] != 1:
        raise BootArtifactPreflightError("bootFormatVersion differs")
    if plan["guestDownloadMaxBytes"] != 2_147_483_648:
        raise BootArtifactPreflightError("guest download cap differs")
    if not isinstance(plan["release"], str) or not plan["release"]:
        raise BootArtifactPreflightError("boot artifact plan release is empty")
    rootfs = _exact(
        plan["rootfsSourceLock"],
        {"artifactCount", "sha256", "totalBytes"},
        "rootfsSourceLock",
    )
    _digest(rootfs["sha256"], "rootfsSourceLock.sha256")
    _size(rootfs["artifactCount"], "rootfsSourceLock.artifactCount")
    _size(rootfs["totalBytes"], "rootfsSourceLock.totalBytes")
    inputs = _exact(plan["inputs"], {"ext4Tool", "kernel", "pid1"}, "inputs")
    _pin(inputs["ext4Tool"], "ext4Tool", "pinned-host-executable")
    _pin(inputs["kernel"], "kernel", "linux-arm64-image")
    _pin(inputs["pid1"], "pid1", "elf64-aarch64-static")
    return plan


def load_plan(path: pathlib.Path) -> dict[str, Any]:
    value, _ = _load_canonical(path, "boot artifact plan")
    return _validate_plan(value)


def _validate_source_lock(
    value: Any, raw: bytes, plan: dict[str, Any]
) -> list[dict[str, Any]]:
    if value.get("schema") != LOCK_SCHEMA:
        raise BootArtifactPreflightError("rootfs source lock schema differs")
    if value.get("activationAllowed") is not False:
        raise BootArtifactPreflightError("rootfs source lock must not allow activation")
    if hashlib.sha256(raw).hexdigest() != plan["rootfsSourceLock"]["sha256"]:
        raise BootArtifactPreflightError("rootfs source lock digest differs")
    platform = value.get("platform")
    if not isinstance(platform, dict) or platform != {
        "debArchitecture": "arm64",
        "ociArchitecture": "arm64",
        "os": "linux",
        "rustTarget": "aarch64-unknown-linux-gnu",
    }:
        raise BootArtifactPreflightError("rootfs source lock platform differs")
    rows = value.get("artifacts")
    if not isinstance(rows, list):
        raise BootArtifactPreflightError("rootfs source lock artifacts are not a list")
    artifacts: list[dict[str, Any]] = []
    seen: set[str] = set()
    identifiers: list[str] = []
    for index, raw_row in enumerate(rows):
        row = _exact(raw_row, {"id", "kind", "sha256", "sizeBytes"}, f"artifacts[{index}]")
        identifier = row["id"]
        if not isinstance(identifier, str) or not identifier or identifier in seen:
            raise BootArtifactPreflightError("rootfs artifact IDs are invalid or duplicated")
        if not isinstance(row["kind"], str) or not row["kind"]:
            raise BootArtifactPreflightError(f"rootfs artifact kind is invalid: {identifier}")
        _digest(row["sha256"], f"rootfs artifact digest: {identifier}")
        _size(row["sizeBytes"], f"rootfs artifact size: {identifier}")
        seen.add(identifier)
        identifiers.append(identifier)
        artifacts.append(row)
    if identifiers != sorted(identifiers):
        raise BootArtifactPreflightError("rootfs artifacts are not sorted by ID")
    if len(artifacts) != plan["rootfsSourceLock"]["artifactCount"]:
        raise BootArtifactPreflightError("rootfs artifact count differs")
    if sum(row["sizeBytes"] for row in artifacts) != plan["rootfsSourceLock"]["totalBytes"]:
        raise BootArtifactPreflightError("rootfs artifact byte total differs")
    return artifacts


def _cas_sha_directory(cas: pathlib.Path) -> int:
    return _open_directory_nofollow(cas / "sha256", "CAS sha256")


def _artifact_state(
    artifact: dict[str, Any], sha_directories: list[int]
) -> tuple[str, int]:
    digest = artifact["sha256"]
    matches: list[bytes] = []
    for directory in sha_directories:
        raw = _read_regular_at(
            directory,
            digest,
            f"CAS artifact {artifact['id']}",
            missing_ok=True,
        )
        if raw is None:
            continue
        if len(raw) != artifact["sizeBytes"] or hashlib.sha256(raw).hexdigest() != digest:
            raise BootArtifactPreflightError(
                f"CAS artifact digest/size differs: {artifact['id']}"
            )
        matches.append(raw)
    if not matches:
        return "missing", 0
    if any(raw != matches[0] for raw in matches[1:]):
        raise BootArtifactPreflightError(f"CAS roots disagree: {artifact['id']}")
    return "present", len(matches[0])


def _pinned_bytes(
    pin: dict[str, Any], path: Optional[pathlib.Path], name: str, *, executable: bool
) -> Optional[bytes]:
    if pin["sha256"] is None:
        return None
    if path is None:
        raise BootArtifactPreflightError(f"pinned input path is absent: {name}")
    raw = _read_regular_nofollow(
        path, f"pinned input {name}", executable=executable
    )
    if len(raw) != pin["sizeBytes"] or hashlib.sha256(raw).hexdigest() != pin["sha256"]:
        raise BootArtifactPreflightError(f"pinned input digest/size differs: {name}")
    return raw


def _validate_arm64_image(raw: bytes) -> None:
    if len(raw) < 0x40 or raw[0x38:0x3C] != ARM64_IMAGE_MAGIC:
        raise BootArtifactPreflightError("kernel is not an uncompressed Linux ARM64 Image")
    image_size = struct.unpack_from("<Q", raw, 0x10)[0]
    # Linux records the effective in-memory Image size here.  It can include
    # zero-filled sections that are not present in the file, so it need not be
    # bounded by len(raw).  Zero is still not a pinned, self-describing Image.
    if image_size == 0:
        raise BootArtifactPreflightError("kernel ARM64 image size is invalid")


def _validate_static_aarch64_elf(raw: bytes) -> None:
    if len(raw) < 64 or raw[:7] != b"\x7fELF\x02\x01\x01":
        raise BootArtifactPreflightError("PID 1 is not ELF64 little-endian")
    if struct.unpack_from("<H", raw, 18)[0] != ELF_MACHINE_AARCH64:
        raise BootArtifactPreflightError("PID 1 is not ELF64 AArch64")
    if struct.unpack_from("<H", raw, 16)[0] not in {ET_EXEC, ET_DYN}:
        raise BootArtifactPreflightError("PID 1 ELF type is not executable")
    if struct.unpack_from("<I", raw, 20)[0] != 1:
        raise BootArtifactPreflightError("PID 1 ELF version differs")
    entrypoint = struct.unpack_from("<Q", raw, 24)[0]
    if entrypoint == 0:
        raise BootArtifactPreflightError("PID 1 entrypoint is absent")
    program_offset = struct.unpack_from("<Q", raw, 32)[0]
    entry_size = struct.unpack_from("<H", raw, 54)[0]
    entry_count = struct.unpack_from("<H", raw, 56)[0]
    if entry_size < 56 or program_offset + (entry_size * entry_count) > len(raw):
        raise BootArtifactPreflightError("PID 1 ELF program headers are invalid")
    entrypoint_is_executable = False
    for index in range(entry_count):
        offset = program_offset + (index * entry_size)
        program_type = struct.unpack_from("<I", raw, offset)[0]
        if program_type == PT_INTERP:
            raise BootArtifactPreflightError("PID 1 has PT_INTERP and is not static")
        if program_type == PT_DYNAMIC:
            file_offset = struct.unpack_from("<Q", raw, offset + 8)[0]
            file_size = struct.unpack_from("<Q", raw, offset + 32)[0]
            memory_size = struct.unpack_from("<Q", raw, offset + 40)[0]
            if (
                file_size == 0
                or file_size > memory_size
                or file_size % 16 != 0
                or file_offset + file_size > len(raw)
            ):
                raise BootArtifactPreflightError(
                    "PID 1 PT_DYNAMIC bounds are invalid"
                )
            saw_terminator = False
            for dynamic_offset in range(
                file_offset, file_offset + file_size, 16
            ):
                tag = struct.unpack_from("<q", raw, dynamic_offset)[0]
                if tag == DT_NULL:
                    saw_terminator = True
                    break
                if tag == DT_NEEDED:
                    raise BootArtifactPreflightError(
                        "PID 1 has DT_NEEDED and is not static"
                    )
            if not saw_terminator:
                raise BootArtifactPreflightError(
                    "PID 1 PT_DYNAMIC has no DT_NULL terminator"
                )
            continue
        if program_type != PT_LOAD:
            continue
        flags = struct.unpack_from("<I", raw, offset + 4)[0]
        file_offset = struct.unpack_from("<Q", raw, offset + 8)[0]
        virtual_address = struct.unpack_from("<Q", raw, offset + 16)[0]
        file_size = struct.unpack_from("<Q", raw, offset + 32)[0]
        memory_size = struct.unpack_from("<Q", raw, offset + 40)[0]
        if file_size > memory_size or file_offset + file_size > len(raw):
            raise BootArtifactPreflightError("PID 1 PT_LOAD bounds are invalid")
        if (
            flags & PF_X
            and memory_size > 0
            and virtual_address <= entrypoint < virtual_address + memory_size
        ):
            entrypoint_is_executable = True
    if not entrypoint_is_executable:
        raise BootArtifactPreflightError(
            "PID 1 entrypoint is outside an executable PT_LOAD segment"
        )


def audit_inputs(
    plan_path: pathlib.Path,
    source_lock_path: pathlib.Path,
    cas_roots: list[pathlib.Path],
    *,
    kernel_path: Optional[pathlib.Path] = None,
    pid1_path: Optional[pathlib.Path] = None,
    ext4_tool_path: Optional[pathlib.Path] = None,
) -> dict[str, Any]:
    plan, plan_raw = _load_canonical(plan_path, "boot artifact plan")
    plan = _validate_plan(plan)
    lock, lock_raw = _load_canonical(source_lock_path, "rootfs source lock")
    artifacts = _validate_source_lock(lock, lock_raw, plan)
    sha_directories: list[int] = []
    try:
        for path in cas_roots:
            sha_directories.append(_cas_sha_directory(path))
        present_count = 0
        present_bytes = 0
        missing: list[dict[str, Any]] = []
        for artifact in artifacts:
            state, size = _artifact_state(artifact, sha_directories)
            if state == "present":
                present_count += 1
                present_bytes += size
            else:
                missing.append(artifact)
    finally:
        for descriptor in sha_directories:
            os.close(descriptor)

    missing_pins: list[str] = []
    paths = {
        "ext4Tool": ext4_tool_path,
        "kernel": kernel_path,
        "pid1": pid1_path,
    }
    for name in sorted(paths):
        pin = plan["inputs"][name]
        if pin["sha256"] is None:
            missing_pins.append(name)
            continue
        raw = _pinned_bytes(
            pin,
            paths[name],
            name,
            executable=name in {"ext4Tool", "pid1"},
        )
        assert raw is not None
        if name == "kernel":
            _validate_arm64_image(raw)
        elif name == "pid1":
            _validate_static_aarch64_elf(raw)

    missing_bytes = sum(row["sizeBytes"] for row in missing)
    blocked = bool(missing or missing_pins)
    return {
        "activationAllowed": False,
        "artifactsWritten": 0,
        "bootableClaim": False,
        "inputSummary": {
            "expectedArtifacts": len(artifacts),
            "expectedBytes": sum(row["sizeBytes"] for row in artifacts),
            "invalidArtifacts": 0,
            "missingArtifacts": len(missing),
            "missingBytes": missing_bytes,
            "presentArtifacts": present_count,
            "presentBytes": present_bytes,
        },
        "missingArtifactIds": sorted(row["id"] for row in missing),
        "missingPinnedInputs": missing_pins,
        "planSha256": hashlib.sha256(plan_raw).hexdigest(),
        "rootfsSourceLockSha256": hashlib.sha256(lock_raw).hexdigest(),
        "schema": RESULT_SCHEMA,
        "status": "BLOCKED_MISSING_INPUTS" if blocked else "PREFLIGHT_READY",
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    audit = subparsers.add_parser("audit")
    audit.add_argument("--plan", type=pathlib.Path, required=True)
    audit.add_argument("--source-lock", type=pathlib.Path, required=True)
    audit.add_argument("--cas", type=pathlib.Path, action="append", required=True)
    audit.add_argument("--kernel", type=pathlib.Path)
    audit.add_argument("--pid1", type=pathlib.Path)
    audit.add_argument("--ext4-tool", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = audit_inputs(
            args.plan,
            args.source_lock,
            args.cas,
            kernel_path=args.kernel,
            pid1_path=args.pid1,
            ext4_tool_path=args.ext4_tool,
        )
    except BootArtifactPreflightError as exc:
        print(f"native-shadow boot artifact preflight: {exc}", file=sys.stderr)
        return 2
    sys.stdout.buffer.write(canonical_json(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
