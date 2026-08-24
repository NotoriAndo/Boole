#!/usr/bin/env python3
"""Append-only Linux/arm64 successor for the frozen native-shadow builder.

The MAC.0 x86 builder is itself part of the authority and therefore cannot be
edited.  This module verifies those exact legacy bytes, applies a small reviewed
architecture projection in memory, and exposes the resulting builder.  The
wrapper bytes and the frozen legacy digest together identify the successor.
"""

from __future__ import annotations

import hashlib
import pathlib
import sys
from typing import Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))


LEGACY = pathlib.Path(__file__).with_name("native_shadow_rootfs_builder.py")
LEGACY_SHA256 = "aa25701a8a29cfb0059c911a5df8dcc2f09c8b4c61b4ff46adfc0ef446cdf689"

REPLACEMENTS = (
    ("boole.native-shadow.runtime-rootfs-source-lock.v1", "boole.native-shadow.runtime-rootfs-source-lock.arm64.runtime.v1", 1),
    ("boole.native-shadow.runtime-rootfs-build-receipt.v1", "boole.native-shadow.runtime-rootfs-build-receipt.arm64.v1", 2),
    ("boole.native-shadow.rootfs-content-manifest.v1", "boole.native-shadow.rootfs-content-manifest.arm64.v1", 2),
    ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", 6),
    ("linuxX8664ArtifactSha256", "linuxArm64ArtifactSha256", 1),
    ("amd64", "arm64", 10),
    (
        "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
        "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST-arm64-v1.json",
        1,
    ),
    (
        "native/containment/native-shadow-execution-policy-v1.json",
        "native/containment/native-shadow-execution-policy-arm64-v1.json",
        1,
    ),
    (
        "native/containment/native-shadow-toolchain-identity-v1.json",
        "native/containment/native-shadow-toolchain-identity-arm64-v1.json",
        1,
    ),
)


def _derived_source() -> str:
    raw = LEGACY.read_bytes()
    if hashlib.sha256(raw).hexdigest() != LEGACY_SHA256:
        raise RuntimeError("frozen x86 rootfs builder bytes differ")
    source = raw.decode("utf-8")
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise RuntimeError(
                f"arm64 builder projection count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    for forbidden in ("x86_64-unknown-linux-gnu", '"amd64"'):
        if forbidden in source:
            raise RuntimeError(f"arm64 builder retains x86 authority: {forbidden}")
    return source


_IMPL: dict[str, object] = {
    "__name__": "scripts.native_shadow_rootfs_builder_arm64_v1_impl",
    "__file__": __file__,
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

RootfsBuildError = _IMPL["RootfsBuildError"]
BUILDER_SHA256 = _IMPL["BUILDER_SHA256"]
canonical_json = _IMPL["canonical_json"]
load_json_exact = _IMPL["load_json_exact"]
validate_source_lock = _IMPL["validate_source_lock"]
build_oci_layout = _IMPL["build_oci_layout"]
verify_oci_layout = _IMPL["verify_oci_layout"]
directory_digest = _IMPL["directory_digest"]


def __getattr__(name: str) -> object:
    """Expose the projected legacy helpers to the projected acquirer."""

    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
