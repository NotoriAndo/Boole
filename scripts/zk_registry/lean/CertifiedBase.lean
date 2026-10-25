/- Payload identity for a provisioned, independently checked environment. -/
import Det3Fuel
import Comparator.Compare
open Lean

namespace GateBase

structure ExprPair where
  left : Expr
  right : Expr

instance : BEq ExprPair where
  beq a b := a.left == b.left && a.right == b.right

instance : Hashable ExprPair where
  hash p := mixHash (hash p.left) (hash p.right)

structure IdentityState where
  seen : Std.HashSet ExprPair := {}
  pairs : Nat := 0
  maximum : Nat := 12000000

abbrev M := ExceptT GateFuel.Replay.Failure (StateRefT IdentityState IO)

/-- Compare the alpha-v1 expression fields with an explicit work list. -/
def expression (first second : Expr) : M Bool := do
  let mut todo := #[(first, second)]
  while !todo.isEmpty do
    let (a, b) := todo.back!
    todo := todo.pop
    let a := a.consumeMData
    let b := b.consumeMData
    if a == b then continue
    let key := { left := a, right := b : ExprPair }
    if (← get).seen.contains key then continue
    if (← get).pairs >= (← get).maximum then throw (.cap "base-identity")
    modify fun s => { s with pairs := s.pairs + 1, seen := s.seen.insert key }
    match a, b with
    | .bvar i, .bvar j => if i != j then return false
    | .fvar i, .fvar j => if i != j then return false
    | .mvar i, .mvar j => if i != j then return false
    | .sort u, .sort v => if u != v then return false
    | .const n us, .const m vs => if n != m || us != vs then return false
    | .lit x, .lit y => if x != y then return false
    | .app f x, .app g y => todo := todo.push (f, g) |>.push (x, y)
    | .lam _ d e _, .lam _ d' e' _
    | .forallE _ d e _, .forallE _ d' e' _ => todo := todo.push (d, d') |>.push (e, e')
    | .letE _ t v e _, .letE _ t' v' e' _ =>
      todo := todo.push (t, t') |>.push (v, v') |>.push (e, e')
    | .proj n i e, .proj m j e' =>
      if n != m || i != j then return false
      todo := todo.push (e, e')
    | _, _ => return false
  return true

/-- Preserve all declaration metadata; only alpha-v1 expression fields vary. -/
def declaration (a b : ConstantInfo) : M Bool := do
  if !(← expression a.type b.type) then return false
  match a, b with
  | .axiomInfo x, .axiomInfo y => return (.axiomInfo { x with type := y.type } : ConstantInfo) == b
  | .defnInfo x, .defnInfo y =>
    return (← expression x.value y.value) &&
      (.defnInfo { x with type := y.type, value := y.value } : ConstantInfo) == b
  | .thmInfo x, .thmInfo y =>
    return (← expression x.value y.value) &&
      (.thmInfo { x with type := y.type, value := y.value } : ConstantInfo) == b
  | .opaqueInfo x, .opaqueInfo y =>
    return (← expression x.value y.value) &&
      (.opaqueInfo { x with type := y.type, value := y.value } : ConstantInfo) == b
  | .inductInfo x, .inductInfo y =>
    return (.inductInfo { x with type := y.type } : ConstantInfo) == b
  | .ctorInfo x, .ctorInfo y => return (.ctorInfo { x with type := y.type } : ConstantInfo) == b
  | .quotInfo x, .quotInfo y => return (.quotInfo { x with type := y.type } : ConstantInfo) == b
  | .recInfo x, .recInfo y =>
    if x.rules.length != y.rules.length then return false
    for (left, right) in x.rules.zip y.rules do
      if left.ctor != right.ctor || left.nfields != right.nfields ||
          !(← expression left.rhs right.rhs) then return false
    return (.recInfo { x with type := y.type, rules := y.rules } : ConstantInfo) == b
  | _, _ => return false

def imported (path : System.FilePath) : IO Environment := do
  let name := `EENV5.CertifiedState
  let artifacts : NameMap ImportArtifacts := ({} : NameMap ImportArtifacts).insert name
    (.ofArrays #[#[path], #[]])
  importModules #[{ module := name }] {} 0 (loadExts := false) (arts := artifacts)

/-- Scope comes from the trusted module CONST record, never exported payloads. -/
def moduleOwned (source : Std.HashMap Name ConstantInfo)
    (base : Kernel.Environment) (declared : Array String) : Except GateFuel.Replay.Failure (Std.HashMap Name ConstantInfo) := do
  let names : Std.HashSet String := declared.foldl (fun set name => set.insert name) {}
  let mut own := {}
  for (name, info) in source.toList do
    if (base.find? name).isNone && names.contains name.toString then
      match info with
      | .defnInfo _ | .thmInfo _ =>
        if info.isUnsafe || info.isPartial then throw (.reject "declaration-profile")
      | _ => throw (.reject "declaration-profile")
      own := own.insert name info
  return own

/-- Missing certification is an availability outcome after own INVALID checks. -/
def hasUncertifiedLibrary (source : Std.HashMap Name ConstantInfo)
    (base : Kernel.Environment) (declared : Array String) : Bool := Id.run do
  let names : Std.HashSet String := declared.foldl (fun set name => set.insert name) {}
  return source.toList.any fun (name, _) =>
    (base.find? name).isNone && !names.contains name.toString

def select (source : Std.HashMap Name ConstantInfo) (base : Kernel.Environment)
    (maximum : Nat := 12000000) (requireOwnShape : Bool := true)
    (candidateDeclarations : Option (Array String) := none) : IO (Except GateFuel.Replay.Failure (Std.HashMap Name ConstantInfo)) := do
  let action : M (Std.HashMap Name ConstantInfo) := do
    -- An independently provable own profile violation takes precedence.
    if requireOwnShape then
      if let .error failure := moduleOwned source base (candidateDeclarations.getD #[]) then
        throw failure
    let uncertified := requireOwnShape &&
      hasUncertifiedLibrary source base (candidateDeclarations.getD #[])
    let mut own := {}
    for (name, info) in source.toList do
      if let some previous := base.find? name then
        if !(← declaration info previous) then throw (.reject "base-payload-mismatch")
      else
        if requireOwnShape && !uncertified then
          match info with
          | .defnInfo _ | .thmInfo _ =>
            if info.isUnsafe || info.isPartial then throw (.reject "declaration-profile")
          | _ => throw (.reject "declaration-profile")
        own := own.insert name info
    -- Identity checks inspect supplied bytes only; no uncertified kernel work.
    if uncertified then throw (.unavailable "allowed-library-outside-base")
    return own
  let (result, _) ← (action.run).run { maximum }
  return result

def equivalent (a b : ConstantInfo) : IO (Except GateFuel.Replay.Failure Bool) := do
  let (result, _) ← (declaration a b).run.run {}
  return result

def nameParts (name : Name) : Array String := Id.run do
  let mut remaining := name
  let mut parts := #[]
  while !remaining.isAnonymous do
    match remaining with
    | .anonymous => break
    | .str parent string => parts := parts.push ("s" ++ string); remaining := parent
    | .num parent number => parts := parts.push ("n" ++ toString number); remaining := parent
  return parts.reverse

end GateBase
