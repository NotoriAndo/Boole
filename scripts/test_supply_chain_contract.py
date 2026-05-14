#!/usr/bin/env python3
"""Regression tests for P0.7: CI must enforce supply-chain gates via
`cargo deny check` and `cargo audit`. The L0 contract requires both as
required CI jobs so a malicious or vulnerable dependency can never silently
ride into a release.

These tests pin the presence of a `deny.toml` policy and the CI workflow
steps that invoke the tools."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DENY_TOML = ROOT / "deny.toml"
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class SupplyChainContractTests(unittest.TestCase):
    def test_deny_toml_exists(self) -> None:
        self.assertTrue(
            DENY_TOML.is_file(),
            f"P0.7: deny.toml policy file must exist at {DENY_TOML}",
        )

    def test_deny_toml_has_advisories_section(self) -> None:
        body = _read(DENY_TOML)
        self.assertRegex(
            body,
            re.compile(r"^\[advisories\]", re.MULTILINE),
            "P0.7: deny.toml must declare `[advisories]` so vulnerable "
            "transitive deps are caught",
        )

    def test_deny_toml_has_bans_section(self) -> None:
        body = _read(DENY_TOML)
        self.assertRegex(
            body,
            re.compile(r"^\[bans\]", re.MULTILINE),
            "P0.7: deny.toml must declare `[bans]` so duplicate-version and "
            "wildcard-version regressions are blocked",
        )

    def test_ci_runs_cargo_deny(self) -> None:
        body = _read(CI_WORKFLOW)
        self.assertRegex(
            body,
            re.compile(r"cargo[- ]deny\s+check", re.MULTILINE),
            "P0.7: ci.yml must invoke `cargo deny check` as a required job",
        )

    def test_ci_runs_cargo_audit(self) -> None:
        body = _read(CI_WORKFLOW)
        self.assertRegex(
            body,
            re.compile(r"cargo[- ]audit\b", re.MULTILINE),
            "P0.7: ci.yml must invoke `cargo audit` as a required job",
        )


if __name__ == "__main__":
    unittest.main()
