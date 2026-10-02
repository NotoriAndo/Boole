"""DET-MOD: modular output determinism of a circom instantiation that is too large for DET (recovery R1).

The statement keeps the main component's own compiled constraints and replaces every sub-component instance by an
uninterpreted function of its inputs, common to all instances of one kind (identical compiled sub-circuits)::

    theorem det_mod [Fact (Nat.Prime p)] :
        ∀ f : ℕ → List F → List F, ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →
          CallsRespect f w₁ → CallsRespect f w₂ →
          (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o

``Calls`` lists every instance as (kind, input wires, output wires) and ``CallsRespect f w`` says that each instance's
outputs are ``f kind (its inputs)``.

Soundness: DET-MOD(parent) and DET of every sub-component imply DET(parent).  ``Constraints`` is a subset of the
parent's compiled constraints; for two satisfying assignments of the parent with equal inputs, each instance's wires
satisfy its own compiled sub-circuit, so DET of the sub-circuits makes {(kind, inputs) -> outputs} over both
assignments a function (instances of one kind have identical sub-circuits up to renaming), which extends to ``f``.
DET-MOD is incomplete where the parent relies on facts its sub-components enforce: a DET-MOD counterexample is not
a finding (it does not refute DET), and such packages are GATE-FAIL with that label.

Constraint-to-component mapping (circom ``--O0`` / circom 1 ``-f`` keep every signal; the ``.sym`` names give the
component hierarchy): a wire is *visible* when one of its names is a signal of the main component itself or an input /
output (by the declaring template's ``signal input`` / ``signal output``) of a direct sub-component; a constraint is
kept iff every wire it mentions is visible.  A circom template refers only to its own signals and to the inputs /
outputs of its direct sub-components, so every constraint the main component writes is kept; the other kept ones are
sub-component constraints over the sub-component's own inputs / outputs (true facts of the compiled circuit).  Not
eligible (:class:`NotEligible`): sub-components that cannot be mapped to one declaring template (anonymous components,
buses, component arrays of mixed templates), a parent whose source reads a sub-component signal that is not an input /
output, a constraint that mixes main-only wires with hidden sub-component wires, tagged parent inputs, and parents
above :data:`MAX_PARENT_CONSTRAINTS` / :data:`MAX_PARENT_WIRES`.
"""
from __future__ import annotations

import hashlib
import os
import re
import shutil
import time
from dataclasses import dataclass, field

from . import check as C
from . import circom_source as cs
from . import det_search
from . import gates as G
from . import lean_emit as E
from . import lean_runner as L
from . import package as P
from . import r1cs as R
from . import witness as W

THEOREM = "det_mod"
PROPERTY = {"template": "DET-MOD",
            "name": "modular output determinism (sub-components as uninterpreted functions of their inputs)"}
SPEC_CLAUSES = [
    "Modular output determinism: for the compiled constraint system of the instantiated main component, keep the "
    "constraints whose wires are all signals of the main component or inputs/outputs of its direct sub-components, "
    "and require every sub-component instance's outputs to equal a function of its kind applied to its inputs, the "
    "function being shared by both assignments and by all instances of one kind; any two assignments that satisfy "
    "this and agree on every input wire of the main component agree on every output wire of the main component.",
    "DET-MOD of the main component and DET of every sub-component imply DET of the main component; a DET-MOD "
    "counterexample is not a DET counterexample (the main component may rely on facts its sub-components enforce).",
    "The field is the circom prime of the compilation; inputs and outputs are the main component's `signal input` "
    "and `signal output` declarations, located by the R1CS header and cross-checked against the .sym names.",
]
SPEC = {"tier": "T1",
        "source": "mathematical definition of output determinism, with sub-components abstracted by uninterpreted "
                  "functions (compositional reasoning)",
        "clauses": SPEC_CLAUSES,
        "sha256": hashlib.sha256("\n".join(SPEC_CLAUSES).encode("utf-8")).hexdigest()}
NOT_A_FINDING = ("not-a-finding: DET-MOD counterexample (sub-components are abstracted by uninterpreted functions, so "
                 "the main component may rely on facts its sub-components enforce; DET of the instantiation is not "
                 "refuted)")
MAPPING_RULE = ("kept: constraints whose wires are all main-component signals or inputs/outputs of direct "
                "sub-components (.sym hierarchy of an unoptimized compile, directions from the declaring templates)")
MAX_PARENT_CONSTRAINTS = 300_000
MAX_PARENT_WIRES = 400_000
SAMPLES = 128                 # witness generator attempts (the parent is large; the DET wave uses 600)
BATCH = 16
REAL_WANTED = 12
MUTANTS = 12
DET_SEARCH_POOL = 64

_IDENT = r"[A-Za-z_$][A-Za-z0-9_$]*"


class NotEligible(ValueError):
    """The instantiation's constraints cannot be mapped reliably to its components (reason in the message)."""


@dataclass
class Call:
    instance: str
    template: str
    kind: int
    inputs: list[int]
    outputs: list[int]


@dataclass
class Split:
    kept: list[int]                          # indices of the kept constraints
    calls: list[Call]                        # instances with at least one output
    kinds: list[dict]
    hidden_wires: int
    instances: int
    stats: dict = field(default_factory=dict)


# ------------------------------------------------------------------------------------------ mapping

def _top_dot(s: str) -> int:
    """Index of the first ``.`` outside brackets, or -1."""
    depth = 0
    for i, ch in enumerate(s):
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
        elif ch == "." and depth == 0:
            return i
    return -1


def split_name(name: str) -> tuple[str | None, str]:
    """``main.c[2].out[1]`` -> (``c[2]``, ``out[1]``); ``main.x`` -> (None, ``x``)."""
    if not name.startswith("main."):
        raise NotEligible(f"signal {name!r} outside the main component")
    rest = name[len("main."):]
    k = _top_dot(rest)
    return (None, rest) if k < 0 else (rest[:k], rest[k + 1:])


def _strip_indexes_back(lhs: str) -> str:
    while lhs.endswith("]"):
        depth = 0
        for i in range(len(lhs) - 1, -1, -1):
            if lhs[i] == "]":
                depth += 1
            elif lhs[i] == "[":
                depth -= 1
                if depth == 0:
                    lhs = lhs[:i].rstrip()
                    break
        else:
            raise NotEligible("unbalanced brackets in a component assignment")
    return lhs


def component_templates(files: dict, parent: cs.Template) -> dict[str, str]:
    """Component variable of the parent -> name of the template assigned to it (``c = T(..)``, ``c[i] = T(..)``,
    ``component c = T(..)``)."""
    sf = files[parent.path]
    names = {t.name for f in files.values() for t in f.templates}
    out: dict[str, str] = {}
    for ins in cs.find_instantiations(sf, names):
        if ins.enclosing != parent.name:
            continue
        before = parent.body[:ins.pos].rstrip()
        if before.endswith(("<==", "==>", "==")) or not before.endswith("="):
            raise NotEligible(f"anonymous sub-component {ins.template}(..)")
        lhs = _strip_indexes_back(before[:-1].rstrip())
        m = re.search(r"(" + _IDENT + r")$", lhs)
        if not m:
            raise NotEligible(f"cannot read the component variable of {ins.template}(..)")
        var = m.group(1)
        if out.get(var, ins.template) != ins.template:
            raise NotEligible(f"component array {var} holds instances of different templates")
        out[var] = ins.template
    return out


def split(r: R.R1cs, syms: list[R.SymEntry], files: dict, parent: cs.Template) -> Split:
    """Kept constraints, sub-component calls and kinds of the parent's compiled constraint system."""
    comp = component_templates(files, parent)
    decls: dict[str, dict[str, str]] = {}
    for var, tname in comp.items():
        t = cs.resolve_template(files, parent.path, tname)
        if t is None:
            raise NotEligible(f"template {tname} of component {var} not found")
        if re.search(r"\b(?:input|output)\s+" + _IDENT + r"\s*\(", t.body):
            raise NotEligible(f"template {tname} declares bus-typed signals")
        decls[tname] = {d.name: d.direction for d in cs.signal_declarations(t.body)}
    for var, tname in comp.items():          # reads of sub-component signals in the parent's source
        for m in re.finditer(r"\b" + re.escape(var) + r"\s*(?:\[[^\[\]]*\]\s*)*\.\s*(" + _IDENT + r")", parent.body):
            if decls[tname].get(m.group(1)) not in ("input", "output"):
                raise NotEligible(f"the parent reads {var}.{m.group(1)}, which is not an input/output of {tname}")
    n = r.n_wires
    visible = [False] * n
    visible[0] = True
    main_only = [False] * n
    owners: list[set] = [set() for _ in range(n)]
    order: list[str] = []
    rel: dict[str, dict[int, str]] = {}
    locals_: dict[str, list[str]] = {}
    io: dict[str, dict[str, list[int]]] = {}
    for e in syms:
        if e.wire < 1 or e.wire >= n:
            continue
        inst, local = split_name(e.name)
        if inst is None:
            visible[e.wire] = True
            owners[e.wire].add("")
            continue
        base = inst.split("[", 1)[0]
        tname = comp.get(base)
        if tname is None:
            raise NotEligible(f"sub-component {inst} is not mapped to a template")
        if inst not in rel:
            order.append(inst)
            rel[inst], locals_[inst], io[inst] = {}, [], {"input": [], "output": []}
        owners[e.wire].add(inst)
        rel[inst].setdefault(e.wire, local)
        locals_[inst].append(local)
        if _top_dot(local) >= 0:
            continue                          # a signal of a deeper component
        d = decls[tname].get(R.signal_base(local))
        if d is None:
            raise NotEligible(f"{inst}.{local} is not a signal declared by {tname}")
        if d in ("input", "output"):
            visible[e.wire] = True
            if e.wire not in io[inst][d]:
                io[inst][d].append(e.wire)
    for w in range(1, n):
        main_only[w] = owners[w] == {""}
    kept = []
    for k, cons in enumerate(r.constraints):
        ws = {i for lc in cons for i, _ in lc}
        if all(visible[i] for i in ws):
            kept.append(k)
        elif any(main_only[i] for i in ws):
            raise NotEligible(f"constraint {k} mixes main-component wires with hidden sub-component wires")
    # kinds: same template and identical compiled sub-circuit (renamed by the instance-relative .sym names)
    sub: dict[str, list[int]] = {inst: [] for inst in order}
    for k, cons in enumerate(r.constraints):
        common = None
        for lc in cons:
            for i, _ in lc:
                if i == 0:
                    continue
                common = set(owners[i]) if common is None else common & owners[i]
        for inst in (common or set()) - {""}:
            sub[inst].append(k)
    sig_kind: dict[tuple[str, str], int] = {}
    kinds: list[dict] = []
    calls: list[Call] = []
    for inst in order:
        names = rel[inst]
        h = hashlib.sha256("\n".join(locals_[inst]).encode("utf-8"))
        rows = sorted(repr(tuple(tuple(sorted(("one" if i == 0 else names[i], c) for i, c in lc)) for lc in r.constraints[k]))
                      for k in sub[inst])
        h.update("\n".join(rows).encode("utf-8"))
        key = (comp[inst.split("[", 1)[0]], h.hexdigest())
        if key not in sig_kind:
            sig_kind[key] = len(kinds)
            kinds.append({"kind": len(kinds), "template": key[0], "signature_sha256": key[1], "instances": 0,
                          "subtree_constraints": len(sub[inst]), "inputs": len(io[inst]["input"]),
                          "outputs": len(io[inst]["output"])})
        kd = sig_kind[key]
        kinds[kd]["instances"] += 1
        if io[inst]["output"]:
            calls.append(Call(inst, key[0], kd, io[inst]["input"], io[inst]["output"]))
    hidden = sum(1 for w in range(1, n) if owners[w] and not visible[w])
    return Split(kept, calls, kinds, hidden, len(order),
                 {"parent_constraints": r.n_constraints, "kept_constraints": len(kept),
                  "dropped_constraints": r.n_constraints - len(kept), "parent_wires": n, "hidden_wires": hidden})


@dataclass
class Reduced:
    r: R.R1cs
    wires: list[int]                         # compact index -> parent wire
    calls: list[Call]                        # compact wire indices


def reduce(r: R.R1cs, sp: Split) -> Reduced:
    n_io = r.n_pub_out + r.n_pub_in + r.n_prv_in
    used = {0, *range(1, 1 + n_io)}
    for k in sp.kept:
        used.update(i for lc in r.constraints[k] for i, _ in lc)
    for c in sp.calls:
        used.update(c.inputs)
        used.update(c.outputs)
    wires = sorted(used)
    idx = {w: j for j, w in enumerate(wires)}
    cons = [tuple([(idx[i], v) for i, v in lc] for lc in r.constraints[k]) for k in sp.kept]
    red = R.R1cs(r.prime, r.field_bytes, len(wires), r.n_pub_out, r.n_pub_in, r.n_prv_in, len(wires), cons)
    calls = [Call(c.instance, c.template, c.kind, [idx[i] for i in c.inputs], [idx[i] for i in c.outputs])
             for c in sp.calls]
    return Reduced(red, wires, calls)


# ------------------------------------------------------------------------------------------ Python semantics

def calls_respect_own(calls: list[Call], w: list[int]) -> bool:
    """``CallsRespect (tabOf w) w``: every instance's outputs equal those of the first instance of its kind with
    the same input values (the table function the FID runner builds from the assignment itself)."""
    first: dict = {}
    for c in calls:
        key = (c.kind, tuple(w[i] for i in c.inputs))
        out = tuple(w[i] for i in c.outputs)
        if first.setdefault(key, out) != out:
            return False
    return True


def pair_consistent(calls: list[Call], w1: list[int], w2: list[int]) -> bool:
    """A function ``f`` with ``CallsRespect f w1`` and ``CallsRespect f w2`` exists."""
    seen: dict = {}
    for w in (w1, w2):
        for c in calls:
            key = (c.kind, tuple(w[i] for i in c.inputs))
            if seen.setdefault(key, tuple(w[i] for i in c.outputs)) != tuple(w[i] for i in c.outputs):
                return False
    return True


# ------------------------------------------------------------------------------------------ Lean

SIGNATURE = (" [Fact (Nat.Prime p)] :\n"
             "    ∀ f : ℕ → List F → List F, ∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →\n"
             "      CallsRespect f w₁ → CallsRespect f w₂ →\n"
             "      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o")
_CONSTRAINTS_DOC = "* `Constraints w` holds iff `w 0 = 1` and every R1CS constraint `(A·w) * (B·w) = C·w` holds."


def calls_block(calls: list[Call]) -> list[str]:
    items = ", ".join(f"({c.kind}, {E._list_literal(c.inputs)}, {E._list_literal(c.outputs)})" for c in calls)
    total = sum(1 + len(c.inputs) + len(c.outputs) for c in calls)
    return ((["set_option maxRecDepth 100000 in"] if total > E.LONG_LIST else []) + [
        "/-- Sub-component instances of the main component as (kind, input wires, output wires); instances of one",
        "kind have identical compiled sub-circuits. -/",
        f"def Calls : List (ℕ × List (Fin nWires) × List (Fin nWires)) := [{items}]",
        "",
        "/-- Every sub-component instance's outputs are `f` of its kind, applied to its inputs. -/",
        "def CallsRespect (f : ℕ → List F → List F) (w : Fin nWires → F) : Prop :=",
        "  ∀ c ∈ Calls, c.2.2.map w = f c.1 (c.2.1.map w)",
        ""])


def emit_model(ns: str, meta: dict, red: Reduced, outputs, inputs, names) -> str:
    text = E.emit_model(ns, meta, red.r, outputs, inputs, names)
    assert _CONSTRAINTS_DOC in text
    text = text.replace(_CONSTRAINTS_DOC, (
        "* DET-MOD model: `Constraints w` holds iff `w 0 = 1` and every kept R1CS constraint `(A·w) * (B·w) = C·w`\n"
        "  holds; kept are the constraints of the compiled main component whose wires are all main-component signals\n"
        "  or inputs/outputs of its direct sub-components.  Sub-components are the uninterpreted functions of\n"
        "  `CallsRespect`.  Wires are renumbered (the main component's inputs and outputs keep their numbers)."), 1)
    tail = f"end {ns}\n"
    assert text.endswith(tail)
    return text[:-len(tail)] + "\n".join(calls_block(red.calls)) + "\n" + tail


def emit_statement(ns: str, meta: dict) -> str:
    return "\n".join([
        f"import {E.model_module(ns)}",
        "",
        f"namespace {ns}",
        "",
        f"/-- Modular output determinism (DET-MOD) of {meta['repo_id']} `{meta['instantiation']}` ({meta['path']}):",
        "two assignments that satisfy the main component's own compiled constraints, in which every sub-component",
        "instance's outputs are a function `f` (shared by both assignments) of its kind and inputs, and that agree on",
        "every input wire agree on every output wire.  DET-MOD and DET of every sub-component imply DET; a",
        "counterexample to DET-MOD is not a counterexample to DET. -/",
        f"theorem {THEOREM}{SIGNATURE} := by",
        "  sorry",
        "",
        f"end {ns}",
        ""])


def unfold_order(n_constraints: int) -> list[str]:
    return ["Constraints", "CallsRespect", "Calls"] + E.block_names(n_constraints) + ["Outputs", "Inputs", "F",
                                                                                     "nWires", "p"]


def battery_prefix(variant: str, n_constraints: int) -> list[str]:
    if variant in ("V3", "V4"):
        out = ["intro f w₁ w₂ h₁ h₂ hc₁ hc₂ hin",
               f"(try simp only [{', '.join(['Inputs', 'Outputs'] + E.FLATTEN_LEMMAS)}] at hin ⊢)",
               f"(try unfold {' '.join(['Constraints'] + E.block_names(n_constraints))} at h₁ h₂)",
               "(try unfold CallsRespect Calls at hc₁ hc₂)",
               f"(try simp only [{', '.join(E.FLATTEN_LEMMAS)}] at hc₁ hc₂)"]
        return out + (["repeat' constructor"] if variant == "V4" else [])
    unf = [f"(try unfold {n} at *)" for n in unfold_order(n_constraints)] + ["(try beta_reduce at *)"]
    return {"V0": [], "V1": unf, "V2": ["intros"] + unf}[variant]


def emit_battery_forms(ns: str, n_constraints: int, forms, heartbeats: int) -> str:
    lines = ["import Mathlib", "import Std.Tactic.BVDecide", f"import {E.model_module(ns)}", "", f"namespace {ns}", ""]
    for variant, tactic in forms:
        name = E.battery_theorem_name(variant, tactic)
        if variant in ("V3", "V4"):
            lines.append("set_option maxRecDepth 100000 in")
        lines.append(f"set_option maxHeartbeats {heartbeats} in")
        lines.append(f"theorem {name}{SIGNATURE} := by")
        last = f"all_goals {tactic}" if variant == "V4" else tactic
        lines += [f"  {t}" for t in battery_prefix(variant, n_constraints) + [last]]
        lines.append(f"#print axioms {name}")
        lines.append("")
    lines += [f"end {ns}", ""]
    return "\n".join(lines)


_TAB = [
    "instance instDecCallsRespect (f : ℕ → List F → List F) (w : Fin nWires → F) : Decidable (CallsRespect f w) := by",
    "  unfold CallsRespect; infer_instance",
    "",
    "/-- The table function of one assignment: the outputs of the first instance of the kind with these inputs. -/",
    "def tabOf (w : Fin nWires → F) (k : ℕ) (x : List F) : List F :=",
    "  match Calls.find? (fun c => c.1 == k && c.2.1.map w == x) with",
    "  | some c => c.2.2.map w",
    "  | none => []",
    "",
    "/-- The table function of two assignments (the first one's instances first). -/",
    "def tabOf2 (w₁ w₂ : Fin nWires → F) (k : ℕ) (x : List F) : List F :=",
    "  match Calls.find? (fun c => c.1 == k && c.2.1.map w₁ == x) with",
    "  | some c => c.2.2.map w₁",
    "  | none => tabOf w₂ k x",
    "",
]


def emit_runner(ns: str, n_constraints: int, files: list[tuple[str, str]], pair: tuple[str, str] | None = None) -> str:
    """FID runner: per witness ``FID <tag>`` (``decide (Constraints w)``) and ``CALLS <tag>`` (``decide (CallsRespect
    (tabOf w) w)``); with ``pair`` (two witness paths) also ``PAIR`` = both assignments respect ``tabOf2``, agree on
    the inputs and differ on an output."""
    base = E.emit_fid_runner(ns, n_constraints, files)
    lines = base.split("\n")
    at = lines.index("def loadWitness (path : String) : IO (Fin nWires → F) := do")
    lines[at:at] = _TAB
    out = []
    for ln in lines:
        if ln == '  IO.println "FID-DONE"' and pair:
            out += [f"  let a ← loadWitness {E._lean_string(pair[0])}", f"  let b ← loadWitness {E._lean_string(pair[1])}",
                    '  IO.println s!"PAIR {if decide (CallsRespect (tabOf2 a b) a ∧ CallsRespect (tabOf2 a b) b ∧ '
                    '(∀ i ∈ Inputs, a i = b i) ∧ ∃ o ∈ Outputs, a o ≠ b o) then "ACCEPT" else "REJECT"}"']
        out.append(ln)
        m = re.match(r'^  IO\.println s!"FID (\S+) ', ln)
        if m:
            out.append(f'  IO.println s!"CALLS {m.group(1)} {{if decide (CallsRespect (tabOf w) w) then '
                       f'"ACCEPT" else "REJECT"}}"')
    return "\n".join(out)


def lean_verdicts(env: L.LeanEnv, build: str, ns: str, n_constraints: int, files, work: str,
                  pair=None, timeout: float = 3600) -> tuple[dict, L.RunResult]:
    src = os.path.join(work, "FidMod.lean")
    with open(src, "w", encoding="utf-8") as f:
        f.write(emit_runner(ns, n_constraints, files, pair))
    r = L.run_lean(env, E.LEAN_OPTIONS + ["--json", src], work, timeout, extra_lean_path=[build])
    text = "\n".join(m.get("data", "") for m in L.parse_messages(r.out))
    v = dict(re.findall(r"^FID (\S+) (ACCEPT|REJECT)$", text, re.M))
    v.update({f"CALLS:{k}": x for k, x in re.findall(r"^CALLS (\S+) (ACCEPT|REJECT)$", text, re.M)})
    m = re.search(r"^PAIR (ACCEPT|REJECT)$", text, re.M)
    if m:
        v["PAIR"] = m.group(1)
    if "FID-DONE" not in text:
        v["__incomplete__"] = "; ".join(L.fmt_msg(x) for x in L.errors(L.parse_messages(r.out))[:3]) or r.out[-300:]
    return v, r


# ------------------------------------------------------------------------------------------ package

def _generate(sh, signals, red: Reduced, wc: str, wasm: str, gen_dir: str, inputs: list[dict], tag: str):
    """Witness generator runs in batches; every witness is projected to the model's wires at once."""
    out = []
    for k in range(0, len(inputs), BATCH):
        for g in W.run_generator(sh.cfg.node, wc, wasm, inputs[k:k + BATCH], gen_dir, tag=f"{tag}{k}"):
            w = [g.witness[i] for i in red.wires] if g.witness is not None else None
            out.append(W.GeneratedWitness(g.inputs, w, g.error))
    return out


def build(sh, t: cs.Template, parent: dict, dir_name: str, work: str) -> dict:
    """The DET-MOD package of the TOO-LARGE record ``parent`` (its recorded instantiation and compiler)."""
    from . import circom_det as D
    cfg = sh.cfg
    t0 = time.time()
    pinst = parent["instantiation"]
    args = tuple(pinst["args"])
    comp = sh.compilers.get("v" + parent["env"]["circom"]) or sh.default_compiler()
    rec = D.base_record(sh, t, dir_name)
    rec["property"] = dict(PROPERTY)
    rec["spec"] = dict(SPEC)
    rec["env"] = sh.env_record(comp)
    rec["instantiation"] = {k: v for k, v in pinst.items() if k != "selection"}
    rec["instantiation"]["selection"] = ("the instantiation of the DET record, which exceeds the DET size policy "
                                        "(DET-MOD models only the main component's own constraints)")
    rec["det_mod"] = {"parent": {"package_id": parent["package_id"], "status": parent["status"],
                                 "n_constraints": parent["circuit"]["n_constraints"],
                                 "generator_sources_sha256": parent["generator"]["sources_sha256"]},
                      "mapping": MAPPING_RULE, "label": NOT_A_FINDING.split(":", 1)[0]}

    def stop(reason: str) -> dict:
        rec.update(status="UNINSTANTIABLE", status_reason=f"DET-MOD not built: {reason}"[:600])
        rec.setdefault("evidence", {})["wall_secs"] = round(time.time() - t0, 1)
        return rec

    if pinst.get("tag_wrapper"):
        return stop("tagged parent inputs are not supported by DET-MOD")
    if parent["circuit"]["n_constraints"] > MAX_PARENT_CONSTRAINTS or parent["circuit"]["n_wires"] > MAX_PARENT_WIRES:
        return stop(f"parent above {MAX_PARENT_CONSTRAINTS} constraints or {MAX_PARENT_WIRES} wires")
    full = D.compile_main(sh, os.path.join(work, "compile"), pinst["include_context"], t.name, args, True,
                          rule_path=t.path, compiler=comp)
    if full["rc"] != 0 or full.get("r1cs_sha256") != parent["circuit"]["r1cs_sha256"]:
        return stop("full compile failed or produced a different R1CS than the DET record: "
                    + (full.get("error") or "digest differs")[:300])
    r = R.read_r1cs(full["r1cs"])
    syms = R.read_sym(full["sym"])
    try:
        sp = split(r, syms, sh.files, t)
    except NotEligible as exc:
        return stop(f"constraint-to-component mapping not reliable: {exc}")
    red = reduce(r, sp)
    rec["det_mod"].update(sp.stats, instances=sp.instances, calls=len(red.calls), kinds=sp.kinds[:200],
                          n_kinds=len(sp.kinds), model_wires=red.r.n_wires)
    io = R.main_io_wires(r, syms)
    full_names = R.wire_names(syms, r.n_wires)
    names = [full_names[w] for w in red.wires]
    rec["circuit"] = D.circuit_record(sh, red.r, dict(full, constraints=red.r.n_constraints, wires=red.r.n_wires),
                                      len(io.inputs), len(io.outputs), P.sha256_file(full["sym"]))
    rec["circuit"]["r1cs_sha256"] = full["r1cs_sha256"]
    if red.r.n_constraints > cfg.max_constraints:
        rec.update(status="TOO-LARGE", status_reason=f"DET-MOD model has {red.r.n_constraints} kept constraints > "
                                                     f"{cfg.max_constraints}")
        rec["evidence"] = {"wall_secs": round(time.time() - t0, 1)}
        return rec
    if not red.calls:
        return stop("the instantiation has no sub-component with outputs (DET-MOD equals DET)")
    ns = P.lean_namespace(cfg.collection, dir_name)
    meta = {"repo_id": cfg.repo_id, "instantiation": pinst["call"],
            "generator": f"{sh.generator['name']} v{sh.generator['version']}", "repo_url": cfg.repo_url,
            "commit": cfg.commit, "path": t.path, "template": t.name, "rule": pinst["rule"],
            "circom_version": comp.version, "circom_flags": full["flags"], "r1cs_sha256": full["r1cs_sha256"],
            "prime_name": r.prime_name or "unknown"}
    stage = os.path.join(work, "pkg")
    shutil.rmtree(stage, ignore_errors=True)
    model_rel = E.model_relpath(ns)
    os.makedirs(os.path.dirname(os.path.join(stage, model_rel)), exist_ok=True)
    outs, ins = list(range(1, 1 + r.n_pub_out)), list(range(1 + r.n_pub_out, 1 + r.n_pub_out + r.n_pub_in + r.n_prv_in))
    with open(os.path.join(stage, model_rel), "w", encoding="utf-8") as f:
        f.write(emit_model(ns, meta, red, outs, ins, names))
    statement_text = emit_statement(ns, meta)
    with open(os.path.join(stage, "Statement.lean"), "w", encoding="utf-8") as f:
        f.write(statement_text)
    fqn = f"{ns}.{THEOREM}"
    rec["statement"] = {"file": "Statement.lean", "theorem": THEOREM, "theorem_fqn": fqn,
                        "model_module": E.model_module(ns), "model_file": model_rel, "text": statement_text,
                        "assumptions": P.statement_assumptions(r.prime_name or "unknown") + [
                            "DET-MOD: sub-component instances are uninterpreted functions of their inputs (one per "
                            "kind, shared by both assignments); with DET of every sub-component it implies DET, and a "
                            "counterexample to it is not a finding."],
                        "truth": "unknown"}
    gates: dict = {}
    evidence: dict = {"work": work}
    signals = W.input_signals(io, {})
    wc, wasm = full["witness_js"], full["wasm"]
    gen_dir = os.path.join(work, "wit-gen")
    if not signals:
        generated = _generate(sh, signals, red, wc, wasm, gen_dir, [{}], "free")
    else:
        half = SAMPLES // 2
        first = _generate(sh, signals, red, wc, wasm, gen_dir,
                          W.sample_inputs(signals, r.prime, rec["package_id"], half), "uniform")
        allowed = W.successful_strategies(first)
        rest = _generate(sh, signals, red, wc, wasm, gen_dir,
                         W.sample_inputs(signals, r.prime, rec["package_id"] + "/mixed", SAMPLES - half,
                                         allowed or None, uniform=False), "mixed")
        generated = first + rest
    accepted, rejected, gen_errors = W.collect_real(red.r, signals, generated, DET_SEARCH_POOL)
    grid = W.boundary_grid(signals, r.prime)
    grid_ok = W.collect_real(red.r, signals, _generate(sh, signals, red, wc, wasm, gen_dir, grid, "grid"),
                             len(grid))[0] if grid else []
    evidence["witness_sampling"] = {"attempts": len(generated), "generator_errors": gen_errors,
                                    "oracle_rejected_generator_witnesses": len(rejected),
                                    "real_distinct": len(accepted), "boundary_grid": len(grid),
                                    "boundary_grid_accepted": len(grid_ok)}
    real_w = [w for _, w in accepted][:REAL_WANTED]
    pool = [w for _, w in accepted] + [w for _, w in grid_ok]
    calls_ok_py = [calls_respect_own(red.calls, w) for w in pool]
    muts = W.mutants(red.r, real_w, MUTANTS, rec["package_id"]) if real_w else []
    build_dir = os.path.join(work, "build")
    shutil.rmtree(build_dir, ignore_errors=True)
    elab = G.g_elab(sh.env, stage, build_dir, ns, os.path.join(work, "elab"), theorem=THEOREM)
    gates["G-ELAB"] = elab.to_json()
    model_ok = elab.detail.get("model_compile_rc") == 0 and os.path.exists(
        os.path.join(build_dir, model_rel[:-len(".lean")] + ".olean"))
    gates["G-FID"], gates["G-NONVAC"] = _fid(sh, build_dir, ns, red, real_w, muts, not signals, calls_ok_py,
                                             os.path.join(work, "fid"), model_ok)
    differ = (lambda a, b: [o for o in outs if a[o] != b[o]] if pair_consistent(red.calls, a, b) else [])
    ce, log = (det_search.search(red.r, pool, ins, outs, rec["package_id"], cfg.det_search_budget_s, differ=differ)
               if pool else (None, {"bases": 0}))
    det_gate = {"status": "PASS", "truth": "unknown", "log": log,
                "note": "PASS means no DET-MOD counterexample was found by the cheap searches (assignment pairs "
                        "respecting a common function for every sub-component kind); truth is not established"}
    if ce is not None:
        cdir = os.path.join(work, "counterexample")
        os.makedirs(cdir, exist_ok=True)
        W.write_witness(os.path.join(cdir, "w1.txt"), ce.base)
        W.write_witness(os.path.join(cdir, "w2.txt"), ce.other)
        v, _ = (lean_verdicts(sh.env, build_dir, ns, red.r.n_constraints,
                              [("w1", os.path.join(cdir, "w1.txt")), ("w2", os.path.join(cdir, "w2.txt"))], cdir,
                              pair=(os.path.join(cdir, "w1.txt"), os.path.join(cdir, "w2.txt")))
                if model_ok else ({}, None))
        lean_ok = v.get("w1") == "ACCEPT" and v.get("w2") == "ACCEPT" and v.get("PAIR") == "ACCEPT"
        det_gate = {"status": "FAIL", "truth": "false-counterexample-found" if lean_ok else "unknown",
                    "method": ce.method, "changed_outputs": [names[o] for o in ce.changed_outputs][:20],
                    "lean_confirms_both_witnesses": lean_ok, "oracle_confirms": True, "log": log,
                    "finding": NOT_A_FINDING,
                    "files": ["evidence/counterexample/w1.txt", "evidence/counterexample/w2.txt"]}
        evidence["counterexample_dir"] = cdir
    gates["DET-SEARCH"] = det_gate
    if elab.status != "PASS":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement did not elaborate"}
    elif det_gate["truth"] == "false-counterexample-found":
        gates["G-TRIV"] = {"status": "SKIPPED", "reason": "statement refuted by a confirmed counterexample"}
    else:
        n = red.r.n_constraints
        gates["G-TRIV"] = G.g_triv(sh.env, build_dir, ns, n, os.path.join(work, "triv"), cfg.battery_heartbeats,
                                   cfg.battery_file_wall, cfg.battery_single_wall,
                                   emit=lambda fs: emit_battery_forms(ns, n, fs, cfg.battery_heartbeats)).to_json()
    rec["gates"] = gates
    D.set_status(rec, gates, bool(outs))
    if rec["status"] == "OPEN":
        rec["status_reason"] = "all gates pass; DET-MOD truth unknown"
    if rec["status"] == "DET-FALSE-CANDIDATE":              # DET-MOD refuted: not a finding, not issuable
        rec["status"] = "GATE-FAIL"
        rec["status_reason"] = (f"DET-SEARCH: {det_gate['method']} found two assignments that satisfy the DET-MOD "
                                f"model with a common sub-component function, equal inputs and different outputs "
                                f"(confirmed in Python and Lean); {NOT_A_FINDING}")
    ref = elab.detail["reference_type_sha256"] if elab.status == "PASS" else "0" * 64
    rec["checker"] = {"statement_file": "Statement.lean", "theorem": THEOREM, "theorem_fqn": fqn,
                      "lean_opts": list(E.LEAN_OPTIONS),
                      "files": [{"path": "Statement.lean", "role": "statement",
                                 "sha256": P.sha256_file(os.path.join(stage, "Statement.lean"))},
                                {"path": model_rel, "role": "import", "module": E.model_module(ns),
                                 "sha256": P.sha256_file(os.path.join(stage, model_rel))}],
                      "reference_type_sha256": ref, "replay_tool_sha256": P.sha256_file(G.REPLAY_TOOL),
                      "allowed_axioms": sorted(C.ALLOWED_AXIOMS), "forbidden_tokens": C.FORBIDDEN_LABELS}
    rec["evidence"] = evidence
    D.write_package(sh, rec, stage, work, dir_name)
    rec["evidence"]["wall_secs"] = round(time.time() - t0, 1)
    return rec


def _fid(sh, build: str, ns: str, red: Reduced, real: list, muts: list, input_free: bool, calls_ok_py: list,
         work: str, model_ok: bool):
    if not model_ok:
        skip = {"reason": "model did not compile"}
        return G.Gate("G-FID", "SKIPPED", skip).to_json(), G.Gate("G-NONVAC", "SKIPPED", dict(skip)).to_json()
    wdir = os.path.join(work, "wit")
    os.makedirs(wdir, exist_ok=True)
    files = []
    for k, w in enumerate(real):
        files.append((f"real_{k:03d}", os.path.join(wdir, f"real_{k:03d}.txt")))
        W.write_witness(files[-1][1], w)
    for k, m in enumerate(muts):
        files.append((f"mut_{k:03d}", os.path.join(wdir, f"mut_{k:03d}.txt")))
        W.write_witness(files[-1][1], m["witness"])
    if not files:
        d = {"real_witnesses": 0, "reason": "the witness generator produced no oracle-accepted witness"}
        return G.Gate("G-FID", "FAIL", d).to_json(), G.Gate("G-NONVAC", "FAIL", dict(d)).to_json()
    v, r = lean_verdicts(sh.env, build, ns, red.r.n_constraints, files, work)
    real_acc = sum(v.get(f"real_{k:03d}") == "ACCEPT" and v.get(f"CALLS:real_{k:03d}") == "ACCEPT"
                   for k in range(len(real)))
    agree = sum(v.get(f"mut_{k:03d}") == ("ACCEPT" if m["oracle"] else "REJECT")
                and v.get(f"CALLS:mut_{k:03d}") == ("ACCEPT" if calls_respect_own(red.calls, m["witness"]) else "REJECT")
                for k, m in enumerate(muts))
    detail = {"method": "Lean #eval of decide (Constraints w) and decide (CallsRespect (tabOf w) w) on every witness, "
                        "compared with the Python R1CS oracle and the Python call-table check",
              "real_witnesses": len(real), "real_lean_accept": real_acc, "mutants": len(muts),
              "mutants_oracle_reject": sum(not m["oracle"] for m in muts), "mutants_lean_agree": agree,
              "pool_calls_functional": sum(calls_ok_py), "pool": len(calls_ok_py),
              "lean_eval_secs": r.secs, "lean_eval_peak_rss_mb": r.peak_rss_mb}
    if "__incomplete__" in v:
        detail.update(error=v["__incomplete__"][:400],
                      reason="the Lean evaluation did not complete (harness error); no verdict is inferred")
        return G.Gate("G-FID", "ERROR", detail).to_json(), G.Gate("G-NONVAC", "ERROR", dict(detail)).to_json()
    need = 1 if input_free else G.FID_MIN_REAL
    ok = (real_acc == len(real) and len(real) >= need and len(muts) >= G.FID_MIN_MUTANTS and agree == len(muts)
          and all(calls_ok_py))
    fd = dict(detail, required_real=need, input_free=input_free)
    if not ok:
        why = []
        if len(real) < need:
            why.append(f"only {len(real)} distinct real witnesses (need {need})")
        if real_acc != len(real):
            why.append("the Lean model rejected a real witness")
        if agree != len(muts) or len(muts) < G.FID_MIN_MUTANTS:
            why.append("Lean and Python verdicts differ on a mutant (or too few mutants)")
        if not all(calls_ok_py):
            why.append("a real witness is not functional per sub-component kind (input/output classification)")
        fd["reason"] = "; ".join(why)
    return (G.Gate("G-FID", "PASS" if ok else "FAIL", fd).to_json(),
            G.Gate("G-NONVAC", "PASS" if real_acc >= 1 else "FAIL",
                   {"real_witnesses_accepted": real_acc, "method": "witness generator + oracle + Lean #eval"}).to_json())
