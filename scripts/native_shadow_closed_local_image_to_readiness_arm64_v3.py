#!/usr/bin/env python3
"""Current image lane for PrivateTmp plus the proxy relay service.

The v2 implementation is hash-bound by the failed MAC.4 observation and stays
byte-for-byte historical.  Its first workflow invocation exposed one wrapper
defect before any image work began: ``python -I -S path/to/script.py`` does not
put the repository root on ``sys.path``.  This additive entry point supplies
that explicit root.  This current lane also replaces the historical AF_VSOCK-
only relay unit with the reviewed AF_VSOCK+AF_UNIX unit.  The v1 and v2 lanes
remain byte-for-byte historical.
"""

from __future__ import annotations

import pathlib
import sys
import contextlib
from typing import Any, Mapping, Optional


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v2 as v2


RELAY_SERVICE_SOURCE = "native/systemd/boole-native-shadow-mac4-relay-v2.service"
RELAY_SERVICE_SHA256 = (
    "738de8e46a3f452acbdd2ec14b1b4d4946d7fc0e6b0648d7372487923eb3a2bb"
)
RELAY_SERVICE_SIZE_BYTES = 1_032

_HISTORICAL_SERVICE_SOURCE = v2.v1.MAC4_SERVICE_SOURCE
_HISTORICAL_SERVICE_SHA256 = v2.v1.MAC4_SERVICE_SHA256
_HISTORICAL_SERVICE_SIZE_BYTES = v2.v1.MAC4_SERVICE_SIZE_BYTES


@contextlib.contextmanager
def _proxy_relay_service_contract():
    """Patch only the current lane and restore the sealed predecessor."""

    if (
        v2.v1.MAC4_SERVICE_SOURCE != _HISTORICAL_SERVICE_SOURCE
        or v2.v1.MAC4_SERVICE_SHA256 != _HISTORICAL_SERVICE_SHA256
        or v2.v1.MAC4_SERVICE_SIZE_BYTES != _HISTORICAL_SERVICE_SIZE_BYTES
    ):
        raise v2.v1.ClosedLocalImageError(
            "closed-local relay service predecessor is already patched"
        )
    v2.v1.MAC4_SERVICE_SOURCE = RELAY_SERVICE_SOURCE
    v2.v1.MAC4_SERVICE_SHA256 = RELAY_SERVICE_SHA256
    v2.v1.MAC4_SERVICE_SIZE_BYTES = RELAY_SERVICE_SIZE_BYTES
    try:
        yield
    finally:
        v2.v1.MAC4_SERVICE_SIZE_BYTES = _HISTORICAL_SERVICE_SIZE_BYTES
        v2.v1.MAC4_SERVICE_SHA256 = _HISTORICAL_SERVICE_SHA256
        v2.v1.MAC4_SERVICE_SOURCE = _HISTORICAL_SERVICE_SOURCE


def _decorate_proxy_relay(document: Mapping[str, Any]) -> dict[str, Any]:
    decorated = dict(document)
    decorated["proxyRelayServiceSuccessor"] = {
        "activationAllowed": False,
        "sha256": RELAY_SERVICE_SHA256,
        "source": RELAY_SERVICE_SOURCE,
    }
    return decorated


def build(*, result: pathlib.Path, run_label: str, **kwargs: Any) -> dict[str, Any]:
    """Capture the predecessor receipt, decorate it, and publish only once."""

    destination = pathlib.Path(result)
    real_publish = v2.v1._publish_result
    captured: list[dict[str, Any]] = []

    def capture(path: pathlib.Path, document: Mapping[str, Any]) -> None:
        if pathlib.Path(path) != destination:
            raise v2.v1.ClosedLocalImageError(
                "closed-local predecessor published to an unexpected result path"
            )
        if captured:
            raise v2.v1.ClosedLocalImageError(
                "closed-local predecessor published more than one result"
            )
        captured.append(dict(document))

    with v2._successor_contract(), _proxy_relay_service_contract():
        v2.v1._publish_result = capture
        try:
            predecessor = v2.v1.build(
                result=destination,
                run_label=run_label,
                **kwargs,
            )
        finally:
            v2.v1._publish_result = real_publish

    if len(captured) != 1 or captured[0] != dict(predecessor):
        raise v2.v1.ClosedLocalImageError(
            "closed-local predecessor result capture differs"
        )
    document = _decorate_proxy_relay(v2._decorate(predecessor))
    real_publish(destination, document)
    return document


def main(argv: Optional[list[str]] = None) -> int:
    options = v2.v1._parser().parse_args(argv)
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
            with _proxy_relay_service_contract():
                document = _decorate_proxy_relay(v2.preflight(**common))
            v2.v1._publish_result(options.result, document)
        else:
            build(
                result=options.result,
                run_label=options.run_label,
                **common,
            )
        print(
            "native-shadow closed-local image-to-readiness isolated successor: "
            f"{options.mode} PASS"
        )
        return 0
    except v2.v1.ClosedLocalImageError as exc:
        print(
            "native-shadow closed-local image-to-readiness isolated successor: "
            f"FAIL: {exc}",
            file=v2.v1.sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
