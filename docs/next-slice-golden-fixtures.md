# Next Slice — Golden Fixture Export

> **Scope note (N5-pre.1, ADR-0014 (a); preimage v3 since ADR-0015 (d))**:
> the block-hash and replay fixtures described below are no longer
> TypeScript-exported. The block-hash preimage (v3) is defined by the Rust
> implementation (`crates/boole-core/src/hash.rs::block_hash`), and
> `fixtures/protocol/block-hash/v3.json` / `fixtures/protocol/replay/*.json`
> are Rust-generated golden vectors — regenerate after a deliberate
> preimage change via the `--ignored` regen tests in
> `block_hash_fixtures.rs` / `replay_fixtures.rs`. This is the "deliberate
> protocol-change ADR" carve-out anticipated by the rule below. The other
> TS-exported fixtures (block-builder, hash-pow, share-pool, ...) are
> unchanged.

## Goal

Freeze the existing TypeScript implementation's protocol behavior before deeper Rust migration.

## Source repo

```text
legacy-pof
```

## Rust workspace

```text
this repository
```

## First fixture target

Start with block hash parity because it is deterministic and small.

Source TypeScript behavior:

```text
legacy-pof/dispatcher/src/chain.ts
legacy-pof/dispatcher/src/hash.ts
```

Target Rust behavior:

```text
this repository/crates/boole-core/src/hash.rs
```

## Planned fixture path

In the original repo, after approval for source repo edits:

```text
legacy-pof/fixtures/protocol/block-hash/v1.json
legacy-pof/dispatcher/scripts/export-block-hash-fixtures.ts
legacy-pof/dispatcher/test/blockHashFixtures.test.ts
```

In this Rust workspace, consume the copied/generated fixture via:

```text
this repository/fixtures/protocol/block-hash/v1.json
this repository/crates/boole-core/tests/block_hash_fixtures.rs
```

## Fixture schema

```json
{
  "version": 1,
  "domain": "block",
  "source": "dispatcher/src/chain.ts:blockHash",
  "cases": [
    {
      "name": "genesis-empty-shares",
      "prevC": "0000000000000000000000000000000000000000000000000000000000000000",
      "shareHashes": [],
      "expectedC": "..."
    }
  ]
}
```

## Verification target

```bash
cargo test -p boole-core block_hash_fixtures
```

Expected:

```text
Rust block_hash matches TypeScript-generated expectedC for every fixture case.
```

## Important rule

Do not manually invent expected hashes. Expected values must come from the existing TypeScript implementation until a deliberate protocol-change ADR says otherwise.
