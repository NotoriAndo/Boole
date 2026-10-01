"""Lean emitters for AIR DET packages.

The model module transcribes one extracted AIR (``air_ir``) for one row window:

* ``p``, ``F := ZMod p``, ``nVars``; ``w : Fin nVars → F`` assigns the window's variables (local row, the next
  row when a constraint references it, preprocessed cells, the public values and row selectors used);
* ``Constraints w``: every constraint polynomial of the AIR vanishes (blocks of :data:`BLOCK` conjuncts);
  shared or large subterms are hoisted into ``t<k> w`` definitions;
* ``Assumptions w``: the bus facts DET is stated under — table lookups the AIR sends to other chips hold
  (``m ≠ 0 → Table [..] = true``) and, when selectors are used, their normalized values;
* ``Fixed``: variables both windows share (preprocessed cells, public values, selectors);
* ``In w`` / ``Out w``: input and output bus messages ``(multiplicity, values)``;
* ``BusEq``: equal bus contributions (equal multiplicities; equal values where the multiplicity is non-zero).

The statement (theorem ``det``): two windows that satisfy the constraints and the assumptions, share the fixed
variables and have equal input messages have equal output messages.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Sequence

from . import air_ir as IR
from . import lean_emit as E

BLOCK = 64
HOIST_MIN = 12          # shared subterms at least this large are hoisted into definitions
HOIST_MAX = 1500        # any subterm larger than this (after hoisting its children) is hoisted
STATEMENT_THEOREM = "det"
STATEMENT_BINDERS = "[Fact (Nat.Prime p)]"
STATEMENT_PROP = ("∀ w₁ w₂ : Fin nVars → F, Constraints w₁ → Constraints w₂ → Assumptions w₁ → Assumptions w₂ →\n"
                  "      (∀ i ∈ Fixed, w₁ i = w₂ i) → BusEq (In w₁) (In w₂) → BusEq (Out w₁) (Out w₂)")
LEAN_OPTIONS = list(E.LEAN_OPTIONS)
BUS_LEMMAS = ["Fixed", "In", "Out", "BusEq", "MsgEq", "List.forall₂_cons", "List.Forall₂.nil",
              "List.forall_mem_cons", "List.forall_mem_nil", "List.not_mem_nil", "IsEmpty.forall_iff",
              "and_true", "implies_true", "forall_const"]


@dataclass
class Message:
    """One input or output message: the interaction it comes from and the value nodes in the role."""
    interaction: int
    mult: int
    values: list[int]
    note: str


@dataclass
class Assumption:
    interaction: int
    table: str                  # Lean predicate name (``List F → Bool``)
    mult: int
    values: list[int]
    note: str


@dataclass
class Roles:
    inputs: list[Message] = field(default_factory=list)
    outputs: list[Message] = field(default_factory=list)
    assumptions: list[Assumption] = field(default_factory=list)
    selectors: bool = False     # add the normalized-selector assumption


# ------------------------------------------------------------------------------------------ expressions

def _const(c: int, p: int) -> str:
    return str(c) if c <= p // 2 else f"(-{p - c})"


class Printer:
    """Renders DAG nodes as Lean terms over ``w``; hoists shared / large subterms into ``t<k> w``."""

    def __init__(self, air: IR.Air, layout: IR.Layout, roots: Sequence[int]):
        self.air, self.layout = air, layout
        uses = [0] * len(air.nodes)
        for r in roots:
            uses[r] += 1
        reach = [False] * len(air.nodes)
        for r in roots:
            reach[r] = True
        for k in range(len(air.nodes) - 1, -1, -1):
            if not reach[k]:
                continue
            n = air.nodes[k]
            for c in n[1:] if n[0] in IR.BIN_OPS or n[0] == "neg" else ():
                uses[c] += 1
                reach[c] = True
        self.reach = reach
        self.text: dict[int, str] = {}
        self.size: dict[int, int] = {}
        self.hoisted: list[int] = []
        self._body: dict[int, str] = {}
        for k, n in enumerate(air.nodes):
            if not reach[k]:
                continue
            op = n[0]
            if op == "const":
                t, s = _const(n[1], air.p), 1
            elif op in IR.LEAF_OPS:
                t, s = f"w {layout.var(n)}", 1
            elif op == "neg":
                t, s = f"(-{self.text[n[1]]})", 1 + self.size[n[1]]
            else:
                sym = {"add": "+", "sub": "-", "mul": "*"}[op]
                t, s = f"({self.text[n[1]]} {sym} {self.text[n[2]]})", 1 + self.size[n[1]] + self.size[n[2]]
            if op not in IR.LEAF_OPS and ((uses[k] >= 2 and s >= HOIST_MIN) or s > HOIST_MAX):
                self.hoisted.append(k)
                self._body[k] = t
                self.text[k], self.size[k] = f"t{k} w", 1
            else:
                self.text[k], self.size[k] = t, s

    def term(self, k: int) -> str:
        t = self.text[k]
        if t.startswith("(") and t.endswith(")") and k not in self.hoisted and _balanced_outer(t):
            return t[1:-1]
        return t

    def definitions(self) -> list[str]:
        out = []
        for k in self.hoisted:
            t = self._body[k]
            if t.startswith("(") and t.endswith(")") and _balanced_outer(t):
                t = t[1:-1]
            out += [f"/-- Shared subterm (node {k}). -/", f"def t{k} (w : Fin nVars → F) : F :=", f"  {t}", ""]
        return out

    def hoisted_names(self) -> list[str]:
        return [f"t{k}" for k in self.hoisted]


def _balanced_outer(t: str) -> bool:
    """``t`` = ``( ... )`` where the first parenthesis closes at the end."""
    depth = 0
    for i, ch in enumerate(t):
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0 and i != len(t) - 1:
                return False
    return depth == 0


# ------------------------------------------------------------------------------------------ model

def model_module(ns: str) -> str:
    return f"{ns}.Model"


def model_relpath(ns: str) -> str:
    return model_module(ns).replace(".", "/") + ".lean"


def block_names(n: int) -> list[str]:
    return [] if n <= BLOCK else [f"Block{k}" for k in range((n + BLOCK - 1) // BLOCK)]


def asm_block_names(n: int) -> list[str]:
    return [] if n <= BLOCK else [f"AsmBlock{k}" for k in range((n + BLOCK - 1) // BLOCK)]


def roots_of(air: IR.Air, roles: Roles) -> list[int]:
    roots = list(air.constraints)
    for m in roles.inputs + roles.outputs:
        roots += [m.mult] + m.values
    for a in roles.assumptions:
        roots += [a.mult] + a.values
    return roots


def _msg(pr: Printer, m) -> str:
    return f"({pr.term(m.mult)}, [{', '.join(pr.term(v) for v in m.values)}])"


def _msg_list(pr: Printer, name: str, doc: str, msgs: Sequence[Message]) -> list[str]:
    out = [f"/-- {doc} -/", f"def {name} (w : Fin nVars → F) : List Msg :="]
    if not msgs:
        return out + ["  []", ""]
    lines = []
    for k, m in enumerate(msgs):
        lines.append(f"    {_msg(pr, m)}{',' if k + 1 < len(msgs) else ''}  -- {m.note}")
    return out + ["  ["] + lines + ["  ]", ""]


def emit_model(ns: str, meta: dict, air: IR.Air, layout: IR.Layout, roles: Roles, tables: dict[str, str]) -> tuple[str, dict]:
    """Model module text and a summary (hoisted definitions, block names, tables used) for the other emitters."""
    pr = Printer(air, layout, roots_of(air, roles))
    names = layout.names()
    used_tables = sorted({a.table for a in roles.assumptions})
    header = [
        "import Mathlib",
        "",
        "/-!",
        f"# DET model: {meta['zkvm_name']} {meta['release']} AIR `{air.name}`",
        "",
        f"Generated by `scripts/zk_registry` ({meta['generator']}); do not edit.",
        "",
        f"* Source: {meta['repo_url']} at `{air.commit}`; AIR `{air.name}` (Rust type `{air.rust_type}`,",
        f"  group `{air.group}`, position {air.index} of the machine's AIR list).",
        f"* Extracted with {meta['extractor']}; IR sha256 `{meta['ir_sha256']}`.",
        f"* Field: {air.field_name}, `p = {air.p}`; `F = ZMod p`.",
        f"* One {'two-row' if layout.main_next or layout.prep_next else 'one-row'} window: `w : Fin nVars → F`"
        " (variables below).",
        f"* `Constraints w`: the AIR's {len(air.constraints)} constraint polynomials vanish (guards and row",
        "  selectors are factors of the polynomials, as the symbolic builder records them). Constants are printed",
        "  balanced (`c > p/2` as `-(p - c)`), which preserves values in `ZMod p`.",
        "* `Assumptions w`: bus facts provided by other chips (table lookups this AIR sends)"
        + (" and normalized row selectors" if roles.selectors else "") + ".",
        "* `Fixed`: variables shared by both windows; `In w` / `Out w`: input / output bus messages",
        "  `(multiplicity, values)`; `BusEq`: equal multiplicities, equal values where the multiplicity is non-zero.",
        "",
        "## Variables",
        "",
    ]
    for k, nm in enumerate(names):
        header.append(f"* `w {k}`: {nm}")
    # long sums and large windows exceed the default recursion depth while elaborating (file-local option)
    header += ["-/", "", "set_option maxRecDepth 100000", "", f"namespace {ns}", ""]
    body = [
        f"/-- The {air.field_name} prime. -/",
        f"abbrev p : ℕ := {air.p}",
        "",
        "/-- Trace cell values. -/",
        "abbrev F : Type := ZMod p",
        "",
        "/-- Number of window variables. -/",
        f"abbrev nVars : ℕ := {layout.n_vars}",
        "",
        "/-- A bus message: multiplicity and values. -/",
        "abbrev Msg : Type := F × List F",
        "",
        "/-- Equal contributions of one message: equal multiplicities, equal values where the multiplicity is",
        "non-zero. -/",
        "def MsgEq (a b : Msg) : Prop := a.1 = b.1 ∧ (a.1 ≠ 0 → a.2 = b.2)",
        "",
        "/-- Equal contributions of two message lists, message by message. -/",
        "def BusEq (a b : List Msg) : Prop := List.Forall₂ MsgEq a b",
        "",
    ]
    for t in used_tables:
        body += [tables[t], ""]
    body += pr.definitions()
    cons = [f"{pr.term(c)} = 0" for c in air.constraints]
    blocks = block_names(len(cons))
    if blocks:
        for k, name in enumerate(blocks):
            chunk = cons[k * BLOCK:(k + 1) * BLOCK]
            body += [f"/-- Constraints {k * BLOCK}–{k * BLOCK + len(chunk) - 1}. -/",
                     f"def {name} (w : Fin nVars → F) : Prop :=", " ∧\n".join(f"  {c}" for c in chunk), ""]
        body += [f"/-- The AIR's constraints ({len(cons)}). -/", "def Constraints (w : Fin nVars → F) : Prop :=",
                 "  " + " ∧ ".join(f"{b} w" for b in blocks), ""]
    else:
        body += [f"/-- The AIR's constraints ({len(cons)}). -/", "def Constraints (w : Fin nVars → F) : Prop :="]
        body += [" ∧\n".join(f"  {c}" for c in cons) if cons else "  True", ""]
    asm = []
    for a in roles.assumptions:
        asm.append(f"({pr.term(a.mult)} ≠ 0 → {a.table} [{', '.join(pr.term(v) for v in a.values)}] = true)")
    if roles.selectors:
        o = layout.offsets()["sel"]
        for i, s in enumerate(layout.selectors):
            v = f"w {o + i}"
            asm.append(f"({v} = 0 ∨ {v} = 1)")
        if "trans" in layout.selectors and "last" in layout.selectors:
            asm.append(f"w {o + layout.selectors.index('trans')} = 1 - w {o + layout.selectors.index('last')}")
    notes = [_asm_note(roles, k) if k < len(roles.assumptions) else "" for k in range(len(asm))]

    def conj(items: list[str], item_notes: list[str]) -> str:
        # the conjunction symbol precedes the line comment, never inside it
        lines = []
        for k, a in enumerate(items):
            sep = " ∧" if k + 1 < len(items) else ""
            note = f"  -- {item_notes[k]}" if item_notes[k] else ""
            lines.append(f"  {a}{sep}{note}")
        return "\n".join(lines)
    asm_doc = ("Bus facts assumed (lookups into tables that other chips provide)"
               + (" and normalized row selectors" if roles.selectors else ""))
    asm_blocks = asm_block_names(len(asm))
    if asm_blocks:
        for k, name in enumerate(asm_blocks):
            lo, hi = k * BLOCK, min((k + 1) * BLOCK, len(asm))
            body += [f"/-- Assumptions {lo}–{hi - 1}. -/", f"def {name} (w : Fin nVars → F) : Prop :=",
                     conj(asm[lo:hi], notes[lo:hi]), ""]
        body += [f"/-- {asm_doc} ({len(asm)}). -/", "def Assumptions (w : Fin nVars → F) : Prop :=",
                 "  " + " ∧ ".join(f"{b} w" for b in asm_blocks), ""]
    else:
        body += [f"/-- {asm_doc}. -/", "def Assumptions (w : Fin nVars → F) : Prop :=",
                 conj(asm, notes) if asm else "  True", ""]
    fixed = layout.fixed()
    body += [*E._long_list_option(fixed),
             "/-- Variables both windows share: preprocessed cells, public values, row selectors. -/",
             f"def Fixed : List (Fin nVars) := [{', '.join(str(i) for i in fixed)}]", ""]
    body += _msg_list(pr, "In", "Input messages (received values, and values provided to this AIR).", roles.inputs)
    body += _msg_list(pr, "Out", "Output messages (values this AIR is responsible for).", roles.outputs)
    body += [f"end {ns}", ""]
    summary = {"hoisted": pr.hoisted_names(), "blocks": blocks, "asm_blocks": asm_blocks, "tables": used_tables,
               "n_constraints": len(cons), "n_vars": layout.n_vars}
    return "\n".join(header + body), summary


def _asm_note(roles: Roles, k: int) -> str:
    return roles.assumptions[k].note


def theorem_signature() -> str:
    return f" {STATEMENT_BINDERS} :\n    {STATEMENT_PROP}"


def emit_statement(ns: str, meta: dict, air: IR.Air) -> str:
    return "\n".join([
        f"import {model_module(ns)}",
        "",
        f"namespace {ns}",
        "",
        f"/-- Output determinism of {meta['zkvm_name']} {meta['release']} AIR `{air.name}`: two windows that satisfy",
        "the AIR's constraints and the bus assumptions, share the fixed variables and carry equal input messages",
        "carry equal output messages. -/",
        f"theorem {STATEMENT_THEOREM}{theorem_signature()} := by",
        "  sorry",
        "",
        f"end {ns}",
        "",
    ])


# ------------------------------------------------------------------------------------------ evaluation harness

LIMITS = ["set_option synthInstance.maxSize 1000000 in", "set_option maxHeartbeats 4000000 in",
          "set_option maxRecDepth 100000 in"]


def _decidable_instances(summary: dict, loader: bool = True) -> list[str]:
    lines = []
    for name in summary["blocks"] + summary.get("asm_blocks", []):
        lines += LIMITS + [f"instance instDec{name} (w : Fin nVars → F) : Decidable ({name} w) := by "
                           f"unfold {name}; infer_instance"]
    lines += LIMITS + ["instance instDecConstraints (w : Fin nVars → F) : Decidable (Constraints w) := by",
                       "  unfold Constraints; infer_instance", ""]
    lines += LIMITS + ["instance instDecAssumptions (w : Fin nVars → F) : Decidable (Assumptions w) := by",
                       "  unfold Assumptions; infer_instance", ""]
    lines += ["instance instDecMsgEq (a b : Msg) : Decidable (MsgEq a b) := by unfold MsgEq; infer_instance",
              "instance instDecBusEq (a b : List Msg) : Decidable (BusEq a b) := by unfold BusEq; infer_instance", ""]
    if not loader:
        return lines
    lines += ["def loadWindow (path : String) : IO (Fin nVars → F) := do",
              "  let ls ← IO.FS.lines path",
              "  let vals := (ls.filter (· ≠ \"\")).map String.toNat!",
              "  if vals.size ≠ nVars then",
              "    throw (IO.userError s!\"{path}: {vals.size} values, expected {nVars}\")",
              "  return fun i => ((vals[i.val]! : ℕ) : F)", ""]
    return lines


def emit_fid_runner(ns: str, summary: dict, files: Sequence[tuple[str, str]]) -> str:
    """Evaluates ``decide (Constraints w)`` and ``decide (Assumptions w)`` on window files (one value per line)."""
    lines = [f"import {model_module(ns)}", "", f"namespace {ns}", ""] + _decidable_instances(summary)
    lines.append("#eval show IO Unit from do")
    for tag, path in files:
        lines.append(f"  let w ← loadWindow {E._lean_string(path)}")
        lines.append(f"  IO.println s!\"FID {tag} {{if decide (Constraints w) then \"ACCEPT\" else \"REJECT\"}}\"")
        lines.append(f"  IO.println s!\"ASM {tag} {{if decide (Assumptions w) then \"ACCEPT\" else \"REJECT\"}}\"")
    lines += ["  IO.println \"FID-DONE\"", "", f"end {ns}", ""]
    return "\n".join(lines)


def emit_pair_runner(ns: str, summary: dict, w1: str, w2: str) -> str:
    """Evaluates every hypothesis and the conclusion of ``det`` on one pair of windows."""
    lines = [f"import {model_module(ns)}", "", f"namespace {ns}", ""] + _decidable_instances(summary)
    lines += ["#eval show IO Unit from do",
              f"  let a ← loadWindow {E._lean_string(w1)}",
              f"  let b ← loadWindow {E._lean_string(w2)}",
              "  IO.println s!\"PAIR constraints {decide (Constraints a)} {decide (Constraints b)}\"",
              "  IO.println s!\"PAIR assumptions {decide (Assumptions a)} {decide (Assumptions b)}\"",
              "  IO.println s!\"PAIR fixed {decide (∀ i ∈ Fixed, a i = b i)}\"",
              "  IO.println s!\"PAIR inputs {decide (BusEq (In a) (In b))}\"",
              "  IO.println s!\"PAIR outputs {decide (BusEq (Out a) (Out b))}\"",
              "  IO.println \"PAIR-DONE\"", "", f"end {ns}", ""]
    return "\n".join(lines)


def emit_kernel_checks(ns: str, summary: dict, windows: Sequence[tuple[str, Sequence[int]]],
                       claims: Sequence[tuple[str, str]]) -> tuple[str, dict[str, tuple[int, int]]]:
    """Kernel-checked evaluation, for models whose compiled evaluation re-evaluates shared subterms exponentially
    (the kernel caches reductions of closed terms).  ``windows``: (tag, values) become closed windows ``w_<tag>``;
    ``claims``: (name, proposition over those windows), each a theorem proved by ``decide +kernel``.  A claim holds
    iff its theorem elaborates without error; returns the text and each claim's line range."""
    lines = [f"import {model_module(ns)}", "", f"namespace {ns}", ""] + _decidable_instances(summary, loader=False)
    for tag, vals in windows:
        lines.append(f"def vals_{tag} : List ℕ := [{', '.join(str(v) for v in vals)}]")
        lines.append(f"def w_{tag} : Fin nVars → F := fun i => ((vals_{tag}.getD i.val 0 : ℕ) : F)")
    lines.append("")
    ranges: dict[str, tuple[int, int]] = {}
    for name, prop in claims:
        start = len(lines) + 1
        lines += ["set_option maxRecDepth 100000 in", "set_option maxHeartbeats 0 in",
                  f"theorem {name} : {prop} := by decide +kernel"]
        ranges[name] = (start, len(lines))
    lines += ["", f"end {ns}", ""]
    return "\n".join(lines), ranges


def write_window(path: str, w: Sequence[int]) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(str(v) for v in w) + "\n")


# ------------------------------------------------------------------------------------------ battery

def unfold_order(summary: dict) -> list[str]:
    """Every definition of the model module, consumers before the definitions they use."""
    return (["Constraints", *summary["blocks"], "Assumptions", *summary.get("asm_blocks", []), *summary["tables"],
             "Out", "In", "Fixed", "BusEq", "MsgEq"] + list(reversed(summary["hoisted"])) + ["Msg", "F", "nVars", "p"])


def battery_prefix(variant: str, summary: dict) -> list[str]:
    if variant in ("V3", "V4"):
        out = ["intro w₁ w₂ h₁ h₂ ha₁ ha₂ hfix hin",
               f"(try simp only [{', '.join(BUS_LEMMAS)}] at hfix hin ⊢)",
               f"(try unfold {' '.join(['Constraints'] + summary['blocks'])} at h₁ h₂)",
               f"(try unfold {' '.join(['Assumptions'] + summary.get('asm_blocks', []))} at ha₁ ha₂)"]
        if summary["hoisted"]:
            out.append(f"(try unfold {' '.join(reversed(summary['hoisted']))} at *)")
        return out + (["repeat' constructor"] if variant == "V4" else [])
    unf = [f"(try unfold {n} at *)" for n in unfold_order(summary)] + ["(try beta_reduce at *)"]
    if variant == "V0":
        return []
    if variant == "V1":
        return unf
    if variant == "V2":
        return ["intros"] + unf
    raise ValueError(variant)


def emit_battery_forms(ns: str, summary: dict, forms: Sequence[tuple[str, str]], heartbeats: int) -> str:
    """One theorem per (variant, tactic) form, each followed by ``#print axioms`` (same layout as the circom
    battery, so ``gates.classify_battery`` reads the results)."""
    lines = ["import Mathlib", "import Std.Tactic.BVDecide", f"import {model_module(ns)}", "", f"namespace {ns}", ""]
    for variant, tactic in forms:
        name = E.battery_theorem_name(variant, tactic)
        if variant in ("V3", "V4"):
            lines.append("set_option maxRecDepth 100000 in")
        lines.append(f"set_option maxHeartbeats {heartbeats} in")
        lines.append(f"theorem {name}{theorem_signature()} := by")
        last = f"all_goals {tactic}" if variant == "V4" else tactic
        lines += [f"  {t}" for t in battery_prefix(variant, summary) + [last]]
        lines.append(f"#print axioms {name}")
        lines.append("")
    lines += [f"end {ns}", ""]
    return "\n".join(lines)
