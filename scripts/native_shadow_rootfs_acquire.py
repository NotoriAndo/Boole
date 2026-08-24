#!/usr/bin/env python3
"""Acquire and seal the pinned native-shadow rootfs source closure.

Networked acquisition and offline authority decisions are intentionally split:
metadata/payload fetches only store pre-declared or signed bytes in a CAS, while
``resolve`` and ``seal`` never use the network.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import pathlib
import re
import secrets
import stat
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Iterable, Optional

if __package__:
    from scripts import native_shadow_rootfs_builder as rootfs
else:
    sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
    from scripts import native_shadow_rootfs_builder as rootfs


PLAN_SCHEMA = "boole.native-shadow.runtime-rootfs-acquisition-plan.v1"
RESOLUTION_SCHEMA = "boole.native-shadow.runtime-rootfs-resolution.v1"
EXPECTED_PLAN_SHA256 = "8d8ac1a4fd82370c1f0c12a270bd38b9b2b78f0c1a155432298b4d654a0fb06e"
EXPECTED_SNAPSHOT_ID = "20240425T160000Z"
EXPECTED_SNAPSHOT_TIME = "2024-04-25T16:00:00Z"
EXPECTED_RELEASE = "NATIVE-SHADOW-RUNTIME-ROOTFS-ACQUISITION-PLAN-V1-QUALIFICATION"
EXPECTED_BASE = "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z"
EXPECTED_HOSTS = ["ci-artifacts.rust-lang.org", "snapshot.ubuntu.com"]
EXPECTED_SEEDS = [
    "binutils-x86-64-linux-gnu",
    "gcc-13-x86-64-linux-gnu",
    "libc6-dev",
    "python3.12",
]
_PLAN_DIGEST_LITERAL = re.compile(
    rb'(?m)^(EXPECTED_PLAN_SHA256 = ")[0-9a-f]{64}("\s*)$'
)


class AcquisitionError(ValueError):
    """An acquisition plan, artifact, or dependency decision is unsafe."""


class ArtifactBudget:
    """Fail closed on both declared and observed acquisition byte totals."""

    def __init__(
        self,
        policy: dict[str, Any],
        specs: Iterable[dict[str, Any]],
        *,
        initial_declared_bytes: int = 0,
        initial_actual_bytes: int = 0,
    ) -> None:
        self.max_artifact_bytes = _size(
            policy.get("maxArtifactBytes"), "maxArtifactBytes", 2147483648
        )
        self.max_total_bytes = _size(
            policy.get("maxTotalBytes"), "maxTotalBytes", 4294967296
        )
        for value, context in (
            (initial_declared_bytes, "initial declared bytes"),
            (initial_actual_bytes, "initial actual bytes"),
        ):
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise AcquisitionError(f"{context} is invalid")
        self.declared_bytes = initial_declared_bytes
        self.actual_bytes = initial_actual_bytes
        for spec in specs:
            size = _size(spec.get("sizeBytes"), "declared artifact", 2147483648)
            if size > self.max_artifact_bytes:
                raise AcquisitionError(
                    f"artifact {spec.get('artifactId')} exceeds maxArtifactBytes"
                )
            self.declared_bytes += size
            if self.declared_bytes > self.max_total_bytes:
                raise AcquisitionError("declared acquisition exceeds maxTotalBytes")
        if self.actual_bytes > self.max_total_bytes:
            raise AcquisitionError("actual acquisition exceeds maxTotalBytes")

    def account(self, actual_bytes: int, artifact_id: str) -> None:
        if (
            isinstance(actual_bytes, bool)
            or not isinstance(actual_bytes, int)
            or actual_bytes < 0
        ):
            raise AcquisitionError(f"actual artifact size is invalid: {artifact_id}")
        if actual_bytes > self.max_artifact_bytes:
            raise AcquisitionError(
                f"actual artifact {artifact_id} exceeds maxArtifactBytes"
            )
        self.actual_bytes += actual_bytes
        if self.actual_bytes > self.max_total_bytes:
            raise AcquisitionError("actual acquisition exceeds maxTotalBytes")


def acquirer_authority_sha256(raw: bytes) -> str:
    """Hash the acquirer while removing only its plan-digest back reference."""

    normalized, replacements = _PLAN_DIGEST_LITERAL.subn(
        rb"\g<1>" + (b"0" * 64) + rb"\g<2>", raw
    )
    if replacements != 1:
        raise AcquisitionError("acquirer contains an invalid plan-digest authority literal")
    return _sha256(normalized)


def _sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise AcquisitionError(f"{context} keys differ")
    return value


def _digest(value: Any, context: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise AcquisitionError(f"{context} is not lowercase SHA-256")
    return value


def _size(value: Any, context: str, maximum: int = 536870912) -> int:
    if (
        isinstance(value, bool)
        or not isinstance(value, int)
        or value <= 0
        or value > maximum
    ):
        raise AcquisitionError(f"{context} size is invalid")
    return value


def _url(value: Any, allowed_hosts: Iterable[str], context: str) -> str:
    if not isinstance(value, str):
        raise AcquisitionError(f"{context} URL is invalid")
    parsed = urllib.parse.urlsplit(value)
    if (
        parsed.scheme != "https"
        or parsed.hostname not in set(allowed_hosts)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.port not in (None, 443)
        or parsed.query
        or parsed.fragment
        or not parsed.path.startswith("/")
        or ".." in pathlib.PurePosixPath(parsed.path).parts
    ):
        raise AcquisitionError(f"{context} URL violates the frozen network policy")
    return value


def load_plan(
    raw: bytes,
    builder_path: pathlib.Path,
    acquirer_path: pathlib.Path = pathlib.Path(__file__),
) -> dict[str, Any]:
    if _sha256(raw) != EXPECTED_PLAN_SHA256:
        raise AcquisitionError("rootfs acquisition plan bytes differ from v1 authority")
    try:
        plan = rootfs.load_json_exact(raw, "rootfs acquisition plan", require_canonical=True)
    except rootfs.RootfsBuildError as exc:
        raise AcquisitionError(str(exc)) from exc
    plan = _exact(
        plan,
        {
            "schema",
            "release",
            "activationAllowed",
            "snapshotId",
            "snapshotTime",
            "builderSha256",
            "acquirerAuthoritySha256",
            "sourceLockScaffoldSha256",
            "repository",
            "keyringBootstrap",
            "rustArtifacts",
            "seeds",
            "trustedUbuntuFingerprints",
            "toolDigests",
            "networkPolicy",
            "selectionPolicy",
        },
        "rootfs acquisition plan",
    )
    if (
        plan["schema"] != PLAN_SCHEMA
        or plan["release"] != EXPECTED_RELEASE
        or plan["activationAllowed"] is not False
        or plan["snapshotId"] != EXPECTED_SNAPSHOT_ID
        or plan["snapshotTime"] != EXPECTED_SNAPSHOT_TIME
    ):
        raise AcquisitionError("rootfs acquisition plan identity differs")
    actual_builder = _sha256(_read_regular_nofollow(builder_path))
    if _digest(plan["builderSha256"], "builderSha256") != actual_builder:
        raise AcquisitionError("acquisition plan builder digest differs")
    actual_acquirer = acquirer_authority_sha256(_read_regular_nofollow(acquirer_path))
    if (
        _digest(plan["acquirerAuthoritySha256"], "acquirerAuthoritySha256")
        != actual_acquirer
    ):
        raise AcquisitionError("acquisition plan acquirer digest differs")
    _digest(plan["sourceLockScaffoldSha256"], "sourceLockScaffoldSha256")

    network = _exact(
        plan["networkPolicy"],
        {
            "allowedHosts",
            "allowEnvironmentProxy",
            "allowRedirects",
            "httpsOnly",
            "maxArtifactBytes",
            "maxTotalBytes",
        },
        "network policy",
    )
    if (
        network["allowedHosts"] != EXPECTED_HOSTS
        or network["allowEnvironmentProxy"] is not False
        or network["allowRedirects"] is not False
        or network["httpsOnly"] is not True
    ):
        raise AcquisitionError("network policy differs")
    max_artifact = _size(network["maxArtifactBytes"], "max artifact", 2147483648)
    max_total = _size(network["maxTotalBytes"], "max total", 4294967296)
    if max_artifact > max_total:
        raise AcquisitionError("maxArtifactBytes exceeds maxTotalBytes")

    repository = _exact(
        plan["repository"],
        {
            "id",
            "snapshotBase",
            "suite",
            "component",
            "architecture",
            "inRelease",
            "packagesIndex",
        },
        "repository",
    )
    if repository != {
        **repository,
        "id": "noble-main",
        "snapshotBase": EXPECTED_BASE,
        "suite": "noble",
        "component": "main",
        "architecture": "amd64",
    }:
        raise AcquisitionError("repository identity differs")
    _url(repository["snapshotBase"], EXPECTED_HOSTS, "snapshot base")
    for label, expected_keys in (
        ("inRelease", {"artifactId", "path", "sha256", "sizeBytes"}),
        (
            "packagesIndex",
            {"artifactId", "path", "releasePath", "sha256", "sizeBytes"},
        ),
    ):
        spec = _exact(repository[label], expected_keys, label)
        _digest(spec["sha256"], f"{label} SHA-256")
        _size(spec["sizeBytes"], label)
        _url(f"{EXPECTED_BASE}/{spec['path']}", EXPECTED_HOSTS, label)
    if repository["inRelease"]["path"] != "dists/noble/InRelease" or repository[
        "packagesIndex"
    ]["path"] != "dists/noble/main/binary-amd64/Packages.xz":
        raise AcquisitionError("repository metadata path differs")
    if repository["packagesIndex"]["releasePath"] != "main/binary-amd64/Packages.xz":
        raise AcquisitionError("Packages release path differs")

    keyring = _exact(
        plan["keyringBootstrap"],
        {
            "bootstrapArtifactId",
            "keyringArtifactId",
            "url",
            "sha256",
            "sizeBytes",
            "memberPath",
        },
        "keyring bootstrap",
    )
    _url(keyring["url"], EXPECTED_HOSTS, "keyring bootstrap")
    _digest(keyring["sha256"], "keyring bootstrap SHA-256")
    _size(keyring["sizeBytes"], "keyring bootstrap")
    if keyring["memberPath"] != "usr/share/keyrings/ubuntu-archive-keyring.gpg":
        raise AcquisitionError("keyring member path differs")
    if keyring["url"] != (
        EXPECTED_BASE
        + "/pool/main/u/ubuntu-keyring/ubuntu-keyring_2023.11.28.1_all.deb"
    ):
        raise AcquisitionError("keyring bootstrap URL differs")

    rust_rows = plan["rustArtifacts"]
    if not isinstance(rust_rows, list) or len(rust_rows) != 3:
        raise AcquisitionError("Rust artifact set differs")
    rust_ids: list[str] = []
    for index, raw_row in enumerate(rust_rows):
        row = _exact(raw_row, {"artifactId", "url", "sha256", "sizeBytes"}, f"rust[{index}]")
        rust_ids.append(row["artifactId"])
        _url(row["url"], EXPECTED_HOSTS, f"rust[{index}]")
        _digest(row["sha256"], f"rust[{index}] SHA-256")
        _size(row["sizeBytes"], f"rust[{index}]")
    if rust_ids != sorted(set(rust_ids)):
        raise AcquisitionError("Rust artifacts are not sorted and unique")
    if plan["seeds"] != EXPECTED_SEEDS:
        raise AcquisitionError("seed set differs")
    if plan["trustedUbuntuFingerprints"] != sorted(rootfs.UBUNTU_ARCHIVE_SIGNING_FINGERPRINTS):
        raise AcquisitionError("Ubuntu signing fingerprint set differs")
    tools = _exact(plan["toolDigests"], {"gpgvSha256", "zstdSha256"}, "tool digests")
    _digest(tools["gpgvSha256"], "gpgv SHA-256")
    _digest(tools["zstdSha256"], "zstd SHA-256")
    selection = _exact(
        plan["selectionPolicy"],
        {
            "dependencyFields",
            "alternativeOrder",
            "directPackageBeforeProvider",
            "ambiguousCandidateAction",
            "qualifiedDependencyAction",
            "unreachableExtraAction",
        },
        "selection policy",
    )
    if selection != {
        "dependencyFields": ["Depends", "Pre-Depends"],
        "alternativeOrder": "left-to-right-first-satisfiable",
        "directPackageBeforeProvider": True,
        "ambiguousCandidateAction": "stop",
        "qualifiedDependencyAction": "stop",
        "unreachableExtraAction": "stop",
    }:
        raise AcquisitionError("selection policy differs")
    return plan


def _candidate(stanza_raw: bytes, fields: dict[str, str], repository_id: str, component: str) -> dict[str, Any]:
    required = ("Package", "Version", "Architecture", "Filename", "Size", "SHA256")
    if any(not fields.get(key) for key in required):
        raise AcquisitionError("Packages stanza lacks a required authority field")
    if fields["Architecture"] not in {"amd64", "all"}:
        raise AcquisitionError("package architecture differs")
    digest = _digest(fields["SHA256"], "package SHA-256")
    try:
        declared_size = int(fields["Size"])
    except (TypeError, ValueError) as exc:
        raise AcquisitionError("package size is not an integer") from exc
    size = _size(declared_size, "package")
    try:
        source_name, source_version = rootfs._source_identity(fields)
        rootfs._relative_path(fields["Filename"], "package pool path")
    except (rootfs.RootfsBuildError, ValueError) as exc:
        raise AcquisitionError(str(exc)) from exc
    return {
        "packageId": f"deb-{digest}",
        "name": fields["Package"],
        "version": fields["Version"],
        "architecture": fields["Architecture"],
        "sourceName": source_name,
        "sourceVersion": source_version,
        "repositoryId": repository_id,
        "component": component,
        "poolPath": fields["Filename"],
        "artifactId": f"deb-{digest}",
        "artifactSha256": digest,
        "artifactSizeBytes": size,
        "indexStanzaSha256": _sha256(stanza_raw),
        "depends": fields.get("Depends", ""),
        "preDepends": fields.get("Pre-Depends", ""),
        "provides": fields.get("Provides", ""),
        "multiArch": fields.get("Multi-Arch", ""),
        "essential": fields.get("Essential", "no") == "yes",
        "dependencyResolutions": [],
    }


def _expression_name(expression: str) -> str:
    match = rootfs._DEPENDENCY_RE.fullmatch(expression)
    if match is None or match.group("qualifier") is not None:
        raise AcquisitionError(f"unsupported dependency expression: {expression}")
    return match.group("name")


def _choose(alternatives: list[str], candidates: list[dict[str, Any]]) -> tuple[int, dict[str, Any]]:
    for alternative_index, expression in enumerate(alternatives):
        name = _expression_name(expression)
        try:
            matching = [item for item in candidates if rootfs._dependency_matches(expression, item)]
        except rootfs.RootfsBuildError as exc:
            raise AcquisitionError(str(exc)) from exc
        direct = [item for item in matching if item["name"] == name]
        pool = direct if direct else matching
        if len(pool) > 1:
            raise AcquisitionError(f"ambiguous dependency resolution: {expression}")
        if pool:
            return alternative_index, pool[0]
    raise AcquisitionError(f"unresolved dependency group: {' | '.join(alternatives)}")


def resolve_package_closure(
    packages_raw: bytes,
    seeds: list[str],
    repository_id: str,
    component: str,
) -> dict[str, Any]:
    try:
        rows = rootfs._deb822_stanzas(packages_raw)
    except rootfs.RootfsBuildError as exc:
        raise AcquisitionError(str(exc)) from exc
    candidates = [_candidate(raw, fields, repository_id, component) for raw, fields in rows]
    candidates.sort(key=lambda item: (item["name"], item["version"], item["architecture"], item["packageId"]))
    ids = [item["packageId"] for item in candidates]
    if len(ids) != len(set(ids)):
        raise AcquisitionError("package artifact identity is ambiguous")

    selected: dict[str, dict[str, Any]] = {}
    seed_ids: list[str] = []
    pending: list[dict[str, Any]] = []
    for seed in seeds:
        direct = [item for item in candidates if item["name"] == seed]
        if len(direct) != 1:
            raise AcquisitionError(f"seed package identity differs: {seed}")
        seed_ids.append(direct[0]["packageId"])
        pending.append(direct[0])
    while pending:
        package = pending.pop(0)
        if package["packageId"] in selected:
            continue
        copied = copy.deepcopy(package)
        resolutions: list[dict[str, Any]] = []
        for field, key in (("Depends", "depends"), ("Pre-Depends", "preDepends")):
            try:
                groups = rootfs._split_dependency_groups(copied[key])
            except rootfs.RootfsBuildError as exc:
                raise AcquisitionError(str(exc)) from exc
            for group_index, alternatives in enumerate(groups):
                alternative_index, chosen = _choose(alternatives, candidates)
                resolutions.append(
                    {
                        "field": field,
                        "groupIndex": group_index,
                        "alternativeIndex": alternative_index,
                        "packageId": chosen["packageId"],
                    }
                )
                pending.append(chosen)
        copied["dependencyResolutions"] = sorted(
            resolutions, key=lambda item: (item["field"], item["groupIndex"])
        )
        selected[copied["packageId"]] = copied
    packages = [selected[key] for key in sorted(selected)]
    return {
        "schema": RESOLUTION_SCHEMA,
        "seedPackageIds": sorted(seed_ids),
        "packages": packages,
    }


def _cas_path(cas: pathlib.Path, digest: str) -> pathlib.Path:
    return cas / "sha256" / digest


def _open_directory_nofollow(
    path: pathlib.Path, context: str, *, create: bool
) -> int:
    flags = (
        os.O_RDONLY
        | getattr(os, "O_CLOEXEC", 0)
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    absolute = pathlib.Path(os.path.abspath(os.fspath(path)))
    if sys.platform == "darwin" and len(absolute.parts) > 1 and absolute.parts[1] in {
        "etc",
        "tmp",
        "var",
    }:
        # macOS exposes these three root-owned compatibility aliases as
        # symlinks into /private. Normalize only the fixed OS aliases; never
        # resolve caller-controlled descendants.
        absolute = pathlib.Path("/private").joinpath(*absolute.parts[1:])
    try:
        descriptor = os.open(os.path.sep, flags)
    except OSError as exc:
        raise AcquisitionError(f"cannot open filesystem root for {context}") from exc
    try:
        for component in absolute.parts[1:]:
            if component in {"", ".", "..", os.path.sep}:
                raise AcquisitionError(f"{context} directory path is not normalized")
            if create:
                try:
                    os.mkdir(component, mode=0o755, dir_fd=descriptor)
                except FileExistsError:
                    pass
                except OSError as exc:
                    raise AcquisitionError(
                        f"cannot create {context} directory component: {component}"
                    ) from exc
            try:
                child = os.open(component, flags, dir_fd=descriptor)
            except OSError as exc:
                raise AcquisitionError(
                    f"{context} directory is absent, unsafe, or contains a symlink: {path}"
                ) from exc
            os.close(descriptor)
            descriptor = child
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            raise AcquisitionError(f"{context} path is not a directory: {path}")
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _open_cas_sha_directory(cas: pathlib.Path, *, create: bool) -> int:
    root_descriptor = _open_directory_nofollow(cas, "CAS", create=create)
    try:
        if create:
            try:
                os.mkdir("sha256", mode=0o755, dir_fd=root_descriptor)
            except FileExistsError:
                pass
            except OSError as exc:
                raise AcquisitionError("cannot create CAS sha256 directory") from exc
        flags = (
            os.O_RDONLY
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        try:
            descriptor = os.open("sha256", flags, dir_fd=root_descriptor)
        except OSError as exc:
            raise AcquisitionError("CAS sha256 directory is absent, unsafe, or a symlink") from exc
        if not stat.S_ISDIR(os.fstat(descriptor).st_mode):
            os.close(descriptor)
            raise AcquisitionError("CAS sha256 path is not a directory")
        return descriptor
    finally:
        os.close(root_descriptor)


def _read_regular_at(directory_descriptor: int, name: str, context: str) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(name, flags, dir_fd=directory_descriptor)
    except OSError as exc:
        raise AcquisitionError(f"cannot open {context} without following links: {name}") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise AcquisitionError(f"{context} is not a regular file")
        chunks: list[bytes] = []
        remaining = metadata.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise AcquisitionError(f"{context} was truncated")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _read_cas_blob(cas: pathlib.Path, digest: str) -> bytes:
    digest = _digest(digest, "CAS artifact SHA-256")
    directory_descriptor = _open_cas_sha_directory(cas, create=False)
    try:
        return _read_regular_at(directory_descriptor, digest, "CAS artifact")
    finally:
        os.close(directory_descriptor)


def _verified_cas_artifact(cas: pathlib.Path, spec: dict[str, Any]) -> bytes:
    digest = _digest(spec["sha256"], "artifact SHA-256")
    size = _size(spec["sizeBytes"], "artifact", 2147483648)
    raw = _read_cas_blob(cas, digest)
    if len(raw) != size or _sha256(raw) != digest:
        raise AcquisitionError("CAS artifact differs from its declared digest/size")
    return raw


def _read_regular_nofollow(path: pathlib.Path) -> bytes:
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(str(path), flags)
    except OSError as exc:
        raise AcquisitionError(f"cannot open artifact without following links: {path}") from exc
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode):
            raise AcquisitionError("artifact source is not a regular file")
        chunks: list[bytes] = []
        remaining = metadata.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise AcquisitionError("artifact source was truncated")
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _store_bytes(cas: pathlib.Path, spec: dict[str, Any], raw: bytes) -> pathlib.Path:
    digest = _digest(spec["sha256"], "artifact SHA-256")
    expected_size = _size(spec["sizeBytes"], "artifact", 2147483648)
    if len(raw) != expected_size or _sha256(raw) != digest:
        raise AcquisitionError("artifact bytes differ from the frozen digest/size")
    directory_descriptor = _open_cas_sha_directory(cas, create=True)
    destination = _cas_path(cas, digest)
    try:
        existing = _read_regular_at(directory_descriptor, digest, "CAS destination")
    except AcquisitionError as exc:
        try:
            os.stat(digest, dir_fd=directory_descriptor, follow_symlinks=False)
        except FileNotFoundError:
            existing = None
        except OSError:
            raise exc
        else:
            raise exc
    if existing is not None:
        os.close(directory_descriptor)
        if existing != raw:
            raise AcquisitionError("CAS destination contains different bytes")
        return destination

    temporary_name = f".{digest}.{os.getpid()}.{secrets.token_hex(16)}"
    try:
        flags = (
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        descriptor = os.open(temporary_name, flags, 0o600, dir_fd=directory_descriptor)
        with os.fdopen(descriptor, "wb", closefd=True) as target:
            target.write(raw)
            target.flush()
            os.fsync(target.fileno())
        try:
            os.link(
                temporary_name,
                digest,
                src_dir_fd=directory_descriptor,
                dst_dir_fd=directory_descriptor,
                follow_symlinks=False,
            )
        except FileExistsError:
            existing = _read_regular_at(directory_descriptor, digest, "CAS destination")
            if existing != raw:
                raise AcquisitionError("concurrent CAS writer stored different bytes")
        os.fsync(directory_descriptor)
    finally:
        try:
            os.unlink(temporary_name, dir_fd=directory_descriptor)
        except FileNotFoundError:
            pass
        finally:
            os.close(directory_descriptor)
    return destination


def import_local_artifact(cas: pathlib.Path, spec: dict[str, Any], source: pathlib.Path) -> pathlib.Path:
    return _store_bytes(cas, spec, _read_regular_nofollow(source))


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, newurl: str) -> None:
        raise AcquisitionError("HTTP redirect is forbidden by the frozen plan")


def fetch_artifact(cas: pathlib.Path, spec: dict[str, Any], allowed_hosts: list[str]) -> pathlib.Path:
    url = _url(spec["url"], allowed_hosts, "artifact")
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), _NoRedirect())
    try:
        with opener.open(url, timeout=60) as response:
            if response.status != 200 or response.geturl() != url:
                raise AcquisitionError("artifact HTTP identity differs")
            maximum = _size(spec["sizeBytes"], "artifact")
            raw = response.read(maximum + 1)
            if len(raw) > maximum or response.read(1):
                raise AcquisitionError("artifact exceeds frozen size")
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AcquisitionError(f"artifact fetch failed: {url}") from exc
    return _store_bytes(cas, spec, raw)


def _specs(plan: dict[str, Any]) -> list[dict[str, Any]]:
    repository = plan["repository"]
    result = [
        {
            "artifactId": repository["inRelease"]["artifactId"],
            "url": f"{repository['snapshotBase']}/{repository['inRelease']['path']}",
            "sha256": repository["inRelease"]["sha256"],
            "sizeBytes": repository["inRelease"]["sizeBytes"],
        },
        {
            "artifactId": repository["packagesIndex"]["artifactId"],
            "url": f"{repository['snapshotBase']}/{repository['packagesIndex']['path']}",
            "sha256": repository["packagesIndex"]["sha256"],
            "sizeBytes": repository["packagesIndex"]["sizeBytes"],
        },
        {
            "artifactId": plan["keyringBootstrap"]["bootstrapArtifactId"],
            "url": plan["keyringBootstrap"]["url"],
            "sha256": plan["keyringBootstrap"]["sha256"],
            "sizeBytes": plan["keyringBootstrap"]["sizeBytes"],
        },
    ]
    result.extend(plan["rustArtifacts"])
    return sorted(result, key=lambda item: item["artifactId"])


def _load_canonical(path: pathlib.Path, context: str) -> Any:
    value, _ = _load_canonical_raw(path, context)
    return value


def _load_canonical_raw(path: pathlib.Path, context: str) -> tuple[Any, bytes]:
    try:
        raw = _read_regular_nofollow(path)
        return rootfs.load_json_exact(raw, context, require_canonical=True), raw
    except (OSError, rootfs.RootfsBuildError) as exc:
        raise AcquisitionError(str(exc)) from exc


def _write_canonical(path: pathlib.Path, value: Any) -> None:
    _write_bytes_atomic(path, rootfs.canonical_json(value))


def _write_bytes_atomic(path: pathlib.Path, raw: bytes) -> None:
    directory_descriptor = _open_directory_nofollow(
        path.parent, "output", create=True
    )
    temporary_name = f".{path.name}.{os.getpid()}.{secrets.token_hex(16)}"
    try:
        try:
            metadata = os.stat(path.name, dir_fd=directory_descriptor, follow_symlinks=False)
        except FileNotFoundError:
            metadata = None
        except OSError as exc:
            raise AcquisitionError(f"cannot inspect output path: {path}") from exc
        if metadata is not None and stat.S_ISLNK(metadata.st_mode):
            raise AcquisitionError(f"existing output is a symlink: {path}")
        if metadata is not None and not stat.S_ISREG(metadata.st_mode):
            raise AcquisitionError(f"existing output is not a regular file: {path}")
        flags = (
            os.O_WRONLY
            | os.O_CREAT
            | os.O_EXCL
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        descriptor = os.open(temporary_name, flags, 0o600, dir_fd=directory_descriptor)
        with os.fdopen(descriptor, "wb", closefd=True) as target:
            target.write(raw)
            target.flush()
            os.fsync(target.fileno())
        os.replace(
            temporary_name,
            path.name,
            src_dir_fd=directory_descriptor,
            dst_dir_fd=directory_descriptor,
        )
        os.fsync(directory_descriptor)
    finally:
        try:
            os.unlink(temporary_name, dir_fd=directory_descriptor)
        except FileNotFoundError:
            pass
        finally:
            os.close(directory_descriptor)
    if _read_regular_nofollow(path) != raw:
        raise AcquisitionError("atomic output bytes are not reachable at the requested path")


def resolve_from_cas(
    plan: dict[str, Any],
    cas: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> dict[str, Any]:
    tools = plan["toolDigests"]
    gpgv = rootfs._read_pinned_executable(str(gpgv_path), tools["gpgvSha256"], "Ubuntu gpgv")
    zstd = rootfs._read_pinned_executable(str(zstd_path), tools["zstdSha256"], "zstd")
    keyring_plan = plan["keyringBootstrap"]
    keyring_deb = _verified_cas_artifact(cas, keyring_plan)
    entries = rootfs._deb_payload(
        keyring_deb,
        "Ubuntu keyring bootstrap",
        {
            "maxEntries": 200000,
            "maxFileBytes": 536870912,
            "maxTotalBytes": 536870912,
            "zstdPath": str(zstd_path),
            "zstdSha256": tools["zstdSha256"],
        },
    )
    keyring_entry = entries.get(keyring_plan["memberPath"])
    if keyring_entry is None or keyring_entry["kind"] != "file":
        raise AcquisitionError("Ubuntu archive keyring member is absent")
    keyring = keyring_entry["raw"]
    keyring_spec = {"sha256": _sha256(keyring), "sizeBytes": len(keyring)}
    _store_bytes(cas, keyring_spec, keyring)

    repository = plan["repository"]
    inrelease_spec = repository["inRelease"]
    index_spec = repository["packagesIndex"]
    inrelease = _verified_cas_artifact(cas, inrelease_spec)
    index_raw = _verified_cas_artifact(cas, index_spec)
    snapshot = rootfs._snapshot_time(plan["snapshotTime"])
    try:
        payload = rootfs._verify_inrelease(
            gpgv,
            keyring,
            inrelease,
            frozenset(plan["trustedUbuntuFingerprints"]),
            snapshot,
        )
        release, hashes = rootfs._release_fields(payload)
        if (
            release.get("Suite") != "noble"
            or release.get("Codename") != "noble"
            or "main" not in release.get("Components", "").split()
            or "amd64" not in release.get("Architectures", "").split()
        ):
            raise AcquisitionError("signed Ubuntu Release identity differs")
        rootfs._release_window_allows(release, snapshot, immutable_release_pocket=True)
        if hashes.get(index_spec["releasePath"]) != (
            index_spec["sha256"],
            index_spec["sizeBytes"],
        ):
            raise AcquisitionError("Packages index differs from signed Release")
        packages_raw = rootfs._packages_payload(
            index_raw, index_spec["releasePath"], plan["networkPolicy"]["maxTotalBytes"]
        )
    except rootfs.RootfsBuildError as exc:
        raise AcquisitionError(str(exc)) from exc
    resolution = resolve_package_closure(
        packages_raw,
        plan["seeds"],
        repository["id"],
        repository["component"],
    )
    resolution.update(
        {
            "snapshotId": plan["snapshotId"],
            "snapshotTime": plan["snapshotTime"],
            "planSha256": _sha256(rootfs.canonical_json(plan)),
            "keyring": {
                "artifactId": keyring_plan["keyringArtifactId"],
                "sha256": keyring_spec["sha256"],
                "sizeBytes": keyring_spec["sizeBytes"],
            },
        }
    )
    return resolution


def replay_resolution_from_cas(
    plan: dict[str, Any],
    resolution_path: pathlib.Path,
    cas: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> dict[str, Any]:
    _, provided_raw = _load_canonical_raw(resolution_path, "rootfs resolution")
    replayed = resolve_from_cas(plan, cas, gpgv_path, zstd_path)
    if provided_raw != rootfs.canonical_json(replayed):
        raise AcquisitionError(
            "stored resolution is not byte-for-byte equal to the CAS replay"
        )
    return replayed


def _package_spec(package: dict[str, Any], plan: dict[str, Any]) -> dict[str, Any]:
    return {
        "artifactId": package["artifactId"],
        "url": f"{plan['repository']['snapshotBase']}/{package['poolPath']}",
        "sha256": package["artifactSha256"],
        "sizeBytes": package["artifactSizeBytes"],
    }


def payload_specs_from_signed_resolution(
    plan: dict[str, Any],
    resolution_path: pathlib.Path,
    cas: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """Replay signed metadata before deriving any network fetch target."""

    resolution = replay_resolution_from_cas(
        plan,
        resolution_path,
        cas,
        gpgv_path,
        zstd_path,
    )
    return resolution, [_package_spec(package, plan) for package in resolution["packages"]]


def _verify_resolution_identity(plan: dict[str, Any], resolution: dict[str, Any]) -> None:
    if (
        resolution.get("schema") != RESOLUTION_SCHEMA
        or resolution.get("snapshotId") != plan["snapshotId"]
        or resolution.get("snapshotTime") != plan["snapshotTime"]
        or resolution.get("planSha256") != _sha256(rootfs.canonical_json(plan))
    ):
        raise AcquisitionError("resolution does not belong to the frozen acquisition plan")


def seal_candidate(
    plan: dict[str, Any],
    scaffold_path: pathlib.Path,
    resolution: dict[str, Any],
    cas: pathlib.Path,
    repository_root: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
) -> tuple[dict[str, Any], bytes]:
    scaffold, scaffold_raw = _load_canonical_raw(
        scaffold_path, "rootfs source-lock scaffold"
    )
    if _sha256(scaffold_raw) != _digest(
        plan.get("sourceLockScaffoldSha256"), "sourceLockScaffoldSha256"
    ):
        raise AcquisitionError("rootfs source-lock scaffold digest differs")
    _verify_resolution_identity(plan, resolution)
    candidate = copy.deepcopy(scaffold)
    candidate["release"] = "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-COMPLETE-NOT-ACTIVATABLE"
    candidate["activationAllowed"] = False

    artifacts: list[dict[str, Any]] = []
    for row in plan["rustArtifacts"]:
        artifacts.append(
            {
                "id": row["artifactId"],
                "kind": "rust-dist",
                "sha256": row["sha256"],
                "sizeBytes": row["sizeBytes"],
            }
        )
    artifacts.extend(
        [
            {
                "id": resolution["keyring"]["artifactId"],
                "kind": "ubuntu-keyring",
                "sha256": resolution["keyring"]["sha256"],
                "sizeBytes": resolution["keyring"]["sizeBytes"],
            },
            {
                "id": plan["repository"]["inRelease"]["artifactId"],
                "kind": "ubuntu-inrelease",
                "sha256": plan["repository"]["inRelease"]["sha256"],
                "sizeBytes": plan["repository"]["inRelease"]["sizeBytes"],
            },
            {
                "id": plan["repository"]["packagesIndex"]["artifactId"],
                "kind": "ubuntu-packages-index",
                "sha256": plan["repository"]["packagesIndex"]["sha256"],
                "sizeBytes": plan["repository"]["packagesIndex"]["sizeBytes"],
            },
        ]
    )
    packages: list[dict[str, Any]] = []
    for row in resolution["packages"]:
        artifacts.append(
            {
                "id": row["artifactId"],
                "kind": "deb",
                "sha256": row["artifactSha256"],
                "sizeBytes": row["artifactSizeBytes"],
            }
        )
        packages.append(
            {key: value for key, value in row.items() if key not in {"artifactSha256", "artifactSizeBytes"}}
        )
    candidate["artifacts"] = sorted(artifacts, key=lambda item: item["id"])
    repository = plan["repository"]
    candidate["ubuntu"] = {
        "snapshot": plan["snapshotTime"],
        "verification": {
            "gpgvPath": str(gpgv_path),
            "gpgvSha256": plan["toolDigests"]["gpgvSha256"],
        },
        "repositories": [
            {
                "id": repository["id"],
                "snapshotBase": repository["snapshotBase"],
                "suite": repository["suite"],
                "component": repository["component"],
                "architecture": repository["architecture"],
                "keyringArtifactId": resolution["keyring"]["artifactId"],
                "inReleaseArtifactId": repository["inRelease"]["artifactId"],
                "packagesIndexArtifactId": repository["packagesIndex"]["artifactId"],
                "packagesIndexPath": repository["packagesIndex"]["releasePath"],
            }
        ],
        "seeds": plan["seeds"],
        "seedPackageIds": resolution["seedPackageIds"],
        "packages": sorted(packages, key=lambda item: item["packageId"]),
    }
    candidate["derivedEntries"] = [
        {
            "logicalPath": "/etc/localtime",
            "kind": "symlink",
            "target": "../usr/share/zoneinfo/Etc/UTC",
            "mode": "0777",
            "uid": 0,
            "gid": 0,
        },
        {
            "logicalPath": "/usr/bin/cc",
            "kind": "symlink",
            "target": "x86_64-linux-gnu-gcc-13",
            "mode": "0777",
            "uid": 0,
            "gid": 0,
        }
    ]
    candidate["buildRecipe"]["builderSha256"] = plan["builderSha256"]
    candidate["buildRecipe"]["zstdPath"] = str(zstd_path)
    candidate["buildRecipe"]["zstdSha256"] = plan["toolDigests"]["zstdSha256"]
    artifact_specs = [
        {"sha256": row["sha256"], "sizeBytes": row["sizeBytes"]}
        for row in candidate["artifacts"]
    ]
    for spec in artifact_specs:
        _verified_cas_artifact(cas, spec)
    raw = rootfs.canonical_json(candidate)
    try:
        rootfs.validate_source_lock(
            candidate,
            raw,
            repository_root,
            cas,
            require_complete=True,
        )
    except rootfs.RootfsBuildError as exc:
        raise AcquisitionError(str(exc)) from exc
    return candidate, raw


def replay_candidate_lock(
    plan: dict[str, Any],
    scaffold_path: pathlib.Path,
    resolution: dict[str, Any],
    cas: pathlib.Path,
    repository_root: pathlib.Path,
    gpgv_path: pathlib.Path,
    zstd_path: pathlib.Path,
    candidate_path: pathlib.Path,
) -> dict[str, Any]:
    provided, provided_raw = _load_canonical_raw(
        candidate_path, "candidate source lock"
    )
    _, expected_raw = seal_candidate(
        plan,
        scaffold_path,
        resolution,
        cas,
        repository_root,
        gpgv_path,
        zstd_path,
    )
    if provided_raw != expected_raw:
        raise AcquisitionError(
            "candidate source lock is not byte-for-byte equal to the sealed replay"
        )
    return provided


def _parse_local(values: list[str]) -> dict[str, pathlib.Path]:
    result: dict[str, pathlib.Path] = {}
    for value in values:
        identifier, separator, path = value.partition("=")
        if not separator or not identifier or identifier in result:
            raise AcquisitionError("--local-artifact must be unique ID=PATH")
        result[identifier] = pathlib.Path(path)
    return result


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for name in ("fetch-metadata", "resolve", "fetch-payloads", "seal", "verify"):
        command = subparsers.add_parser(name)
        command.add_argument("--plan", type=pathlib.Path, required=True)
        command.add_argument("--builder", type=pathlib.Path, default=pathlib.Path(__file__).with_name("native_shadow_rootfs_builder.py"))
        command.add_argument("--cas", type=pathlib.Path, required=True)
    fetch = subparsers.choices["fetch-metadata"]
    fetch.add_argument("--local-artifact", action="append", default=[])
    resolve = subparsers.choices["resolve"]
    resolve.add_argument("--gpgv", type=pathlib.Path, required=True)
    resolve.add_argument("--zstd", type=pathlib.Path, required=True)
    resolve.add_argument("--output", type=pathlib.Path, required=True)
    payloads = subparsers.choices["fetch-payloads"]
    payloads.add_argument("--resolution", type=pathlib.Path, required=True)
    payloads.add_argument("--gpgv", type=pathlib.Path, required=True)
    payloads.add_argument("--zstd", type=pathlib.Path, required=True)
    seal = subparsers.choices["seal"]
    seal.add_argument("--resolution", type=pathlib.Path, required=True)
    seal.add_argument("--scaffold", type=pathlib.Path, required=True)
    seal.add_argument("--repo-root", type=pathlib.Path, required=True)
    seal.add_argument("--gpgv", type=pathlib.Path, required=True)
    seal.add_argument("--zstd", type=pathlib.Path, required=True)
    seal.add_argument("--output", type=pathlib.Path, required=True)
    verify = subparsers.choices["verify"]
    verify.add_argument("--resolution", type=pathlib.Path, required=True)
    verify.add_argument("--scaffold", type=pathlib.Path, required=True)
    verify.add_argument("--candidate-lock", type=pathlib.Path, required=True)
    verify.add_argument("--repo-root", type=pathlib.Path, required=True)
    verify.add_argument("--gpgv", type=pathlib.Path, required=True)
    verify.add_argument("--zstd", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    plan = load_plan(_read_regular_nofollow(args.plan), args.builder)
    if args.command == "fetch-metadata":
        local = _parse_local(args.local_artifact)
        specs = _specs(plan)
        known_ids = {spec["artifactId"] for spec in specs}
        if set(local) - known_ids:
            raise AcquisitionError(f"unknown local artifact IDs: {sorted(set(local) - known_ids)}")
        budget = ArtifactBudget(plan["networkPolicy"], specs)
        for spec in specs:
            if spec["artifactId"] in local:
                path = import_local_artifact(args.cas, spec, local.pop(spec["artifactId"]))
            else:
                path = fetch_artifact(
                    args.cas, spec, plan["networkPolicy"]["allowedHosts"]
                )
            budget.account(len(_verified_cas_artifact(args.cas, spec)), spec["artifactId"])
            if path != _cas_path(args.cas, spec["sha256"]):
                raise AcquisitionError("artifact storage path differs from CAS authority")
    elif args.command == "resolve":
        resolution = resolve_from_cas(plan, args.cas, args.gpgv.resolve(), args.zstd.resolve())
        _write_canonical(args.output, resolution)
    elif args.command == "fetch-payloads":
        resolution, package_specs = payload_specs_from_signed_resolution(
            plan,
            args.resolution,
            args.cas,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
        base_specs = _specs(plan)
        base_actual = sum(
            len(_verified_cas_artifact(args.cas, spec)) for spec in base_specs
        )
        budget = ArtifactBudget(
            plan["networkPolicy"],
            package_specs,
            initial_declared_bytes=sum(spec["sizeBytes"] for spec in base_specs),
            initial_actual_bytes=base_actual,
        )
        for spec in package_specs:
            fetch_artifact(
                args.cas,
                spec,
                plan["networkPolicy"]["allowedHosts"],
            )
            budget.account(len(_verified_cas_artifact(args.cas, spec)), spec["artifactId"])
    elif args.command == "seal":
        if args.output.resolve() == args.scaffold.resolve():
            raise AcquisitionError("seal output must not overwrite the tracked scaffold")
        resolution = replay_resolution_from_cas(
            plan,
            args.resolution,
            args.cas,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
        _, raw = seal_candidate(
            plan,
            args.scaffold,
            resolution,
            args.cas,
            args.repo_root,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
        _write_bytes_atomic(args.output, raw)
    else:
        resolution = replay_resolution_from_cas(
            plan,
            args.resolution,
            args.cas,
            args.gpgv.resolve(),
            args.zstd.resolve(),
        )
        replay_candidate_lock(
            plan,
            args.scaffold,
            resolution,
            args.cas,
            args.repo_root,
            args.gpgv.resolve(),
            args.zstd.resolve(),
            args.candidate_lock,
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AcquisitionError as exc:
        print(f"native-shadow rootfs acquisition failed: {exc}", file=sys.stderr)
        raise SystemExit(2)
