#!/usr/bin/env python3
"""Regression tests for P0.3: self-test contract requires RUST_TEST_THREADS=1
and --locked on every cargo invocation that resolves dependencies."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SELF_TEST = ROOT / "scripts" / "self-test.sh"
RUST_PARITY = ROOT / "scripts" / "check-rust-parity.sh"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class SelfTestContractTests(unittest.TestCase):
    def test_self_test_exports_rust_test_threads(self) -> None:
        body = _read(SELF_TEST)
        pattern = re.compile(r"^\s*export\s+RUST_TEST_THREADS=1\b", re.MULTILINE)
        self.assertRegex(
            body,
            pattern,
            "scripts/self-test.sh must `export RUST_TEST_THREADS=1` so the "
            "single-threaded invariant is enforced regardless of caller env",
        )

    def test_self_test_uses_locked_on_cargo_test(self) -> None:
        body = _read(SELF_TEST)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+test\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo test invocation in self-test.sh missing --locked: {stripped!r}",
                )

    def test_self_test_uses_locked_on_cargo_clippy(self) -> None:
        body = _read(SELF_TEST)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+clippy\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo clippy invocation in self-test.sh missing --locked: {stripped!r}",
                )

    def test_rust_parity_uses_locked_on_cargo_test(self) -> None:
        body = _read(RUST_PARITY)
        for line in body.splitlines():
            stripped = line.strip()
            if stripped.startswith("#"):
                continue
            if re.search(r"\bcargo\s+test\b", stripped):
                self.assertIn(
                    "--locked",
                    stripped,
                    f"cargo test invocation in check-rust-parity.sh missing --locked: {stripped!r}",
                )


if __name__ == "__main__":
    unittest.main()
