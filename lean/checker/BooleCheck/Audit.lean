/-
  BooleCheck/Audit.lean — artifact-only kernel replay and axiom audit.

  ADR-0013 requires the auditor to live outside the process that elaborates
  untrusted source. `BooleCheck.Main` is therefore the only process that
  receives the submitted `.lean` path. It emits a request-local `.olean`;
  this fresh process receives only that serialized environment, reloads its
  trusted imports without initializers, and replays every serialized safe
  declaration through Lean's kernel.

  The submitted source is neither opened nor parsed here, so elaboration-time
  commands cannot run a second time inside the auditor. For every declaration
  named by the artifact, the audit computes the transitive axiom closure via
  `Lean.CollectAxioms` — the machinery behind `#print axioms` — and prints:

    BOOLE_AXIOM <axiom name>       -- one line per axiom in the closure
    BOOLE_AXIOM_AUDIT_DONE         -- sentinel: audit ran to completion

  `crates/boole-lean-runner/src/lib.rs` parses this stdout and rejects
  the submission unless every printed axiom is in the allowlist
  {propext, Classical.choice, Quot.sound} AND the `BOOLE_AXIOM_AUDIT_DONE`
  sentinel is present. A missing sentinel (crash, timeout, kill) is treated
  as retryable unavailability, never as a proof verdict or silent acceptance.
-/
import Lean
import Lean.Replay

open Lean

/-- Submitted declarations mapped to their combined transitive axiom closure. -/
def declaredAxioms (newNames : Array Name) (finalEnv : Environment) : Array Name := Id.run do
  let mut st : Lean.CollectAxioms.State := {}
  for name in newNames do
    st := (((Lean.CollectAxioms.collect name).run finalEnv).run st).snd
  return st.axioms

def main (args : List String) : IO UInt32 := do
  let some artifactPath := args.head?
    | IO.eprintln "usage: boole_axiom_audit <proof.olean>"
      return 64
  let (artifact, _region) ← Lean.readModuleData artifactPath
  if artifact.isModule then
    IO.eprintln "BOOLE_UNSUPPORTED_MODULE_ARTIFACT"
    return 1
  if artifact.constNames.size != artifact.constants.size then
    IO.eprintln "BOOLE_MALFORMED_ARTIFACT constant-count-mismatch"
    return 1
  for (name, info) in artifact.constNames.zip artifact.constants do
    if name != info.name then
      IO.eprintln "BOOLE_MALFORMED_ARTIFACT constant-name-mismatch"
      return 1
  -- The primary serializer records Lean's implicit `Init` import first.  Beyond that
  -- unavoidable base, the only import a submission may bind into its
  -- artifact is the request-private trusted helper compiled by the parent.
  -- Check the exact ordered vector before loading anything: this is the
  -- auditor's independent backstop if source intake is ever bypassed.
  if artifact.imports.map (fun entry => entry.module) !=
      #[`Init, `Boole.Family.V0Helpers] then
    IO.eprintln "BOOLE_UNEXPECTED_IMPORT exact-import-set-required"
    return 1
  -- `loadExts := false` is the default: imported environment extensions and
  -- initializers cannot execute inside this process.
  let baseEnv ← Lean.importModules artifact.imports {}
  for info in artifact.constants do
    if info.isUnsafe || info.isPartial then
      IO.eprintln s!"BOOLE_UNREPLAYABLE_CONSTANT {info.name}"
      return 1
  let constants := artifact.constants.foldl
    (fun out info => out.insert info.name info)
    ({} : Std.HashMap Name ConstantInfo)
  if constants.size != artifact.constants.size then
    IO.eprintln "BOOLE_MALFORMED_ARTIFACT duplicate-constant-name"
    return 1
  -- Re-kernel-check every safe declaration from the serialized environment.
  let finalEnv? ← try
    pure (some (← baseEnv.replay constants))
  catch _ =>
    IO.eprintln "BOOLE_MALFORMED_ARTIFACT kernel-replay-failed"
    pure none
  let some finalEnv := finalEnv?
    | return 1
  let axioms := declaredAxioms artifact.constNames finalEnv
  for ax in axioms.qsort (fun a b => a.toString < b.toString) do
    IO.println s!"BOOLE_AXIOM {ax}"
  IO.println "BOOLE_AXIOM_AUDIT_DONE"
  return 0
