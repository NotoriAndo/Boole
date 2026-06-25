#!/usr/bin/env python3
"""P1.2 contract: every NDJSON ledger in boole-node uses the shared
durable-append helper and the tail-truncation recover path.

L7 contract: a line acknowledged to the caller survives every crash short
of disk hardware loss. The single helper `append_ndjson_line_durable`
performs `write_all + flush + sync_all` (and `fsync_parent_dir` on file
creation). The single recover entry point `read_stable_prefix` truncates
any torn trailing line on boot via `stable_jsonl_prefix_len`.

This test pins the static surface so a new on-disk write site (or a
regression on an existing one) is caught before the first integration
run instead of being discovered by a crash in the field.

Scope (the eight NDJSON append ledgers in boole-node):
- block, reward, session, nonce, receipt, reputation, bounty-event,
  signed-nonce

Plus the submit-receipt audit log appended from `local_node.rs` after a
block commits — it is a per-acceptance NDJSON ledger and the L7 invariant
applies to it equally.

P1.2 closure note: the contract enumerates EVERY NDJSON append ledger so
a new on-disk write site cannot ship without joining the durable path.
`signed_nonce_ledger.rs` (the P1.6b per-signer nonce burn ledger) was the
last live append store missing from this set; it is now pinned here.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "crates" / "boole-node" / "src"

DURABLE_STORES = {
    "block_store.rs",
    "reward_store.rs",
    "session_store.rs",
    "nonce_ledger.rs",
    "receipt_store.rs",
    "reputation_store.rs",
    "bounty_event_store.rs",
    "signed_nonce_ledger.rs",
    "proof_dedup_ledger.rs",
}


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


class StorageDurabilityContractTests(unittest.TestCase):
    def test_durable_stores_set_enumerates_every_live_ndjson_append_ledger(self) -> None:
        # P1.2 closure: the contract must enumerate EVERY NDJSON append
        # ledger in boole-node, not a hand-picked subset. We derive the
        # live set from the source (every file that calls the shared
        # append helper, minus the helper module itself and the
        # `local_node.rs` submit-receipt writer which has its own
        # dedicated test below) and assert the pinned set matches it
        # exactly. A new store that wires durability but is not added
        # here — or a store dropped from the set — fails this test.
        live = set()
        for path in SRC.glob("*.rs"):
            if path.name in {"durability.rs", "local_node.rs"}:
                continue
            if "append_ndjson_line_durable" in _read(path):
                live.add(path.name)
        self.assertEqual(
            DURABLE_STORES,
            live,
            "L7/P1.2: DURABLE_STORES must list exactly the live NDJSON "
            "append ledgers. Missing from the pinned set: "
            f"{sorted(live - DURABLE_STORES)}; stale entries: "
            f"{sorted(DURABLE_STORES - live)}.",
        )

    def test_every_ndjson_store_imports_durable_append_helper(self) -> None:
        for filename in sorted(DURABLE_STORES):
            with self.subTest(store=filename):
                body = _read(SRC / filename)
                self.assertIn(
                    "append_ndjson_line_durable",
                    body,
                    f"L7: {filename} must call the shared "
                    "`append_ndjson_line_durable` helper so write_all + "
                    "flush + sync_all + parent-dir fsync run on every "
                    "append. A bypass means a torn line on crash.",
                )

    def test_every_ndjson_store_uses_tail_healing_recover(self) -> None:
        for filename in sorted(DURABLE_STORES):
            with self.subTest(store=filename):
                body = _read(SRC / filename)
                self.assertIn(
                    "read_stable_prefix",
                    body,
                    f"L7: {filename} must call `read_stable_prefix` so a "
                    "torn trailing line from a previous crash is "
                    "truncated to the last newline on boot instead of "
                    "bricking the node.",
                )

    def test_submit_receipt_writer_is_durable(self) -> None:
        body = _read(SRC / "local_node.rs")
        match = re.search(
            r"fn\s+append_submit_receipt\s*\([^)]*\)[^{]*\{[^}]*\}",
            body,
            re.DOTALL,
        )
        self.assertIsNotNone(
            match,
            "L7: expected `fn append_submit_receipt` in local_node.rs as "
            "the single writer for the submit-receipt audit log.",
        )
        function_body = match.group(0)
        self.assertIn(
            "append_ndjson_line_durable",
            function_body,
            "L7: append_submit_receipt must route the write through "
            "`append_ndjson_line_durable` so the post-commit submit "
            "receipt survives a crash. A `flush()` without `sync_all()` "
            "leaves the latest receipt in the page cache only.",
        )


if __name__ == "__main__":
    unittest.main()
