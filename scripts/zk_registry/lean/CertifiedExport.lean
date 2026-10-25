/- Export the complete pinned state for independent two-kernel certification. -/
import CertifiedBase
import Export
open Lean

def main (args : List String) : IO UInt32 := do
  let [path] := args | return 70
  initSearchPath (← findSysroot)
  let env ← GateBase.imported path
  let mut names : NameSet := {}
  for (name, _) in env.constants.toList do names := names.insert name
  M.run env do
    initState env
    dumpMetadata
    for name in names do
      modify fun s => { s with noMDataExprs := {} }
      dumpConstant name
  return 0
