#!/usr/bin/env python3
"""Regression tests for P0.1a: the `boole-testkit` crate exists, is a workspace
member, and exports the L10-mandated shared helpers `rand_suffix`, `repo_root`,
and `lake_and_lean_available`. At least one external test file must import
from the crate so we know the contract has a live caller (master plan §0.1
rule 4 — extraction is only justified once a real call site adopts it).

Later P0.1 slices will expand this contract to TempStateDir, start_node,
FixtureCatalog, MockBountyVerifier, MockSubmitter, MockChainHead. This file
locks the minimal first slice so the helper crate cannot regress to a
no-callsite shell."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_TOML = ROOT / "Cargo.toml"
TESTKIT_DIR = ROOT / "crates" / "boole-testkit"
TESTKIT_TOML = TESTKIT_DIR / "Cargo.toml"
TESTKIT_LIB = TESTKIT_DIR / "src" / "lib.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class BooleTestkitContractTests(unittest.TestCase):
    def test_crate_directory_exists(self) -> None:
        self.assertTrue(
            TESTKIT_DIR.is_dir(),
            f"P0.1a: crates/boole-testkit/ must exist (got {TESTKIT_DIR})",
        )

    def test_crate_manifest_exists(self) -> None:
        self.assertTrue(
            TESTKIT_TOML.is_file(),
            f"P0.1a: boole-testkit/Cargo.toml must exist (got {TESTKIT_TOML})",
        )

    def test_crate_is_workspace_member(self) -> None:
        body = _read(WORKSPACE_TOML)
        self.assertRegex(
            body,
            re.compile(r'"crates/boole-testkit"', re.MULTILINE),
            "P0.1a: root Cargo.toml [workspace.members] must include "
            "\"crates/boole-testkit\"",
        )

    def test_lib_exports_rand_suffix(self) -> None:
        body = _read(TESTKIT_LIB)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+rand_suffix\s*\(", re.MULTILINE),
            "P0.1a: boole_testkit must export `pub fn rand_suffix(...)`",
        )

    def test_lib_exports_repo_root(self) -> None:
        body = _read(TESTKIT_LIB)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+repo_root\s*\(", re.MULTILINE),
            "P0.1a: boole_testkit must export `pub fn repo_root(...)`",
        )

    def test_lib_exports_lake_and_lean_available(self) -> None:
        body = _read(TESTKIT_LIB)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+lake_and_lean_available\s*\(", re.MULTILINE),
            "P0.1a: boole_testkit must export `pub fn "
            "lake_and_lean_available(...)`",
        )

    def test_at_least_one_external_caller(self) -> None:
        """Master plan §0.1 rule 4: extraction needs at least one proven
        call site. Walk every other crate's tests/ and src/ for a `use
        boole_testkit::` import."""
        callers: list[Path] = []
        for cargo_toml in (ROOT / "crates").glob("*/Cargo.toml"):
            if cargo_toml.parent == TESTKIT_DIR:
                continue
            for rs in cargo_toml.parent.rglob("*.rs"):
                try:
                    text = rs.read_text(encoding="utf-8")
                except OSError:
                    continue
                if re.search(r"\buse\s+boole_testkit\s*::", text):
                    callers.append(rs)
                    break
            if callers:
                break
        self.assertTrue(
            callers,
            "P0.1a: at least one external crate must `use boole_testkit::...` "
            "so the shared helper is a real call site, not speculative "
            "scaffolding",
        )


if __name__ == "__main__":
    unittest.main()
