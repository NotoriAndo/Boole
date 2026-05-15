#!/usr/bin/env python3
"""P1.8 contract: VERIFY_ANSWER_PAYMENT_SIGNATURE is behind a Cargo
feature so a production release build can compile the magic
test-payment string out of the binary.

L0+L6 contract: the bytes `boole-native-test:paid` must NOT be present
in a `cargo build --release --no-default-features` artifact. Today the
magic string is unconditional in `local_node.rs`, which means the
release binary will gladly accept a forged "payment" header. The first
defensive step is the cfg-gate so a future release build can drop the
constant by simply not enabling the `dev-mock-payment` feature.

Scope of this contract:
1. `boole-node/Cargo.toml` declares a `dev-mock-payment` feature.
2. `dev-mock-payment` is NOT in the crate's default feature set —
   otherwise the gate is decorative; a release build still ships the
   magic string and we have only false security.
3. The `VERIFY_ANSWER_PAYMENT_SIGNATURE` constant in `local_node.rs`
   is annotated with `#[cfg(feature = "dev-mock-payment")]`.
4. The match arm that compares against the constant is also annotated
   with `#[cfg(feature = "dev-mock-payment")]` so the production build
   does not even reference it.
5. There is a `#[cfg(not(feature = "dev-mock-payment"))]` arm that
   ALWAYS rejects with `payment_invalid` so the production build still
   compiles and never silently accepts a header.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
NODE_CRATE = ROOT / "crates" / "boole-node"
LOCAL_NODE = NODE_CRATE / "src" / "local_node.rs"
CARGO_TOML = NODE_CRATE / "Cargo.toml"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class VerifyAnswerPaymentGateContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.cargo = _read(CARGO_TOML)
        self.source = _read(LOCAL_NODE)

    def test_cargo_toml_declares_dev_mock_payment_feature(self) -> None:
        self.assertRegex(
            self.cargo,
            r"(?m)^\[features\]",
            "P1.8: boole-node/Cargo.toml must declare a `[features]` "
            "section so the magic test-payment string can be gated.",
        )
        self.assertRegex(
            self.cargo,
            r"(?m)^dev-mock-payment\s*=\s*\[\s*\]",
            "P1.8: Cargo.toml must declare `dev-mock-payment = []` so "
            "the gate has a name to flip on for tests.",
        )

    def test_dev_mock_payment_is_not_a_default_feature(self) -> None:
        match = re.search(
            r"(?m)^default\s*=\s*\[(.*?)\]",
            self.cargo,
            re.DOTALL,
        )
        if match is None:
            return
        default_list = match.group(1)
        self.assertNotIn(
            "dev-mock-payment",
            default_list,
            "P1.8: `dev-mock-payment` must NOT be a default feature. A "
            "default-on gate is decorative — release builds still ship "
            "the magic string and silently accept the test-payment "
            "header. Tests must opt in explicitly via "
            "`--features boole-node/dev-mock-payment`.",
        )

    def test_payment_signature_constant_is_cfg_gated(self) -> None:
        pattern = re.compile(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-mock-payment\"\s*\)\]\s*\n"
            r"\s*const\s+VERIFY_ANSWER_PAYMENT_SIGNATURE\s*:",
        )
        self.assertRegex(
            self.source,
            pattern,
            "P1.8: VERIFY_ANSWER_PAYMENT_SIGNATURE must be annotated "
            "`#[cfg(feature = \"dev-mock-payment\")]` so a release "
            "build with the feature off never compiles the magic "
            "string into the binary.",
        )

    def test_constant_is_referenced_only_under_feature(self) -> None:
        # Strip the contracted constant declaration so the regex below
        # only sees USE sites, not the gated definition.
        without_decl = re.sub(
            r"#\[cfg\(\s*feature\s*=\s*\"dev-mock-payment\"\s*\)\]\s*\n"
            r"\s*const\s+VERIFY_ANSWER_PAYMENT_SIGNATURE\s*:[^;]+;",
            "",
            self.source,
        )
        for use_match in re.finditer(
            r"VERIFY_ANSWER_PAYMENT_SIGNATURE",
            without_decl,
        ):
            preceding = without_decl[: use_match.start()]
            preceding_block_start = preceding.rfind(
                "#[cfg(feature = \"dev-mock-payment\")]"
            )
            preceding_block_start_alt = preceding.rfind(
                "#[cfg(any(feature = \"dev-mock-payment\""
            )
            self.assertTrue(
                preceding_block_start != -1
                or preceding_block_start_alt != -1,
                "P1.8: every reference to "
                "VERIFY_ANSWER_PAYMENT_SIGNATURE must be inside a "
                "`#[cfg(feature = \"dev-mock-payment\")]` item so the "
                "release build does not link the magic-string code.",
            )

    def test_production_path_always_rejects(self) -> None:
        pattern = re.compile(
            r"#\[cfg\(\s*not\(\s*feature\s*=\s*\"dev-mock-payment\"\s*\)\s*\)\]\s*\n"
            r"\s*fn\s+enforce_verify_answer_payment\s*\(",
        )
        self.assertRegex(
            self.source,
            pattern,
            "P1.8: there must be a "
            "`#[cfg(not(feature = \"dev-mock-payment\"))]` variant of "
            "`enforce_verify_answer_payment` so the release build "
            "still compiles when the magic string is excluded.",
        )
        # The non-feature variant must reject Some(_) with payment_invalid.
        non_feature_match = re.search(
            r"#\[cfg\(\s*not\(\s*feature\s*=\s*\"dev-mock-payment\"\s*\)\s*\)\]\s*\n"
            r"\s*fn\s+enforce_verify_answer_payment\s*\([^{]*\{(.*?)\n\}",
            self.source,
            re.DOTALL,
        )
        self.assertIsNotNone(
            non_feature_match,
            "P1.8: could not locate the body of the no-feature "
            "`enforce_verify_answer_payment` variant.",
        )
        body = non_feature_match.group(1)
        self.assertIn(
            "Some(_)",
            body,
            "P1.8: no-feature variant must match `Some(_)` so any "
            "header value the caller sends is uniformly rejected.",
        )
        self.assertIn(
            "payment_invalid",
            body,
            "P1.8: no-feature variant must reject with "
            "`payment_invalid` — never silently accept a header it "
            "cannot validate.",
        )


if __name__ == "__main__":
    unittest.main()
