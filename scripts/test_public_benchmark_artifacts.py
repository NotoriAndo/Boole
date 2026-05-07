#!/usr/bin/env python3
"""Public-safe benchmark artifact regression tests."""
from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE_DIR = ROOT / "fixtures" / "benchmarks" / "proof-to-block-v0.1"
SUMMARY = SAMPLE_DIR / "sample-summary.json"
LEADERBOARD = SAMPLE_DIR / "sample-leaderboard.md"
DOC = ROOT / "docs" / "benchmarks" / "proof-to-block-v0.1-sample.md"


class PublicBenchmarkArtifactTests(unittest.TestCase):
    def test_public_sample_benchmark_artifacts_are_safe_and_explicitly_sample_only(self) -> None:
        for path in [SUMMARY, LEADERBOARD, DOC]:
            self.assertTrue(path.exists(), f"missing public sample artifact: {path}")

        summary = json.loads(SUMMARY.read_text(encoding="utf-8"))
        leaderboard = LEADERBOARD.read_text(encoding="utf-8")
        doc = DOC.read_text(encoding="utf-8")
        combined = json.dumps(summary, sort_keys=True) + "\n" + leaderboard + "\n" + doc

        self.assertEqual(summary["benchmark"], "proof-to-block-v0.1-public-sample")
        self.assertEqual(summary["sampleOnly"], True)
        self.assertEqual(summary["claimBoundary"], "pipeline sample, not real model performance")
        self.assertEqual(summary["safety"]["invalidAccepted"], 0)
        self.assertEqual(summary["safety"]["chainDivergence"], 0)
        self.assertEqual(summary["safety"]["replayFailures"], 0)
        self.assertTrue(summary["replayPassed"])
        self.assertGreaterEqual(summary["totals"]["generatedAttempts"], 1)

        self.assertIn("Sample benchmark artifact", doc)
        self.assertIn("not real model performance", doc)
        self.assertIn("not public-network mining", doc)
        self.assertIn("invalid accepted: 0", doc)
        self.assertIn("replay: PASS", doc)
        self.assertIn("sample-leaderboard.md", doc)
        self.assertIn("sample-summary.json", doc)
        self.assertIn("qwen2.5-coder:fake", leaderboard)
        self.assertIn("fixture/mock", leaderboard)

        forbidden_claims = [
            "Ollama mined",
            "mined real Boole blocks",
            "real network mined",
            "token reward",
            "mainnet",
        ]
        for phrase in forbidden_claims:
            self.assertNotIn(phrase, combined)
        self.assertNotRegex(combined, re.compile(r"/Users/|/home/runner|/tmp/|sk-[A-Za-z0-9]"))


if __name__ == "__main__":
    unittest.main()
