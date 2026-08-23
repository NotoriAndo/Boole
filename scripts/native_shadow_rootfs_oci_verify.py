#!/usr/bin/env python3
"""Independently verify a native-shadow OCI rootfs layout.

This verifier intentionally does not import the rootfs builder.  It treats the
layout as hostile input, opens every path relative to already-open directory
descriptors with ``O_NOFOLLOW``, validates the OCI descriptor graph, parses the
single uncompressed layer without extracting it, and checks the two sidecar
documents against the bytes that were actually observed.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import pathlib
import stat
import sys
import tarfile
from dataclasses import dataclass
from typing import Any, Optional


OCI_MANIFEST_MEDIA_TYPE = "application/vnd.oci.image.manifest.v1+json"
OCI_CONFIG_MEDIA_TYPE = "application/vnd.oci.image.config.v1+json"
OCI_LAYER_MEDIA_TYPE = "application/vnd.oci.image.layer.v1.tar"

TOP_LEVEL_ENTRIES = frozenset(
    {
        "BUILD-RECEIPT.json",
        "ROOTFS-CONTENT-MANIFEST.json",
        "blobs",
        "index.json",
        "oci-layout",
    }
)
FIXED_CLOSURES: tuple[tuple[str, tuple[str, ...]], ...] = (
    (
        "installed-rust-toolchain-file-manifest",
        ("/opt/boole/native-checker-toolchain",),
    ),
    (
        "python-interpreter-and-stdlib-file-manifest",
        ("/usr/bin/python3.12", "/usr/lib/python3.12"),
    ),
    (
        "system-linker-and-runtime-file-manifest",
        ("/lib", "/usr/bin", "/usr/lib"),
    ),
)
FORBIDDEN_ROOTFS_PATHS = frozenset({"etc/ld.so.preload"})

# These are the fixed ceilings of the v1 rootfs recipe.  Layer overhead is
# bounded separately because tar headers are not part of maxTotalBytes.
MAX_ENTRIES = 200_000
MAX_FILE_BYTES = 512 * 1024 * 1024
MAX_TOTAL_FILE_BYTES = 2 * 1024 * 1024 * 1024
MAX_LAYER_BYTES = MAX_TOTAL_FILE_BYTES + (MAX_ENTRIES * 1536) + (20 * 1024)
MAX_CONTENT_MANIFEST_BYTES = 256 * 1024 * 1024
MAX_JSON_BLOB_BYTES = 16 * 1024 * 1024
MAX_SMALL_SIDECAR_BYTES = 4 * 1024 * 1024
MAX_TAR_HEADERS = (MAX_ENTRIES * 2) + 32
MAX_PAX_HEADER_BYTES = 1024 * 1024
MAX_JSON_NESTING = 128

_DIRECTORY_FLAGS = (
    os.O_RDONLY
    | getattr(os, "O_DIRECTORY", 0)
    | getattr(os, "O_CLOEXEC", 0)
    | getattr(os, "O_NOFOLLOW", 0)
)
_FILE_FLAGS = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)


class OciVerificationError(ValueError):
    """The supplied OCI layout violates the native-shadow contract."""


def _snapshot(metadata: os.stat_result) -> tuple[int, ...]:
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_size,
        metadata.st_nlink,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def _strict_name(value: str, context: str) -> str:
    if not isinstance(value, str) or not value or value in (".", ".."):
        raise OciVerificationError(f"invalid {context}")
    if "/" in value or "\\" in value or "\x00" in value:
        raise OciVerificationError(f"unsafe {context}")
    try:
        value.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise OciVerificationError(f"non-UTF-8 {context}") from exc
    return value


def _directory_names(descriptor: int, context: str) -> frozenset[str]:
    try:
        names = os.listdir(descriptor)
    except OSError as exc:
        raise OciVerificationError(f"cannot list {context}") from exc
    checked = [_strict_name(name, f"{context} entry") for name in names]
    if len(checked) != len(set(checked)):
        raise OciVerificationError(f"duplicate {context} entry")
    return frozenset(checked)


def _stat_at(parent: int, name: str, context: str) -> os.stat_result:
    try:
        return os.stat(name, dir_fd=parent, follow_symlinks=False)
    except OSError as exc:
        raise OciVerificationError(f"cannot stat {context}") from exc


@dataclass
class _OpenedEntry:
    parent: int
    name: str
    descriptor: int
    metadata: os.stat_result
    context: str
    is_directory: bool

    def assert_unchanged(self) -> None:
        try:
            opened = os.fstat(self.descriptor)
            named = os.stat(self.name, dir_fd=self.parent, follow_symlinks=False)
        except OSError as exc:
            raise OciVerificationError(f"{self.context} changed while verifying") from exc
        expected = _snapshot(self.metadata)
        if _snapshot(opened) != expected or _snapshot(named) != expected:
            raise OciVerificationError(f"{self.context} changed while verifying")

    def close(self) -> None:
        try:
            os.close(self.descriptor)
        except OSError:
            pass


def _open_directory_at(parent: int, name: str, context: str) -> _OpenedEntry:
    name = _strict_name(name, context)
    before = _stat_at(parent, name, context)
    if not stat.S_ISDIR(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o755:
        raise OciVerificationError(f"{context} must be a mode-0755 real directory")
    try:
        descriptor = os.open(name, _DIRECTORY_FLAGS, dir_fd=parent)
    except OSError as exc:
        raise OciVerificationError(f"cannot open {context} without following links") from exc
    try:
        opened = os.fstat(descriptor)
    except OSError as exc:
        os.close(descriptor)
        raise OciVerificationError(f"cannot inspect opened {context}") from exc
    if _snapshot(opened) != _snapshot(before):
        os.close(descriptor)
        raise OciVerificationError(f"{context} changed while opening")
    return _OpenedEntry(parent, name, descriptor, opened, context, True)


def _open_file_at(parent: int, name: str, context: str) -> _OpenedEntry:
    name = _strict_name(name, context)
    before = _stat_at(parent, name, context)
    if (
        not stat.S_ISREG(before.st_mode)
        or stat.S_IMODE(before.st_mode) != 0o444
        or before.st_nlink != 1
    ):
        raise OciVerificationError(
            f"{context} must be a mode-0444, single-link regular file"
        )
    try:
        descriptor = os.open(name, _FILE_FLAGS, dir_fd=parent)
    except OSError as exc:
        raise OciVerificationError(f"cannot open {context} without following links") from exc
    try:
        opened = os.fstat(descriptor)
    except OSError as exc:
        os.close(descriptor)
        raise OciVerificationError(f"cannot inspect opened {context}") from exc
    if _snapshot(opened) != _snapshot(before):
        os.close(descriptor)
        raise OciVerificationError(f"{context} changed while opening")
    return _OpenedEntry(parent, name, descriptor, opened, context, False)


def _read_opened(entry: _OpenedEntry, limit: int) -> bytes:
    size = entry.metadata.st_size
    if size < 0 or size > limit:
        raise OciVerificationError(f"{entry.context} exceeds its byte limit")
    chunks: list[bytes] = []
    offset = 0
    try:
        while offset < size:
            chunk = os.pread(entry.descriptor, min(1024 * 1024, size - offset), offset)
            if not chunk:
                raise OciVerificationError(f"{entry.context} was truncated while reading")
            chunks.append(chunk)
            offset += len(chunk)
    except OciVerificationError:
        raise
    except OSError as exc:
        raise OciVerificationError(f"cannot read {entry.context}") from exc
    entry.assert_unchanged()
    return b"".join(chunks)


def _hash_opened(entry: _OpenedEntry, limit: int) -> str:
    size = entry.metadata.st_size
    if size < 0 or size > limit:
        raise OciVerificationError(f"{entry.context} exceeds its byte limit")
    digest = hashlib.sha256()
    offset = 0
    try:
        while offset < size:
            chunk = os.pread(entry.descriptor, min(1024 * 1024, size - offset), offset)
            if not chunk:
                raise OciVerificationError(f"{entry.context} was truncated while hashing")
            digest.update(chunk)
            offset += len(chunk)
    except OciVerificationError:
        raise
    except OSError as exc:
        raise OciVerificationError(f"cannot hash {entry.context}") from exc
    entry.assert_unchanged()
    return digest.hexdigest()


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise OciVerificationError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_json_constant(value: str) -> None:
    raise OciVerificationError(f"non-finite JSON number is forbidden: {value}")


def _validate_json_nesting(value: Any) -> None:
    stack: list[tuple[Any, int, bool]] = [(value, 0, False)]
    active: set[int] = set()
    while stack:
        current, depth, leaving = stack.pop()
        if leaving:
            active.remove(id(current))
            continue
        if depth > MAX_JSON_NESTING:
            raise OciVerificationError("JSON nesting exceeds the verifier limit")
        if isinstance(current, dict):
            children = tuple(current.values())
        elif isinstance(current, (list, tuple)):
            children = tuple(current)
        else:
            continue
        identity = id(current)
        if identity in active:
            raise OciVerificationError("cyclic JSON value is forbidden")
        active.add(identity)
        stack.append((current, depth, True))
        stack.extend((child, depth + 1, False) for child in reversed(children))


def _canonical_json(value: Any, *, compact: bool) -> bytes:
    _validate_json_nesting(value)
    try:
        if compact:
            encoded = json.dumps(
                value,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
                allow_nan=False,
            )
        else:
            encoded = json.dumps(
                value,
                ensure_ascii=False,
                sort_keys=True,
                indent=2,
                allow_nan=False,
            )
        return (encoded + "\n").encode("utf-8")
    except RecursionError as exc:
        raise OciVerificationError("JSON nesting exceeds the verifier limit") from exc
    except (TypeError, ValueError, UnicodeError) as exc:
        raise OciVerificationError("cannot encode canonical JSON") from exc


def _json_document(raw: bytes, context: str, *, compact: bool) -> Any:
    try:
        value = json.loads(
            raw.decode("utf-8", errors="strict"),
            object_pairs_hook=_duplicate_keys,
            parse_constant=_reject_json_constant,
        )
    except OciVerificationError:
        raise
    except RecursionError as exc:
        raise OciVerificationError("JSON nesting exceeds the verifier limit") from exc
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise OciVerificationError(f"invalid {context} JSON") from exc
    _validate_json_nesting(value)
    if _canonical_json(value, compact=compact) != raw:
        raise OciVerificationError(f"{context} is not canonical JSON")
    return value


def _exact_object(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise OciVerificationError(f"{context} must be an object")
    actual = set(value)
    if actual != keys:
        raise OciVerificationError(
            f"{context} keys differ: missing={sorted(keys - actual)!r} "
            f"extra={sorted(actual - keys)!r}"
        )
    return value


def _integer(value: Any, context: str, *, minimum: int = 0) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise OciVerificationError(f"{context} must be an integer >= {minimum}")
    return value


def _sha256(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise OciVerificationError(f"{context} must be a lowercase SHA-256")
    return value


def _digest(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value.startswith("sha256:"):
        raise OciVerificationError(f"{context} must use sha256")
    return "sha256:" + _sha256(value[7:], context)


def _relative_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise OciVerificationError(f"{context} must be a nonempty string")
    try:
        value.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise OciVerificationError(f"{context} must be UTF-8") from exc
    pure = pathlib.PurePosixPath(value)
    if (
        pure.is_absolute()
        or str(pure) != value
        or any(part in ("", ".", "..") for part in pure.parts)
    ):
        raise OciVerificationError(f"{context} must be a normalized relative path")
    return value


def _logical_path(value: Any, context: str) -> str:
    if not isinstance(value, str) or value == "/":
        raise OciVerificationError(f"{context} must be a non-root absolute path")
    try:
        value.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise OciVerificationError(f"{context} must be UTF-8") from exc
    pure = pathlib.PurePosixPath(value)
    if (
        not pure.is_absolute()
        or str(pure) != value
        or any(part in ("", ".", "..") for part in pure.parts[1:])
    ):
        raise OciVerificationError(f"{context} must be a normalized absolute path")
    return value


def _mode(value: Any, context: str) -> int:
    if (
        not isinstance(value, str)
        or len(value) != 4
        or any(character not in "01234567" for character in value)
    ):
        raise OciVerificationError(f"{context} must be four octal digits")
    parsed = int(value, 8)
    if parsed & (stat.S_ISUID | stat.S_ISGID):
        raise OciVerificationError(f"{context} contains set-id bits")
    return parsed


def _safe_link(path: str, target: Any, context: str) -> str:
    if not isinstance(target, str) or not target or target.startswith("/"):
        raise OciVerificationError(f"{context} link target is unsafe")
    try:
        target.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise OciVerificationError(f"{context} link target is not UTF-8") from exc
    base = list(pathlib.PurePosixPath(path).parent.parts)
    for part in pathlib.PurePosixPath(target).parts:
        if part in ("", "."):
            continue
        if part == "..":
            if not base:
                raise OciVerificationError(f"{context} link escapes the rootfs")
            base.pop()
        else:
            base.append(part)
    if not base:
        raise OciVerificationError(f"{context} link resolves to root")
    return "/".join(base)


def _tar_octal(field: bytes, context: str) -> int:
    if field and field[0] & 0x80:
        raise OciVerificationError(f"{context} uses non-canonical base-256 numbering")
    stripped = field.rstrip(b"\x00 ").lstrip(b" ")
    if not stripped or any(character not in b"01234567" for character in stripped):
        raise OciVerificationError(f"{context} has an invalid octal number")
    try:
        return int(stripped, 8)
    except ValueError as exc:
        raise OciVerificationError(f"{context} has an invalid octal number") from exc


def _validate_tar_envelope(layer: _OpenedEntry) -> None:
    size = layer.metadata.st_size
    if size <= 0 or size > MAX_LAYER_BYTES or size % (20 * 512) != 0:
        raise OciVerificationError("OCI layer has an invalid uncompressed tar size")
    zero = b"\x00" * 512
    offset = 0
    headers = 0
    found_end = False
    try:
        while offset < size:
            block = os.pread(layer.descriptor, 512, offset)
            if len(block) != 512:
                raise OciVerificationError("OCI layer tar header is truncated")
            if block == zero:
                following = os.pread(layer.descriptor, 512, offset + 512)
                if len(following) != 512 or following != zero:
                    raise OciVerificationError("OCI layer lacks the two-block tar terminator")
                tail = offset + 1024
                while tail < size:
                    chunk = os.pread(layer.descriptor, min(1024 * 1024, size - tail), tail)
                    if not chunk or any(chunk):
                        raise OciVerificationError("OCI layer has nonzero bytes after tar end")
                    tail += len(chunk)
                found_end = True
                break
            headers += 1
            if headers > MAX_TAR_HEADERS:
                raise OciVerificationError("OCI layer exceeds its tar header limit")
            stored_checksum = _tar_octal(block[148:156], "OCI layer checksum")
            computed_checksum = sum(block[:148]) + (8 * 32) + sum(block[156:])
            if stored_checksum != computed_checksum:
                raise OciVerificationError("OCI layer tar checksum differs")
            if block[257:263] != b"ustar\x00" or block[263:265] != b"00":
                raise OciVerificationError("OCI layer tar format is not canonical POSIX/PAX")
            header_type = block[156:157]
            if header_type not in (b"\x00", b"0", b"1", b"2", b"5", b"x"):
                raise OciVerificationError("OCI layer contains an unsupported tar header type")
            payload_size = _tar_octal(block[124:136], "OCI layer member size")
            if header_type == b"x" and payload_size > MAX_PAX_HEADER_BYTES:
                raise OciVerificationError("OCI layer PAX header exceeds its byte limit")
            if header_type == b"x" and (
                block[:100].rstrip(b"\x00") != b"././@PaxHeader"
                or _tar_octal(block[100:108], "OCI PAX header mode") != 0
                or _tar_octal(block[108:116], "OCI PAX header uid") != 0
                or _tar_octal(block[116:124], "OCI PAX header gid") != 0
                or _tar_octal(block[136:148], "OCI PAX header mtime") != 0
                or any(block[157:257])
                or any(block[265:329])
            ):
                raise OciVerificationError("OCI layer PAX header metadata differs")
            payload_end = offset + 512 + payload_size
            rounded_end = offset + 512 + (((payload_size + 511) // 512) * 512)
            padding_offset = payload_end
            while padding_offset < rounded_end:
                padding = os.pread(
                    layer.descriptor,
                    min(1024 * 1024, rounded_end - padding_offset),
                    padding_offset,
                )
                if not padding or any(padding):
                    raise OciVerificationError("OCI layer tar member padding is not zero")
                padding_offset += len(padding)
            offset = rounded_end
            if offset > size:
                raise OciVerificationError("OCI layer tar member exceeds the blob")
    except OciVerificationError:
        raise
    except OSError as exc:
        raise OciVerificationError("cannot inspect OCI layer tar envelope") from exc
    if not found_end:
        raise OciVerificationError("OCI layer has no canonical tar terminator")
    layer.assert_unchanged()


def _layer_entries(layer: _OpenedEntry) -> dict[str, dict[str, Any]]:
    _validate_tar_envelope(layer)
    entries: dict[str, dict[str, Any]] = {}
    total_file_bytes = 0
    observed_order: list[str] = []
    try:
        with os.fdopen(os.dup(layer.descriptor), "rb", closefd=True) as source:
            source.seek(0)
            with tarfile.open(fileobj=source, mode="r:") as archive:
                for member in archive:
                    path = _relative_path(member.name, "OCI layer member path")
                    if path in entries:
                        raise OciVerificationError(f"OCI layer duplicate path: {path}")
                    if path in FORBIDDEN_ROOTFS_PATHS:
                        raise OciVerificationError(f"OCI layer forbidden path: {path}")
                    if pathlib.PurePosixPath(path).name.startswith(".wh."):
                        raise OciVerificationError(f"OCI layer whiteout is forbidden: {path}")
                    if len(entries) >= MAX_ENTRIES:
                        raise OciVerificationError("OCI layer exceeds its entry limit")
                    if member.uid != 0 or member.gid != 0 or member.uname or member.gname:
                        raise OciVerificationError(f"OCI layer member is not canonical root:root: {path}")
                    if member.mtime != 0:
                        raise OciVerificationError(f"OCI layer member mtime differs: {path}")
                    if member.mode < 0 or member.mode > 0o7777:
                        raise OciVerificationError(f"OCI layer member mode is invalid: {path}")
                    mode = member.mode & 0o7777
                    if mode & (stat.S_ISUID | stat.S_ISGID):
                        raise OciVerificationError(f"OCI layer set-id member is forbidden: {path}")
                    if member.sparse is not None:
                        raise OciVerificationError(f"OCI layer sparse member is forbidden: {path}")
                    if member.pax_headers:
                        allowed = {"path", "linkpath"}
                        if not set(member.pax_headers).issubset(allowed):
                            raise OciVerificationError(f"OCI layer PAX metadata is forbidden: {path}")
                        if "path" in member.pax_headers and member.pax_headers["path"] != member.name:
                            raise OciVerificationError(f"OCI layer PAX path differs: {path}")
                        if "linkpath" in member.pax_headers and member.pax_headers["linkpath"] != member.linkname:
                            raise OciVerificationError(f"OCI layer PAX link target differs: {path}")

                    entry: dict[str, Any] = {"kind": "", "mode": mode}
                    if member.isdir():
                        if member.size != 0 or member.linkname:
                            raise OciVerificationError(f"OCI layer directory metadata differs: {path}")
                        entry["kind"] = "directory"
                    elif member.isreg():
                        if member.linkname or member.size > MAX_FILE_BYTES:
                            raise OciVerificationError(f"OCI layer regular file metadata differs: {path}")
                        total_file_bytes += member.size
                        if total_file_bytes > MAX_TOTAL_FILE_BYTES:
                            raise OciVerificationError("OCI layer exceeds its total file-byte limit")
                        payload = archive.extractfile(member)
                        if payload is None:
                            raise OciVerificationError(f"cannot read OCI layer file: {path}")
                        digest = hashlib.sha256()
                        remaining = member.size
                        while remaining:
                            chunk = payload.read(min(1024 * 1024, remaining))
                            if not chunk:
                                raise OciVerificationError(f"OCI layer file is truncated: {path}")
                            digest.update(chunk)
                            remaining -= len(chunk)
                        if payload.read(1):
                            raise OciVerificationError(f"OCI layer file exceeds declared size: {path}")
                        entry.update(
                            {
                                "kind": "file",
                                "sizeBytes": member.size,
                                "sha256": digest.hexdigest(),
                            }
                        )
                    elif member.issym():
                        if member.size != 0:
                            raise OciVerificationError(f"OCI layer symlink has payload: {path}")
                        target = member.linkname
                        entry.update(
                            {
                                "kind": "symlink",
                                "target": target,
                                "resolvedTarget": _safe_link(path, target, f"OCI layer symlink {path}"),
                            }
                        )
                    elif member.islnk():
                        raise OciVerificationError(
                            f"OCI layer hardlink is not emitted by the v1 builder: {path}"
                        )
                    else:
                        raise OciVerificationError(f"OCI layer special member is forbidden: {path}")
                    entries[path] = entry
                    observed_order.append(path)
    except OciVerificationError:
        raise
    except (OSError, EOFError, UnicodeError, tarfile.TarError) as exc:
        raise OciVerificationError("cannot parse OCI layer") from exc
    expected_order = sorted(observed_order, key=lambda value: value.encode("utf-8"))
    if observed_order != expected_order:
        raise OciVerificationError("OCI layer entries are not in canonical byte order")
    for path in entries:
        parent = pathlib.PurePosixPath(path).parent
        while str(parent) != ".":
            parent_path = str(parent)
            parent_entry = entries.get(parent_path)
            if parent_entry is None or parent_entry["kind"] != "directory":
                raise OciVerificationError(
                    f"OCI layer parent directory is absent or not a directory: {path}"
                )
            parent = parent.parent
    for path, entry in entries.items():
        if entry["kind"] == "hardlink":
            target = entries.get(entry["target"])
            if target is None or target["kind"] != "file":
                raise OciVerificationError(f"OCI layer hardlink target is absent: {path}")
        elif entry["kind"] == "symlink" and entry["resolvedTarget"] not in entries:
            raise OciVerificationError(f"OCI layer symlink target is absent: {path}")
    for path, entry in entries.items():
        if entry["kind"] != "symlink":
            continue
        visited: set[str] = set()
        target = entry["resolvedTarget"]
        while entries[target]["kind"] == "symlink":
            if target in visited:
                raise OciVerificationError(f"OCI layer symlink cycle is forbidden: {path}")
            visited.add(target)
            target = entries[target]["resolvedTarget"]
            if target not in entries:
                raise OciVerificationError(f"OCI layer symlink target is absent: {path}")
    layer.assert_unchanged()
    return entries


def _expected_closures(logical_path: str) -> list[str]:
    return [
        name
        for name, roots in FIXED_CLOSURES
        if any(
            logical_path == root or logical_path.startswith(root.rstrip("/") + "/")
            for root in roots
        )
    ]


def _verify_content_manifest(raw: bytes, layer_entries: dict[str, dict[str, Any]]) -> None:
    content = _exact_object(
        _json_document(raw, "rootfs content manifest", compact=False),
        {"schema", "activationAllowed", "productionByteProvenanceComplete", "entries"},
        "rootfs content manifest",
    )
    if (
        content["schema"] != "boole.native-shadow.rootfs-content-manifest.v1"
        or content["activationAllowed"] is not False
        or content["productionByteProvenanceComplete"] is not False
        or not isinstance(content["entries"], list)
    ):
        raise OciVerificationError("rootfs content manifest authority differs")
    by_path: dict[str, dict[str, Any]] = {}
    order: list[str] = []
    for raw_entry in content["entries"]:
        if not isinstance(raw_entry, dict):
            raise OciVerificationError("rootfs content manifest entry must be an object")
        logical = _logical_path(raw_entry.get("logicalPath"), "rootfs content logicalPath")
        path = logical[1:]
        if path in by_path:
            raise OciVerificationError("rootfs content manifest has a duplicate path")
        by_path[path] = raw_entry
        order.append(path)
    if order != sorted(order, key=lambda value: value.encode("utf-8")):
        raise OciVerificationError("rootfs content manifest entries are not in canonical order")
    if set(by_path) != set(layer_entries):
        raise OciVerificationError("rootfs content manifest path set differs from layer")

    populated_closures: set[str] = set()
    for path, layer_entry in layer_entries.items():
        item = by_path[path]
        logical = "/" + path
        closures = _expected_closures(logical)
        common: dict[str, Any] = {
            "logicalPath": logical,
            "kind": layer_entry["kind"],
            "mode": f"{layer_entry['mode']:04o}",
            "uid": 0,
            "gid": 0,
            "closures": closures,
        }
        expected_keys = set(common)
        if layer_entry["kind"] == "file":
            common.update(
                {
                    "sizeBytes": layer_entry["sizeBytes"],
                    "sha256": layer_entry["sha256"],
                }
            )
            expected_keys.update({"sizeBytes", "sha256"})
            populated_closures.update(closures)
        elif layer_entry["kind"] in ("symlink", "hardlink"):
            common["target"] = layer_entry["target"]
            expected_keys.add("target")
        if set(item) != expected_keys or item != common:
            raise OciVerificationError(f"rootfs content manifest entry differs: {path}")
    required = {name for name, _ in FIXED_CLOSURES}
    if populated_closures != required:
        raise OciVerificationError("rootfs content manifest does not populate every fixed closure")


def _descriptor(
    value: Any,
    context: str,
    *,
    expected_media_type: str,
) -> tuple[str, int]:
    document = _exact_object(value, {"mediaType", "digest", "size"}, context)
    if document["mediaType"] != expected_media_type:
        raise OciVerificationError(f"{context} media type differs")
    digest = _digest(document["digest"], f"{context}.digest")
    size = _integer(document["size"], f"{context}.size")
    return digest, size


def _verify_blob(
    sha_directory: int,
    digest: str,
    size: int,
    context: str,
    *,
    limit: int,
) -> _OpenedEntry:
    if size > limit:
        raise OciVerificationError(f"{context} descriptor exceeds its byte limit")
    name = digest[7:]
    opened = _open_file_at(sha_directory, name, context)
    try:
        matches = opened.metadata.st_size == size and _hash_opened(opened, limit) == name
    except Exception:
        opened.close()
        raise
    if not matches:
        opened.close()
        raise OciVerificationError(f"{context} bytes differ from descriptor")
    return opened


def _verify_receipt(
    raw: bytes,
    *,
    expected_source_lock_sha256: str,
    expected_builder_sha256: str,
    content_raw: bytes,
    index_raw: bytes,
    manifest_digest: str,
    manifest_size: int,
    config_digest: str,
    config_size: int,
    layer_digest: str,
    layer_size: int,
) -> dict[str, Any]:
    receipt = _exact_object(
        _json_document(raw, "rootfs build receipt", compact=False),
        {
            "schema",
            "authorityStatus",
            "activationAllowed",
            "productionByteProvenanceComplete",
            "sourceLockSha256",
            "builderSha256",
            "rootfsContentManifestSha256",
            "rootfsContentManifestSizeBytes",
            "layerDigest",
            "layerSizeBytes",
            "configDigest",
            "configSizeBytes",
            "manifestDigest",
            "manifestSizeBytes",
            "indexSha256",
            "indexSizeBytes",
            "layerCount",
            "parentLayerCount",
        },
        "rootfs build receipt",
    )
    for field in (
        "rootfsContentManifestSizeBytes",
        "layerSizeBytes",
        "configSizeBytes",
        "manifestSizeBytes",
        "indexSizeBytes",
        "layerCount",
        "parentLayerCount",
    ):
        _integer(receipt[field], f"rootfs build receipt.{field}")
    for field in ("sourceLockSha256", "builderSha256", "rootfsContentManifestSha256"):
        _sha256(receipt[field], f"rootfs build receipt.{field}")
    for field in ("layerDigest", "configDigest", "manifestDigest"):
        _digest(receipt[field], f"rootfs build receipt.{field}")

    expected_content_hash = hashlib.sha256(content_raw).hexdigest()
    expected_index_hash = hashlib.sha256(index_raw).hexdigest()
    if (
        receipt["schema"] != "boole.native-shadow.runtime-rootfs-build-receipt.v1"
        or receipt["authorityStatus"] != "BUILT-NOT-ACTIVATABLE"
        or receipt["activationAllowed"] is not False
        or receipt["productionByteProvenanceComplete"] is not False
        or receipt["sourceLockSha256"] != expected_source_lock_sha256
        or receipt["builderSha256"] != expected_builder_sha256
        or receipt["rootfsContentManifestSha256"] != expected_content_hash
        or receipt["rootfsContentManifestSizeBytes"] != len(content_raw)
        or receipt["layerDigest"] != layer_digest
        or receipt["layerSizeBytes"] != layer_size
        or receipt["configDigest"] != config_digest
        or receipt["configSizeBytes"] != config_size
        or receipt["manifestDigest"] != manifest_digest
        or receipt["manifestSizeBytes"] != manifest_size
        or receipt["indexSha256"] != expected_index_hash
        or receipt["indexSizeBytes"] != len(index_raw)
        or receipt["layerCount"] != 1
        or receipt["parentLayerCount"] != 0
    ):
        raise OciVerificationError("rootfs build receipt differs from observed OCI bytes")
    return receipt


def verify_layout(
    layout: os.PathLike[str] | str,
    expected_source_lock_sha256: str,
    expected_builder_sha256: str,
    expected_layer_digest: str,
    expected_content_manifest_sha256: str,
) -> dict[str, Any]:
    """Verify *layout* and return its validated build receipt.

    All four expected identities are supplied by the caller's authority
    boundary; none is learned from the untrusted layout itself.  The layer and
    content-manifest pins are mandatory because receipt fields are claims, not
    provenance.  This verifier checks exact OCI bytes and structure; it does
    not independently rebuild the package closure.
    """

    expected_source_lock_sha256 = _sha256(
        expected_source_lock_sha256, "expected source-lock SHA-256"
    )
    expected_builder_sha256 = _sha256(
        expected_builder_sha256, "expected builder SHA-256"
    )
    expected_layer_digest = _digest(
        expected_layer_digest, "expected OCI layer digest"
    )
    expected_content_manifest_sha256 = _sha256(
        expected_content_manifest_sha256,
        "expected rootfs content-manifest SHA-256",
    )
    layout_path = os.fspath(layout)
    opened: list[_OpenedEntry] = []
    root_descriptor = -1
    root_metadata: Optional[os.stat_result] = None
    try:
        try:
            before = os.stat(layout_path, follow_symlinks=False)
            if not stat.S_ISDIR(before.st_mode) or stat.S_IMODE(before.st_mode) != 0o755:
                raise OciVerificationError("OCI layout must be a mode-0755 real directory")
            root_descriptor = os.open(layout_path, _DIRECTORY_FLAGS)
        except OciVerificationError:
            raise
        except OSError as exc:
            raise OciVerificationError("cannot open OCI layout without following links") from exc
        root_metadata = os.fstat(root_descriptor)
        if _snapshot(root_metadata) != _snapshot(before):
            raise OciVerificationError("OCI layout changed while opening")
        if _directory_names(root_descriptor, "OCI layout") != TOP_LEVEL_ENTRIES:
            raise OciVerificationError("OCI layout top-level entry set differs")

        sidecars: dict[str, _OpenedEntry] = {}
        for name in (
            "BUILD-RECEIPT.json",
            "ROOTFS-CONTENT-MANIFEST.json",
            "index.json",
            "oci-layout",
        ):
            sidecars[name] = _open_file_at(root_descriptor, name, f"OCI layout {name}")
            opened.append(sidecars[name])
        blobs = _open_directory_at(root_descriptor, "blobs", "OCI blobs directory")
        opened.append(blobs)
        if _directory_names(blobs.descriptor, "OCI blobs directory") != frozenset({"sha256"}):
            raise OciVerificationError("OCI blobs directory entry set differs")
        sha_directory = _open_directory_at(blobs.descriptor, "sha256", "OCI sha256 directory")
        opened.append(sha_directory)

        layout_raw = _read_opened(sidecars["oci-layout"], MAX_SMALL_SIDECAR_BYTES)
        if _json_document(layout_raw, "OCI layout marker", compact=True) != {
            "imageLayoutVersion": "1.0.0"
        }:
            raise OciVerificationError("OCI layout marker differs")
        index_raw = _read_opened(sidecars["index.json"], MAX_SMALL_SIDECAR_BYTES)
        index = _exact_object(
            _json_document(index_raw, "OCI index", compact=True),
            {"schemaVersion", "manifests"},
            "OCI index",
        )
        if (
            index["schemaVersion"] != 2
            or not isinstance(index["manifests"], list)
            or len(index["manifests"]) != 1
        ):
            raise OciVerificationError("OCI index shape differs")
        index_item = _exact_object(
            index["manifests"][0],
            {"mediaType", "digest", "size", "platform", "annotations"},
            "OCI index manifest descriptor",
        )
        if index_item["platform"] != {"architecture": "amd64", "os": "linux"}:
            raise OciVerificationError("OCI index platform differs")
        if index_item["annotations"] != {
            "org.boole.native-shadow.activation-allowed": "false"
        }:
            raise OciVerificationError("OCI index activation annotation differs")
        manifest_digest, manifest_size = _descriptor(
            {key: index_item[key] for key in ("mediaType", "digest", "size")},
            "OCI manifest descriptor",
            expected_media_type=OCI_MANIFEST_MEDIA_TYPE,
        )
        manifest_blob = _verify_blob(
            sha_directory.descriptor,
            manifest_digest,
            manifest_size,
            "OCI manifest blob",
            limit=MAX_JSON_BLOB_BYTES,
        )
        opened.append(manifest_blob)
        manifest_raw = _read_opened(manifest_blob, MAX_JSON_BLOB_BYTES)
        manifest = _exact_object(
            _json_document(manifest_raw, "OCI manifest", compact=True),
            {"schemaVersion", "mediaType", "config", "layers", "annotations"},
            "OCI manifest",
        )
        if (
            manifest["schemaVersion"] != 2
            or manifest["mediaType"] != OCI_MANIFEST_MEDIA_TYPE
            or not isinstance(manifest["layers"], list)
            or len(manifest["layers"]) != 1
        ):
            raise OciVerificationError("OCI manifest shape differs")
        if manifest["annotations"] != {
            "org.boole.native-shadow.activation-allowed": "false"
        }:
            raise OciVerificationError("OCI manifest activation annotation differs")
        config_digest, config_size = _descriptor(
            manifest["config"], "OCI config descriptor", expected_media_type=OCI_CONFIG_MEDIA_TYPE
        )
        layer_digest, layer_size = _descriptor(
            manifest["layers"][0],
            "OCI layer descriptor",
            expected_media_type=OCI_LAYER_MEDIA_TYPE,
        )
        if layer_digest != expected_layer_digest:
            raise OciVerificationError("OCI layer differs from external authority")
        blob_names = frozenset(
            {manifest_digest[7:], config_digest[7:], layer_digest[7:]}
        )
        if len(blob_names) != 3:
            raise OciVerificationError("OCI manifest/config/layer digests must be distinct")
        if _directory_names(sha_directory.descriptor, "OCI sha256 directory") != blob_names:
            raise OciVerificationError("OCI blob set contains missing or extra entries")

        config_blob = _verify_blob(
            sha_directory.descriptor,
            config_digest,
            config_size,
            "OCI config blob",
            limit=MAX_JSON_BLOB_BYTES,
        )
        opened.append(config_blob)
        layer_blob = _verify_blob(
            sha_directory.descriptor,
            layer_digest,
            layer_size,
            "OCI layer blob",
            limit=MAX_LAYER_BYTES,
        )
        opened.append(layer_blob)
        config = _exact_object(
            _json_document(_read_opened(config_blob, MAX_JSON_BLOB_BYTES), "OCI config", compact=True),
            {"architecture", "config", "os", "rootfs"},
            "OCI config",
        )
        if config["architecture"] != "amd64" or config["os"] != "linux":
            raise OciVerificationError("OCI config platform differs")
        # Exact equality intentionally forbids Cmd, Entrypoint, User, WorkingDir,
        # and every other executable runtime instruction.
        if config["config"] != {
            "Env": ["LANG=C", "LC_ALL=C", "TZ=UTC"],
            "Labels": {"org.boole.native-shadow.activation-allowed": "false"},
        }:
            raise OciVerificationError("OCI config is activatable or contains a command")
        if config["rootfs"] != {"diff_ids": [layer_digest], "type": "layers"}:
            raise OciVerificationError("OCI config rootfs differs")

        layer_entries = _layer_entries(layer_blob)
        content_raw = _read_opened(
            sidecars["ROOTFS-CONTENT-MANIFEST.json"], MAX_CONTENT_MANIFEST_BYTES
        )
        if hashlib.sha256(content_raw).hexdigest() != expected_content_manifest_sha256:
            raise OciVerificationError(
                "rootfs content manifest differs from external authority"
            )
        _verify_content_manifest(content_raw, layer_entries)
        receipt = _verify_receipt(
            _read_opened(sidecars["BUILD-RECEIPT.json"], MAX_SMALL_SIDECAR_BYTES),
            expected_source_lock_sha256=expected_source_lock_sha256,
            expected_builder_sha256=expected_builder_sha256,
            content_raw=content_raw,
            index_raw=index_raw,
            manifest_digest=manifest_digest,
            manifest_size=manifest_size,
            config_digest=config_digest,
            config_size=config_size,
            layer_digest=layer_digest,
            layer_size=layer_size,
        )

        if _directory_names(root_descriptor, "OCI layout") != TOP_LEVEL_ENTRIES:
            raise OciVerificationError("OCI layout changed while verifying")
        if _directory_names(blobs.descriptor, "OCI blobs directory") != frozenset({"sha256"}):
            raise OciVerificationError("OCI blobs directory changed while verifying")
        if _directory_names(sha_directory.descriptor, "OCI sha256 directory") != blob_names:
            raise OciVerificationError("OCI sha256 directory changed while verifying")
        for entry in opened:
            entry.assert_unchanged()
        try:
            named_root = os.stat(layout_path, follow_symlinks=False)
        except OSError as exc:
            raise OciVerificationError("OCI layout path changed while verifying") from exc
        if root_metadata is None or _snapshot(named_root) != _snapshot(root_metadata):
            raise OciVerificationError("OCI layout path changed while verifying")
        return receipt
    finally:
        for entry in reversed(opened):
            entry.close()
        if root_descriptor >= 0:
            try:
                os.close(root_descriptor)
            except OSError:
                pass


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", nargs="?", choices=("verify",))
    parser.add_argument("--layout", required=True, type=pathlib.Path)
    parser.add_argument("--expected-source-lock-sha256", required=True)
    parser.add_argument("--expected-builder-sha256", required=True)
    parser.add_argument("--expected-layer-digest", required=True)
    parser.add_argument("--expected-content-manifest-sha256", required=True)
    args = parser.parse_args(argv)
    try:
        receipt = verify_layout(
            args.layout,
            args.expected_source_lock_sha256,
            args.expected_builder_sha256,
            args.expected_layer_digest,
            args.expected_content_manifest_sha256,
        )
        print(json.dumps(receipt, sort_keys=True, separators=(",", ":")))
        return 0
    except (OciVerificationError, OSError) as exc:
        print(f"native-shadow-rootfs-oci-verify: FAIL: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
