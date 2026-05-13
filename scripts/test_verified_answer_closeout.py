#!/usr/bin/env python3
"""Verified-answer local MVP closeout document regression tests."""
from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLOSEOUT = ROOT / "docs" / "verified-answer-local-mvp-closeout.md"
README = ROOT / "README.md"


class VerifiedAnswerCloseoutTests(unittest.TestCase):
    def test_closeout_doc_records_completed_batches_and_dod_without_overclaiming(self) -> None:
        self.assertTrue(CLOSEOUT.exists(), f"missing closeout doc: {CLOSEOUT}")
        text = CLOSEOUT.read_text(encoding="utf-8")

        required = [
            "# Verified-answer local MVP closeout",
            "Batch 4 — Verified Answer product surface: COMPLETE for local MVP",
            "Batch 5 — Gates/docs: COMPLETE",
            "S-preV1.R3.2",
            "V1.1",
            "V1.2",
            "X1.1",
            "P1.1",
            "G1.1",
            "G1.2",
            "Definition of Done status",
            "Default CLI never prints sk",
            "Secret export exists only through explicit unsafe command",
            "Session policies validate canWithdraw=false and canTransfer=false",
            "Local signer denies over-cap/unknown-family/unknown-verifier requests",
            "Node can persist and query session state",
            "Node can reject session-bound submissions that violate session policy",
            "Rewards credit rewardRecipient, not session key",
            "Node can persist receipt commitments without raw prompt/artifact data",
            "Mock /verify-answer demonstrates local payment-required flow and receipt creation",
            "Agent passport remains indexer/primitive-event based, not rich consensus state",
            "Full workspace tests, docs smoke, and gitleaks pass",
            "RUST_TEST_THREADS=1 ./scripts/self-test.sh",
            "wallet-session-receipt-gate: PASS",
            "NEXT-BATCH.1 — Select the next official batch from operating evidence",
        ]
        for fragment in required:
            self.assertIn(fragment, text)

        forbidden = [
            "public live x402 settlement",
            "agents have autonomous wallets",
            "Boole verifies all AI answers",
            "real network mining rewards",
        ]
        for phrase in forbidden:
            self.assertNotIn(phrase, text)

    def test_readme_links_closeout_doc(self) -> None:
        readme = README.read_text(encoding="utf-8")
        self.assertIn("docs/verified-answer-local-mvp-closeout.md", readme)


if __name__ == "__main__":
    unittest.main()
