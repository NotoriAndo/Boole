/-
Counter identity probe for the pinned native replay driver. Inputs are the
content-bound positive corpus; timestamps and memory observations are excluded.
-/
import Det3Fuel
import CertifiedBase
import Export.Parse
open Lean

def counterCore (args : List String) : IO UInt32 := do
  let input :: vectors :: limitsJson :: rest := args | return 70
  let started ← IO.monoNanosNow
  if rest.length > 1 then return 70
  let limits : GateFuel.Replay.Limits ← IO.ofExcept <| fromJson? <| ← IO.ofExcept <| Json.parse limitsJson
  if ← System.FilePath.pathExists vectors then return 70
  let stream ← IO.FS.Handle.mk vectors .write
  let source ← IO.FS.Handle.mk input .read
  let parsed ← Export.parseStream (IO.FS.Stream.ofHandle source)
  let parsedAt ← IO.monoNanosNow
  let quotients := [`Quot.mk, `Quot.lift, `Quot.ind]
  let empty ← mkEmptyEnvironment
  let mut constants := quotients.foldl (init := parsed.constMap) (·.erase ·)
  let mut base := empty.toKernelEnv
  let mut extra := quotients.foldl (fun n name => n + if parsed.constMap.contains name then 1 else 0) 0
  if let [path] := rest then
    if let .error _ := GateFuel.checkInputs parsed.constMap limits.literal_bytes (limits.max_rec_depth * 16) then return 3
    initSearchPath (← findSysroot)
    let imported ← GateBase.imported path
    match ← GateBase.select parsed.constMap imported.toKernelEnv 12000000 true
        (some (parsed.constMap.toList.map (fun (name, _) => name.toString)).toArray) with
    | .ok own => constants := own; base := imported.toKernelEnv; extra := 0
    | .error (.cap _) => return 3
    | .error (.reject _) => return 2
    | .error (.unavailable _) => return 70
  let preparedAt ← IO.monoNanosNow
  let result ← GateFuel.replay constants base limits (fun observation => do
    let names := observation.names.toArray.map (·.toString) |>.qsort (· < ·)
    stream.putStrLn <| (Json.mkObj [
      ("names", toJson names),
      ("name_parts", toJson (observation.names.toArray.map GateBase.nameParts |>.qsort (fun a b => a.toList < b.toList))),
      ("checkpoints", toJson observation.checkpoints),
      ("depth", toJson observation.depth)]).compress) extra
  let checkedAt ← IO.monoNanosNow
  stream.flush
  if let some path ← IO.getEnv "COUNTER_TIMING_FILE" then
    IO.FS.writeFile path <| (Json.mkObj [
      ("parse_ns", toJson (parsedAt - started)),
      ("prepare_ns", toJson (preparedAt - parsedAt)),
      ("check_ns", toJson (checkedAt - preparedAt))]).compress
  let mut code : UInt32 := match result.outcome with
    | .ok _ => 0
    | .error (.cap _) => 3
    | .error (.reject _) => 2
    | .error (.unavailable _) => 70
  -- Match the production Comparator's mandatory quotient post-check. The
  -- three generated quotient declarations spend their fixed charge above.
  if let .ok kernelEnv := result.outcome then
    for name in `Quot :: quotients do
      if let some info := parsed.constMap[name]? then
        let some checked := kernelEnv.find? name | code := 2; break
        if rest.isEmpty then
          if info != checked then code := 2; break
        else
          let .ok true ← GateBase.equivalent info checked | code := 2; break
  IO.println <| (Json.mkObj [("version", toJson (1 : Nat)), ("code", toJson code.toNat),
    ("debit", toJson result.debit), ("checked", toJson result.checked),
    ("calls", toJson result.calls), ("depth", toJson result.peakDepth)]).compress
  return code

def main (args : List String) : IO UInt32 := do
  let task ← IO.asTask (counterCore args) Task.Priority.dedicated
  match ← IO.wait task with
  | .ok code => return code
  | .error _ => return 70
