#!/usr/bin/env python3
"""Regression tests for P0.8: every secret-bearing type carries a hand-written
`impl Debug` that prints `<redacted>` for the secret fields. The L1/L8
contract forbids `#[derive(Debug)]` on these types because the default
formatter would leak the secret into logs and panic messages.

This file pins the bar for `MinerState.sk` and `LlmConfig.api_key` first.
Other secret-bearing types (`OwnerState`, `SessionState`, vault material)
follow in later P0.8 slices."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MINER_STATE_RS = ROOT / "crates" / "boole-miner" / "src" / "state.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _block_starting_at(body: str, struct_name: str) -> str:
    """Return the rough source span around `struct <name>` for grep checks.
    Heuristic: from 200 chars before to 600 chars after the struct keyword."""
    m = re.search(rf"\bstruct\s+{re.escape(struct_name)}\b", body)
    if not m:
        return ""
    start = max(0, m.start() - 200)
    end = min(len(body), m.start() + 600)
    return body[start:end]


class SecretRedactorContractTests(unittest.TestCase):
    def test_miner_state_has_hand_written_debug(self) -> None:
        body = _read(MINER_STATE_RS)
        block = _block_starting_at(body, "MinerState")
        self.assertNotRegex(
            block,
            re.compile(r"#\[derive\([^)]*\bDebug\b[^)]*\)\]\s*(?:pub\s+)?struct\s+MinerState"),
            "P0.8: MinerState must NOT use `#[derive(Debug)]` — the ed25519 "
            "secret seed `sk` would leak into logs. Use a hand-written "
            "`impl Debug` that prints `<redacted>` for `sk`.",
        )
        self.assertRegex(
            body,
            re.compile(r"impl\s+(?:std::fmt::|fmt::)?Debug\s+for\s+MinerState\b"),
            "P0.8: MinerState must have a hand-written `impl Debug` that "
            "redacts the secret seed",
        )

    def test_miner_state_debug_redacts_sk(self) -> None:
        body = _read(MINER_STATE_RS)
        m = re.search(r"impl\s+(?:std::fmt::|fmt::)?Debug\s+for\s+MinerState\b", body)
        if not m:
            self.skipTest("hand-written Debug not yet present — previous test covers")
        impl_block = body[m.start():m.start() + 800]
        self.assertIn(
            "<redacted>",
            impl_block,
            "P0.8: MinerState Debug impl must print `<redacted>` for the "
            "secret seed",
        )

    def test_llm_config_has_hand_written_debug(self) -> None:
        body = _read(MINER_STATE_RS)
        block = _block_starting_at(body, "LlmConfig")
        self.assertNotRegex(
            block,
            re.compile(r"#\[derive\([^)]*\bDebug\b[^)]*\)\]\s*(?:pub\s+)?struct\s+LlmConfig"),
            "P0.8: LlmConfig must NOT use `#[derive(Debug)]` — `api_key` "
            "would leak into logs. Use a hand-written `impl Debug` that "
            "prints `<redacted>` for `api_key`.",
        )
        self.assertRegex(
            body,
            re.compile(r"impl\s+(?:std::fmt::|fmt::)?Debug\s+for\s+LlmConfig\b"),
            "P0.8: LlmConfig must have a hand-written `impl Debug` that "
            "redacts the api_key",
        )

    def test_llm_config_debug_redacts_api_key(self) -> None:
        body = _read(MINER_STATE_RS)
        m = re.search(r"impl\s+(?:std::fmt::|fmt::)?Debug\s+for\s+LlmConfig\b", body)
        if not m:
            self.skipTest("hand-written Debug not yet present — previous test covers")
        impl_block = body[m.start():m.start() + 800]
        self.assertIn(
            "<redacted>",
            impl_block,
            "P0.8: LlmConfig Debug impl must print `<redacted>` for api_key",
        )


if __name__ == "__main__":
    unittest.main()
