#!/usr/bin/env python3
"""Acquire the frozen ARM64 package closure in baseline-then-delta order.

This module deliberately handles opaque package bytes only.  It never invokes
``apt``, ``dpkg`` or package maintainer scripts and it grants no boot or
activation authority.
"""

from __future__ import annotations

import argparse
import hashlib
import http.client
import contextlib
import fcntl
import json
import os
import pathlib
import re
import secrets
import ssl
import stat
import sys
import types
import urllib.parse
from typing import Any, Callable, Iterable, Optional


class PayloadAcquisitionError(RuntimeError):
    """The frozen payload contract or a downloaded byte stream differs."""


StreamFactory = Callable[[dict[str, object]], Iterable[bytes]]
ReplayCandidate = Callable[[], bool]
Preflight = Callable[[int], None]
SNAPSHOT_BASE = "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z"
LOCK_NAME = ".arm64-payload-acquisition.lock"
PLAN_SCHEMA = "boole.native-shadow.boot-rootfs-payload-acquisition-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-rootfs-payload-acquisition-result.arm64.v1"
RESULT_STATUS = "PACKAGE-PAYLOADS-ACQUIRED-VERIFIED-NOT-BOOT-AUTHORITY"
EXPECTED_PLAN_SHA256 = "f6589fe619e83531d9e76c998dbd5ab33436595e307579ccfecd2de644069fd1"
SCRIPT_ROOT = pathlib.Path(__file__).resolve().parents[1]
_PLAN_DIGEST_LITERAL = re.compile(
    rb'(?m)^(EXPECTED_PLAN_SHA256 = ")[0-9a-f]{64}("\s*)$'
)


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise PayloadAcquisitionError(f"{context} is not lowercase SHA-256")
    return value


def _size(value: Any, context: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise PayloadAcquisitionError(f"{context} size is invalid")
    return value


def canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False)
        + "\n"
    ).encode("utf-8")


def payload_acquirer_authority_sha256(raw: bytes) -> str:
    normalized, replacements = _PLAN_DIGEST_LITERAL.subn(
        rb"\g<1>" + b"0" * 64 + rb"\g<2>", raw
    )
    if replacements != 1:
        raise PayloadAcquisitionError("payload acquirer plan digest literal differs")
    return hashlib.sha256(normalized).hexdigest()


def _duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise PayloadAcquisitionError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _canonical_object(raw: bytes, context: str) -> dict[str, Any]:
    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=_duplicate_keys)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PayloadAcquisitionError(f"{context} is not canonical JSON") from exc
    if not isinstance(value, dict) or canonical_json(value) != raw:
        raise PayloadAcquisitionError(f"{context} is not byte-exact canonical JSON")
    return value


def _read_regular_nofollow_stable(path: pathlib.Path, context: str) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(os.fspath(path), flags)
    except OSError as exc:
        raise PayloadAcquisitionError(f"cannot open {context} without following links") from exc
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise PayloadAcquisitionError(f"{context} is not a regular file")
        actual = bytearray()
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise PayloadAcquisitionError(f"{context} was truncated")
            actual.extend(chunk)
            remaining -= len(chunk)
        after = os.fstat(descriptor)
        if (
            before.st_dev,
            before.st_ino,
            before.st_size,
            before.st_mtime_ns,
            before.st_ctime_ns,
        ) != (
            after.st_dev,
            after.st_ino,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        ):
            raise PayloadAcquisitionError(f"{context} changed while reading")
        return bytes(actual)
    finally:
        os.close(descriptor)


def _pin(value: Any, context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != {"sha256", "sizeBytes"}:
        raise PayloadAcquisitionError(f"{context} pin keys differ")
    _digest(value["sha256"], f"{context}.sha256")
    _size(value["sizeBytes"], f"{context}.sizeBytes")
    return value


def _read_pinned(path: pathlib.Path, pin: dict[str, Any], context: str) -> bytes:
    expected = _pin(pin, context)
    raw = _read_regular_nofollow_stable(path, context)
    if len(raw) != expected["sizeBytes"] or hashlib.sha256(raw).hexdigest() != expected["sha256"]:
        raise PayloadAcquisitionError(f"{context} bytes differ")
    return raw


def _spec(value: Any) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != {
        "artifactId",
        "sha256",
        "sizeBytes",
        "url",
    }:
        raise PayloadAcquisitionError("payload spec keys differ")
    identifier = value["artifactId"]
    url = value["url"]
    if not isinstance(identifier, str) or not identifier:
        raise PayloadAcquisitionError("payload artifactId is invalid")
    if not isinstance(url, str) or not url:
        raise PayloadAcquisitionError("payload URL is invalid")
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname != "snapshot.ubuntu.com"
        or parsed.port not in (None, 443)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or not parsed.path.startswith("/ubuntu/20240425T160000Z/")
        or "\\" in parsed.path
        or "%" in parsed.path
        or "//" in parsed.path
        or ".." in pathlib.PurePosixPath(parsed.path).parts
    ):
        raise PayloadAcquisitionError("payload URL violates the frozen snapshot policy")
    _digest(value["sha256"], "payload")
    _size(value["sizeBytes"], "payload")
    return value


def _open_directory_nofollow(path: pathlib.Path, *, create: bool) -> int:
    flags = (
        os.O_RDONLY
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    absolute = pathlib.Path(os.path.abspath(os.fspath(path)))
    if sys_platform_is_darwin_alias(absolute):
        absolute = pathlib.Path("/private").joinpath(*absolute.parts[1:])
    descriptor = os.open(os.path.sep, flags)
    try:
        for component in absolute.parts[1:]:
            if component in {"", ".", "..", os.path.sep}:
                raise PayloadAcquisitionError("CAS path is not normalized")
            if create:
                try:
                    os.mkdir(component, 0o755, dir_fd=descriptor)
                except FileExistsError:
                    pass
            child = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            raise PayloadAcquisitionError("CAS path is not a directory")
        return descriptor
    except (OSError, PayloadAcquisitionError) as exc:
        os.close(descriptor)
        if isinstance(exc, PayloadAcquisitionError):
            raise
        raise PayloadAcquisitionError("CAS path is absent, unsafe, or a symlink") from exc


def sys_platform_is_darwin_alias(path: pathlib.Path) -> bool:
    """Normalize only Apple's fixed root aliases, never caller descendants."""

    import sys

    return (
        sys.platform == "darwin"
        and len(path.parts) > 1
        and path.parts[1] in {"etc", "tmp", "var"}
    )


def _open_sha_directory(cas: pathlib.Path, *, create: bool) -> int:
    root = _open_directory_nofollow(cas, create=create)
    flags = (
        os.O_RDONLY
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    try:
        root_metadata = os.fstat(root)
        if (
            root_metadata.st_uid != os.getuid()
            or stat.S_IMODE(root_metadata.st_mode) & 0o022
        ):
            raise PayloadAcquisitionError("CAS root owner or mode differs")
        if create:
            try:
                os.mkdir("sha256", 0o755, dir_fd=root)
            except FileExistsError:
                pass
        try:
            result = os.open("sha256", flags, dir_fd=root)
        except OSError as exc:
            raise PayloadAcquisitionError("CAS sha256 path is unsafe") from exc
        if not stat.S_ISDIR(os.fstat(result).st_mode):
            os.close(result)
            raise PayloadAcquisitionError("CAS sha256 path is not a directory")
        sha_metadata = os.fstat(result)
        if sha_metadata.st_uid != os.getuid() or stat.S_IMODE(sha_metadata.st_mode) & 0o022:
            os.close(result)
            raise PayloadAcquisitionError("CAS sha256 owner or mode differs")
        return result
    finally:
        os.close(root)


def _verify_name(directory: int, spec: dict[str, object]) -> bool:
    digest = str(spec["sha256"])
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(digest, flags, dir_fd=directory)
    except FileNotFoundError:
        return False
    except OSError as exc:
        raise PayloadAcquisitionError("CAS artifact cannot be opened safely") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise PayloadAcquisitionError("CAS artifact is not a regular file")
        if (
            stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
        ):
            raise PayloadAcquisitionError("CAS artifact owner, link count, or mode differs")
        expected_size = int(spec["sizeBytes"])
        if metadata.st_size != expected_size:
            raise PayloadAcquisitionError("CAS artifact size differs")
        actual = hashlib.sha256()
        remaining = expected_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise PayloadAcquisitionError("CAS artifact was truncated")
            actual.update(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1) or actual.hexdigest() != digest:
            raise PayloadAcquisitionError("CAS artifact digest differs")
        return True
    finally:
        os.close(descriptor)


def _store_stream(
    directory: int,
    spec: dict[str, object],
    chunks: Iterable[bytes],
) -> None:
    digest = str(spec["sha256"])
    expected_size = int(spec["sizeBytes"])
    temporary = f".{digest}.{os.getpid()}.{secrets.token_hex(16)}"
    flags = (
        os.O_WRONLY
        | os.O_CREAT
        | os.O_EXCL
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    descriptor = -1
    try:
        descriptor = os.open(temporary, flags, 0o600, dir_fd=directory)
        actual = hashlib.sha256()
        observed = 0
        for chunk in chunks:
            if not isinstance(chunk, bytes) or not chunk:
                raise PayloadAcquisitionError("payload stream chunk is invalid")
            observed += len(chunk)
            if observed > expected_size:
                raise PayloadAcquisitionError("payload exceeds frozen size")
            view = memoryview(chunk)
            while view:
                written = os.write(descriptor, view)
                if written <= 0:
                    raise PayloadAcquisitionError("payload CAS write stopped")
                view = view[written:]
            actual.update(chunk)
        if observed != expected_size or actual.hexdigest() != digest:
            raise PayloadAcquisitionError("payload differs from frozen digest/size")
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = -1
        try:
            os.link(
                temporary,
                digest,
                src_dir_fd=directory,
                dst_dir_fd=directory,
                follow_symlinks=False,
            )
        except FileExistsError:
            if not _verify_name(directory, spec):
                raise PayloadAcquisitionError("concurrent CAS publication differs")
        os.fsync(directory)
    except OSError as exc:
        raise PayloadAcquisitionError("payload CAS publication failed") from exc
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        try:
            os.unlink(temporary, dir_fd=directory)
            os.fsync(directory)
        except FileNotFoundError:
            pass


def _ordered_unique(specs: Iterable[dict[str, object]]) -> list[dict[str, object]]:
    result = [_spec(value) for value in specs]
    identifiers = [str(value["artifactId"]) for value in result]
    digests = [str(value["sha256"]) for value in result]
    if len(identifiers) != len(set(identifiers)) or len(digests) != len(set(digests)):
        raise PayloadAcquisitionError("payload identity is duplicated")
    return sorted(result, key=lambda value: str(value["artifactId"]))


def snapshot_https_stream(
    spec: dict[str, object],
    *,
    connection_factory: Any = http.client.HTTPSConnection,
    context_factory: Callable[[], ssl.SSLContext] = ssl.create_default_context,
) -> Iterable[bytes]:
    """Yield one exact snapshot response without proxy, redirect, retry or Range."""

    frozen = _spec(spec)
    parsed = urllib.parse.urlsplit(str(frozen["url"]))
    expected_size = int(frozen["sizeBytes"])
    context = context_factory()
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.verify_mode = ssl.CERT_REQUIRED
    context.check_hostname = True
    connection = connection_factory(
        "snapshot.ubuntu.com",
        443,
        timeout=60,
        context=context,
    )
    observed = 0
    try:
        connection.putrequest(
            "GET",
            parsed.path,
            skip_host=True,
            skip_accept_encoding=True,
        )
        connection.putheader("Host", "snapshot.ubuntu.com")
        connection.putheader("Accept-Encoding", "identity")
        connection.putheader("Connection", "close")
        connection.putheader("User-Agent", "boole-arm64-payload-acquirer-v1")
        connection.endheaders()
        response = connection.getresponse()
        if response.status != 200:
            raise PayloadAcquisitionError("snapshot response status is not 200")
        encoding = response.getheader("Content-Encoding")
        if encoding not in (None, "identity"):
            raise PayloadAcquisitionError("snapshot response encoding is forbidden")
        length = response.getheader("Content-Length")
        if length != str(expected_size):
            raise PayloadAcquisitionError("snapshot Content-Length differs")
        while True:
            chunk = response.read(min(1024 * 1024, expected_size - observed + 1))
            if not chunk:
                break
            if not isinstance(chunk, bytes):
                raise PayloadAcquisitionError("snapshot response chunk is invalid")
            observed += len(chunk)
            if observed > expected_size:
                raise PayloadAcquisitionError("snapshot response exceeds frozen size")
            yield chunk
        if observed != expected_size:
            raise PayloadAcquisitionError("snapshot response is shorter than frozen size")
    except PayloadAcquisitionError:
        raise
    except (OSError, TimeoutError, http.client.HTTPException) as exc:
        raise PayloadAcquisitionError("snapshot request failed") from exc
    finally:
        connection.close()


@contextlib.contextmanager
def _cas_lock(cas: pathlib.Path) -> Iterable[None]:
    root = _open_directory_nofollow(cas, create=True)
    flags = (
        os.O_RDWR
        | os.O_CREAT
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    try:
        try:
            descriptor = os.open(LOCK_NAME, flags, 0o600, dir_fd=root)
        except OSError as exc:
            raise PayloadAcquisitionError("CAS acquisition lock is unsafe") from exc
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
        ):
            os.close(descriptor)
            raise PayloadAcquisitionError("CAS acquisition lock owner, link count, or mode differs")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except (BlockingIOError, OSError) as exc:
            os.close(descriptor)
            raise PayloadAcquisitionError("CAS acquisition is busy") from exc
        os.fsync(root)
    finally:
        os.close(root)
    try:
        yield
    finally:
        fcntl.flock(descriptor, fcntl.LOCK_UN)
        os.close(descriptor)


def acquire_two_stage_payloads(
    cas: pathlib.Path,
    baseline_specs: Iterable[dict[str, object]],
    delta_specs: Iterable[dict[str, object]],
    *,
    stream_factory: StreamFactory,
) -> dict[str, object]:
    """Acquire baseline fully before opening any successor-delta stream."""

    baseline = _ordered_unique(baseline_specs)
    delta = _ordered_unique(delta_specs)
    if {str(row["artifactId"]) for row in baseline} & {
        str(row["artifactId"]) for row in delta
    } or {str(row["sha256"]) for row in baseline} & {
        str(row["sha256"]) for row in delta
    }:
        raise PayloadAcquisitionError("baseline and delta overlap")
    with _cas_lock(cas):
        directory = _open_sha_directory(cas, create=True)
        try:
            summary = _empty_summary()
            _acquire_ordered_locked(directory, "baseline", baseline, stream_factory, summary)
            _acquire_ordered_locked(directory, "delta", delta, stream_factory, summary)
            return summary
        finally:
            os.close(directory)


def _empty_summary() -> dict[str, object]:
    return {
        "metadataFetched": 0,
        "metadataReused": 0,
        "baselineFetched": 0,
        "baselineReused": 0,
        "deltaFetched": 0,
        "deltaReused": 0,
        "fetchedBytes": 0,
    }


def _acquire_ordered_locked(
    directory: int,
    label: str,
    specs: list[dict[str, object]],
    stream_factory: StreamFactory,
    summary: dict[str, object],
) -> None:
    for spec in specs:
        if _verify_name(directory, spec):
            summary[f"{label}Reused"] = int(summary[f"{label}Reused"]) + 1
            continue
        _store_stream(directory, spec, stream_factory(spec))
        if not _verify_name(directory, spec):
            raise PayloadAcquisitionError("published payload is absent")
        summary[f"{label}Fetched"] = int(summary[f"{label}Fetched"]) + 1
        summary["fetchedBytes"] = int(summary["fetchedBytes"]) + int(
            spec["sizeBytes"]
        )
    if not all(_verify_name(directory, spec) for spec in specs):
        raise PayloadAcquisitionError(f"{label} stage is incomplete")


def acquire_after_signed_replay(
    cas: pathlib.Path,
    metadata_spec: dict[str, object],
    baseline_specs: Iterable[dict[str, object]],
    delta_specs: Iterable[dict[str, object]],
    *,
    replay_candidate: ReplayCandidate,
    stream_factory: StreamFactory,
    preflight: Optional[Preflight] = None,
) -> dict[str, object]:
    """Acquire one index, replay authority, then baseline and successor delta."""

    metadata = _ordered_unique([metadata_spec])
    baseline = _ordered_unique(baseline_specs)
    delta = _ordered_unique(delta_specs)
    identity_sets = [
        {str(row[key]) for row in rows}
        for key in ("artifactId", "sha256")
        for rows in (metadata, baseline, delta)
    ]
    for offset in (0, 3):
        if (
            identity_sets[offset] & identity_sets[offset + 1]
            or identity_sets[offset] & identity_sets[offset + 2]
            or identity_sets[offset + 1] & identity_sets[offset + 2]
        ):
            raise PayloadAcquisitionError("metadata, baseline and delta overlap")
    with _cas_lock(cas):
        directory = _open_sha_directory(cas, create=True)
        try:
            summary = _empty_summary()
            if preflight is not None:
                preflight(directory)
            _acquire_ordered_locked(directory, "metadata", metadata, stream_factory, summary)
            if replay_candidate() is not True:
                raise PayloadAcquisitionError("signed metadata replay differs")
            _acquire_ordered_locked(directory, "baseline", baseline, stream_factory, summary)
            _acquire_ordered_locked(directory, "delta", delta, stream_factory, summary)
            return summary
        finally:
            os.close(directory)


def _load_execution_plan(path: pathlib.Path) -> tuple[dict[str, Any], bytes]:
    raw = _read_regular_nofollow_stable(path, "payload acquisition plan")
    if hashlib.sha256(raw).hexdigest() != EXPECTED_PLAN_SHA256:
        raise PayloadAcquisitionError("payload acquisition plan bytes differ")
    plan = _canonical_object(raw, "payload acquisition plan")
    expected_keys = {
        "activationAllowed",
        "authorityInputs",
        "bootableClaim",
        "boundaries",
        "excludedRustArtifactIds",
        "expected",
        "initialCas",
        "release",
        "repository",
        "schema",
        "toolDigests",
    }
    if set(plan) != expected_keys:
        raise PayloadAcquisitionError("payload acquisition plan keys differ")
    if (
        plan["schema"] != PLAN_SCHEMA
        or not isinstance(plan["release"], str)
        or not plan["release"]
        or plan["activationAllowed"] is not False
        or plan["bootableClaim"] is not False
    ):
        raise PayloadAcquisitionError("payload acquisition plan identity differs")
    authority = plan["authorityInputs"]
    if not isinstance(authority, dict) or set(authority) != {
        "acquisitionPlan",
        "baselineResolution",
        "candidateGenerator",
        "candidatePlan",
        "candidateResult",
        "payloadAcquirer",
    }:
        raise PayloadAcquisitionError("payload authorityInputs keys differ")
    for name, pin in authority.items():
        _pin(pin, f"authorityInputs.{name}")
    boundaries = plan["boundaries"]
    if not isinstance(boundaries, dict) or set(boundaries) != {
        "bootAuthority",
        "imageBuilderAuthorityPresent",
        "kernelImageExtracted",
        "launcherElfPresent",
        "maintainerScriptsExecuted",
        "packagePayloadsAcquired",
        "packagePayloadsVerified",
        "runtimeCompatibilityVerified",
    } or any(value is not False for value in boundaries.values()):
        raise PayloadAcquisitionError("payload plan attempts an authority claim")
    expected = plan["expected"]
    if not isinstance(expected, dict) or set(expected) != {
        "baselineFetchBytes",
        "baselineFetches",
        "baselinePackageBytes",
        "baselinePackages",
        "candidatePackageBytes",
        "candidatePackages",
        "deltaFetchBytes",
        "deltaFetches",
        "deltaPackageBytes",
        "deltaPackages",
        "metadataFetchBytes",
        "metadataFetches",
    }:
        raise PayloadAcquisitionError("payload expected keys differ")
    for name, value in expected.items():
        _size(value, f"expected.{name}")
    initial = plan["initialCas"]
    if not isinstance(initial, dict) or set(initial) != {
        "packagesIndexPresent",
        "presentBaselinePackageIds",
        "presentDeltaPackageIds",
    }:
        raise PayloadAcquisitionError("payload initialCas keys differ")
    if initial["packagesIndexPresent"] is not False:
        raise PayloadAcquisitionError("payload initial index state differs")
    for name in ("presentBaselinePackageIds", "presentDeltaPackageIds"):
        values = initial[name]
        if (
            not isinstance(values, list)
            or values != sorted(values)
            or len(values) != len(set(values))
            or any(not isinstance(value, str) or not value for value in values)
        ):
            raise PayloadAcquisitionError(f"initialCas.{name} differs")
    return plan, raw


def _load_pinned_authorities(
    plan: dict[str, Any], repository_root: pathlib.Path
) -> dict[str, Any]:
    root = repository_root.resolve()
    if root != repository_root or root != SCRIPT_ROOT:
        raise PayloadAcquisitionError("repository root differs")
    authority = plan["authorityInputs"]
    paths = {
        "acquisitionPlan": root
        / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json",
        "baselineResolution": root
        / "native/containment/native-shadow-runtime-rootfs-resolution-arm64-v1.json",
        "candidateGenerator": root
        / "scripts/native_shadow_boot_rootfs_dependency_candidate_arm64_v1.py",
        "candidatePlan": root
        / "native/containment/native-shadow-boot-rootfs-dependency-candidate-plan-arm64-v1.json",
        "candidateResult": root
        / "native/containment/native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json",
        "payloadAcquirer": root
        / "scripts/native_shadow_boot_rootfs_payload_acquire_arm64_v1.py",
    }
    raw = {
        name: _read_pinned(path, authority[name], name)
        for name, path in paths.items()
        if name != "payloadAcquirer"
    }
    payload_raw = _read_regular_nofollow_stable(
        paths["payloadAcquirer"], "payloadAcquirer"
    )
    if len(payload_raw) != authority["payloadAcquirer"]["sizeBytes"]:
        raise PayloadAcquisitionError("payload acquirer size differs")
    raw["payloadAcquirer"] = payload_raw
    if payload_acquirer_authority_sha256(raw["payloadAcquirer"]) != authority[
        "payloadAcquirer"
    ]["sha256"]:
        raise PayloadAcquisitionError("payload acquirer authority digest differs")
    return {
        "paths": paths,
        "raw": raw,
        "acquisition": _canonical_object(raw["acquisitionPlan"], "ARM64 acquisition plan"),
        "baseline": _canonical_object(raw["baselineResolution"], "baseline resolution"),
        "candidatePlan": _canonical_object(raw["candidatePlan"], "candidate plan"),
        "candidateResult": _canonical_object(raw["candidateResult"], "candidate result"),
    }


def _validate_authority_and_specs(
    plan: dict[str, Any], loaded: dict[str, Any], gpgv: pathlib.Path, zstd: pathlib.Path
) -> tuple[dict[str, object], list[dict[str, object]], list[dict[str, object]]]:
    acquisition = loaded["acquisition"]
    baseline = loaded["baseline"]
    candidate_plan = loaded["candidatePlan"]
    candidate = loaded["candidateResult"]
    tools = plan["toolDigests"]
    if not isinstance(tools, dict) or set(tools) != {"gpgvSha256", "zstdSha256"}:
        raise PayloadAcquisitionError("payload toolDigests keys differ")
    actual_tools = {
        "gpgvSha256": hashlib.sha256(
            _read_regular_nofollow_stable(gpgv, "gpgv")
        ).hexdigest(),
        "zstdSha256": hashlib.sha256(
            _read_regular_nofollow_stable(zstd, "zstd")
        ).hexdigest(),
    }
    if tools != actual_tools or tools != acquisition.get("toolDigests"):
        raise PayloadAcquisitionError("payload metadata tool identity differs")
    if candidate.get("planSha256") != hashlib.sha256(canonical_json(candidate_plan)).hexdigest():
        raise PayloadAcquisitionError("candidate result plan identity differs")
    counts = candidate.get("counts")
    expected = plan["expected"]
    if counts != {
        "baselinePackageBytes": expected["baselinePackageBytes"],
        "baselinePackages": expected["baselinePackages"],
        "candidatePackageBytes": expected["candidatePackageBytes"],
        "candidatePackages": expected["candidatePackages"],
        "newPackageBytes": expected["deltaPackageBytes"],
        "newPackages": expected["deltaPackages"],
    }:
        raise PayloadAcquisitionError("candidate payload count/byte conservation differs")
    if (
        candidate.get("signedRepositoryMetadataVerified") is not True
        or candidate.get("activationAllowed") is not False
        or candidate.get("bootableClaim") is not False
        or candidate.get("productionByteProvenanceComplete") is not False
    ):
        raise PayloadAcquisitionError("candidate authority boundary differs")
    baseline_rows = baseline.get("packages")
    candidate_rows = candidate.get("resolution", {}).get("packages")
    if not isinstance(baseline_rows, list) or not isinstance(candidate_rows, list):
        raise PayloadAcquisitionError("candidate package rows differ")
    baseline_by_id = {row.get("packageId"): row for row in baseline_rows}
    candidate_by_id = {row.get("packageId"): row for row in candidate_rows}
    if (
        len(baseline_by_id) != len(baseline_rows)
        or len(candidate_by_id) != len(candidate_rows)
        or any(candidate_by_id.get(key) != row for key, row in baseline_by_id.items())
    ):
        raise PayloadAcquisitionError("baseline is not an exact candidate subset")
    delta_ids = candidate.get("newPackageIds")
    if delta_ids != sorted(set(candidate_by_id) - set(baseline_by_id)):
        raise PayloadAcquisitionError("candidate delta identity differs")

    repository = acquisition.get("repository")
    if not isinstance(repository, dict) or repository.get("snapshotBase") != SNAPSHOT_BASE:
        raise PayloadAcquisitionError("snapshot repository identity differs")
    if plan["repository"] != {
        "inRelease": repository["inRelease"],
        "keyringBootstrap": acquisition["keyringBootstrap"],
        "packagesIndex": repository["packagesIndex"],
        "snapshotBase": SNAPSHOT_BASE,
        "snapshotId": acquisition["snapshotId"],
    }:
        raise PayloadAcquisitionError("payload repository plan differs")
    excluded = plan["excludedRustArtifactIds"]
    actual_excluded = sorted(row["artifactId"] for row in acquisition["rustArtifacts"])
    if excluded != actual_excluded:
        raise PayloadAcquisitionError("Rust artifact exclusion differs")

    def package_spec(row: dict[str, Any]) -> dict[str, object]:
        return _spec(
            {
                "artifactId": row["artifactId"],
                "sha256": row["artifactSha256"],
                "sizeBytes": row["artifactSizeBytes"],
                "url": f"{SNAPSHOT_BASE}/{row['poolPath']}",
            }
        )

    baseline_specs = _ordered_unique(package_spec(row) for row in baseline_rows)
    delta_specs = _ordered_unique(package_spec(candidate_by_id[key]) for key in delta_ids)
    index = repository["packagesIndex"]
    metadata_spec = _spec(
        {
            "artifactId": index["artifactId"],
            "sha256": index["sha256"],
            "sizeBytes": index["sizeBytes"],
            "url": f"{SNAPSHOT_BASE}/{index['path']}",
        }
    )
    if (
        sum(int(row["sizeBytes"]) for row in baseline_specs)
        != expected["baselinePackageBytes"]
        or sum(int(row["sizeBytes"]) for row in delta_specs)
        != expected["deltaPackageBytes"]
    ):
        raise PayloadAcquisitionError("payload spec byte totals differ")
    return metadata_spec, baseline_specs, delta_specs


def _load_candidate_module(raw: bytes, path: pathlib.Path) -> types.ModuleType:
    module = types.ModuleType("scripts.native_shadow_boot_rootfs_dependency_candidate_arm64_v1_pinned")
    module.__file__ = str(path)
    module.__package__ = "scripts"
    exec(compile(raw, str(path), "exec"), module.__dict__)
    return module


def _write_result_once(path: pathlib.Path, raw: bytes) -> None:
    parent = path.parent.resolve()
    if parent != path.parent or not parent.is_dir():
        raise PayloadAcquisitionError("result parent is not a normalized real directory")
    directory = _open_directory_nofollow(parent, create=False)
    temporary = f".{path.name}.{os.getpid()}.{secrets.token_hex(16)}"
    descriptor = -1
    try:
        try:
            os.stat(path.name, dir_fd=directory, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise PayloadAcquisitionError("payload result already exists")
        descriptor = os.open(
            temporary,
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0),
            0o600,
            dir_fd=directory,
        )
        view = memoryview(raw)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                raise PayloadAcquisitionError("payload result write stopped")
            view = view[written:]
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = -1
        os.link(
            temporary,
            path.name,
            src_dir_fd=directory,
            dst_dir_fd=directory,
            follow_symlinks=False,
        )
        os.fsync(directory)
    except OSError as exc:
        raise PayloadAcquisitionError("payload result publication failed") from exc
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        try:
            os.unlink(temporary, dir_fd=directory)
            os.fsync(directory)
        except FileNotFoundError:
            pass
        finally:
            os.close(directory)


def _ensure_result_absent(path: pathlib.Path) -> None:
    parent = path.parent.resolve()
    if parent != path.parent or not parent.is_dir():
        raise PayloadAcquisitionError("result parent is not a normalized real directory")
    directory = _open_directory_nofollow(parent, create=False)
    try:
        try:
            os.stat(path.name, dir_fd=directory, follow_symlinks=False)
        except FileNotFoundError:
            return
        raise PayloadAcquisitionError("payload result already exists")
    finally:
        os.close(directory)


def execute_frozen_acquisition(
    *,
    plan_path: pathlib.Path,
    repository_root: pathlib.Path,
    cas: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
    output: pathlib.Path,
    stream_factory: StreamFactory = snapshot_https_stream,
) -> dict[str, Any]:
    plan, plan_raw = _load_execution_plan(plan_path)
    loaded = _load_pinned_authorities(plan, repository_root)
    metadata_spec, baseline_specs, delta_specs = _validate_authority_and_specs(
        plan, loaded, gpgv_path, zstd_path
    )
    _ensure_result_absent(output)
    acquisition = loaded["acquisition"]
    initial = plan["initialCas"]
    expected_baseline = set(initial["presentBaselinePackageIds"])
    expected_delta = set(initial["presentDeltaPackageIds"])
    baseline_ids = {str(row["artifactId"]) for row in baseline_specs}
    delta_ids = {str(row["artifactId"]) for row in delta_specs}
    if not expected_baseline <= baseline_ids or not expected_delta <= delta_ids:
        raise PayloadAcquisitionError("initial CAS package identities are outside the candidate")

    inrelease = acquisition["repository"]["inRelease"]
    inrelease_spec = _spec(
        {
            "artifactId": inrelease["artifactId"],
            "sha256": inrelease["sha256"],
            "sizeBytes": inrelease["sizeBytes"],
            "url": f"{SNAPSHOT_BASE}/{inrelease['path']}",
        }
    )
    keyring = acquisition["keyringBootstrap"]
    keyring_spec = _spec(
        {
            "artifactId": keyring["bootstrapArtifactId"],
            "sha256": keyring["sha256"],
            "sizeBytes": keyring["sizeBytes"],
            "url": keyring["url"],
        }
    )

    def preflight(directory: int) -> None:
        if not _verify_name(directory, inrelease_spec) or not _verify_name(directory, keyring_spec):
            raise PayloadAcquisitionError("required signed metadata bootstrap is absent")
        if _verify_name(directory, metadata_spec):
            raise PayloadAcquisitionError("initial Packages.xz state differs")
        for spec in baseline_specs:
            present = _verify_name(directory, spec)
            if present != (str(spec["artifactId"]) in expected_baseline):
                raise PayloadAcquisitionError("initial baseline CAS inventory differs")
        for spec in delta_specs:
            present = _verify_name(directory, spec)
            if present != (str(spec["artifactId"]) in expected_delta):
                raise PayloadAcquisitionError("initial delta CAS inventory differs")

    candidate_module = _load_candidate_module(
        loaded["raw"]["candidateGenerator"], loaded["paths"]["candidateGenerator"]
    )

    def replay_candidate() -> bool:
        replayed = candidate_module.generate_from_cas(
            plan_path=loaded["paths"]["candidatePlan"],
            repository_root=repository_root,
            cas=cas,
            gpgv_path=gpgv_path,
            zstd_path=zstd_path,
        )
        return candidate_module.canonical_json(replayed) == loaded["raw"]["candidateResult"]

    summary = acquire_after_signed_replay(
        cas,
        metadata_spec,
        baseline_specs,
        delta_specs,
        replay_candidate=replay_candidate,
        stream_factory=stream_factory,
        preflight=preflight,
    )
    expected = plan["expected"]
    if summary != {
        "metadataFetched": expected["metadataFetches"],
        "metadataReused": 0,
        "baselineFetched": expected["baselineFetches"],
        "baselineReused": len(expected_baseline),
        "deltaFetched": expected["deltaFetches"],
        "deltaReused": len(expected_delta),
        "fetchedBytes": expected["metadataFetchBytes"]
        + expected["baselineFetchBytes"]
        + expected["deltaFetchBytes"],
    }:
        raise PayloadAcquisitionError("payload acquisition request conservation differs")
    result = {
        "activationAllowed": False,
        "bootArtifactsWritten": 0,
        "bootableClaim": False,
        "boundaries": {
            "bootAuthority": False,
            "imageBuilderAuthorityPresent": False,
            "kernelImageExtracted": False,
            "launcherElfPresent": False,
            "maintainerScriptsExecuted": False,
            "packagePayloadsAcquired": True,
            "packagePayloadsVerified": True,
            "runtimeCompatibilityVerified": False,
        },
        "candidateResultSha256": hashlib.sha256(loaded["raw"]["candidateResult"]).hexdigest(),
        "counts": {
            **expected,
            **summary,
        },
        "excludedRustArtifactIds": plan["excludedRustArtifactIds"],
        "fetchedArtifactIds": [str(metadata_spec["artifactId"])]
        + sorted(baseline_ids - expected_baseline)
        + sorted(delta_ids - expected_delta),
        "planSha256": hashlib.sha256(plan_raw).hexdigest(),
        "productionByteProvenanceComplete": False,
        "reusedPackageIds": sorted(expected_baseline | expected_delta),
        "schema": RESULT_SCHEMA,
        "signedRepositoryMetadataVerified": True,
        "status": RESULT_STATUS,
    }
    _write_result_once(output, canonical_json(result))
    return result


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", type=pathlib.Path, required=True)
    parser.add_argument("--repository-root", type=pathlib.Path, required=True)
    parser.add_argument("--cas", type=pathlib.Path, required=True)
    parser.add_argument("--gpgv", type=pathlib.Path, required=True)
    parser.add_argument("--zstd", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args(argv)
    try:
        execute_frozen_acquisition(
            plan_path=args.plan,
            repository_root=args.repository_root,
            cas=args.cas,
            gpgv_path=args.gpgv.resolve(),
            zstd_path=args.zstd.resolve(),
            output=args.output,
        )
    except PayloadAcquisitionError as exc:
        print(f"native-shadow ARM64 payload acquisition: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
