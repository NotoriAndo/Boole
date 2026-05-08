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
        self.assertEqual(summary["publicScore"]["primaryMetric"], "blockProductionRatePct")
        self.assertEqual(summary["publicScore"]["formula"], "blocksProduced / generatedAttempts * 100")
        self.assertEqual(summary["totals"]["generatedAttempts"], 2)
        self.assertEqual(summary["totals"]["blocksProduced"], 1)
        self.assertEqual(summary["totals"]["blockProductionRatePct"], 50.0)

        self.assertIn("Sample benchmark artifact", doc)
        self.assertIn("not real model performance", doc)
        self.assertIn("not public-network mining", doc)
        self.assertIn("invalid accepted: 0", doc)
        self.assertIn("replay: PASS", doc)
        self.assertIn("sample-leaderboard.md", doc)
        self.assertIn("sample-summary.json", doc)
        self.assertIn("qwen2.5-coder:fake", leaderboard)
        self.assertIn("fixture/mock", leaderboard)
        self.assertIn("blockProductionRate: 1/2 (50.00%)", leaderboard)
        self.assertIn("blockProductionRate: 1/2 (50.00%)", doc)

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

    def test_readme_exposes_public_benchmark_card_without_overclaiming(self) -> None:
        readme = (ROOT / "README.md").read_text(encoding="utf-8")
        self.assertIn("## Proof-to-Block Benchmark v0.1 card", readme)
        self.assertIn("Which AI agents can create verified work that becomes blocks?", readme)
        self.assertIn("blockProductionRate = blocksProduced / generatedAttempts", readme)
        self.assertIn("17 replay-valid blocks", readme)
        self.assertIn("invalid accepted: 0", readme)
        self.assertIn("chain divergence: 0", readme)
        self.assertIn("fake-command CI path: PASS", readme)
        self.assertIn("sample artifact", readme)
        self.assertIn("docs/benchmarks/proof-to-block-v0.1-sample.md", readme)
        self.assertIn("not real model performance", readme)
        self.assertNotIn("Ollama mined", readme)
        self.assertNotIn("real network mined", readme)
    def test_local_ollama_manual_smoke_guide_is_safe_and_optional(self) -> None:
        guide_path = ROOT / "docs" / "local-ollama-benchmark.md"
        self.assertTrue(guide_path.exists(), "missing docs/local-ollama-benchmark.md")
        guide = guide_path.read_text(encoding="utf-8")
        self.assertIn("Optional local Ollama", guide)
        self.assertIn("No automatic model pull", guide)
        self.assertIn("No automatic daemon start", guide)
        self.assertIn("ollama serve", guide)
        self.assertIn("ollama pull qwen2.5-coder:7b", guide)
        self.assertIn("--model-preset ollama", guide)
        self.assertIn("--ollama-model qwen2.5-coder:7b", guide)
        self.assertIn("setup-required", guide)
        self.assertIn("Local model-generated proof attempts are evaluated", guide)
        self.assertIn("not public-network mining", guide)
        self.assertNotIn("Ollama mined", guide)
        self.assertNotIn("token reward", guide)


if __name__ == "__main__":
    unittest.main()
