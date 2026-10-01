"""ZoKrates compile results -> the shared internal R1CS model (:mod:`r1cs`).

Two compilers, one export path each, one shared model:

* ZoKrates 0.8.8 (official release binary / a source build of the tag; :mod:`zokrates_toolchain`)
  emits the iden3/circom ``.r1cs`` binary format directly with ``--r1cs`` (confirmed: the magic,
  header and constraint sections are byte-identical to circom's), so :func:`build_native` hands the
  file to :mod:`r1cs` unchanged and only has to recover wire *names* and the input/output wire sets
  from the compiler's ``abi.json`` (which, unlike circom's ``.sym``, carries no per-wire names at all —
  only the declared parameter/output *types*, recursively flattened here into one synthetic name per
  leaf wire).
* ZoKrates 0.6.1 (ethereum-oasis-op/baseline's pin; no release asset for this host, no R1CS export at
  this version either; a native source build, :mod:`zokrates_toolchain.build_source`) is read from its
  own human-readable ``.ztf`` constraint listing and a plain-text witness, both parsed by
  :mod:`zokrates_legacy` and renumbered into the same wire layout (``0``: one; ``1..nPubOut``: outputs;
  next ``nPubIn``: public inputs; next ``nPrvIn``: private inputs; rest: internal), exactly as a
  circom ``.r1cs``.

Both paths produce an :class:`r1cs.R1cs` plus a :class:`Model` (wire names, the input/output wire
lists in :mod:`r1cs`'s ``MainIo`` shape, and the scalar ZoKrates type of every input leaf, used to
restrict :mod:`witness` sampling to each type's domain).  ZoKrates's own directives (division,
bit-decomposition, ... solvers) are never part of either R1CS export: their output wires are ordinary
internal wires of the constraint system built here, free unless some later real ``Constraint``
pins them down — exactly the wave plan's "directives/solvers are unconstrained helpers".
"""
from __future__ import annotations

from dataclasses import dataclass, field

from . import r1cs as R


class AbiError(ValueError):
    """The ABI (or the program it describes) cannot be turned into a model."""


# ------------------------------------------------------------------------------------------ ABI flattening

def _inner_spec(container: dict) -> dict:
    """A component's own type, as a spec dict (``{"type", "components"?}``)."""
    spec = {"type": container["type"]}
    if "components" in container:
        spec["components"] = container["components"]
    return spec


def leaf_specs(spec: dict) -> list[tuple[str, str]]:
    """``(name suffix, scalar type)`` of every leaf wire of an ABI type, in flattening order
    (array index, then struct member declaration order — the same order ZoKrates itself flattens
    arguments and return values into scalar wires, checked against real compiles)."""
    t = spec["type"]
    if t == "array":
        c = spec["components"]
        inner = _inner_spec(c)
        subs = leaf_specs(inner)
        return [(f"[{i}]{suf}", ty) for i in range(c["size"]) for suf, ty in subs]
    if t == "struct":
        c = spec["components"]
        out: list[tuple[str, str]] = []
        for m in c["members"]:
            for suf, ty in leaf_specs(_inner_spec(m)):
                out.append((f"_{m['name']}{suf}", ty))
        return out
    return [("", t)]


def flatten_inputs(items: list[dict]) -> list[tuple[str, bool, str]]:
    """``(leaf name, is_public, scalar type)`` of every leaf wire of an ABI ``inputs`` list, in
    declaration order (the order ZoKrates assigns raw argument wires)."""
    out = []
    for item in items:
        for suf, ty in leaf_specs(item):
            out.append((f"{item['name']}{suf}", bool(item.get("public", True)), ty))
    return out


def _output_items(abi: dict) -> list[dict]:
    """ABI output type(s) as a list of specs, whichever shape the compiler used: ``outputs`` (a list,
    zokrates-js 1.0.25 / ZoKrates 0.6.1) or ``output`` (a single spec, possibly a ``tuple``, the
    0.8.8 CLI's ``abi.json``)."""
    if "outputs" in abi:
        return list(abi["outputs"])
    out = abi["output"]
    if out.get("type") == "tuple":
        return list(out["components"]["elements"])
    return [out]


def flatten_outputs(abi: dict) -> list[tuple[str, str]]:
    """``(leaf name, scalar type)`` of every output leaf wire, in return order; outputs have no ABI
    name, so leaves are numbered positionally (``ret0``, ``ret1[2]``, ...)."""
    out = []
    for k, item in enumerate(_output_items(abi)):
        for suf, ty in leaf_specs(item):
            out.append((f"ret{k}{suf}", ty))
    return out


SCALAR_DOMAIN = {"bool": "binary", "u8": ("maxbit", 8), "u16": ("maxbit", 16), "u32": ("maxbit", 32),
                 "u64": ("maxbit", 64), "field": None}


def input_domains(leaves: list[tuple[str, bool, str]]) -> dict[str, object]:
    return {name: SCALAR_DOMAIN.get(ty) for name, _pub, ty in leaves}


# ------------------------------------------------------------------------------------------ model

@dataclass
class Model:
    r: R.R1cs
    inputs: list[int]                  # public then private input wires, in declaration order within each group
    outputs: list[int]
    wire_names: list[str | None]
    input_leaf_types: list[str]        # parallel to ``inputs``
    n_directives: int = 0
    solver_kinds: dict[str, int] = field(default_factory=dict)   # solver name -> directive count (bincode path only)

    @property
    def io(self) -> R.MainIo:
        return R.MainIo(self.outputs, self.inputs,
                        [self.wire_names[o] or "" for o in self.outputs],
                        [self.wire_names[i] or "" for i in self.inputs])


def _entries(prefix_outputs: list[tuple[int, str]], prefix_inputs: list[tuple[int, str]]) -> list[R.SymEntry]:
    entries = [R.SymEntry(0, 0, 0, "one")]
    for wire, name in prefix_outputs + prefix_inputs:
        entries.append(R.SymEntry(0, wire, 0, name))
    return entries


def _grouped_inputs(leaves: list[tuple[str, bool, str]]) -> tuple[list[str], list[str]]:
    """Leaf names split into (public, private), each preserving declaration order."""
    pub = [n for n, is_pub, _ in leaves if is_pub]
    prv = [n for n, is_pub, _ in leaves if not is_pub]
    return pub, prv


def build_native(r1cs_path: str, abi: dict) -> Model:
    """ZoKrates's own ``--r1cs`` export (iden3 format) plus its ``abi.json`` (no per-wire names)."""
    r = R.read_r1cs(r1cs_path)
    out_leaves = flatten_outputs(abi)
    in_leaves = flatten_inputs(abi["inputs"])
    pub_names, prv_names = _grouped_inputs(in_leaves)
    if r.n_pub_out != len(out_leaves) or r.n_pub_in != len(pub_names) or r.n_prv_in != len(prv_names):
        raise AbiError(f"ABI leaf counts (out {len(out_leaves)}, pub {len(pub_names)}, prv {len(prv_names)}) "
                        f"do not match the R1CS header (out {r.n_pub_out}, pub {r.n_pub_in}, prv {r.n_prv_in})")
    out_wires = list(range(1, 1 + r.n_pub_out))
    in_wires = list(range(1 + r.n_pub_out, 1 + r.n_pub_out + r.n_pub_in + r.n_prv_in))
    entries = _entries([(w, f"main.{n}") for w, (n, _ty) in zip(out_wires, out_leaves)],
                       [(w, f"main.{n}") for w, n in zip(in_wires, pub_names + prv_names)])
    io = R.main_io_wires(r, entries)
    names = R.wire_names(entries, r.n_wires)
    by_name = {n: ty for n, _pub, ty in in_leaves}
    leaf_types = [by_name[nm[len("main."):]] for nm in io.input_names]
    return Model(r, io.inputs, io.outputs, names, leaf_types)


# ------------------------------------------------------------------------------------------ iden3 .wtns (native path)

def read_wtns(path: str) -> list[int]:
    """The full witness vector, already in the R1CS's own wire order (ZoKrates >= 0.7.10 writes this
    file next to ``--r1cs`` alongside its own witness format); :func:`r1cs.read_wtns` parses the format."""
    _prime, values = R.read_wtns(path)
    return values


# ------------------------------------------------------------------------------------------ nested argument encoding (0.6.1)

def unflatten_value(spec: dict, values: "list[object]", pos: list[int]):
    """The inverse of :func:`leaf_specs`: a nested JS-ready value (list / dict / scalar) for one ABI
    item, consuming scalar leaves from ``values`` (in :func:`leaf_specs` order) starting at ``pos[0]``."""
    t = spec["type"]
    if t == "array":
        c = spec["components"]
        inner = _inner_spec(c)
        return [unflatten_value(inner, values, pos) for _ in range(c["size"])]
    if t == "struct":
        c = spec["components"]
        obj = {}
        for m in c["members"]:
            obj[m["name"]] = unflatten_value(_inner_spec(m), values, pos)
        return obj
    v = values[pos[0]]
    pos[0] += 1
    return True if v is True else (False if v is False else str(v))


def unflatten_args(items: list[dict], flat_values: list[object]) -> list[object]:
    pos = [0]
    return [unflatten_value(item, flat_values, pos) for item in items]
