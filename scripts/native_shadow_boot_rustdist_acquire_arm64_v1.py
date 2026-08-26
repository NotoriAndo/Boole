#!/usr/bin/env python3
"""Acquire the three frozen ARM64 Rust distribution archives into the CAS.

The ARM64 launcher cannot be built until the exact cross toolchain archives
exist locally as bytes. Their identity is *not* invented here: URL, SHA-256 and
size for ``cargo``, ``rust-std`` and ``rustc`` were frozen in the merged
``native-shadow-runtime-rootfs-acquisition-plan-arm64-v1`` document and carried
into the sealed successor source lock. This tool only turns that frozen
identity into verified bytes.

The download is deliberately unforgiving. One exact HTTPS request per artifact,
no environment proxy, no redirect, no retry, no Range, no parallelism, TLS 1.2
or better with certificate and hostname validation, ``Content-Length`` must
equal the frozen size, the digest is computed while streaming, and publication
into the content-addressed store is an atomic link that can never expose a
partial file. An artifact already present in the CAS is verified in place and
never re-fetched, so re-running this tool issues no network request at all.

Nothing here installs a toolchain, builds a launcher, extracts a kernel or
boots anything. Holding the archive bytes is an input fact; every boundary in
the result document stays false.
"""
from __future__ import annotations

import argparse
import hashlib
import http.client
import os
import pathlib
import ssl
import sys
import urllib.parse
from typing import Any, Callable, Iterable, Optional

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload

canonical_json = payload.canonical_json


class RustDistAcquisitionError(RuntimeError):
    """Raised when frozen Rust distribution identity is not honoured exactly."""


CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
PLAN_PATH = CONTAINMENT / "native-shadow-boot-rustdist-acquisition-plan-arm64-v1.json"
RESULT_PATH = CONTAINMENT / "native-shadow-boot-rustdist-acquisition-result-arm64-v1.json"
SOURCE_PLAN_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json"
SOURCE_LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"

PLAN_SCHEMA = "boole.native-shadow.boot-rustdist-acquisition-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-rustdist-acquisition-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-RUSTDIST-ACQUISITION-ARM64-V1"
RESULT_STATUS = "RUSTDIST-PAYLOADS-ACQUIRED-VERIFIED-NOT-TOOLCHAIN-AUTHORITY"

CAS_ROOT = REPO_ROOT / "local-docs" / "native-shadow-runtime-rootfs-source-lock-v1" / "cas"

ARTIFACT_KEYS = {"artifactId", "sha256", "sizeBytes", "url"}
RESULT_ARTIFACT_KEYS = {"artifactId", "disposition", "sha256", "sizeBytes"}
BOUNDARY_KEYS = {
    "bootAuthority",
    "imageBuilderAuthorityPresent",
    "kernelImageExtracted",
    "launcherElfBuilt",
    "reproducibleBuildProven",
    "runtimeCompatibilityVerified",
    "toolchainInstalled",
}
NETWORK_POLICY = {
    "allowEnvironmentProxy": False,
    "allowRangeRequests": False,
    "allowRedirects": False,
    "allowRetries": False,
    "allowedHosts": ["ci-artifacts.rust-lang.org"],
    "concurrency": 1,
    "httpsOnly": True,
    "maxArtifactBytes": 536870912,
    "maxTotalBytes": 2147483648,
    "minimumTlsVersion": "TLSv1.2",
    "requireCertificateValidation": True,
    "requireContentLengthMatch": True,
    "requireHostnameValidation": True,
}

# The plan pins this tool by the digest it has with the literal below zeroed,
# and this tool pins the plan by the literal. Neither can drift unnoticed.
PLAN_SHA256 = "8ee39ab4c828c31bdd82bf8da12546d9b6595aeac8e6e9f4da9899eaacf0accc"


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def rustdist_acquirer_authority_sha256(raw: bytes) -> str:
    """Digest this tool with its embedded plan digest normalized to zeros."""

    marker = b'PLAN_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def _digest(value: Any, context: str) -> str:
    if not isinstance(value, str) or len(value) != 64:
        raise RustDistAcquisitionError(f"{context} digest is not a sha256 value")
    if value.strip("0123456789abcdef"):
        raise RustDistAcquisitionError(f"{context} digest is not a sha256 value")
    return value


def _size(value: Any, context: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
        raise RustDistAcquisitionError(f"{context} size is not a positive integer")
    return value


def _pin(path: pathlib.Path, pin: Any, context: str) -> bytes:
    if not isinstance(pin, dict) or set(pin) != {"sha256", "sizeBytes"}:
        raise RustDistAcquisitionError(f"{context} pin keys differ")
    expected_digest = _digest(pin["sha256"], context)
    expected_size = _size(pin["sizeBytes"], context)
    raw = payload._read_regular_nofollow_stable(path, context)
    if len(raw) != expected_size or sha256_bytes(raw) != expected_digest:
        raise RustDistAcquisitionError(f"{context} digest or size differs from the pin")
    return raw


def _artifact(value: Any, prefix: str) -> dict[str, Any]:
    """Validate one frozen archive row against the Rust CI URL policy."""

    if not isinstance(value, dict) or set(value) != ARTIFACT_KEYS:
        raise RustDistAcquisitionError("rust artifact keys differ")
    identifier = value["artifactId"]
    url = value["url"]
    if not isinstance(identifier, str) or not identifier:
        raise RustDistAcquisitionError("rust artifact artifactId is invalid")
    if not isinstance(url, str) or not url:
        raise RustDistAcquisitionError("rust artifact URL is invalid")
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname not in NETWORK_POLICY["allowedHosts"]
        or parsed.port not in (None, 443)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or not parsed.path.startswith(prefix)
        or "\\" in parsed.path
        or "%" in parsed.path
        or "//" in parsed.path
        or ".." in pathlib.PurePosixPath(parsed.path).parts
        or not parsed.path.endswith(".tar.xz")
    ):
        raise RustDistAcquisitionError("rust artifact URL violates the frozen host policy")
    _digest(value["sha256"], "rust artifact")
    size = _size(value["sizeBytes"], "rust artifact")
    if size > int(NETWORK_POLICY["maxArtifactBytes"]):
        raise RustDistAcquisitionError("rust artifact exceeds the per-artifact cap")
    return value


def rustdist_https_stream(
    spec: dict[str, Any],
    *,
    connection_factory: Any = http.client.HTTPSConnection,
    context_factory: Callable[[], ssl.SSLContext] = ssl.create_default_context,
) -> Iterable[bytes]:
    """Yield one exact Rust CI response without proxy, redirect, retry or Range."""

    parsed = urllib.parse.urlsplit(str(spec["url"]))
    host = parsed.hostname
    if host not in NETWORK_POLICY["allowedHosts"]:
        raise RustDistAcquisitionError("rust artifact host is not allowlisted")
    expected_size = int(spec["sizeBytes"])
    context = context_factory()
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.verify_mode = ssl.CERT_REQUIRED
    context.check_hostname = True
    connection = connection_factory(host, 443, timeout=120, context=context)
    observed = 0
    try:
        connection.putrequest(
            "GET",
            parsed.path,
            skip_host=True,
            skip_accept_encoding=True,
        )
        connection.putheader("Host", host)
        connection.putheader("Accept-Encoding", "identity")
        connection.putheader("Connection", "close")
        connection.putheader("User-Agent", "boole-arm64-rustdist-acquirer-v1")
        connection.endheaders()
        response = connection.getresponse()
        if response.status != 200:
            raise RustDistAcquisitionError("rust artifact response status is not 200")
        encoding = response.getheader("Content-Encoding")
        if encoding not in (None, "identity"):
            raise RustDistAcquisitionError("rust artifact response encoding is forbidden")
        length = response.getheader("Content-Length")
        if length != str(expected_size):
            raise RustDistAcquisitionError("rust artifact Content-Length differs")
        while True:
            chunk = response.read(min(1024 * 1024, expected_size - observed + 1))
            if not chunk:
                break
            if not isinstance(chunk, bytes):
                raise RustDistAcquisitionError("rust artifact response chunk is invalid")
            observed += len(chunk)
            if observed > expected_size:
                raise RustDistAcquisitionError("rust artifact response exceeds frozen size")
            yield chunk
        if observed != expected_size:
            raise RustDistAcquisitionError("rust artifact response is shorter than frozen size")
    except RustDistAcquisitionError:
        raise
    except (OSError, TimeoutError, http.client.HTTPException) as exc:
        raise RustDistAcquisitionError("rust artifact request failed") from exc
    finally:
        connection.close()


def load_plan(path: pathlib.Path = PLAN_PATH) -> dict[str, Any]:
    """Read the pre-registered plan and validate every frozen field."""

    raw = payload._read_regular_nofollow_stable(path, "rustdist plan")
    if sha256_bytes(raw) != PLAN_SHA256:
        raise RustDistAcquisitionError("rustdist plan digest differs from the embedded pin")
    plan = payload._canonical_object(raw, "rustdist plan")
    validate_plan(plan)
    return plan


def validate_plan(plan: dict[str, Any]) -> dict[str, Any]:
    """Refuse any plan whose frozen fields drift from their authority."""

    expected_keys = {
        "activationAllowed",
        "artifacts",
        "authorityInputs",
        "boundaries",
        "bootableClaim",
        "cas",
        "expected",
        "networkPolicy",
        "release",
        "schema",
        "toolchain",
    }
    if not isinstance(plan, dict) or set(plan) != expected_keys:
        raise RustDistAcquisitionError("rustdist plan keys differ")
    if plan["schema"] != PLAN_SCHEMA or plan["release"] != RELEASE:
        raise RustDistAcquisitionError("rustdist plan schema or release differs")
    if plan["activationAllowed"] is not False or plan["bootableClaim"] is not False:
        raise RustDistAcquisitionError("rustdist plan must not claim activation or boot")
    _validate_boundaries(plan["boundaries"])
    if plan["networkPolicy"] != NETWORK_POLICY:
        raise RustDistAcquisitionError("rustdist plan network policy differs")
    _validate_toolchain(plan["toolchain"])
    _validate_cas(plan["cas"])
    _validate_artifacts(plan)
    _validate_authority_inputs(plan)
    _validate_expected(plan)
    return plan


def _validate_boundaries(value: Any) -> None:
    if not isinstance(value, dict) or set(value) != BOUNDARY_KEYS:
        raise RustDistAcquisitionError("rustdist boundary keys differ")
    for name, flag in sorted(value.items()):
        if flag is not False:
            raise RustDistAcquisitionError(f"rustdist boundary {name} must stay false")


def _validate_toolchain(value: Any) -> None:
    expected = {
        "cargoCommitHash",
        "commitPathPrefix",
        "installPrefix",
        "rustTarget",
        "rustcCommitHash",
    }
    if not isinstance(value, dict) or set(value) != expected:
        raise RustDistAcquisitionError("rustdist toolchain keys differ")
    if value["rustTarget"] != "aarch64-unknown-linux-gnu":
        raise RustDistAcquisitionError("rustdist toolchain target is not aarch64")
    for name in ("cargoCommitHash", "rustcCommitHash"):
        commit = value[name]
        if not isinstance(commit, str) or len(commit) != 40:
            raise RustDistAcquisitionError(f"rustdist toolchain {name} is not a commit hash")
        if commit.strip("0123456789abcdef"):
            raise RustDistAcquisitionError(f"rustdist toolchain {name} is not a commit hash")
    prefix = value["commitPathPrefix"]
    if prefix != f"/rustc-builds/{value['rustcCommitHash']}/":
        raise RustDistAcquisitionError("rustdist commit path prefix is not commit-derived")


def _validate_cas(value: Any) -> None:
    if not isinstance(value, dict) or set(value) != {"layout", "relativeRoot"}:
        raise RustDistAcquisitionError("rustdist cas keys differ")
    if value["layout"] != "sha256":
        raise RustDistAcquisitionError("rustdist cas layout differs")
    if value["relativeRoot"] != "local-docs/native-shadow-runtime-rootfs-source-lock-v1/cas":
        raise RustDistAcquisitionError("rustdist cas root differs")


def _validate_artifacts(plan: dict[str, Any]) -> None:
    artifacts = plan["artifacts"]
    prefix = plan["toolchain"]["commitPathPrefix"]
    if not isinstance(artifacts, list) or not artifacts:
        raise RustDistAcquisitionError("rustdist artifacts are missing")
    rows = [_artifact(value, prefix) for value in artifacts]
    identifiers = [row["artifactId"] for row in rows]
    digests = [row["sha256"] for row in rows]
    urls = [row["url"] for row in rows]
    if len(set(identifiers)) != len(rows) or len(set(digests)) != len(rows):
        raise RustDistAcquisitionError("rustdist artifact identity is duplicated")
    if len(set(urls)) != len(rows):
        raise RustDistAcquisitionError("rustdist artifact identity is duplicated")
    if identifiers != sorted(identifiers):
        raise RustDistAcquisitionError("rustdist artifacts are not sorted by artifactId")
    total = sum(int(row["sizeBytes"]) for row in rows)
    if total > int(NETWORK_POLICY["maxTotalBytes"]):
        raise RustDistAcquisitionError("rustdist artifacts exceed the total cap")


def _validate_authority_inputs(plan: dict[str, Any]) -> None:
    inputs = plan["authorityInputs"]
    expected = {"bootSourceLock", "rustdistAcquirer", "runtimeAcquisitionPlan"}
    if not isinstance(inputs, dict) or set(inputs) != expected:
        raise RustDistAcquisitionError("rustdist authority input keys differ")

    tool_pin = inputs["rustdistAcquirer"]
    if not isinstance(tool_pin, dict) or set(tool_pin) != {"sha256", "sizeBytes"}:
        raise RustDistAcquisitionError("rustdist acquirer pin keys differ")
    raw_tool = payload._read_regular_nofollow_stable(TOOL_PATH, "rustdist acquirer")
    if len(raw_tool) != _size(tool_pin["sizeBytes"], "rustdist acquirer"):
        raise RustDistAcquisitionError("rustdist acquirer size differs from the pin")
    if rustdist_acquirer_authority_sha256(raw_tool) != _digest(
        tool_pin["sha256"], "rustdist acquirer"
    ):
        raise RustDistAcquisitionError("rustdist acquirer digest differs from the pin")

    source_plan = payload._canonical_object(
        _pin(SOURCE_PLAN_PATH, inputs["runtimeAcquisitionPlan"], "runtime acquisition plan"),
        "runtime acquisition plan",
    )
    frozen = {
        row["artifactId"]: row
        for row in source_plan["rustArtifacts"]
        if isinstance(row, dict) and "artifactId" in row
    }
    for row in plan["artifacts"]:
        origin = frozen.get(row["artifactId"])
        if origin is None:
            raise RustDistAcquisitionError(
                "rustdist artifact is absent from the frozen acquisition plan"
            )
        if (
            origin.get("url") != row["url"]
            or origin.get("sha256") != row["sha256"]
            or origin.get("sizeBytes") != row["sizeBytes"]
        ):
            raise RustDistAcquisitionError(
                "rustdist artifact differs from the frozen acquisition plan"
            )

    source_lock = payload._canonical_object(
        _pin(SOURCE_LOCK_PATH, inputs["bootSourceLock"], "boot source lock"),
        "boot source lock",
    )
    sealed = {
        row["id"]: row
        for row in source_lock["artifacts"]
        if isinstance(row, dict) and row.get("kind") == "rust-dist"
    }
    for row in plan["artifacts"]:
        entry = sealed.get(row["artifactId"])
        if entry is None:
            raise RustDistAcquisitionError("rustdist artifact is absent from the sealed lock")
        if entry.get("sha256") != row["sha256"] or entry.get("sizeBytes") != row["sizeBytes"]:
            raise RustDistAcquisitionError("rustdist artifact differs from the sealed lock")
    if set(sealed) != {row["artifactId"] for row in plan["artifacts"]}:
        raise RustDistAcquisitionError("rustdist artifact set differs from the sealed lock")


def _validate_expected(plan: dict[str, Any]) -> None:
    expected = plan["expected"]
    keys = {"artifactCount", "fetchArtifactIds", "fetchBytes", "presentArtifactIds", "totalBytes"}
    if not isinstance(expected, dict) or set(expected) != keys:
        raise RustDistAcquisitionError("rustdist expected keys differ")
    identifiers = [row["artifactId"] for row in plan["artifacts"]]
    if expected["artifactCount"] != len(identifiers):
        raise RustDistAcquisitionError("rustdist expected artifactCount differs")
    total = sum(int(row["sizeBytes"]) for row in plan["artifacts"])
    if expected["totalBytes"] != total:
        raise RustDistAcquisitionError("rustdist expected totalBytes differs")
    present = expected["presentArtifactIds"]
    fetch = expected["fetchArtifactIds"]
    for name, value in (("presentArtifactIds", present), ("fetchArtifactIds", fetch)):
        if not isinstance(value, list) or value != sorted(set(value)):
            raise RustDistAcquisitionError(f"rustdist expected {name} is not a sorted unique list")
        if any(item not in identifiers for item in value):
            raise RustDistAcquisitionError(f"rustdist expected {name} names an unknown artifact")
    if sorted(present + fetch) != sorted(identifiers):
        raise RustDistAcquisitionError("rustdist expected partition is not the artifact set")
    sizes = {row["artifactId"]: int(row["sizeBytes"]) for row in plan["artifacts"]}
    if expected["fetchBytes"] != sum(sizes[item] for item in fetch):
        raise RustDistAcquisitionError("rustdist expected fetchBytes differs")


def acquire(
    plan: dict[str, Any],
    *,
    stream_factory: Callable[[dict[str, Any]], Iterable[bytes]] = rustdist_https_stream,
    cas_root: pathlib.Path = CAS_ROOT,
) -> dict[str, Any]:
    """Verify or fetch each frozen archive exactly once, newest state last."""

    rows: list[dict[str, Any]] = []
    fetched_bytes = 0
    with payload._cas_lock(cas_root):
        directory = payload._open_sha_directory(cas_root, create=True)
        try:
            for spec in plan["artifacts"]:
                if payload._verify_name(directory, spec):
                    disposition = "cas-hit"
                else:
                    payload._store_stream(directory, spec, stream_factory(spec))
                    if not payload._verify_name(directory, spec):
                        raise RustDistAcquisitionError("rustdist artifact is absent after fetch")
                    disposition = "fetched"
                    fetched_bytes += int(spec["sizeBytes"])
                rows.append(
                    {
                        "artifactId": spec["artifactId"],
                        "disposition": disposition,
                        "sha256": spec["sha256"],
                        "sizeBytes": spec["sizeBytes"],
                    }
                )
        finally:
            os.close(directory)
    return {
        "artifacts": sorted(rows, key=lambda row: str(row["artifactId"])),
        "fetchedBytes": fetched_bytes,
    }


def build_result(plan: dict[str, Any], acquired: dict[str, Any]) -> dict[str, Any]:
    rows = acquired["artifacts"]
    for row in rows:
        if set(row) != RESULT_ARTIFACT_KEYS:
            raise RustDistAcquisitionError("rustdist result artifact keys differ")
        if row["disposition"] not in ("cas-hit", "fetched"):
            raise RustDistAcquisitionError("rustdist result disposition is unknown")
    return {
        "activationAllowed": False,
        "artifacts": rows,
        "boundaries": dict(plan["boundaries"]),
        "bootableClaim": False,
        "casHitCount": sum(1 for row in rows if row["disposition"] == "cas-hit"),
        "fetchedBytes": acquired["fetchedBytes"],
        "fetchedCount": sum(1 for row in rows if row["disposition"] == "fetched"),
        "planSha256": PLAN_SHA256,
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "status": RESULT_STATUS,
        "totalBytes": sum(int(row["sizeBytes"]) for row in rows),
        "verifiedCount": len(rows),
    }


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--check",
        action="store_true",
        help="validate the pre-registered plan and CAS state without any network request",
    )
    group.add_argument(
        "--acquire",
        action="store_true",
        help="verify or fetch the frozen archives and write the result document once",
    )
    args = parser.parse_args(argv)

    plan = load_plan()
    if args.check:
        directory = payload._open_sha_directory(CAS_ROOT, create=True)
        try:
            present = sorted(
                str(spec["artifactId"])
                for spec in plan["artifacts"]
                if payload._verify_name(directory, spec)
            )
        finally:
            os.close(directory)
        missing = sorted(
            str(spec["artifactId"]) for spec in plan["artifacts"] if str(spec["artifactId"]) not in present
        )
        print(
            "rustdist plan: "
            f"{PLAN_SHA256} artifacts={len(plan['artifacts'])} "
            f"present={present} missing={missing}"
        )
        return 0

    acquired = acquire(plan)
    result = build_result(plan, acquired)
    payload._write_result_once(RESULT_PATH, canonical_json(result))
    print(
        "rustdist acquisition: "
        f"{result['status']} verified={result['verifiedCount']} "
        f"fetched={result['fetchedCount']} casHits={result['casHitCount']} "
        f"bytes={result['totalBytes']}"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover - exercised through main()
    raise SystemExit(main())
