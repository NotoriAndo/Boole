# Migration Source References

This Rust workspace is a separate migration/spike workspace.

## Original Boole/PoF repo

```text
/Users/seoyong/projects/pof
```

## Local planning docs in original repo

```text
/Users/seoyong/projects/pof/local-docs/rust-core-migration-implementation-plan.md
/Users/seoyong/projects/pof/local-docs/actual-code-spec-family-design.md
/Users/seoyong/projects/pof/local-docs/signal-shot-lean-flow-and-boole.md
/Users/seoyong/projects/pof/local-docs/idle-ai-capacity-positioning.md
```

## Initial TypeScript modules to preserve by fixture parity

```text
/Users/seoyong/projects/pof/dispatcher/src/chain.ts
/Users/seoyong/projects/pof/dispatcher/src/hash.ts
/Users/seoyong/projects/pof/dispatcher/src/blockStore.ts
/Users/seoyong/projects/pof/dispatcher/src/rewardLedger.ts
/Users/seoyong/projects/pof/dispatcher/src/blockBuilder.ts
/Users/seoyong/projects/pof/dispatcher/src/booleCli.ts
```

## Migration invariant

Rust must match golden behavior before it replaces any existing TypeScript runtime path.
