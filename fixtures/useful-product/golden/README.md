# Useful-product golden fixtures (BF.5-pre / A2)

Pinned golden subset of the experiment-GO artifacts that the BF.5
verifier-adapter gate re-verifies. Imported from the gitignored
`local-docs` experiment records under the A2 rules:

1. upstream repository URL + immutable commit pin (`PROVENANCE.json`)
2. license verified; license texts ship in `licenses/`
3. `SHA256SUMS` covers every imported byte
4. release files larger than 64 KiB are NOT imported - they stay hash
   references pinned by the packet's own `manifest.json`, and can be
   regenerated from the pinned upstream commit with the toolchain in
   `meta/toolchain-lock.json`

**CI rule (BF.5)**: CI golden input is ONLY this directory. Tests must
never read `local-docs`.

Items:
- `adapter-r5-gnark-crypto/` - ADAPTER-P0-R5 strict-blind card
  (gnark-crypto Grumpkin marshal vector-length contract)
- `llm-mining-strong-r1/` - LLM-MINING-P0 strong-model GO product
  package (15 files, ~68 KB) + experiment result record
- `supply-chain-poseidon/` - SUPPLY-CHAIN-FIDELITY-P0 Poseidon release
  (small verdict files; wasm/ptau/zkey/r1cs/constants hash-referenced)

Integrity is enforced by
`crates/boole-core/tests/useful_product_golden_fixtures.rs`.
