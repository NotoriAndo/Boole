#!/usr/bin/env python3
"""P1.3b contract: the per-block commit writes its stores in a fixed,
recovery-safe order, and boot HEALS (does not bail on) a reward ledger that
trails the block store after a crash mid-commit.

L7 contract — atomic multi-store commit. The block store is the source of
truth; every other store is re-derivable from it. So `submit_json` must
write its stores in this order:

  1. burn the submit nonce      (nonces.ndjson)
  2. commit the block           (blocks.ndjson + reward ledger)
  3. append bounty events       (bounty-events.ndjson)
  4. append the submit receipt  (submit-receipts.ndjson)

and the boot path must RE-DERIVE the missing trailing reward event from the
block store (the crash-mid-commit window) rather than refusing to come up.
This test pins the source-line order and the presence of the heal so a
refactor cannot silently regress either half.

P1.3b deliberately closes the L7 "atomic multi-store commit" row with this
re-derive-on-mismatch heal instead of a `staging/commit-<height>.json`
write-ahead intent file: the block store already makes every other store
re-derivable, so an intent file would add a second source of truth that
could only diverge. (See the ADR note in the production-readiness master
plan.)
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL_NODE = ROOT / "crates" / "boole-node" / "src" / "local_node.rs"
RUNTIME = ROOT / "crates" / "boole-node" / "src" / "runtime.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _function_span(body: str, signature_regex: str) -> tuple[int, int]:
    """Return the body text of the function whose signature matches
    `signature_regex` (from the opening `{` to its matching `}`)."""
    match = re.search(signature_regex, body)
    if match is None:
        raise AssertionError(f"could not locate signature {signature_regex!r}")
    brace_index = body.index("{", match.end())
    depth = 1
    cursor = brace_index + 1
    while cursor < len(body) and depth > 0:
        ch = body[cursor]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        cursor += 1
    if depth != 0:
        raise AssertionError("unbalanced braces while scanning function body")
    return brace_index, cursor


class MultiStoreCommitOrderingContractTests(unittest.TestCase):
    def setUp(self) -> None:
        body = _read(LOCAL_NODE)
        start, end = _function_span(body, r"fn\s+submit_json\s*\(")
        self.span = body[start:end]

    def _offset(self, needle: str) -> int:
        idx = self.span.find(needle)
        self.assertNotEqual(idx, -1, f"`{needle}` must appear in submit_json's body")
        return idx

    def test_write_order_nonce_block_bounty_receipt(self) -> None:
        # N3.3 — the bounty-event rows moved into the shared
        # `append_block_bounty_events` helper (reused by the p2p block
        # ingest path); the ordering contract now pins the helper CALL's
        # position inside submit_json and the actual ledger appends inside
        # the helper's own body (checked below).
        nonce = self._offset("burn_submit_nonce(")
        block = self._offset("commit_next_block_for_current_c_with_promoted(")
        bounty = self._offset("append_block_bounty_events(")
        receipt = self._offset("append_submit_receipt(")
        self.assertLess(nonce, block, "nonce burn must precede the block commit")
        self.assertLess(block, bounty, "block commit must precede the bounty-event append")
        self.assertLess(
            bounty, receipt, "bounty-event append must precede the submit receipt"
        )

    def test_bounty_helper_appends_credit_then_share_promoted(self) -> None:
        # SC reset window (ADR-0015 (a)): the helper no longer builds the
        # rows inline — it calls the SAME `derive_bounty_events` the P1.3b
        # boot heal and the N4 reorg rebuild use (credit rows derived from
        # committed shares via settlement), then appends credits before
        # share_promoted rows. The row construction order itself is pinned
        # in derive_bounty_events' body (runtime.rs).
        body = _read(LOCAL_NODE)
        start, end = _function_span(body, r"fn\s+append_block_bounty_events\s*\(")
        span = body[start:end]
        derive = span.find("derive_bounty_events(")
        chain = span.find("credits.iter().chain(shares.iter())")
        append = span.find("FileBountyEventLedger::append(")
        self.assertNotEqual(
            derive, -1, "the helper must derive rows via derive_bounty_events"
        )
        self.assertNotEqual(
            chain, -1, "the helper must append credit rows before share_promoted rows"
        )
        self.assertNotEqual(
            append, -1, "the helper must append to the bounty event ledger"
        )

        runtime = _read(RUNTIME)
        start, end = _function_span(runtime, r"fn\s+derive_bounty_events\s*\(")
        span = runtime[start:end]
        credit = span.find('"kind": "credit"')
        share = span.find('"kind": "share_promoted"')
        self.assertNotEqual(credit, -1, "derive_bounty_events must build credit rows")
        self.assertNotEqual(
            share, -1, "derive_bounty_events must build share_promoted rows"
        )
        self.assertLess(
            credit,
            share,
            "credit rows must precede share_promoted rows (P1.3b heal derivation order)",
        )


class RewardLedgerHealContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.runtime = _read(RUNTIME)

    def test_boot_re_derives_trailing_reward_events(self) -> None:
        self.assertIn(
            "reward ledger healed from block store",
            self.runtime,
            "P1.3b: boot_from_store_with_bounty_ledger must re-derive trailing "
            "reward events from the block store (crash-mid-commit heal), not bail.",
        )
        self.assertIn(
            "derive_reward_event",
            self.runtime,
            "P1.3b: a single derive_reward_event helper must back both the "
            "absent-ledger re-derive and the trailing-event heal.",
        )


if __name__ == "__main__":
    unittest.main()
