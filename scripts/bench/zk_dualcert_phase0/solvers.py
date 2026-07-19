"""Solver portfolio wrappers (CaDiCaL, Kissat, Z3) for the dual-cert harness.

THROWAWAY offline experiment code — never wired into self-test/consensus.
Local binaries/libraries only; no network, no paid APIs.

Timeout policy (spec): a solver timeout or `unknown` is recorded as UNDECIDED,
never as evidence of hardness and never as a solve.
"""
from __future__ import annotations

import os
import subprocess
import tempfile
import time
from dataclasses import dataclass, field


@dataclass
class SolveResult:
    solver: str
    status: str  # SAT | UNSAT | UNKNOWN | ERROR
    wall_s: float
    model: list[int] | None = None  # 0/1 assignment, index 0 unused
    lrat_text: str | None = None
    note: str = ""


def tool_versions() -> dict[str, str]:
    out = {}
    for tool in ("cadical", "kissat"):
        try:
            v = subprocess.run(
                [tool, "--version"], capture_output=True, text=True, timeout=10
            ).stdout.strip()
        except (OSError, subprocess.TimeoutExpired):
            v = "unavailable"
        out[tool] = v
    try:
        import z3

        out["z3-python"] = z3.get_version_string()
    except ImportError:
        out["z3-python"] = "unavailable"
    return out


def _parse_model(stdout: str, n_vars: int) -> list[int]:
    model = [0] * (n_vars + 1)
    for line in stdout.splitlines():
        if line.startswith("v "):
            for tok in line[2:].split():
                lit = int(tok)
                if lit != 0 and abs(lit) <= n_vars:
                    model[abs(lit)] = 1 if lit > 0 else 0
    return model


def run_cadical(
    dimacs: bytes, n_vars: int, timeout_s: float, want_lrat: bool
) -> SolveResult:
    with tempfile.TemporaryDirectory(prefix="zkdc-cadical-") as td:
        cnf_path = os.path.join(td, "d.cnf")
        with open(cnf_path, "wb") as f:
            f.write(dimacs)
        cmd = ["cadical", "-t", str(max(1, int(timeout_s))), cnf_path]
        proof_path = None
        if want_lrat:
            proof_path = os.path.join(td, "d.lrat")
            cmd = [
                "cadical",
                "-t",
                str(max(1, int(timeout_s))),
                "--lrat",
                "--no-binary",
                cnf_path,
                proof_path,
            ]
        t0 = time.perf_counter()
        try:
            proc = subprocess.run(
                cmd, capture_output=True, text=True, timeout=timeout_s + 30
            )
        except subprocess.TimeoutExpired:
            return SolveResult("cadical", "UNKNOWN", time.perf_counter() - t0,
                               note="hard subprocess timeout")
        wall = time.perf_counter() - t0
        if proc.returncode == 10:
            return SolveResult(
                "cadical", "SAT", wall, model=_parse_model(proc.stdout, n_vars)
            )
        if proc.returncode == 20:
            lrat_text = None
            note = ""
            if proof_path and os.path.exists(proof_path):
                size = os.path.getsize(proof_path)
                if size <= 256 * 1024 * 1024:
                    with open(proof_path, "r") as f:
                        lrat_text = f.read()
                else:
                    note = f"lrat too large to load ({size} bytes)"
            return SolveResult("cadical", "UNSAT", wall, lrat_text=lrat_text, note=note)
        if "UNKNOWN" in proc.stdout or proc.returncode == 0:
            return SolveResult("cadical", "UNKNOWN", wall, note="solver time limit")
        return SolveResult(
            "cadical", "ERROR", wall, note=f"rc={proc.returncode}"
        )


def run_kissat(dimacs: bytes, n_vars: int, timeout_s: float) -> SolveResult:
    with tempfile.TemporaryDirectory(prefix="zkdc-kissat-") as td:
        cnf_path = os.path.join(td, "d.cnf")
        with open(cnf_path, "wb") as f:
            f.write(dimacs)
        cmd = ["kissat", f"--time={max(1, int(timeout_s))}", cnf_path]
        t0 = time.perf_counter()
        try:
            proc = subprocess.run(
                cmd, capture_output=True, text=True, timeout=timeout_s + 30
            )
        except subprocess.TimeoutExpired:
            return SolveResult("kissat", "UNKNOWN", time.perf_counter() - t0,
                               note="hard subprocess timeout")
        wall = time.perf_counter() - t0
        if proc.returncode == 10:
            return SolveResult(
                "kissat", "SAT", wall, model=_parse_model(proc.stdout, n_vars)
            )
        if proc.returncode == 20:
            return SolveResult("kissat", "UNSAT", wall)
        return SolveResult("kissat", "UNKNOWN", wall, note="solver time limit")


def run_z3(
    clauses: list[tuple[int, ...]], n_vars: int, timeout_s: float
) -> SolveResult:
    import z3

    t0 = time.perf_counter()
    s = z3.Solver()
    s.set("timeout", int(timeout_s * 1000))
    xs = [None] + [z3.Bool(f"x{i}") for i in range(1, n_vars + 1)]
    for cl in clauses:
        s.add(z3.Or([xs[l] if l > 0 else z3.Not(xs[-l]) for l in cl]))
    res = s.check()
    wall = time.perf_counter() - t0
    if res == z3.sat:
        m = s.model()
        model = [0] * (n_vars + 1)
        for i in range(1, n_vars + 1):
            v = m.eval(xs[i], model_completion=True)
            model[i] = 1 if z3.is_true(v) else 0
        return SolveResult("z3", "SAT", wall, model=model)
    if res == z3.unsat:
        return SolveResult("z3", "UNSAT", wall)
    # `unknown` (incl. timeout) is UNDECIDED — never hardness evidence.
    return SolveResult("z3", "UNKNOWN", wall, note=str(s.reason_unknown()))
