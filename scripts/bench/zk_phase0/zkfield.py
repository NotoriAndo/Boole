"""Prime-field arithmetic for the ZK.0 offline feasibility spike.

This is a THROWAWAY experiment harness under scripts/bench/zk_phase0/. It is
never wired into self-test, consensus, or any production path (L1 master §ZK).
It exists only to produce the go/no-go numbers ADR-0017 needs.
"""
from __future__ import annotations


class Field:
    """Operations in F_p for a prime modulus p."""

    def __init__(self, p: int):
        self.p = p

    def add(self, a: int, b: int) -> int:
        return (a + b) % self.p

    def sub(self, a: int, b: int) -> int:
        return (a - b) % self.p

    def mul(self, a: int, b: int) -> int:
        return (a * b) % self.p

    def neg(self, a: int) -> int:
        return (-a) % self.p

    def normalize(self, a: int) -> int:
        return a % self.p


# Mersenne primes keep the modulus small enough for Z3's nonlinear integer
# solver to stay tractable while remaining a genuine prime field (no zero
# divisors, unlike a 2^k ring). The spike sweeps field size as one axis.
MERSENNE_PRIMES = {
    13: (1 << 13) - 1,   # 8191
    17: (1 << 17) - 1,   # 131071
    19: (1 << 19) - 1,   # 524287
    31: (1 << 31) - 1,   # 2147483647
}
