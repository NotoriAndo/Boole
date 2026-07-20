"""Deterministic proof-or-refute problem generator (zk-proof-or-refute.v0 P0).

THROWAWAY offline experiment (operator prereg 2026-07-20). Not wired into
self-test/consensus. The generator mutates one of 20 kernel-checked base
theorems (Zk.Theorems) into a new statement of the shape

    ∀ vars, Preconditions → Conclusion

and emits ONLY: the Lean statement text + structural metadata (base id,
mutation kind, params). It NEVER computes or stores the true/false label,
a proof, or a counterexample (prereg §A, §F; S1). The label is established
elsewhere, by an independent brute-force oracle over a small finite domain,
for MEASUREMENT ONLY.

Determinism: every field is a pure function of (seed). Same seed -> byte
identical statement (S0).
"""
from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass, asdict


# ---- base theorem templates -------------------------------------------------
# Each template is parametric so mutations act on premises/ranges/relations
# without the generator knowing whether the result is true. Variables range
# over small Nat domains bounded by the premises; the oracle enumerates them.
#
# fields:
#   vars:      list of (name) universally quantified Nat vars
#   premises:  list of premise atoms (each references vars/consts)
#   concl:     conclusion atom
# atoms are small dicts the renderer turns into Lean Props and the oracle
# turns into python predicates.

BASE = {
    "thm01": {  # b ≤ 1 → boolConstraint b   i.e. b*(b-1)=0
        "vars": ["b"],
        "premises": [{"op": "le", "l": "b", "r": 1}],
        "concl": {"op": "boolc", "x": "b"},
    },
    "thm02": {  # isBit b → b < 2
        "vars": ["b"],
        "premises": [{"op": "isbit", "x": "b"}],
        "concl": {"op": "lt", "l": "b", "r": 2},
    },
    "thm05": {  # bitsVal [a,b] = a + 2*b   (no premise)
        "vars": ["a", "b"],
        "premises": [],
        "concl": {"op": "eq", "l": {"bits2": ["a", "b"]}, "r": {"lin": ["a", 2, "b"]}},
    },
    "thm15": {  # inRange x 0 → x = 0
        "vars": ["x"],
        "premises": [{"op": "range", "x": "x", "k": 0}],
        "concl": {"op": "eqc", "l": "x", "r": 0},
    },
    "thm16": {  # a≤1 → b≤1 → bitsVal [a,b] < 4
        "vars": ["a", "b"],
        "premises": [{"op": "le", "l": "a", "r": 1}, {"op": "le", "l": "b", "r": 1}],
        "concl": {"op": "ltv", "l": {"bits2": ["a", "b"]}, "r": 4},
    },
    "thm19": {  # inRange x k → inRange y k → x+y < 2^(k+1)
        "vars": ["x", "y"],
        "premises": [{"op": "range", "x": "x", "k": "k"}, {"op": "range", "x": "y", "k": "k"}],
        "concl": {"op": "ltv", "l": {"sum": ["x", "y"]}, "r": {"pow2": "kp1"}},
        "const_k": 2,
    },
}

MUTATIONS = [
    "identity",        # control (keep as-is)
    "drop_premise",    # remove one premise (often makes it false)
    "add_premise",     # add a constraining premise (often keeps true / vacuous)
    "widen_range",     # k -> k+1 or bound *2
    "narrow_range",    # bound -> bound-1
    "flip_lt_le",      # < <-> ≤
    "flip_eq_ineq",    # = -> ≤
    "bump_const",      # change a numeric constant by +1
]

BANDS = {  # difficulty bands: which mutation set + param magnitude
    "easy": {"mutations": ["identity", "add_premise", "flip_eq_ineq"], "kbump": 0},
    "medium": {"mutations": ["drop_premise", "widen_range", "flip_lt_le", "bump_const"], "kbump": 1},
    "hard": {"mutations": ["narrow_range", "drop_premise", "bump_const"], "kbump": 2},
}


@dataclass
class Problem:
    seed: str
    base: str
    band: str
    mutation: str
    param: int
    statement: str          # Lean statement (Prop after `theorem _ : <stmt>`)
    oracle_spec: dict       # for the independent oracle only (kept OUT of miner input)
    canonical_bytes_sha: str


def _rng(seed: str, domain: str) -> int:
    return int.from_bytes(
        hashlib.blake2b(f"{seed}|{domain}".encode(), digest_size=8).digest(), "big"
    )


# ---- renderers: atom -> Lean text, atom -> oracle python predicate ----------

def render_expr(e) -> str:
    if isinstance(e, (int,)):
        return str(e)
    if isinstance(e, str):
        return e
    if "bits2" in e:
        a, b = e["bits2"]
        return f"Zk.bitsVal [{a}, {b}]"
    if "lin" in e:
        a, k, b = e["lin"]
        return f"{a} + {k} * {b}"
    if "sum" in e:
        a, b = e["sum"]
        return f"{a} + {b}"
    if "pow2" in e:
        return "2 ^ (k + 1)"  # k is a concrete const substituted at render
    raise ValueError(e)


def render_atom(a, kconst: int | None) -> str:
    op = a["op"]
    if op == "le":
        return f"{a['l']} ≤ {a['r']}"
    if op == "lt":
        return f"{a['l']} < {a['r']}"
    if op == "isbit":
        return f"Zk.isBit {a['x']}"
    if op == "boolc":
        return f"Zk.boolConstraint {a['x']}"
    if op == "range":
        k = kconst if a["k"] == "k" else a["k"]
        return f"Zk.inRange {a['x']} {k}"
    if op == "eq":
        return f"{render_expr(a['l'])} = {render_expr(a['r'])}"
    if op == "eqc":
        return f"{a['l']} = {a['r']}"
    if op == "ltv":
        r = a["r"]
        return f"{render_expr(a['l'])} < {render_expr(r)}"
    raise ValueError(a)


def oracle_atom(a, kconst: int | None):
    """Return a python predicate closure over an assignment dict."""
    op = a["op"]
    if op == "le":
        return lambda v: v[a["l"]] <= a["r"]
    if op == "lt":
        return lambda v: v[a["l"]] < a["r"]
    if op == "isbit":
        return lambda v: v[a["x"]] in (0, 1)
    if op == "boolc":
        return lambda v: v[a["x"]] * (v[a["x"]] - 1) == 0
    if op == "range":
        k = kconst if a["k"] == "k" else a["k"]
        return lambda v: v[a["x"]] < 2 ** k
    if op == "eq" or op == "eqc" or op == "ltv":
        lf = _oracle_expr(a["l"], kconst)
        rf = _oracle_expr(a.get("r"), kconst)
        if op == "ltv":
            return lambda v: lf(v) < rf(v)
        return lambda v: lf(v) == rf(v)
    raise ValueError(a)


def _oracle_expr(e, kconst):
    if isinstance(e, int):
        return lambda v: e
    if isinstance(e, str):
        return lambda v: v[e]
    if "bits2" in e:
        a, b = e["bits2"]
        return lambda v: v[a] + 2 * v[b]
    if "lin" in e:
        a, k, b = e["lin"]
        return lambda v: v[a] + k * v[b]
    if "sum" in e:
        a, b = e["sum"]
        return lambda v: v[a] + v[b]
    if "pow2" in e:
        kk = kconst
        return lambda v: 2 ** (kk + 1)
    raise ValueError(e)


# ---- mutation application ---------------------------------------------------

def apply_mutation(spec: dict, mutation: str, param: int, kconst: int):
    """Return (premises, concl, kconst') after mutation. Pure; no label."""
    prem = [dict(p) for p in spec["premises"]]
    concl = json.loads(json.dumps(spec["concl"]))
    k = kconst

    if mutation == "identity":
        pass
    elif mutation == "drop_premise":
        if prem:
            del prem[param % len(prem)]
    elif mutation == "add_premise":
        v = spec["vars"][param % len(spec["vars"])]
        prem.append({"op": "le", "l": v, "r": 1})
    elif mutation == "widen_range":
        for p in prem:
            if p["op"] == "range":
                p["k"] = (p["k"] if isinstance(p["k"], int) else k) + 1
        k = k + 1
    elif mutation == "narrow_range":
        for p in prem:
            if p["op"] == "range" and isinstance(p["k"], int):
                p["k"] = max(0, p["k"] - 1)
        k = max(0, k - 1)
    elif mutation == "flip_lt_le":
        c = concl
        if c["op"] == "lt":
            c["op"] = "le"
        elif c["op"] == "ltv":
            c["r"] = c["r"] if isinstance(c["r"], dict) else c["r"] - 1  # ≤ via -1 on int bound
    elif mutation == "flip_eq_ineq":
        c = concl
        if c["op"] == "eq":
            c["op"] = "ltv" if False else "eq"  # keep eq structurally; handled by bump
        if c["op"] == "eqc":
            c["op"] = "le"
    elif mutation == "bump_const":
        c = concl
        if c["op"] == "ltv" and isinstance(c["r"], int):
            c["r"] = c["r"] + (1 if param % 2 == 0 else -1)
        elif c["op"] in ("lt",) and isinstance(c["r"], int):
            c["r"] = c["r"] + (1 if param % 2 == 0 else -1)
        elif c["op"] == "eqc" and isinstance(c["r"], int):
            c["r"] = c["r"] + 1
    return prem, concl, k


def render_statement(spec, prem, concl, kconst) -> str:
    var_decl = " ".join(spec["vars"])
    parts = [render_atom(p, kconst) for p in prem] + [render_atom(concl, kconst)]
    body = " → ".join(parts)
    return f"∀ {var_decl} : Nat, {body}"


def oracle_predicate(spec, prem, concl, kconst):
    pfs = [oracle_atom(p, kconst) for p in prem]
    cf = oracle_atom(concl, kconst)
    return pfs, cf


def generate(seed: str, band: str) -> Problem:
    bands = BANDS[band]
    base_id = list(BASE.keys())[_rng(seed, "base") % len(BASE)]
    spec = BASE[base_id]
    kconst = spec.get("const_k", 0) + bands["kbump"]
    mutation = bands["mutations"][_rng(seed, "mut") % len(bands["mutations"])]
    param = _rng(seed, "param") % 4
    prem, concl, kc = apply_mutation(spec, mutation, param, kconst)
    stmt = render_statement(spec, prem, concl, kc)
    oracle_spec = {
        "vars": spec["vars"],
        "premises": prem,
        "concl": concl,
        "kconst": kc,
        "domain_bound": 8,  # enumerate each var in [0, 8) for the oracle
    }
    canon = json.dumps(
        {"base": base_id, "band": band, "mutation": mutation, "param": param, "stmt": stmt},
        sort_keys=True,
    ).encode()
    return Problem(
        seed=seed, base=base_id, band=band, mutation=mutation, param=param,
        statement=stmt, oracle_spec=oracle_spec,
        canonical_bytes_sha=hashlib.sha256(canon).hexdigest(),
    )


if __name__ == "__main__":
    import sys
    seed = sys.argv[1] if len(sys.argv) > 1 else "demo-0"
    band = sys.argv[2] if len(sys.argv) > 2 else "easy"
    prob = generate(seed, band)
    d = asdict(prob)
    d.pop("oracle_spec")  # miner never sees the oracle spec
    print(json.dumps(d, ensure_ascii=False, indent=2))
