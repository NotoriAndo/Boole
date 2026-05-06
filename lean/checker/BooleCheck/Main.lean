/-
  BooleCheck — canonical Lean proof checker invoked by `boole-lean-runner`.

  The checker is intentionally tiny: it shells out to the host `lean`
  executable and forwards stdout/stderr. Acceptance is the proof file
  elaborating without errors and `lean` returning exit code 0. The hash of
  this file plus `lakefile.lean` is recorded in proof evidence as
  `checker_artifact_hash`, so any modification to the checker invalidates
  every previously produced proof package.
-/

def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_check <proof.lean>"; return 64
  let output ← IO.Process.output {
    cmd := "lean"
    args := #[proofPath]
  }
  if output.stdout.length > 0 then
    IO.print output.stdout
  if output.stderr.length > 0 then
    IO.eprint output.stderr
  if output.exitCode == 0 then
    return 0
  else
    return 1
