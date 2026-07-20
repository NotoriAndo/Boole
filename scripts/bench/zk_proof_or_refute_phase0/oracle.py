"""Independent ground-truth oracle (MEASUREMENT ONLY).

Brute-forces the true/false label of a generated statement over a small finite
domain. This is SEPARATE from the generator (which never computes the label)
and is used only to: score S1 leakage (can an attacker predict what the oracle
finds?), S4 balance, and to give the automation/LLM arms a checkable target.

For a statement ∀ vars, P1..Pm → C:
  - TRUE  if for every assignment in the domain satisfying all premises, C holds.
  - FALSE (with counterexample) if some assignment satisfies all premises but not C.
Over the bounded domain this is decidable by enumeration; because every base
theorem is bounded-domain (bits, small ranges) the bounded verdict matches the
Lean-provable verdict for the templates used (checked in S0 totality).
"""
from __future__ import annotations

import itertools

from generator import oracle_predicate, BASE


def evaluate(oracle_spec: dict):
    """Return dict: label 'TRUE'/'FALSE', counterexample (or None),
    n_satisfying_premises, n_total."""
    vars_ = oracle_spec["vars"]
    bound = oracle_spec["domain_bound"]
    spec_like = {"vars": vars_}
    pfs, cf = oracle_predicate(
        spec_like, oracle_spec["premises"], oracle_spec["concl"], oracle_spec["kconst"]
    )
    n_prem_ok = 0
    counterexample = None
    total = 0
    for combo in itertools.product(range(bound), repeat=len(vars_)):
        total += 1
        v = dict(zip(vars_, combo))
        try:
            if all(pf(v) for pf in pfs):
                n_prem_ok += 1
                if not cf(v):
                    counterexample = dict(v)
                    break
        except (KeyError, ZeroDivisionError):
            continue
    label = "FALSE" if counterexample is not None else "TRUE"
    # vacuous-true guard: if no assignment satisfies premises, mark it
    vacuous = (counterexample is None and n_prem_ok == 0)
    return {
        "label": label,
        "counterexample": counterexample,
        "n_premises_satisfied": n_prem_ok,
        "n_total": total,
        "vacuous_true": vacuous,
    }


if __name__ == "__main__":
    import sys, json
    from generator import generate
    seed = sys.argv[1] if len(sys.argv) > 1 else "demo-0"
    band = sys.argv[2] if len(sys.argv) > 2 else "easy"
    p = generate(seed, band)
    print("stmt:", p.statement)
    print(json.dumps(evaluate(p.oracle_spec), indent=2))
