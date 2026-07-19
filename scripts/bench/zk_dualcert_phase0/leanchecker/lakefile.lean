import Lake
open Lake DSL

/-!
Throwaway Phase 0 experiment executable (zk-circuit-uniqueness-dual-cert.v0).
Uses the SAME pinned toolchain as `lean/checker` (leanprover/lean4:v4.29.1) so
the measured LRAT verification cost is the cost of the real pinned
`Std.Tactic.BVDecide.LRAT.Checker`, but it is a SEPARATE project: the pinned
consensus checker files and SHA256SUMS are not touched.
-/

package zk_dualcert_lrat

lean_exe zk_dualcert_lrat where
  root := `Main
