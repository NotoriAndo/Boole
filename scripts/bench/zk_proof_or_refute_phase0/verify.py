"""Lean verification wrapper (pinned v4.29.1 via the zklib project).

THROWAWAY offline experiment. Renders a submission into the zklib lake project
and elaborates it under the pinned checker budget. A submission is accepted iff
`lean` returns 0 AND the source passes the same forbidden-token discipline the
consensus intake uses (no sorry/admit/native_decide/#eval/run_tac/unsafe/axiom).

TRUE submission:  theorem probe : <stmt> := <proof>
FALSE submission: theorem probe : ¬ (<stmt>) := <refuter>
"""
from __future__ import annotations

import os
import re
import subprocess
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ZKLIB = os.path.join(HERE, "zklib")
LAKE = os.path.expanduser("~/.elan/bin/lake")

FORBIDDEN = re.compile(
    r"\b(sorry|admit|native_decide|run_tac|unsafe|axiom|proof_irrel_ax)\b|#eval|#check"
)
MAX_HEARTBEATS = 400000
MAX_REC_DEPTH = 512


def _blank_comments(src: str) -> str:
    src = re.sub(r"--[^\n]*", "", src)
    src = re.sub(r"/-.*?-/", "", src, flags=re.DOTALL)
    return src


def intake_ok(body: str) -> tuple[bool, str]:
    scan = _blank_comments(body)
    m = FORBIDDEN.search(scan)
    if m:
        return False, f"forbidden_token:{m.group(0)}"
    return True, ""


def verify(statement: str, polarity: str, body: str, timeout: float = 25.0) -> dict:
    """polarity: 'TRUE' -> prove statement; 'FALSE' -> prove ¬statement.
    body: the Lean proof text after ':='."""
    ok, reason = intake_ok(body)
    if not ok:
        return {"accepted": False, "reason": reason, "wall_s": 0.0}
    goal = statement if polarity == "TRUE" else f"¬ ({statement})"
    src = (
        "import Zk\n\n"
        f"set_option maxHeartbeats {MAX_HEARTBEATS} in\n"
        f"theorem probe : {goal} := {body}\n"
    )
    fd, path = tempfile.mkstemp(suffix=".lean", dir=os.path.join(ZKLIB))
    with os.fdopen(fd, "w") as f:
        f.write(src)
    try:
        t0 = time.perf_counter()
        try:
            proc = subprocess.run(
                [LAKE, "env", "lean", f"-DmaxRecDepth={MAX_REC_DEPTH}", path],
                cwd=ZKLIB, capture_output=True, text=True, timeout=timeout,
            )
        except subprocess.TimeoutExpired:
            return {"accepted": False, "reason": "timeout", "wall_s": timeout}
        wall = time.perf_counter() - t0
        if proc.returncode == 0:
            return {"accepted": True, "reason": "ok", "wall_s": wall}
        return {
            "accepted": False,
            "reason": "lean_error",
            "wall_s": wall,
            "stderr": (proc.stdout + proc.stderr)[-300:],
        }
    finally:
        os.unlink(path)


if __name__ == "__main__":
    r = verify("∀ b : Nat, b ≤ 1 → Zk.boolConstraint b", "TRUE",
               "by intro b hb; rcases (by omega : b = 0 ∨ b = 1) with h | h <;> subst h <;> rfl")
    print("TRUE proof:", r)
    r = verify("∀ x : Nat, x = 0", "FALSE",
               "by intro h; have := h 1; omega")
    print("FALSE refute:", r)
