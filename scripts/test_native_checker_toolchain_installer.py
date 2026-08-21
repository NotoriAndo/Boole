#!/usr/bin/env python3
"""Supply and CI contract for the native checker's exact Rust toolchain."""

from __future__ import annotations

import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "scripts" / "install-native-checker-toolchain.sh"


class NativeCheckerToolchainInstallerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.assertTrue(INSTALLER.is_file(), "exact per-commit installer is missing")
        self.text = INSTALLER.read_text(encoding="utf-8")

    def test_installer_pins_the_exact_commit_and_all_archive_hashes(self) -> None:
        required = (
            "e7795af6d2449fb05a6393c3320ced873a999eb3",
            "rustc-nightly-x86_64-unknown-linux-gnu.tar.xz",
            "12cd470422b39da22a7b8c2f069c25e66200d5a46c1be5dac0bfe7620ed0d415",
            "rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz",
            "fd04194fb361ef69735a0b722fcaf6d9b49a339944f485aebcc4c172adb5c339",
            "cargo-nightly-x86_64-unknown-linux-gnu.tar.xz",
            "53e718c828a16746abdf3f8fb6f4c75ce5494a6f547ef6f02d45d72faef4c426",
        )
        for value in required:
            self.assertIn(value, self.text)

    def test_installer_is_fail_closed_and_not_date_nightly_based(self) -> None:
        for value in (
            "set -euo pipefail",
            "sha256sum -c",
            "mktemp -d",
            "trap cleanup EXIT",
            "--disable-ldconfig",
            "rustc_commit_hash",
            "cargo_commit_hash",
        ):
            self.assertIn(value, self.text)
        self.assertNotIn("rustup toolchain install nightly", self.text)
        self.assertNotIn("static.rust-lang.org/dist/2026-07-22", self.text)


if __name__ == "__main__":
    unittest.main()
