# Migration Source References

This Rust workspace is a separate migration/spike workspace.

## Original Boole/PoF repo

```text
legacy-pof
```

## Local planning docs in original repo

```text
legacy-pof/local-docs/rust-core-migration-implementation-plan.md
legacy-pof/local-docs/actual-code-spec-family-design.md
legacy-pof/local-docs/signal-shot-lean-flow-and-boole.md
legacy-pof/local-docs/idle-ai-capacity-positioning.md
```

## Initial TypeScript modules to preserve by fixture parity

```text
legacy-pof/dispatcher/src/chain.ts
legacy-pof/dispatcher/src/hash.ts
legacy-pof/dispatcher/src/blockStore.ts
legacy-pof/dispatcher/src/rewardLedger.ts
legacy-pof/dispatcher/src/blockBuilder.ts
legacy-pof/dispatcher/src/booleCli.ts
```

## Migration invariant

Rust must match golden behavior before it replaces any existing TypeScript runtime path.
