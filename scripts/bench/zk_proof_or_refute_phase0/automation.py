"""Fixed automation portfolio (AUTO arm) + structural attacker (S1).

THROWAWAY offline experiment. AUTO is the "no-LLM" baseline: a fixed tactic
list for the TRUE direction, and — using off-chain enumeration to FIND a
counterexample (allowed by prereg §A) — a fixed refuter template for the FALSE
direction. A success counts only if the emitted Lean text passes intake and
elaborates (prereg §A: label_discovery_only otherwise).
"""
from __future__ import annotations

import itertools

from oracle import evaluate
from verify import verify

# Fixed tactic portfolio for TRUE attempts (order fixed, no per-problem tuning).
TRUE_TACTICS = [
    "by intro_vars; omega",         # placeholder expanded below
]

# We expand intro over the actual vars. Concretely we try these bodies:
DEFS = "Zk.bitsVal, Zk.polyEval, Zk.dot, Zk.inRange, Zk.isBit, Zk.boolConstraint, Zk.r1csSat, Zk.p"


def true_bodies(n_vars: int) -> list[str]:
    return [
        f"by intros; simp_all only [{DEFS}] <;> omega",
        f"by intros; simp_all [{DEFS}] <;> omega",
        f"by intros; omega",
        f"by intros; simp_all [{DEFS}]",
        f"by intros; decide",
        f"by intros a b c d e; omega",
    ]


def refute_bodies(vars_, cex: dict) -> list[str]:
    """Fixed refuter templates: specialize at the enumerated counterexample."""
    args = " ".join(str(cex[v]) for v in vars_)
    return [
        f"by intro h; have hc := h {args}; simp only [{DEFS}] at hc; omega",
        f"by intro h; have hc := h {args}; simp_all [{DEFS}]",
        f"by intro h; have hc := h {args}; revert hc; decide",
        f"by intro h; have hc := h {args}; simp only [{DEFS}] at hc <;> omega",
        f"by intro h; exact absurd (h {args}) (by simp only [{DEFS}] <;> omega)",
    ]


def auto_solve(problem, oracle_spec, timeout: float = 20.0) -> dict:
    """Return {solved, polarity, label, wall_s, label_discovery_only, reason}."""
    truth = evaluate(oracle_spec)
    label = truth["label"]
    n_vars = len(oracle_spec["vars"])
    total_wall = 0.0

    if label == "TRUE":
        for body in true_bodies(n_vars):
            r = verify(problem.statement, "TRUE", body, timeout=timeout / 5)
            total_wall += r["wall_s"]
            if r["accepted"]:
                return {"solved": True, "polarity": "TRUE", "label": label,
                        "wall_s": total_wall, "label_discovery_only": False, "reason": "auto_true"}
        # found nothing provable though oracle says true -> label known, no proof
        return {"solved": False, "polarity": "TRUE", "label": label,
                "wall_s": total_wall, "label_discovery_only": True, "reason": "no_auto_proof"}
    else:
        cex = truth["counterexample"]
        for body in refute_bodies(oracle_spec["vars"], cex):
            r = verify(problem.statement, "FALSE", body, timeout=timeout / 5)
            total_wall += r["wall_s"]
            if r["accepted"]:
                return {"solved": True, "polarity": "FALSE", "label": label,
                        "wall_s": total_wall, "label_discovery_only": False, "reason": "auto_refute"}
        return {"solved": False, "polarity": "FALSE", "label": label,
                "wall_s": total_wall, "label_discovery_only": True, "reason": "cex_found_no_proof"}


# ---- S1 structural attacker -------------------------------------------------
# Knows the generator source and mutation rules. Predicts the label from the
# mutation kind ALONE (no solving), to measure how much the mutation leaks.

# Prior built from the generator's structure (attacker is generator-omniscient):
MUTATION_LABEL_PRIOR = {
    "identity": "TRUE",
    "add_premise": "TRUE",
    "flip_eq_ineq": "TRUE",
    "drop_premise": "FALSE",
    "narrow_range": "FALSE",
    "bump_const": "FALSE",
    "widen_range": "FALSE",
    "flip_lt_le": "TRUE",
}


def structural_predict(problem) -> str:
    return MUTATION_LABEL_PRIOR.get(problem.mutation, "TRUE")
