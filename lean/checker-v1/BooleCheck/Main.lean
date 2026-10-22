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

/-- ADR-0016 (a-2) layer 2, independent from the Rust intake scan. -/
def budgetOverrideTokens : List String := ["maxHeartbeats", "maxRecDepth"]

private def parseBudget (name value : String) : IO (Option Nat) := do
  match value.toNat? with
  | some parsed => return some parsed
  | none =>
    IO.eprintln s!"invalid {name}: expected an unsigned integer"
    return none

def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_check <proof.lean> <maxHeartbeats> <maxRecDepth> <proof.olean>"
      return 64
  let some maxHeartbeats := (args.drop 1).head?
    | IO.eprintln "missing maxHeartbeats"; return 64
  let some maxRecDepth := (args.drop 2).head?
    | IO.eprintln "missing maxRecDepth"; return 64
  let some artifactPath := (args.drop 3).head?
    | IO.eprintln "missing checker artifact path"; return 64
  if args.length != 4 then
    IO.eprintln "usage: boole_check <proof.lean> <maxHeartbeats> <maxRecDepth> <proof.olean>"
    return 64

  let some maxHeartbeats ← parseBudget "maxHeartbeats" maxHeartbeats
    | return 64
  let some maxRecDepth ← parseBudget "maxRecDepth" maxRecDepth
    | return 64

  -- Keep the second budget-ceiling guard in the one process that is allowed
  -- to see the submitted bytes. The artifact-only auditor never opens them.
  let input ← IO.FS.readFile proofPath
  for token in budgetOverrideTokens do
    if (input.splitOn token).length > 1 then
      IO.eprintln s!"BOOLE_BUDGET_OVERRIDE {token}"
      return 1

  -- Elaboration happens inside this process. In particular, there is no
  -- nested `lean` process that submitted syntax could leave detached after
  -- the primary exits. Async elaboration is disabled so the request also has
  -- no background task that can outlive artifact serialization.
  let opts : Options := {}
  let opts := opts.set `maxHeartbeats maxHeartbeats
  let opts := opts.set `maxRecDepth maxRecDepth
  let opts := Elab.async.set opts false
  let some leanSysroot ← IO.getEnv "LEAN_SYSROOT"
    | IO.eprintln "missing trusted LEAN_SYSROOT"; return 64
  initSearchPath ⟨leanSysroot⟩
  let env? ← Elab.runFrontend
    input
    opts
    proofPath
    `BooleSubmission
    0
    (some ⟨artifactPath⟩)
  if env?.isSome then
    return 0
  IO.eprintln "BOOLE_PRIMARY_REJECT"
  return 1
