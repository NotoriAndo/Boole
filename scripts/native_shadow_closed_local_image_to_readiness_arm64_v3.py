#!/usr/bin/env python3
"""Isolated-CLI entry point for the read-only PrivateTmp successor lane.

The v2 implementation is hash-bound by the failed MAC.4 observation and stays
byte-for-byte historical.  Its first workflow invocation exposed one wrapper
defect before any image work began: ``python -I -S path/to/script.py`` does not
put the repository root on ``sys.path``.  This additive entry point supplies
that explicit root, then delegates without changing the v2 image contract.
"""

from __future__ import annotations

import pathlib
import sys
from typing import Any, Mapping, Optional


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v2 as v2


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

    with v2._successor_contract():
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
    document = v2._decorate(predecessor)
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
            document = v2.preflight(**common)
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
