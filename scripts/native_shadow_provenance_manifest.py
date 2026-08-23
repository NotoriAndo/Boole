#!/usr/bin/env python3
"""Compile and verify canonical native-shadow byte-provenance manifests.

This is deliberately a scaffold compiler, not an activation switch.  It proves
that the same closed filesystem inventory always has the same canonical bytes
and that any entry or metadata drift is rejected.  A later release must bind
those bytes to immutable source artifacts before checker execution is allowed.
"""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
import pathlib
import stat
import sys
import tempfile
from typing import Any, Iterable, Optional


SCHEMA = "boole.native-shadow.production-byte-provenance.v1"
INVENTORY_SCHEMA = "boole.native-shadow.provenance-inventory.v1"
REQUIRED_CLOSURES = (
    "installed-rust-toolchain-file-manifest",
    "python-interpreter-and-stdlib-file-manifest",
    "system-linker-and-runtime-file-manifest",
)


class ManifestError(ValueError):
    """The inventory or installed tree is not an exact admissible authority."""


def _reject_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ManifestError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def canonical_json_bytes(document: Any) -> bytes:
    try:
        return (
            json.dumps(
                document,
                ensure_ascii=False,
                indent=2,
                sort_keys=True,
                allow_nan=False,
            )
            + "\n"
        ).encode("utf-8")
    except (TypeError, ValueError, UnicodeError) as exc:
        raise ManifestError("document is not canonical UTF-8 JSON") from exc


def _load_json(raw: bytes) -> Any:
    try:
        text = raw.decode("utf-8")
        return json.loads(text, object_pairs_hook=_reject_duplicate_keys)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise ManifestError("invalid UTF-8 JSON") from exc


def _exact_keys(value: dict[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ManifestError(
            f"{context} keys differ: missing={sorted(expected - actual)!r} "
            f"extra={sorted(actual - expected)!r}"
        )


def _logical_path(raw: Any, context: str) -> str:
    if not isinstance(raw, str) or not raw.startswith("/") or raw == "/":
        raise ManifestError(f"{context} must be an absolute non-root path")
    try:
        raw.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise ManifestError(f"{context} is not strict UTF-8") from exc
    parts = raw.split("/")[1:]
    if not parts or any(part in ("", ".", "..") for part in parts):
        raise ManifestError(f"{context} is not a normalized absolute path")
    if str(pathlib.PurePosixPath(raw)) != raw:
        raise ManifestError(f"{context} is not canonical")
    return raw


def _path_sort_key(path: str) -> bytes:
    try:
        return path.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise ManifestError("manifest path is not strict UTF-8") from exc


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


def _sha256_file(descriptor: int, expected: os.stat_result, display: str) -> str:
    digest = hashlib.sha256()
    try:
        before = os.fstat(descriptor)
        if _snapshot(before) != _snapshot(expected) or not stat.S_ISREG(before.st_mode):
            raise ManifestError(f"regular file changed before hashing: {display}")
        os.lseek(descriptor, 0, os.SEEK_SET)
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
        after = os.fstat(descriptor)
        if _snapshot(after) != _snapshot(before):
            raise ManifestError(f"regular file changed while hashing: {display}")
        return digest.hexdigest()
    except ManifestError:
        raise
    except OSError as exc:
        raise ManifestError(f"cannot hash opened regular file: {display}") from exc


def _darwin_xattr_names(descriptor: int) -> list[bytes]:
    library = ctypes.CDLL(None, use_errno=True)
    function = library.flistxattr
    function.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int]
    function.restype = ctypes.c_ssize_t
    size = function(descriptor, None, 0, 0)
    if size < 0:
        raise OSError(ctypes.get_errno(), "flistxattr failed")
    if size == 0:
        return []
    buffer = ctypes.create_string_buffer(size)
    written = function(descriptor, buffer, size, 0)
    if written < 0:
        raise OSError(ctypes.get_errno(), "flistxattr failed")
    return [name for name in bytes(buffer.raw[:written]).split(b"\0") if name]


def _darwin_xattr_value(descriptor: int, name: bytes) -> bytes:
    library = ctypes.CDLL(None, use_errno=True)
    function = library.fgetxattr
    function.argtypes = [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.c_uint32,
        ctypes.c_int,
    ]
    function.restype = ctypes.c_ssize_t
    size = function(descriptor, name, None, 0, 0, 0)
    if size < 0:
        raise OSError(ctypes.get_errno(), "fgetxattr failed")
    if size == 0:
        return b""
    buffer = ctypes.create_string_buffer(size)
    written = function(descriptor, name, buffer, size, 0, 0)
    if written < 0:
        raise OSError(ctypes.get_errno(), "fgetxattr failed")
    return bytes(buffer.raw[:written])


def _extended_attributes(descriptor: int, display: str) -> list[dict[str, str]]:
    try:
        if hasattr(os, "listxattr"):
            raw_names = os.listxattr(descriptor)
            pairs = [(name, os.getxattr(descriptor, name)) for name in raw_names]
        elif sys.platform == "darwin":
            names = _darwin_xattr_names(descriptor)
            pairs = [(name, _darwin_xattr_value(descriptor, name)) for name in names]
        else:
            raise ManifestError("platform cannot inspect extended attributes")
    except ManifestError:
        raise
    except (OSError, TypeError, ValueError) as exc:
        raise ManifestError(f"cannot inspect extended attributes: {display}") from exc
    result: list[dict[str, str]] = []
    for raw_name, value in pairs:
        try:
            name = (
                raw_name.decode("utf-8", errors="strict")
                if isinstance(raw_name, bytes)
                else str(raw_name)
            )
            name.encode("utf-8", errors="strict")
        except UnicodeError as exc:
            raise ManifestError(f"extended attribute name is not UTF-8: {display}") from exc
        result.append({"name": name, "valueHex": bytes(value).hex()})
    result.sort(key=lambda item: _path_sort_key(item["name"]))
    return result


def _common_entry(
    metadata: os.stat_result,
    logical: str,
    closures: Iterable[str],
    *,
    xattrs: list[dict[str, str]],
) -> dict[str, Any]:
    common: dict[str, Any] = {
        "logicalPath": logical,
        "closures": sorted(closures),
        "mode": format(stat.S_IMODE(metadata.st_mode), "04o"),
        "uid": metadata.st_uid,
        "gid": metadata.st_gid,
        "xattrs": xattrs,
    }
    return common


def _opened_entry(
    descriptor: int,
    expected: os.stat_result,
    logical: str,
    closures: Iterable[str],
    display: str,
) -> dict[str, Any]:
    try:
        metadata = os.fstat(descriptor)
    except OSError as exc:
        raise ManifestError(f"cannot fstat opened source: {display}") from exc
    if _snapshot(metadata) != _snapshot(expected):
        raise ManifestError(f"source changed before no-follow open: {display}")
    common = _common_entry(
        metadata,
        logical,
        closures,
        xattrs=_extended_attributes(descriptor, display),
    )
    try:
        after_xattrs = os.fstat(descriptor)
    except OSError as exc:
        raise ManifestError(f"cannot recheck opened source metadata: {display}") from exc
    if _snapshot(after_xattrs) != _snapshot(metadata):
        raise ManifestError(f"source changed while reading metadata: {display}")
    if stat.S_ISREG(metadata.st_mode):
        common.update(
            kind="file",
            sizeBytes=metadata.st_size,
            sha256=_sha256_file(descriptor, metadata, display),
            _device=metadata.st_dev,
            _inode=metadata.st_ino,
            _linkCount=metadata.st_nlink,
        )
    elif stat.S_ISDIR(metadata.st_mode):
        common["kind"] = "directory"
    else:
        raise ManifestError(f"unsupported opened filesystem object: {display}")
    return common


def _symlink_entry(
    parent_descriptor: int,
    name: str,
    expected: os.stat_result,
    logical: str,
    closure: str,
    display: str,
) -> dict[str, Any]:
    try:
        target = os.readlink(name, dir_fd=parent_descriptor)
        target.encode("utf-8", errors="strict")
        if target.startswith("/"):
            raise ManifestError(f"absolute symlink target is forbidden: {display}")
        after = os.stat(name, dir_fd=parent_descriptor, follow_symlinks=False)
    except ManifestError:
        raise
    except (OSError, TypeError, ValueError, UnicodeError) as exc:
        raise ManifestError(f"cannot read stable strict-UTF-8 symlink: {display}") from exc
    if _snapshot(after) != _snapshot(expected):
        raise ManifestError(f"symlink changed while reading: {display}")
    common = _common_entry(expected, logical, (closure,), xattrs=[])
    common.update(
        kind="symlink",
        target=target,
        _sourcePath=display,
    )
    return common


def _walk_opened(
    descriptor: int,
    expected: os.stat_result,
    logical: str,
    closure: str,
    display: str,
) -> list[dict[str, Any]]:
    entry = _opened_entry(descriptor, expected, logical, (closure,), display)
    if entry["kind"] != "directory":
        return [entry]
    try:
        with os.scandir(descriptor) as iterator:
            names = [child.name for child in iterator]
    except (OSError, TypeError, ValueError) as exc:
        raise ManifestError(f"cannot enumerate opened closed root: {display}") from exc
    names.sort(key=_path_sort_key)
    result = [entry]
    for name in names:
        if not isinstance(name, str):
            raise ManifestError("closed-root child name must be text")
        if name in ("", ".", "..") or "/" in name:
            raise ManifestError(f"invalid child name in closed root: {name!r}")
        try:
            name.encode("utf-8", errors="strict")
            child_metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
        except (OSError, TypeError, ValueError, UnicodeError) as exc:
            raise ManifestError(f"cannot lstat closed-root child: {display}/{name}") from exc
        child_logical = f"{logical}/{name}"
        child_display = f"{display}/{name}"
        if stat.S_ISLNK(child_metadata.st_mode):
            result.append(
                _symlink_entry(
                    descriptor,
                    name,
                    child_metadata,
                    child_logical,
                    closure,
                    child_display,
                )
            )
            continue
        if not (stat.S_ISREG(child_metadata.st_mode) or stat.S_ISDIR(child_metadata.st_mode)):
            raise ManifestError(f"unsupported filesystem object: {child_display}")
        flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
        if stat.S_ISDIR(child_metadata.st_mode):
            flags |= getattr(os, "O_DIRECTORY", 0)
        try:
            child_descriptor = os.open(name, flags, dir_fd=descriptor)
        except (OSError, TypeError, ValueError) as exc:
            raise ManifestError(f"cannot no-follow open child: {child_display}") from exc
        try:
            result.extend(
                _walk_opened(
                    child_descriptor,
                    child_metadata,
                    child_logical,
                    closure,
                    child_display,
                )
            )
        finally:
            os.close(child_descriptor)
    try:
        after = os.fstat(descriptor)
    except OSError as exc:
        raise ManifestError(f"cannot recheck opened closed root: {display}") from exc
    if _snapshot(after) != _snapshot(expected):
        raise ManifestError(f"closed directory changed during walk: {display}")
    return result


def _walk_tree(source: pathlib.Path, logical: str, closure: str) -> list[dict[str, Any]]:
    try:
        expected = source.lstat()
    except (OSError, TypeError, ValueError) as exc:
        raise ManifestError(f"cannot lstat source root: {source}") from exc
    if not (stat.S_ISREG(expected.st_mode) or stat.S_ISDIR(expected.st_mode)):
        raise ManifestError(f"source root must be a regular file or directory: {source}")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    if stat.S_ISDIR(expected.st_mode):
        flags |= getattr(os, "O_DIRECTORY", 0)
    try:
        descriptor = os.open(source, flags)
    except (OSError, TypeError, ValueError) as exc:
        raise ManifestError(f"cannot no-follow open source root: {source}") from exc
    try:
        return _walk_opened(
            descriptor,
            expected,
            logical,
            closure,
            str(source),
        )
    finally:
        os.close(descriptor)


def _normalize_target(parent: str, target: str) -> str:
    if target.startswith("/"):
        parts: list[str] = []
        incoming = target.split("/")[1:]
    else:
        parts = parent.split("/")[1:]
        incoming = target.split("/")
    for part in incoming:
        if part in ("", "."):
            continue
        if part == "..":
            if not parts:
                raise ManifestError("symlink target escapes the absolute namespace")
            parts.pop()
        else:
            part.encode("utf-8", errors="strict")
            parts.append(part)
    if not parts:
        return "/"
    return "/" + "/".join(parts)


def _validate_symlinks(
    entries: list[dict[str, Any]], roots: list[dict[str, str]]
) -> None:
    by_path = {entry["logicalPath"]: entry for entry in entries}
    mappings = [
        (root["logicalPath"], os.path.normpath(root["sourcePath"])) for root in roots
    ]

    def mapped_physical_path(target: str) -> str:
        matching = [
            (logical_root, source_root)
            for logical_root, source_root in mappings
            if target == logical_root or target.startswith(logical_root + "/")
        ]
        if not matching:
            raise ManifestError("symlink target is outside declared source-root mappings")
        logical_root, source_root = max(matching, key=lambda item: len(item[0]))
        relative = target[len(logical_root) :].lstrip("/")
        return os.path.normpath(os.path.join(source_root, relative))

    for entry in entries:
        if entry["kind"] != "symlink":
            continue
        current = entry
        visited: set[str] = set()
        while current["kind"] == "symlink":
            current_path = current["logicalPath"]
            if current_path in visited:
                raise ManifestError("symlink cycle in closed authority")
            visited.add(current_path)
            target_text = current["target"]
            target = _normalize_target(
                str(pathlib.PurePosixPath(current_path).parent), target_text
            )
            target_entry = by_path.get(target)
            if target_entry is None:
                raise ManifestError("symlink target escapes its declared closure")
            if not target_text.startswith("/"):
                actual_physical = os.path.normpath(
                    os.path.join(os.path.dirname(current["_sourcePath"]), target_text)
                )
                if actual_physical != mapped_physical_path(target):
                    raise ManifestError(
                        "relative symlink physical target differs from its logical mapping"
                    )
            current = target_entry


def _validated_inventory(inventory: Any) -> tuple[str, dict[str, str], list[dict[str, str]], list[dict[str, Any]]]:
    if not isinstance(inventory, dict):
        raise ManifestError("inventory must be an object")
    _exact_keys(
        inventory,
        {"schema", "release", "platform", "sourceArtifacts", "closures"},
        "inventory",
    )
    if inventory["schema"] != INVENTORY_SCHEMA:
        raise ManifestError("inventory schema differs")
    release = inventory["release"]
    if not isinstance(release, str) or not release:
        raise ManifestError("inventory release must be nonempty")
    platform = inventory["platform"]
    if platform != {"os": "linux", "arch": "x86_64"}:
        raise ManifestError("only the frozen linux/x86_64 platform is admissible")
    artifacts = inventory["sourceArtifacts"]
    if not isinstance(artifacts, list) or not artifacts:
        raise ManifestError("sourceArtifacts must be a nonempty list")
    normalized_artifacts: list[dict[str, str]] = []
    names: set[str] = set()
    for index, artifact in enumerate(artifacts):
        if not isinstance(artifact, dict):
            raise ManifestError(f"sourceArtifacts[{index}] must be an object")
        _exact_keys(artifact, {"name", "sha256"}, f"sourceArtifacts[{index}]")
        name = artifact["name"]
        digest = artifact["sha256"]
        if not isinstance(name, str) or not name or name in names:
            raise ManifestError("source artifact names must be nonempty and unique")
        if (
            not isinstance(digest, str)
            or len(digest) != 64
            or any(character not in "0123456789abcdef" for character in digest)
        ):
            raise ManifestError("source artifact digest must be 64 lowercase hex")
        names.add(name)
        normalized_artifacts.append({"name": name, "sha256": digest})
    normalized_artifacts.sort(key=lambda value: _path_sort_key(value["name"]))

    closures = inventory["closures"]
    if not isinstance(closures, list) or len(closures) != len(REQUIRED_CLOSURES):
        raise ManifestError("inventory must define exactly three closures")
    by_name: dict[str, dict[str, Any]] = {}
    for index, closure in enumerate(closures):
        if not isinstance(closure, dict):
            raise ManifestError(f"closures[{index}] must be an object")
        _exact_keys(closure, {"name", "roots"}, f"closures[{index}]")
        name = closure["name"]
        if not isinstance(name, str) or not name:
            raise ManifestError("closure names must be nonempty strings")
        if name in by_name:
            raise ManifestError("duplicate closure name")
        by_name[name] = closure
    if set(by_name) != set(REQUIRED_CLOSURES):
        raise ManifestError("closure names differ from the three frozen blockers")

    normalized_closures: list[dict[str, Any]] = []
    for name in REQUIRED_CLOSURES:
        roots = by_name[name]["roots"]
        if not isinstance(roots, list) or not roots:
            raise ManifestError(f"closure {name} must have at least one root")
        normalized_roots: list[dict[str, str]] = []
        seen_logical: list[str] = []
        for index, root in enumerate(roots):
            if not isinstance(root, dict):
                raise ManifestError(f"closure {name} root {index} must be an object")
            _exact_keys(root, {"logicalPath", "sourcePath"}, f"closure {name} root {index}")
            logical = _logical_path(root["logicalPath"], "logicalPath")
            if any(
                logical == previous
                or logical.startswith(previous + "/")
                or previous.startswith(logical + "/")
                for previous in seen_logical
            ):
                raise ManifestError("duplicate or overlapping logical closed root")
            seen_logical.append(logical)
            source_raw = root["sourcePath"]
            if not isinstance(source_raw, str) or not source_raw:
                raise ManifestError("sourcePath must be a nonempty string")
            source = pathlib.Path(source_raw)
            if not source.is_absolute():
                raise ManifestError("sourcePath must be absolute")
            try:
                source.lstat()
            except (OSError, TypeError, ValueError) as exc:
                raise ManifestError(f"sourcePath is unavailable: {source}") from exc
            normalized_roots.append({"logicalPath": logical, "sourcePath": str(source)})
        normalized_roots.sort(key=lambda value: _path_sort_key(value["logicalPath"]))
        normalized_closures.append({"name": name, "roots": normalized_roots})
    return release, dict(platform), normalized_artifacts, normalized_closures


def compile_inventory(inventory: Any) -> bytes:
    release, platform, artifacts, closures = _validated_inventory(inventory)
    by_path: dict[str, dict[str, Any]] = {}
    closure_summary: list[dict[str, Any]] = []
    for closure in closures:
        name = closure["name"]
        logical_roots: list[str] = []
        candidates: list[dict[str, Any]] = []
        for root in closure["roots"]:
            logical = root["logicalPath"]
            source = pathlib.Path(root["sourcePath"])
            logical_roots.append(logical)
            candidates.extend(_walk_tree(source, logical, name))
        _validate_symlinks(candidates, closure["roots"])

        physical_groups: dict[tuple[int, int], list[dict[str, Any]]] = {}
        for candidate in candidates:
            if candidate["kind"] == "file":
                physical_groups.setdefault(
                    (candidate["_device"], candidate["_inode"]), []
                ).append(candidate)
        hardlink_groups: list[dict[str, list[str]]] = []
        for group in physical_groups.values():
            link_count = group[0]["_linkCount"]
            if any(member["_linkCount"] != link_count for member in group):
                raise ManifestError("hardlink metadata changed during inventory walk")
            if link_count > 1:
                members = sorted(
                    (member["logicalPath"] for member in group), key=_path_sort_key
                )
                if len(members) != link_count:
                    raise ManifestError("hardlink escapes its declared closure")
                hardlink_groups.append({"members": members})
        hardlink_groups.sort(key=lambda value: _path_sort_key(value["members"][0]))

        for candidate in candidates:
            candidate.pop("_device", None)
            candidate.pop("_inode", None)
            candidate.pop("_linkCount", None)
            candidate.pop("_sourcePath", None)
            path = candidate["logicalPath"]
            existing = by_path.get(path)
            if existing is None:
                by_path[path] = candidate
                continue
            left = dict(existing)
            right = dict(candidate)
            left_closures = set(left.pop("closures"))
            right_closures = set(right.pop("closures"))
            if left != right:
                raise ManifestError("overlapping logical paths have different metadata")
            existing["closures"] = sorted(left_closures | right_closures)
        closure_summary.append(
            {
                "name": name,
                "closedRoots": logical_roots,
                "hardlinkGroups": hardlink_groups,
            }
        )
    entries = sorted(by_path.values(), key=lambda value: _path_sort_key(value["logicalPath"]))
    document = {
        "schema": SCHEMA,
        "release": release,
        "authorityStatus": "SCAFFOLD-NOT-ACTIVATABLE",
        "activationAllowed": False,
        "productionByteProvenanceComplete": False,
        "platform": platform,
        "sourceArtifacts": artifacts,
        "metadataPolicy": {
            "identityFields": [
                "logicalPath",
                "kind",
                "mode",
                "uid",
                "gid",
                "sizeBytes",
                "sha256",
                "target",
                "closures",
                "xattrs",
            ],
            "excludedMachineLocalFields": ["device", "inode", "mtime", "ctime"],
            "walkSymlinks": False,
            "absoluteSymlinkTargets": "reject",
            "unexpectedEntryAction": "reject",
            "extendedAttributes": "canonical-name-and-value-hex",
            "symlinkExtendedAttributes": "excluded-on-linux-nonexecuting-link-object",
        },
        "closures": closure_summary,
        "entries": entries,
        "verification": {
            "canonicalEncoding": "utf8-json-sort-keys-indent-2-lf",
            "comparison": "exact-canonical-bytes-from-current-closed-roots",
            "scaffoldMayAuthorizeExecution": False,
        },
    }
    return canonical_json_bytes(document)


def verify_inventory(manifest_raw: bytes, inventory: Any) -> dict[str, Any]:
    document = _load_json(manifest_raw)
    if canonical_json_bytes(document) != manifest_raw:
        raise ManifestError("manifest bytes are not canonical")
    expected = compile_inventory(inventory)
    if manifest_raw != expected:
        raise ManifestError("installed tree differs from the canonical manifest")
    if not isinstance(document, dict):
        raise ManifestError("manifest must be an object")
    return document


def _load_inventory(path: pathlib.Path) -> Any:
    try:
        return _load_json(path.read_bytes())
    except OSError as exc:
        raise ManifestError(f"cannot read inventory: {path}") from exc


def _atomic_write(path: pathlib.Path, raw: bytes) -> None:
    if path.exists() and path.is_symlink():
        raise ManifestError("output path must not be a symlink")
    if not path.parent.is_dir() or path.parent.is_symlink():
        raise ManifestError("output parent must be an existing real directory")
    temporary: Optional[pathlib.Path] = None
    try:
        descriptor, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
        temporary = pathlib.Path(name)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        temporary = None
    except OSError as exc:
        raise ManifestError(f"cannot write manifest: {path}") from exc
    finally:
        if temporary is not None:
            try:
                temporary.unlink()
            except FileNotFoundError:
                pass


def _arguments(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    generate = subparsers.add_parser("generate")
    generate.add_argument("--inventory", required=True, type=pathlib.Path)
    generate.add_argument("--output", required=True, type=pathlib.Path)
    verify = subparsers.add_parser("verify")
    verify.add_argument("--inventory", required=True, type=pathlib.Path)
    verify.add_argument("--manifest", required=True, type=pathlib.Path)
    return parser.parse_args(argv)


def main(argv: Optional[list[str]] = None) -> int:
    args = _arguments(sys.argv[1:] if argv is None else argv)
    try:
        inventory = _load_inventory(args.inventory)
        if args.command == "generate":
            _atomic_write(args.output, compile_inventory(inventory))
        else:
            try:
                manifest_raw = args.manifest.read_bytes()
            except OSError as exc:
                raise ManifestError(f"cannot read manifest: {args.manifest}") from exc
            verify_inventory(manifest_raw, inventory)
            print("native-shadow-provenance-manifest: PASS")
    except ManifestError as exc:
        print(f"native-shadow-provenance-manifest: FAIL: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
