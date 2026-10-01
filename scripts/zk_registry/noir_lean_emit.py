"""Lean emitters for Noir DET packages (ACIR models).

The model module transcribes the flattened ACIR of the instantiated function (``noir_acir.Flat``):

* ``p`` (BN254 scalar field order), ``F := ZMod p``, ``nWires`` (ACIR witnesses; witnesses of inlined
  ACIR calls follow the caller's);
* ``Inputs``: the witnesses of the function parameters (ACIR private and public parameters);
  ``Outputs``: the return-value witnesses (literal lists);
* ``Constraints w`` (or ``Constraints bb w`` when the circuit calls hash / curve black boxes): the
  conjunction over the constrained opcodes, in program order:

  - ``AssertZero``: the polynomial equation ``Σ q·w a·w b + Σ q·w i + q_c = 0`` (balanced coefficients);
  - ``RANGE``: ``(w i).val < 2 ^ n``;
  - ``AND`` / ``XOR``: both operands below ``2 ^ n`` and the output equal to ``&&&`` / ``^^^`` of the
    operands' values;
  - other black boxes: ``outputs = bb k inputs`` (``bb k`` an uninterpreted function, universally
    quantified in the statement and shared by both assignments, i.e. deterministic); a black box with
    a witness predicate only constrains when the predicate is non-zero; one without outputs
    (recursive aggregation) is the uninterpreted condition ``bb k inputs = []``;
  - memory: per block, ``memRun init ops = true`` with the block's initial witnesses and its
    operations in program order (``memRun`` checks every index against the block length, reads
    against the current contents, and applies writes);
  - Brillig calls: nothing (their outputs are unconstrained hints).

The statement states output determinism exactly as the circom template does, with ``∀ bb : BlackBox``
in front when the model has black boxes.
"""
from __future__ import annotations

import re
from typing import Sequence

from . import lean_emit as E
from . import noir_acir as A

BLOCK = 64
MAX_BLOCKS_DEFAULT_DEPTH = 32         # above this many blocks `Constraints` raises maxRecDepth (models > 2,048 opcodes)
LARGE_HEARTBEATS = 4000000            # ... and the heartbeat budget of its declaration (20x the default)
LONG_LIST_HEARTBEATS = 4096           # Inputs / Outputs lists longer than this get LARGE_HEARTBEATS
STATEMENT_THEOREM = E.STATEMENT_THEOREM
STATEMENT_BINDERS = E.STATEMENT_BINDERS
STATEMENT_PROP = E.STATEMENT_PROP
STATEMENT_PROP_BB = ("∀ (bb : BlackBox) (w₁ w₂ : Fin nWires → F), Constraints bb w₁ → Constraints bb w₂ →\n"
                     "      (∀ i ∈ Inputs, w₁ i = w₂ i) → ∀ o ∈ Outputs, w₁ o = w₂ o")
LEAN_OPTIONS = E.LEAN_OPTIONS

MEM_RUN = """/-- Memory semantics of one ACIR block: `ops` are (active, write flag, index, value) in program order;
an active operation needs an index below the block length and a write flag 0 or 1; a read (flag 0) needs the
value to equal the current contents, a write (flag 1) replaces them. -/
def memRun : List F → List (Bool × F × F × F) → Bool
  | _, [] => true
  | m, (act, wr, i, v) :: ops =>
    if act then
      if wr = 1 then decide (i.val < m.length) && memRun (m.set i.val v) ops
      else if wr = 0 then decide (i.val < m.length) && decide (m.getD i.val 0 = v) && memRun m ops
      else false
    else memRun m ops"""


def _signed(c: int, p: int = A.BN254) -> int:
    c %= p
    return c if c <= p // 2 else c - p


def _num(c: int) -> str:
    return str(c) if c >= 0 else f"-{-c}"


def render_expr(e: A.Expr, p: int = A.BN254) -> str:
    """``Σ c·w a·w b + Σ c·w i + c`` with balanced coefficients, ``0`` when empty."""
    terms = []
    for q, a, b in e.mul:
        terms.append((_signed(q, p), f"w {a} * w {b}"))
    for q, i in e.lin:
        terms.append((_signed(q, p), f"w {i}"))
    if e.const % p:
        terms.append((_signed(e.const, p), None))
    if not terms:
        return "0"
    out = []
    for k, (c, atom) in enumerate(terms):
        mag, neg = abs(c), c < 0
        if atom is None:
            body = str(mag)
        else:
            body = atom if mag == 1 else f"{mag} * {atom}"
        if k == 0:
            out.append(f"-{body}" if neg and atom is None else (f"-({body})" if neg else body))
        else:
            out.append((" - " if neg else " + ") + body)
    return "".join(out)


def _in(x, typed: bool = True) -> str:
    if x[0] == "w":
        return f"w {x[1]}"
    return f"({x[1]} : F)" if typed else str(x[1])


def _val(x) -> str:
    return f"(w {x[1]}).val" if x[0] == "w" else f"({x[1]} : ℕ)"


def _list(items: Sequence[str]) -> str:
    return "[" + ", ".join(items) + "]"


def memory_conjuncts(ops: list[dict]) -> list[str]:
    blocks: dict[int, dict] = {}
    order: list[int] = []
    for op in ops:
        if op["kind"] == "mem_init":
            if op["block"] not in blocks:
                order.append(op["block"])
            blocks[op["block"]] = {"init": op["init"], "ops": []}
        elif op["kind"] == "mem_op":
            b = blocks.setdefault(op["block"], {"init": None, "ops": []})
            if op["block"] not in order:
                order.append(op["block"])
            pred = op["predicate"]
            if pred is None or pred.as_const() not in (None, 0):
                act = "true"
            elif pred.as_const() == 0:
                act = "false"
            else:
                act = f"decide ({render_expr(pred)} ≠ 0)"
            wr = op["write"].as_const()
            wr_s = str(wr) if wr is not None else f"({render_expr(op['write'])})"
            b["ops"].append(f"({act}, {wr_s}, {_term(op['index'])}, {_term(op['value'])})")
    out = []
    for blk in order:
        b = blocks[blk]
        if b["init"] is None:
            raise A.Unsupported(f"memory block {blk} is used before it is initialized")
        init = _list([f"w {i}" for i in b["init"]])
        out.append(f"memRun {init} {_list(b['ops'])} = true")
    return out


def _term(e: A.Expr) -> str:
    s = render_expr(e)
    return s if re.fullmatch(r"w \d+|\d+", s) else f"({s})"


def opcode_conjuncts(ops: list[dict], keys: list[str]) -> list[str]:
    """Lean conjuncts for every constrained opcode (memory blocks after the others, one per block)."""
    out = []
    for op in ops:
        k = op["kind"]
        if k == "assert_zero":
            out.append(f"{render_expr(op['expr'])} = 0")
        elif k == "range":
            out.append(f"{_val(op['input'])} < 2 ^ {op['bits']}")
        elif k in ("and", "xor"):
            sym = "&&&" if k == "and" else "^^^"
            out.append(f"{_val(op['lhs'])} < 2 ^ {op['bits']} ∧ {_val(op['rhs'])} < 2 ^ {op['bits']} ∧ "
                       f"(w {op['output']}).val = {_val(op['lhs'])} {sym} {_val(op['rhs'])}")
        elif k == "bb":
            idx = keys.index(op["key"])
            body = f"{_list([f'w {o}' for o in op['outputs']])} = bb {idx} {_list([_in(x) for x in op['inputs']])}"
            pred = op["predicate"]
            if pred is None or (pred[0] == "c" and pred[1] % A.BN254 != 0):
                out.append(body)
            elif pred[0] == "c":
                continue                     # constant-zero predicate: the call is disabled
            else:
                out.append(f"(w {pred[1]} ≠ 0 → {body})")
        elif k in ("mem_init", "mem_op", "brillig"):
            continue
        else:
            raise A.Unsupported(f"no Lean model for opcode kind {k}")
    return out + memory_conjuncts(ops)


def _list_options(xs) -> list[str]:
    """Options above a long list literal: the shared recursion-depth option, and for lists of thousands of
    witnesses (models beyond the wave-N1 size policy) the heartbeat budget of the compiler pass."""
    return E._long_list_option(xs) + ([f"set_option maxHeartbeats {LARGE_HEARTBEATS} in"]
                                      if len(xs) > LONG_LIST_HEARTBEATS else [])


def block_names(n: int) -> list[str]:
    return [] if n <= BLOCK else [f"Block{k}" for k in range((n + BLOCK - 1) // BLOCK)]


def model_module(ns: str) -> str:
    return E.model_module(ns)


def model_relpath(ns: str) -> str:
    return E.model_relpath(ns)


def has_memory(ops: list[dict]) -> bool:
    return any(op["kind"] in ("mem_init", "mem_op") for op in ops)


def unfold_order(n_conjuncts: int, bb: bool, memory: bool) -> list[str]:
    return (["Constraints"] + block_names(n_conjuncts) + ["Outputs", "Inputs"] + (["BlackBox"] if bb else [])
            + (["memRun"] if memory else []) + ["F", "nWires", "p"])


def _wire_comment(i: int, names: dict[int, str]) -> str:
    return f"* `w {i}`: {names[i]}"


def emit_model(ns: str, meta: dict, flat: A.Flat, names: dict[int, str]) -> tuple[str, dict]:
    keys = A.bb_keys(flat.opcodes)
    conj = opcode_conjuncts(flat.opcodes, keys)
    bb = bool(keys)
    mem = has_memory(flat.opcodes)
    header = [
        "import Mathlib",
        "",
        "/-!",
        f"# DET model: {meta['repo_id']} `{meta['instantiation']}`",
        "",
        f"Generated by `scripts/zk_registry` ({meta['generator']}); do not edit.",
        "",
        f"* Source: {meta['repo_url']} at `{meta['commit']}`, `{meta['path']}`, function `{meta['template']}`,",
        f"  instantiation `{meta['instantiation']}` (rule `{meta['rule']}`).",
        f"* Compiled with nargo {meta['nargo_version']} (`{meta['nargo_command']}`); ACIR sha256 `{meta['acir_sha256']}`.",
        "* Field: the BN254 scalar field; `F = ZMod p`.",
        "* `w : Fin nWires → F` gives a value to every ACIR witness (index = witness number; witnesses of",
        "  inlined ACIR calls follow the caller's).",
        "* `Constraints` holds iff every constrained ACIR opcode holds: AssertZero as a polynomial equation,",
        "  RANGE as `val < 2 ^ n`, AND / XOR as bitwise operations on `n`-bit operands, memory blocks through",
        "  `memRun`, and black boxes through `bb`.  Brillig calls add no constraint: their outputs are free.",
    ]
    if bb:
        header += ["* `bb k xs`: the outputs of black-box function `k` on inputs `xs` (uninterpreted; the statement",
                   "  quantifies over every interpretation, shared by both assignments):"]
        header += [f"  - `{i}`: {k}" for i, k in enumerate(keys)]
    header += ["", "## Named witnesses", ""]
    header += [_wire_comment(i, names) for i in sorted(names)] or ["(none)"]
    header += ["-/", "", f"namespace {ns}", ""]
    body = [
        "/-- The BN254 scalar field order (ACIR field). -/",
        f"abbrev p : ℕ := {A.BN254}",
        "",
        "/-- Witness values. -/",
        "abbrev F : Type := ZMod p",
        "",
        "/-- Number of witnesses. -/",
        f"abbrev nWires : ℕ := {flat.n_witnesses}",
        "",
        *_list_options(flat.outputs),
        "/-- Return-value witnesses of the instantiated function. -/",
        f"def Outputs : List (Fin nWires) := {E._list_literal(flat.outputs)}",
        "",
        *_list_options(flat.inputs),
        "/-- Parameter witnesses of the instantiated function. -/",
        f"def Inputs : List (Fin nWires) := {E._list_literal(flat.inputs)}",
        "",
    ]
    if bb:
        body += ["/-- Interpretations of the black-box functions (numbered as in the module comment). -/",
                 "abbrev BlackBox : Type := ℕ → List F → List F", ""]
    if mem:
        body += [MEM_RUN, ""]
    arg = "(bb : BlackBox) (w : Fin nWires → F)" if bb else "(w : Fin nWires → F)"
    app = "bb w" if bb else "w"
    blocks = block_names(len(conj))
    if blocks:
        for k, name in enumerate(blocks):
            chunk = conj[k * BLOCK:(k + 1) * BLOCK]
            body += [f"/-- Constraints {k * BLOCK}–{k * BLOCK + len(chunk) - 1}. -/",
                     f"def {name} {arg} : Prop :=", " ∧\n".join(f"  {c}" for c in chunk), ""]
        if len(blocks) > MAX_BLOCKS_DEFAULT_DEPTH:
            # a conjunction of hundreds of blocks: parser recursion depth and compiler heartbeats
            body += ["set_option maxRecDepth 100000 in", f"set_option maxHeartbeats {LARGE_HEARTBEATS} in"]
        body += [f"/-- The compiled constraint system ({len(conj)} conjuncts). -/",
                 f"def Constraints {arg} : Prop :=", "  " + " ∧ ".join(f"{b} {app}" for b in blocks)]
    else:
        body += [f"/-- The compiled constraint system ({len(conj)} conjuncts). -/",
                 f"def Constraints {arg} : Prop :="]
        body.append(" ∧\n".join(f"  {c}" for c in conj) if conj else "  True")
    body += ["", f"end {ns}", ""]
    info = {"black_boxes": keys, "n_conjuncts": len(conj), "memory": mem, "bb": bb}
    return "\n".join(header + body), info


def theorem_signature(bb: bool) -> str:
    return f" {STATEMENT_BINDERS} :\n    {STATEMENT_PROP_BB if bb else STATEMENT_PROP}"


def emit_statement(ns: str, meta: dict, bb: bool) -> str:
    return "\n".join([
        f"import {model_module(ns)}",
        "",
        f"namespace {ns}",
        "",
        f"/-- Output determinism of {meta['repo_id']} `{meta['instantiation']}` ({meta['path']}): two",
        "assignments that satisfy the compiled ACIR constraints" + (" (for one interpretation of the black boxes)"
                                                                     if bb else "") +
        " and agree on every parameter witness agree on every",
        "return-value witness. -/",
        f"theorem {STATEMENT_THEOREM}{theorem_signature(bb)} := by",
        "  sorry",
        "",
        f"end {ns}",
        "",
    ])


def _lean_string(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def emit_fid_runner(ns: str, n_conjuncts: int, bb: bool, witness_files: Sequence[tuple[str, str]],
                    table_file: str | None) -> str:
    """Evaluate ``decide (Constraints …)`` on witness files (one value per line); with black boxes the
    interpretation is the table in ``table_file`` (lines ``k;x1,x2,…;y1,y2,…``; missing entries give ``[]``)."""
    limits = ["set_option synthInstance.maxSize 1000000 in", "set_option maxHeartbeats 4000000 in",
              "set_option maxRecDepth 100000 in"]
    arg = "(bb : BlackBox) (w : Fin nWires → F)" if bb else "(w : Fin nWires → F)"
    app = "bb w" if bb else "w"
    lines = [f"import {model_module(ns)}", "", f"namespace {ns}", ""]
    for name in block_names(n_conjuncts):
        lines += limits
        lines.append(f"instance instDec{name} {arg} : Decidable ({name} {app}) := by unfold {name}; infer_instance")
    lines += limits + [f"instance instDecConstraints {arg} : Decidable (Constraints {app}) := by",
                       "  unfold Constraints; infer_instance", ""]
    lines += [
        "def parseVals (s : String) : List F :=",
        "  (s.splitOn \",\").filter (· ≠ \"\") |>.map (fun t => ((t.toNat! : ℕ) : F))",
        "",
        "def loadWitness (path : String) : IO (Fin nWires → F) := do",
        "  let ls ← IO.FS.lines path",
        "  let vals := (ls.filter (· ≠ \"\")).map String.toNat!",
        "  if vals.size ≠ nWires then",
        "    throw (IO.userError s!\"{path}: {vals.size} values, expected {nWires}\")",
        "  return fun i => ((vals[i.val]! : ℕ) : F)",
        "",
    ]
    if bb:
        lines += [
            "def loadTable (path : String) : IO (List (ℕ × List F × List F)) := do",
            "  let ls ← IO.FS.lines path",
            "  return (ls.toList.filter (· ≠ \"\")).map fun l =>",
            "    match l.splitOn \";\" with",
            "    | [k, xs, ys] => (k.toNat!, parseVals xs, parseVals ys)",
            "    | _ => (0, [], [])",
            "",
            "def tableBB (t : List (ℕ × List F × List F)) : BlackBox := fun k xs =>",
            "  match t.find? (fun e => e.1 = k && e.2.1 = xs) with",
            "  | some e => e.2.2",
            "  | none => []",
            "",
        ]
    lines.append("#eval show IO Unit from do")
    if bb:
        lines.append(f"  let tbl ← loadTable {_lean_string(table_file or '')}")
    for tag, path in witness_files:
        lines.append(f"  let w ← loadWitness {_lean_string(path)}")
        expr = "Constraints (tableBB tbl) w" if bb else "Constraints w"
        lines.append(f"  IO.println s!\"FID {tag} {{if decide ({expr}) then \"ACCEPT\" else \"REJECT\"}}\"")
    lines += ["  IO.println \"FID-DONE\"", "", f"end {ns}", ""]
    return "\n".join(lines)


def battery_prefix(variant: str, n_conjuncts: int, bb: bool, memory: bool) -> list[str]:
    if variant in ("V3", "V4"):
        hyps = (["bb"] if bb else []) + ["w₁", "w₂", "h₁", "h₂", "hin"]
        out = [f"intro {' '.join(hyps)}",
               f"(try simp only [{', '.join(['Inputs', 'Outputs'] + E.FLATTEN_LEMMAS)}] at hin ⊢)",
               f"(try unfold {' '.join(['Constraints'] + block_names(n_conjuncts))} at h₁ h₂)"]
        return out + (["repeat' constructor"] if variant == "V4" else [])
    unf = [f"(try unfold {n} at *)" for n in unfold_order(n_conjuncts, bb, memory)] + ["(try beta_reduce at *)"]
    if variant == "V0":
        return []
    if variant == "V1":
        return unf
    if variant == "V2":
        return ["intros"] + unf
    raise ValueError(variant)


def emit_battery_forms(ns: str, n_conjuncts: int, forms: Sequence[tuple[str, str]], heartbeats: int, bb: bool,
                       memory: bool) -> str:
    lines = ["import Mathlib", "import Std.Tactic.BVDecide", f"import {model_module(ns)}", "", f"namespace {ns}", ""]
    for variant, tactic in forms:
        name = E.battery_theorem_name(variant, tactic)
        if variant in ("V3", "V4"):
            lines.append("set_option maxRecDepth 100000 in")
        lines.append(f"set_option maxHeartbeats {heartbeats} in")
        lines.append(f"theorem {name}{theorem_signature(bb)} := by")
        last = f"all_goals {tactic}" if variant == "V4" else tactic
        lines += [f"  {t}" for t in battery_prefix(variant, n_conjuncts, bb, memory) + [last]]
        lines.append(f"#print axioms {name}")
        lines.append("")
    lines += [f"end {ns}", ""]
    return "\n".join(lines)


def abi_witness_names(abi: dict) -> list[str]:
    """Parameter element names in ABI encoding order (witness 0, 1, …)."""
    out: list[str] = []

    def walk(t: dict, name: str) -> None:
        k = t.get("kind")
        if k == "array":
            for i in range(t["length"]):
                walk(t["type"], f"{name}[{i}]")
        elif k == "string":
            for i in range(t["length"]):
                out.append(f"{name}[{i}]")
        elif k == "struct":
            for f in t["fields"]:
                walk(f["type"], f"{name}.{f['name']}")
        elif k == "tuple":
            for i, f in enumerate(t["fields"]):
                walk(f, f"{name}.{i}")
        else:
            out.append(name)

    for prm in abi.get("parameters", []):
        walk(prm["type"], prm["name"])
    return out
