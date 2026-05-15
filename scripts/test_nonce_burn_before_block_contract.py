#!/usr/bin/env python3
"""P1.3a contract: nonce burn is appended to disk BEFORE block append.

L7 contract: a burned nonce with no block is recoverable (orphan-burn
cleaner on the recover path can drop it); a committed block with no
burn is not — it leaves a window where the same `(submittedBy, nonce)`
can be replayed because the burn record never reached disk.

So the durable-write order inside `submit_json` must be:
1. session/nonce gate (already checked in submit_session_gate)
2. admit + share hash
3. **burn `(submittedBy, nonce)` to nonces.ndjson**
4. THEN `commit_next_block_for_current_c_with_promoted` (block append)

This test pins the source-line order so a refactor cannot silently
flip the steps back. Earlier wiring kept the burn in `submit_handler`,
running AFTER `submit_json` returned, which violated the invariant.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LOCAL_NODE = ROOT / "crates" / "boole-node" / "src" / "local_node.rs"


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _function_span(body: str, signature_regex: str) -> tuple[int, int]:
    """Return (start_offset, end_offset) for the body of the function whose
    signature matches `signature_regex`. End is the matching `}` for the
    opening `{` after the signature.
    """
    match = re.search(signature_regex, body)
    if match is None:
        raise AssertionError(f"could not locate signature {signature_regex!r}")
    # Find the opening brace.
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


class NonceBurnOrderingContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.body = _read(LOCAL_NODE)
        start, end = _function_span(self.body, r"fn\s+submit_json\s*\(")
        self.submit_json_body = self.body[start:end]

    def test_burn_appears_inside_submit_json(self) -> None:
        self.assertRegex(
            self.submit_json_body,
            r"append_burn|burn_submit_nonce|nonce_ledger",
            "P1.3a: submit_json must perform the nonce burn itself so "
            "the burn fsync precedes block append fsync. Earlier the "
            "burn ran in submit_handler AFTER submit_json returned, "
            "leaving a crash window between block commit and burn.",
        )

    def test_burn_precedes_block_commit(self) -> None:
        burn_match = re.search(
            r"append_burn|burn_submit_nonce\s*\(",
            self.submit_json_body,
        )
        commit_match = re.search(
            r"commit_next_block_for_current_c_with_promoted\s*\(",
            self.submit_json_body,
        )
        self.assertIsNotNone(
            burn_match,
            "P1.3a: submit_json must contain a nonce-burn call",
        )
        self.assertIsNotNone(
            commit_match,
            "expected commit_next_block_for_current_c_with_promoted "
            "call inside submit_json",
        )
        self.assertLess(
            burn_match.start(),
            commit_match.start(),
            "P1.3a: the nonce burn must be appended to disk BEFORE "
            "commit_next_block_for_current_c_with_promoted. Block "
            "first, burn second leaves an irrecoverable replay window.",
        )

    def test_submit_handler_no_longer_burns_after_submit_json(self) -> None:
        start, end = _function_span(self.body, r"fn\s+submit_handler\s*\(")
        handler_body = self.body[start:end]
        self.assertNotRegex(
            handler_body,
            r"burn_submit_nonce\s*\(",
            "P1.3a: submit_handler must NOT call burn_submit_nonce "
            "after submit_json. Once the burn moves inside submit_json "
            "the post-commit re-burn is dead code and re-enabling it "
            "creates a double-burn race.",
        )


if __name__ == "__main__":
    unittest.main()
