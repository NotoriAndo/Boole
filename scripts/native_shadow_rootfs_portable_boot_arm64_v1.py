#!/usr/bin/env python3
"""Boot projection of the frozen arm64 portable rootfs authority.

``materialize_runtime_lock`` is the one step between a sealed source lock and a
builder input: it binds the replay host's ``gpgv`` and ``zstd`` into an
ephemeral copy and records their digests in a receipt.  That logic is frozen and
worth keeping frozen, so this module reprojects it rather than reimplementing
it, changing exactly two things.

The accepted release string becomes the boot lock's own, because the sealed boot
lock is a different release from the portable one and says so in its name.  And
the builder the chain reaches is the boot projection, which is the only builder
whose tables are wide enough to read this lock.

Neither change touches what ``materialize_runtime_lock`` verifies: the lock must
still be canonical exact, must still declare ``activationAllowed: false``, and
must still carry the ``gpgv`` and ``zstd`` tool roles.  The runtime lock it
returns is not source authority here either.
"""

from __future__ import annotations

import hashlib
import pathlib
import sys
from typing import Any, Optional


sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_rootfs_portable_arm64_v1 as portable
from scripts.native_shadow_rootfs_builder_boot_arm64_v1 import BOOT_SOURCE_LOCK_RELEASE


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

ARM64_PORTABLE = pathlib.Path(portable.__file__).resolve()
ARM64_PORTABLE_SHA256 = "67ef26fecbbeaeaf38cf081388b99dab37f9b6417c9e699738a81ab5e45f9cb7"

REPLACEMENTS = (
    (
        "native_shadow_rootfs_builder_arm64_v1",
        "native_shadow_rootfs_builder_boot_arm64_v1",
        3,
    ),
    (
        "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-ARM64-V1-PORTABLE-NOT-ACTIVATABLE",
        BOOT_SOURCE_LOCK_RELEASE,
        1,
    ),
)


class BootPortableProjectionError(RuntimeError):
    """The frozen portable authority is not the one this projection expects."""


def _derived_source() -> str:
    raw = ARM64_PORTABLE.read_bytes()
    if hashlib.sha256(raw).hexdigest() != ARM64_PORTABLE_SHA256:
        raise BootPortableProjectionError("frozen arm64 portable authority bytes differ")
    source = portable._derived_source()
    for old, new, expected_count in REPLACEMENTS:
        actual_count = source.count(old)
        if actual_count != expected_count:
            raise BootPortableProjectionError(
                f"boot portable projection count differs for {old!r}: "
                f"{actual_count} != {expected_count}"
            )
        source = source.replace(old, new)
    return source


_IMPL: dict[str, Any] = {
    "__name__": "scripts.native_shadow_rootfs_portable_boot_arm64_v1_impl",
    "__file__": __file__,
    "__package__": "scripts",
}
exec(compile(_derived_source(), __file__ + "<derived>", "exec"), _IMPL)

# Linux/arm64 Noble has no /usr/lib64, exactly as in the portable successor this
# projects.  Carried forward so the two paths keep the same alias set.
_IMPL["PORTABLE_V2_DERIVED_ENTRIES"] = tuple(
    row for row in _IMPL["PORTABLE_V2_DERIVED_ENTRIES"] if row["logicalPath"] != "/lib64"
)

PortableAuthorityError = _IMPL["PortableAuthorityError"]
SOURCE_LOCK_RELEASE = _IMPL["SOURCE_LOCK_RELEASE"]
SOURCE_LOCK_SCHEMA = _IMPL["SOURCE_LOCK_SCHEMA"]
materialize_runtime_lock = _IMPL["materialize_runtime_lock"]

BOOT_PROJECTION_SHA256 = hashlib.sha256(
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
