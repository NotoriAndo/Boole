"""Input sampling, witness generation with circom's wasm witness generator, and mutants.

Inputs are sampled deterministically (seeded by the package id) from a fixed strategy list that
includes the boundary values 0, 1 and p-1; every witness returned by the generator is re-checked
by the Python R1CS oracle before it is used.
"""
from __future__ import annotations

import json
import os
import random
import subprocess
from dataclasses import dataclass

from . import r1cs as R

STRATEGIES = ["zero", "one", "minus_one", "bit", "small", "byte", "u32", "u64", "field", "shared"]
RUNNER_JS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "witness_runner.js")


@dataclass
class InputSignal:
    key: str            # circom input name as the witness calculator expects it (no "main.", no indices)
    wires: list[int]    # wires in flattening order


def input_signals(io: R.MainIo) -> list[InputSignal]:
    """Group the main component's input wires by signal (declaration order = wire order)."""
    groups: list[InputSignal] = []
    for wire, name in zip(io.inputs, io.input_names):
        key = R.signal_base(name)[len("main."):]
        if groups and groups[-1].key == key:
            groups[-1].wires.append(wire)
        elif any(g.key == key for g in groups):
            raise ValueError(f"input signal {key} is not contiguous in the wire order")
        else:
            groups.append(InputSignal(key, [wire]))
    return groups


def sample_value(strategy: str, rng: random.Random, p: int, shared: int = 0) -> int:
    """One input value; ``shared`` is the attempt's common value (equal inputs expose degenerate cases)."""
    if strategy == "shared":
        return shared
    if strategy == "zero":
        return 0
    if strategy == "one":
        return 1
    if strategy == "minus_one":
        return p - 1
    if strategy == "bit":
        return rng.randrange(2)
    if strategy == "small":
        return rng.randrange(16)
    if strategy == "byte":
        return rng.randrange(256)
    if strategy == "u32":
        return rng.randrange(2**32)
    if strategy == "u64":
        return rng.randrange(2**64)
    if strategy == "field":
        return rng.randrange(p)
    raise ValueError(strategy)


UNIFORM_ROUNDS = 2


def uniform_attempts() -> int:
    return len(STRATEGIES) * UNIFORM_ROUNDS


def sample_inputs(signals: list[InputSignal], p: int, seed: str, attempts: int,
                  allowed: list[str] | None = None, uniform: bool = True) -> list[dict[str, list[str]]]:
    """``attempts`` input objects.  With ``uniform``, the first :func:`uniform_attempts` use one
    strategy for every input (each strategy twice); the rest alternate between one strategy per
    signal and one strategy per signal element, drawn from ``allowed`` (default: all strategies).

    The driver runs two phases: the uniform phase, then a mixed phase restricted to the strategies
    whose uniform attempts produced a witness (inputs that must be bits or small then stay valid)."""
    rng = random.Random(seed)
    pool = [s for s in (allowed or STRATEGIES) if s in STRATEGIES] or STRATEGIES
    out = []
    for k in range(attempts):
        obj = {}
        shared = sample_value(rng.choice([s for s in pool if s != "shared"] or ["zero"]), rng, p)
        for sig in signals:
            if uniform and k < uniform_attempts():
                strategies = [STRATEGIES[k % len(STRATEGIES)]] * len(sig.wires)
            elif k % 2 == 0:
                strategies = [rng.choice(pool)] * len(sig.wires)
            else:
                strategies = [rng.choice(pool) for _ in sig.wires]
            obj[sig.key] = [str(sample_value(s, rng, p, shared)) for s in strategies]
        out.append(obj)
    return out


GRID_MAX_ELEMENTS = 6


def boundary_grid(signals: list[InputSignal], p: int, max_elements: int = GRID_MAX_ELEMENTS) -> list[dict[str, list[str]]]:
    """Every assignment of {0, 1, p-1} to the input elements when there are at most ``max_elements``
    of them (at most 3^6 = 729 objects); otherwise none.  Used only to seed the DET searches, where
    degenerate points (zero denominators, equal points) matter."""
    n = sum(len(s.wires) for s in signals)
    if not signals or n > max_elements:
        return []
    values = ["0", "1", str(p - 1)]
    out = []
    for code in range(3 ** n):
        flat = []
        for _ in range(n):
            flat.append(values[code % 3])
            code //= 3
        obj, k = {}, 0
        for sig in signals:
            obj[sig.key] = flat[k:k + len(sig.wires)]
            k += len(sig.wires)
        out.append(obj)
    return out


def successful_strategies(generated: list["GeneratedWitness"]) -> list[str]:
    """Strategies whose uniform-phase attempts produced a witness (``generated`` is the uniform phase)."""
    return sorted({STRATEGIES[k % len(STRATEGIES)] for k, g in enumerate(generated) if g.witness is not None})


def input_vector(obj: dict[str, list[str]], signals: list[InputSignal]) -> tuple[int, ...]:
    return tuple(int(v) for sig in signals for v in obj[sig.key])


@dataclass
class GeneratedWitness:
    inputs: dict[str, list[str]]
    witness: list[int] | None
    error: str | None


def run_generator(node: str, wc_js: str, wasm: str, inputs: list[dict], workdir: str, timeout: float = 600,
                  tag: str = "gen") -> list[GeneratedWitness]:
    os.makedirs(workdir, exist_ok=True)
    inp = os.path.join(workdir, f"{tag}.inputs.jsonl")
    outp = os.path.join(workdir, f"{tag}.witnesses.jsonl")
    with open(inp, "w", encoding="utf-8") as f:
        for obj in inputs:
            f.write(json.dumps(obj) + "\n")
    env = {"PATH": os.path.dirname(node) + ":/usr/bin:/bin", "HOME": workdir, "NODE_OPTIONS": "--max-old-space-size=8192"}
    r = subprocess.run([node, RUNNER_JS, wc_js, wasm, inp, outp], capture_output=True, text=True, timeout=timeout,
                       env=env, cwd=workdir)
    if r.returncode != 0:
        raise RuntimeError(f"witness runner failed: {(r.stderr or r.stdout)[-500:]}")
    res = []
    with open(outp, encoding="utf-8") as f:
        for obj, line in zip(inputs, f):
            d = json.loads(line)
            res.append(GeneratedWitness(obj, [int(v) for v in d["w"]] if d["ok"] else None, d.get("err")))
    os.remove(outp)
    return res


def collect_real(r: R.R1cs, signals: list[InputSignal], generated: list[GeneratedWitness], want: int):
    """Distinct (by input vector) generator witnesses that the oracle accepts, in sampling order.

    Returns (accepted, oracle_rejected, generator_errors) where ``accepted`` is a list of at most
    ``want`` (input object, witness) pairs."""
    seen, accepted, oracle_rejected, errors = set(), [], [], {}
    for g in generated:
        if g.witness is None:
            key = (g.error or "").split("\n")[0][:120]
            errors[key] = errors.get(key, 0) + 1
            continue
        vec = input_vector(g.inputs, signals)
        if vec in seen:
            continue
        seen.add(vec)
        if R.satisfies(r, g.witness):
            if len(accepted) < want:
                accepted.append((g.inputs, g.witness))
        else:
            oracle_rejected.append((g.inputs, g.witness))
    return accepted, oracle_rejected, errors


def mutants(r: R.R1cs, bases: list[list[int]], n: int, seed: str) -> list[dict]:
    """Single-wire mutations of real witnesses (wire 0 included, so the constant-one check is exercised)."""
    rng = random.Random(seed + "/mutants")
    out = []
    p = r.prime
    for k in range(n):
        bi = k % len(bases)
        w = list(bases[bi])
        wire = 0 if k == 0 else rng.randrange(r.n_wires)
        old = w[wire]
        new = (old + rng.choice([1, p - 1, rng.randrange(1, p)])) % p
        if new == old:
            new = (old + 1) % p
        w[wire] = new
        out.append({"base": bi, "wire": wire, "old": str(old), "new": str(new), "witness": w,
                    "oracle": R.satisfies(r, w)})
    return out


def write_witness(path: str, w: list[int]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(str(v) for v in w) + "\n")
