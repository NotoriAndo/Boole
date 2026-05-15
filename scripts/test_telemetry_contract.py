#!/usr/bin/env python3
"""Regression tests for P0.5: `boole_telemetry::init(BinaryName)` exists and
every binary calls it from `main`. The L8 contract requires structured
tracing on every process so panics, request IDs, and error counters are
observable from the first second of boot.

This is a P0.5a pin: the helper exists and at least one binary adopts it.
Later P0.5 slices migrate the remaining binaries."""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _find_telemetry_module() -> Path | None:
    """boole_telemetry can live as a standalone crate (crates/boole-telemetry)
    or as a module inside boole-core (crates/boole-core/src/telemetry.rs).
    Either is acceptable for P0.5a as long as the public init symbol exists."""
    candidates = [
        ROOT / "crates" / "boole-telemetry" / "src" / "lib.rs",
        ROOT / "crates" / "boole-core" / "src" / "telemetry.rs",
        ROOT / "crates" / "boole-core" / "src" / "telemetry" / "mod.rs",
    ]
    for path in candidates:
        if path.is_file():
            return path
    return None


class TelemetryContractTests(unittest.TestCase):
    def test_telemetry_module_exists(self) -> None:
        path = _find_telemetry_module()
        self.assertIsNotNone(
            path,
            "P0.5: a boole_telemetry surface must exist (either as crate "
            "`boole-telemetry/src/lib.rs` or module "
            "`boole-core/src/telemetry.rs`)",
        )

    def test_init_function_exported(self) -> None:
        path = _find_telemetry_module()
        if path is None:
            self.skipTest("module not yet present — covered by previous test")
        body = _read(path)
        self.assertRegex(
            body,
            re.compile(r"\bpub\s+fn\s+init\b", re.MULTILINE),
            "P0.5: boole_telemetry must export `pub fn init(...)` taking a "
            "binary name",
        )

    def test_at_least_one_binary_calls_init(self) -> None:
        """Walk binary main.rs files; at least one must reference
        boole_telemetry::init or boole_core::telemetry::init."""
        callers: list[Path] = []
        candidates = [
            ROOT / "crates" / "boole-node" / "src" / "main.rs",
            ROOT / "crates" / "boole-cli" / "src" / "main.rs",
            ROOT / "crates" / "boole-miner" / "src" / "main.rs",
        ]
        for path in candidates:
            if not path.is_file():
                continue
            body = _read(path)
            if re.search(r"boole_telemetry::init|telemetry::init\s*\(", body):
                callers.append(path)
                break
        self.assertTrue(
            callers,
            "P0.5: at least one binary `main` must call "
            "`boole_telemetry::init(...)` so the contract has a proven caller",
        )


if __name__ == "__main__":
    unittest.main()
