/- Test-only state, entirely generated from a public reference already checked
   by both kernels. Never a production certificate or calibration authority. -/
import Lean.Environment
import Export.Parse
open Lean

def main (args : List String) : IO UInt32 := do
  let [input, output, omitted] := args | return 70
  let source ← IO.FS.Handle.mk input .read
  let parsed ← Export.parseStream (IO.FS.Stream.ofHandle source)
  let constants := parsed.constMap.toList.filter (fun (name, _) => name.toString != omitted)
    |>.map Prod.snd |>.toArray
  let data : ModuleData := {
    isModule := false
    imports := #[]
    constants := constants
    constNames := constants.map (·.name)
    extraConstNames := #[]
    entries := #[]
  }
  saveModuleData output `EENV5.CertifiedState data
  return 0
