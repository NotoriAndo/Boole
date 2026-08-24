#!/usr/bin/env python3
"""Append-only arm64 projection of the frozen native-shadow acquirer."""

from __future__ import annotations

import hashlib
import pathlib
import re
import sys
from typing import Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))


EXPECTED_PLAN_SHA256 = "bfc11d201ab72b42afa218948e89bc48e3555aa2ed79f4657c9d3a4b37dfd331"
LEGACY = pathlib.Path(__file__).with_name("native_shadow_rootfs_acquire.py")
LEGACY_SHA256 = "31348981687939ff7cf63b5584947b3e09a92bb35f9f4e76f78a657ae139d49b"

REPLACEMENTS = (
    ("native_shadow_rootfs_builder", "native_shadow_rootfs_builder_arm64_v1", 3),
    ("boole.native-shadow.runtime-rootfs-acquisition-plan.v1", "boole.native-shadow.runtime-rootfs-acquisition-plan.arm64.v1", 1),
    ("boole.native-shadow.runtime-rootfs-resolution.v1", "boole.native-shadow.runtime-rootfs-resolution.arm64.runtime.v1", 1),
    ("NATIVE-SHADOW-RUNTIME-ROOTFS-ACQUISITION-PLAN-V1-QUALIFICATION", "NATIVE-SHADOW-RUNTIME-ROOTFS-ACQUISITION-PLAN-ARM64-V1-QUALIFICATION", 1),
    ("binutils-x86-64-linux-gnu", "binutils-aarch64-linux-gnu", 1),
    ("gcc-13-x86-64-linux-gnu", "gcc-13-aarch64-linux-gnu", 1),
    ("x86_64-linux-gnu", "aarch64-linux-gnu", 1),
    ("binary-amd64", "binary-arm64", 2),
    ("amd64", "arm64", 3),
)


def _derived_source() -> str:
    raw = LEGACY.read_bytes()
    if hashlib.sha256(raw).hexdigest() != LEGACY_SHA256:
        raise RuntimeError("frozen x86 rootfs acquirer bytes differ")
    source = raw.decode("utf-8")
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise RuntimeError(
                f"arm64 acquirer projection count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


_IMPL: dict[str, object] = {
    "__name__": "scripts.native_shadow_rootfs_acquire_arm64_v1_impl",
    "__file__": __file__,
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)
_IMPL["EXPECTED_PLAN_SHA256"] = EXPECTED_PLAN_SHA256

AcquisitionError = _IMPL["AcquisitionError"]
acquirer_authority_sha256 = _IMPL["acquirer_authority_sha256"]
load_plan = _IMPL["load_plan"]
resolve_package_closure = _IMPL["resolve_package_closure"]
resolve_from_cas = _IMPL["resolve_from_cas"]
seal_candidate = _IMPL["seal_candidate"]


def __getattr__(name: str) -> object:
    """Expose projected acquisition helpers to the portable successor."""

    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
