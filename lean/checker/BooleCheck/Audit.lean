/-
  BooleCheck/Audit.lean — post-elaboration axiom-closure audit (ADR-0013).

  Why this file exists as a SEPARATE entrypoint rather than a check bolted
  onto `BooleCheck.Main`: ADR-0013 ratified that the axiom audit "must run in
  a stage the submitted source cannot influence — a separate pass/process
  from the elaboration of user input, not code executing inside the same
  elaboration session (the auditor must not live inside the audited)". This
  file is invoked as its own `lean --run` process, started fresh AFTER
  `BooleCheck.Main` has already accepted the submitted file in a first,
  independent process. It re-parses and re-elaborates the same source text
  from scratch into a brand-new `Environment`/`Command.State` that the
  submitted file's own commands never touch: `declaredAxioms` below calls
  `Lean.CollectAxioms.collect`, a reference resolved against this file's own
  compiled code, not looked up dynamically through the elaborated
  environment — so nothing the submitted source declares (even via
  `Lean.addDecl`) can redirect what this audit itself runs. The submitted
  source can only influence what ends up IN the environment; this audit
  inspects that environment from the outside, after the fact.

  For every declaration the submitted file *newly introduces* (constants
  present in the fully elaborated environment but absent from the
  header-only baseline environment obtained before any of the file's own
  commands run), this computes the transitive axiom closure via
  `Lean.CollectAxioms` — the same machinery backing `#print axioms` — and
  prints it in a machine-readable form on stdout:

    BOOLE_AXIOM <axiom name>       -- one line per axiom in the closure
    BOOLE_AXIOM_AUDIT_DONE         -- sentinel: audit ran to completion

  `crates/boole-lean-runner/src/lib.rs` (see the `run_axiom_audit`/
  `enforce_axiom_allowlist` comment there) parses this stdout and rejects
  the submission unless every printed axiom is in the allowlist
  {propext, Classical.choice, Quot.sound} AND the `BOOLE_AXIOM_AUDIT_DONE`
  sentinel is present. A missing sentinel (crash, timeout, kill) is treated
  as rejection, never as silent acceptance.
-/
import Lean

open Lean Elab

/-- Names present in `finalEnv` but not in `baseEnv` — the declarations the
submitted file itself introduced, as opposed to anything already visible
from its imports — mapped to their combined transitive axiom closure. -/
def declaredAxioms (baseEnv finalEnv : Environment) : Array Name := Id.run do
  let mut newNames : Array Name := #[]
  for (name, _) in finalEnv.constants.toList do
    unless baseEnv.constants.contains name do
      newNames := newNames.push name
  let mut st : Lean.CollectAxioms.State := {}
  for name in newNames do
    st := (((Lean.CollectAxioms.collect name).run finalEnv).run st).snd
  return st.axioms

/-- SC.9a / ADR-0016 (a-2) layer 2 — budget-bearing option names a submitted
source must never mention. `set_option maxHeartbeats <M>` (including `0` =
unlimited) or `set_option maxRecDepth <M>` would override the committed
budget this audit itself elaborates under, making the consensus budget
advisory. The scan is over the RAW source text (comments and strings
included): deliberately stricter than the Rust-side intake scan, because
this is the last line and must stay simple enough to be obviously right. -/
def budgetOverrideTokens : List String := ["maxHeartbeats", "maxRecDepth"]

def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_axiom_audit <proof.lean> [maxHeartbeats] [maxRecDepth]"
      return 64
  let input ← IO.FS.readFile proofPath
  -- ADR-0016 (a-2) layer 2: refuse budget-override tokens BEFORE any of the
  -- submitted file's content is parsed or elaborated. Independent of the
  -- Rust-side intake scan: a source slipping past that layer still cannot
  -- buy steps here.
  for token in budgetOverrideTokens do
    if (input.splitOn token).length > 1 then
      IO.eprintln s!"BOOLE_BUDGET_OVERRIDE {token}"
      return 1
  -- SC.9a / ADR-0016 (a)(b) — elaborate under the SAME committed step
  -- budget the primary checker ran under (runner passes it as trailing
  -- args), so audit and primary cannot diverge on resource grounds.
  let opts : Lean.Options := {}
  let opts := match (args.drop 1).head?.bind (·.toNat?) with
    | some maxHeartbeats => opts.set `maxHeartbeats maxHeartbeats
    | none => opts
  let opts := match (args.drop 2).head?.bind (·.toNat?) with
    | some maxRecDepth => opts.set `maxRecDepth maxRecDepth
    | none => opts
  let inputCtx := Lean.Parser.mkInputContext input proofPath
  let (header, parserState, msgs) ← Lean.Parser.parseHeader inputCtx
  let (baseEnv, msgs) ← Lean.Elab.processHeader header opts msgs inputCtx
  if msgs.hasErrors then
    IO.eprintln "AUDIT_ERROR: header failed to process"
    return 1
  let commandState := Lean.Elab.Command.mkState baseEnv msgs opts
  let frontendState ← Lean.Elab.IO.processCommands inputCtx parserState commandState
  if frontendState.commandState.messages.hasErrors then
    IO.eprintln "AUDIT_ERROR: elaboration failed"
    for msg in frontendState.commandState.messages.toList do
      IO.eprintln (← msg.toString)
    return 1
  let finalEnv := frontendState.commandState.env
  let axioms := declaredAxioms baseEnv finalEnv
  for ax in axioms.qsort (fun a b => a.toString < b.toString) do
    IO.println s!"BOOLE_AXIOM {ax}"
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
