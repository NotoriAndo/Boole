# ADR 0002: P2.9 wallet CLI scope narrowed to init|address|sign|migrate

## Status

Status: Implemented (2026-05-27).

P2.9 of the production-readiness master plan is narrowed from the original
`boole wallet init|unlock|lock|restore|migrate` surface to the four
subcommands actually shipped: `boole wallet init|address|sign|migrate`.
The three deferred subcommands — `unlock`, `lock`, `restore` — move to
Wave P3 and are not preconditions for P2 closure.

## Context

The §4 L1 master target for P2.9 (recorded in
`local-docs/master-todo-progress-and-design-review-2026-05-18.md`) lists
the wallet CLI surface as `boole wallet init|unlock|lock|restore|migrate`.

The implementation that landed in commit `b99fc58`
(`cli: boole wallet init/address/sign/migrate façade (P2.9)`) shipped
`init|address|sign|migrate` instead. The four shipped subcommands are
thin façades over `boole-wallet-agent` (see `crates/boole-cli/src/main.rs`
`wallet_init`, `wallet_address`, `wallet_sign`, `wallet_migrate` and
`crates/boole-wallet-agent/src/main.rs` `init`, `pubkey`, `sign`,
`migrate-from-hex` subcommands). `address` was added in place of the
master target's bare `unlock` to give wallet operators a read-only key
inspection path that does not require unsealing the AEAD vault.

The two surfaces diverge because the master target was authored before
the AEAD-vault signing-isolation work (P1.10f, commit `78ae7e1`) settled
on a stateless passphrase-per-operation model. Under that model:

- `boole-wallet-agent` reads the passphrase from the first line of stdin
  on every `init` / `sign` / `migrate-from-hex` invocation
  (`read_passphrase` in `crates/boole-wallet-agent/src/main.rs`).
- The unsealed seed material lives only for the duration of one
  subcommand invocation and is zeroized on drop via `EncryptedVault::open`.
- There is no long-lived agent session, so `unlock` (start a session)
  and `lock` (end one) have no in-process state to act on.

`restore` from a 24-word BIP39 mnemonic was held back because the
`boole-wallet-agent` `migrate-from-hex` path already accepts a 32-byte
hex seed on stdin, which covers the equivalent recovery use case for
the current closed-local deployment. Adding BIP39 word-list dependence
and a mnemonic-parsing surface area is deferred until a wallet UX
review that has not happened yet.

The P2.9 closure criterion in
`local-docs/todo/todo-production-readiness-master.md` §6.5 explicitly
allows either implementing the missing subcommands (Option A) or
amending the plan in a checked-in document (Option B). This ADR is the
Option B amendment.

## Decision

P2.9 closes against the shipped surface:

- `boole wallet init` — seal a fresh ed25519 seed into an AEAD vault at
  `--vault`; passphrase read from stdin.
- `boole wallet address` — print the hex ed25519 pubkey stored in the
  vault without unsealing the seed.
- `boole wallet sign` — open the vault with the passphrase and produce
  an ed25519 signature over `--message` bytes.
- `boole wallet migrate` — re-seal a plaintext 32-byte hex seed read
  from stdin into a fresh AEAD vault.

`unlock`, `lock`, and `restore` are deferred to Wave P3. They must be
explicitly scheduled in P3 if and when the wallet-agent transitions
from a stateless passphrase-per-operation model to a long-lived agent
session — that transition is itself a P3 decision and not a P2 close
blocker.

In the interim, the lock/unlock UX is carried by the AEAD vault itself:
the vault stays sealed at rest, every signing operation re-prompts for
the passphrase via stdin, and the unsealed seed never outlives one
subcommand invocation. Operators who want a session-key flow can use
the existing `boole-wallet-agent` policy-bound signer surface
(`crates/boole-wallet-agent/src/main.rs` plus the session/policy work
from `6a4da8e`, `922c301`, `7d387f0`, `2b95637`) — that surface is
independent of the umbrella `boole wallet` CLI and is not part of the
P2.9 scope being closed here.

## Consequences

- P2.9 closes without `unlock`, `lock`, `restore`. The §6.5 closure
  block records this ADR's commit SHA as the closure note.
- Any future expansion of `boole wallet` to add `unlock`, `lock`,
  `restore`, or BIP39 `restore` must land under a P3 slice and update
  this ADR's Status to "Superseded by ADR-NNNN".
- The lock/unlock UX boundary statement above is binding: passphrase is
  re-read on every signing operation; no in-process secret survives a
  subcommand exit. Any change to that property is a P3 ADR.
- This amendment is doc-only; no Rust source changes ride with it.
  Self-test gate behaviour is unchanged.
- Closure does not constitute a public-network claim. P2.9 is closed
  for closed-local wallet operation only; public-testnet wallet
  operability rides on P2.10 and is tracked separately.
