# ADR 0004: P0.5 telemetry subscriber, request-id propagation, panic hook, and /metrics

Status: Accepted (2026-05-31)

P0.5 observability of the production-readiness master plan (§4 L8). This
ADR fixes the scope and sub-slice sequence so the work lands as small,
independently-gated commits rather than one large observability rewrite.

## Context

The current telemetry surface (`crates/boole-core/src/telemetry.rs`) is
the minimum landed in `00b0159` / the P0.5-boot slice:

- `telemetry::init(BinaryName)` exists and is idempotent (`Once`).
- It emits a single boot line **only** when `BOOLE_TELEMETRY_BOOT=1`,
  to preserve the stderr-clean contract that node/cli integration tests
  assert on.
- Only `boole-node`'s `main` calls it today (`boole-miner`, `boole-cli`,
  `boole-mcp` do not).
- There is no `tracing` subscriber, no `request_id` propagation, no
  panic hook, and no `/metrics` counter wiring beyond the existing
  hand-rolled `/metrics` text body in `local_node.rs`.

The L8 contract (master plan §4) requires:

1. `telemetry::init(BinaryName)` called from `main` of **every** binary;
   JSON-formatted `tracing` events to stderr by default; `RUST_LOG`
   honoured; `NO_COLOR` honoured.
2. Every HTTP handler `#[instrument(skip(state, body), fields(request_id))]`;
   `request_id` generated in middleware, copied into the response
   envelope and downstream ledger lines.
3. `/metrics` Prometheus counters: `boole_submits_total{outcome}`,
   `boole_proofs_total{outcome,reason}`, `boole_payments_total{outcome}`,
   `boole_lean_verify_duration_seconds`, `boole_replay_match{layer}`,
   `boole_panic_total`, `boole_active_locks`.
4. Panic boundary: release `panic = "abort"` with a `std::panic::set_hook`
   that emits a final structured line and bumps `boole_panic_total`
   before the process aborts. Debug/test keep unwind.

## Decision

### Constraints that shape the sequence

- **Stderr-clean contract.** Many node/cli integration tests assert a
  clean stderr (or parse stdout JSON). The default subscriber level must
  not break them. The subscriber is installed at a level that is silent
  by default (`RUST_LOG` unset → `error` only, and boot/info lines stay
  behind the existing `BOOLE_TELEMETRY_BOOT` gate) so existing tests keep
  passing. A test that wants to observe telemetry sets `RUST_LOG`.
- **New dependency.** `tracing` + `tracing-subscriber` enter the
  workspace. This is a Cargo.lock churn and a `cargo deny` advisory
  surface, so the dependency lands in its own slice with the deny/audit
  gate run, before any handler is instrumented.
- **No consensus change.** Telemetry is strictly additive: it observes,
  never alters admission/commit/replay. Each slice must leave
  `replayMatchesRuntime` and every existing test byte-identical except
  for added observability assertions.

### Sub-slice sequence (each: RED → GREEN → focused gate → full gate → NotoriAndo commit → push)

| Slice | Scope | File / site |
|---|---|---|
| 63 | This ADR + docs-smoke pin (doc-only) | `docs/adr/0004-*.md`, `scripts/docs-smoke.sh` |
| 64 | Add `tracing` + `tracing-subscriber` deps; `telemetry::init` installs a JSON subscriber (stderr, `RUST_LOG`-driven, `NO_COLOR`-honoured, default-silent). Idempotent via the existing `Once`. cargo-deny/audit re-run. No handler instrumentation yet. | `crates/boole-core/Cargo.toml`, `telemetry.rs`, deny config |
| 65 | Call `telemetry::init(BinaryName::{Cli,Miner,Mcp})` from the three mains that omit it; extend `BinaryName` with `Mcp`. Pin via a contract test that every binary main references `telemetry::init`. | `crates/boole-{cli,miner,mcp}/src/main.rs`, `telemetry.rs` |
| 66 | `request_id` middleware on `boole-node`: generate a per-request id, attach to a `tracing` span, echo it in the response envelope (`requestId` field) and every ledger line written during that request. | `crates/boole-node/src/local_node.rs` + node tests |
| 67 | `/metrics` typed counters: replace/extend the hand-rolled text body with the L8 counter set incremented at the real outcome sites (`boole_submits_total{outcome}`, `boole_proofs_total{outcome,reason}`, `boole_panic_total`, …). | `crates/boole-node/src/local_node.rs` + metrics test |
| 68 | Panic hook: `std::panic::set_hook` emitting a structured final line + `boole_panic_total` bump; release profile `panic = "abort"` confirmed. Debug/test keep unwind so `catch_unwind` tests still work. | `telemetry.rs`, workspace `Cargo.toml` profile, node main |

Ordering rationale: the dependency and subscriber (64) must exist before
anything emits structured events. The all-mains init (65) is independent
and low-risk, so it lands second. `request_id` (66) and `/metrics`
counters (67) are the observable payload and depend on the subscriber.
The panic hook (68) lands last because the release `panic = "abort"`
profile flag interacts with the whole workspace and is the highest-blast
change.

### What stays out of P0.5

- OpenTelemetry / OTLP export, sampling, distributed tracing across
  processes — deferred to a future observability milestone.
- A `secret_types::*` module + `cargo deny` lint forbidding
  `#[derive(Debug)]` on secret types. P0.8 already redacts the known
  secret types by hand (commit `833dc35`); the lint is a belt-and-
  suspenders hardening that rides a later slice, not P0.5.

## Consequences

- P0.5 closes against the L8 contract once slices 64–68 land and the
  §6.5/observability checklist is green on a fresh full gate.
- Closure is not a public-network or production-deployment claim; it is
  the local observability contract only.
- The `BOOLE_TELEMETRY_BOOT` gate and the stderr-clean test contract are
  preserved throughout: the default subscriber is silent unless
  `RUST_LOG` opts in.
- This binding plan supersedes any earlier informal P0.5 sketch: closure
  needs five sub-slices (64–68) capped by the actual subscriber,
  all-mains, request-id, metrics, and panic-hook scope.
