#!/usr/bin/env python3
"""Check actual Cargo artifacts, not just the requested manifest settings."""
from __future__ import annotations

import argparse
import json
import sys
from typing import Iterable

EXPECTED = {"curve25519_dalek": "3", "boole_core": "0", "boole_node": "0"}


def check_profiles(lines: Iterable[str]) -> None:
    seen = set()
    finished = False
    for line in lines:
        # run_logged combines Cargo JSON stdout with ordinary progress stderr.
        if not line.lstrip().startswith("{"):
            continue
        try:
            record = json.loads(line)
        except ValueError as error:
            raise ValueError("malformed Cargo JSON record") from error
        if not isinstance(record, dict):
            raise ValueError("Cargo record is not an object")
        if record.get("reason") == "build-finished":
            if record.get("success") is not True:
                raise ValueError("Cargo build did not succeed")
            finished = True
        if record.get("reason") != "compiler-artifact":
            continue
        target = record.get("target", {})
        name = target.get("name")
        if name not in EXPECTED or "lib" not in target.get("kind", []):
            continue
        profile = record.get("profile", {})
        if (
            profile.get("opt_level") != EXPECTED[name]
            or profile.get("debug_assertions") is not True
            or profile.get("overflow_checks") is not True
        ):
            raise ValueError(f"unexpected test compiler profile for {name}")
        seen.add(name)
    if not finished or seen != set(EXPECTED):
        raise ValueError("missing successful build or required library artifacts")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", help="Cargo JSON build log, or - for stdin")
    args = parser.parse_args()
    try:
        if args.log == "-":
            check_profiles(sys.stdin)
        else:
            with open(args.log, encoding="utf-8") as stream:
                check_profiles(stream)
    except (OSError, ValueError) as error:
        print(f"cargo test profile: FAIL ({error})", file=sys.stderr)
        return 1
    print("cargo test profile: PASS (curve=3, core/node=0; debug/overflow checks ON)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
