import Lake
open Lake DSL

package boole_check

lean_lib «Boole» where
  globs := #[.submodules `Boole.Family]

lean_exe boole_check where
  root := `BooleCheck.Main
