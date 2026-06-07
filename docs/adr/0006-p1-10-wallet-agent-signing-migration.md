# ADR 0006: P1.10 — delegate prover signing to the wallet-agent subprocess

**Status:** accepted (2026-06-07)

**Context.** P1.10's acute leak — the ed25519 prover seed appearing on the
`ps`-visible command line via `--prover-sk-hex` — was closed by accepting the
seed through `BOOLE_PROVER_SK_HEX` instead (`cd88eaf`). But the seed still
enters the `boole-miner` process address space (`SigningKeyV2::from_seed_hex`
holds it for the lifetime of the sign). The P1.10 target is for the seed to
never enter the miner at all. `boole-wallet-agent` already owns an AEAD vault
and signs without the seed ever touching argv:

```text
boole-wallet-agent sign --vault <path> --message <hex>   # passphrase on stdin
  → opens the vault, raw-ed25519-signs the message bytes, prints signature hex
boole-wallet-agent pubkey --vault <path>                  # passphrase on stdin
  → prints the verifying-key hex
```

**Decision.** Migrate the miner's bounty-proof signing to delegate the raw
signature to the wallet-agent subprocess, keeping the digest computation and
envelope assembly in the caller. A `boole.signed.v1` signature is
`ed25519.sign(digest_for(payload, network_id))` where `digest_for` is a 32-byte
SHA-256 (canonical JSON, with a network domain-separation tag when scoped). The
wallet-agent's `sign --message <hex>` does exactly `ed25519.sign(message_bytes)`.
So passing `message = hex(digest_for(payload, network_id))` to the agent yields a
**byte-identical** signature to the in-process `SigningKeyV2::sign_for_network`
— the seed never enters the miner.

Concretely:
1. **boole-core** exposes `signing_digest_hex(payload, network_id)` (hex of the
   private `digest_for`), so a caller without the key can compute exactly what
   must be raw-signed.
2. **boole-miner** introduces a `ProofSigner` abstraction with two impls:
   - `KeySigner(SigningKeyV2)` — the in-process path (unchanged behaviour).
   - `AgentSigner { agent_bin, vault_path, passphrase }` — resolves the pk via
     `pubkey`, computes the digest via `signing_digest_hex`, gets the raw
     signature via `sign --message <hex(digest)>`, and assembles the
     `SignedEnvelope` from parts (`SIGNED_ENVELOPE_SCHEMA`, payload, pk,
     signature, network_id). The passphrase is read from
     `BOOLE_WALLET_PASSPHRASE` (never argv) and piped to the agent's stdin.
3. **boole-miner cli** lets `mine bounty` take `--prover-vault <path>` as an
   alternative to a raw seed; with a vault, the seed never enters the miner.

**Why subprocess-per-sign, not a long-running socket daemon.** The existing
agent is a per-invocation binary (the passphrase-per-operation model from
P1.10f). Bounty submit is one-shot (one or two agent calls), so the subprocess
cost is irrelevant and we avoid introducing a socket lifecycle / auth surface.
A socket daemon is a possible future optimisation for high-frequency signing.

**Why byte-identical matters.** The node verifier is unchanged; a vault-signed
envelope and a seed-signed envelope are indistinguishable on the wire, so this
is a pure key-custody migration with zero consensus/protocol change. Pinned by a
boole-core test (raw-signing `signing_digest_hex` reproduces
`sign_for_network`'s signature) and a boole-miner test (`AgentSigner` over a
`migrate-from-hex` vault equals `KeySigner` with the same seed).

**Scope / residual.** This migrates the **bounty-proof** signing path. The
broader session-key / `boole bounty submit|announce|status|sign-work` signing
call sites can adopt the same `ProofSigner` seam incrementally; they remain on
the (now env-based, non-argv) seed path until then. The seed path is retained
for back-compat; `--prover-vault` is the opt-in secure path.
