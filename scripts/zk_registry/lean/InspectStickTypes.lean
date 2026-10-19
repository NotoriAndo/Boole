import Lean

/- Public type observer. The wire representation deliberately erases binder names
   and metadata; declarations and universe expressions otherwise remain intact. -/
namespace StickTypes
open Lean

partial def normalize (input : Expr) : Expr :=
  input.replace fun node =>
    match node with
    | .mdata _ body => some (normalize body)
    | .forallE _ domain body info =>
      some (.forallE `x (normalize domain) (normalize body) info)
    | .lam _ domain body info =>
      some (.lam `x (normalize domain) (normalize body) info)
    | .letE _ type value body nondependent =>
      some (.letE `x (normalize type) (normalize value) (normalize body) nondependent)
    | _ => none

structure Request where
  module : Name
  names : System.FilePath
  output : System.FilePath

def parse (args : List String) : Except String Request :=
  match args with
  | [mod, names, output] =>
    if mod.isEmpty then .error "empty import module"
    else .ok { module := mod.toName, names := names, output := output }
  | _ => .error "expected IMPORT_MODULE NAMES_FILE OUTPUT_FILE"

def resolve (environment : Environment) (query : String) : Array Name := Id.run do
  let direct := query.toName
  if (environment.find? direct).isSome then return #[direct]
  let parts := query.splitOn "._@."
  if parts.length < 2 then return #[]
  let stemPrefix := parts.head! ++ "._@."
  return environment.constants.toList.foldl (init := #[]) fun found entry =>
    if entry.1.toString.startsWith stemPrefix then found.push entry.1 else found

def observe (request : Request) : IO UInt32 := do
  let some sysroot ← IO.getEnv "LEAN_SYSROOT"
    | throw (IO.userError "LEAN_SYSROOT is required")
  initSearchPath sysroot
  let environment ← importModules #[{ module := request.module }] {} (trustLevel := 0)
  let queries := (← IO.FS.readFile request.names).splitOn "\n" |>.filter (! ·.isEmpty)
  let mut rows : Array Json := #[]
  let mut missing : Array Json := #[]
  let mut renames : Array Json := #[]
  for query in queries do
    let candidates := resolve environment query
    if candidates.size != 1 then
      missing := missing.push (Json.mkObj [
        ("name", toJson query), ("candidates", toJson (candidates.map Name.toString))])
    else
      let current := candidates[0]!
      let some declaration := environment.find? current
        | throw (IO.userError "resolved declaration disappeared")
      rows := rows.push (Json.mkObj [
        ("name", toJson query), ("current_name", toJson current.toString),
        ("canonical_type", toJson (normalize declaration.type).dbgToString)])
      if current.toString != query then
        renames := renames.push (Json.mkObj [
          ("old", toJson query), ("current", toJson current.toString),
          ("reason", toJson "Unique same-stem compiler hygiene identifier")])
  IO.FS.writeFile request.output (Json.pretty (Json.mkObj [
    ("rows", toJson rows), ("missing", toJson missing),
    ("compiler_name_renames", toJson renames)]))
  IO.println s!"Type observation: {rows.size} found, {missing.size} unresolved"
  return if missing.isEmpty then 0 else 3

end StickTypes

def main (args : List String) : IO UInt32 := do
  match StickTypes.parse args with
  | .ok request => StickTypes.observe request
  | .error reason => IO.eprintln reason; return 2
