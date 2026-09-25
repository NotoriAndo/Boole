#!/usr/bin/env python3
"""Fail-closed consumer tests for Cargo's emitted compiler-artifact profiles."""
from __future__ import annotations

import json
import unittest

from scripts.check_cargo_test_profile import check_profiles


def artifact(name: str, opt_level: str) -> dict:
    return {
        "reason": "compiler-artifact",
        "target": {"name": name, "kind": ["lib"]},
        "profile": {
            "opt_level": opt_level,
            "debuginfo": 2,
            "debug_assertions": True,
            "overflow_checks": True,
            "test": False,
        },
    }


def records() -> list:
    return [
        artifact("curve25519_dalek", "3"),
        artifact("boole_core", "0"),
        artifact("boole_node", "0"),
        {"reason": "build-finished", "success": True},
    ]


class CargoTestProfileTests(unittest.TestCase):
    def test_optimized_curve_keeps_application_debug_and_overflow_guards(self) -> None:
        check_profiles(["   Compiling boole-node\n", *map(json.dumps, records())])
        for index in range(3):
            for field, wrong in (
                ("opt_level", "0" if index == 0 else "3"),
                ("debug_assertions", False),
                ("overflow_checks", False),
            ):
                with self.subTest(index=index, field=field):
                    changed = records()
                    changed[index]["profile"][field] = wrong
                    with self.assertRaisesRegex(ValueError, "unexpected test compiler profile"):
                        check_profiles(map(json.dumps, changed))

    def test_missing_library_build_failure_or_conflicting_duplicate_cannot_pass(self) -> None:
        for index in range(4):
            incomplete = records()
            del incomplete[index]
            with self.subTest(missing=index), self.assertRaises(ValueError):
                check_profiles(map(json.dumps, incomplete))
        failed = records()
        failed[-1]["success"] = False
        with self.assertRaisesRegex(ValueError, "did not succeed"):
            check_profiles(map(json.dumps, failed))
        with self.assertRaisesRegex(ValueError, "unexpected test compiler profile"):
            check_profiles(map(json.dumps, records() + [artifact("boole_node", "3")]))
        build_script = records()
        build_script[0]["target"]["kind"] = ["custom-build"]
        with self.assertRaisesRegex(ValueError, "missing successful build"):
            check_profiles(map(json.dumps, build_script))
        with self.assertRaisesRegex(ValueError, "malformed Cargo JSON"):
            check_profiles(["{broken-json"])


if __name__ == "__main__":
    unittest.main()
