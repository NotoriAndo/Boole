import Std.Tactic.BVDecide

/-!
LRAT verification driver for the dual-cert Phase 0 experiment.

Contract under measurement (pinned Lean v4.29.1):
  `Std.Tactic.BVDecide.LRAT.check lratProof cnf = true → cnf.Unsat`
(theorem `check_sound` in `Std.Tactic.BVDecide.LRAT.Checker`).

Variable mapping: DIMACS variable v (1-based) becomes `CNF Nat` variable v-1;
`CNF.convertLRAT` lifts it back to the 1-based `PosFin` numbering that the
LRAT proof references, so solver-produced proofs check unmodified. The
harness self-check exercises this mapping end-to-end (known UNSAT accepted,
corrupted proof rejected).
-/

open Std.Tactic.BVDecide

def parseDimacs (content : String) : Except String (Nat × Std.Sat.CNF Nat) := do
  let mut nVars := 0
  let mut clauses : Array (Std.Sat.CNF.Clause Nat) := #[]
  for rawLine in content.splitOn "\n" do
    let line := rawLine.trim
    if line.isEmpty || line.startsWith "c" then
      continue
    let toks := (line.splitOn " ").filter (fun t => !t.isEmpty)
    if line.startsWith "p" then
      match toks with
      | [_, "cnf", nv, _] =>
        match nv.toNat? with
        | some n => nVars := n
        | none => throw s!"bad header: {line}"
      | _ => throw s!"bad header: {line}"
      continue
    let mut clause : Std.Sat.CNF.Clause Nat := []
    let mut terminated := false
    for tok in toks do
      match tok.toInt? with
      | none => throw s!"bad literal: {tok}"
      | some 0 => terminated := true
      | some l =>
        if terminated then throw s!"literal after clause terminator: {line}"
        let v := l.natAbs
        if v == 0 || v > nVars then throw s!"variable out of range: {tok}"
        clause := clause ++ [(v - 1, decide (l > 0))]
    if !terminated then throw s!"clause missing 0 terminator: {line}"
    clauses := clauses.push clause
  return (nVars, ⟨clauses⟩)

def main (args : List String) : IO UInt32 := do
  match args with
  | [cnfPath, lratPath] =>
    let content ← IO.FS.readFile cnfPath
    match parseDimacs content with
    | .error e =>
      IO.eprintln s!"dimacs-error: {e}"
      return 2
    | .ok (nVars, cnf) =>
      let proof ← LRAT.loadLRATProof lratPath
      let t0 ← IO.monoNanosNow
      let ok := LRAT.check proof cnf
      -- Reading t1 branches on `ok`, so the compiler cannot defer the pure
      -- check past the second clock read (observed as check_ns=0 otherwise).
      let t1 ← if ok then IO.monoNanosNow else IO.monoNanosNow
      IO.println
        s!"result={ok} check_ns={t1 - t0} vars={nVars} clauses={cnf.clauses.size} proof_steps={proof.size}"
      return (if ok then 0 else 1)
  | _ =>
    IO.eprintln "usage: zk_dualcert_lrat <cnf-file> <lrat-file>"
    return 2
