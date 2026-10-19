These two small exports come from the adjacent Lean sources, compiled with Lean v4.34.1
and the pinned lean4export format 3.1 exporter. They were generated directly from these
sources. They are test fixtures, not copied research exports.

Rebuild each directory with the provisioned toolchain and exporter:

```sh
lean -o "$BUILD/ProofMod.olean" ProofMod.lean
LEAN_PATH="$BUILD" lean4export ProofMod -- PublicClosureFixture.same > Challenge.ndjson
```

Both targets are `PublicClosureFixture.same : payload = payload`, with the same canonical root-type
hash `3c4a3ac6f10f48d32bd431c07ae728f55ba9fb30522ed40ecc713289a634ad42`.
The definition of `payload` changes from 0 to 1. Their full closure identities are:

- original: `ca0ec3458a9087742bcaff6f6fa7e3e34e1124d04b0b68ecddbc6f525bf325c7`
- changed: `2a4a5b42b27665d913ab70097de9f7589e53d29a91b01a5b2e2b2857cfb04097`
