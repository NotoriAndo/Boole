# EVM zkVM feasibility — CI verifier fixtures (provenance)

Ceiling label: **EVM-ZKVM-FEASIBILITY-PASS**. These fixtures back a
verifier-only, default-OFF compressed-STARK check outside the consensus path.
They do NOT make EVM mineable; EVM `mineable_now` stays **0**. Nothing here is a
public-network / leaderboard / production claim.

## What these files are

| file | bytes | sha256 |
| --- | ---: | --- |
| `proof-compressed.bin` | 1,272,546 | `59a5887ef682d47c18cea964f9c5df57b7d91edf6b946b6113645816f0ea24cf` |
| `vk-hash.bin` | 32 | `149abd9de7f3157bdab1acd22cb37a0136288c7a2b31f080294246dd4ac9d1c6` |
| `public-values.bin` | 782 | `040e678db61ebd8e406d765ace2c85229fa79f77538fde5161bca8dc85d9b255` |

* `proof-compressed.bin` — the single frozen feasibility proof, re-serialized to
  the exact wire form the verify-only `sp1-verifier` crate deserializes:
  `bincode::serialize(&SP1Proof)` (the proof ENUM). See "Transcode" below.
* `vk-hash.bin` — the FIXED verifying-key hash `bincode::serialize(&[SP1Field; 8])`
  (KoalaBear vk hash) of the frozen guest ELF. This is the only key the verifier
  trusts; the production surface has no vk-taking constructor.
* `public-values.bin` — the proof's public values byte-string (782 B). Header =
  6 × 32-byte digests (offsets: task_contract@0, case_or_batch_root@32, fork@64,
  canonical_input@96, author_oracle@128, observed_accounts@160); body @192.

Byte integrity is pinned by `SHA256SUMS` (run `shasum -a 256 -c SHA256SUMS`).

## Digest scope (operator msg 3526)

* The proof-blob sha256 in `SHA256SUMS` is a **fixture-integrity** digest ONLY.
  It is NOT a production accept condition: a different valid proof of the same
  frozen task would also ACCEPT. The verifier never source-rejects a proof for
  having a different byte digest.
* The compressed STARK cryptographically binds the proof to the FIXED vk and to
  the whole public-values byte-string, and to nothing more.
* The `task_contract` / `fork` / `author_oracle` header digests the verifier
  checks for equality are an **external admission filter** (a policy check on
  caller-supplied pv bytes), NOT themselves cryptographic binding — even though
  those bytes happen to live inside the cryptographically-bound pv string.

## Transcode (before → after)

The frozen proof was produced out of band by the SP1 SDK wrapper and stored as
an `SP1ProofWithPublicValues` wrapper. The verify-only `sp1-verifier` crate
consumes a different (smaller) wire form, so a ONE-TIME transcode re-serialized
it. No new proof was generated; no proving key exists in this repo.

* Original SDK wrapper: sha256
  `eceea6e9fcfddcba71a6e2dc8cd2ae65fa717529e8cee131bde3d5369be57e55`
  (1,273,351 bytes, `SP1ProofWithPublicValues`).
* Guest ELF: sha256
  `0983d5fe6dd205a6487e5ddb5a5850031b69ce43192090376cc6c5816320c1fb`
  (2,311,024 bytes).
* Tooling: SP1 SDK `sp1-sdk` **6.3.1**, circuit `SP1_CIRCUIT_VERSION` **v6.1.0**.
  The transcode tool lives OUT of the workspace (git-ignored sandbox,
  `local-docs/evm-zkvm-feasibility/project/transcode`); the SP1 SDK is NOT a
  Boole workspace dependency — only `sp1-verifier` (verify-only) is.
* `vk.bytes32()` = `0x0055a63544a5bf8ea7b944c2208fa9d011312da36a70afda0d73222ff2302eae`
  (matches the frozen freeze record).
* Encoding chosen empirically by round-trip: `bincode(&SP1Proof)` (the ENUM)
  VERIFIED via `SP1CompressedVerifierRaw::verify_with_public_values`
  (pure verify 0.924 s); the inner-struct encoding FAILED ("invalid proof
  type"). After transcode: `proof-compressed.bin` sha256
  `59a5887ef682d47c18cea964f9c5df57b7d91edf6b946b6113645816f0ea24cf`.

## Reproduce the verify (no SDK, verify-only)

`cargo test -p boole-evm-adapter --test zk_verify_accept` runs the real-proof
ACCEPT and the reject matrix against the compiled-in FIXED vk — CI-affordable
(~1 s crypto check), using only `sp1-verifier` with `default-features = false`.
