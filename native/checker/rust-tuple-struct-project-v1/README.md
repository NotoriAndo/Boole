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

The semantic compiler is the official rust-lang CI per-commit build for rustc
`e7795af6d2449fb05a6393c3320ced873a999eb3`, with Cargo commit
`3efb1f477e99b42974b982d939fd100303cdf7db`. It is not the date-based
`nightly-2026-07-22` channel, which points at a different compiler commit. The
checker validates both release/commit identities. Clean Linux CI downloads the
three x86_64 artifacts through `scripts/install-native-checker-toolchain.sh`
and verifies their frozen SHA-256 values before installation. Compiler binaries
are not committed to this repository.

Linux qualification enforces the frozen address-space and process-count
kernel limits. macOS cannot reliably apply those two limits to this compiler
process tree, so its local qualification run enforces the remaining limits
but is not containment evidence. This is one reason `activationAllowed`
remains false; production activation requires the separate containment gate.

After supplying the exact toolchain bin directory, run the public qualification
test with:

```bash
BOOLE_NATIVE_TOOLCHAIN_BIN=/path/to/exact/bin \
  python3 -m unittest scripts/test_native_shadow_authority.py
```
