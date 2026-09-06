#!/usr/bin/env python3
"""Contract for the closed-local miner smoke invocation."""
from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SMOKE = ROOT / "scripts" / "boole-miner-smoke.sh"


class BooleMinerSmokeContractTests(unittest.TestCase):
    def test_mock_verifier_is_explicitly_dev_tools_only(self) -> None:
        body = SMOKE.read_text(encoding="utf-8")
        self.assertIn("cargo build -q -p boole-node -p boole-miner --features boole-miner/dev-tools", body)
        self.assertIn('"$MINER_BIN" start', body)
        self.assertIn("--mock-verify-accept", body)
        self.assertIn("--profile v1-lenbound", body)
        self.assertNotIn("--profile v01", body)

    def test_smoke_owns_its_temporary_outputs(self) -> None:
        body = SMOKE.read_text(encoding="utf-8")
        self.assertIn('mktemp -d "${TMPDIR:-/tmp}/boole-miner-smoke.', body)
        self.assertIn('rm -rf "$TMP_DIR"', body)
        self.assertNotIn("/tmp/boole-miner-smoke-", body)

    def test_owned_node_cleanup_has_a_bounded_term_then_kill_path(self) -> None:
        body = SMOKE.read_text(encoding="utf-8")
        self.assertIn("stop_owned_node()", body)
        self.assertIn('kill -TERM "$owned_pid"', body)
        self.assertIn('kill -KILL "$owned_pid"', body)
        self.assertNotIn('wait "$PID"', body)


if __name__ == "__main__":
    unittest.main()
