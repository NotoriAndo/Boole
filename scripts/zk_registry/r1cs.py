"""iden3 ``.r1cs`` (binary format v1) and ``.sym`` reader, plus an independent R1CS evaluator.

The evaluator is the Python oracle used by the gates: it shares no code with the generated
Lean model, so agreement between the two is evidence that the model transcribes the compiled
constraint system.

Wire layout of a circom R1CS (checked by :func:`main_io_wires`)::

    0                          the constant-one wire
    1 .. nOut                  public outputs of the main component
    .. + nPubIn                public inputs
    .. + nPrvIn                private inputs
    rest                       internal signals
"""
from __future__ import annotations

import os
import re
import struct
from dataclasses import dataclass, field
from typing import Iterable, Sequence

BN254_SCALAR = 21888242871839275222246405745257275088548364400416034343698204186575808495617
BLS12_381_SCALAR = 0x73EDA753299D7D483339D80809A1D80553BDA402FFFE5BFEFFFFFFFF00000001
GOLDILOCKS = 2**64 - 2**32 + 1

# circom `--prime` names for the primes this tool knows.
KNOWN_PRIMES = {
    BN254_SCALAR: "bn128",
    BLS12_381_SCALAR: "bls12381",
    GOLDILOCKS: "goldilocks",
}

SECTION_HEADER = 1
SECTION_CONSTRAINTS = 2
SECTION_WIRE2LABEL = 3

Term = tuple[int, int]            # (wire index, coefficient in [0, p))
LinComb = list[Term]
Constraint = tuple[LinComb, LinComb, LinComb]


class R1csFormatError(ValueError):
    """The file is not a well-formed iden3 R1CS file of a supported version."""


@dataclass
class R1cs:
    prime: int
    field_bytes: int
    n_wires: int
    n_pub_out: int
    n_pub_in: int
    n_prv_in: int
    n_labels: int
    constraints: list[Constraint]
    wire_to_label: list[int] = field(default_factory=list)
    section_order: list[int] = field(default_factory=lambda: [SECTION_HEADER, SECTION_CONSTRAINTS, SECTION_WIRE2LABEL])

    @property
    def n_constraints(self) -> int:
        return len(self.constraints)

    @property
    def prime_name(self) -> str | None:
        return KNOWN_PRIMES.get(self.prime)


def parse_r1cs(data: bytes) -> R1cs:
    """Parse an iden3 R1CS file (version 1); raise :class:`R1csFormatError` on any inconsistency."""
    if len(data) < 12 or data[:4] != b"r1cs":
        raise R1csFormatError("missing r1cs magic")
    version, n_sections = struct.unpack_from("<II", data, 4)
    if version != 1:
        raise R1csFormatError(f"unsupported r1cs version {version}")
    off = 12
    sections: dict[int, tuple[int, int]] = {}
    for _ in range(n_sections):
        if off + 12 > len(data):
            raise R1csFormatError("truncated section table")
        stype, ssize = struct.unpack_from("<IQ", data, off)
        off += 12
        if off + ssize > len(data):
            raise R1csFormatError(f"section {stype} overruns the file")
        if stype in sections:
            raise R1csFormatError(f"duplicate section {stype}")
        sections[stype] = (off, ssize)
        off += ssize
    if off != len(data):
        raise R1csFormatError("trailing bytes after the last section")
    if SECTION_HEADER not in sections or SECTION_CONSTRAINTS not in sections:
        raise R1csFormatError("header or constraint section missing")

    ho, hsize = sections[SECTION_HEADER]
    (n8,) = struct.unpack_from("<I", data, ho)
    if n8 <= 0 or n8 % 8 or hsize != 4 + n8 + 4 * 4 + 8 + 4:
        raise R1csFormatError("bad header size")
    prime = int.from_bytes(data[ho + 4:ho + 4 + n8], "little")
    n_wires, n_pub_out, n_pub_in, n_prv_in, n_labels, n_cons = struct.unpack_from("<IIIIQI", data, ho + 4 + n8)
    if prime < 2:
        raise R1csFormatError("bad prime")
    if 1 + n_pub_out + n_pub_in + n_prv_in > n_wires:
        raise R1csFormatError("public/private signal counts exceed the wire count")

    co, csize = sections[SECTION_CONSTRAINTS]
    end = co + csize
    o = co

    def lin_comb() -> LinComb:
        nonlocal o
        if o + 4 > end:
            raise R1csFormatError("truncated linear combination")
        (m,) = struct.unpack_from("<I", data, o)
        o += 4
        terms: LinComb = []
        for _ in range(m):
            if o + 4 + n8 > end:
                raise R1csFormatError("truncated term")
            (wire,) = struct.unpack_from("<I", data, o)
            o += 4
            coeff = int.from_bytes(data[o:o + n8], "little")
            o += n8
            if wire >= n_wires:
                raise R1csFormatError(f"wire {wire} out of range")
            if coeff >= prime:
                raise R1csFormatError("coefficient not reduced")
            terms.append((wire, coeff))
        return terms

    constraints: list[Constraint] = []
    for _ in range(n_cons):
        a = lin_comb()
        b = lin_comb()
        c = lin_comb()
        constraints.append((a, b, c))
    if o != end:
        raise R1csFormatError("constraint section size mismatch")

    wire_to_label: list[int] = []
    if SECTION_WIRE2LABEL in sections:
        wo, wsize = sections[SECTION_WIRE2LABEL]
        if wsize != 8 * n_wires:
            raise R1csFormatError("wire2label section size mismatch")
        wire_to_label = list(struct.unpack_from(f"<{n_wires}Q", data, wo))
    return R1cs(prime, n8, n_wires, n_pub_out, n_pub_in, n_prv_in, n_labels, constraints, wire_to_label,
                list(sections))


@dataclass
class R1csHeader:
    prime: int
    n_wires: int
    n_constraints: int


def read_header(path: str) -> R1csHeader:
    """Prime, wire and constraint counts from the header section only (no constraint parsing, so
    very large systems can be sized cheaply); same header checks as :func:`parse_r1cs`."""
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        head = f.read(12)
        if len(head) < 12 or head[:4] != b"r1cs":
            raise R1csFormatError("missing r1cs magic")
        version, n_sections = struct.unpack_from("<II", head, 4)
        if version != 1:
            raise R1csFormatError(f"unsupported r1cs version {version}")
        off = 12
        for _ in range(n_sections):
            f.seek(off)
            raw = f.read(12)
            if len(raw) < 12:
                raise R1csFormatError("truncated section table")
            stype, ssize = struct.unpack("<IQ", raw)
            off += 12
            if off + ssize > size:
                raise R1csFormatError(f"section {stype} overruns the file")
            if stype == SECTION_HEADER:
                data = f.read(ssize)
                (n8,) = struct.unpack_from("<I", data, 0)
                if n8 <= 0 or n8 % 8 or ssize != 4 + n8 + 4 * 4 + 8 + 4:
                    raise R1csFormatError("bad header size")
                prime = int.from_bytes(data[4:4 + n8], "little")
                n_wires, _, _, _, _, n_cons = struct.unpack_from("<IIIIQI", data, 4 + n8)
                if prime < 2:
                    raise R1csFormatError("bad prime")
                return R1csHeader(prime, n_wires, n_cons)
            off += ssize
    raise R1csFormatError("header or constraint section missing")


def read_r1cs(path: str) -> R1cs:
    with open(path, "rb") as f:
        return parse_r1cs(f.read())


def encode_r1cs(r: R1cs) -> bytes:
    """Serialize ``r`` in the iden3 format (used for fixtures and round-trip tests)."""
    n8 = r.field_bytes

    def fe(v: int) -> bytes:
        return v.to_bytes(n8, "little")

    header = struct.pack("<I", n8) + fe(r.prime) + struct.pack(
        "<IIIIQI", r.n_wires, r.n_pub_out, r.n_pub_in, r.n_prv_in, r.n_labels, len(r.constraints))
    body = bytearray()
    for cons in r.constraints:
        for lc in cons:
            body += struct.pack("<I", len(lc))
            for wire, coeff in lc:
                body += struct.pack("<I", wire) + fe(coeff)
    payloads = {SECTION_HEADER: header, SECTION_CONSTRAINTS: bytes(body)}
    if r.wire_to_label:
        payloads[SECTION_WIRE2LABEL] = struct.pack(f"<{len(r.wire_to_label)}Q", *r.wire_to_label)
    order = [t for t in r.section_order if t in payloads] + [t for t in sorted(payloads) if t not in r.section_order]
    out = bytearray(b"r1cs" + struct.pack("<II", 1, len(order)))
    for stype in order:
        out += struct.pack("<IQ", stype, len(payloads[stype])) + payloads[stype]
    return bytes(out)


# ------------------------------------------------------------------------------------------ .sym

@dataclass(frozen=True)
class SymEntry:
    label: int
    wire: int          # -1 when the signal was removed by the optimizer
    component: int
    name: str


def parse_sym(text: str) -> list[SymEntry]:
    entries = []
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split(",", 3)
        if len(parts) != 4:
            raise R1csFormatError(f".sym line {n}: expected 4 fields")
        entries.append(SymEntry(int(parts[0]), int(parts[1]), int(parts[2]), parts[3]))
    return entries


def read_sym(path: str) -> list[SymEntry]:
    with open(path, encoding="utf-8") as f:
        return parse_sym(f.read())


def wire_names(entries: Iterable[SymEntry], n_wires: int) -> list[str | None]:
    """Signal name of each wire (first .sym entry wins; wire 0 is the constant one)."""
    names: list[str | None] = [None] * n_wires
    names[0] = "one"
    for e in entries:
        if 0 < e.wire < n_wires and names[e.wire] is None:
            names[e.wire] = e.name
    return names


_INDEX_RE = re.compile(r"(\[\d+\])+$")


def signal_base(name: str) -> str:
    """``main.out[3][1]`` -> ``main.out``."""
    return _INDEX_RE.sub("", name)


@dataclass
class MainIo:
    outputs: list[int]
    inputs: list[int]
    output_names: list[str]
    input_names: list[str]


def main_io_wires(r: R1cs, entries: Sequence[SymEntry], main_prefix: str = "main.") -> MainIo:
    """Input and output wires of the main component, cross-checked against the .sym names.

    The header fixes the ranges; every wire in them must carry a ``main.<signal>`` name of the
    main component itself (no deeper component path)."""
    names = wire_names(entries, r.n_wires)
    outs = list(range(1, 1 + r.n_pub_out))
    ins = list(range(1 + r.n_pub_out, 1 + r.n_pub_out + r.n_pub_in + r.n_prv_in))
    for w in outs + ins:
        nm = names[w]
        if nm is None or not nm.startswith(main_prefix) or "." in signal_base(nm)[len(main_prefix):]:
            raise R1csFormatError(f"wire {w} ({nm}) is not a signal of the main component")
    return MainIo(outs, ins, [names[w] for w in outs], [names[w] for w in ins])


# ------------------------------------------------------------------------------------------ evaluator

def lc_value(lc: LinComb, w: Sequence[int], p: int) -> int:
    return sum(c * w[i] for i, c in lc) % p


def violated(r: R1cs, w: Sequence[int], limit: int | None = None) -> list[int]:
    """Indices of violated constraints (the constant wire is checked separately by :func:`satisfies`)."""
    if len(w) != r.n_wires:
        raise ValueError(f"witness has {len(w)} values, expected {r.n_wires}")
    p = r.prime
    bad = []
    for k, (a, b, c) in enumerate(r.constraints):
        if (lc_value(a, w, p) * lc_value(b, w, p) - lc_value(c, w, p)) % p:
            bad.append(k)
            if limit is not None and len(bad) >= limit:
                break
    return bad


def satisfies(r: R1cs, w: Sequence[int]) -> bool:
    """The oracle: wire 0 is one, every value is reduced, and every constraint holds."""
    if len(w) != r.n_wires or w[0] != 1 or any(not 0 <= v < r.prime for v in w):
        return False
    return not violated(r, w, limit=1)


def constraint_sites(r: R1cs) -> list[set[int]]:
    """For each wire, the set of constraints that mention it."""
    sites: list[set[int]] = [set() for _ in range(r.n_wires)]
    for k, cons in enumerate(r.constraints):
        for lc in cons:
            for i, _ in lc:
                sites[i].add(k)
    return sites
