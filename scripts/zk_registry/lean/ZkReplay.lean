/-
Kernel-level post-check of a compiled module (run with `lean --run`), used by
`scripts/zk_registry/check.py` and by the statement-elaboration gate.

Derived from the ZK-PILOT-DIFFICULTY-P2 checker's `P2Check.lean`, including its fix F1: the body runs on
a dedicated task thread, so the kernel replay gets the thread stack size set by `--tstack` (the stack the
module was compiled with) instead of the default main-thread stack.

Change from P2Check: the TYPE record is printed after `canonical`, which replaces every binder name by
`x` and drops metadata. Binder names are irrelevant to the proposition, but the elaborator gives
anonymous binders (arrows, instance binders) hygienic names that embed the module name and a macro-scope
counter, so the raw string of an unchanged statement differs between `Statement` and `ProofMod`, or when
helper declarations are inserted above the theorem. Binder kinds (explicit / implicit / instance) are kept.

Usage: LEAN_SYSROOT=<toolchain> LEAN_PATH=<...> lean [--tstack=N] --run ZkReplay.lean <module.olean> <theorem> <out>

The module's .olean is read as data (its syntax extensions are never activated here), its imports are
loaded, and every constant it declares is re-sent to the kernel (`Lean.Environment.replay`, the
lean4checker method). On the replayed environment it writes, one record per line:
  CONST  <name> <kind> <isUnsafe> <isPartial>      for every constant declared by the module
  TARGET <kind> <levelParams>                      for the theorem (or TARGET-MISSING)
  AXIOM  <name>                                    for every axiom the theorem depends on
  TYPE   <Expr.dbgToString of the canonical type>  (compared byte-for-byte with the reference)
  REPLAY ok | REPLAY-FAIL <message>
-/
import Lean
open Lean

def kindOf : ConstantInfo → String
  | .axiomInfo _ => "axiom" | .defnInfo _ => "def" | .thmInfo _ => "theorem" | .opaqueInfo _ => "opaque"
  | .quotInfo _ => "quot" | .inductInfo _ => "inductive" | .ctorInfo _ => "ctor" | .recInfo _ => "rec"

def oneLine (s : String) : String := s.replace "\n" "\\n"

/-- The expression with every binder named `x` and metadata removed (alpha-equivalence representative). -/
partial def canonical : Expr → Expr
  | .forallE _ t b bi => .forallE `x (canonical t) (canonical b) bi
  | .lam _ t b bi => .lam `x (canonical t) (canonical b) bi
  | .letE _ t v b nd => .letE `x (canonical t) (canonical v) (canonical b) nd
  | .app f a => .app (canonical f) (canonical a)
  | .mdata _ e => canonical e
  | .proj s i e => .proj s i (canonical e)
  | e => e

/-- Axioms a constant depends on, over a kernel environment (same traversal as `Lean.collectAxioms`). -/
partial def collectAx (env : Kernel.Environment) (c : Name) : StateM (NameSet × Array Name) Unit := do
  if (← get).1.contains c then return
  modify fun (s, a) => (s.insert c, a)
  let goE (e : Expr) : StateM (NameSet × Array Name) Unit := e.getUsedConstants.forM (collectAx env)
  match env.find? c with
  | some (.axiomInfo _) => modify fun (s, a) => (s, a.push c)
  | some (.defnInfo v) => goE v.type *> goE v.value
  | some (.thmInfo v) => goE v.type *> goE v.value
  | some (.opaqueInfo v) => goE v.type *> goE v.value
  | some (.quotInfo _) => pure ()
  | some (.ctorInfo v) => goE v.type
  | some (.recInfo v) => goE v.type
  | some (.inductInfo v) => goE v.type *> v.ctors.forM (collectAx env)
  | none => modify fun (s, a) => (s, a.push (`MISSING ++ c))

def mainCore (args : List String) : IO UInt32 := do
  let [olean, target, out] := args
    | IO.eprintln "usage: ZkReplay <module.olean> <theorem-fqn> <out-file>"; return 2
  -- sysroot only from LEAN_SYSROOT (never `lean --print-prefix` via PATH, which could reach an elan proxy)
  let some sysroot ← IO.getEnv "LEAN_SYSROOT"
    | IO.eprintln "LEAN_SYSROOT is not set"; return 2
  initSearchPath sysroot
  let tgt := target.toName
  let (mod, _region) ← readModuleData olean
  let mut lines : Array String := #[]
  let mut newConsts : Std.HashMap Name ConstantInfo := {}
  for ci in mod.constants do
    newConsts := newConsts.insert ci.name ci
    lines := lines.push s!"CONST\t{ci.name}\t{kindOf ci}\t{ci.isUnsafe}\t{ci.isPartial}"
  let env0 ← importModules mod.imports {} (trustLevel := 0)
  let env ← try
      let e ← env0.replay newConsts
      lines := lines.push "REPLAY\tok"
      pure e
    catch ex =>
      lines := lines.push s!"REPLAY-FAIL\t{oneLine (toString ex)}"
      IO.FS.writeFile out (String.intercalate "\n" lines.toList ++ "\n")
      return 1
  let kenv := env.toKernelEnv
  match kenv.find? tgt with
  | none => lines := lines.push "TARGET-MISSING"
  | some ci =>
    lines := lines.push s!"TARGET\t{kindOf ci}\t{ci.levelParams}"
    let ((), (_, axs)) := (collectAx kenv tgt).run ({}, #[])
    for a in axs do
      lines := lines.push s!"AXIOM\t{a}"
    lines := lines.push s!"TYPE\t{oneLine (canonical ci.type).dbgToString}"
  IO.FS.writeFile out (String.intercalate "\n" lines.toList ++ "\n")
  return 0

/-- Run the check on a dedicated thread so the kernel replay gets the thread stack size set by `--tstack`
(fix F1), instead of the default main-thread stack. -/
def main (args : List String) : IO UInt32 := do
  let t ← IO.asTask (mainCore args) Task.Priority.dedicated
  match ← IO.wait t with
  | .ok r => return r
  | .error e => throw e
