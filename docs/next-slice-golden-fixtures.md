# Next Slice — Golden Fixture Export

## Goal

Freeze the existing TypeScript implementation's protocol behavior before deeper Rust migration.

## Source repo

```text
/Users/seoyong/projects/pof
```

## Rust workspace

```text
/Users/seoyong/projects/Boole
```

## First fixture target

Start with block hash parity because it is deterministic and small.

Source TypeScript behavior:

```text
/Users/seoyong/projects/pof/dispatcher/src/chain.ts
/Users/seoyong/projects/pof/dispatcher/src/hash.ts
```

Target Rust behavior:

```text
/Users/seoyong/projects/Boole/crates/boole-core/src/hash.rs
```

## Planned fixture path

In the original repo, after approval for source repo edits:

```text
/Users/seoyong/projects/pof/fixtures/protocol/block-hash/v1.json
/Users/seoyong/projects/pof/dispatcher/scripts/export-block-hash-fixtures.ts
/Users/seoyong/projects/pof/dispatcher/test/blockHashFixtures.test.ts
```

In this Rust workspace, consume the copied/generated fixture via:

```text
/Users/seoyong/projects/Boole/fixtures/protocol/block-hash/v1.json
/Users/seoyong/projects/Boole/crates/boole-core/tests/block_hash_fixtures.rs
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
