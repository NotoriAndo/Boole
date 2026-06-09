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

    def test_third_party_actions_are_sha_pinned(self) -> None:
        # P0.7 (audit) — third-party actions must be pinned to an immutable
        # 40-hex commit SHA, never a mutable branch/tag (a `@master` ref is
        # arbitrary-code-execution on the next upstream push).
        body = _read(CI_WORKFLOW)
        self.assertNotIn(
            "dtolnay/rust-toolchain@master",
            body,
            "P0.7: dtolnay/rust-toolchain must be SHA-pinned, not @master",
        )
        self.assertRegex(
            body,
            re.compile(r"dtolnay/rust-toolchain@[0-9a-f]{40}\b"),
            "P0.7: dtolnay/rust-toolchain must be pinned to a 40-hex commit SHA",
        )

    def test_curled_installers_are_integrity_checked(self) -> None:
        # P0.7 (audit) — binaries/scripts fetched by curl must be pinned to an
        # immutable ref or verified by checksum, not fetched from a mutable
        # branch tip without integrity.
        body = _read(CI_WORKFLOW)
        self.assertNotIn(
            "leanprover/elan/master/elan-init.sh",
            body,
            "P0.7: elan-init.sh must be fetched from an immutable tag, not master",
        )
        self.assertIn(
            "sha256sum -c",
            body,
            "P0.7: the gitleaks download must be sha256-verified before use",
        )

    def test_ci_builds_release_binaries(self) -> None:
        # P0.7 (audit) — the self-test gate only exercises the dev profile, so
        # CI must additionally build the shipped binaries in release; otherwise
        # the release profile (panic = "abort", optimizations) never runs until
        # production.
        body = _read(CI_WORKFLOW)
        self.assertRegex(
            body,
            re.compile(r"cargo build --release", re.MULTILINE),
            "P0.7: ci.yml must build the release profile so panic=abort and "
            "release optimizations are exercised before shipping",
        )


if __name__ == "__main__":
    unittest.main()
