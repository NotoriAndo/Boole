#!/usr/bin/env python3
"""Keep the audit's six formerly-unregistered tests consciously classified."""
from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SELF_TEST = ROOT / "scripts" / "self-test.sh"
EXCLUSIONS = ROOT / "scripts" / "test-registration-exclusions.txt"
AUDITED = {
    "scripts/test_boole_testkit_contract.py",
    "scripts/test_clap_derive_contract.py",
    "scripts/test_public_benchmark_artifacts.py",
    "scripts/test_secret_redactor_contract.py",
    "scripts/test_telemetry_contract.py",
    "scripts/test_verified_answer_closeout.py",
}


class AuditTestRegistrationTests(unittest.TestCase):
    def test_every_audited_test_is_registered_or_has_an_explicit_reason(self) -> None:
        registered = SELF_TEST.read_text(encoding="utf-8")
        self.assertIn("scripts/test_self_test_prewarm.py", registered)
        exclusions: dict[str, str] = {}
        for line in EXCLUSIONS.read_text(encoding="utf-8").splitlines():
            if not line or line.startswith("#"):
                continue
            path, reason = (part.strip() for part in line.split("|", 1))
            exclusions[path] = reason
        self.assertEqual(set(exclusions), {"scripts/test_verified_answer_closeout.py"})
        self.assertTrue(exclusions["scripts/test_verified_answer_closeout.py"])
        for path in AUDITED:
            registered_here = path in registered
            excluded_here = path in exclusions
            self.assertNotEqual(
                registered_here,
                excluded_here,
                f"{path} must be in exactly one of self-test registration or explicit exclusion",
            )


if __name__ == "__main__":
    unittest.main()
