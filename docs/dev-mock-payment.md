# Development-only mock payment (`dev-mock-payment`)

> **⚠️ DEVELOPMENT-ONLY. THIS IS NOT A PRODUCTION PAYMENT PATH.**
>
> The `/verify-answer` route ships with a compile-time **mock** payment check
> used only for local development and tests. It accepts a hard-coded magic
> header string instead of verifying a real payment. It is gated behind the
> `dev-mock-payment` Cargo feature, which is **NOT** in any default feature set,
> so a normal release build (`cargo build -p boole-node`) never compiles it in.
> Real payment settlement is tracked as the x402 facilitator work in Wave P3
> (P3.1), not delivered here.

## What the mock is

`crates/boole-node/src/local_node.rs` defines, **only** under
`#[cfg(feature = "dev-mock-payment")]`:

```rust
#[cfg(feature = "dev-mock-payment")]
const VERIFY_ANSWER_PAYMENT_SIGNATURE: &str = "boole-native-test:paid";
```

When the feature is enabled, `enforce_verify_answer_payment` compares the
incoming `Payment-Signature` header against this magic string and lets the
request through on a match. This exists so the local smoke (`/verify-answer`
round-trip) can run without a real payment facilitator.

## What a release build does

Without `dev-mock-payment`:

- The `VERIFY_ANSWER_PAYMENT_SIGNATURE` constant **is not compiled at all** — it
  is not present in the binary, so it cannot be "discovered" and replayed by a
  caller.
- `enforce_verify_answer_payment` is the `#[cfg(not(feature = "dev-mock-payment"))]`
  variant, which uniformly returns `payment_invalid` regardless of the header.
  There is no magic string to match, so a forged `Payment-Signature` header is
  always rejected.

This structural guarantee is pinned by
`scripts/test_verify_answer_payment_gate_contract.py` (the magic constant and
its comparison are both annotated `#[cfg(feature = "dev-mock-payment")]`, and
`dev-mock-payment` is asserted absent from the crate's default features).

## Claim boundary

This mock is a closed-local development affordance only. It is **not** evidence
of a working payment system, **not** a production x402 integration, and must
never be cited as such. Real pay-before-verify settlement lands with the x402
facilitator interface, quote endpoint, and payment nonce ledger in Wave P3.
