#!/usr/bin/env python3
"""Successor projection of the boot release gate: the successor lock is a build input.

``materialize_runtime_lock`` is the one step between a sealed source lock and a
builder input, and it accepts exactly one release string.  Its predecessor
projection names the predecessor boot lock's release, so the sealed successor lock
is refused there -- before the builder whose table was widened for it is ever
reached.

This module moves that one string and the builder the chain arrives at.  Nothing
else changes: the lock must still be canonical exact, must still declare
``activationAllowed: false``, and must still carry the ``gpgv`` and ``zstd`` tool
roles.  Widening which lock is accepted is not accepting both, so the predecessor
release is refused here exactly as the successor release is refused there.
"""

from __future__ import annotations

import hashlib
import pathlib
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_portable_boot_arm64_v1 as portable_v1
from scripts.native_shadow_rootfs_builder_boot_arm64_v2 import BOOT_SOURCE_LOCK_RELEASE


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

BOOT_PORTABLE_V1 = pathlib.Path(portable_v1.__file__).resolve()
BOOT_PORTABLE_V1_SHA256 = (
    "4598e73f9389f41d739edb59660b69b99376a7be1788af24406a58b64d6e0a62"
)

REPLACEMENTS = (
    (
        "native_shadow_rootfs_builder_boot_arm64_v1",
        "native_shadow_rootfs_builder_boot_arm64_v2",
        3,
    ),
    (
        portable_v1.SOURCE_LOCK_RELEASE,
        BOOT_SOURCE_LOCK_RELEASE,
        1,
    ),
)


class BootPortableSuccessorError(RuntimeError):
    """The predecessor release gate is not the one this successor expects."""


def _derived_source() -> str:
    raw = BOOT_PORTABLE_V1.read_bytes()
    if hashlib.sha256(raw).hexdigest() != BOOT_PORTABLE_V1_SHA256:
        raise BootPortableSuccessorError("boot portable projection bytes differ")
    source = portable_v1._derived_source()
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise BootPortableSuccessorError(
                f"boot portable successor count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_portable_boot_arm64_v2_impl",
    "__file__": __file__,
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

# Inherited by identity rather than recomputed: the predecessor already dropped
# the x86-only loader alias, and the successor must carry the same alias set.
_IMPL["PORTABLE_V2_DERIVED_ENTRIES"] = portable_v1.__getattr__("PORTABLE_V2_DERIVED_ENTRIES")

PortableAuthorityError = _IMPL["PortableAuthorityError"]
SOURCE_LOCK_RELEASE = _IMPL["SOURCE_LOCK_RELEASE"]
SOURCE_LOCK_SCHEMA = _IMPL["SOURCE_LOCK_SCHEMA"]
materialize_runtime_lock = _IMPL["materialize_runtime_lock"]

SUCCESSOR_PROJECTION_SHA256 = hashlib.sha256(
    pathlib.Path(__file__).resolve().read_bytes()
).hexdigest()


def __getattr__(name: str) -> Any:
    try:
        return _IMPL[name]
    except KeyError as exc:
        raise AttributeError(name) from exc


def main(argv: Optional[list[str]] = None) -> int:
    return _IMPL["main"](argv)


if __name__ == "__main__":
    raise SystemExit(main())
