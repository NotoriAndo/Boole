"""Witness generation for ZoKrates DET packages: input sampling (reusing :mod:`witness`'s strategies,
one artificial single-wire "signal" per flattened ABI leaf) and ``compute-witness`` invocation for both
ZoKrates generations.  Both the modern (``--r1cs``) and legacy (``.ztf``) compilers accept the same
plain ``-a <v0> <v1> ...`` flattened-argument form, in ABI declaration order
(:func:`zokrates_r1cs.flatten_inputs`'s order) -- confirmed on real compiles of both -- so one sampler
and one invocation path serve both; only the result's encoding differs (circom ``.wtns`` vs. a
plain-text ``"<variable> <value>"`` file), decoded by the caller-supplied ``read_result``.
"""
from __future__ import annotations

import os
import subprocess

from . import witness as W
from . import zokrates_r1cs as ZR
from . import zokrates_toolchain as T

LEAF_DOMAIN = {"bool": "binary", "u8": ("maxbit", 8), "u16": ("maxbit", 16), "u32": ("maxbit", 32),
              "u64": ("maxbit", 64), "field": None}


def leaf_signals(leaf_types: list) -> list:
    return [W.InputSignal(f"leaf{i}", [i], LEAF_DOMAIN.get(ty)) for i, ty in enumerate(leaf_types)]


def sample_flat_values(leaf_types: list, p: int, seed: str, attempts: int) -> list:
    """``attempts`` flat (ABI declaration order) integer argument lists."""
    signals = leaf_signals(leaf_types)
    objs = W.sample_inputs(signals, p, seed, attempts)
    return [[int(o[f"leaf{i}"][0]) for i in range(len(leaf_types))] for o in objs]


class WitnessError(RuntimeError):
    pass


def compute_witness_modern(pin: dict, program: str, abi_path: str, args: list, workdir: str,
                           timeout: float = 60) -> tuple:
    wtns = os.path.join(workdir, "out.wtns")
    witf = os.path.join(workdir, "witness")
    for p in (wtns, witf):
        if os.path.exists(p):
            os.remove(p)
    try:
        r = T.run(pin, ["compute-witness", "-i", program, "-o", witf, "-s", abi_path,
                       "--circom-witness", wtns, "-a", *[str(v) for v in args]], cwd=workdir, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, "compute-witness timed out"
    if r.returncode != 0 or not os.path.isfile(wtns):
        err = (r.stderr or r.stdout or "compute-witness failed").strip()
        return None, err.splitlines()[-1][:300] if err else "compute-witness failed"
    return ZR.read_wtns(wtns), None


def compute_witness_legacy(pin: dict, program: str, abi_path: str, args: list, workdir: str,
                           renumber: dict, n_wires: int, p: int, timeout: float = 60) -> tuple:
    witf = os.path.join(workdir, "witness")
    if os.path.exists(witf):
        os.remove(witf)
    try:
        r = T.run(pin, ["compute-witness", "-i", program, "-o", witf, "-s", abi_path,
                       "-a", *[str(v) for v in args]], cwd=workdir, timeout=timeout)
    except subprocess.TimeoutExpired:
        return None, "compute-witness timed out"
    if r.returncode != 0 or not os.path.isfile(witf):
        err = (r.stderr or r.stdout or "compute-witness failed").strip()
        return None, err.splitlines()[-1][:300] if err else "compute-witness failed"
    from . import zokrates_legacy as L
    with open(witf, encoding="utf-8") as f:
        values = L.parse_witness_text(f.read())
    try:
        w = L.witness_vector(values, renumber, n_wires, p)
    except ValueError as exc:
        return None, str(exc)
    return w, None
