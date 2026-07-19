"""Independent native LRAT checker (text format) for the dual-cert harness.

THROWAWAY offline experiment code — never wired into self-test/consensus.

This is the *independent* checker the spec requires next to the pinned Lean
`Std.Tactic.BVDecide.LRAT.Checker` (lean_lrat.py): both must agree before a
SAFE certificate counts. It implements the LRAT format of Cruz-Filipe et al.
(text encoding): hint-driven RUP additions, RAT additions with per-candidate
hint groups, and deletion lines. It is deliberately strict — any malformed or
non-verifying step rejects the whole proof.
"""
from __future__ import annotations

from dataclasses import dataclass


@dataclass
class LratResult:
    ok: bool
    reason: str
    steps: int


def _propagate(alpha: dict[int, bool], hint_ids: list[int], db: dict[int, tuple[int, ...]]):
    """Run hint-driven unit propagation. Mutates alpha.

    Returns "conflict" if the final hint clause is falsified, "open" if all
    hints were consumed as units without reaching a conflict, or an error
    string. Strict: every hint must be unit or falsified when visited, and a
    falsified hint must be the last one.
    """
    for idx, h in enumerate(hint_ids):
        cl = db.get(h)
        if cl is None:
            return f"hint {h} not in clause db"
        unassigned = []
        satisfied = False
        for lit in cl:
            val = alpha.get(abs(lit))
            if val is None:
                unassigned.append(lit)
            elif val == (lit > 0):
                satisfied = True
                break
        if satisfied:
            return f"hint {h} is satisfied, not unit/falsified"
        if not unassigned:
            if idx != len(hint_ids) - 1:
                return "conflict before final hint"
            return "conflict"
        if len(unassigned) > 1:
            return f"hint {h} is not unit"
        lit = unassigned[0]
        alpha[abs(lit)] = lit > 0
    return "open"


def check_lrat(
    n_vars: int, cnf_clauses: list[tuple[int, ...]], lrat_text: str
) -> LratResult:
    db: dict[int, tuple[int, ...]] = {
        i + 1: cl for i, cl in enumerate(cnf_clauses)
    }
    steps = 0
    for raw in lrat_text.splitlines():
        line = raw.strip()
        if not line or line.startswith("c"):
            continue
        toks = line.split()
        steps += 1
        try:
            step_id = int(toks[0])
        except ValueError:
            return LratResult(False, f"bad step id in line: {line[:80]}", steps)

        if len(toks) >= 2 and toks[1] == "d":
            try:
                ids = [int(t) for t in toks[2:]]
            except ValueError:
                return LratResult(False, "bad deletion line", steps)
            if not ids or ids[-1] != 0:
                return LratResult(False, "deletion line missing terminator", steps)
            for cid in ids[:-1]:
                db.pop(cid, None)
            continue

        try:
            ints = [int(t) for t in toks[1:]]
        except ValueError:
            return LratResult(False, "bad literal/hint token", steps)
        if ints.count(0) != 2 or ints[-1] != 0:
            return LratResult(False, "addition line needs two 0 terminators", steps)
        z = ints.index(0)
        clause = tuple(ints[:z])
        hints = ints[z + 1 : -1]
        if any(abs(l) > n_vars for l in clause):
            return LratResult(False, "literal out of variable range", steps)

        # Split RUP hints from RAT groups (first negative id starts RAT part).
        rup_hints: list[int] = []
        rat_groups: list[tuple[int, list[int]]] = []
        i = 0
        while i < len(hints) and hints[i] > 0:
            rup_hints.append(hints[i])
            i += 1
        while i < len(hints):
            if hints[i] >= 0:
                return LratResult(False, "malformed RAT hint group", steps)
            cid = -hints[i]
            i += 1
            group: list[int] = []
            while i < len(hints) and hints[i] > 0:
                group.append(hints[i])
                i += 1
            rat_groups.append((cid, group))

        # Negate the new clause.
        alpha: dict[int, bool] = {}
        tautology = False
        for lit in clause:
            want = not (lit > 0)
            prev = alpha.get(abs(lit))
            if prev is not None and prev != want:
                tautology = True
                break
            alpha[abs(lit)] = want

        verdict = "conflict" if tautology else _propagate(alpha, rup_hints, db)
        if verdict == "conflict":
            pass  # RUP success
        elif verdict != "open":
            return LratResult(False, f"step {step_id}: {verdict}", steps)
        else:
            # RUP did not close: RAT check on the pivot (first literal).
            if not clause:
                return LratResult(False, f"step {step_id}: empty clause RUP failed", steps)
            if not rat_groups:
                return LratResult(False, f"step {step_id}: RUP open and no RAT hints", steps)
            pivot = clause[0]
            groups = dict(rat_groups)
            for cid, cl in db.items():
                if -pivot not in cl:
                    continue
                beta = dict(alpha)
                trivial = False
                for lit in cl:
                    if lit == -pivot:
                        continue
                    val = beta.get(abs(lit))
                    if val is not None and val == (lit > 0):
                        trivial = True  # resolvent already satisfied-negation
                        break
                    beta[abs(lit)] = not (lit > 0)
                if trivial:
                    continue
                if cid not in groups:
                    return LratResult(
                        False, f"step {step_id}: RAT candidate {cid} uncovered", steps
                    )
                sub = _propagate(beta, groups[cid], db)
                if sub != "conflict":
                    return LratResult(
                        False, f"step {step_id}: RAT group {cid}: {sub}", steps
                    )

        if not clause:
            return LratResult(True, "empty clause derived", steps)
        db[step_id] = clause

    return LratResult(False, "proof ended without empty clause", steps)
