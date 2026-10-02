"""Lean emitters for circom DET packages.

The model module is a verbatim transcription of the compiled R1CS:

* ``p``, ``F := ZMod p``, ``nWires``;
* ``Outputs`` / ``Inputs``: the main component's output and input wires (literal lists);
* ``Constraints w``: ``w 0 = 1`` and, for every constraint, ``(A·w) * (B·w) = C·w``.  Linear
  combinations are printed with balanced coefficients (a coefficient ``c > p/2`` is printed as
  ``-(p - c)``), and a product with an empty factor is printed as ``0``.  Both are value-preserving
  in ``ZMod p``.  Systems with more than :data:`BLOCK` constraints are split into ``Block<k>``
  definitions of at most :data:`BLOCK` conjuncts, so no term nests deeper than that.

The statement module states output determinism (T1: the definition of output determinism) with
the primality of ``p`` as an instance hypothesis, so that field lemmas are usable; ``p`` is prime,
so the hypothesis does not weaken the statement.

Templates whose inputs carry known circom tags (``binary``, ``maxbit``) are compiled through an
untagged wrapper, and their model has ``Preconditions w``: the tag precondition on every tagged main
input wire (``BinaryInputs``: each value 0 or 1; ``MaxbitInputs<n>``: each value below 2^n).  Their
statement is DET under input preconditions: both assignments satisfy ``Preconditions``.  Statements
of untagged templates are unchanged.
"""
from __future__ import annotations

import re
from typing import Sequence

from .r1cs import LinComb, R1cs, signal_base

BLOCK = 64
STATEMENT_THEOREM = "det"
STATEMENT_BINDERS = "[Fact (Nat.Prime p)]"
STATEMENT_PROP = ("∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ →\n"
                  "      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o")
STATEMENT_PROP_PRE = ("∀ w₁ w₂ : Fin nWires → F, Constraints w₁ → Constraints w₂ → Preconditions w₁ → Preconditions w₂ →\n"
                      "      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o")
LEAN_OPTIONS = ["-DautoImplicit=false", "--tstack=400000"]

# ZK-PILOT-TRIV-P1 battery: 11 tactics x variants V0 (as stated), V1 (unfold), V2 (intros, unfold).
BATTERY_TACTICS = ["decide", "simp", "simp_all", "omega", "norm_num", "aesop", "bv_decide", "grind", "ring_nf",
                   "exact?", "apply?"]
BATTERY_VARIANTS = ["V0", "V1", "V2"]
# Battery P2 (wave 1b), aimed at linear and copy circuits, whose DET proofs need the literal Inputs /
# Outputs lists case-split: V3 introduces the hypotheses, flattens `∀ i ∈ [..]` into conjunctions and
# unfolds the constraint system in the hypotheses, then runs the tactic; V4 additionally splits the goal
# conjunction and runs the tactic on every goal.  One file, 7 forms, same heartbeat cap as P1.
BATTERY_P2 = [("V3", ["simp", "simp_all", "decide", "omega", "grind"]), ("V4", ["simp_all", "grind"])]
FLATTEN_LEMMAS = ["List.forall_mem_cons", "List.forall_mem_nil", "List.not_mem_nil", "IsEmpty.forall_iff",
                  "and_true", "implies_true", "forall_const"]


def lean_ident(text: str) -> str:
    """A Lean identifier component from arbitrary text: [A-Za-z0-9_], starting with a letter."""
    s = re.sub(r"[^A-Za-z0-9]+", "_", text).strip("_")
    if not s or not s[0].isalpha():
        s = "x" + s
    return s


def _signed(c: int, p: int) -> int:
    return c if c <= p // 2 else c - p


def render_lc(lc: LinComb, p: int) -> str:
    """``Σ c·w i`` with balanced coefficients, in R1CS term order; ``0`` when empty."""
    parts = []
    for k, (wire, coeff) in enumerate(lc):
        s = _signed(coeff, p)
        mag, neg = abs(s), s < 0
        atom = f"w {wire}" if mag == 1 else f"{mag} * w {wire}"
        if k > 0:
            parts.append((" - " if neg else " + ") + atom)
        elif not neg:
            parts.append(atom)
        else:
            parts.append(f"-{atom}" if mag == 1 else f"-({atom})")
    return "".join(parts) if parts else "0"


def _factor(lc: LinComb, p: int) -> str:
    text = render_lc(lc, p)
    return text if re.fullmatch(r"w \d+", text) else f"({text})"


def render_constraint(a: LinComb, b: LinComb, c: LinComb, p: int) -> str:
    lhs = "0" if not a or not b else f"{_factor(a, p)} * {_factor(b, p)}"
    return f"{lhs} = {render_lc(c, p)}"


def summarize_names(names: Sequence[str]) -> str:
    """``main.out[0]``, ``main.out[1]``, ... -> ``main.out[0..1]`` (consecutive runs of one base)."""
    out: list[str] = []
    i = 0
    while i < len(names):
        base = signal_base(names[i])
        m = re.fullmatch(re.escape(base) + r"\[(\d+)\]", names[i])
        j = i
        if m:
            start = int(m.group(1))
            while j + 1 < len(names) and names[j + 1] == f"{base}[{start + j + 1 - i}]":
                j += 1
        out.append(names[i] if j == i else f"{base}[{start}..{start + j - i}]")
        i = j + 1
    return ", ".join(f"`{x}`" for x in out) if out else "none"


def _list_literal(items: Sequence[int]) -> str:
    return "[" + ", ".join(str(i) for i in items) + "]"


LONG_LIST = 512
# models with more blocks than this (above 8,192 constraints; recovery R1 size ladder) raise ``maxRecDepth`` for the
# ``Constraints`` conjunction; smaller models are emitted exactly as before
LONG_CONJUNCTION = 128


def _long_list_option(items: Sequence[int]) -> list[str]:
    """Lean's code generator recurses once per list element; lists above :data:`LONG_LIST` elements
    get a raised ``maxRecDepth`` (shorter lists are emitted exactly as before)."""
    return ["set_option maxRecDepth 100000 in"] if len(items) > LONG_LIST else []


def model_module(ns: str) -> str:
    return f"{ns}.Model"


def model_relpath(ns: str) -> str:
    return model_module(ns).replace(".", "/") + ".lean"


def block_names(n_constraints: int) -> list[str]:
    if n_constraints <= BLOCK:
        return []
    return [f"Block{k}" for k in range((n_constraints + BLOCK - 1) // BLOCK)]


def precondition_lists(groups) -> list[tuple[str, str, list[int]]]:
    """(list name, per-wire condition, wires) for precondition groups [{"tag", "value", "wires"}]; wires with the
    same tag and value share one list."""
    merged: dict[str, tuple[str, list[int]]] = {}
    for g in groups:
        if g["tag"] == "binary":
            name, cond = "BinaryInputs", "w i = 0 ∨ w i = 1"
        elif g["tag"] == "maxbit":
            name, cond = f"MaxbitInputs{g['value']}", f"(w i).val < 2 ^ {g['value']}"
        else:
            raise ValueError(f"no precondition for tag {g['tag']!r}")
        merged.setdefault(name, (cond, []))[1].extend(g["wires"])
    return [(name, cond, wires) for name, (cond, wires) in merged.items()]


def unfold_order(n_constraints: int, preconditions: bool = False, pre_lists=()) -> list[str]:
    """Every definition of the model module, consumers before the definitions they use."""
    pre = ["Preconditions", *pre_lists] if preconditions else []
    return ["Constraints", *pre] + block_names(n_constraints) + ["Outputs", "Inputs", "F", "nWires", "p"]


def emit_model(ns: str, meta: dict, r: R1cs, outputs: Sequence[int], inputs: Sequence[int],
               wire_names: Sequence[str | None], preconditions=None) -> str:
    p = r.prime
    header = [
        "import Mathlib",
        "",
        "/-!",
        f"# DET model: {meta['repo_id']} `{meta['instantiation']}`",
        "",
        f"Generated by `scripts/zk_registry` ({meta['generator']}); do not edit.",
        "",
        f"* Source: {meta['repo_url']} at `{meta['commit']}`, `{meta['path']}`, template `{meta['template']}`,",
        f"  instantiation `{meta['instantiation']}` (rule `{meta['rule']}`).",
        f"* Compiled with circom {meta['circom_version']} (`{' '.join(meta['circom_flags'])}`).",
        f"* R1CS sha256 `{meta['r1cs_sha256']}`.",
        f"* Field: circom prime `{meta['prime_name']}`; `F = ZMod p`.",
        "* `w : Fin nWires → F` gives a value to every wire; wire 0 is the constant one.",
        "* `Constraints w` holds iff `w 0 = 1` and every R1CS constraint `(A·w) * (B·w) = C·w` holds.",
        "  Coefficients are printed balanced (`c > p/2` as `-(p - c)`) and a product with an empty",
        "  factor as `0`; both preserve values in `ZMod p`.",
        "",
        "## Wires",
        "",
    ]
    for wire in range(r.n_wires):
        header.append(f"* `w {wire}`: `{wire_names[wire] or '(unnamed)'}`")
    header += ["-/", "", f"namespace {ns}", ""]
    body = [
        f"/-- The circom prime `{meta['prime_name']}`. -/",
        f"abbrev p : ℕ := {p}",
        "",
        "/-- Signal values. -/",
        "abbrev F : Type := ZMod p",
        "",
        "/-- Number of R1CS wires (wire 0 is the constant one). -/",
        f"abbrev nWires : ℕ := {r.n_wires}",
        "",
        *_long_list_option(outputs),
        f"/-- Output signals of the main component: {summarize_names([wire_names[i] or '' for i in outputs])}. -/",
        f"def Outputs : List (Fin nWires) := {_list_literal(outputs)}",
        "",
        *_long_list_option(inputs),
        f"/-- Input signals of the main component: {summarize_names([wire_names[i] or '' for i in inputs])}. -/",
        f"def Inputs : List (Fin nWires) := {_list_literal(inputs)}",
        "",
    ]
    if preconditions:
        lists = precondition_lists(preconditions)
        for name, cond, wires in lists:
            body += [*_long_list_option(wires),
                     f"/-- Main input wires whose template input carries a circom tag (precondition `{cond}`): "
                     f"{summarize_names([wire_names[i] or '' for i in wires])}. -/",
                     f"def {name} : List (Fin nWires) := {_list_literal(wires)}", ""]
        body += ["/-- Input preconditions of the circom tags on the template's inputs; DET is stated under them. -/",
                 "def Preconditions (w : Fin nWires → F) : Prop :=",
                 "  " + " ∧ ".join(f"(∀ i ∈ {name}, {cond})" for name, cond, _ in lists), ""]
    cons = [render_constraint(a, b, c, p) for a, b, c in r.constraints]
    blocks = block_names(len(cons))
    if blocks:
        for k, name in enumerate(blocks):
            chunk = cons[k * BLOCK:(k + 1) * BLOCK]
            body.append(f"/-- R1CS constraints {k * BLOCK}–{k * BLOCK + len(chunk) - 1}. -/")
            body.append(f"def {name} (w : Fin nWires → F) : Prop :=")
            body.append(" ∧\n".join(f"  {c}" for c in chunk))
            body.append("")
        if len(blocks) > LONG_CONJUNCTION:          # a long right-nested conjunction exceeds the default depth
            body.append("set_option maxRecDepth 100000 in")
        body.append(f"/-- The compiled constraint system ({len(cons)} constraints). -/")
        body.append("def Constraints (w : Fin nWires → F) : Prop :=")
        body.append("  w 0 = 1 ∧ " + " ∧ ".join(f"{b} w" for b in blocks))
    else:
        body.append(f"/-- The compiled constraint system ({len(cons)} constraints). -/")
        body.append("def Constraints (w : Fin nWires → F) : Prop :=")
        body.append(" ∧\n".join(["  w 0 = 1"] + [f"  {c}" for c in cons]))
    body += ["", f"end {ns}", ""]
    return "\n".join(header + body)


def theorem_signature(preconditions: bool = False) -> str:
    """Text between ``theorem det`` and ``:= by`` (shared by the statement and the battery)."""
    return f" {STATEMENT_BINDERS} :\n    {STATEMENT_PROP_PRE if preconditions else STATEMENT_PROP}"


def emit_statement(ns: str, meta: dict, preconditions: bool = False) -> str:
    return "\n".join([
        f"import {model_module(ns)}",
        "",
        f"namespace {ns}",
        "",
        f"/-- Output determinism of {meta['repo_id']} `{meta['instantiation']}` ({meta['path']}): two",
        "assignments that satisfy the compiled constraints" + (" and the input preconditions of the template's"
                                                               if preconditions else "") +
        (" tags" if preconditions else "") + " and agree on every input wire agree on every",
        "output wire. -/",
        f"theorem {STATEMENT_THEOREM}{theorem_signature(preconditions)} := by",
        "  sorry",
        "",
        f"end {ns}",
        "",
    ])


def emit_fid_runner(ns: str, n_constraints: int, witness_files: Sequence[tuple[str, str]],
                    preconditions: bool = False) -> str:
    """A Lean file that evaluates ``decide (Constraints w)`` (and ``decide (Preconditions w)``) on witness
    files (one value per line)."""
    # a conjunction of BLOCK + 1 equations exceeds the default `synthInstance.maxSize`
    limits = ["set_option synthInstance.maxSize 1000000 in", "set_option maxHeartbeats 4000000 in",
              "set_option maxRecDepth 100000 in"]      # long linear combinations (wave 1)
    lines = [f"import {model_module(ns)}", "", f"namespace {ns}", ""]
    for name in block_names(n_constraints):
        lines += limits
        lines.append(f"instance instDec{name} (w : Fin nWires → F) : Decidable ({name} w) := by "
                     f"unfold {name}; infer_instance")
    lines += limits + [
        "instance instDecConstraints (w : Fin nWires → F) : Decidable (Constraints w) := by",
        "  unfold Constraints; infer_instance",
        "",
    ]
    if preconditions:
        lines += limits + [
            "instance instDecPreconditions (w : Fin nWires → F) : Decidable (Preconditions w) := by",
            "  unfold Preconditions; infer_instance",
            ""]
    lines += [
        "def loadWitness (path : String) : IO (Fin nWires → F) := do",
        "  let ls ← IO.FS.lines path",
        "  let vals := (ls.filter (· ≠ \"\")).map String.toNat!",
        "  if vals.size ≠ nWires then",
        "    throw (IO.userError s!\"{path}: {vals.size} values, expected {nWires}\")",
        "  return fun i => ((vals[i.val]! : ℕ) : F)",
        "",
        "#eval show IO Unit from do",
    ]
    for tag, path in witness_files:
        lines.append(f"  let w ← loadWitness {_lean_string(path)}")
        lines.append(f"  IO.println s!\"FID {tag} {{if decide (Constraints w) then \"ACCEPT\" else \"REJECT\"}}\"")
        if preconditions:
            lines.append(f"  IO.println s!\"PRE {tag} {{if decide (Preconditions w) then \"ACCEPT\" else \"REJECT\"}}\"")
    lines += ["  IO.println \"FID-DONE\"", "", f"end {ns}", ""]
    return "\n".join(lines)


def _lean_string(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def battery_prefix(variant: str, n_constraints: int, preconditions: bool = False, pre_lists=()) -> list[str]:
    if variant in ("V3", "V4"):
        hyps = ["w₁", "w₂", "h₁", "h₂"] + (["hp₁", "hp₂"] if preconditions else []) + ["hin"]
        out = [f"intro {' '.join(hyps)}",
               f"(try simp only [{', '.join(['Inputs', 'Outputs'] + FLATTEN_LEMMAS)}] at hin ⊢)",
               f"(try unfold {' '.join(['Constraints'] + block_names(n_constraints))} at h₁ h₂)"]
        if preconditions:
            out += ["(try unfold Preconditions at hp₁ hp₂)",
                    f"(try simp only [{', '.join(list(pre_lists) + FLATTEN_LEMMAS)}] at hp₁ hp₂)"]
        return out + (["repeat' constructor"] if variant == "V4" else [])
    unf = ([f"(try unfold {n} at *)" for n in unfold_order(n_constraints, preconditions, pre_lists)]
           + ["(try beta_reduce at *)"])
    if variant == "V0":
        return []
    if variant == "V1":
        return unf
    if variant == "V2":
        return ["intros"] + unf
    raise ValueError(variant)


def battery_theorem_name(variant: str, tactic: str) -> str:
    return f"triv_{variant}_{tactic.replace('?', 'Q')}"


def emit_battery(ns: str, n_constraints: int, variant: str, tactics: Sequence[str], heartbeats: int,
                 preconditions: bool = False, pre_lists=()) -> str:
    """One file per variant; one theorem per tactic, each followed by ``#print axioms``."""
    return emit_battery_forms(ns, n_constraints, [(variant, t) for t in tactics], heartbeats, preconditions, pre_lists)


def emit_battery_forms(ns: str, n_constraints: int, forms: Sequence[tuple[str, str]], heartbeats: int,
                       preconditions: bool = False, pre_lists=()) -> str:
    """One theorem per (variant, tactic) form, each followed by ``#print axioms``.  P2 forms (V3, V4) raise
    ``maxRecDepth`` (long literal lists) and run V4's tactic on every goal."""
    lines = ["import Mathlib", "import Std.Tactic.BVDecide", f"import {model_module(ns)}", "",
             f"namespace {ns}", ""]
    for variant, tactic in forms:
        name = battery_theorem_name(variant, tactic)
        p2 = variant in ("V3", "V4")
        if p2:
            lines.append("set_option maxRecDepth 100000 in")
        lines.append(f"set_option maxHeartbeats {heartbeats} in")
        lines.append(f"theorem {name}{theorem_signature(preconditions)} := by")
        last = f"all_goals {tactic}" if variant == "V4" else tactic
        lines += [f"  {t}" for t in battery_prefix(variant, n_constraints, preconditions, pre_lists) + [last]]
        lines.append(f"#print axioms {name}")
        lines.append("")
    lines += [f"end {ns}", ""]
    return "\n".join(lines)
