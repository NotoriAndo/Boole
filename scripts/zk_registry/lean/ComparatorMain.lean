/-
Copyright (c) 2025 Lean FRO, LLC. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE.
Authors: Henrik Böving
-/
import Lean
import Det3Fuel
import CertifiedBase
import Comparator
import Export.Parse

namespace Comparator

/-- Own semantic status channel, never kernel stdout. Inner kernels have no write grant here. -/
def recordStatus (fields : List (String × Lean.Json)) : IO Unit := do
  if let some path ← IO.getEnv "COMPARATOR_STATUS_FILE" then
    IO.FS.withFile path .append fun handle => do
      handle.putStrLn <| Lean.Json.compress <| Lean.Json.mkObj
        (("version", Lean.toJson (1 : Nat)) :: fields)

structure NanodaLimits where
  counter_version : Nat := 1
  steps_per_declaration : Nat := 33554432
  depth_per_declaration : Nat := 32768
  work_per_declaration : Nat := 34359738368
  aggregate_work : Nat := 34359738368000000
  declaration_charge : Nat := 64
  literal_bytes : Nat := 4096
  worker_stack_bytes : Nat := 67108864
  deriving Lean.FromJson, Lean.ToJson, Repr

structure NanodaStatus where
  version : Nat
  code : Nat
  workers : Nat
  worker_stack_bytes : Array Nat
  checked_declarations : Nat
  selected_declarations : Nat := 0
  aggregate_work : Nat
  aggregate_steps : Nat
  maximum_depth : Nat
  maximum_composed_depth : Nat
  peak_stack_bytes : Nat
  deriving Lean.FromJson, Lean.ToJson

structure Context where
  projectDir : System.FilePath
  challengeModule : Lean.Name
  solutionModule : Lean.Name
  theoremNames : Array Lean.Name
  definitionNames : Array Lean.Name
  legalAxioms : Array Lean.Name
  leanPrefix : System.FilePath
  gitLocation : System.FilePath
  whichLandrun : String
  whichLean4Export : String
  externalKernels : (Std.TreeMap String (Array String))
  solutionExportFile : Option System.FilePath := none
  maxExportedConstants : Nat := 1000000
  leanKernel : GateFuel.Replay.Limits := {}
  nanodaKernel : NanodaLimits := {}
  certifiedBasePath : Option System.FilePath := none
  candidateDeclarations : Option (Array String) := none

abbrev M := ReaderT Context IO

structure LandrunArgs where
  cmd : String
  args : Array String
  envPass : Array String
  envOverride : Array (String × Option String) := #[]
  readablePaths : Array System.FilePath
  writablePaths : Array System.FilePath
  executablePaths : Array System.FilePath

@[inline]
def getExternalKernels : M (Std.TreeMap String (Array String)) := do return (← read).externalKernels

@[inline]
def getTheoremNames : M (Array Lean.Name) := do return (← read).theoremNames

@[inline]
def getDefinitionNames : M (Array Lean.Name) := do return (← read).definitionNames

@[inline]
def getProjectDir : M System.FilePath := do return (← read).projectDir

@[inline]
def getChallengeModule : M Lean.Name := do return (← read).challengeModule

@[inline]
def getSolutionModule : M Lean.Name := do return (← read).solutionModule

@[inline]
def getLegalAxioms : M (Array Lean.Name) := do return (← read).legalAxioms

@[inline]
def getLeanPrefix : M System.FilePath := do return (← read).leanPrefix

@[inline]
def getGitLocation : M System.FilePath := do return (← read).gitLocation

def queryGitLocation : IO System.FilePath := do
  let out ← IO.Process.run {
    cmd := "which",
    args := #["git"],
    stdout := .piped,
  }
  return out.trimAscii.toString

def queryLeanPrefix (projectDir : System.FilePath) : IO System.FilePath := do
  let out ← IO.Process.run {
    cmd := "lean",
    args := #["--print-prefix"],
    stdout := .piped,
    cwd := projectDir
  }
  return out.trimAscii.toString

def buildLandrunArgs (spawnArgs : LandrunArgs) : Array String :=
  let args := #["--best-effort", "--ro", "/", "--rw", "/dev", "-ldd", "-add-exec"]
  let args := spawnArgs.envPass.foldl (init := args) (fun acc env => acc ++ #["--env", env])
  let args := spawnArgs.readablePaths.foldl (init := args) (fun acc path => acc ++ #["--ro", path.toString])
  let args := spawnArgs.writablePaths.foldl (init := args) (fun acc path => acc ++ #["--rwx", path.toString])
  let args := spawnArgs.executablePaths.foldl (init := args) (fun acc path => acc ++ #["--rox", path.toString])
  args ++ #["--", spawnArgs.cmd] ++ spawnArgs.args

def runSandBoxedWithStdout (spawnArgs : LandrunArgs) : M String := do
  let args := buildLandrunArgs spawnArgs
  let { stdout, stderr, exitCode } ← IO.Process.output {
    cmd := (← read).whichLandrun,
    args,
    env := spawnArgs.envOverride
    cwd := (← getProjectDir)
  }
  IO.eprint stderr
  if exitCode != 0 then
    throw <| .userError s!"Child exited with {exitCode}"
  return stdout


def runSandBoxed (spawnArgs : LandrunArgs) : M Unit := do
  let args := buildLandrunArgs spawnArgs
  let proc ← IO.Process.spawn {
    cmd := (← read).whichLandrun,
    args,
    env := spawnArgs.envOverride
    cwd := (← getProjectDir)
  }
  let ret ← proc.wait
  if ret != 0 then
    throw <| .userError s!"Child exited with {ret}"

def safeLakeBuild (target : Lean.Name) : M Unit := do
  IO.println s!"Building {target}"
  let leanPrefix ← getLeanPrefix
  let projectDir ← getProjectDir
  let dotLakeDir := projectDir / ".lake"
  let gitLocation ← getGitLocation

  if !(← System.FilePath.pathExists dotLakeDir) then
    IO.FS.createDir dotLakeDir

  runSandBoxed {
    cmd := "lake",
    args := #["build", target.toString],
    envPass := #["PATH", "HOME", "LEAN_ABORT_ON_PANIC"]
    envOverride := #[("LEAN_ABORT_ON_PANIC", some "1")]
    readablePaths := #[projectDir]
    writablePaths := #[dotLakeDir]
    executablePaths := #[leanPrefix, gitLocation]
  }

def safeExport (module : Lean.Name) (decls : Array Lean.Name) : M String := do
  IO.println s!"Exporting {decls} from {module}"
  let baseArgs := #[module.toString, "--"]
  let args := decls.foldl (·.push <| ·.toString) baseArgs

  let leanPrefix ← getLeanPrefix
  let projectDir ← getProjectDir
  let dotLakeDir := projectDir / ".lake"
  runSandBoxedWithStdout {
    cmd := (← read).whichLean4Export
    args := args,
    envPass := #["PATH", "HOME", "LEAN_PATH", "LEAN_ABORT_ON_PANIC"]
    envOverride := #[("LEAN_ABORT_ON_PANIC", some "1")]
    readablePaths := #[projectDir, dotLakeDir]
    writablePaths := #[]
    executablePaths := #[leanPrefix]
  }

def runExternalKernel (kernelName : String) (kernelCommand : Array String)
    (solutionExport : String) (checkedNames : Option (Array (Array String)) := none) : M (Option String) := do
  IO.println s!"Running {kernelName} kernel on solution"
  -- just always put out a nanoda-like config file for now
  IO.FS.withTempFile fun nanodaStatusHandle nanodaStatusPath => do
  IO.FS.withTempFile fun configHandle configPath => do
  IO.FS.withTempFile fun solutionHandle solutionPath => do
    let legalAxioms ← getLegalAxioms
    let solutionPath ← match (← read).solutionExportFile with
      | some pinnedPath => do
        if (← IO.FS.readFile pinnedPath) != solutionExport then
          recordStatus [("stage", .str "error"), ("reason", .str "export-integrity")]
          throw <| .userError "export-integrity: solution export changed"
        pure pinnedPath
      | none => do
        solutionHandle.putStr solutionExport
        solutionHandle.flush
        pure solutionPath
    configHandle.putStr <| Lean.Json.compress <| Lean.Json.mkObj [
      ("use_stdin", false),
      ("export_file_path", solutionPath.toString),
      ("permitted_axioms", .arr <| legalAxioms.map (.str ∘ Lean.Name.toString)),
      ("unpermitted_axiom_hard_error", true),
      ("num_threads", 4),
      ("nat_extension", true),
      ("string_extension", true),
      ("gate_limits", Lean.toJson (← read).nanodaKernel),
      ("checked_declaration_names", Lean.toJson checkedNames),
    ]
    configHandle.flush

    let mut kernelArgs := kernelCommand[1...*].toArray
    if isNanodaKernel kernelName then
      kernelArgs := kernelArgs.push configPath.toString
    else
      kernelArgs := kernelArgs.push solutionPath.toString

    let spawnArgs := {
      cmd := kernelCommand[0]!,
      args := kernelArgs,
      envPass := #[s!"NANODA_STATUS_FILE={nanodaStatusPath}"]
      envOverride := #[("NANODA_STATUS_FILE", some nanodaStatusPath.toString)]
      readablePaths := #[configPath.toString, solutionPath.toString]
      writablePaths := #[nanodaStatusPath]
      executablePaths := #[]
    }
    let args := buildLandrunArgs spawnArgs

    try
      let proc ← IO.Process.spawn {
        cmd := (← read).whichLandrun,
        args,
        env := spawnArgs.envOverride
        cwd := (← getProjectDir)
      }

      let ret ← proc.wait
      if ret == 125 || ret >= 128 then
        return ← IO.Process.exit 70
      if isNanodaKernel kernelName then
        nanodaStatusHandle.flush
        let raw ← IO.FS.readFile nanodaStatusPath
        if raw.utf8ByteSize > 16384 then throw <| .userError "invalid native status"
        let status : NanodaStatus ← IO.ofExcept <| Lean.FromJson.fromJson? <| ← IO.ofExcept <| Lean.Json.parse raw
        if status.version != 1 || status.code != ret.toNat || !([0, 2, 3, 4, 70].contains status.code) then
          throw <| .userError "invalid native status"
        let nativeStack := (← read).nanodaKernel.worker_stack_bytes
        if ret == 0 && (status.workers != 4 || status.worker_stack_bytes.size != 4 ||
            status.worker_stack_bytes.any (· < nativeStack)) then
          throw <| .userError "invalid native workers"
        if let some names := checkedNames then
          if ret == 0 && (status.selected_declarations != names.size ||
              status.checked_declarations != names.size) then
            throw <| .userError "invalid native selection"
        recordStatus [("stage", .str "nanoda-fuel"), ("kernel", .str "nanoda"),
          ("debit", Lean.toJson status.aggregate_work), ("checked", Lean.toJson status.checked_declarations),
          ("calls", Lean.toJson status.aggregate_steps), ("depth", Lean.toJson status.maximum_depth),
          ("composed_depth", Lean.toJson status.maximum_composed_depth)]
        if ret == 3 || ret == 4 then
          recordStatus [("stage", .str "policy"), ("reason", .str (if ret == 3 then "resource-cap" else "tool-fault:replay"))]
          recordStatus [("stage", .str "final"), ("rc", Lean.toJson ret.toNat)]
          return ← IO.Process.exit ret.toUInt8
        if ret == 70 then
          recordStatus [("stage", .str "error"), ("reason", .str "kernel-unavailable")]
          recordStatus [("stage", .str "final"), ("rc", Lean.toJson (70 : Nat))]
          return ← IO.Process.exit 70
      else
        return ← IO.Process.exit 70
      if ret != 0 then
        recordStatus [("stage", .str kernelName), ("result", .str "reject")]
        IO.println s!"{kernelName} kernel rejected the solution"
        return some s!"{kernelName} exited with {ret}"
      else
        recordStatus [("stage", .str kernelName), ("result", .str "accept")]
        IO.println s!"{kernelName} kernel accepts the solution"
        return none
    catch e => do
      IO.println s!"Error while interacting with {kernelName} kernel"
      IO.eprintln e.toString
      return ← IO.Process.exit 70
where
  isNanodaKernel (kernelName : String) : Bool :=
    -- TODO: get rid of this heuristic
    kernelName.contains "noda"

def runBuiltinKernel (solution : Export.ExportedEnv) (cached : Option Lean.Environment := none)
    (own : Option (Std.HashMap Lean.Name Lean.ConstantInfo) := none) : M (Option String) := do
  IO.println "Running Lean default kernel on solution."
  let env ← Lean.mkEmptyEnvironment
  let mut kernelEnv := (cached.getD env).toKernelEnv
  let origConstMap := solution.constMap
  let inputConstMap := own.getD origConstMap
  -- Lean's kernel interprets just the addition of `Quot as adding all of these so adding them
  -- multiple times leads to errors.
  let quotTargets := [`Quot.mk, `Quot.lift, `Quot.ind]
  let kernelConstMap := quotTargets.foldl (init := inputConstMap) (·.erase ·)
  let extra := quotTargets.foldl (fun n name => n + if inputConstMap.contains name then 1 else 0) 0
  let result ← GateFuel.replay kernelConstMap kernelEnv (← read).leanKernel (fun _ => pure ()) extra
  recordStatus [("stage", .str "fuel"), ("kernel", .str "lean"),
    ("debit", Lean.toJson result.debit), ("checked", Lean.toJson result.checked),
    ("calls", Lean.toJson result.calls), ("depth", Lean.toJson result.peakDepth)]
  match result.outcome with
  | .ok env =>
    kernelEnv := env
    recordStatus [("stage", .str "lean"), ("result", .str "accept")]
  | .error (.cap _) =>
    recordStatus [("stage", .str "policy"), ("reason", .str "resource-cap")]
    recordStatus [("stage", .str "final"), ("rc", Lean.toJson (3 : Nat))]
    return ← IO.Process.exit 3
  | .error (.reject reason) =>
    recordStatus [("stage", .str "lean"), ("result", .str "reject")]
    return some reason
  | .error (.unavailable _) =>
    recordStatus [("stage", .str "error"), ("reason", .str "kernel-unavailable")]
    recordStatus [("stage", .str "final"), ("rc", Lean.toJson (70 : Nat))]
    return ← IO.Process.exit 70

  try
    let verifyTargets := `Quot :: quotTargets
    for quotTarget in verifyTargets do
      if let some info := origConstMap[quotTarget]? then
        let some info' := kernelEnv.find? quotTarget |
          throw <| .userError s!"Could not find quotient constant in final kernel env: {quotTarget}"
        if cached.isSome then
          let .ok true ← GateBase.equivalent info info' |
            throw <| .userError "Quotient constant mismatch"
        else if info != info' then
          throw <| .userError s!"Quotient constant mismatch on: {quotTarget}"
    recordStatus [("stage", .str "quotient"), ("result", .str "accept")]
    return none
  catch e =>
    recordStatus [("stage", .str "quotient"), ("result", .str "reject")]
    IO.println "Quotient post-check rejects the solution"
    return some e.toString

def primitiveTargets : M (Array Lean.Name) := do
  -- The challenge needs to have all the built-in constants of the kernel, as the
  -- kernel makes no guarantees when fed other definitions here.
  -- List from `git grep new_persistent_expr_const src/kernel/`
  return #[
    -- ``Nat.zero,
    -- ``Nat.succ,
    ``Nat.add,
    ``Nat.sub,
    ``Nat.mul,
    ``Nat.pow,
    ``Nat.gcd,
    ``Nat.div,
    ``Nat.mod,
    ``Nat.beq,
    ``Nat.ble,
    ``Nat.land,
    ``Nat.lor,
    ``Nat.xor,
    ``Nat.shiftLeft,
    ``Nat.shiftRight,
    ``String.ofList,
    ``Char.ofNat,
    ``List,
    ``eagerReduce,
    ``Nat,
    ``String,
    ``String.mk,
    ``Char,
    ``optParam,
    ``autoParam,
    ``semiOutParam,
    ``outParam
  ]

def builtinTargets : M (Array Lean.Name) := do
  let mut additional := #[]
  if (← getLegalAxioms).contains ``Quot.sound then
    additional := additional ++ #[``Quot, ``Quot.mk, ``Quot.lift, ``Quot.ind]
  return additional

def stringStream (s : String) : BaseIO IO.FS.Stream := do
  let ref ← IO.mkRef {
    data := s.toByteArray
  }
  return IO.FS.Stream.ofBuffer ref

def verifyMatch (challengeExport : String) (solutionExport : String) :
    M Unit := do
  let challenge ← Export.parseStream (← stringStream challengeExport)
  let solution ← Export.parseStream (← stringStream solutionExport)
  let theoremNames ← getTheoremNames
  let definitionNames ← getDefinitionNames
  let mut provisioned : Option Lean.Environment := none
  let mut guardedConstants := solution.constMap
  let mut uncertifiedLibrary := false
  if let some path := (← read).certifiedBasePath then
    let base ← try GateBase.imported path catch _ =>
      recordStatus [("stage", .str "error"), ("reason", .str "kernel-unavailable")]
      recordStatus [("stage", .str "final"), ("rc", Lean.toJson (70 : Nat))]
      return ← IO.Process.exit 70
    let declared := (← read).candidateDeclarations.getD
      (challenge.constMap.toList.map (fun (name, _) => name.toString)).toArray
    let own ← match GateBase.moduleOwned solution.constMap base.toKernelEnv declared with
      | .error _ =>
        recordStatus [("stage", .str "policy"), ("reason", .str "certified-base-mismatch")]
        throw <| .userError "module-owned declaration profile mismatch"
      | .ok own => pure own
    if theoremNames.any (fun name => !own.contains name) then
      recordStatus [("stage", .str "policy"), ("reason", .str "certified-base-mismatch")]
      throw <| .userError "target must belong to the own delta"
    uncertifiedLibrary := GateBase.hasUncertifiedLibrary solution.constMap base.toKernelEnv declared
    -- Unsupported library payloads cannot consume the miner's input allowance.
    -- The module-owned subset still has all independently decidable guards.
    if uncertifiedLibrary then guardedConstants := own
    provisioned := some base
  if guardedConstants.size > (← read).maxExportedConstants then
    recordStatus [("stage", .str "policy"), ("reason", .str "resource-cap")]
    throw <| .userError "resource-cap: exported constant count"
  let limits := (← read).leanKernel
  let nanodaLimits := (← read).nanodaKernel
  if let .error _ := GateFuel.checkInputs guardedConstants limits.literal_bytes (limits.max_rec_depth * 16) then
    recordStatus [("stage", .str "policy"), ("reason", .str "resource-cap")]
    recordStatus [("stage", .str "final"), ("rc", Lean.toJson (3 : Nat))]
    return ← IO.Process.exit 3
  if uncertifiedLibrary then
    -- These mandatory per-declaration debits need no uncertified dependencies.
    if guardedConstants.size * limits.declaration_charge > limits.aggregate_raw ||
        guardedConstants.size * nanodaLimits.declaration_charge > nanodaLimits.aggregate_work then
      recordStatus [("stage", .str "policy"), ("reason", .str "resource-cap")]
      recordStatus [("stage", .str "final"), ("rc", Lean.toJson (3 : Nat))]
      return ← IO.Process.exit 3
  let mut cached : Option Lean.Environment := none
  let mut own : Option (Std.HashMap Lean.Name Lean.ConstantInfo) := none
  let mut checkedNames : Option (Array (Array String)) := none
  if let some base := provisioned then
    if let .error _ ← GateBase.select challenge.constMap base.toKernelEnv 12000000 false then
      recordStatus [("stage", .str "error"), ("reason", .str "kernel-unavailable")]
      recordStatus [("stage", .str "final"), ("rc", Lean.toJson (70 : Nat))]
      return ← IO.Process.exit 70
    -- Direct reference-measurement callers may use the pristine challenge's
    -- declarations. Production check() supplies module-owned CONST names.
    let declared := (← read).candidateDeclarations.getD
      (challenge.constMap.toList.map (fun (name, _) => name.toString)).toArray
    cached := some base
    match ← GateBase.select solution.constMap base.toKernelEnv 12000000 true (some declared) with
    | .error (.cap _) =>
      recordStatus [("stage", .str "policy"), ("reason", .str "resource-cap")]
      recordStatus [("stage", .str "final"), ("rc", Lean.toJson (3 : Nat))]
      return ← IO.Process.exit 3
    | .error (.reject _) =>
      recordStatus [("stage", .str "policy"), ("reason", .str "certified-base-mismatch")]
      throw <| .userError "certified base or declaration profile mismatch"
    | .error (.unavailable _) =>
      -- Structural statement/axiom checks also need no uncertified kernel work.
      uncertifiedLibrary := true
    | .ok declarations =>
      if theoremNames.any (fun name => !declarations.contains name) then
        recordStatus [("stage", .str "policy"), ("reason", .str "certified-base-mismatch")]
        throw <| .userError "target must belong to the own delta"
      let names := declarations.toList.map Prod.fst |>.toArray.qsort (fun a b => a.toString < b.toString)
      checkedNames := some (names.map GateBase.nameParts)
      own := some declarations
      cached := some base
  -- Every supplied certified-base payload has been checked for identity.
  -- Statement and axiom traversal is structural, including uncertified material.
  -- Omitted base declarations come from that state for comparison;
  -- supplied declarations remain intact and the own replay set stays unchanged.
  let mut comparisonChallenge := challenge
  let mut comparisonSolution := solution
  if let some base := cached then
    for (name, info) in base.constants.toList do
      if !comparisonChallenge.constMap.contains name then
        comparisonChallenge := { comparisonChallenge with
          constMap := comparisonChallenge.constMap.insert name info }
      if !comparisonSolution.constMap.contains name then
        comparisonSolution := { comparisonSolution with
          constMap := comparisonSolution.constMap.insert name info }
  let targets := (← getTheoremNames) ++ (← getLegalAxioms)
  if let .error error := Comparator.compareAt comparisonChallenge comparisonSolution targets definitionNames (← primitiveTargets) then
    recordStatus [("stage", .str "policy"), ("reason", .str "comparator-statement-mismatch")]
    throw <| .userError error
  if let .error error := Comparator.checkAxioms comparisonSolution theoremNames definitionNames (← getLegalAxioms) then
    recordStatus [("stage", .str "policy"), ("reason", .str "disallowed-axiom")]
    throw <| .userError error
  if uncertifiedLibrary then
    recordStatus [("stage", .str "error"), ("reason", .str "library-outside-certified-base")]
    recordStatus [("stage", .str "final"), ("rc", Lean.toJson (70 : Nat))]
    return ← IO.Process.exit 70
  let mut result := none
  for (kernelName, kernelCommand) in ← getExternalKernels do
    result := result <|> (← runExternalKernel kernelName kernelCommand solutionExport checkedNames)
  result := result <|> (← runBuiltinKernel solution cached own)
  if let some error := result then
    throw <| IO.userError error

def compareIt : M Unit := do
  let exportTargets := (← builtinTargets) ++ (← getTheoremNames) ++ (← getLegalAxioms)
    ++ (← primitiveTargets) ++ (← getDefinitionNames)

  let challengeModule ← getChallengeModule
  safeLakeBuild challengeModule
  let challengeExport ← safeExport challengeModule exportTargets

  let solutionModule ← getSolutionModule
  safeLakeBuild solutionModule
  let solutionExport ← safeExport solutionModule exportTargets

  verifyMatch challengeExport solutionExport

  IO.println "Your solution is okay!"

structure Config where
  challenge_module : String
  solution_module : String
  theorem_names : Array String
  definition_names : Option (Array String) := none
  permitted_axioms : Array String
  enable_nanoda? : Option Bool
  external_kernels? : Option (Std.TreeMap String (Array String))
  solution_export_file? : Option String := none
  max_exported_constants : Nat := 1000000
  lean_kernel : GateFuel.Replay.Limits := {}
  nanoda_kernel : NanodaLimits := {}
  certified_base_olean : Option String := none
  candidate_declarations : Option (Array String) := none
  deriving Lean.FromJson, Lean.ToJson, Repr

def M.run (x : M α) (cfg : Config) : IO α := do
  let cwd ← IO.Process.getCurrentDir
  let leanPrefix ← queryLeanPrefix cwd
  let gitLocation ← queryGitLocation
  let whichLean4Export := (← IO.getEnv "COMPARATOR_LEAN4EXPORT").getD "lean4export"
  let whichLandrun := (← IO.getEnv "COMPARATOR_LANDRUN").getD "landrun"
  let mut externalKernels := cfg.external_kernels?.getD {}
  let defaultNanoda := "nanoda_bin"
  let nanodaOverride? ← IO.getEnv "COMPARATOR_NANODA"

  if cfg.enable_nanoda?.getD false && !externalKernels.isEmpty then
    throw <| .userError "Cannot use enable_nanoda and an external kernel list at the same time, register nanoda in the list instead."

  for (kernelName, kernelCommand) in externalKernels do
    if kernelCommand.isEmpty then
      throw <| .userError s!"{kernelName} has an empty command"

  if cfg.enable_nanoda?.getD false then
    let whichNanoda := nanodaOverride?.getD defaultNanoda
    externalKernels := externalKernels.insert "nanoda" #[whichNanoda]
  else if let some nanodaOverride := nanodaOverride? then
    externalKernels := externalKernels.modify "nanoda" fun cmd => cmd.set! 0 nanodaOverride

  ReaderT.run x {
    projectDir := cwd
    challengeModule := cfg.challenge_module.toName,
    solutionModule := cfg.solution_module.toName,
    theoremNames := cfg.theorem_names.map String.toName,
    definitionNames := cfg.definition_names.getD #[] |>.map String.toName,
    legalAxioms := cfg.permitted_axioms.map String.toName,
    leanPrefix := leanPrefix,
    gitLocation := gitLocation,
    whichLean4Export := whichLean4Export,
    whichLandrun := whichLandrun,
    externalKernels := externalKernels
    solutionExportFile := cfg.solution_export_file?.map System.FilePath.mk
    maxExportedConstants := cfg.max_exported_constants
    leanKernel := cfg.lean_kernel
    nanodaKernel := cfg.nanoda_kernel
    certifiedBasePath := cfg.certified_base_olean.map System.FilePath.mk
    candidateDeclarations := cfg.candidate_declarations
  }

end Comparator

def runMain (args : List String) : IO Unit := do
  let some (configPath : String) := args[0]?
    | throw <| .userError "Expected config file path as first argument."
  let content ← IO.FS.readFile configPath
  let config ← IO.ofExcept <| Lean.FromJson.fromJson? <| ← IO.ofExcept <| Lean.Json.parse content
  if let [_, "--exports", challengePath, solutionPath] := args then
    let challengeExport ← IO.FS.readFile challengePath
    let solutionExport ← IO.FS.readFile solutionPath
    let config := { config with solution_export_file? := some solutionPath }
    Comparator.M.run (Comparator.verifyMatch challengeExport solutionExport) config
  else
    Comparator.M.run Comparator.compareIt config

/-- Structured process contract: success 0, semantic rejection 2, transport/launch 70. -/
def mainCore (args : List String) : IO UInt32 := do
  try
    runMain args
    Comparator.recordStatus [("stage", .str "final"), ("rc", Lean.toJson (0 : Nat))]
    return 0
  catch e =>
    IO.eprintln e.toString
    let rc : UInt32 := match e with
      | .userError _ => 2
      | _ => 70
    Comparator.recordStatus [("stage", .str "final"), ("rc", Lean.toJson rc.toNat)]
    return rc

/-- Use the configured virtual stack reservation for all replay work. -/
def main (args : List String) : IO UInt32 := do
  let task ← IO.asTask (mainCore args) Task.Priority.dedicated
  match ← IO.wait task with
  | .ok code => return code
  | .error e => IO.eprintln e.toString; return 70
