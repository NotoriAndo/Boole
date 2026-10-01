"""gnark harness results -> the internal R1CS representation of the Circom generator.

The Go harness (``gnark_tool``) compiles a wrapper circuit with gnark's R1CS builder and writes one
JSON result: the *symbolic* compile (range checks by gnark's non-commitment checker, a commitment as
a challenge wire), and, when the circuit takes a commitment, *constant* compiles at N fixed distinct
challenges, plus solver witnesses of every compile.  This module turns a result into an
:class:`r1cs.R1cs` model, its input/output wires, emulated output groups and witnesses:

* no commitment: the model is the symbolic compile itself;
* a commitment: the model is the common pre-commitment part (constraints and wires before the
  commitment, identical in every compile; checked) and, for every challenge, the challenge-dependent
  part of that constant compile with its wires renumbered after the previous ones.  The challenge-
  dependent part of the symbolic compile must be a polynomial identity in the challenge of degree D
  (re-derived here, independently of the harness), and N = D + 1 points make the union equivalent to
  the identity holding for every challenge.  Every challenge-dependent wire of a constant compile must
  be defined by its constraint (so the union adds no free wire).

Witnesses of the model concatenate gnark's solver witnesses of the compiles (the pre-commitment part
must agree across compiles).
"""
from __future__ import annotations

from dataclasses import dataclass, field

from . import r1cs as R


class ModelError(ValueError):
    """The harness result cannot be turned into a model (recorded as the package's reason)."""


Cons = tuple[list[tuple[int, int]], list[tuple[int, int]], list[tuple[int, int]]]


def parse_constraints(raw: list, p: int) -> list[Cons]:
    out = []
    for k, c in enumerate(raw):
        if len(c) != 3:
            raise ModelError(f"constraint {k}: expected L, R, O")
        lcs = []
        for lc in c:
            terms = []
            for w, coeff in lc:
                v = int(coeff)
                if not 0 <= v < p:
                    raise ModelError(f"constraint {k}: coefficient not reduced")
                terms.append((int(w), v))
            lcs.append(terms)
        out.append((lcs[0], lcs[1], lcs[2]))
    return out


def degree(constraints: list[Cons], boundary_constraints: int, boundary_wires: int,
           challenge_wire: int | None) -> int:
    """Largest degree in the challenge of a check of the challenge-dependent part (see the module
    docstring); raises :class:`ModelError` when that part is not a polynomial identity whose wires are
    defined by their constraints.  With ``challenge_wire`` None every challenge-dependent wire must
    still be defined (degree 0)."""
    B = boundary_wires
    deg: dict[int, int] = {}
    if challenge_wire is not None:
        deg[challenge_wire] = 1

    def lc_deg(lc, skip=None):
        d = 0
        for w, _ in lc:
            if w == skip or w < B:
                continue
            d = max(d, deg[w])
        return d

    best = 0
    for k in range(boundary_constraints, len(constraints)):
        L, Rr, O = constraints[k]
        undef = {w for lc in (L, Rr, O) for w, _ in lc if w >= B and w not in deg}
        if not undef:
            d = 0 if not L or not Rr else lc_deg(L) + lc_deg(Rr)
            best = max(best, d, lc_deg(O))
        elif len(undef) == 1:
            u = next(iter(undef))
            if any(w == u for w, _ in L + Rr):
                raise ModelError(f"constraint {k} uses the solver-computed wire {u} as a factor (the check is not "
                                 f"a polynomial identity in the challenge)")
            deg[u] = max(lc_deg(L) + lc_deg(Rr) if L and Rr else 0, lc_deg(O, skip=u))
        else:
            raise ModelError(f"constraint {k} introduces {len(undef)} challenge-dependent wires at once")
    return best


@dataclass
class Model:
    r: R.R1cs
    inputs: list[int]
    outputs: list[int]                       # every exposed output wire, in exposure order
    groups: list[dict]                       # emulated groups: {"start", "len", "bits", "modulus", "field", "path"}
    output_paths: list[str]
    wire_names: list[str | None]
    witnesses: list[list[int]]
    commitment: dict = field(default_factory=dict)
    hints: dict = field(default_factory=dict)     # hint name -> number of output wires
    solver_errors: dict = field(default_factory=dict)
    test_engine: dict = field(default_factory=dict)
    sample_profiles: dict = field(default_factory=dict)
    domains: list[str] = field(default_factory=list)

    @property
    def native_outputs(self) -> list[int]:
        grouped = {self.outputs[g["start"] + i] for g in self.groups for i in range(g["len"])}
        return [o for o in self.outputs if o not in grouped]

    def group_wires(self) -> list[tuple[list[int], int, int]]:
        return [([self.outputs[g["start"] + i] for i in range(g["len"])], int(g["bits"]), int(g["modulus"]))
                for g in self.groups]


def em_value(w: list[int], limbs: list[int], bits: int) -> int:
    v = 0
    for i in reversed(limbs):
        v = (v << bits) + w[i]
    return v


def differ(model: Model):
    """``differ(w1, w2)``: output wires on which two assignments disagree under the statement's output
    relation (native outputs: equality; emulated outputs: equal value modulo the emulated modulus)."""
    native = model.native_outputs
    groups = model.group_wires()

    def fn(w1: list[int], w2: list[int]) -> list[int]:
        d = [o for o in native if w1[o] != w2[o]]
        for limbs, bits, mod in groups:
            if em_value(w1, limbs, bits) % mod != em_value(w2, limbs, bits) % mod:
                d.extend(limbs)
        return d
    return fn


def _names(comp: dict) -> list[str | None]:
    n = comp["n_wires"]
    names: list[str | None] = [None] * n
    names[0] = "one"
    pub = comp.get("public_names") or []
    sec = comp.get("secret_names") or []
    for i, nm in enumerate(pub[1:], start=1):
        names[i] = f"public {nm}"
    for i, nm in enumerate(sec):
        names[comp["n_public"] + i] = f"secret {nm}"
    for h in comp.get("hints") or []:
        short = h["name"].rsplit("/", 1)[-1]
        for k, wv in enumerate(range(h["start"], h["end"])):
            if 0 <= wv < n and names[wv] is None:
                names[wv] = f"hint {short} out {k}"
    for k, (o, path) in enumerate(zip(comp["outputs"], comp["output_paths"])):
        names[o] = f"output {path}"
    return names


def build(res: dict) -> Model:
    """The model of a harness result with status ``ok``."""
    if res.get("status") != "ok":
        raise ModelError(f"harness status {res.get('status')}: {res.get('error', '')}")
    p = int(res["field"])
    sym = res["symbolic"]
    n_pub, n_sec = sym["n_public"], sym["n_secret"]
    inputs = list(range(1, n_pub + n_sec))
    outputs = list(sym["outputs"])
    if len(set(outputs)) != len(outputs) or any(o < n_pub + n_sec for o in outputs):
        raise ModelError("exposed outputs are not distinct internal wires")
    sym_cons = parse_constraints(sym.get("constraints") or [], p)
    hints: dict[str, int] = {}
    commitment: dict = {"commits": sym["commits"]}
    consts = res.get("constant") or []
    if sym["commits"] == 0:
        cons = sym_cons
        n_wires = sym["n_wires"]
        names = _names(sym)
        comps = [sym]
    else:
        Bc, Bw = sym["boundary_constraints"], sym["boundary_wires"]
        if sym["challenge_wire"] != Bw:
            raise ModelError("the challenge wire is not the first wire after the commitment")
        d = degree(sym_cons, Bc, Bw, sym["challenge_wire"])
        if d != res["degree"] or res["points"] != d + 1 or len(consts) != d + 1:
            raise ModelError(f"challenge degree {d} (harness {res['degree']}), points {res['points']}, "
                             f"constant compiles {len(consts)}")
        challenges = [int(c["challenge"]) for c in consts]
        if len(set(challenges)) != len(challenges):
            raise ModelError("challenges are not distinct")
        if any(o >= Bw for o in outputs):
            raise ModelError("an exposed output depends on the challenge")
        pre = sym_cons[:Bc]
        cons = list(pre)
        n_wires = Bw
        names = _names(sym)[:Bw]
        offsets = []
        for j, c in enumerate(consts):
            if (c["boundary_constraints"], c["boundary_wires"]) != (Bc, Bw) or c["outputs"] != outputs:
                raise ModelError(f"constant compile {j}: boundary or outputs differ from the symbolic compile")
            cc = parse_constraints(c.get("constraints") or [], p)
            if cc[:Bc] != pre:
                raise ModelError(f"constant compile {j}: the pre-commitment constraints differ")
            degree(cc, Bc, Bw, None)          # every challenge-dependent wire is defined by its constraint
            off = n_wires - Bw
            offsets.append(off)

            def ren(lc, off=off):
                return [(w if w < Bw else w + off, v) for w, v in lc]
            for L, Rr, O in cc[Bc:]:
                cons.append((ren(L), ren(Rr), ren(O)))
            names += [f"challenge {j} wire {w - Bw}" for w in range(Bw, c["n_wires"])]
            n_wires += c["n_wires"] - Bw
        commitment.update(degree=d, points=d + 1, challenges=challenges, boundary_constraints=Bc,
                          boundary_wires=Bw, committed=sym["committed"],
                          symbolic_post_constraints=len(sym_cons) - Bc,
                          model_post_constraints=len(cons) - Bc)
        comps = consts
    for h in sym.get("hints") or []:
        if h["start"] < (sym["boundary_wires"] if sym["commits"] else sym["n_wires"]):
            hints[h["name"]] = hints.get(h["name"], 0) + h["end"] - h["start"]
    nbytes = (p.bit_length() + 63) // 64 * 8
    r = R.R1cs(prime=p, field_bytes=nbytes, n_wires=n_wires, n_pub_out=0, n_pub_in=n_pub - 1, n_prv_in=n_sec,
               n_labels=n_wires, constraints=cons)
    witnesses, seen = [], set()
    errors: dict[str, int] = {}
    engine: dict[str, int] = {}
    profiles: dict[str, int] = {}
    domains: set[str] = set()
    for s in res.get("samples") or []:
        engine[s.get("test_engine", "")[:9]] = engine.get(s.get("test_engine", "")[:9], 0) + 1
        domains |= set(s.get("domains") or [])
        if not s.get("solved"):
            key = (s.get("error") or "")[:100]
            errors[key] = errors.get(key, 0) + 1
            continue
        ws = [[int(v) for v in vec] for vec in s["witnesses"]]
        if len(ws) != len(comps):
            raise ModelError("a sample does not carry one witness per compile")
        if sym["commits"] == 0:
            w = ws[0]
        else:
            Bw = sym["boundary_wires"]
            base = ws[0][:Bw]
            if any(x[:Bw] != base for x in ws[1:]):
                raise ModelError("solver witnesses of the constant compiles differ before the commitment")
            w = list(base)
            for x in ws:
                w.extend(x[Bw:])
        if len(w) != n_wires:
            raise ModelError("merged witness length differs from the model")
        key = tuple(w)
        if key in seen:
            continue
        seen.add(key)
        witnesses.append(w)
        profiles[s["profile"]] = profiles.get(s["profile"], 0) + 1
    return Model(r=r, inputs=inputs, outputs=outputs, groups=list(sym["groups"]), output_paths=list(sym["output_paths"]),
                 wire_names=names, witnesses=witnesses, commitment=commitment, hints=dict(sorted(hints.items())),
                 solver_errors=errors, test_engine=engine, sample_profiles=profiles, domains=sorted(domains))
