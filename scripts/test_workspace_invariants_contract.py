#!/usr/bin/env python3
"""Regression tests for P0.6: workspace-level invariants for production
readiness — `[workspace.lints]` deny set, `[profile.release]` panic = "abort",
and a pinned MSRV `rust-version`.

These pin the L0 contract: a single root Cargo.toml controls panic policy
and lint policy for every member crate so a new crate cannot accidentally
opt out. Per-crate `lints.workspace = true` is enforced separately by P0.6b
once this lands."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE_TOML = ROOT / "Cargo.toml"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class WorkspaceInvariantsContractTests(unittest.TestCase):
    def test_workspace_lints_block_exists(self) -> None:
        body = _read(WORKSPACE_TOML)
        self.assertRegex(
            body,
            re.compile(r"^\[workspace\.lints\.rust\]", re.MULTILINE),
            "P0.6: root Cargo.toml must declare `[workspace.lints.rust]` "
            "so every member inherits the same deny set",
        )

    def test_workspace_lints_deny_unsafe(self) -> None:
        body = _read(WORKSPACE_TOML)
        self.assertRegex(
            body,
            re.compile(r'^\s*unsafe_code\s*=\s*"deny"', re.MULTILINE),
            "P0.6: [workspace.lints.rust] must `unsafe_code = \"deny\"` to "
            "prevent accidental unsafe blocks in new crates",
        )

    def test_release_profile_panic_abort(self) -> None:
        body = _read(WORKSPACE_TOML)
        self.assertRegex(
            body,
            re.compile(r'^\[profile\.release\][^[]*?panic\s*=\s*"abort"', re.MULTILINE | re.DOTALL),
            "P0.6: [profile.release] must set `panic = \"abort\"` per L8 "
            "panic-boundary contract",
        )

    def test_workspace_msrv_pinned(self) -> None:
        body = _read(WORKSPACE_TOML)
        self.assertRegex(
            body,
            re.compile(r'^\s*rust-version\s*=\s*"\d+\.\d+(?:\.\d+)?"', re.MULTILINE),
            "P0.6: [workspace.package] must pin `rust-version` (MSRV) so "
            "downstream environments cannot silently drift",
        )

    def test_at_least_one_member_opts_into_workspace_lints(self) -> None:
        """Master plan §0 rule 4: a workspace policy with no opted-in
        member is speculative scaffolding. At least one member crate must
        carry `[lints] workspace = true` so the deny set is exercised by
        a real compile."""
        opted_in: list[Path] = []
        for cargo_toml in (ROOT / "crates").glob("*/Cargo.toml"):
            body = _read(cargo_toml)
            if re.search(
                r"\[lints\]\s*\n[^\[]*?workspace\s*=\s*true", body, flags=re.DOTALL
            ):
                opted_in.append(cargo_toml)
        self.assertTrue(
            opted_in,
            "P0.6: at least one member crate Cargo.toml must declare "
            "`[lints] workspace = true` so [workspace.lints] is a live "
            "contract, not dead declaration",
        )

    def test_every_member_opts_into_workspace_lints(self) -> None:
        """P0.6b: with the workspace deny set live, every member crate
        must inherit it. A new crate that forgets `[lints] workspace =
        true` would silently opt out of `unsafe_code = "deny"` and any
        future workspace-wide policy (clippy.too_many_lines, etc.), so
        the gate must enumerate members rather than trust convention.

        boole-lean-runner needs `unsafe` for libc rlimit syscalls used
        in Lean child sandboxing; rather than carve out a per-crate
        manifest exception (which would skip the workspace opt-in
        entirely), it inherits `[lints] workspace = true` and locally
        relaxes the deny set via a crate-root `#![allow(unsafe_code)]`
        attribute. That keeps the manifest gate uniform while still
        documenting the unsafe boundary in code."""
        missing: list[str] = []
        for cargo_toml in sorted((ROOT / "crates").glob("*/Cargo.toml")):
            body = _read(cargo_toml)
            if not re.search(
                r"\[lints\]\s*\n[^\[]*?workspace\s*=\s*true", body, flags=re.DOTALL
            ):
                missing.append(cargo_toml.parent.name)
        self.assertEqual(
            missing,
            [],
            "P0.6b: every member crate must declare `[lints] workspace "
            "= true` so the workspace deny set is enforced uniformly; "
            f"missing: {missing}",
        )


if __name__ == "__main__":
    unittest.main()
