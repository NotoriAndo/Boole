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
from typing import Optional


REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v2 as v2


def main(argv: Optional[list[str]] = None) -> int:
    return v2.main(argv)


if __name__ == "__main__":
    raise SystemExit(main())
