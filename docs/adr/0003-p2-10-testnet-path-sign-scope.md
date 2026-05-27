# ADR 0003: P2.10 testnet-path signing scope and migration plan

## Status

Status: Implemented (2026-05-28).

P2.10 criterion 2 of the production-readiness master plan
(`local-docs/todo/todo-production-readiness-master.md` §6.5) — every
place that constructs a signed envelope on the testnet path calls
`sign_for_network(payload, Some("boole-testnet"))` and not the legacy
`sign(payload)` — is closed via the sub-slice migration plan recorded
below. This ADR fixes the binding scope of "testnet path" and lists
the call sites that participate in (and are excluded from) the
enforcement gate.

## Context

`crates/boole-core/src/signed_envelope.rs` exposes two signing entry
points on `SigningKeyV2`:

- `sign(payload) -> SignedEnvelope` — legacy entry point that calls
  `sign_for_network(payload, None)`. The resulting envelope has
  `network_id: None`, byte-identical to pre-P2.10 persisted
  signatures.
- `sign_for_network(payload, network_id) -> SignedEnvelope` — P2.10
  entry point that folds `network_id` into the signing digest via
  `DIGEST_NETWORK_TAG`, so a signature produced for one network
  cannot be replayed against another.

A workspace audit performed for slice 58 found six production
`.sign(` call sites that today produce `network_id: None` envelopes:

| File | Payload schema | Testnet-path? |
|---|---|---|
| `crates/boole-cli/src/main.rs:2140` | `boole.bounty.proof.v1` | yes |
| `crates/boole-cli/src/main.rs:2274` | `boole.bounty.announce.v1` | yes |
| `crates/boole-cli/src/main.rs:2385` | `boole.bounty.status.v1` | yes |
| `crates/boole-cli/src/main.rs:3399` | `boole.signer.work.v1` | yes |
| `crates/boole-cli/src/main.rs:3505` | arbitrary user payload (`keys.sign` CLI) | no |
| `crates/boole-miner/src/bounty_client.rs:124` | `boole.bounty.proof.v1` | yes |

The `boole-wallet-agent` `cmd_sign` site at
`crates/boole-wallet-agent/src/main.rs:127` calls
`ed25519_dalek::SigningKey::sign(&message)` and is not a
`SignedEnvelope` constructor; it is out of scope for this ADR by
construction.

`NetworkPreset::network_id()` in `crates/boole-cli/src/main.rs`
(lines 648-656) is the canonical source of the literal `"boole-testnet"`
(and `"boole-dev"`, `"boole-mainnet"`).

## Decision

The binding definition of **testnet-path signed envelope construction**
is: any production `SigningKeyV2::sign(...)` or
`SigningKeyV2::sign_for_network(...)` call that produces a
`boole.signed.v1` envelope whose payload schema is one of:

- `boole.bounty.proof.v1`
- `boole.bounty.announce.v1`
- `boole.bounty.status.v1`
- `boole.signer.work.v1`

regardless of which crate the call lives in
(`boole-node`, `boole-miner`, `boole-cli`, future bounty-emitting
crates).

Every in-scope call site MUST call
`sign_for_network(payload, Some(network_id))` where `network_id` is
resolved from the caller's `NetworkPreset` context (typically
`NetworkPreset::network_id()`). Legacy `sign(payload)` on the
testnet path is forbidden.

The `keys.sign` CLI site at `crates/boole-cli/src/main.rs:3505` is
**explicitly excluded** from this enforcement: it signs an arbitrary
user-supplied payload as a generic ed25519-over-canonical-JSON utility
and has no NetworkPreset context. It stays on the legacy `sign()`
path. Any future change that gives `keys.sign` a network preset
context must update this ADR and migrate the call.

### Migration sub-slice plan

P2.10 criterion 2 closes via the following sub-slice sequence. Each
sub-slice is one focused-test-driven RED → GREEN → focused gate →
full gate → commit → push cycle:

| Slice | Migration | File / site |
|---|---|---|
| 58 | This ADR + docs-smoke pin (doc-only) | `docs/adr/0003-*.md`, `scripts/docs-smoke.sh` |
| 59 | **Node-side first**: every `verify_signature(pk, signature, payload)` call site on a route that ingests a `boole.signed.v1` envelope (`crates/boole-node/src/local_node.rs` ≈8 sites) parses an optional `network_id` field from the outer body, dispatches to `verify_signature_with_network` with that value, and rejects mismatches against `LocalNodeConfig::network_id` with a typed `cross_network_rejected` envelope (closes criterion 3's server-side half). Backward-compatible: `network_id` absent in the body falls through to legacy `verify_signature(... None)`. | `crates/boole-node/src/local_node.rs` + node tests |
| 60 | **Client-side**: all five in-scope callers migrate to `sign_for_network(payload, Some(network_id))` and include `network_id` in the wire body. `--network <testnet\|dev\|mainnet>` becomes required on the affected CLIs (`boole-miner mine bounty`, `boole bounty submit\|announce\|status\|sign-work`). | `crates/boole-miner/src/bounty_client.rs`, `crates/boole-miner/src/cli.rs`, `crates/boole-cli/src/main.rs` |
| 61 | Workspace grep test (strict: zero in-scope `.sign(` calls; `keys.sign` site annotated with `// P2.10-exempt` per this ADR) + cross-network verifier rejection regression test that drives slice 59's server-side rejection through a real HTTP round-trip. | `crates/boole-core/tests/sign_scope_grep.rs`, `crates/boole-node/tests/cross_network_rejection.rs` |
| 62 | P2.10 criterion 1 — `scripts/smoke-testnet-faucet-to-block.sh` + `tests/fixtures/testnet-faucet-smoke/` transcript | scripts + fixture |

Sub-slice ordering rationale: the **node side must migrate first**.
If client-side `sign_for_network` lands before the node parses the
wire `network_id`, the node still recomputes the digest via legacy
`verify_signature` (no network binding), the digests diverge, and
every signed-envelope route fails with `signature_invalid` — a
disastrous regression. Slice 59 keeps backward compatibility by
falling through to legacy verification when `network_id` is absent
from the body, so client crates keep working until slice 60 flips
them. Slice 60 then lands the client-side switch atomically across
all five callers and starts wire bodies including `network_id`.
Slice 61 enforces the no-regression grep policy and adds an HTTP-level
cross-network rejection test. Slice 62 is the external-user e2e
script.

This binding plan supersedes earlier drafts (the originally landed
3-slice plan, the still-earlier 7-slice plan): closure needs four
src-changing sub-slices, capped by the actual node-side and
client-side scope discovered during slice 58 investigation.

## Consequences

- P2.10 criterion 2 closes against the strict zero-`.sign(`-in-scope
  invariant once slices 59-64 land. The §6.5 P2.10 closure block
  records each sub-slice commit SHA as it lands.
- The `keys.sign` exemption is binding. The slice-64 grep test
  enforces it via an in-file annotation (`// P2.10-exempt:
  user-utility, see ADR-0003`) so any future change that drops the
  annotation breaks the test.
- This ADR is doc-only; no Rust source changes ride with it. Self-test
  gate behaviour is unchanged.
- The migration sequence (slices 59-66) is strictly ordered: a
  lower-numbered slice MUST land before any higher-numbered slice in
  the same closure to keep the testnet-path enforcement gate
  monotonically tightening.
- Closure does not constitute a public-network claim. P2.10 is closed
  for closed-local testnet operability only; any public-testnet claim
  rides on Wave P3 release gating.
