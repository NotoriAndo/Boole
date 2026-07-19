"""Wiring for the pinned Lean LRAT checker (leanchecker/ subproject).

THROWAWAY offline experiment code — never wired into self-test/consensus.

The subproject pins the same toolchain as the consensus checker
(leanprover/lean4:v4.29.1) but is fully separate: nothing under lean/checker/
(pinned files, SHA256SUMS) is read or modified.

Cost accounting note (recorded honestly in the report): `LRAT.check` runs as
COMPILED code inside a `lake`-built executable, so Lean elaboration heartbeats
and `maxRecDepth` do not apply to this path — the deterministic budget analog
for consensus would be a step/step-size bound on the checker itself. We record
wall-clock (whole process and the in-process check phase) and peak RSS.
"""
from __future__ import annotations

import os
import re
import subprocess
import time
from dataclasses import dataclass

HERE = os.path.dirname(os.path.abspath(__file__))
PROJECT = os.path.join(HERE, "leanchecker")
EXE = os.path.join(PROJECT, ".lake", "build", "bin", "zk_dualcert_lrat")


def _elan_env() -> dict:
    env = dict(os.environ)
    elan_bin = os.path.expanduser("~/.elan/bin")
    env["PATH"] = elan_bin + os.pathsep + env.get("PATH", "")
    return env


def ensure_built() -> str:
    """Build the checker once; returns the pinned toolchain string."""
    if not os.path.exists(EXE):
        proc = subprocess.run(
            ["lake", "build"],
            cwd=PROJECT,
            env=_elan_env(),
            capture_output=True,
            text=True,
            timeout=1800,
        )
        if proc.returncode != 0 or not os.path.exists(EXE):
            raise RuntimeError(
                f"lake build failed:\n{proc.stdout}\n{proc.stderr}"
            )
    with open(os.path.join(PROJECT, "lean-toolchain")) as f:
        return f.read().strip()


@dataclass
class LeanLratResult:
    ok: bool
    status: str  # ACCEPT | REJECT | ERROR | TIMEOUT
    wall_s: float  # whole-process wall clock
    check_s: float | None  # in-process LRAT.check phase only
    peak_rss_bytes: int | None
    proof_steps: int | None
    note: str = ""


def check(cnf_path: str, lrat_path: str, timeout_s: float = 600) -> LeanLratResult:
    ensure_built()
    cmd = ["/usr/bin/time", "-l", EXE, cnf_path, lrat_path]
    t0 = time.perf_counter()
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout_s
        )
    except subprocess.TimeoutExpired:
        return LeanLratResult(
            False, "TIMEOUT", time.perf_counter() - t0, None, None, None,
            note=f"exceeded {timeout_s}s",
        )
    wall = time.perf_counter() - t0

    check_s = None
    steps = None
    ok = False
    m = re.search(r"result=(\w+) check_ns=(\d+).*proof_steps=(\d+)", proc.stdout)
    if m:
        ok = m.group(1) == "true"
        check_s = int(m.group(2)) / 1e9
        steps = int(m.group(3))

    peak = None
    rss = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
    if rss:
        peak = int(rss.group(1))

    if proc.returncode == 0 and ok:
        return LeanLratResult(True, "ACCEPT", wall, check_s, peak, steps)
    if proc.returncode == 1:
        return LeanLratResult(False, "REJECT", wall, check_s, peak, steps)
    return LeanLratResult(
        False, "ERROR", wall, check_s, peak, steps,
        note=(proc.stderr.strip().splitlines() or [""])[0],
    )
