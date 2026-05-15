#!/usr/bin/env python3
"""Regression tests for P1.1: boole-node ships a state-directory advisory
lock and a `state.manifest.json` contract.

L7 contract: a single boole-node owns its state directory while the
process runs. The crate must export an `acquire`/`StateDirGuard` surface
that takes an exclusive non-blocking advisory lock on `state.lock`, and
an `ensure_manifest` surface plus a `StateManifest` struct that records
`created_at`, `network_id`, `binary_sha`, and `schema_versions`.

This is a P1.1a pin: the module exists with the correct public symbols.
The follow-up P1.1b slice wires it into `LocalNodeState::from_config`
and adds the integration test that two boole-node processes cannot
share a directory."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULE = ROOT / "crates" / "boole-node" / "src" / "state_dir.rs"
LIB = ROOT / "crates" / "boole-node" / "src" / "lib.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class StateDirContractTests(unittest.TestCase):
    def test_module_exists(self) -> None:
        self.assertTrue(
            MODULE.is_file(),
            "P1.1: boole-node must ship `crates/boole-node/src/state_dir.rs`",
        )

    def test_lib_declares_module(self) -> None:
        body = _read(LIB)
        self.assertRegex(
            body,
            re.compile(r"^\s*mod\s+state_dir\s*;", re.MULTILINE),
            "P1.1: boole-node `lib.rs` must declare the state_dir module",
        )

    def test_acquire_function_exported(self) -> None:
        body = _read(MODULE)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+acquire\b", re.MULTILINE),
            "P1.1: state_dir must export `pub fn acquire(...)` taking a state-dir path",
        )

    def test_ensure_manifest_function_exported(self) -> None:
        body = _read(MODULE)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+ensure_manifest\b", re.MULTILINE),
            "P1.1: state_dir must export `pub fn ensure_manifest(...)`",
        )

    def test_state_manifest_struct_carries_required_fields(self) -> None:
        body = _read(MODULE)
        # The struct definition itself must name the four required fields.
        # Order is not important; presence is. Comments or attributes
        # between fields are tolerated by the regex.
        for field in ("created_at", "network_id", "binary_sha", "schema_versions"):
            self.assertRegex(
                body,
                re.compile(
                    r"pub\s+" + re.escape(field) + r"\s*:", re.MULTILINE
                ),
                f"P1.1: StateManifest must carry a `pub {field}` field",
            )

    def test_uses_non_blocking_exclusive_flock(self) -> None:
        body = _read(MODULE)
        self.assertRegex(
            body,
            re.compile(
                r"LOCK_EX\s*\|\s*(?:libc::)?LOCK_NB", re.MULTILINE
            ),
            "P1.1: acquire must use `LOCK_EX | LOCK_NB` so a second "
            "process is rejected immediately rather than blocking",
        )


if __name__ == "__main__":
    unittest.main()
