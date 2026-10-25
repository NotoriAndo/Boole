/-
Copyright (c) 2023 Kim Morrison. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
Authors: Kim Morrison

Budgeted adaptation of Lean/Replay.lean. The traversal and inductive validation
follow the pinned upstream replay; exit classes retain Kernel.Exception values.
-/
import Lean

namespace GateFuel
open Lean

namespace Replay

inductive Failure where
  | cap (reason : String)
  | reject (reason : String)
  | unavailable (reason : String)
  deriving Repr

structure Limits where
  max_heartbeats : Nat := 262144
  max_rec_depth : Nat := 4096
  aggregate_raw : Nat := 262144000000000
  declaration_charge : Nat := 64
  literal_bytes : Nat := 4096
  deriving FromJson, ToJson, Repr

structure Observation where
  names : List Name
  checkpoints : Nat
  depth : Nat

structure Context where
  observe : Observation → IO Unit
  limits : Limits
  newConstants : Std.HashMap Name ConstantInfo

structure State where
  env : Kernel.Environment
  debit : Nat := 0
  declarations : Nat := 0
  peakDepth : Nat := 0
  remaining : NameSet := {}
  pending : NameSet := {}
  postponedConstructors : NameSet := {}
  postponedRecursors : NameSet := {}

abbrev M := ReaderT Context (ExceptT Failure (StateRefT State IO))

/-- Check if a `Name` still needs processing. If so, move it from `remaining` to `pending`. -/
def isTodo (name : Name) : M Bool := do
  let r := (← get).remaining
  if r.contains name then
    modify fun s => { s with remaining := s.remaining.erase name, pending := s.pending.insert name }
    return true
  else
    return false

@[extern "gate_kernel_budget_hit"] opaque kernelBudgetHit : BaseIO Nat

/-- A physical stack exception is unavailable unless the logical guard actually fired. -/
def throwKernelException (ex : Kernel.Exception) : M Unit := do
  let hit ← kernelBudgetHit
  match ex with
  | .deterministicTimeout =>
    if hit % 2 == 1 then throw (.cap "heartbeats") else throw (.unavailable "kernel-interruption")
  | .deepRecursion =>
    if hit / 2 % 2 == 1 then throw (.cap "depth") else throw (.unavailable "kernel-stack")
  | .excessiveMemory | .interrupted => throw (.unavailable "kernel-resource")
  | _ => throw (.reject "kernel-rejection")

@[extern "gate_kernel_reset"] opaque resetKernel : BaseIO Unit
@[extern "gate_kernel_checkpoints"] opaque kernelCheckpoints : BaseIO Nat
@[extern "gate_kernel_depth"] opaque kernelDepth : BaseIO Nat
@[extern "gate_kernel_stack_capacity"] opaque stackCapacity : BaseIO Nat
@[extern "gate_kernel_peak_stack"] opaque peakStack : BaseIO Nat

/-- Debit all checked declarations, including generated constructor and recursor checks. -/
def addDecl (d : Declaration) : M Unit := do
  let limits := (← read).limits
  let spent := (← get).debit
  if spent >= limits.aggregate_raw then throw (.cap "aggregate")
  let allowance := min (limits.max_heartbeats * 1000) (limits.aggregate_raw - spent)
  if allowance == 0 then throw (.cap "heartbeats")
  let threshold := allowance
  if threshold >= USize.size then throw (.unavailable "counter-overflow")
  resetKernel
  let result := (← get).env.addDeclCore threshold.toUSize limits.max_rec_depth.toUSize d (cancelTk? := none)
  let forced ← IO.mkRef result
  let result ← forced.get
  let used ← kernelCheckpoints
  let depth ← kernelDepth
  modify fun s => {s with debit := s.debit + used, declarations := s.declarations + 1, peakDepth := max s.peakDepth depth}
  (← read).observe { names := d.getTopLevelNames, checkpoints := used, depth }
  if used > allowance || (← get).debit > limits.aggregate_raw then throw (.cap "aggregate")
  match result with
  | .ok env => modify fun s => { s with env := env }
  | .error ex => throwKernelException ex

mutual
/--
Check if a `Name` still needs to be processed (i.e. is in `remaining`).

If so, recursively replay any constants it refers to,
to ensure we add declarations in the right order.

The construct the `Declaration` from its stored `ConstantInfo`,
and add it to the environment.
-/
partial def replayConstant (name : Name) : M Unit := do
  if ← isTodo name then
    let some ci := (← read).newConstants[name]? | unreachable!
    replayConstants ci.getUsedConstantsAsSet
    -- Check that this name is still pending: a mutual block may have taken care of it.
    if (← get).pending.contains name then
      match ci with
      | .defnInfo   info =>
        addDecl (Declaration.defnDecl   info)
      | .thmInfo    info =>
        -- Ignore duplicate theorems. This code is identical to that in `finalizeImport` before it
        -- added extended duplicates support for the module system, which is not relevant for us
        -- here as we always load all .olean information. We need this case *because* of the module
        -- system -- as we have more data loaded than it, we might encounter duplicate private
        -- theorems where elaboration under the module system would have only one of them in scope.
        if let some (.thmInfo info') := (← get).env.find? ci.name then
          if info.name == info'.name &&
            info.type == info'.type &&
            info.levelParams == info'.levelParams &&
            info.all == info'.all
          then
            return
        addDecl (Declaration.thmDecl    info)
      | .axiomInfo  info =>
        addDecl (Declaration.axiomDecl  info)
      | .opaqueInfo info =>
        addDecl (Declaration.opaqueDecl info)
      | .inductInfo info =>
        let lparams := info.levelParams
        let nparams := info.numParams
        let all ← info.all.mapM fun n => do pure <| ((← read).newConstants[n]!)
        for o in all do
          modify fun s =>
            { s with remaining := s.remaining.erase o.name, pending := s.pending.erase o.name }
        let ctorInfo ← all.mapM fun ci => do
          pure (ci, ← ci.inductiveVal!.ctors.mapM fun n => do
            pure ((← read).newConstants[n]!))
        -- Make sure we are really finished with the constructors.
        for (_, ctors) in ctorInfo do
          for ctor in ctors do
            replayConstants ctor.getUsedConstantsAsSet
        let types : List InductiveType := ctorInfo.map fun ⟨ci, ctors⟩ =>
          { name := ci.name
            type := ci.type
            ctors := ctors.map fun ci => { name := ci.name, type := ci.type } }
        addDecl (Declaration.inductDecl lparams nparams types false)
      -- We postpone checking constructors,
      -- and at the end make sure they are identical
      -- to the constructors generated when we replay the inductives.
      | .ctorInfo info =>
        modify fun s => { s with postponedConstructors := s.postponedConstructors.insert info.name }
      -- Similarly we postpone checking recursors.
      | .recInfo info =>
        modify fun s => { s with postponedRecursors := s.postponedRecursors.insert info.name }
      | .quotInfo _ =>
        -- `Quot.lift` and `Quot.ind` have types that reference `Eq`,
        -- so we need to ensure `Eq` is replayed before adding the quotient declaration.
        replayConstant `Eq
        addDecl (Declaration.quotDecl)
      modify fun s => { s with pending := s.pending.erase name }

/-- Replay a set of constants one at a time. -/
partial def replayConstants (names : NameSet) : M Unit := do
  for n in names do replayConstant n

end

/--
Check that all postponed constructors are identical to those generated
when we replayed the inductives.
-/
def checkPostponedConstructors : M Unit := do
  for ctor in (← get).postponedConstructors do
    match (← get).env.find? ctor, (← read).newConstants[ctor]? with
    | some (.ctorInfo info), some (.ctorInfo info') =>
      if ! (info == info') then throw <| .reject "constructor-mismatch"
    | _, _ => throw <| .reject "constructor-missing"

/--
Check that all postponed recursors are identical to those generated
when we replayed the inductives.
-/
def checkPostponedRecursors : M Unit := do
  for ctor in (← get).postponedRecursors do
    match (← get).env.find? ctor, (← read).newConstants[ctor]? with
    | some (.recInfo info), some (.recInfo info') =>
      if ! (info == info') then throw <| .reject "recursor-mismatch"
    | _, _ => throw <| .reject "recursor-missing"

end Replay

open Replay

/-- Pointer memoization preserves every input literal and avoids structural recursion. -/
unsafe def checkInputsImpl (constants : Std.HashMap Name ConstantInfo)
    (literalBytes maxDepth : Nat) : Except Failure Unit := do
  let mut todo : Array (Expr × Nat × Bool) := #[]
  for (_, ci) in constants.toList do
    todo := todo.push (ci.type, 1, false)
    if let some value := ci.value? (allowOpaque := true) then todo := todo.push (value, 1, false)
  let mut heights : PtrMap Expr Nat := mkPtrMap
  while !todo.isEmpty do
    let (e, pathDepth, closing) := todo.back!
    todo := todo.pop
    if let some height := heights.find? e then
      if pathDepth + height - 1 > maxDepth then throw (.cap "syntax-depth")
      continue
    if pathDepth > maxDepth then throw (.cap "syntax-depth")
    let children : Array Expr := match e with
      | .app f a | .forallE _ f a _ | .lam _ f a _ => #[f, a]
      | .letE _ t v b _ => #[t, v, b]
      | .mdata _ e | .proj _ _ e => #[e]
      | _ => #[]
    if closing then
      let height := 1 + children.foldl (fun h child => max h ((heights.find? child).getD 0)) 0
      if pathDepth + height - 1 > maxDepth then throw (.cap "syntax-depth")
      heights := heights.insert e height
    else
      match e with
      | .lit (.natVal n) => if n.log2 / 8 + 1 > literalBytes then throw (.cap "literal")
      | .lit (.strVal v) => if v.utf8ByteSize > literalBytes then throw (.cap "literal")
      | _ => pure ()
      todo := todo.push (e, pathDepth, true)
      for child in children do todo := todo.push (child, pathDepth + 1, false)

@[implemented_by checkInputsImpl]
opaque checkInputs (constants : Std.HashMap Name ConstantInfo)
    (literalBytes maxDepth : Nat) : Except Failure Unit := .ok ()

structure Result where
  outcome : Except Failure Kernel.Environment
  debit : Nat := 0
  checked : Nat := 0
  calls : Nat := 0
  peakDepth : Nat := 0

def replay (newConstants : Std.HashMap Name ConstantInfo) (env : Kernel.Environment)
    (limits : Limits) (observe : Observation → IO Unit := fun _ => pure ())
    (checkedElsewhere : Nat := 0) : IO Result := do
  if limits.max_rec_depth == 0 || limits.max_heartbeats == 0 || limits.literal_bytes == 0 then
    return { outcome := .error (.cap "zero-limit") }
  if let .error e := checkInputs newConstants limits.literal_bytes (limits.max_rec_depth * 16) then return { outcome := .error e }
  let mut remaining : NameSet := ∅
  for (n, ci) in newConstants.toList do
    if !ci.isUnsafe && !ci.isPartial then remaining := remaining.insert n
  let count := remaining.size + checkedElsewhere
  let debit := count * limits.declaration_charge
  if debit > limits.aggregate_raw then return { outcome := .error (.cap "aggregate"), debit, checked := count }
  let (outcome, s) ← StateRefT'.run (s := { env, remaining, debit }) do
    ExceptT.run <| ReaderT.run (r := { newConstants, limits, observe }) do
      for n in remaining do replayConstant n
      checkPostponedConstructors
      checkPostponedRecursors
  return {
    outcome := outcome.map (fun _ => s.env)
    debit := s.debit
    checked := count
    calls := s.declarations
    peakDepth := s.peakDepth
  }

end GateFuel
