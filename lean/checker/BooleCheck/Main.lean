/-
  BooleCheck — canonical Lean proof checker invoked by `boole-lean-runner`.

  The runner supplies one submitted source, the consensus-committed step
  budgets, and a fresh private `.olean` destination. This process is the only
  stage allowed to read and elaborate the source. On success, the separate
  audit process receives only the serialized environment.

  The checker source, helper surface, package manifest, and toolchain are
  covered by `checker_artifact_hash`; changing this boundary rotates that
  identity rather than silently changing a historical verdict.
-/
import Lean

open Lean

/-- ADR-0016 (a-2) layer 2, independent of the Rust intake scan. -/
def budgetOverrideTokens : List String := ["maxHeartbeats", "maxRecDepth"]

private def reject (marker : String := "BOOLE_PRIMARY_REJECT") : IO UInt32 := do
  IO.eprintln marker
  return 1

/-- The same snapshot frontend as `Lean.Elab.runFrontend`, retaining its typed diagnostics.
    Lean 4.29.1 uses `runtime.maxHeartbeats` and `runtime.maxRecDepth` tags;
    `stripNestedTags` also recognizes limits propagated through nested tactics. -/
private def checkedFrontend (input : String) (opts : Options) (fileName : String)
    (artifact : System.FilePath) : ExceptT Exception IO Unit := do
  let opts := Lean.internal.cmdlineSnapshots.setIfNotSet opts true
  let inputCtx := Parser.mkInputContext input fileName
  let snap ← Language.Lean.process (fun stx => pure (.ok {
    imports := stx.imports, isModule := stx.isModule,
    mainModuleName := `BooleSubmission, opts := opts, trustLevel := 0
  })) none {inputCtx with}
  let tree := Language.toSnapshotTree snap
  let hasErrors ← tree.runAndReport opts false {}
  if hasErrors then
    for child in tree.getAll do
      for msg in child.diagnostics.msgLog.toList do
        let data := msg.data.stripNestedTags
        if msg.severity == .error &&
            (data.kind == `runtime.maxHeartbeats || data.kind == `runtime.maxRecDepth) then
          throw (Exception.error .missing data)
    throw (Exception.error .missing MessageData.nil)
  let some state := Language.Lean.waitForFinalCmdState? snap
    | throw (Exception.error .missing MessageData.nil)
  writeModule state.env artifact

private def check (args : List String) : IO UInt32 := do
  let [proofPath, heartbeats, recursion, artifactPath] := args
    | return ← reject
  let some maxHeartbeats := heartbeats.toNat?
    | return ← reject
  let some maxRecDepth := recursion.toNat?
    | return ← reject
  let input ← IO.FS.readFile proofPath
  for token in budgetOverrideTokens do
    if (input.splitOn token).length > 1 then
      return ← reject s!"BOOLE_BUDGET_OVERRIDE {token}"
  let some leanSysroot ← IO.getEnv "LEAN_SYSROOT"
    | return ← reject
  initSearchPath ⟨leanSysroot⟩
  let opts := ({} : Options) |>.set `maxHeartbeats maxHeartbeats
    |>.set `maxRecDepth maxRecDepth |>.set `Elab.async false
  -- Lean exceptions retain their structured kinds until this single handler.
  -- IO failures escaping the frontend are handled by `main`, without text inference.
  match ← (checkedFrontend input opts proofPath ⟨artifactPath⟩).run with
  | .ok () => return 0
  | .error ex =>
    if ex.isMaxHeartbeat then
      reject "BOOLE_BUDGET_EXCEEDED maxHeartbeats"
    else if ex.isMaxRecDepth then
      reject "BOOLE_BUDGET_EXCEEDED maxRecDepth"
    else
      reject

def main (args : List String) : IO UInt32 := do
  try check args
  catch _ => reject
