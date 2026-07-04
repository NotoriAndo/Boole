# Replay consensus evidence

Boole replay is the safety rail that lets an external reviewer rebuild chain state from persisted blocks instead of trusting runtime memory. The current replay surface intentionally supports both legacy migration fixtures and stricter evidence-backed blocks.

## Replay path

```text
PersistedBlock JSON
→ shape validation
→ selected share evidence verification (required by default)
→ block hash/head replay
→ reward/account state replay
```

The Rust entry point is `boole_core::replay_blocks`. It rejects a block whose `selectedShareEvidence` is empty — replay no longer degrades to "block c hash matches, done" when evidence is missing (N3-pre.1). `replay_blocks` is the strict, no-opt-in entry point; it is the one a future p2p ingest replay path must use.

## legacy/no-evidence replay compatibility (explicit opt-in only)

Older replay fixtures do not carry full selected proof packages. They remain accepted for migration compatibility, but only through an explicit opt-in — `boole_core::LegacyEvidenceOptIn` (constructed via `LegacyEvidenceOptIn::for_legacy_replay_only()`) passed to `replay_blocks_allow_legacy_evidence_less` / `replay_blocks_with_retarget_allow_legacy_evidence_less`:

```text
fixtures/protocol/replay/v1.json
```

Compatibility rule:

- `selectedShareEvidence` empty or absent is rejected by the default `replay_blocks` entry point;
- a caller that must replay a pre-evidence legacy chain (existing golden fixtures, hand-built test chains, the node's own historical block store at boot, or the offline `boole state verify` / `boole chain replay` CLI tools) opts in explicitly by threading a `LegacyEvidenceOptIn` through the `_allow_legacy_evidence_less` entry points; every such call site is greppable by that suffix;
- this opt-in path is only for legacy/local replay of already-exported golden fixtures or a node's own persisted state, and must never be reachable from a future p2p ingest replay path — that path has to call the strict `replay_blocks`/`replay_blocks_with_retarget`, which have no parameter that could accept the opt-in.

## Evidence-backed replay blocks

New runtime-produced/evidence-backed blocks carry enough selected-share data for replay to recompute the proof binding:

```json
{
  "selectedShareEvidence": [
    {
      "pk": "<32-byte hex public key>",
      "n": "<32-byte hex ticket nonce>",
      "j": "<32-byte hex share nonce>",
      "c": "<previous chain head>",
      "canonHash": "<sha256(proofPackage)>",
      "proofPackage": "<canonical POFP package bytes as hex>"
    }
  ],
  "minShareScoreMultiplierNanos": 1000000000
}
```

Replay rejects evidence-backed blocks unless every selected share has matching evidence. The verifier checks:

- `selectedShareEvidence.length == selectedShareHashes.length`;
- each evidence `c` equals the block `prevC`;
- each evidence `pk` equals the corresponding `selectedSharePks` entry;
- `proofPackage` is valid hex and has an accepted POFP package shape;
- `canonHash == sha256(proofPackage)`;
- `shareHash(c, pk, n, j, canonHash)` equals the corresponding `selectedShareHashes` entry.

This means runtime-selected work is not merely recorded; replay can re-derive the selected share identity from the persisted proof package.

## Admission-policy binding

Evidence-backed replay also binds the block-carried minimum share score to the admission policy multiplier. The persisted field is:

```text
minShareScoreMultiplierNanos
```

Replay parses the block `tShare`, recomputes:

```text
minShareScore = tShare * minShareScoreMultiplierNanos / 1_000_000_000
```

and compares it to the block's persisted `minShareScore`.

Two policy-binding failures are intentionally stable regression surfaces:

```text
selected share evidence requires minShareScoreMultiplierNanos
selected share evidence minShareScore mismatch
```

The first protects against evidence-backed blocks silently falling back to a zero/default policy. The second protects against accepting a selected-share set under a different multiplier than the one used at admission.

## Golden fixtures

Replay fixtures are deliberately split by compatibility level:

- `fixtures/protocol/replay/v1.json`
  - legacy TypeScript-derived replay fixture;
  - no `selectedShareEvidence` requirement;
  - preserves migration compatibility.
- `fixtures/protocol/replay/v2.json`
  - Rust consensus golden fixture;
  - includes `selectedShareEvidence` and `minShareScoreMultiplierNanos`;
  - verifies the stricter replay path used for evidence-backed blocks.

Both fixtures are checked by:

```bash
cargo test -p boole-core --test replay_fixtures
```

## Public-testnet rule

For public-testnet evidence, prefer blocks that include `selectedShareEvidence` and `minShareScoreMultiplierNanos`. Legacy/no-evidence replay compatibility exists so historical migration fixtures keep testing the old surface; it is not the target security posture for new chain evidence.
