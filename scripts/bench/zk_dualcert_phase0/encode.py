"""Canonical byte encodings for the dual-cert Phase 0 harness (S0/S7).

THROWAWAY offline experiment code — never wired into self-test/consensus.

S0 compares these bytes across independent process invocations, so every
encoder here must be a pure function of its input with a fixed ordering
(generation order — no dict/set iteration).
"""
from __future__ import annotations

import hashlib

from gen import Circuit


def dimacs_bytes(n_vars: int, clauses: list[tuple[int, ...]]) -> bytes:
    lines = [f"p cnf {n_vars} {len(clauses)}\n"]
    for cl in clauses:
        lines.append(" ".join(str(l) for l in cl) + " 0\n")
    return "".join(lines).encode("ascii")


def circuit_canonical_bytes(c: Circuit) -> bytes:
    """Full canonical serialization of the generated instance.

    Includes constraints, roles, reference witness and reference output —
    everything the generator derives from the seed — so S0's byte comparison
    cannot pass on a lossy summary (counts only).
    """
    parts = [
        f"candidate=zk-circuit-uniqueness-dual-cert.v0\n",
        f"seed={c.seed}\n",
        f"params={c.params.label()}\n",
        f"public={','.join(str(v) for v in c.public_vars)}\n",
        f"output={','.join(str(v) for v in c.output_vars)}\n",
        f"ref_witness={''.join(str(b) for b in c.ref_witness[1:])}\n",
        f"ref_output={''.join(str(b) for b in c.ref_output)}\n",
        "constraints:\n",
    ]
    for cl in c.clauses:
        parts.append(" ".join(str(l) for l in cl) + " 0\n")
    return "".join(parts).encode("ascii")


def d_dimacs_bytes(c: Circuit) -> bytes:
    """Canonical DIMACS for D(seed) = C(w) AND output(w) != ref_output."""
    return dimacs_bytes(c.n_vars, c.d_clauses())


def witness_bytes(witness_bits: list[int]) -> bytes:
    """Canonical BUG-certificate bytes: packed bits of w[1..n]."""
    bits = witness_bits[1:]
    out = bytearray()
    for i in range(0, len(bits), 8):
        byte = 0
        for j, b in enumerate(bits[i : i + 8]):
            byte |= (b & 1) << j
        out.append(byte)
    return bytes(out)


def canon_binding(seed: str, outcome_tag: str, certificate: bytes) -> str:
    """S7 bytes-level prototype of `canon = f(seed, outcome_tag, cert_bytes)`.

    NOT a consensus format — it only demonstrates that the canonical answer
    hash cannot be reproduced from the seed alone and reacts to any single
    certificate byte. Length-prefixed fields prevent boundary ambiguity.
    """
    if outcome_tag not in ("BUG", "SAFE"):
        raise ValueError("outcome_tag must be BUG or SAFE")
    h = hashlib.blake2b(digest_size=32)
    for part in (b"zk-dualcert-canon-v0", seed.encode(), outcome_tag.encode(), certificate):
        h.update(len(part).to_bytes(8, "big"))
        h.update(part)
    return h.hexdigest()
