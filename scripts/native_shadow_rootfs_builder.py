#!/usr/bin/env python3
"""Assemble a deterministic non-activatable native-shadow rootfs offline.

The builder has no fetch command.  It reads content-addressed Rust distribution
archives and Ubuntu ``.deb`` files, copies reviewed repository authority files,
and emits a FROM-scratch single-layer OCI image layout.  The tracked lock is an
honest scaffold: Ubuntu package closure and the final recipe digest are absent,
so ``build`` refuses it.
"""

from __future__ import annotations

import argparse
import datetime
import email.utils
import gzip
import hashlib
import io
import json
import lzma
import os
import pathlib
import re
import resource
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
from typing import Any, Iterable, Optional


LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.v1"
REQUIRED_PROVENANCE_CLOSURES = (
    "installed-rust-toolchain-file-manifest",
    "python-interpreter-and-stdlib-file-manifest",
    "system-linker-and-runtime-file-manifest",
)
EXPECTED_PROVENANCE_CLOSURE_ROOTS = {
    "installed-rust-toolchain-file-manifest": (
        "/opt/boole/native-checker-toolchain",
    ),
    "python-interpreter-and-stdlib-file-manifest": (
        "/usr/bin/python3.12",
        "/usr/lib/python3.12",
    ),
    "system-linker-and-runtime-file-manifest": (
        "/lib",
        "/usr/bin",
        "/usr/lib",
    ),
}
EXPECTED_AUTHORITY_FILES = {
    "checker-entrypoint": (
        "native/checker/rust-tuple-struct-project-v1/checker.py",
        "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py",
    ),
    "checker-policy": (
        "native/checker/rust-tuple-struct-project-v1/policy.json",
        "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/policy.json",
    ),
    "checker-release": (
        "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
        "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
    ),
    "execution-policy": (
        "native/containment/native-shadow-execution-policy-v1.json",
        "/usr/share/boole/native-shadow/execution-policy-v1.json",
    ),
    "registry": (
        "fixtures/native-shadow/registry-v1.json",
        "/usr/share/boole/native-shadow/registry-v1.json",
    ),
    "toolchain-identity": (
        "native/containment/native-shadow-toolchain-identity-v1.json",
        "/usr/share/boole/native-shadow/toolchain-identity-v1.json",
    ),
}
FORBIDDEN_ROOTFS_PATHS = {
    "etc/ld.so.preload",
}
UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS = frozenset(
    {
        # Ubuntu Archive Automatic Signing Key (2018).
        "F6ECB3762474EDA9D21B7022871920D1991BC93C",
    }
)
RUST_ARCHIVE_FILENAMES = {
    "cargo": "cargo-nightly-x86_64-unknown-linux-gnu.tar.xz",
    "rust-std-x86_64-unknown-linux-gnu": "rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz",
    "rustc": "rustc-nightly-x86_64-unknown-linux-gnu.tar.xz",
}
OCI_MANIFEST_MEDIA_TYPE = "application/vnd.oci.image.manifest.v1+json"
OCI_CONFIG_MEDIA_TYPE = "application/vnd.oci.image.config.v1+json"
OCI_LAYER_MEDIA_TYPE = "application/vnd.oci.image.layer.v1.tar"


BUILDER_SOURCE_BYTES = pathlib.Path(__file__).read_bytes()
BUILDER_SHA256 = hashlib.sha256(BUILDER_SOURCE_BYTES).hexdigest()


class RootfsBuildError(ValueError):
    """A source lock, source byte, archive, or output violates the contract."""


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise RootfsBuildError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def canonical_json(document: Any, *, compact: bool = False) -> bytes:
    try:
        if compact:
            text = json.dumps(
                document,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
                allow_nan=False,
            )
        else:
            text = json.dumps(
                document,
                ensure_ascii=False,
                sort_keys=True,
                indent=2,
                allow_nan=False,
            )
        return (text + "\n").encode("utf-8")
    except (TypeError, ValueError, UnicodeError) as exc:
        raise RootfsBuildError("cannot encode canonical JSON") from exc


def load_json_exact(raw: bytes, context: str, *, require_canonical: bool = False) -> Any:
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeError, json.JSONDecodeError) as exc:
        raise RootfsBuildError(f"invalid {context} UTF-8 JSON") from exc
    if require_canonical and canonical_json(value) != raw:
        raise RootfsBuildError(f"{context} is not canonical JSON")
    return value


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise RootfsBuildError(f"{context} must be an object")
    actual = set(value)
    if actual != keys:
        raise RootfsBuildError(
            f"{context} keys differ: missing={sorted(keys - actual)!r} "
            f"extra={sorted(actual - keys)!r}"
        )
    return value


def _text(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value:
        raise RootfsBuildError(f"{context} must be a nonempty string")
    try:
        value.encode("utf-8", errors="strict")
    except UnicodeError as exc:
        raise RootfsBuildError(f"{context} must be strict UTF-8") from exc
    return value


def _sha(value: Any, context: str) -> str:
    value = _text(value, context)
    if len(value) != 64 or any(character not in "0123456789abcdef" for character in value):
        raise RootfsBuildError(f"{context} must be lowercase SHA-256")
    return value


def _git_commit(value: Any, context: str) -> str:
    value = _text(value, context)
    if len(value) != 40 or any(character not in "0123456789abcdef" for character in value):
        raise RootfsBuildError(f"{context} must be a lowercase 40-hex Git commit")
    return value


def _plain_id(value: Any, context: str) -> str:
    value = _text(value, context)
    if value in (".", "..") or "/" in value or "\\" in value:
        raise RootfsBuildError(f"{context} must be one plain identifier")
    return value


def _relative_path(value: Any, context: str) -> str:
    value = _text(value, context)
    pure = pathlib.PurePosixPath(value)
    if pure.is_absolute() or str(pure) != value or any(part in ("", ".", "..") for part in pure.parts):
        raise RootfsBuildError(f"{context} must be a normalized relative path")
    return value


def _logical_path(value: Any, context: str) -> str:
    value = _text(value, context)
    pure = pathlib.PurePosixPath(value)
    if not pure.is_absolute() or value == "/" or str(pure) != value:
        raise RootfsBuildError(f"{context} must be a normalized non-root absolute path")
    if any(part in ("", ".", "..") for part in pure.parts[1:]):
        raise RootfsBuildError(f"{context} contains an unsafe component")
    return value


def _mode(value: Any, context: str) -> int:
    if not isinstance(value, str) or len(value) != 4 or any(character not in "01234567" for character in value):
        raise RootfsBuildError(f"{context} must be four octal digits")
    parsed = int(value, 8)
    if parsed & (stat.S_ISUID | stat.S_ISGID):
        raise RootfsBuildError(f"{context} must not contain set-id bits")
    return parsed


def _integer(value: Any, context: str, *, minimum: int = 0) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise RootfsBuildError(f"{context} must be an integer >= {minimum}")
    return value


def _hash_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _open_relative_file(root: pathlib.Path, relative: str) -> bytes:
    if not root.is_dir() or root.is_symlink():
        raise RootfsBuildError("repository root must be an existing real directory")
    parts = _relative_path(relative, "repository source path").split("/")
    flags_dir = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    flags_file = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptors: list[int] = []
    try:
        current = os.open(str(root), flags_dir)
        descriptors.append(current)
        for part in parts[:-1]:
            current = os.open(
                part,
                flags_dir | getattr(os, "O_NOFOLLOW", 0),
                dir_fd=current,
            )
            descriptors.append(current)
        descriptor = os.open(parts[-1], flags_file, dir_fd=current)
        descriptors.append(descriptor)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise RootfsBuildError(f"repository source is not a regular file: {relative}")
        chunks: list[bytes] = []
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            chunks.append(chunk)
        after = os.fstat(descriptor)
        snapshot = lambda item: (
            item.st_dev,
            item.st_ino,
            item.st_mode,
            item.st_size,
            item.st_mtime_ns,
            item.st_ctime_ns,
        )
        if snapshot(before) != snapshot(after):
            raise RootfsBuildError(f"repository source changed while reading: {relative}")
        return b"".join(chunks)
    except RootfsBuildError:
        raise
    except OSError as exc:
        raise RootfsBuildError(f"cannot read repository source: {relative}") from exc
    finally:
        for descriptor in reversed(descriptors):
            try:
                os.close(descriptor)
            except OSError:
                pass


def _artifact_bytes(store: pathlib.Path, digest: str, expected_size: int) -> bytes:
    if not store.is_dir() or store.is_symlink():
        raise RootfsBuildError("artifact store must be an existing real directory")
    sha_dir = store / "sha256"
    if not sha_dir.is_dir() or sha_dir.is_symlink():
        raise RootfsBuildError("artifact store sha256 directory is missing")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    store_descriptor: Optional[int] = None
    directory_descriptor: Optional[int] = None
    descriptor: Optional[int] = None
    try:
        store_descriptor = os.open(
            str(store),
            os.O_RDONLY
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0),
        )
        if not stat.S_ISDIR(os.fstat(store_descriptor).st_mode):
            raise RootfsBuildError("artifact store is not a directory")
        directory_descriptor = os.open(
            "sha256",
            os.O_RDONLY
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0),
            dir_fd=store_descriptor,
        )
        if not stat.S_ISDIR(os.fstat(directory_descriptor).st_mode):
            raise RootfsBuildError("artifact store sha256 entry is not a directory")
        descriptor = os.open(digest, flags, dir_fd=directory_descriptor)
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_size != expected_size:
            raise RootfsBuildError(f"artifact size or type differs: {digest}")
        chunks: list[bytes] = []
        hasher = hashlib.sha256()
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            chunks.append(chunk)
            hasher.update(chunk)
        after = os.fstat(descriptor)
        if (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise RootfsBuildError(f"artifact changed while reading: {digest}")
        if hasher.hexdigest() != digest:
            raise RootfsBuildError(f"artifact digest differs: {digest}")
        return b"".join(chunks)
    except RootfsBuildError:
        raise
    except OSError as exc:
        raise RootfsBuildError(f"cannot read artifact: {digest}") from exc
    finally:
        if descriptor is not None:
            try:
                os.close(descriptor)
            except OSError:
                pass
        if directory_descriptor is not None:
            try:
                os.close(directory_descriptor)
            except OSError:
                pass
        if store_descriptor is not None:
            try:
                os.close(store_descriptor)
            except OSError:
                pass


def _artifact_table(values: Any) -> dict[str, dict[str, Any]]:
    if not isinstance(values, list):
        raise RootfsBuildError("source lock.artifacts must be a list")
    result: dict[str, dict[str, Any]] = {}
    ordered: list[str] = []
    for index, raw in enumerate(values):
        value = _exact(raw, {"id", "kind", "sizeBytes", "sha256"}, f"artifacts[{index}]")
        identifier = _plain_id(value["id"], f"artifacts[{index}].id")
        kind = _text(value["kind"], f"artifacts[{index}].kind")
        if kind not in {"rust-dist", "ubuntu-keyring", "ubuntu-inrelease", "ubuntu-packages-index", "deb"}:
            raise RootfsBuildError(f"artifacts[{index}].kind is unsupported")
        digest = _sha(value["sha256"], f"artifacts[{index}].sha256")
        size = value["sizeBytes"]
        if size is not None and (not isinstance(size, int) or isinstance(size, bool) or size < 0):
            raise RootfsBuildError(f"artifacts[{index}].sizeBytes is invalid")
        if identifier in result:
            raise RootfsBuildError(f"duplicate artifact id: {identifier}")
        result[identifier] = value
        ordered.append(identifier)
    if ordered != sorted(ordered):
        raise RootfsBuildError("source lock.artifacts must be sorted by id")
    return result


def _inactive_authority_checks(authority_bytes: dict[str, bytes]) -> None:
    parsed: dict[str, Any] = {}
    for identifier in (
        "checker-policy",
        "checker-release",
        "execution-policy",
        "registry",
        "toolchain-identity",
    ):
        parsed[identifier] = load_json_exact(
            authority_bytes[identifier],
            f"{identifier} authority",
            require_canonical=False,
        )
        if not isinstance(parsed[identifier], dict):
            raise RootfsBuildError(f"{identifier} authority must be an object")
        if parsed[identifier].get("activationAllowed") is not False:
            raise RootfsBuildError(f"{identifier} authority must remain non-activatable")

    release_fixture = parsed["checker-release"].get("qualificationFixture")
    if not isinstance(release_fixture, dict) or release_fixture.get("nonIssuable") is not True:
        raise RootfsBuildError("checker release fixture must remain non-issuable")
    registry_templates = parsed["registry"].get("templates")
    if not isinstance(registry_templates, list) or not registry_templates:
        raise RootfsBuildError("qualification registry must contain frozen templates")
    if any(
        not isinstance(template, dict) or template.get("nonIssuable") is not True
        for template in registry_templates
    ):
        raise RootfsBuildError("qualification registry templates must remain non-issuable")
    execution_release = parsed["execution-policy"].get("checkerInvocation")
    if not isinstance(execution_release, dict) or execution_release.get(
        "executionAllowedUnderThisRelease"
    ) is not False:
        raise RootfsBuildError("execution policy release must remain disabled")
    runtime = parsed["toolchain-identity"].get("runtimeVerification")
    if (
        not isinstance(runtime, dict)
        or runtime.get("productionByteProvenanceComplete") is not False
        or runtime.get("executionAllowedBeforeProvenanceClosure") is not False
    ):
        raise RootfsBuildError("toolchain identity must remain qualification-only")


def _read_pinned_executable(
    path_value: Any,
    expected_sha: Any,
    context: str,
) -> bytes:
    path_text = _text(path_value, f"{context} path")
    path = pathlib.Path(path_text)
    if (
        not path.is_absolute()
        or str(path) != path_text
        or os.path.realpath(path_text) != path_text
    ):
        raise RootfsBuildError(f"{context} path must be a normalized real absolute path")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path_text, flags)
    except OSError as exc:
        raise RootfsBuildError(f"cannot read pinned {context} executable") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or not metadata.st_mode & 0o111:
            raise RootfsBuildError(f"pinned {context} is not an executable regular file")
        if metadata.st_size <= 0 or metadata.st_size > 64 * 1024 * 1024:
            raise RootfsBuildError(f"pinned {context} executable size is invalid")
        chunks: list[bytes] = []
        remaining = metadata.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise RootfsBuildError(
                    f"pinned {context} executable was truncated while reading"
                )
            chunks.append(chunk)
            remaining -= len(chunk)
        raw = b"".join(chunks)
    finally:
        os.close(descriptor)
    if _hash_bytes(raw) != _sha(expected_sha, f"{context} SHA-256"):
        raise RootfsBuildError(f"pinned {context} bytes differ")
    return raw


def _read_absolute_executable(path_value: Any, expected_sha: Any) -> bytes:
    return _read_pinned_executable(path_value, expected_sha, "Ubuntu gpgv")


def _verify_inrelease(
    gpgv: bytes,
    keyring: bytes,
    inrelease: bytes,
    allowed_fingerprints: frozenset[str],
    snapshot: datetime.datetime,
) -> bytes:
    if not allowed_fingerprints:
        raise RootfsBuildError("Ubuntu signing fingerprint authority is empty")
    with tempfile.TemporaryDirectory(prefix="boole-ubuntu-signature.") as temporary_text:
        temporary = pathlib.Path(temporary_text)
        keyring_path = temporary / "archive-keyring.gpg"
        inrelease_path = temporary / "InRelease"
        payload_path = temporary / "Release"
        gpgv_path = temporary / "gpgv"
        home = temporary / "gnupg-home"
        home.mkdir(mode=0o700)
        keyring_path.write_bytes(keyring)
        inrelease_path.write_bytes(inrelease)
        gpgv_path.write_bytes(gpgv)
        gpgv_path.chmod(0o500)
        try:
            completed = subprocess.run(
                [
                    str(gpgv_path),
                    "--homedir",
                    str(home),
                    "--status-fd",
                    "1",
                    "--keyring",
                    str(keyring_path),
                    "--output",
                    str(payload_path),
                    str(inrelease_path),
                ],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
                timeout=30,
                env={"LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/bin"},
                close_fds=True,
            )
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise RootfsBuildError("Ubuntu InRelease verification could not run") from exc
        status = completed.stdout.decode("utf-8", errors="replace").splitlines()
        forbidden = {
            "BADSIG",
            "ERRSIG",
            "EXPSIG",
            "EXPKEYSIG",
            "REVKEYSIG",
            "NO_PUBKEY",
            "NODATA",
        }
        tags = {
            line.split()[1]
            for line in status
            if line.startswith("[GNUPG:] ") and len(line.split()) >= 2
        }
        valid: list[tuple[str, str, int]] = []
        for line in status:
            parts = line.split()
            if len(parts) >= 11 and parts[:2] == ["[GNUPG:]", "VALIDSIG"]:
                try:
                    signature_time = int(parts[4])
                except ValueError as exc:
                    raise RootfsBuildError("Ubuntu InRelease signature timestamp is invalid") from exc
                if parts[9] not in {"8", "9", "10"}:
                    raise RootfsBuildError("Ubuntu InRelease signature hash is too weak")
                primary = parts[11] if len(parts) >= 12 else parts[2]
                valid.append((parts[2], primary, signature_time))
        if completed.returncode != 0 or tags & forbidden or not valid:
            raise RootfsBuildError("Ubuntu InRelease signature is invalid")
        if len(valid) != 1 or not ({valid[0][0], valid[0][1]} & allowed_fingerprints):
            raise RootfsBuildError("Ubuntu InRelease signer is not trusted")
        if datetime.datetime.fromtimestamp(
            valid[0][2], tz=datetime.timezone.utc
        ) > snapshot:
            raise RootfsBuildError("Ubuntu InRelease signature postdates the snapshot")
        try:
            return payload_path.read_bytes()
        except OSError as exc:
            raise RootfsBuildError("gpgv did not emit a verified Release payload") from exc


def _release_fields(raw: bytes) -> tuple[dict[str, str], dict[str, tuple[str, int]]]:
    try:
        text = raw.decode("utf-8")
    except UnicodeError as exc:
        raise RootfsBuildError("verified Ubuntu Release payload is not UTF-8") from exc
    if "\r" in text or not text.endswith("\n"):
        raise RootfsBuildError("verified Ubuntu Release line endings differ")
    fields: dict[str, str] = {}
    sha_entries: dict[str, tuple[str, int]] = {}
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        if not line or line.startswith(" ") or ":" not in line:
            raise RootfsBuildError("verified Ubuntu Release field syntax differs")
        name, value = line.split(":", 1)
        if name in fields:
            raise RootfsBuildError(f"duplicate Ubuntu Release field: {name}")
        if name == "SHA256":
            index += 1
            while index < len(lines) and lines[index].startswith(" "):
                parts = lines[index].split()
                if len(parts) != 3:
                    raise RootfsBuildError("Ubuntu Release SHA256 entry syntax differs")
                digest = _sha(parts[0], "Ubuntu Release SHA256 digest")
                try:
                    size = int(parts[1])
                except ValueError as exc:
                    raise RootfsBuildError("Ubuntu Release SHA256 size is invalid") from exc
                path = _relative_path(parts[2], "Ubuntu Release SHA256 path")
                if size < 0 or path in sha_entries:
                    raise RootfsBuildError("Ubuntu Release SHA256 entry is invalid or duplicate")
                sha_entries[path] = (digest, size)
                index += 1
            fields[name] = "verified-table"
            continue
        continuations: list[str] = []
        index += 1
        while index < len(lines) and lines[index].startswith(" "):
            continuations.append(lines[index].strip())
            index += 1
        fields[name] = value.strip()
        if continuations:
            fields[name] += "\n" + "\n".join(continuations)
    if not sha_entries:
        raise RootfsBuildError("Ubuntu Release SHA256 table is absent")
    return fields, sha_entries


def _packages_payload(raw: bytes, path: str, limit: int) -> bytes:
    if path.endswith(".xz"):
        return _decompress_limited(raw, "Ubuntu Packages index", limit, compression="xz")
    if path.endswith(".gz"):
        return _decompress_limited(raw, "Ubuntu Packages index", limit, compression="gzip")
    if path.endswith("/Packages") or path == "Packages":
        if len(raw) > limit:
            raise RootfsBuildError("Ubuntu Packages index exceeds byte limit")
        return raw
    raise RootfsBuildError("Ubuntu Packages index compression is unsupported")


def _deb822_stanzas(raw: bytes) -> list[tuple[bytes, dict[str, str]]]:
    try:
        text = raw.decode("utf-8")
    except UnicodeError as exc:
        raise RootfsBuildError("Ubuntu Packages index is not UTF-8") from exc
    if "\r" in text or not text.endswith("\n"):
        raise RootfsBuildError("Ubuntu Packages index line endings differ")
    blocks = text.rstrip("\n").split("\n\n")
    result: list[tuple[bytes, dict[str, str]]] = []
    for block in blocks:
        if not block:
            raise RootfsBuildError("Ubuntu Packages index has an empty stanza")
        fields: dict[str, str] = {}
        current: Optional[str] = None
        for line in block.split("\n"):
            if line.startswith((" ", "\t")):
                if current is None:
                    raise RootfsBuildError("Ubuntu Packages continuation lacks a field")
                fields[current] += " " + line.strip()
                continue
            if ":" not in line:
                raise RootfsBuildError("Ubuntu Packages field syntax differs")
            name, value = line.split(":", 1)
            if not name or name in fields:
                raise RootfsBuildError("Ubuntu Packages field is empty or duplicate")
            fields[name] = value.lstrip()
            current = name
        result.append(((block + "\n").encode("utf-8"), fields))
    return result


def _split_dependency_groups(value: str) -> list[list[str]]:
    if not value:
        return []
    if re.search(r"\[[^\]]*\]|<[^<>=]+>", value):
        raise RootfsBuildError("architecture/profile-qualified runtime dependency is unsupported")
    groups: list[list[str]] = []
    for raw_group in value.split(","):
        alternatives = [item.strip() for item in raw_group.split("|")]
        if not alternatives or any(not item for item in alternatives):
            raise RootfsBuildError("Ubuntu dependency group syntax differs")
        groups.append(alternatives)
    return groups


_DEPENDENCY_RE = re.compile(
    r"^(?P<name>[a-z0-9][a-z0-9+.-]*)(?P<qualifier>:(?:any|native))?"
    r"(?:\s*\((?P<op><<|<=|=|>=|>>)\s*(?P<version>[^()\s]+)\))?$"
)


def _debian_version_parts(value: str) -> tuple[int, str, str]:
    epoch_text, separator, rest = value.partition(":")
    if separator:
        if not epoch_text.isdigit():
            raise RootfsBuildError("Debian version epoch is invalid")
        epoch = int(epoch_text)
    else:
        epoch = 0
        rest = epoch_text
    upstream, separator, revision = rest.rpartition("-")
    if not separator:
        upstream, revision = rest, "0"
    if not upstream:
        raise RootfsBuildError("Debian version is invalid")
    return epoch, upstream, revision


def _debian_non_digit_order(character: str) -> int:
    if character == "~":
        return -1
    if character == "":
        return 0
    if character.isalpha():
        return ord(character)
    return ord(character) + 256


def _debian_part_compare(left: str, right: str) -> int:
    while left or right:
        while (left and not left[0].isdigit()) or (right and not right[0].isdigit()):
            left_character = left[0] if left and not left[0].isdigit() else ""
            right_character = right[0] if right and not right[0].isdigit() else ""
            order = _debian_non_digit_order(left_character) - _debian_non_digit_order(
                right_character
            )
            if order:
                return -1 if order < 0 else 1
            if left_character:
                left = left[1:]
            if right_character:
                right = right[1:]
        left_digits = re.match(r"\d*", left).group(0)
        right_digits = re.match(r"\d*", right).group(0)
        left = left[len(left_digits) :]
        right = right[len(right_digits) :]
        left_number = left_digits.lstrip("0")
        right_number = right_digits.lstrip("0")
        if len(left_number) != len(right_number):
            return -1 if len(left_number) < len(right_number) else 1
        if left_number != right_number:
            return -1 if left_number < right_number else 1
    return 0


def _debian_version_compare(left: str, right: str) -> int:
    left_epoch, left_upstream, left_revision = _debian_version_parts(left)
    right_epoch, right_upstream, right_revision = _debian_version_parts(right)
    if left_epoch != right_epoch:
        return -1 if left_epoch < right_epoch else 1
    upstream = _debian_part_compare(left_upstream, right_upstream)
    return upstream or _debian_part_compare(left_revision, right_revision)


def _version_satisfies(actual: str, operator: Optional[str], expected: Optional[str]) -> bool:
    if operator is None:
        return True
    if expected is None:
        return False
    comparison = _debian_version_compare(actual, expected)
    return {
        "<<": comparison < 0,
        "<=": comparison <= 0,
        "=": comparison == 0,
        ">=": comparison >= 0,
        ">>": comparison > 0,
    }[operator]


def _provided_names(package: dict[str, Any]) -> dict[str, Optional[str]]:
    result: dict[str, Optional[str]] = {package["name"]: package["version"]}
    provides = package["provides"]
    if provides:
        for raw in provides.split(","):
            match = _DEPENDENCY_RE.fullmatch(raw.strip())
            if match is None or match.group("qualifier"):
                raise RootfsBuildError("Ubuntu Provides syntax differs")
            if match.group("op") not in (None, "="):
                raise RootfsBuildError("Ubuntu versioned Provides must use equality")
            result[match.group("name")] = match.group("version")
    return result


def _dependency_matches(expression: str, candidate: dict[str, Any]) -> bool:
    match = _DEPENDENCY_RE.fullmatch(expression)
    if match is None:
        raise RootfsBuildError(f"Ubuntu dependency alternative is unsupported: {expression}")
    if match.group("qualifier") is not None:
        raise RootfsBuildError("Ubuntu Multi-Arch dependency qualifier is not supported in v1")
    provided = _provided_names(candidate).get(match.group("name"))
    if match.group("name") not in _provided_names(candidate):
        return False
    if match.group("op") is not None and provided is None:
        return False
    return _version_satisfies(provided or candidate["version"], match.group("op"), match.group("version"))


def _snapshot_time(value: str) -> datetime.datetime:
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise RootfsBuildError("Ubuntu snapshot timestamp is invalid") from exc
    if parsed.tzinfo is None:
        raise RootfsBuildError("Ubuntu snapshot timestamp must include UTC offset")
    return parsed.astimezone(datetime.timezone.utc)


def _release_time(value: str, context: str) -> datetime.datetime:
    try:
        parsed = email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError) as exc:
        raise RootfsBuildError(f"{context} is invalid") from exc
    if parsed is None or parsed.tzinfo is None:
        raise RootfsBuildError(f"{context} lacks a timezone")
    return parsed.astimezone(datetime.timezone.utc)


def _release_window_allows(
    release: dict[str, str],
    snapshot: datetime.datetime,
    *,
    immutable_release_pocket: bool,
) -> bool:
    if "Date" not in release:
        raise RootfsBuildError("Ubuntu Release Date authority is absent")
    release_date = _release_time(release["Date"], "Ubuntu Release Date")
    valid_until = release.get("Valid-Until")
    if valid_until is not None:
        if not (
            release_date
            <= snapshot
            <= _release_time(valid_until, "Ubuntu Release Valid-Until")
        ):
            raise RootfsBuildError("Ubuntu snapshot is outside signed Release validity")
        return True
    if not immutable_release_pocket:
        raise RootfsBuildError("Ubuntu Release Valid-Until authority is absent")
    if not release_date <= snapshot <= release_date + datetime.timedelta(hours=24):
        raise RootfsBuildError("Ubuntu immutable release snapshot window differs")
    return True


def _source_identity(fields: dict[str, str]) -> tuple[str, str]:
    package_name = fields.get("Package")
    package_version = fields.get("Version")
    if not package_name or not package_version:
        raise RootfsBuildError("Ubuntu Packages stanza lacks Package/Version")
    source = fields.get("Source")
    if source is None:
        return package_name, package_version
    match = re.fullmatch(r"([^\s()]+)(?:\s+\(([^()\s]+)\))?", source)
    if match is None:
        raise RootfsBuildError("Ubuntu Packages Source field syntax differs")
    return match.group(1), match.group(2) or package_version


def _verify_ubuntu_source_closure(
    ubuntu: dict[str, Any],
    artifacts: dict[str, dict[str, Any]],
    artifact_store: pathlib.Path,
    recipe: dict[str, Any],
    allowed_fingerprints: frozenset[str],
) -> None:
    verification = ubuntu["verification"]
    gpgv = _read_absolute_executable(
        verification["gpgvPath"], verification["gpgvSha256"]
    )
    snapshot = _snapshot_time(ubuntu["snapshot"])
    stanzas_by_repository: dict[str, list[tuple[bytes, dict[str, str]]]] = {}
    repositories = {repo["id"]: repo for repo in ubuntu["repositories"]}
    for repo in ubuntu["repositories"]:
        keyring_artifact = artifacts[repo["keyringArtifactId"]]
        inrelease_artifact = artifacts[repo["inReleaseArtifactId"]]
        packages_artifact = artifacts[repo["packagesIndexArtifactId"]]
        keyring = _artifact_bytes(
            artifact_store, keyring_artifact["sha256"], keyring_artifact["sizeBytes"]
        )
        inrelease = _artifact_bytes(
            artifact_store,
            inrelease_artifact["sha256"],
            inrelease_artifact["sizeBytes"],
        )
        packages_raw = _artifact_bytes(
            artifact_store,
            packages_artifact["sha256"],
            packages_artifact["sizeBytes"],
        )
        release_payload = _verify_inrelease(
            gpgv, keyring, inrelease, allowed_fingerprints, snapshot
        )
        release, release_hashes = _release_fields(release_payload)
        if (
            release.get("Codename") != repo["suite"]
            or release.get("Suite") != repo["suite"]
        ):
            raise RootfsBuildError("Ubuntu Release codename differs")
        if repo["component"] not in release.get("Components", "").split():
            raise RootfsBuildError("Ubuntu Release component differs")
        if "amd64" not in release.get("Architectures", "").split():
            raise RootfsBuildError("Ubuntu Release architecture differs")
        _release_window_allows(
            release,
            snapshot,
            immutable_release_pocket=(repo["suite"] == "noble"),
        )
        signed_index = release_hashes.get(repo["packagesIndexPath"])
        if signed_index != (packages_artifact["sha256"], packages_artifact["sizeBytes"]):
            raise RootfsBuildError("Ubuntu Packages artifact differs from signed Release")
        packages_payload = _packages_payload(
            packages_raw, repo["packagesIndexPath"], recipe["maxTotalBytes"]
        )
        stanzas_by_repository[repo["id"]] = _deb822_stanzas(packages_payload)

    selected: dict[str, dict[str, Any]] = {}
    graph: dict[str, set[str]] = {}
    for package in ubuntu["packages"]:
        matches: list[tuple[bytes, dict[str, str]]] = []
        for stanza_raw, fields in stanzas_by_repository[package["repositoryId"]]:
            if (
                fields.get("Package") == package["name"]
                and fields.get("Version") == package["version"]
                and fields.get("Architecture") == package["architecture"]
                and fields.get("Filename") == package["poolPath"]
            ):
                matches.append((stanza_raw, fields))
        if len(matches) != 1:
            raise RootfsBuildError(f"Ubuntu package stanza identity is not unique: {package['packageId']}")
        stanza_raw, fields = matches[0]
        artifact = artifacts[package["artifactId"]]
        source_name, source_version = _source_identity(fields)
        try:
            stanza_size = int(fields.get("Size", ""))
        except ValueError as exc:
            raise RootfsBuildError("Ubuntu Packages Size field is invalid") from exc
        expected = {
            "sourceName": source_name,
            "sourceVersion": source_version,
            "depends": fields.get("Depends", ""),
            "preDepends": fields.get("Pre-Depends", ""),
            "provides": fields.get("Provides", ""),
            "multiArch": fields.get("Multi-Arch", ""),
            "essential": fields.get("Essential", "no") == "yes",
        }
        if any(package[key] != value for key, value in expected.items()):
            raise RootfsBuildError(f"Ubuntu package metadata differs: {package['packageId']}")
        if (
            package["indexStanzaSha256"] != _hash_bytes(stanza_raw)
            or fields.get("SHA256") != artifact["sha256"]
            or stanza_size != artifact["sizeBytes"]
        ):
            raise RootfsBuildError(f"Ubuntu package bytes differ from signed stanza: {package['packageId']}")
        selected[package["packageId"]] = package
        graph[package["packageId"]] = set()

    selected_seed_names = {
        selected[package_id]["name"]
        for package_id in ubuntu["seedPackageIds"]
        if package_id in selected
    }
    if (
        len(ubuntu["seedPackageIds"]) != len(ubuntu["seeds"])
        or selected_seed_names != set(ubuntu["seeds"])
    ):
        raise RootfsBuildError("Ubuntu seed package roots differ from declared seeds")

    for package in ubuntu["packages"]:
        expected_resolution_keys: set[tuple[str, int]] = set()
        groups_by_field: dict[str, list[list[str]]] = {}
        for field, key in (("Depends", "depends"), ("Pre-Depends", "preDepends")):
            groups = _split_dependency_groups(package[key])
            groups_by_field[field] = groups
            expected_resolution_keys.update((field, index) for index in range(len(groups)))
        actual_resolution_keys: set[tuple[str, int]] = set()
        for resolution in package["dependencyResolutions"]:
            field = resolution["field"]
            group_index = resolution["groupIndex"]
            alternative_index = resolution["alternativeIndex"]
            key = (field, group_index)
            if key in actual_resolution_keys or key not in expected_resolution_keys:
                raise RootfsBuildError("Ubuntu dependency resolution key differs")
            actual_resolution_keys.add(key)
            alternatives = groups_by_field[field][group_index]
            if (
                not isinstance(alternative_index, int)
                or isinstance(alternative_index, bool)
                or alternative_index < 0
                or alternative_index >= len(alternatives)
            ):
                raise RootfsBuildError("Ubuntu dependency alternative index differs")
            chosen_id = resolution["packageId"]
            chosen = selected.get(chosen_id)
            if chosen is None or not _dependency_matches(
                alternatives[alternative_index], chosen
            ):
                raise RootfsBuildError("Ubuntu dependency resolution does not satisfy expression")
            graph[package["packageId"]].add(chosen_id)
        if actual_resolution_keys != expected_resolution_keys:
            raise RootfsBuildError("Ubuntu dependency closure has an unresolved group")

    roots = set(ubuntu["seedPackageIds"])
    roots.update(
        package_id for package_id, package in selected.items() if package["essential"]
    )
    if not roots or not roots.issubset(selected):
        raise RootfsBuildError("Ubuntu seed/essential root set differs")
    reached: set[str] = set()
    pending = sorted(roots)
    while pending:
        package_id = pending.pop()
        if package_id in reached:
            continue
        reached.add(package_id)
        pending.extend(sorted(graph[package_id] - reached))
    if reached != set(selected):
        raise RootfsBuildError("Ubuntu package set contains unreachable extras")


def _validate_lock_shape(lock: Any) -> dict[str, Any]:
    value = _exact(
        lock,
        {
            "schema",
            "release",
            "activationAllowed",
            "platform",
            "authorityBindings",
            "artifacts",
            "ubuntu",
            "rust",
            "trackedFiles",
            "derivedEntries",
            "closureRoots",
            "buildRecipe",
        },
        "source lock",
    )
    if value["schema"] != LOCK_SCHEMA:
        raise RootfsBuildError("source lock schema differs")
    _text(value["release"], "source lock.release")
    if value["activationAllowed"] is not False:
        raise RootfsBuildError("source lock.activationAllowed must be literal false")
    platform = _exact(
        value["platform"],
        {"os", "ociArchitecture", "debArchitecture", "rustTarget"},
        "source lock.platform",
    )
    if platform != {
        "os": "linux",
        "ociArchitecture": "amd64",
        "debArchitecture": "amd64",
        "rustTarget": "x86_64-unknown-linux-gnu",
    }:
        raise RootfsBuildError("source lock platform differs")
    return value


def validate_source_lock(
    lock: Any,
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: Optional[pathlib.Path],
    *,
    require_complete: bool,
    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,
) -> dict[str, Any]:
    if canonical_json(lock) != lock_raw:
        raise RootfsBuildError("source lock is not canonical JSON")
    value = _validate_lock_shape(lock)
    artifacts = _artifact_table(value["artifacts"])

    bindings = value["authorityBindings"]
    if not isinstance(bindings, list):
        raise RootfsBuildError("source lock.authorityBindings must be a list")
    binding_ids: list[str] = []
    binding_by_source: dict[str, str] = {}
    binding_bytes_by_id: dict[str, bytes] = {}
    binding_source_by_id: dict[str, str] = {}
    for index, raw in enumerate(bindings):
        binding = _exact(raw, {"id", "sourcePath", "sha256"}, f"authorityBindings[{index}]")
        identifier = _plain_id(binding["id"], f"authorityBindings[{index}].id")
        source = _relative_path(binding["sourcePath"], f"authorityBindings[{index}].sourcePath")
        digest = _sha(binding["sha256"], f"authorityBindings[{index}].sha256")
        source_bytes = _open_relative_file(repository_root, source)
        if _hash_bytes(source_bytes) != digest:
            raise RootfsBuildError(f"authority binding bytes differ: {identifier}")
        binding_ids.append(identifier)
        if source in binding_by_source:
            raise RootfsBuildError(f"duplicate authority binding source: {source}")
        binding_by_source[source] = digest
        binding_bytes_by_id[identifier] = source_bytes
        binding_source_by_id[identifier] = source
    if binding_ids != sorted(set(binding_ids)):
        raise RootfsBuildError("authority bindings must be sorted by unique id")
    expected_binding_paths = {
        identifier: source for identifier, (source, _) in EXPECTED_AUTHORITY_FILES.items()
    }
    if binding_source_by_id != expected_binding_paths:
        raise RootfsBuildError("authority binding identity/source set differs")
    _inactive_authority_checks(binding_bytes_by_id)

    tracked = value["trackedFiles"]
    if not isinstance(tracked, list):
        raise RootfsBuildError("source lock.trackedFiles must be a list")
    tracked_paths: list[str] = []
    tracked_sources: dict[str, bytes] = {}
    expected_id_by_source = {
        source: identifier for identifier, (source, _) in EXPECTED_AUTHORITY_FILES.items()
    }
    for index, raw in enumerate(tracked):
        item = _exact(
            raw,
            {"sourcePath", "logicalPath", "sha256", "mode", "uid", "gid"},
            f"trackedFiles[{index}]",
        )
        source = _relative_path(item["sourcePath"], f"trackedFiles[{index}].sourcePath")
        logical = _logical_path(item["logicalPath"], f"trackedFiles[{index}].logicalPath")
        digest = _sha(item["sha256"], f"trackedFiles[{index}].sha256")
        _mode(item["mode"], f"trackedFiles[{index}].mode")
        if item["uid"] != 0 or item["gid"] != 0:
            raise RootfsBuildError("tracked rootfs files must be root:root")
        if binding_by_source.get(source) != digest:
            raise RootfsBuildError(f"tracked file lacks matching authority binding: {source}")
        authority_id = expected_id_by_source.get(source)
        if authority_id is None:
            raise RootfsBuildError(f"tracked authority source is unexpected: {source}")
        source_bytes = binding_bytes_by_id[authority_id]
        if _hash_bytes(source_bytes) != digest:
            raise RootfsBuildError(f"tracked file bytes differ: {source}")
        tracked_sources[source] = source_bytes
        tracked_paths.append(logical)
    if tracked_paths != sorted(set(tracked_paths)):
        raise RootfsBuildError("tracked files must be sorted by unique logicalPath")
    expected_tracked = {
        source: logical for source, logical in EXPECTED_AUTHORITY_FILES.values()
    }
    actual_tracked = {
        item["sourcePath"]: item["logicalPath"] for item in tracked
    }
    if actual_tracked != expected_tracked:
        raise RootfsBuildError("tracked authority source/logical path set differs")

    ubuntu = _exact(
        value["ubuntu"],
        {
            "snapshot",
            "verification",
            "repositories",
            "seeds",
            "seedPackageIds",
            "packages",
        },
        "source lock.ubuntu",
    )
    verification = _exact(
        ubuntu["verification"],
        {"gpgvPath", "gpgvSha256"},
        "source lock.ubuntu.verification",
    )
    if (verification["gpgvPath"] is None) != (verification["gpgvSha256"] is None):
        raise RootfsBuildError("Ubuntu gpgv path and digest must both be present or absent")
    if not isinstance(ubuntu["seeds"], list):
        raise RootfsBuildError("source lock.ubuntu.seeds must be a list")
    seeds = [_text(item, "source lock.ubuntu seed") for item in ubuntu["seeds"]]
    if seeds != sorted(set(seeds)):
        raise RootfsBuildError("source lock.ubuntu.seeds must be sorted and unique")
    if not isinstance(ubuntu["seedPackageIds"], list):
        raise RootfsBuildError("source lock.ubuntu.seedPackageIds must be a list")
    seed_package_ids = [
        _plain_id(item, "source lock.ubuntu seed package id")
        for item in ubuntu["seedPackageIds"]
    ]
    if seed_package_ids != sorted(set(seed_package_ids)):
        raise RootfsBuildError("source lock.ubuntu.seedPackageIds must be sorted and unique")
    if not isinstance(ubuntu["repositories"], list) or not isinstance(ubuntu["packages"], list):
        raise RootfsBuildError("source lock Ubuntu repositories/packages must be lists")
    repository_ids: list[str] = []
    referenced_artifacts: set[str] = set()
    for index, raw in enumerate(ubuntu["repositories"]):
        repo = _exact(
            raw,
            {
                "id",
                "snapshotBase",
                "suite",
                "component",
                "architecture",
                "keyringArtifactId",
                "inReleaseArtifactId",
                "packagesIndexArtifactId",
                "packagesIndexPath",
            },
            f"ubuntu.repositories[{index}]",
        )
        identifier = _plain_id(repo["id"], f"ubuntu.repositories[{index}].id")
        for key in ("snapshotBase", "suite", "component"):
            _text(repo[key], f"ubuntu.repositories[{index}].{key}")
        if trusted_ubuntu_fingerprints == UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS:
            if not repo["snapshotBase"].startswith("https://snapshot.ubuntu.com/ubuntu/"):
                raise RootfsBuildError("Ubuntu repository must use the pinned snapshot service")
            if repo["suite"] != "noble":
                raise RootfsBuildError("Ubuntu repository suite must remain noble")
        _relative_path(
            repo["packagesIndexPath"],
            f"ubuntu.repositories[{index}].packagesIndexPath",
        )
        if repo["architecture"] != "amd64":
            raise RootfsBuildError("Ubuntu repository architecture must be amd64")
        expected_kinds = (
            ("keyringArtifactId", "ubuntu-keyring"),
            ("inReleaseArtifactId", "ubuntu-inrelease"),
            ("packagesIndexArtifactId", "ubuntu-packages-index"),
        )
        for key, kind in expected_kinds:
            artifact_id = _plain_id(repo[key], f"ubuntu.repositories[{index}].{key}")
            if artifact_id not in artifacts or artifacts[artifact_id]["kind"] != kind:
                raise RootfsBuildError(f"Ubuntu repository {key} kind differs")
            referenced_artifacts.add(artifact_id)
        repository_ids.append(identifier)
    if repository_ids != sorted(set(repository_ids)):
        raise RootfsBuildError("Ubuntu repositories must be sorted by unique id")

    package_ids: list[str] = []
    package_names: set[str] = set()
    package_name_by_id: dict[str, str] = {}
    for index, raw in enumerate(ubuntu["packages"]):
        package = _exact(
            raw,
            {
                "packageId",
                "name",
                "version",
                "architecture",
                "sourceName",
                "sourceVersion",
                "repositoryId",
                "component",
                "poolPath",
                "artifactId",
                "indexStanzaSha256",
                "depends",
                "preDepends",
                "provides",
                "multiArch",
                "essential",
                "dependencyResolutions",
            },
            f"ubuntu.packages[{index}]",
        )
        package_id = _plain_id(package["packageId"], f"ubuntu.packages[{index}].packageId")
        name = _text(package["name"], f"ubuntu.packages[{index}].name")
        for key in ("version", "sourceName", "sourceVersion", "component"):
            _text(package[key], f"ubuntu.packages[{index}].{key}")
        _relative_path(package["poolPath"], f"ubuntu.packages[{index}].poolPath")
        if package["architecture"] not in {"amd64", "all"} or package["repositoryId"] not in repository_ids:
            raise RootfsBuildError("Ubuntu package architecture or repository differs")
        repository_component = next(
            repo["component"]
            for repo in ubuntu["repositories"]
            if repo["id"] == package["repositoryId"]
        )
        if package["component"] != repository_component:
            raise RootfsBuildError("Ubuntu package component differs from repository")
        _sha(package["indexStanzaSha256"], f"ubuntu.packages[{index}].indexStanzaSha256")
        artifact_id = _plain_id(package["artifactId"], f"ubuntu.packages[{index}].artifactId")
        if artifact_id not in artifacts or artifacts[artifact_id]["kind"] != "deb":
            raise RootfsBuildError("Ubuntu package artifact kind differs")
        referenced_artifacts.add(artifact_id)
        for key in ("depends", "preDepends", "provides", "multiArch"):
            if not isinstance(package[key], str):
                raise RootfsBuildError(f"ubuntu.packages[{index}].{key} must be a string")
        if not isinstance(package["essential"], bool):
            raise RootfsBuildError("Ubuntu package essential must be a boolean")
        if not isinstance(package["dependencyResolutions"], list):
            raise RootfsBuildError("dependencyResolutions must be a list")
        resolutions: list[tuple[str, int]] = []
        for resolution_index, resolution_raw in enumerate(package["dependencyResolutions"]):
            resolution = _exact(
                resolution_raw,
                {"field", "groupIndex", "alternativeIndex", "packageId"},
                f"ubuntu.packages[{index}].dependencyResolutions[{resolution_index}]",
            )
            if resolution["field"] not in {"Depends", "Pre-Depends"}:
                raise RootfsBuildError("dependency resolution field differs")
            for integer_key in ("groupIndex", "alternativeIndex"):
                integer = resolution[integer_key]
                if not isinstance(integer, int) or isinstance(integer, bool) or integer < 0:
                    raise RootfsBuildError("dependency resolution index is invalid")
            _plain_id(resolution["packageId"], "resolved dependency package id")
            resolutions.append((resolution["field"], resolution["groupIndex"]))
        if resolutions != sorted(set(resolutions)):
            raise RootfsBuildError("dependencyResolutions must be sorted and unique by group")
        package_ids.append(package_id)
        package_names.add(name)
        package_name_by_id[package_id] = name
    if package_ids != sorted(set(package_ids)):
        raise RootfsBuildError("Ubuntu packages must be sorted by unique packageId")
    if not set(seed_package_ids).issubset(set(package_ids)):
        raise RootfsBuildError("seedPackageIds references an unknown package")
    if seed_package_ids and (
        len(seed_package_ids) != len(seeds)
        or {package_name_by_id[package_id] for package_id in seed_package_ids}
        != set(seeds)
    ):
        raise RootfsBuildError("Ubuntu seed package roots differ from declared seeds")

    rust = _exact(
        value["rust"],
        {"rustcCommitHash", "cargoCommitHash", "installPrefix", "components"},
        "source lock.rust",
    )
    _git_commit(rust["rustcCommitHash"], "source lock.rust.rustcCommitHash")
    _git_commit(rust["cargoCommitHash"], "source lock.rust.cargoCommitHash")
    if rust["installPrefix"] != "/opt/boole/native-checker-toolchain":
        raise RootfsBuildError("Rust install prefix differs")
    if not isinstance(rust["components"], list):
        raise RootfsBuildError("Rust components must be a list")
    component_names: list[str] = []
    toolchain_identity = load_json_exact(
        binding_bytes_by_id["toolchain-identity"],
        "toolchain identity",
    )
    try:
        expected_rust_hashes = toolchain_identity["rust"]["linuxX8664ArtifactSha256"]
        expected_rustc_commit = toolchain_identity["rust"]["rustcCommitHash"]
        expected_cargo_commit = toolchain_identity["rust"]["cargoCommitHash"]
    except (KeyError, TypeError) as exc:
        raise RootfsBuildError("toolchain identity Rust mapping is missing") from exc
    if (
        rust["rustcCommitHash"] != expected_rustc_commit
        or rust["cargoCommitHash"] != expected_cargo_commit
    ):
        raise RootfsBuildError("Rust commit lock differs from toolchain identity")
    for index, raw in enumerate(rust["components"]):
        component = _exact(raw, {"name", "target", "artifactId"}, f"rust.components[{index}]")
        name = _text(component["name"], f"rust.components[{index}].name")
        if component["target"] != "x86_64-unknown-linux-gnu" or name not in RUST_ARCHIVE_FILENAMES:
            raise RootfsBuildError("Rust component name or target differs")
        artifact_id = _plain_id(component["artifactId"], f"rust.components[{index}].artifactId")
        if artifact_id not in artifacts or artifacts[artifact_id]["kind"] != "rust-dist":
            raise RootfsBuildError("Rust component artifact kind differs")
        expected_hash = expected_rust_hashes.get(RUST_ARCHIVE_FILENAMES[name])
        if artifacts[artifact_id]["sha256"] != expected_hash:
            raise RootfsBuildError("Rust archive differs from toolchain identity")
        referenced_artifacts.add(artifact_id)
        component_names.append(name)
    if component_names != sorted(RUST_ARCHIVE_FILENAMES):
        raise RootfsBuildError("Rust components must be the exact sorted frozen set")

    derived = value["derivedEntries"]
    if not isinstance(derived, list):
        raise RootfsBuildError("source lock.derivedEntries must be a list")
    derived_paths: list[str] = []
    for index, raw in enumerate(derived):
        entry = _exact(raw, {"logicalPath", "kind", "target", "mode", "uid", "gid"}, f"derivedEntries[{index}]")
        path = _logical_path(entry["logicalPath"], f"derivedEntries[{index}].logicalPath")
        if entry["kind"] != "symlink":
            raise RootfsBuildError("only explicit derived symlinks are supported")
        _text(entry["target"], f"derivedEntries[{index}].target")
        _mode(entry["mode"], f"derivedEntries[{index}].mode")
        if entry["uid"] != 0 or entry["gid"] != 0:
            raise RootfsBuildError("derived entries must be root:root")
        derived_paths.append(path)
    if derived_paths != sorted(set(derived_paths)):
        raise RootfsBuildError("derived entries must be sorted by unique logicalPath")

    closures = value["closureRoots"]
    if not isinstance(closures, list):
        raise RootfsBuildError("source lock.closureRoots must be a list")
    closure_names: list[str] = []
    for index, raw in enumerate(closures):
        closure = _exact(raw, {"name", "logicalRoots"}, f"closureRoots[{index}]")
        name = _text(closure["name"], f"closureRoots[{index}].name")
        if not isinstance(closure["logicalRoots"], list) or not closure["logicalRoots"]:
            raise RootfsBuildError("each provenance closure needs logical roots")
        roots = [_logical_path(item, "provenance logical root") for item in closure["logicalRoots"]]
        if roots != sorted(set(roots)):
            raise RootfsBuildError("provenance logical roots must be sorted and unique")
        if tuple(roots) != EXPECTED_PROVENANCE_CLOSURE_ROOTS.get(name):
            raise RootfsBuildError(f"provenance logical roots differ: {name}")
        closure_names.append(name)
    if tuple(closure_names) != REQUIRED_PROVENANCE_CLOSURES:
        raise RootfsBuildError("provenance closure names or order differ")

    recipe = _exact(
        value["buildRecipe"],
        {
            "builderSha256",
            "zstdPath",
            "zstdSha256",
            "baseImage",
            "network",
            "maintainerScripts",
            "canonicalMtime",
            "ownership",
            "output",
            "maxEntries",
            "maxFileBytes",
            "maxTotalBytes",
        },
        "source lock.buildRecipe",
    )
    expected_recipe = {
        "baseImage": "empty",
        "network": "forbidden",
        "maintainerScripts": "never-execute-or-copy",
        "canonicalMtime": 0,
        "ownership": "root:root-only",
        "output": "oci-image-layout-single-uncompressed-layer-v1",
        "maxEntries": 200000,
        "maxFileBytes": 536870912,
        "maxTotalBytes": 2147483648,
    }
    for key in ("canonicalMtime", "maxEntries", "maxFileBytes", "maxTotalBytes"):
        _integer(recipe[key], f"buildRecipe.{key}")
    for key, expected in expected_recipe.items():
        if recipe[key] != expected:
            raise RootfsBuildError(f"build recipe differs: {key}")

    complete = bool(
        ubuntu["snapshot"]
        and verification["gpgvPath"]
        and verification["gpgvSha256"]
        and repository_ids
        and package_ids
        and seed_package_ids
        and set(seeds).issubset(package_names)
        and all(item["sizeBytes"] is not None for item in artifacts.values())
        and recipe["builderSha256"] is not None
        and recipe["zstdPath"] is not None
        and recipe["zstdSha256"] is not None
    )
    if (recipe["zstdPath"] is None) != (recipe["zstdSha256"] is None):
        raise RootfsBuildError("build recipe zstd path and digest must both be present or absent")
    if complete:
        _text(ubuntu["snapshot"], "source lock.ubuntu.snapshot")
        builder_digest = _sha(recipe["builderSha256"], "buildRecipe.builderSha256")
        if builder_digest != BUILDER_SHA256:
            raise RootfsBuildError("builder recipe digest differs from actual builder bytes")
        _read_pinned_executable(recipe["zstdPath"], recipe["zstdSha256"], "zstd")
        if set(artifacts) != referenced_artifacts:
            raise RootfsBuildError("source lock contains unreferenced or missing artifacts")
        if artifact_store is None:
            raise RootfsBuildError("complete source lock needs an artifact store")
        for artifact in artifacts.values():
            if artifact["sizeBytes"] > recipe["maxTotalBytes"]:
                raise RootfsBuildError("source artifact exceeds the build byte ceiling")
            _artifact_bytes(artifact_store, artifact["sha256"], artifact["sizeBytes"])
        _verify_ubuntu_source_closure(
            ubuntu,
            artifacts,
            artifact_store,
            recipe,
            trusted_ubuntu_fingerprints,
        )
    else:
        if (
            recipe["builderSha256"] is not None
            or recipe["zstdPath"] is not None
            or recipe["zstdSha256"] is not None
        ):
            raise RootfsBuildError("incomplete source lock must not pin host build tools")
        if require_complete:
            raise RootfsBuildError("source closure is incomplete")
    return {
        "lock": value,
        "artifacts": artifacts,
        "trackedSourceBytes": tracked_sources,
        "sourceClosureComplete": complete,
        "authorityStatus": (
            "SOURCE-CLOSURE-COMPLETE-NOT-ACTIVATABLE"
            if complete
            else "UBUNTU-DEB-CLOSURE-NOT-RESOLVED"
        ),
    }


def _safe_archive_path(raw: str, context: str, *, allow_dot_prefix: bool) -> str:
    value = raw
    if allow_dot_prefix:
        while value.startswith("./"):
            value = value[2:]
    value = value.rstrip("/")
    if not value or value == ".":
        return ""
    return _relative_path(value, context)


def _safe_link(path: str, target: str, context: str) -> str:
    absolute = target.startswith("/")
    base = [] if absolute else list(pathlib.PurePosixPath(path).parent.parts)
    for part in pathlib.PurePosixPath(target).parts:
        if part in ("", ".", "/"):
            continue
        if part == "..":
            if not base:
                raise RootfsBuildError(f"{context} link escapes rootfs")
            base.pop()
        else:
            base.append(part)
    if not base:
        raise RootfsBuildError(f"{context} link resolves to root")
    return "/".join(base)


def _tar_entries(
    raw: bytes,
    context: str,
    recipe: dict[str, Any],
    *,
    allow_output_pax: bool = False,
) -> dict[str, dict[str, Any]]:
    entries: dict[str, dict[str, Any]] = {}
    total = 0
    member_count = 0
    try:
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:*") as archive:
            for member in archive:
                member_count += 1
                if member_count > recipe["maxEntries"]:
                    raise RootfsBuildError(f"{context} exceeds entry limit")
                path = _safe_archive_path(member.name, f"{context} member", allow_dot_prefix=True)
                if not path:
                    continue
                if path in FORBIDDEN_ROOTFS_PATHS:
                    raise RootfsBuildError(f"{context} forbidden rootfs path: {path}")
                if member.uid != 0 or member.gid != 0:
                    raise RootfsBuildError(f"{context} member is not root:root: {path}")
                if pathlib.PurePosixPath(path).name.startswith(".wh."):
                    raise RootfsBuildError(f"{context} whiteout member is forbidden: {path}")
                mode = member.mode & 0o7777
                if mode & (stat.S_ISUID | stat.S_ISGID):
                    raise RootfsBuildError(f"{context} member has set-id mode: {path}")
                if member.sparse is not None:
                    raise RootfsBuildError(f"{context} sparse member is forbidden: {path}")
                if member.pax_headers:
                    allowed_pax = {"path", "linkpath"}
                    if not allow_output_pax or not set(member.pax_headers).issubset(allowed_pax):
                        raise RootfsBuildError(f"{context} PAX metadata is forbidden: {path}")
                entry: dict[str, Any] = {"path": path, "mode": mode, "uid": 0, "gid": 0}
                if member.isdir():
                    entry["kind"] = "directory"
                elif member.isfile():
                    if member.size > recipe["maxFileBytes"]:
                        raise RootfsBuildError(f"{context} member exceeds file limit: {path}")
                    total += member.size
                    if total > recipe["maxTotalBytes"]:
                        raise RootfsBuildError(f"{context} exceeds total byte limit")
                    source = archive.extractfile(member)
                    if source is None:
                        raise RootfsBuildError(f"cannot read {context} member: {path}")
                    payload = source.read()
                    if len(payload) != member.size:
                        raise RootfsBuildError(f"{context} member size differs: {path}")
                    entry.update({"kind": "file", "raw": payload})
                elif member.issym():
                    target = _text(member.linkname, f"{context} symlink target")
                    entry.update(
                        {
                            "kind": "symlink",
                            "target": target,
                            "resolvedTarget": _safe_link(path, target, context),
                        }
                    )
                elif member.islnk():
                    target = _safe_archive_path(
                        member.linkname, f"{context} hardlink target", allow_dot_prefix=True
                    )
                    if not target:
                        raise RootfsBuildError(f"{context} hardlink target is empty")
                    entry.update({"kind": "hardlink", "target": target})
                else:
                    raise RootfsBuildError(f"{context} special member is forbidden: {path}")
                if path in entries:
                    raise RootfsBuildError(f"{context} duplicate path: {path}")
                entries[path] = entry
    except RootfsBuildError:
        raise
    except (tarfile.TarError, OSError, EOFError, lzma.LZMAError) as exc:
        raise RootfsBuildError(f"cannot parse {context}") from exc
    return entries


def _parse_ar(raw: bytes, context: str) -> dict[str, bytes]:
    if not raw.startswith(b"!<arch>\n"):
        raise RootfsBuildError(f"{context} is not an ar archive")
    offset = 8
    members: dict[str, bytes] = {}
    while offset < len(raw):
        if offset + 60 > len(raw):
            raise RootfsBuildError(f"{context} has a truncated ar header")
        header = raw[offset : offset + 60]
        offset += 60
        if header[58:60] != b"`\n":
            raise RootfsBuildError(f"{context} ar header trailer differs")
        try:
            name = header[:16].decode("ascii").strip().rstrip("/")
            size = int(header[48:58].decode("ascii").strip())
        except (UnicodeError, ValueError) as exc:
            raise RootfsBuildError(f"{context} ar header is invalid") from exc
        if not name or name in members or name.startswith("#1/") or name in ("/", "//"):
            raise RootfsBuildError(f"{context} ar member name is unsupported")
        if size < 0 or offset + size > len(raw):
            raise RootfsBuildError(f"{context} ar member size is invalid")
        members[name] = raw[offset : offset + size]
        offset += size + (size % 2)
    if offset != len(raw):
        raise RootfsBuildError(f"{context} ar padding differs")
    return members


def _decompress_limited(
    raw: bytes,
    context: str,
    limit: int,
    *,
    compression: str,
) -> bytes:
    output = io.BytesIO()
    try:
        if compression == "xz":
            compressed = raw
            while compressed:
                if not compressed.startswith(b"\xfd7zXZ\x00"):
                    raise RootfsBuildError(f"{context} has trailing compressed data")
                decompressor = lzma.LZMADecompressor(format=lzma.FORMAT_XZ)
                source = compressed
                while not decompressor.eof:
                    remaining = limit + 1 - output.tell()
                    if remaining <= 0:
                        raise RootfsBuildError(f"{context} exceeds decompression limit")
                    decoded = decompressor.decompress(source, max_length=remaining)
                    output.write(decoded)
                    if output.tell() > limit:
                        raise RootfsBuildError(f"{context} exceeds decompression limit")
                    source = b""
                    if decompressor.needs_input and not decompressor.eof:
                        raise RootfsBuildError(f"{context} compressed stream is truncated")
                compressed = decompressor.unused_data
                padding = len(compressed) - len(compressed.lstrip(b"\x00"))
                if padding % 4:
                    raise RootfsBuildError(f"{context} XZ stream padding differs")
                compressed = compressed[padding:]
        elif compression == "gzip":
            with gzip.GzipFile(fileobj=io.BytesIO(raw), mode="rb") as source:
                while True:
                    remaining = limit + 1 - output.tell()
                    if remaining <= 0:
                        raise RootfsBuildError(f"{context} exceeds decompression limit")
                    chunk = source.read(min(1024 * 1024, remaining))
                    if not chunk:
                        break
                    output.write(chunk)
        else:
            raise RootfsBuildError(f"{context} compression is unsupported")
    except RootfsBuildError:
        raise
    except (lzma.LZMAError, OSError, EOFError) as exc:
        raise RootfsBuildError(f"{context} decompression failed") from exc
    if output.tell() > limit:
        raise RootfsBuildError(f"{context} exceeds decompression limit")
    return output.getvalue()


def _zstd_decompress_limited(
    raw: bytes,
    context: str,
    limit: int,
    zstd_path: pathlib.Path,
    zstd: bytes,
) -> bytes:
    if not isinstance(limit, int) or isinstance(limit, bool) or limit <= 0:
        raise RootfsBuildError(f"{context} decompression limit is invalid")

    address_space_limit = max(
        256 * 1024 * 1024,
        min(2 * 1024 * 1024 * 1024, limit + 256 * 1024 * 1024),
    )

    def child_limits() -> None:
        resource.setrlimit(resource.RLIMIT_FSIZE, (limit, limit))
        resource.setrlimit(resource.RLIMIT_CPU, (30, 30))
        if sys.platform.startswith("linux"):
            resource.setrlimit(
                resource.RLIMIT_AS,
                (address_space_limit, address_space_limit),
            )

    with tempfile.TemporaryDirectory(prefix="boole-zstd.") as temporary_text:
        temporary = pathlib.Path(temporary_text)
        source = temporary / "source.zst"
        output = temporary / "output.tar"
        executable = temporary / "zstd"
        source.write_bytes(raw)
        executable.write_bytes(zstd)
        executable.chmod(0o500)
        if _hash_bytes(executable.read_bytes()) != _hash_bytes(zstd):
            raise RootfsBuildError(f"{context} private zstd copy differs")
        child_environment = {"LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/bin"}
        if sys.platform == "darwin":
            # Homebrew's zstd uses @rpath for libzstd. Execute the verified
            # private copy while retaining only its installation's library
            # directory; this qualification path is still explicitly
            # non-activatable until the Linux builder image closes shared-lib
            # provenance.
            child_environment["DYLD_LIBRARY_PATH"] = str(zstd_path.parent.parent / "lib")
        try:
            with output.open("wb") as output_file:
                completed = subprocess.run(
                    [
                        str(executable),
                        "--decompress",
                        "--stdout",
                        "--quiet",
                        "--no-progress",
                        "--",
                        str(source),
                    ],
                    stdin=subprocess.DEVNULL,
                    stdout=output_file,
                    stderr=subprocess.PIPE,
                    check=False,
                    timeout=30,
                    env=child_environment,
                    close_fds=True,
                    start_new_session=True,
                    preexec_fn=child_limits,
                )
        except subprocess.TimeoutExpired as exc:
            raise RootfsBuildError(f"{context} decompression timed out") from exc
        except OSError as exc:
            raise RootfsBuildError(f"{context} decompression failed") from exc
        size = output.stat().st_size
        if completed.returncode != 0:
            if size >= limit or completed.returncode in {
                -getattr(signal, "SIGXFSZ", 25),
                128 + getattr(signal, "SIGXFSZ", 25),
            }:
                raise RootfsBuildError(f"{context} exceeds decompression limit")
            raise RootfsBuildError(f"{context} decompression failed")
        return output.read_bytes()


def _deb_payload(raw: bytes, context: str, recipe: dict[str, Any]) -> dict[str, dict[str, Any]]:
    members = _parse_ar(raw, context)
    if members.get("debian-binary") != b"2.0\n":
        raise RootfsBuildError(f"{context} debian-binary differs")
    control = [name for name in members if name.startswith("control.tar")]
    data = [name for name in members if name.startswith("data.tar")]
    if len(control) != 1 or len(data) != 1:
        raise RootfsBuildError(f"{context} must contain one control and one data archive")
    allowed = {"debian-binary", control[0], data[0], "_gpgorigin"}
    if set(members) - allowed:
        raise RootfsBuildError(f"{context} contains an unexpected ar member")
    # control.tar is intentionally never parsed, executed, or copied.
    data_name = data[0]
    payload = members[data_name]
    try:
        if data_name.endswith(".xz"):
            payload = _decompress_limited(
                payload,
                f"{context} data archive",
                recipe["maxTotalBytes"],
                compression="xz",
            )
        elif data_name.endswith(".gz"):
            payload = _decompress_limited(
                payload,
                f"{context} data archive",
                recipe["maxTotalBytes"],
                compression="gzip",
            )
        elif data_name.endswith(".zst"):
            zstd = _read_pinned_executable(
                recipe["zstdPath"], recipe["zstdSha256"], "zstd"
            )
            payload = _zstd_decompress_limited(
                payload,
                f"{context} data archive",
                recipe["maxTotalBytes"],
                pathlib.Path(recipe["zstdPath"]),
                zstd,
            )
        elif data_name != "data.tar":
            raise RootfsBuildError(f"{context} data archive compression is unsupported")
    except (lzma.LZMAError, OSError) as exc:
        raise RootfsBuildError(f"{context} data archive decompression failed") from exc
    return _tar_entries(payload, f"{context} data", recipe)


def _rust_component_entries(
    raw: bytes,
    component: str,
    prefix: str,
    recipe: dict[str, Any],
) -> dict[str, dict[str, Any]]:
    archive = _tar_entries(raw, f"Rust component {component}", recipe)
    roots = sorted({path.split("/", 1)[0] for path in archive})
    if len(roots) != 1:
        raise RootfsBuildError(f"Rust component {component} top directory differs")
    top = roots[0]
    components_path = f"{top}/components"
    components_entry = archive.get(components_path)
    if components_entry is None or components_entry["kind"] != "file":
        raise RootfsBuildError(f"Rust component {component} components file is missing")
    try:
        component_names = components_entry["raw"].decode("utf-8").splitlines()
    except UnicodeError as exc:
        raise RootfsBuildError(f"Rust component {component} components file is invalid") from exc
    if component_names != [component]:
        raise RootfsBuildError(f"Rust component {component} archive identity differs")
    component_root = f"{top}/{component}"
    manifest_entry = archive.get(f"{component_root}/manifest.in")
    if manifest_entry is None or manifest_entry["kind"] != "file":
        raise RootfsBuildError(f"Rust component {component} manifest is missing")
    try:
        lines = manifest_entry["raw"].decode("utf-8").splitlines()
    except UnicodeError as exc:
        raise RootfsBuildError(f"Rust component {component} manifest is invalid") from exc
    result: dict[str, dict[str, Any]] = {}
    manifest_paths: set[str] = set()
    for line in lines:
        if not line.startswith("file:"):
            raise RootfsBuildError(f"Rust component {component} manifest directive is unsupported")
        relative = _relative_path(line[5:], f"Rust component {component} manifest path")
        if relative in manifest_paths:
            raise RootfsBuildError(f"Rust component {component} manifest path is duplicate")
        manifest_paths.add(relative)
        source_path = f"{component_root}/{relative}"
        source = archive.get(source_path)
        if source is None or source["kind"] not in {"file", "symlink"}:
            raise RootfsBuildError(f"Rust component {component} manifest source is missing: {relative}")
        logical = prefix.strip("/") + "/" + relative
        copied = dict(source)
        copied["path"] = logical
        if copied["kind"] == "symlink":
            copied["resolvedTarget"] = _safe_link(logical, copied["target"], component)
        result[logical] = copied
    return result


def _directory_entry(path: str, mode: int = 0o755) -> dict[str, Any]:
    return {"path": path, "kind": "directory", "mode": mode, "uid": 0, "gid": 0}


def _ensure_parents(entries: dict[str, dict[str, Any]]) -> None:
    additions: dict[str, dict[str, Any]] = {}
    for path in list(entries):
        parent = pathlib.PurePosixPath(path).parent
        while str(parent) != ".":
            parent_path = str(parent)
            existing = entries.get(parent_path) or additions.get(parent_path)
            if existing is not None and existing["kind"] != "directory":
                raise RootfsBuildError(f"rootfs parent collides with non-directory: {parent_path}")
            additions.setdefault(parent_path, _directory_entry(parent_path))
            parent = parent.parent
    for path, entry in additions.items():
        entries.setdefault(path, entry)


def _merge(target: dict[str, dict[str, Any]], incoming: dict[str, dict[str, Any]], context: str) -> None:
    for path, entry in incoming.items():
        existing = target.get(path)
        if existing is None:
            target[path] = entry
        elif existing != entry:
            if existing["kind"] == entry["kind"] == "directory" and existing["mode"] == entry["mode"]:
                continue
            raise RootfsBuildError(f"rootfs path collision differs ({context}): {path}")


def _verify_link_graph(entries: dict[str, dict[str, Any]]) -> None:
    for start, entry in entries.items():
        if entry["kind"] != "symlink":
            continue
        visited: set[str] = set()
        current = start
        while True:
            if current in visited:
                raise RootfsBuildError(f"rootfs symlink cycle is forbidden: {start}")
            visited.add(current)
            target = entries.get(entries[current]["resolvedTarget"])
            if target is None:
                raise RootfsBuildError(f"rootfs symlink target is absent: {current}")
            if target["kind"] != "symlink":
                break
            current = target["path"]


def _assemble_entries(
    validated: dict[str, Any], repository_root: pathlib.Path, artifact_store: pathlib.Path
) -> dict[str, dict[str, Any]]:
    lock = validated["lock"]
    artifacts = validated["artifacts"]
    tracked_source_bytes = validated["trackedSourceBytes"]
    recipe = lock["buildRecipe"]
    entries: dict[str, dict[str, Any]] = {}
    for package in lock["ubuntu"]["packages"]:
        artifact = artifacts[package["artifactId"]]
        raw = _artifact_bytes(artifact_store, artifact["sha256"], artifact["sizeBytes"])
        _merge(entries, _deb_payload(raw, f"package {package['packageId']}", recipe), package["packageId"])
    for component in lock["rust"]["components"]:
        artifact = artifacts[component["artifactId"]]
        raw = _artifact_bytes(artifact_store, artifact["sha256"], artifact["sizeBytes"])
        rust_entries = _rust_component_entries(
            raw,
            component["name"],
            lock["rust"]["installPrefix"],
            recipe,
        )
        _merge(entries, rust_entries, component["name"])
    for tracked in lock["trackedFiles"]:
        path = tracked["logicalPath"].lstrip("/")
        raw = tracked_source_bytes[tracked["sourcePath"]]
        if _hash_bytes(raw) != tracked["sha256"]:
            raise RootfsBuildError(f"tracked source snapshot differs: {tracked['sourcePath']}")
        entry = {
            "path": path,
            "kind": "file",
            "mode": int(tracked["mode"], 8),
            "uid": 0,
            "gid": 0,
            "raw": raw,
        }
        _merge(entries, {path: entry}, tracked["sourcePath"])
    for derived in lock["derivedEntries"]:
        path = derived["logicalPath"].lstrip("/")
        entry = {
            "path": path,
            "kind": "symlink",
            "mode": int(derived["mode"], 8),
            "uid": 0,
            "gid": 0,
            "target": derived["target"],
            "resolvedTarget": _safe_link(path, derived["target"], "derived entry"),
        }
        _merge(entries, {path: entry}, "derived entry")
    _ensure_parents(entries)
    for path, entry in entries.items():
        if entry["kind"] == "symlink" and entry["resolvedTarget"] not in entries:
            raise RootfsBuildError(f"rootfs symlink target is absent: {path}")
        if entry["kind"] == "hardlink":
            target = entries.get(entry["target"])
            if target is None or target["kind"] != "file":
                raise RootfsBuildError(f"rootfs hardlink target is absent: {path}")
            entries[path] = {
                "path": path,
                "kind": "file",
                "mode": target["mode"],
                "uid": 0,
                "gid": 0,
                "raw": target["raw"],
            }
    _verify_link_graph(entries)
    total_files = sum(1 for entry in entries.values() if entry["kind"] == "file")
    total_bytes = sum(
        len(entry["raw"]) for entry in entries.values() if entry["kind"] == "file"
    )
    if len(entries) > recipe["maxEntries"] or total_files > recipe["maxEntries"]:
        raise RootfsBuildError("assembled rootfs exceeds entry limit")
    if total_bytes > recipe["maxTotalBytes"]:
        raise RootfsBuildError("assembled rootfs exceeds total byte limit")
    return entries


def _entry_manifest(entries: dict[str, dict[str, Any]], closures: list[dict[str, Any]]) -> dict[str, Any]:
    manifest_entries: list[dict[str, Any]] = []
    for path in sorted(entries, key=lambda value: value.encode("utf-8")):
        source = entries[path]
        logical = "/" + path
        closure_names = [
            closure["name"]
            for closure in closures
            if any(
                logical == root or logical.startswith(root.rstrip("/") + "/")
                for root in closure["logicalRoots"]
            )
        ]
        item = {
            "logicalPath": logical,
            "kind": source["kind"],
            "mode": f"{source['mode']:04o}",
            "uid": 0,
            "gid": 0,
            "closures": closure_names,
        }
        if source["kind"] == "file":
            item.update({"sizeBytes": len(source["raw"]), "sha256": _hash_bytes(source["raw"])})
        elif source["kind"] in {"symlink", "hardlink"}:
            item["target"] = source["target"]
        manifest_entries.append(item)
    for required in REQUIRED_PROVENANCE_CLOSURES:
        if not any(required in entry["closures"] and entry["kind"] == "file" for entry in manifest_entries):
            raise RootfsBuildError(f"rootfs does not populate provenance closure: {required}")
    return {
        "schema": "boole.native-shadow.rootfs-content-manifest.v1",
        "activationAllowed": False,
        "productionByteProvenanceComplete": False,
        "entries": manifest_entries,
    }


def _layer_bytes(entries: dict[str, dict[str, Any]], mtime: int) -> bytes:
    if any(entry["kind"] == "hardlink" for entry in entries.values()):
        raise RootfsBuildError("output hardlinks must be materialized as regular files")
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.PAX_FORMAT) as archive:
        for path in sorted(entries, key=lambda value: value.encode("utf-8")):
            entry = entries[path]
            info = tarfile.TarInfo(path)
            info.mode = entry["mode"]
            info.uid = 0
            info.gid = 0
            info.uname = ""
            info.gname = ""
            info.mtime = mtime
            info.pax_headers = {}
            if entry["kind"] == "directory":
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            elif entry["kind"] == "file":
                info.type = tarfile.REGTYPE
                info.size = len(entry["raw"])
                archive.addfile(info, io.BytesIO(entry["raw"]))
            elif entry["kind"] == "symlink":
                info.type = tarfile.SYMTYPE
                info.linkname = entry["target"]
                archive.addfile(info)
            elif entry["kind"] == "hardlink":
                info.type = tarfile.LNKTYPE
                info.linkname = entry["target"]
                archive.addfile(info)
    return stream.getvalue()


def _descriptor(media_type: str, raw: bytes) -> dict[str, Any]:
    return {
        "mediaType": media_type,
        "digest": f"sha256:{_hash_bytes(raw)}",
        "size": len(raw),
    }


def _write_blob(layout: pathlib.Path, descriptor: dict[str, Any], raw: bytes) -> None:
    path = layout / "blobs" / "sha256" / descriptor["digest"].split(":", 1)[1]
    path.write_bytes(raw)
    path.chmod(0o444)


def _layout_snapshot(root: pathlib.Path) -> dict[str, tuple[str, int, bytes]]:
    if not root.is_dir() or root.is_symlink():
        raise RootfsBuildError("OCI layout root must be an existing real directory")
    result: dict[str, tuple[str, int, bytes]] = {}

    def visit(directory: pathlib.Path, prefix: str) -> None:
        try:
            children = sorted(os.scandir(directory), key=lambda item: item.name.encode("utf-8"))
        except OSError as exc:
            raise RootfsBuildError("cannot scan OCI layout") from exc
        for child in children:
            relative = f"{prefix}/{child.name}" if prefix else child.name
            try:
                metadata = child.stat(follow_symlinks=False)
            except OSError as exc:
                raise RootfsBuildError(f"cannot stat OCI layout entry: {relative}") from exc
            mode = stat.S_IMODE(metadata.st_mode)
            if stat.S_ISLNK(metadata.st_mode):
                raise RootfsBuildError(f"OCI layout symlink is forbidden: {relative}")
            if stat.S_ISDIR(metadata.st_mode):
                result[relative] = ("directory", mode, b"")
                visit(pathlib.Path(child.path), relative)
            elif stat.S_ISREG(metadata.st_mode):
                raw = _open_relative_file(root, relative)
                result[relative] = ("file", mode, raw)
            else:
                raise RootfsBuildError(f"OCI layout special entry is forbidden: {relative}")

    visit(root, "")
    return result


def directory_digest(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    for relative, (kind, mode, raw) in _layout_snapshot(root).items():
        digest.update(relative.encode("utf-8") + b"\0")
        digest.update(kind.encode("ascii") + b"\0")
        digest.update(f"{mode:04o}".encode("ascii") + b"\0")
        digest.update(raw)
    return digest.hexdigest()


def build_oci_layout(
    lock: Any,
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    output_dir: pathlib.Path,
    *,
    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,
) -> dict[str, Any]:
    validated = validate_source_lock(
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=True,
        trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,
    )
    entries = _assemble_entries(validated, repository_root, artifact_store)
    rootfs_manifest = _entry_manifest(entries, lock["closureRoots"])
    rootfs_manifest_raw = canonical_json(rootfs_manifest)
    layer_raw = _layer_bytes(entries, lock["buildRecipe"]["canonicalMtime"])
    layer = _descriptor(OCI_LAYER_MEDIA_TYPE, layer_raw)
    config_raw = canonical_json(
        {
            "architecture": "amd64",
            "config": {
                "Env": ["LANG=C", "LC_ALL=C", "TZ=UTC"],
                "Labels": {"org.boole.native-shadow.activation-allowed": "false"},
            },
            "os": "linux",
            "rootfs": {"diff_ids": [layer["digest"]], "type": "layers"},
        },
        compact=True,
    )
    config = _descriptor(OCI_CONFIG_MEDIA_TYPE, config_raw)
    manifest_raw = canonical_json(
        {
            "schemaVersion": 2,
            "mediaType": OCI_MANIFEST_MEDIA_TYPE,
            "config": config,
            "layers": [layer],
            "annotations": {"org.boole.native-shadow.activation-allowed": "false"},
        },
        compact=True,
    )
    manifest = _descriptor(OCI_MANIFEST_MEDIA_TYPE, manifest_raw)
    index_raw = canonical_json(
        {
            "schemaVersion": 2,
            "manifests": [
                {
                    **manifest,
                    "platform": {"os": "linux", "architecture": "amd64"},
                    "annotations": {"org.boole.native-shadow.activation-allowed": "false"},
                }
            ],
        },
        compact=True,
    )
    layout_raw = canonical_json({"imageLayoutVersion": "1.0.0"}, compact=True)
    receipt = {
        "schema": "boole.native-shadow.runtime-rootfs-build-receipt.v1",
        "authorityStatus": "BUILT-NOT-ACTIVATABLE",
        "activationAllowed": False,
        "productionByteProvenanceComplete": False,
        "sourceLockSha256": _hash_bytes(lock_raw),
        "builderSha256": BUILDER_SHA256,
        "rootfsContentManifestSha256": _hash_bytes(rootfs_manifest_raw),
        "rootfsContentManifestSizeBytes": len(rootfs_manifest_raw),
        "layerDigest": layer["digest"],
        "layerSizeBytes": layer["size"],
        "configDigest": config["digest"],
        "configSizeBytes": config["size"],
        "manifestDigest": manifest["digest"],
        "manifestSizeBytes": manifest["size"],
        "indexSha256": _hash_bytes(index_raw),
        "indexSizeBytes": len(index_raw),
        "layerCount": 1,
        "parentLayerCount": 0,
    }
    receipt_raw = canonical_json(receipt)
    if output_dir.exists() or output_dir.is_symlink():
        raise RootfsBuildError("output path already exists")
    if not output_dir.parent.is_dir() or output_dir.parent.is_symlink():
        raise RootfsBuildError("output parent must be an existing real directory")
    temporary = pathlib.Path(tempfile.mkdtemp(prefix=f".{output_dir.name}.", dir=output_dir.parent))
    try:
        (temporary / "blobs" / "sha256").mkdir(parents=True)
        temporary.chmod(0o755)
        (temporary / "blobs").chmod(0o755)
        (temporary / "blobs" / "sha256").chmod(0o755)
        _write_blob(temporary, layer, layer_raw)
        _write_blob(temporary, config, config_raw)
        _write_blob(temporary, manifest, manifest_raw)
        (temporary / "oci-layout").write_bytes(layout_raw)
        (temporary / "index.json").write_bytes(index_raw)
        (temporary / "ROOTFS-CONTENT-MANIFEST.json").write_bytes(rootfs_manifest_raw)
        (temporary / "BUILD-RECEIPT.json").write_bytes(receipt_raw)
        for path in (
            temporary / "oci-layout",
            temporary / "index.json",
            temporary / "ROOTFS-CONTENT-MANIFEST.json",
            temporary / "BUILD-RECEIPT.json",
        ):
            path.chmod(0o444)
        temporary.rename(output_dir)
    except Exception:
        shutil.rmtree(temporary, ignore_errors=True)
        raise
    return receipt


def _canonical_document(raw: bytes, context: str, *, compact: bool) -> Any:
    value = load_json_exact(raw, context)
    if canonical_json(value, compact=compact) != raw:
        raise RootfsBuildError(f"{context} is not canonical JSON")
    return value


def _oci_descriptor(
    value: Any,
    context: str,
    *,
    media_type: str,
    snapshot: dict[str, tuple[str, int, bytes]],
) -> tuple[dict[str, Any], bytes]:
    descriptor = _exact(value, {"mediaType", "digest", "size"}, context)
    if descriptor["mediaType"] != media_type:
        raise RootfsBuildError(f"{context} media type differs")
    digest = _text(descriptor["digest"], f"{context}.digest")
    if not digest.startswith("sha256:"):
        raise RootfsBuildError(f"{context} digest algorithm differs")
    hex_digest = _sha(digest[7:], f"{context}.digest")
    size = descriptor["size"]
    if not isinstance(size, int) or isinstance(size, bool) or size < 0:
        raise RootfsBuildError(f"{context}.size is invalid")
    path = f"blobs/sha256/{hex_digest}"
    entry = snapshot.get(path)
    if entry is None or entry[0] != "file":
        raise RootfsBuildError(f"{context} blob is absent")
    raw = entry[2]
    if len(raw) != size or _hash_bytes(raw) != hex_digest:
        raise RootfsBuildError(f"{context} blob bytes differ")
    return descriptor, raw


def _verify_oci_structure(
    layout: pathlib.Path,
    recipe: dict[str, Any],
    closures: list[dict[str, Any]],
) -> dict[str, Any]:
    snapshot = _layout_snapshot(layout)
    required_sidecars = {
        "BUILD-RECEIPT.json",
        "ROOTFS-CONTENT-MANIFEST.json",
        "index.json",
        "oci-layout",
    }
    for path in required_sidecars:
        if path not in snapshot or snapshot[path][0] != "file":
            raise RootfsBuildError(f"OCI layout sidecar is absent: {path}")
    if _canonical_document(snapshot["oci-layout"][2], "OCI layout marker", compact=True) != {
        "imageLayoutVersion": "1.0.0"
    }:
        raise RootfsBuildError("OCI layout marker differs")

    index = _exact(
        _canonical_document(snapshot["index.json"][2], "OCI index", compact=True),
        {"schemaVersion", "manifests"},
        "OCI index",
    )
    if index["schemaVersion"] != 2 or not isinstance(index["manifests"], list) or len(
        index["manifests"]
    ) != 1:
        raise RootfsBuildError("OCI index shape differs")
    index_descriptor = _exact(
        index["manifests"][0],
        {"mediaType", "digest", "size", "platform", "annotations"},
        "OCI index manifest descriptor",
    )
    if index_descriptor["platform"] != {"architecture": "amd64", "os": "linux"}:
        raise RootfsBuildError("OCI index platform differs")
    if index_descriptor["annotations"] != {
        "org.boole.native-shadow.activation-allowed": "false"
    }:
        raise RootfsBuildError("OCI index activation annotation differs")
    manifest_descriptor, manifest_raw = _oci_descriptor(
        {key: index_descriptor[key] for key in ("mediaType", "digest", "size")},
        "OCI manifest descriptor",
        media_type=OCI_MANIFEST_MEDIA_TYPE,
        snapshot=snapshot,
    )
    manifest = _exact(
        _canonical_document(manifest_raw, "OCI manifest", compact=True),
        {"schemaVersion", "mediaType", "config", "layers", "annotations"},
        "OCI manifest",
    )
    if manifest["schemaVersion"] != 2 or manifest["mediaType"] != OCI_MANIFEST_MEDIA_TYPE:
        raise RootfsBuildError("OCI manifest identity differs")
    if manifest["annotations"] != {
        "org.boole.native-shadow.activation-allowed": "false"
    }:
        raise RootfsBuildError("OCI manifest activation annotation differs")
    if not isinstance(manifest["layers"], list) or len(manifest["layers"]) != 1:
        raise RootfsBuildError("OCI manifest must have one layer")
    config_descriptor, config_raw = _oci_descriptor(
        manifest["config"],
        "OCI config descriptor",
        media_type=OCI_CONFIG_MEDIA_TYPE,
        snapshot=snapshot,
    )
    layer_descriptor, layer_raw = _oci_descriptor(
        manifest["layers"][0],
        "OCI layer descriptor",
        media_type=OCI_LAYER_MEDIA_TYPE,
        snapshot=snapshot,
    )
    expected_blobs = {
        f"blobs/sha256/{descriptor['digest'][7:]}"
        for descriptor in (manifest_descriptor, config_descriptor, layer_descriptor)
    }
    actual_blobs = {
        path for path, (kind, _, _) in snapshot.items()
        if kind == "file" and path.startswith("blobs/sha256/")
    }
    if actual_blobs != expected_blobs:
        raise RootfsBuildError("OCI blob set differs")

    config = _exact(
        _canonical_document(config_raw, "OCI config", compact=True),
        {"architecture", "config", "os", "rootfs"},
        "OCI config",
    )
    if config["architecture"] != "amd64" or config["os"] != "linux":
        raise RootfsBuildError("OCI config platform differs")
    if config["config"] != {
        "Env": ["LANG=C", "LC_ALL=C", "TZ=UTC"],
        "Labels": {"org.boole.native-shadow.activation-allowed": "false"},
    }:
        raise RootfsBuildError("OCI runtime config differs")
    if config["rootfs"] != {"diff_ids": [layer_descriptor["digest"]], "type": "layers"}:
        raise RootfsBuildError("OCI rootfs diff-id differs")

    content_raw = snapshot["ROOTFS-CONTENT-MANIFEST.json"][2]
    content = _exact(
        _canonical_document(content_raw, "rootfs content manifest", compact=False),
        {"schema", "activationAllowed", "productionByteProvenanceComplete", "entries"},
        "rootfs content manifest",
    )
    if (
        content["schema"] != "boole.native-shadow.rootfs-content-manifest.v1"
        or content["activationAllowed"] is not False
        or content["productionByteProvenanceComplete"] is not False
        or not isinstance(content["entries"], list)
    ):
        raise RootfsBuildError("rootfs content manifest authority differs")
    layer_entries = _tar_entries(
        layer_raw, "OCI layer", recipe, allow_output_pax=True
    )
    content_by_path: dict[str, dict[str, Any]] = {}
    for item in content["entries"]:
        if not isinstance(item, dict):
            raise RootfsBuildError("rootfs content entry must be an object")
        logical = _logical_path(item.get("logicalPath"), "rootfs content logicalPath")
        path = logical.lstrip("/")
        if path in content_by_path:
            raise RootfsBuildError("rootfs content manifest contains a duplicate path")
        content_by_path[path] = item
    if set(content_by_path) != set(layer_entries):
        raise RootfsBuildError("rootfs content manifest path set differs from layer")
    for path, layer_entry in layer_entries.items():
        item = content_by_path[path]
        expected_closures = [
            closure["name"]
            for closure in closures
            if any(
                "/" + path == root or ("/" + path).startswith(root.rstrip("/") + "/")
                for root in closure["logicalRoots"]
            )
        ]
        _integer(item.get("uid"), f"rootfs content uid: {path}")
        _integer(item.get("gid"), f"rootfs content gid: {path}")
        common = {
            "logicalPath": "/" + path,
            "kind": layer_entry["kind"],
            "mode": f"{layer_entry['mode']:04o}",
            "uid": 0,
            "gid": 0,
            "closures": expected_closures,
        }
        expected_keys = set(common)
        if layer_entry["kind"] == "file":
            common.update(
                {
                    "sizeBytes": len(layer_entry["raw"]),
                    "sha256": _hash_bytes(layer_entry["raw"]),
                }
            )
            expected_keys |= {"sizeBytes", "sha256"}
        elif layer_entry["kind"] in {"symlink", "hardlink"}:
            common["target"] = layer_entry["target"]
            expected_keys.add("target")
        if set(item) != expected_keys or item != common:
            raise RootfsBuildError(f"rootfs content entry differs from layer: {path}")

    receipt_raw = snapshot["BUILD-RECEIPT.json"][2]
    receipt = _exact(
        _canonical_document(receipt_raw, "rootfs build receipt", compact=False),
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
    _sha(receipt["sourceLockSha256"], "rootfs build receipt.sourceLockSha256")
    _sha(receipt["builderSha256"], "rootfs build receipt.builderSha256")
    if (
        receipt["schema"] != "boole.native-shadow.runtime-rootfs-build-receipt.v1"
        or receipt["authorityStatus"] != "BUILT-NOT-ACTIVATABLE"
        or receipt["activationAllowed"] is not False
        or receipt["productionByteProvenanceComplete"] is not False
        or receipt["builderSha256"] != BUILDER_SHA256
        or receipt["rootfsContentManifestSha256"] != _hash_bytes(content_raw)
        or receipt["rootfsContentManifestSizeBytes"] != len(content_raw)
        or receipt["layerDigest"] != layer_descriptor["digest"]
        or receipt["layerSizeBytes"] != layer_descriptor["size"]
        or receipt["configDigest"] != config_descriptor["digest"]
        or receipt["configSizeBytes"] != config_descriptor["size"]
        or receipt["manifestDigest"] != manifest_descriptor["digest"]
        or receipt["manifestSizeBytes"] != manifest_descriptor["size"]
        or receipt["indexSha256"] != _hash_bytes(snapshot["index.json"][2])
        or receipt["indexSizeBytes"] != len(snapshot["index.json"][2])
        or receipt["layerCount"] != 1
        or receipt["parentLayerCount"] != 0
    ):
        raise RootfsBuildError("rootfs build receipt differs from OCI bytes")
    return receipt


def verify_oci_layout(
    lock: Any,
    lock_raw: bytes,
    repository_root: pathlib.Path,
    artifact_store: pathlib.Path,
    layout: pathlib.Path,
    *,
    trusted_ubuntu_fingerprints: frozenset[str] = UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS,
) -> dict[str, Any]:
    validated = validate_source_lock(
        lock,
        lock_raw,
        repository_root,
        artifact_store,
        require_complete=True,
        trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,
    )
    independent_receipt = _verify_oci_structure(
        layout,
        validated["lock"]["buildRecipe"],
        validated["lock"]["closureRoots"],
    )
    with tempfile.TemporaryDirectory(prefix="boole-rootfs-verify.", dir=layout.parent) as temporary:
        rebuilt = pathlib.Path(temporary) / "rebuilt"
        receipt = build_oci_layout(
            lock,
            lock_raw,
            repository_root,
            artifact_store,
            rebuilt,
            trusted_ubuntu_fingerprints=trusted_ubuntu_fingerprints,
        )
        if directory_digest(layout) != directory_digest(rebuilt):
            raise RootfsBuildError("OCI layout differs from deterministic rebuild")
        if _layout_snapshot(layout) != _layout_snapshot(rebuilt):
            raise RootfsBuildError("OCI layout differs from deterministic rebuild")
        if receipt != independent_receipt:
            raise RootfsBuildError("OCI independent receipt differs from rebuild")
        return independent_receipt


def _load(path: pathlib.Path, context: str, *, canonical: bool) -> tuple[Any, bytes]:
    try:
        raw = path.read_bytes()
    except OSError as exc:
        raise RootfsBuildError(f"cannot read {context}: {path}") from exc
    return load_json_exact(raw, context, require_canonical=canonical), raw


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("validate-lock", "build", "verify"):
        sub = subparsers.add_parser(command)
        sub.add_argument("--lock", required=True, type=pathlib.Path)
        sub.add_argument("--artifact-store", required=True, type=pathlib.Path)
        sub.add_argument("--repo-root", required=True, type=pathlib.Path)
        if command == "build":
            sub.add_argument("--output", required=True, type=pathlib.Path)
        elif command == "verify":
            sub.add_argument("--layout", required=True, type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        lock, raw = _load(args.lock, "source lock", canonical=True)
        if args.command == "validate-lock":
            result = validate_source_lock(
                lock,
                raw,
                args.repo_root,
                args.artifact_store,
                require_complete=False,
            )
            print(
                json.dumps(
                    {
                        "sourceClosureComplete": result["sourceClosureComplete"],
                        "authorityStatus": result["authorityStatus"],
                        "activationAllowed": False,
                    },
                    sort_keys=True,
                    separators=(",", ":"),
                )
            )
        elif args.command == "build":
            receipt = build_oci_layout(
                lock, raw, args.repo_root, args.artifact_store, args.output
            )
            print(json.dumps(receipt, sort_keys=True, separators=(",", ":")))
        else:
            receipt = verify_oci_layout(
                lock, raw, args.repo_root, args.artifact_store, args.layout
            )
            print(json.dumps(receipt, sort_keys=True, separators=(",", ":")))
        return 0
    except (RootfsBuildError, OSError) as exc:
        print(f"native-shadow-rootfs-builder: FAIL: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
