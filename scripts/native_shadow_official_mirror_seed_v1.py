#!/usr/bin/env python3
"""Seed frozen Ubuntu package bytes from the official architecture mirrors.

The snapshot remains the authority for signed repository metadata.  Package
payloads are transport-only: their expected size and SHA-256 already come from
the signed, frozen records.  This helper copies only ``/pool/`` objects from
Ubuntu's official archive/ports mirrors into the existing content-addressed
store, where the original acquirers independently verify and reuse them.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import pathlib
import ssl
import sys
import time
import urllib.parse
from collections.abc import Callable, Iterable
from typing import Any, Optional


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_ci_payload_acquire_arm64_v1 as ci_payload
from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload
from scripts import native_shadow_boot_writer_set_acquire_arm64_v1 as writer_set


MIRRORS = {
    "amd64": ("archive.ubuntu.com", "/ubuntu/"),
    "arm64": ("ports.ubuntu.com", "/ubuntu-ports/"),
}
METADATA_PATHS = {
    "amd64": {
        "dists/noble/InRelease",
        "dists/noble/main/binary-amd64/Packages.xz",
    },
    "arm64": {
        "dists/noble/InRelease",
        "dists/noble/main/binary-arm64/Packages.xz",
    },
}
MAX_ATTEMPTS = 3


class MirrorSeedError(RuntimeError):
    """The official mirror could not reproduce a frozen package object."""


class TransientMirrorSeedError(MirrorSeedError):
    """The official mirror was temporarily unavailable."""


def mirror_url(snapshot_url: str, architecture: str) -> str:
    if architecture not in MIRRORS:
        raise ValueError(f"unsupported mirror architecture: {architecture}")
    parsed = urllib.parse.urlsplit(snapshot_url)
    prefix = "/ubuntu/"
    if (
        parsed.scheme != "https"
        or parsed.netloc != "snapshot.ubuntu.com"
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("package source is not the frozen Ubuntu snapshot")
    remainder = parsed.path.removeprefix(prefix)
    if remainder == parsed.path or "/" not in remainder:
        raise ValueError("frozen Ubuntu mirror path differs")
    timestamp, artifact_path = remainder.split("/", 1)
    if len(timestamp) != 16 or timestamp[8] != "T" or timestamp[-1] != "Z":
        raise ValueError("snapshot timestamp shape differs")
    if not timestamp[:8].isdigit() or not timestamp[9:15].isdigit():
        raise ValueError("snapshot timestamp shape differs")
    if ".." in artifact_path.split("/"):
        raise ValueError("snapshot artifact path differs")
    if not artifact_path.startswith("pool/") and artifact_path not in METADATA_PATHS[architecture]:
        raise ValueError("snapshot artifact is not an approved pool or metadata object")
    host, base = MIRRORS[architecture]
    return urllib.parse.urlunsplit(("https", host, base + artifact_path, "", ""))


def official_https_stream(
    url: str,
    expected_size: int,
    *,
    connection_factory: Any = http.client.HTTPSConnection,
    context_factory: Callable[[], ssl.SSLContext] = ssl.create_default_context,
) -> Iterable[bytes]:
    parsed = urllib.parse.urlsplit(url)
    allowed = {host for host, _ in MIRRORS.values()}
    if parsed.scheme != "https" or parsed.hostname not in allowed or parsed.port is not None:
        raise MirrorSeedError("official mirror URL differs")
    context = context_factory()
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.verify_mode = ssl.CERT_REQUIRED
    context.check_hostname = True
    connection = connection_factory(parsed.hostname, 443, timeout=60, context=context)
    observed = 0
    try:
        connection.putrequest("GET", parsed.path, skip_host=True, skip_accept_encoding=True)
        connection.putheader("Host", parsed.hostname)
        connection.putheader("Accept-Encoding", "identity")
        connection.putheader("Connection", "close")
        connection.putheader("User-Agent", "boole-official-mirror-seed-v1")
        connection.endheaders()
        response = connection.getresponse()
        if response.status in (500, 502, 503, 504):
            raise TransientMirrorSeedError("official mirror returned a transient 5xx")
        if response.status != 200:
            raise MirrorSeedError("official mirror response status is not 200")
        if response.getheader("Content-Encoding") not in (None, "identity"):
            raise MirrorSeedError("official mirror response encoding is forbidden")
        if response.getheader("Content-Length") != str(expected_size):
            raise MirrorSeedError("official mirror Content-Length differs")
        while True:
            chunk = response.read(min(1024 * 1024, expected_size - observed + 1))
            if not chunk:
                break
            observed += len(chunk)
            if observed > expected_size:
                raise MirrorSeedError("official mirror response exceeds frozen size")
            yield chunk
        if observed != expected_size:
            raise MirrorSeedError("official mirror response is shorter than frozen size")
    except MirrorSeedError:
        raise
    except (TimeoutError, ConnectionError, http.client.HTTPException, OSError) as exc:
        raise TransientMirrorSeedError("official mirror request failed") from exc
    finally:
        connection.close()


def seed_specs(
    *,
    cas: pathlib.Path,
    specs: Iterable[dict[str, object]],
    architecture: str,
    stream_factory: Callable[[str, int], Iterable[bytes]] = official_https_stream,
    sleep: Callable[[float], None] = time.sleep,
) -> dict[str, int]:
    fetched = 0
    reused = 0
    ordered = sorted(specs, key=lambda row: str(row["artifactId"]))
    with payload._cas_lock(cas):
        directory = payload._open_sha_directory(cas, create=True)
        try:
            for spec in ordered:
                if payload._verify_name(directory, spec):
                    reused += 1
                    continue
                mirror = mirror_url(str(spec["url"]), architecture)
                for attempt in range(1, MAX_ATTEMPTS + 1):
                    try:
                        payload._store_stream(
                            directory,
                            spec,
                            stream_factory(mirror, int(spec["sizeBytes"])),
                        )
                        break
                    except TransientMirrorSeedError:
                        if attempt == MAX_ATTEMPTS:
                            raise
                        sleep(30.0)
                if not payload._verify_name(directory, spec):
                    raise MirrorSeedError(
                        f"{spec['artifactId']} did not survive official mirror publication"
                    )
                fetched += 1
        finally:
            os.close(directory)
    return {"fetched": fetched, "reused": reused}


def boot_specs() -> list[dict[str, object]]:
    rows = [dict(row) for row in ci_payload.derive_plan()["artifacts"]]
    rows.extend(dict(row) for row in writer_set.derive_plan()["artifacts"])
    digests = [str(row["sha256"]) for row in rows]
    if len(rows) != len(set(digests)):
        raise MirrorSeedError("boot mirror seed identities overlap")
    return sorted(rows, key=lambda row: str(row["artifactId"]))


def _read_object(path: pathlib.Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise MirrorSeedError(f"cannot read {context}") from exc
    if not isinstance(value, dict):
        raise MirrorSeedError(f"{context} is not an object")
    return value


def runtime_bootstrap_specs(plan_path: pathlib.Path) -> list[dict[str, object]]:
    plan = _read_object(plan_path, "runtime acquisition plan")
    row = plan.get("keyringBootstrap")
    if not isinstance(row, dict):
        raise MirrorSeedError("runtime acquisition plan has no keyring bootstrap")
    return [
        {
            "artifactId": str(row["bootstrapArtifactId"]),
            "sha256": str(row["sha256"]),
            "sizeBytes": int(row["sizeBytes"]),
            "url": str(row["url"]),
        }
    ]


def runtime_metadata_specs(plan_path: pathlib.Path) -> list[dict[str, object]]:
    plan = _read_object(plan_path, "runtime acquisition plan")
    repository = plan.get("repository")
    if not isinstance(repository, dict):
        raise MirrorSeedError("runtime acquisition plan has no repository")
    base = str(repository["snapshotBase"]).rstrip("/")
    rows = []
    for key in ("inRelease", "packagesIndex"):
        row = repository.get(key)
        if not isinstance(row, dict):
            raise MirrorSeedError(f"runtime acquisition plan has no {key}")
        rows.append(
            {
                "artifactId": str(row["artifactId"]),
                "sha256": str(row["sha256"]),
                "sizeBytes": int(row["sizeBytes"]),
                "url": f"{base}/{row['path']}",
            }
        )
    return sorted(rows, key=lambda row: str(row["artifactId"]))


def runtime_package_specs(
    plan_path: pathlib.Path, resolution_path: pathlib.Path
) -> list[dict[str, object]]:
    plan = _read_object(plan_path, "runtime acquisition plan")
    resolution = _read_object(resolution_path, "runtime resolution")
    base = str(plan["repository"]["snapshotBase"])
    rows = []
    for package in resolution.get("packages", []):
        rows.append(
            {
                "artifactId": str(package["artifactId"]),
                "sha256": str(package["artifactSha256"]),
                "sizeBytes": int(package["artifactSizeBytes"]),
                "url": f"{base}/{package['poolPath']}",
            }
        )
    if not rows:
        raise MirrorSeedError("runtime resolution has no packages")
    return rows


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    boot = commands.add_parser("boot")
    boot.add_argument("--cas", type=pathlib.Path, required=True)
    for name in ("runtime-bootstrap", "runtime-metadata", "runtime-packages"):
        command = commands.add_parser(name)
        command.add_argument("--architecture", choices=sorted(MIRRORS), required=True)
        command.add_argument("--plan", type=pathlib.Path, required=True)
        command.add_argument("--cas", type=pathlib.Path, required=True)
        if name == "runtime-packages":
            command.add_argument("--resolution", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "boot":
            architecture = "arm64"
            specs = boot_specs()
        elif args.command == "runtime-bootstrap":
            architecture = args.architecture
            specs = runtime_bootstrap_specs(args.plan)
        elif args.command == "runtime-metadata":
            architecture = args.architecture
            specs = runtime_metadata_specs(args.plan)
        else:
            architecture = args.architecture
            specs = runtime_package_specs(args.plan, args.resolution)
        summary = seed_specs(cas=args.cas, specs=specs, architecture=architecture)
    except (MirrorSeedError, payload.PayloadAcquisitionError, ValueError) as exc:
        print(f"official-mirror-seed: {exc}", file=sys.stderr)
        return 1
    print(
        f"official mirror seed: fetched={summary['fetched']} reused={summary['reused']}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
