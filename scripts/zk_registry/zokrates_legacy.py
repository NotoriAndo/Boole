"""ZoKrates <= 0.7.x ("legacy") compile results -> the shared :class:`zokrates_r1cs.Model`.

These releases predate the ``--r1cs`` circom-format exporter.  For a release line with no prebuilt
binary for this host, whose crate needs a nightly-only Rust feature (``#![feature(box_patterns,
box_syntax)]``), :func:`zokrates_toolchain.build_source` builds it from an *already installed* stable
rustc with ``RUSTC_BOOTSTRAP=1`` (confirmed against 0.6.1: this nightly gate is still accepted by rustc
1.60.0 this way, even though the feature predates any rustc version still carrying it on its nightly
channel).  That binary's ``compile`` (no ``-r``/``--circom-witness``) writes the compiled program, a
human-readable ``.ztf`` listing of its R1CS and ``abi.json``; ``compute-witness`` (plain ``-a``, no
``--abi``) writes a plain-text witness (``"<variable> <value>"`` per line, the same variable names the
``.ztf`` uses: ``~one``, ``_0.._n-1`` for the flattened arguments in ABI declaration order, ``~out_k``
for the flattened return values, other names for internal wires).  This module parses both text formats
(verified against real 0.6.1 compiles: :mod:`test_zk_registry_zokrates`) and renumbers them into the
same wire layout :mod:`zokrates_r1cs`'s native path uses (0: one; outputs; public inputs; private
inputs; internal), so the rest of the pipeline treats every ZoKrates generation alike.
"""
from __future__ import annotations

import re

from . import r1cs as R
from . import zokrates_r1cs as ZR


class ZtfFormatError(ValueError):
    pass


_HEADER_RE = re.compile(r"def\s+main\s*\(([^)]*)\)\s*->\s*\(\s*(\d+)\s*\)\s*:")
_RETURN_RE = re.compile(r"^return\s+(.*)$")


def _split_depth0(text: str, sep: str) -> list:
    """Split ``text`` on every top-level (paren-depth 0) occurrence of ``sep``."""
    out, depth, i, start = [], 0, 0, 0
    while i < len(text):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
        elif depth == 0 and text.startswith(sep, i):
            out.append(text[start:i])
            i += len(sep)
            start = i
            continue
        i += 1
    out.append(text[start:])
    return out


def _unwrap(text: str) -> str:
    """Strip every outer pair of parens that wraps the whole (already-stripped) string."""
    text = text.strip()
    while text.startswith("(") and _match_paren(text, 0) == len(text) - 1:
        text = text[1:-1].strip()
    return text


def _match_paren(text: str, open_pos: int) -> int:
    depth = 0
    for i in range(open_pos, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return i
    return -1


def _parse_term(piece: str) -> tuple:
    """One ``+``-joined summand: ``coeff * var`` (either side optionally parenthesized, any number of
    times -- the pretty-printer wraps a negative literal in its own parens, and wraps the whole term too
    when it is the sum's only one) or a bare constant ``K`` (``K * ~one`` implied)."""
    factors = _split_depth0(piece.strip(), "*")
    if len(factors) == 1:
        k = _unwrap(factors[0])
        if not re.fullmatch(r"-?\d+", k):
            raise ZtfFormatError(f"unparsed linear combination term: {piece!r}")
        return "~one", int(k)
    if len(factors) != 2:
        raise ZtfFormatError(f"unparsed linear combination term: {piece!r}")
    coeff, var = _unwrap(factors[0]), _unwrap(factors[1])
    if not re.fullmatch(r"-?\d+", coeff) or not re.fullmatch(r"[~A-Za-z0-9_]+", var):
        raise ZtfFormatError(f"unparsed linear combination term: {piece!r}")
    return var, int(coeff)


def _parse_sum(text: str) -> list:
    """A sum of ``coeff * var`` terms, ``+``-joined, with any amount of (possibly nested, possibly
    whole-term or coefficient-only) parenthesization -- the compiler's pretty-printer's bracketing has
    varied by version and by whether a term is the sum's only one; a bare ``0`` is the empty sum."""
    text = _unwrap(text.strip())
    if text == "0":
        return []
    return [_parse_term(p) for p in _split_depth0(text, "+")]


def parse_ztf(text: str) -> tuple:
    """``(argument variable names in declaration order, output variable names in return order,
    constraints)`` from a ``zokrates compile`` ``.ztf`` file; ``#``-led directive comment lines are
    skipped (a directive's outputs are ordinary internal wires, introduced the first time some real
    constraint below mentions them -- free unless one does, exactly a circom/iden3 R1CS's own
    convention for any wire no constraint pins to the inputs)."""
    header = None
    outputs: list = []
    constraints: list = []
    for raw in text.splitlines():
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        if header is None:
            m = _HEADER_RE.search(s)
            if not m:
                raise ZtfFormatError(f"expected a `def main(...)` header, got {s!r}")
            header = [a.strip() for a in m.group(1).split(",") if a.strip()]
            continue
        rm = _RETURN_RE.match(s)
        if rm:
            outputs = [v.strip() for v in rm.group(1).split(",") if v.strip()]
            continue
        lr = _split_depth0(s, "==")
        if len(lr) != 2:
            raise ZtfFormatError(f"unparsed constraint line (expected one top-level '=='): {s!r}")
        factors = _split_depth0(lr[0], "*")
        if len(factors) != 2:
            raise ZtfFormatError(f"unparsed constraint line (expected one top-level '*' before '=='): {s!r}")
        a, b, c = _parse_sum(factors[0]), _parse_sum(factors[1]), _parse_sum(lr[1])
        constraints.append((a, b, c))
    if header is None:
        raise ZtfFormatError("no `def main` header")
    return header, outputs, constraints


def parse_witness_text(text: str) -> dict:
    """``zokrates compute-witness``'s plain-text witness (one ``"<variable> <value>"`` line per wire,
    any order) -> variable name -> value."""
    values = {}
    for raw in text.splitlines():
        ln = raw.strip()
        if not ln:
            continue
        name, val = ln.rsplit(" ", 1)
        values[name] = int(val)
    return values


def _sym_entries(out_wires, out_leaves, in_wires, names) -> list:
    entries = [R.SymEntry(0, 0, 0, "one")]
    for wire, (name, _ty) in zip(out_wires, out_leaves):
        entries.append(R.SymEntry(0, wire, 0, f"main.{name}"))
    for wire, name in zip(in_wires, names):
        entries.append(R.SymEntry(0, wire, 0, f"main.{name}"))
    return entries


def build_model(ztf_text: str, abi: dict, p: int = R.BN254_SCALAR):
    """The :class:`zokrates_r1cs.Model` of a legacy compile, and the ``.ztf``/witness variable name ->
    model wire index map (:func:`witness_vector` needs it to translate a computed witness)."""
    header, out_names_raw, cons_raw = parse_ztf(ztf_text)
    in_leaves = ZR.flatten_inputs(abi["inputs"])
    if len(header) != len(in_leaves):
        raise ZtfFormatError(f"ztf header has {len(header)} argument(s), the ABI has {len(in_leaves)} leaf/leaves")
    out_leaves = ZR.flatten_outputs(abi)
    if len(out_names_raw) != len(out_leaves):
        raise ZtfFormatError(f"ztf return has {len(out_names_raw)} value(s), the ABI has {len(out_leaves)} leaf/leaves")
    pub_idx = [i for i, (_, is_pub, _) in enumerate(in_leaves) if is_pub]
    prv_idx = [i for i, (_, is_pub, _) in enumerate(in_leaves) if not is_pub]
    pub_leaf_names = [in_leaves[i][0] for i in pub_idx]
    prv_leaf_names = [in_leaves[i][0] for i in prv_idx]

    renumber = {"~one": 0}
    for k, nm in enumerate(out_names_raw):
        renumber[nm] = 1 + k
    base = 1 + len(out_names_raw)
    for k, i in enumerate(pub_idx):
        renumber[header[i]] = base + k
    base += len(pub_idx)
    for k, i in enumerate(prv_idx):
        renumber[header[i]] = base + k
    base += len(prv_idx)
    nxt = [base]

    def wire_of(name: str) -> int:
        if name not in renumber:
            renumber[name] = nxt[0]
            nxt[0] += 1
        return renumber[name]

    def lc(terms) -> R.LinComb:
        return [(wire_of(nm), c % p) for nm, c in terms]

    constraints = [(lc(a), lc(b), lc(c)) for a, b, c in cons_raw]
    n_wires = nxt[0]
    nbytes = (p.bit_length() + 63) // 64 * 8
    r = R.R1cs(prime=p, field_bytes=nbytes, n_wires=n_wires, n_pub_out=len(out_names_raw), n_pub_in=len(pub_idx),
              n_prv_in=len(prv_idx), n_labels=n_wires, constraints=constraints)

    out_wires = list(range(1, 1 + r.n_pub_out))
    in_wires = list(range(1 + r.n_pub_out, 1 + r.n_pub_out + r.n_pub_in + r.n_prv_in))
    entries = _sym_entries(out_wires, out_leaves, in_wires, pub_leaf_names + prv_leaf_names)
    io = R.main_io_wires(r, entries)
    names = R.wire_names(entries, r.n_wires)
    by_name = {n: ty for n, _pub, ty in in_leaves}
    leaf_types = [by_name[nm[len("main."):]] for nm in io.input_names]
    model = ZR.Model(r, io.inputs, io.outputs, names, leaf_types)
    return model, renumber


def witness_vector(values_by_name: dict, renumber: dict, n_wires: int, p: int) -> list:
    w = [0] * n_wires
    seen = [False] * n_wires
    for name, wire in renumber.items():
        if name in values_by_name:
            w[wire] = values_by_name[name] % p
            seen[wire] = True
    if not all(seen):
        missing = [i for i, s in enumerate(seen) if not s]
        raise ValueError(f"witness is missing {len(missing)} wire(s) (first: {missing[:5]})")
    return w
