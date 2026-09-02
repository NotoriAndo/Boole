#!/usr/bin/env python3
"""Successor image lane that supplies PrivateTmp on a read-only guest root.

The predecessor lane is already hash-bound by its zero-image result and remains
byte-for-byte historical.  This wrapper narrows one observed boot defect: the
MAC.4 relay unit uses ``PrivateTmp=yes``, which requires both ``/tmp`` and
``/var/tmp`` to exist before systemd creates its mount namespace.  The sealed
root is read-only, so systemd cannot repair an absent ``/var/tmp`` at boot.
"""

from __future__ import annotations

import contextlib
import hashlib
import pathlib
from typing import Any, Mapping, Optional

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as v1


PREDECESSOR_PATH = pathlib.Path(v1.__file__).resolve()
PREDECESSOR_SHA256 = "8d8b8d4e4391271620133514db0fd021b78734a754515d763fd2c16c012a23aa"
PREDECESSOR_SIZE_BYTES = 54_801
PRIVATE_TMP_PATH = "var/tmp"
PRIVATE_TMP_MODE = 0o1777

_PREDECESSOR_MAC4_ENTRIES = v1._development_mac4_entries
_PREDECESSOR_OVERLAY_PATHS = v1.MAC4_OVERLAY_PATHS
_PREDECESSOR_READBACK_EFFECTS = v1.DevelopmentAutoclearReadbackEffects


def _verify_predecessor() -> None:
    raw = PREDECESSOR_PATH.read_bytes()
    if len(raw) != PREDECESSOR_SIZE_BYTES or hashlib.sha256(raw).hexdigest() != PREDECESSOR_SHA256:
        raise v1.ClosedLocalImageError("closed-local image predecessor differs from its successor pin")


def _successor_mac4_entries(
    repository_root: pathlib.Path, relay_binary: bytes
) -> dict[str, dict[str, Any]]:
    entries = _PREDECESSOR_MAC4_ENTRIES(repository_root, relay_binary)
    if PRIVATE_TMP_PATH in entries:
        raise v1.ClosedLocalImageError("MAC.4 predecessor already stages private temporary directory")
    return {
        **entries,
        PRIVATE_TMP_PATH: {
            "path": PRIVATE_TMP_PATH,
            "kind": "directory",
            "mode": PRIVATE_TMP_MODE,
            "uid": 0,
            "gid": 0,
        },
    }


class SuccessorAutoclearReadbackEffects(_PREDECESSOR_READBACK_EFFECTS):
    """Require the exact sticky directory in the mounted-image consumer."""

    def read_tree(self, mountpoint: pathlib.Path) -> dict[str, dict[str, Any]]:
        tree = super().read_tree(mountpoint)
        row = tree.get("/" + PRIVATE_TMP_PATH)
        if (
            not isinstance(row, Mapping)
            or row.get("kind") != "directory"
            or row.get("mode") != PRIVATE_TMP_MODE
            or row.get("uid") != 0
            or row.get("gid") != 0
        ):
            raise v1.ClosedLocalImageError(
                "MAC.4 private temporary directory differs"
            )
        return tree


@contextlib.contextmanager
def _successor_contract():
    """Scope the additive successor to one call and restore the predecessor."""

    _verify_predecessor()
    if (
        v1._development_mac4_entries is not _PREDECESSOR_MAC4_ENTRIES
        or v1.MAC4_OVERLAY_PATHS != _PREDECESSOR_OVERLAY_PATHS
        or v1.DevelopmentAutoclearReadbackEffects is not _PREDECESSOR_READBACK_EFFECTS
    ):
        raise v1.ClosedLocalImageError("closed-local image predecessor is already patched")
    v1._development_mac4_entries = _successor_mac4_entries
    v1.MAC4_OVERLAY_PATHS = (*_PREDECESSOR_OVERLAY_PATHS, PRIVATE_TMP_PATH)
    v1.DevelopmentAutoclearReadbackEffects = SuccessorAutoclearReadbackEffects
    try:
        yield
    finally:
        v1.DevelopmentAutoclearReadbackEffects = _PREDECESSOR_READBACK_EFFECTS
        v1.MAC4_OVERLAY_PATHS = _PREDECESSOR_OVERLAY_PATHS
        v1._development_mac4_entries = _PREDECESSOR_MAC4_ENTRIES


def _decorate(document: Mapping[str, Any]) -> dict[str, Any]:
    decorated = dict(document)
    decorated["privateTmpSuccessor"] = {
        "activationAllowed": False,
        "mode": "1777",
        "path": "/" + PRIVATE_TMP_PATH,
        "predecessorSha256": PREDECESSOR_SHA256,
        "reason": "systemd PrivateTmp needs /tmp and /var/tmp before namespacing a read-only root",
    }
    return decorated


def preflight(**kwargs: Any) -> dict[str, Any]:
    with _successor_contract():
        return _decorate(v1.preflight(**kwargs))


def build(*, result: pathlib.Path, run_label: str, **kwargs: Any) -> dict[str, Any]:
    with _successor_contract():
        document = _decorate(
            v1.build(result=result, run_label=run_label, **kwargs)
        )
    v1._publish_result(result, document)
    return document


def main(argv: Optional[list[str]] = None) -> int:
    options = v1._parser().parse_args(argv)
    common = {
        "repository_root": options.repository_root,
        "artifact_store": options.cas,
        "outputs": options.outputs,
        "scratch": options.scratch,
        "gpgv": options.gpgv,
        "zstd": options.zstd,
        "launcher": options.launcher,
        "mac4_relay": options.mac4_relay,
        "depmod": options.depmod,
    }
    try:
        if options.mode == "preflight":
            document = preflight(**common)
            v1._publish_result(options.result, document)
        else:
            document = build(
                result=options.result,
                run_label=options.run_label,
                **common,
            )
        print(
            "native-shadow closed-local image-to-readiness successor: "
            f"{options.mode} PASS"
        )
        return 0
    except v1.ClosedLocalImageError as exc:
        print(
            "native-shadow closed-local image-to-readiness successor: "
            f"FAIL: {exc}",
            file=v1.sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
