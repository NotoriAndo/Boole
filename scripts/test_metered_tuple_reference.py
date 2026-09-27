#!/usr/bin/env python3
"""Independent integer/hash oracle for the non-activated metered tuple corpus.

This does not call Rust, a compiler, a model, or the V1 checker. It evaluates
only the explicit synthetic corpus formulas, not arbitrary submitted programs.
Run with --emit to print candidate fixtures for review; it never edits files.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import struct
import sys
import unittest

FIXTURES = Path(__file__).resolve().parents[1] / "fixtures/native-metered-tuple-v1"
MASK = (1 << 64) - 1
ADAPTER = "BOOLE-METERED-GENERATED-TUPLE-V1"


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def domain(label: str, *parts: bytes) -> str:
    return sha(label.encode() + b"\0" + b"".join(struct.pack(">Q", len(p)) + p for p in parts))


def wrap(value: int) -> int:
    return (value + (1 << 63)) % (1 << 64) - (1 << 63)


def case_values(task_digest: str, answer_digest: str):
    seed = domain("boole.metered-tuple-cases.v1", task_digest.encode(), answer_digest.encode())
    state = int(seed[:16], 16)
    values = []
    for _ in range(128):
        state = (state + 0x9E3779B97F4A7C15) & MASK
        value = ((state ^ (state >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & MASK
        value ^= value >> 31
        values.append(value % (61 if len(values) % 2 == 0 else 2))
    return list(zip(values[::2], values[1::2]))


def outputs(rows, first_coefficient=3):
    acc = 7
    result = []
    for unsigned, flag in rows:
        acc = wrap(acc * 5 + unsigned * first_coefficient - flag * 2)
        result.append(acc)
    return result


def words(values) -> bytes:
    return b"".join(struct.pack(">q", n) for n in values)


def corpus():
    task = json.loads((FIXTURES / "task.json").read_text())
    task = {key: task[key] for key in ("schema", "fieldTypes", "taskSeed", "a0", "mul", "coeffs")}
    task_digest = domain("boole.metered-tuple-task.v1", json.dumps(task, separators=(",", ":")).encode())
    policy_words = (4096, 64, 8192, 512, 256, 32, 100_000, 100_000, 2080)
    policy_digest = domain("boole.metered-tuple-policy.v1", ADAPTER.encode(), struct.pack(">9Q", *policy_words))
    accepted = (FIXTURES / "answer.rs").read_bytes()
    sources = {
        "accepted": accepted,
        "wrong_answer": accepted.replace(b"wrapping_mul(3)", b"wrapping_mul(4)"),
        "fuel_exhausted": ("let mut acc: i64 = 0; for it in items { " + "acc = acc.wrapping_add(0);" * 20 + " } acc").encode(),
        "forbidden_loop": b"loop {}",
        "invalid_utf8": b"\xff",
    }
    reasons = {
        "accepted": "accepted", "wrong_answer": "answer_mismatch",
        "fuel_exhausted": "budget_exceeded:fuel", "forbidden_loop": "forbidden_construct",
        "invalid_utf8": "invalid_utf8",
    }
    cases = []
    for name, source in sources.items():
        answer_digest = sha(source)
        result = {
            "schema": "boole.metered-tuple-verification.v1", "adapter": ADAPTER,
            "policyDigest": policy_digest, "taskDigest": task_digest,
            "answerDigest": answer_digest,
            "verdict": "accepted" if name == "accepted" else "deterministic_reject",
            "reason": reasons[name], "resourceUse": None,
            "corpusDigest": None, "outputsDigest": None,
            "nonIssuable": True, "activationAllowed": False,
        }
        if name not in ("forbidden_loop", "invalid_utf8"):
            rows = case_values(task_digest, answer_digest)
            expected = outputs(rows)
            result["corpusDigest"] = sha(words([n for row in rows for n in row]) + words(expected))
        if name in ("accepted", "wrong_answer"):
            # Reviewed meter semantics for this exact body: 73 tokens, 27 AST
            # nodes, depth 8; 12 operations/item + 1 initial let/prefix;
            # 22 fuel/item + 5 fixed fuel/prefix, 1+...+64 item visits.
            result["resourceUse"] = (b"boole.native-rust-meter.resource-use.v1\0" + struct.pack(
                ">7Q", len(source), 73, 27, 8, 12 * 2080 + 64, 22 * 2080 + 5 * 64, 2080
            )).hex()
            result["outputsDigest"] = sha(words(outputs(rows, 3 if name == "accepted" else 4)))
        cases.append({"name": name, "answerHex": source.hex(), "expected": result})
    return {"schema": "boole.metered-tuple-test-corpus.v1", "cases": cases}


class MeteredTupleReferenceTests(unittest.TestCase):
    def test_tracked_golden_matches_independent_integer_and_hash_oracle(self):
        self.assertEqual(json.loads((FIXTURES / "corpus.json").read_text()), corpus())


if __name__ == "__main__":
    if sys.argv[1:] == ["--emit"]:
        print(json.dumps(corpus(), indent=2, sort_keys=True))
    else:
        unittest.main()
