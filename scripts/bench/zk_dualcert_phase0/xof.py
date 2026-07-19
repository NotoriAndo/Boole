"""Deterministic XOF-backed randomness for the dual-cert Phase 0 harness.

THROWAWAY offline experiment code under scripts/bench/zk_dualcert_phase0/.
Never wired into self-test, consensus, or any production path (tasks/todo.md
paragraph "ZK-DC"). Everything an instance contains must be a pure function of
(seed, domain) so that S0 (determinism) can compare canonical bytes across
independent process invocations.
"""
from __future__ import annotations

import hashlib


class Xof:
    """Counter-mode BLAKE2b stream keyed by (seed, domain).

    Uniform sampling uses rejection so the distribution does not depend on
    word-size artifacts; rejection draws consume the stream deterministically.
    """

    def __init__(self, seed: str, domain: str):
        self._key = f"zk-dualcert-p0|{seed}|{domain}".encode()
        self._counter = 0
        self._buf = bytearray()
        self._pos = 0
        # Attackers may inspect this to correlate generator branching with
        # the BUG/SAFE outcome (S1 seed-branch predictor).
        self.draws = 0
        self.rejections = 0

    def bytes(self, n: int) -> bytes:
        out = bytearray()
        while len(out) < n:
            if self._pos >= len(self._buf):
                block = hashlib.blake2b(
                    self._key + self._counter.to_bytes(8, "big"), digest_size=64
                ).digest()
                self._counter += 1
                self._buf = bytearray(block)
                self._pos = 0
            take = min(n - len(out), len(self._buf) - self._pos)
            out += self._buf[self._pos : self._pos + take]
            self._pos += take
        return bytes(out)

    def u64(self) -> int:
        self.draws += 1
        return int.from_bytes(self.bytes(8), "big")

    def randbelow(self, n: int) -> int:
        """Uniform integer in [0, n) via rejection sampling."""
        if n <= 0:
            raise ValueError("randbelow needs n >= 1")
        limit = (1 << 64) - ((1 << 64) % n)
        while True:
            v = self.u64()
            if v < limit:
                return v % n
            self.rejections += 1

    def bit(self) -> int:
        return self.randbelow(2)

    def choice(self, seq):
        return seq[self.randbelow(len(seq))]

    def sample_distinct(self, upper: int, count: int) -> list[int]:
        """`count` distinct integers in [1, upper], order as drawn."""
        if count > upper:
            raise ValueError("cannot sample more distinct values than range")
        seen: set[int] = set()
        out: list[int] = []
        while len(out) < count:
            v = 1 + self.randbelow(upper)
            if v in seen:
                self.rejections += 1
                continue
            seen.add(v)
            out.append(v)
        return out
