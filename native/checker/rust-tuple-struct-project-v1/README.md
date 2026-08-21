# RUST-TUPLE-STRUCT-CHECKER-V1

This directory is the answer-free, tracked semantic checker release for the
`TUPLE-STRUCT-PROJECT` Rust family. It generates candidate-dependent hidden
tests from a pinned task contract, compiles the submitted module with the
pinned Rust toolchain, and returns a three-state verdict.

The verdict reproduces the sealed family's deterministic 64-prefix hidden-test
contract. It is behavioral qualification evidence, not a mathematical proof
that the submitted function is correct for every possible Rust input.

The release contains no solution generator, mining answer, author witness,
model prompt or model transcript. Its public positive fixture is synthetic and
permanently non-issuable.

## Status boundary

This release is for clean-checkout qualification only. `activationAllowed` is
false. It is not connected to an HTTP route, share pool, block, reward or
consensus path. Runtime activation remains blocked until kernel network and
filesystem containment and a node-owned strict registry are separately
qualified.

## Toolchain

The semantic compiler identity is `nightly-2026-07-22`, rustc commit
`e7795af6d2449fb05a6393c3320ced873a999eb3`. The checker validates the rustc
and Cargo release/commit metadata. Platform-specific compiler binaries are not
included or claimed byte-identical; production qualification still requires
separate binary provenance.

Linux qualification enforces the frozen address-space and process-count
kernel limits. macOS cannot reliably apply those two limits to this compiler
process tree, so its local qualification run enforces the remaining limits
but is not containment evidence. This is one reason `activationAllowed`
remains false; production activation requires the separate containment gate.

Run the public qualification test with:

```bash
python3 -m unittest scripts/test_native_shadow_authority.py
```
