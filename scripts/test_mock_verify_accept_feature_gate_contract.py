#!/usr/bin/env python3
"""P1.9 contract: `--mock-verify-accept` and the `AcceptingVerifier`
bypass path are behind a Cargo feature so a release build of
`boole-miner` cannot accept proofs without invoking the real Lean
verifier.

L0 contract: the always-accept stub exists so smoke tests can run
without Lean installed. It must never compile into a production miner.
Today the flag is unconditional; a release `boole-miner` build will
happily skip Lean verification on `--mock-verify-accept`, which
defeats the entire purpose of the verifier and lets any miner forge
shares against the network.

P1.9 plants the gate that a future release script can flip off:

1. `boole-miner/Cargo.toml` declares a `dev-tools` feature.
2. `dev-tools` is NOT in the crate's default feature set — otherwise
   the gate is decorative.
3. The `mock_verify_accept: bool` field on the start-args struct is
   annotated `#[cfg(feature = "dev-tools")]`.
4. Every reference to `AcceptingVerifier` in the CLI builder lives
   inside `#[cfg(feature = "dev-tools")]` so the no-feature build
   does not link the always-accept stub.
5. The `AcceptingVerifier` struct itself is gated, so a release build
   does not even compile the bypass.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MINER_CRATE = ROOT / "crates" / "boole-miner"
CARGO_TOML = MINER_CRATE / "Cargo.toml"
CLI_RS = MINER_CRATE / "src" / "cli.rs"
LOCAL_VERIFY_RS = MINER_CRATE / "src" / "local_verify.rs"
LEAN_RUNNER_LIB = ROOT / "crates" / "boole-lean-runner" / "src" / "lib.rs"
NODE_MAIN_RS = ROOT / "crates" / "boole-node" / "src" / "main.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class MockVerifyAcceptGateContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.cargo = _read(CARGO_TOML)
        self.cli = _read(CLI_RS)
        self.local_verify = _read(LOCAL_VERIFY_RS)

    def test_cargo_toml_declares_dev_tools_feature(self) -> None:
        self.assertRegex(
            self.cargo,
            r"(?m)^\[features\]",
            "P1.9: boole-miner/Cargo.toml must declare a `[features]` "
            "section so dev-only affordances can be gated.",
        )
        self.assertRegex(
            self.cargo,
            r"(?m)^dev-tools\s*=\s*\[\s*\]",
            "P1.9: Cargo.toml must declare `dev-tools = []` so the "
            "gate has a name to flip on for tests.",
        )

    def test_dev_tools_is_not_a_default_feature(self) -> None:
        match = re.search(
            r"(?m)^default\s*=\s*\[(.*?)\]",
            self.cargo,
            re.DOTALL,
        )
        if match is None:
            return
        default_list = match.group(1)
        self.assertNotIn(
            "dev-tools",
            default_list,
            "P1.9: `dev-tools` must NOT be a default feature. A "
            "default-on gate is decorative — release builds still "
            "ship `--mock-verify-accept`. Tests opt in explicitly via "
            "`--features boole-miner/dev-tools`.",
        )

    def test_mock_verify_accept_field_is_cfg_gated(self) -> None:
        pattern = re.compile(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-tools\"\s*\)\][^\n]*\n"
            r"(?:[^\n]*\n){0,3}\s*pub\s+mock_verify_accept\s*:",
        )
        self.assertRegex(
            self.cli,
            pattern,
            "P1.9: the `mock_verify_accept` field on the start-args "
            "struct must be annotated `#[cfg(feature = \"dev-tools\")]` "
            "so a release build does not even surface the flag in "
            "`--help`.",
        )

    def test_accepting_verifier_struct_is_cfg_gated(self) -> None:
        pattern = re.compile(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-tools\"\s*\)\]\s*\n"
            r"\s*pub\s+struct\s+AcceptingVerifier",
        )
        self.assertRegex(
            self.local_verify,
            pattern,
            "P1.9: `pub struct AcceptingVerifier` itself must be "
            "annotated `#[cfg(feature = \"dev-tools\")]` so the "
            "release-mode build does not even compile the bypass.",
        )

    def test_cli_references_to_accepting_verifier_are_gated(self) -> None:
        # Strip cfg-gated items so the remaining text only contains
        # unconditional code. Order matters: comments first (they may
        # legitimately mention `AcceptingVerifier`), then single-line
        # `use` items, then multi-line `fn` bodies that the cfg attr
        # applies to.
        stripped = re.sub(r"//[^\n]*", "", self.cli)
        stripped = re.sub(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-tools\"\s*\)\]\s*\n"
            r"\s*use\s+[^;]+;",
            "",
            stripped,
        )
        stripped = re.sub(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-tools\"\s*\)\]\s*\n"
            r"\s*fn\s+[^{]*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}",
            "",
            stripped,
        )
        self.assertNotIn(
            "AcceptingVerifier",
            stripped,
            "P1.9: every reference to `AcceptingVerifier` inside "
            "cli.rs must be `#[cfg(feature = \"dev-tools\")]`-gated. "
            "An unconditional reference means the release build "
            "still links the bypass.",
        )


class ForbiddenTokenScannerContractTests(unittest.TestCase):
    """P1.9 — the pre-checker scanner rejects every unsound escape token
    (`sorry`, `axiom`, `native_decide`) before a proof reaches `lake`."""

    def setUp(self) -> None:
        self.lib = _read(LEAN_RUNNER_LIB)

    def test_forbidden_tokens_const_covers_all_unsound_escapes(self) -> None:
        match = re.search(
            r"const\s+FORBIDDEN_TOKENS\s*:[^=]*=\s*&\[(.*?)\];",
            self.lib,
            re.DOTALL,
        )
        self.assertIsNotNone(
            match,
            "P1.9: boole-lean-runner must declare a FORBIDDEN_TOKENS table.",
        )
        body = match.group(1)
        for token in ("sorry", "axiom", "native_decide"):
            self.assertIn(
                f'"{token}"',
                body,
                f"P1.9: FORBIDDEN_TOKENS must reject `{token}`.",
            )

    def test_check_file_scans_before_lake_spawn(self) -> None:
        scan_idx = self.lib.find("scan_for_forbidden_tokens(proof_path)")
        spawn_idx = self.lib.find('Command::new("lake")')
        self.assertNotEqual(
            scan_idx, -1, "P1.9: check_file must call scan_for_forbidden_tokens."
        )
        self.assertNotEqual(spawn_idx, -1)
        self.assertLess(
            scan_idx,
            spawn_idx,
            "P1.9: the forbidden-token scan must run BEFORE lake is spawned.",
        )


class InsecureVerifierBootRefusalContractTests(unittest.TestCase):
    """P1.9 — `boole-node run-local --lean-checker-disabled` refuses to
    boot without `--allow-insecure-verifier`, so a production node cannot
    silently serve unverified proofs."""

    def setUp(self) -> None:
        self.main = _read(NODE_MAIN_RS)

    def test_allow_insecure_verifier_flag_exists(self) -> None:
        self.assertIn(
            "allow-insecure-verifier",
            self.main,
            "P1.9: run-local must expose an `--allow-insecure-verifier` opt-in flag.",
        )

    def test_boot_refusal_guard_present(self) -> None:
        self.assertRegex(
            self.main,
            r"args\.lean_checker_disabled\s*&&\s*!\s*args\.allow_insecure_verifier",
            "P1.9: run_local_command must refuse when the verifier is "
            "disabled without the explicit opt-in.",
        )
        self.assertIn(
            "insecure_verifier_config",
            self.main,
            "P1.9: the refusal must emit a typed `insecure_verifier_config` error.",
        )

    def test_boot_refusal_exits_ex_config(self) -> None:
        # Anchor on the actual guard expression (not a doc-comment mention)
        # so the exit-code assertion keys off the executable refusal path.
        guard = re.search(
            r"args\.lean_checker_disabled\s*&&\s*!\s*args\.allow_insecure_verifier",
            self.main,
        )
        self.assertIsNotNone(guard, "P1.9: refusal guard must exist.")
        window = self.main[guard.start() : guard.start() + 700]
        self.assertIn(
            "std::process::exit(78)",
            window,
            "P1.9: the insecure-verifier refusal must exit 78 (EX_CONFIG), "
            "not merely log and proceed to bind.",
        )


if __name__ == "__main__":
    unittest.main()
