#!/usr/bin/env python3
"""Independent Linux/arm64 projection of the frozen OCI verifier."""

from __future__ import annotations

import hashlib
import pathlib
import sys
import types
from typing import Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

LEGACY = pathlib.Path(__file__).with_name("native_shadow_rootfs_oci_verify.py")
LEGACY_SHA256 = "150b9cf5a37d2841c1c8132a08ac072256fbce39387a689f3601bbaacfd0ed93"
REPLACEMENTS = (
    (
        "boole.native-shadow.runtime-rootfs-build-receipt.v1",
        "boole.native-shadow.runtime-rootfs-build-receipt.arm64.v1",
        1,
    ),
    (
        "boole.native-shadow.rootfs-content-manifest.v1",
        "boole.native-shadow.rootfs-content-manifest.arm64.v1",
        1,
    ),
    ('"amd64"', '"arm64"', 2),
)


def _derived_source() -> str:
    raw = LEGACY.read_bytes()
    if hashlib.sha256(raw).hexdigest() != LEGACY_SHA256:
        raise RuntimeError("frozen x86 OCI verifier bytes differ")
    source = raw.decode("utf-8")
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise RuntimeError(
                f"arm64 OCI verifier projection count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


_MODULE_NAME = "scripts.native_shadow_rootfs_oci_verify_arm64_v1_impl"
_MODULE = types.ModuleType(_MODULE_NAME)
_MODULE.__file__ = __file__
_MODULE.__package__ = "scripts"
sys.modules[_MODULE_NAME] = _MODULE
exec(
    compile(_derived_source(), __file__ + "<derived>", "exec"),
    _MODULE.__dict__,
)
_IMPL: dict[str, object] = _MODULE.__dict__

OciVerificationError = _IMPL["OciVerificationError"]
verify_layout = _IMPL["verify_layout"]


def __getattr__(name: str) -> object:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
