# Public nanoda source identity

These source bytes derive only from public nanoda commit
`0f88b5f382f0f3f08aed4af0dd6c0e48c3fd7a44` and the committed
`nanoda-0f88b5f-fuel.patch` and `nanoda-0f88b5f-own.patch`.
The three canonical driver modules remain under `scripts/zk_registry/rust/`.
The offline publication check binds every recorded source hash to these bytes;
the builder verifies this snapshot after applying the public patches.
This is source identity, not executable reproducibility or a new toolchain.
See LICENSE for the upstream license. No proof, export or private run is input.
