/-
  BooleCheck — canonical Lean proof checker invoked by `boole-lean-runner`.

  The checker is intentionally tiny: it shells out to the host `lean`
  executable and forwards stdout/stderr. Acceptance is the proof file
  elaborating without errors and `lean` returning exit code 0. The hash of
  this file plus `lakefile.lean` is recorded in proof evidence as
  `checker_artifact_hash`, so any modification to the checker invalidates
  every previously produced proof package.

  SC.9a / ADR-0016 (a)(b) — the runner passes the committed step budget as
  two trailing args (`boole_check <proof.lean> <maxHeartbeats> <maxRecDepth>`)
  and this checker forwards them to `lean` as `-D` defaults, so the verdict
  is a pure function of (proof bytes, this checker, committed budget) and
  never of the host's wall clock. When the args are absent (manual use) the
  inner `lean` falls back to its own defaults — the committed-budget
  guarantee is owned by `boole-lean-runner`, which always passes them.
-/

def main (args : List String) : IO UInt32 := do
  let some proofPath := args.head?
    | IO.eprintln "usage: boole_check <proof.lean> [maxHeartbeats] [maxRecDepth]"; return 64
  let leanArgs :=
    match args with
    | _ :: maxHeartbeats :: maxRecDepth :: _ =>
      #[s!"-DmaxHeartbeats={maxHeartbeats}", s!"-DmaxRecDepth={maxRecDepth}", proofPath]
    | _ => #[proofPath]
  let output ← IO.Process.output {
    cmd := "lean"
    args := leanArgs
  }
  if output.stdout.length > 0 then
    IO.print output.stdout
  if output.stderr.length > 0 then
    IO.eprint output.stderr
  if output.exitCode == 0 then
    return 0
  else
    return 1
