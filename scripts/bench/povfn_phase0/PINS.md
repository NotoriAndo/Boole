# PoVFN Phase 0-A — pinned tools and build recipe

Offline experiment harness (operator directive 2026-07-19). Vendored clones
live under `vendor/` (gitignored); this file is the reproducibility record.
Nothing here modifies the consensus checker pin (`lean/checker/`).

## Toolchains

| tool | version | source of pin |
|---|---|---|
| Lean | `leanprover/lean4:v4.29.1` (githash `f72c35b3f637c8c6571d353742168ab66cc22c00`) | same as `lean/checker/lean-toolchain` (consensus pin, untouched) |
| Rust | rustc 1.95.0 (59807616e 2026-04-14) | host toolchain, recorded |

## lean4export (exporter: pinned Lean env -> NDJSON)

- repo: https://github.com/leanprover/lean4export
- branch `bump_to_v4.29.0-rc8`, commit `9fd1dbd65294ff8eac2d04382144eac95fb3abbb` (2026-03-24)
- local patch: `lean-toolchain` overridden `v4.29.0-rc8` -> `v4.29.1`
  (patch-level bump; builds clean, emitted meta line reports
  `lean.version 4.29.1`, `lean.githash f72c35b...` matching the consensus
  RELEASE-MANIFEST githash)
- build: `~/.elan/bin/lake build` in `vendor/lean4export`
- binary: `vendor/lean4export/.lake/build/bin/lean4export`
- format: NDJSON, exporter/format version `3.1.0`
- usage: `lake env <binary> <Module> [-- <decl>...] > out.ndjson`
  (`-- decls` restricts to the dependency closure of the named declarations:
  measured 139 KB / 2,543 lines for a `by decide` theorem closure vs
  324 MB / 6.06 M lines for the module + full transitive env)

## nanoda_lib (independent Rust Lean4 kernel checker)

- repo: https://github.com/ammkrn/nanoda_lib (Apache-2.0)
- commit `f58f2f6d535e189a40fcb02ede8eb95f97a92d37` (2026-06-02), v0.4.10-beta
- local patch: appended empty `[workspace]` table to `Cargo.toml` so the
  vendored clone is not absorbed into the Boole cargo workspace
- build: `cargo build --release` in `vendor/nanoda_lib`
- binary: `vendor/nanoda_lib/target/release/nanoda_bin <config.json>`
- config knobs used: `permitted_axioms`, `nat_extension`, `string_extension`
- behavior notes (measured 2026-07-19):
  - checks every declaration present in the export file; exit 0 means
    "all present declarations typecheck" — it does NOT assert that any
    particular theorem is present. Target-declaration presence + expected
    statement equality is a separate driver-side binding check (P-stage).
  - truncated export files therefore still exit 0 (fewer decls checked).
  - genuinely false statements / corrupted proof terms are rejected via
    panic (exit 101), not a clean exit 1 — fail-closed, but structured
    rejection would need a thin library-mode wrapper.

## leanchecker (toolchain-bundled kernel replayer, third judge)

- binary: `~/.elan/toolchains/leanprover--lean4---v4.29.1/bin/leanchecker`
- ships with the pinned toolchain (Lean >= 4.28 bundles it); replays a
  module's environment through the C++ kernel. Same kernel as `boole_check`
  but a separate invocation path that bypasses elaboration trust.
