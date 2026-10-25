/- Generated-input unit controls for the canonical Lean replay, linked only in the test executable. -/
import Det3Fuel
import CertifiedBase
open Lean GateFuel GateFuel.Replay

@[extern "gate_test_heartbeat_limit"] opaque heartbeatLimit : BaseIO Unit
@[extern "gate_test_depth_limit"] opaque depthLimit : BaseIO Unit

def require (condition : Bool) (label : String) : IO Unit :=
  unless condition do throw <| IO.userError label

def axiomOf (name : Name) (type : Expr) : ConstantInfo :=
  .axiomInfo { name, levelParams := [], type, isUnsafe := false }

def constantsOf (type : Expr) : Std.HashMap Name ConstantInfo :=
  ({} : Std.HashMap Name ConstantInfo).insert `Generated (axiomOf `Generated type)

def classify (ex : Kernel.Exception) : IO String := do
  let env ← mkEmptyEnvironment
  let (result, _) ← StateRefT'.run (s := { env := env.toKernelEnv : State }) do
    ExceptT.run <| ReaderT.run (r := { newConstants := {}, limits := {}, observe := fun _ => pure () : Context }) do
      throwKernelException ex
  return match result with
    | .error (.cap _) => "cap"
    | .error (.unavailable _) => "unavailable"
    | .error (.reject _) => "reject"
    | .ok _ => "accept"

def tests : IO UInt32 := do
  resetKernel
  require ((← classify .deterministicTimeout) == "unavailable") "physical timeout without logical bit"
  require ((← classify .deepRecursion) == "unavailable") "physical stack without logical bit"
  heartbeatLimit
  require ((← kernelBudgetHit) == 1) "logical heartbeat bit"
  require ((← classify .deterministicTimeout) == "cap") "logical heartbeat mapping"
  resetKernel
  require ((← kernelBudgetHit) == 0) "reset heartbeat bit"
  depthLimit
  require ((← kernelBudgetHit) == 2) "logical depth bit"
  require ((← classify .deepRecursion) == "cap") "logical depth mapping"
  resetKernel
  require ((← kernelBudgetHit) == 0) "reset depth bit"
  let env ← mkEmptyEnvironment
  let charged ← replay {} env.toKernelEnv {} (checkedElsewhere := 3)
  require (charged.outcome.isOk && charged.debit == 192 && charged.checked == 3 && charged.calls == 0) "three external declarations charged 64 each"
  let tight ← replay {} env.toKernelEnv { aggregate_raw := 191 } (checkedElsewhere := 3)
  require (match tight.outcome with | .error (.cap _) => true | _ => false) "aggregate includes declaration precharge"
  let observations ← IO.mkRef (0 : Nat)
  let checked ← replay (constantsOf (.sort .zero)) env.toKernelEnv {} (fun o => observations.modify (· + o.checkpoints))
  require (checked.outcome.isOk && checked.checked == 1 && checked.calls == 1) "actual generated declaration replay"
  require (checked.debit == 64 + (← observations.get)) "aggregate equals declaration charge plus checkpoints"
  let mut shared := Expr.sort .zero
  for _ in [:32] do shared := .app shared shared
  require ((checkInputs (constantsOf shared) 16 33).isOk) "shared DAG is memoized"
  require (!(checkInputs (constantsOf shared) 16 32).isOk) "exact generated depth boundary"
  require ((checkInputs (constantsOf (.lit (.natVal (2 ^ 127)))) 16 1).isOk) "literal at boundary"
  require (!(checkInputs (constantsOf (.lit (.natVal (2 ^ 128)))) 16 1).isOk) "literal above boundary"
  let forbidden : Array ConstantInfo := #[
    axiomOf `Generated (.sort .zero),
    .opaqueInfo { (default : OpaqueVal) with name := `Generated },
    .inductInfo { (default : InductiveVal) with name := `Generated }]
  for info in forbidden do
    let source := ({} : Std.HashMap Name ConstantInfo).insert `Generated info
    let rejected ← GateBase.select source env.toKernelEnv 12000000 true (some #["Generated"])
    require (match rejected with | .error (.reject _) => true | _ => false) "genuinely own forbidden profile stays rejected"
    let mixed := source.insert `UncertifiedLibrary (axiomOf `UncertifiedLibrary (.sort .zero))
    let ownFirst ← GateBase.select mixed env.toKernelEnv 12000000 true (some #["Generated"])
    require (match ownFirst with | .error (.reject _) => true | _ => false) "unsupported library cannot hide own INVALID"
    let library ← GateBase.select source env.toKernelEnv 0 true
    require (match library with | .error (.unavailable _) => true | _ => false) "unknown library profile unavailable before debit"
  let libraryDef : ConstantInfo := .defnInfo { (default : DefinitionVal) with name := `Generated }
  let unknownDef ← GateBase.select (({} : Std.HashMap Name ConstantInfo).insert `Generated libraryDef) env.toKernelEnv 0 true
  require (match unknownDef with | .error (.unavailable _) => true | _ => false) "unknown library definition unavailable before debit"
  IO.println "PASS logical-limit mapping; reset; aggregate; 64 charge; DAG memo; generated cap boundaries"
  return 0

def main : IO UInt32 := do
  let task ← IO.asTask tests Task.Priority.dedicated
  match ← IO.wait task with
  | .ok result => return result
  | .error error => IO.eprintln error; return 1
