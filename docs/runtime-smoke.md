# Boole Node Runtime Smoke

`runtime-smoke` is a small executable proof that the Rust node can run the current core loop from the actual `boole-node` binary:

```text
scenario input
→ runtime policy boot
→ ticket observation
→ admission
→ share/candidate tracking
→ block commit
→ block store persistence
→ replay verification
→ runtime/store/replay consistency JSON
```

It is not a public testnet, networking layer, or economic benchmark. It is a local deterministic smoke harness for proving the node runtime can produce replayable blocks.

## Run the checked multi-case harness

From the workspace root:

```bash
./scripts/runtime-smoke-all.sh
```

This runs and validates every case listed in the tracked manifest:

```text
fixtures/protocol/runtime-smoke/cases.v1.json
```

Current cases:
- `runtime-smoke-multistep`: `--scenario fixtures/protocol/runtime-smoke/v1.json`, expected `storeSize == 2`.
- `admission-fixture-compat`: `--fixture fixtures/protocol/admission/v1.json`, expected `storeSize == 1`.

The harness prints per-case PASS lines and `runtime-smoke-all: PASS` to stderr, and emits aggregate JSON to stdout:

```json
{
  "ok": true,
  "caseCount": 2,
  "cases": [
    {
      "name": "runtime-smoke-multistep",
      "mode": "scenario",
      "storeSize": 2,
      "replayHeight": 2,
      "latestMatchesRuntime": true,
      "replayMatchesRuntime": true
    },
    {
      "name": "admission-fixture-compat",
      "mode": "fixture",
      "storeSize": 1,
      "replayHeight": 1,
      "latestMatchesRuntime": true,
      "replayMatchesRuntime": true
    }
  ]
}
```

Use the single-case script when you only want the tracked two-block scenario JSON:

```bash
./scripts/runtime-smoke.sh
```

The single-case script removes the target block store, runs the tracked scenario, validates the JSON consistency fields, prints `runtime-smoke: PASS` to stderr, and emits the raw scenario JSON output to stdout.

Optional overrides:

```bash
SCENARIO=fixtures/protocol/runtime-smoke/v1.json \
BLOCK_STORE=/tmp/boole-runtime-smoke.ndjson \
./scripts/runtime-smoke.sh
```

Equivalent direct command:

```bash
cargo run -q -p boole-node -- runtime-smoke \
  --scenario fixtures/protocol/runtime-smoke/v1.json \
  --block-store /tmp/boole-runtime-smoke.ndjson
```

Expected shape:

```json
{
  "ok": true,
  "accepted": true,
  "height": 1,
  "prevC": "<block-0-c>",
  "c": "<block-1-c>",
  "replayHeight": 2,
  "replayLatestC": "<block-1-c>",
  "runtimeHead": "<block-1-c>",
  "droppedStaleShares": 2,
  "storeSize": 2,
  "latestMatchesRuntime": true,
  "replayMatchesRuntime": true,
  "blockStorePath": "/tmp/boole-runtime-smoke.ndjson",
  "blocks": [
    {
      "height": 0,
      "prevC": "0000000000000000000000000000000000000000000000000000000000000000",
      "c": "<block-0-c>"
    },
    {
      "height": 1,
      "prevC": "<block-0-c>",
      "c": "<block-1-c>"
    }
  ]
}
```

## Output fields

- `accepted`: every scenario step was admitted.
- `height`: latest committed block height.
- `prevC`: previous chain head for the latest block.
- `c`: latest committed block hash/head.
- `replayHeight`: height obtained by replaying the recovered block store.
- `replayLatestC`: latest head obtained from replay.
- `runtimeHead`: runtime head after committing all scenario steps.
- `droppedStaleShares`: total stale shares/candidates pruned while advancing heads.
- `storeSize`: number of blocks recovered from `blockStorePath`.
- `latestMatchesRuntime`: recovered store latest block head equals runtime head.
- `replayMatchesRuntime`: replay latest head equals runtime head.
- `blocks`: per-block summaries emitted during the scenario.

For a valid smoke run, the important consistency checks are:

```text
accepted == true
storeSize == replayHeight == blocks.length
latestMatchesRuntime == true
replayMatchesRuntime == true
runtimeHead == replayLatestC == blocks[-1].c
```

## Scenario fixture format

The tracked scenario is:

```text
fixtures/protocol/runtime-smoke/v1.json
```

It contains:

- `cfg`: calibration report used to create `RuntimeConfig`.
- `genesisC`: initial runtime head; the smoke fixture uses the zero genesis hash because replay starts from zero.
- `steps`: ordered admission/block steps.

Each step contains:

- `body`: admission body with `c`, `pk`, `n`, `j`, `nonceS`, and `bytes`.
- `ip`: submission IP used by the rate limiter.
- `canonTag`: accepted canonical verifier tag used by block selection.
- `ts`: block timestamp.
- `cFromRuntimeHead` optional boolean. When true, the runner overwrites `body.c` with the current runtime head before observing/admitting the ticket. This lets later steps build on the previous block without hard-coding the previous block hash.

## Single-fixture compatibility mode

The older fixture-adapter smoke path is still supported:

```bash
cargo run -q -p boole-node -- runtime-smoke \
  --fixture fixtures/protocol/admission/v1.json \
  --block-store /tmp/boole-runtime-smoke-fixture.ndjson
```

This mode adapts the admission fixture into a one-block `RuntimeSmokeScenario`. Prefer `--scenario fixtures/protocol/runtime-smoke/v1.json` for mini-chain demos and benchmark harnesses.

## Verification commands

Use the focused smoke tests:

```bash
./scripts/runtime-smoke-all.sh
./scripts/runtime-smoke.sh
cargo test -q -p boole-node --test runtime_smoke_cli -- --nocapture
cargo test -q -p boole-node --test runtime_smoke_library -- --nocapture
```

Use the full Rust parity gate before committing changes:

```bash
cargo fmt --all
./scripts/check-rust-parity.sh
git diff --check
```
