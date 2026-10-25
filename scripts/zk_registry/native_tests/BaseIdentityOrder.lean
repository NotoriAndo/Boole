/- A generated payload mismatch and its identity cap must precede library availability. -/
import CertifiedBase
open Lean

def main (args : List String) : IO Unit := do
  let [path] := args | throw <| IO.userError "expected pruned state"
  let base ← GateBase.imported path
  let some (.inductInfo info) := base.toKernelEnv.find? ``True
    | throw <| IO.userError "True missing from generated base"
  let changed : ConstantInfo := .inductInfo { info with type := .sort (.succ .zero) }
  let library : ConstantInfo := .axiomInfo {
    name := `GeneratedLibrary, levelParams := [], type := .sort .zero, isUnsafe := false }
  let source := ({} : Std.HashMap Name ConstantInfo).insert ``True changed
    |>.insert `GeneratedLibrary library
  let capped ← GateBase.select source base.toKernelEnv 0 true
  unless (match capped with | .error (.cap _) => true | _ => false) do
    throw <| IO.userError "base identity cap hidden by library availability"
  let mismatch ← GateBase.select source base.toKernelEnv 12000000 true
  unless (match mismatch with | .error (.reject _) => true | _ => false) do
    throw <| IO.userError "base payload mismatch hidden by library availability"
  IO.println "PASS base identity cap and payload before library availability"
