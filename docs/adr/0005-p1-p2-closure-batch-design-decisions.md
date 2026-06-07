# ADR 0005: P1.9 / P1.7 / P2.7 / P2.6 / P1.3b closure-batch design decisions

Status: Accepted (2026-06-04)

This batch closes five production-readiness rows (P1.9, P1.7, P2.7, P2.6,
P1.3b) of the master plan. Several rows are closed with an approach that
differs from the master plan's first-draft prescription. This ADR records
each deviation and why it is the simpler, safer closure, so a future
auditor reading the master plan does not flag the missing artifact (a
staging file, a side-pool snapshot, a compile-time feature gate) as
incomplete.

## P1.3b — re-derive-on-mismatch, not a `staging/commit-<height>.json` file

**Master plan (§4 L7):** "Block commit uses an intent record:
`staging/commit-<height>.json` is created with the full write set ... before
any store is appended."

**Decision:** close the "power loss mid-commit" / atomic-multi-store-commit
row with a **re-derive-on-mismatch heal** instead. The per-block write order
is nonce → block → reward → bounty-event → submit-receipt. The block store is
the single source of truth: every block fully determines its reward event, so
`boot_from_store_with_bounty_ledger` re-derives and appends any reward events
that trail the block store (the crash window between the block append and the
reward append), then re-runs `verify_ledger_matches_replay`. A genuine balance
tamper (a wrong amount in an *existing* event — the event count already
matches the block store, so nothing is re-derived) still bails.

**Why:** a write-ahead staging file would introduce a *second* source of truth
that can only diverge from the block store. Re-deriving from the canonical
store is strictly simpler and cannot drift. The same `derive_reward_event`
helper backs both the absent-ledger re-derive and the trailing-event heal.
Pinned by `tests/reward_ledger_crash_heal.rs` and
`scripts/test_multi_store_commit_ordering_contract.py`.

**Narrower bounty-event window — now closed (P1.3b follow-up).** The reward
heal above closes the *common* crash window (block append → reward append). A
crash one step later — after the reward append but before the per-row
`FileBountyEventLedger::append` calls in `submit_json` — leaves the bounty-event
ledger short of the last block's `credit` + `share_promoted` rows, and boot
bails on a bounty-family divergence (an unbootable node for a `--bounty-events`
operator who committed a promoted-credit block). The earlier blocker was that a
`credit`-only re-derive is consensus-unsafe: the paired `share_promoted` rows
carry a `proofHash` and include zero-credit shares that are absent from
`promoted_bounty_credits`, so a missing `share_promoted` row would leave
`rebuild_bounty_side_pool` treating an already-committed share as still-pending
and re-promotable — a *double-credit*.

The fix records the full re-derive input on the canonical store: each block now
persists `promoted_bounty_shares` (the `proofHash` carrier, including
zero-credit shares) alongside the existing `promoted_bounty_credits`. The full
expected ledger is, across **all** blocks in commit order, each block's `credit`
rows followed by its `share_promoted` rows; the file is append-only in exactly
that order, so whatever survived a crash — or a deleted ledger — is a strict
*prefix* of it. Boot re-derives that full sequence from the block store and
appends the missing suffix (`recover` first truncates any torn trailing line on
disk). This is the same append-only, prefix-heal-from-the-canonical-store shape
as the reward ledger, with no second source of truth, and it covers **both** the
trailing-last-block crash **and** a fully rebuilt (absent/deleted) ledger across
many blocks. `submit_json` writes the live `share_promoted` rows from the
now-persisted block field (not the in-memory selection) so the on-disk block and
the ledger share one source. A genuine tamper (count matches, value wrong) is
not a short prefix, so nothing is appended and `verify_ledger_matches_replay`
still bails.

`promoted_bounty_shares` is **not** part of `block_hash` (which hashes only
`prev_c` + `selected_share_hashes`), so the new field is node-local audit data
and changes no block identity or consensus outcome — pinned by
`bounty_event_crash_heal::block_hash_is_unchanged_by_promoted_bounty_shares_field`.
The single-block heal, idempotency, the `proofHash`-restore (double-promotion
guard), the prefix-torn-inside-one-block suffix heal, the multi-block
deleted-ledger rebuild, and the tamper-still-bails path are pinned by
`tests/bounty_event_crash_heal.rs`; the runbook row by
`recovery_playbook_matrix::row_bounty_event_crash_heals_on_reboot`.

*Upgrade note.* On the current binary, the operator recovery for a damaged
bounty-event ledger is simply to delete it and reboot — the prefix heal rebuilds
every block's rows. The one residual gap is blocks committed by the **pre-fix**
binary: they carry an empty `promoted_bounty_shares`, so their `share_promoted`
rows cannot be re-derived (their `credit` rows still can, from
`promoted_bounty_credits`). A ledger spanning such legacy blocks should be
rebuilt only after they have aged out, or accepted as missing the legacy
`share_promoted` audit rows.

*Integrity scope.* `verify_ledger_matches_replay` cross-checks `credit` rows by
per-family total but does not integrity-check `share_promoted` row contents; the
block store's `promoted_*` fields are likewise outside `block_hash`. The whole
bounty-promotion subsystem therefore trusts node-local storage — a process with
write access to these files is already in the trusted boundary. This heal closes
the *crash* window (missing rows); hardening against on-disk *tampering* of
bounty audit data is a separate, explicitly out-of-scope concern.

## P2.7 — `BountySidePool` durability via the bounty-event ledger, not a snapshot

**Master plan (§4 L8):** the SIGTERM drain should "persist `BountySidePool`".

**Decision:** the SIGTERM/SIGINT handler does **not** write a `bounty_side_pool.json`
snapshot. The side pool is a pure projection of the durable bounty-event ledger
and is rebuilt on the next boot (P1.5b `rebuild_bounty_side_pool`). The graceful
drain finishes in-flight requests (so any accepted proof's `kind="inserted"`
event reaches the durable ledger), and the boot rebuild restores the pool.

**Why:** same reasoning as P1.3b — a snapshot file is a second source of truth
that can diverge from the ledger. Closing P2.7 added only the missing piece: OS
signal handlers (`serve_local_node_with_os_signals`) that fire the existing
graceful-drain trigger. Ledger fsync (per-append), Lean-child reap
(`ChildKillOnDrop`), and state-dir flock release (RAII `Drop`) were already in
place. Pinned by `tests/shutdown_drain.rs`.

## P1.9 — runtime `--allow-insecure-verifier` opt-in, not a compile-time gate

**Master plan (§4 L0/L5):** "release builds do not allow mock-accept".

**Decision:** `boole-node run-local --lean-checker-disabled` refuses to boot
(exit 78 `insecure_verifier_config`) unless the operator also passes
`--allow-insecure-verifier`. The guard is a **runtime CLI-level opt-in**
(mirroring the P2.4 paid-API `--allow-paid-api` posture), not a
`#[cfg(not(feature = "dev-tools"))]` gate.

**Why:** a `cfg`-gated guard would only be compiled under `--no-default-features`,
which the `self-test.sh` gate never runs (clippy/test use default + dev
features). The guard — and its test — would never be exercised by CI. A
runtime opt-in is always compiled, gate-testable, and consistent with the
existing paid-API opt-in. The library `from_config` stays permissive so the
many node integration tests that legitimately use `lean_checker_disabled: true`
are unaffected; only the CLI refuses. The substantive soundness fix — the
forbidden-token scanner now rejects `sorry`, `axiom`, and `native_decide`
before `lake` is spawned — is always compiled and always tested.

## P2.6 — disk-full sentinel via a runtime `AtomicBool`, not a config field

**Decision:** the disk-full `/ready` precondition reads an
`Arc<AtomicBool>` on `LocalNodeState` (default `false`), surfaced as
`checks.disk_space_ok`. A `#[doc(hidden)]` `serve_local_node_with_disk_full_sentinel`
test seam injects it. No `LocalNodeConfig` field is added.

**Why:** a `LocalNodeConfig` field would force a one-line edit into ~50
struct-literal call sites across `boole-node` and `boole-cli` tests (the
config has no `Default` impl). The `AtomicBool` is also the natural home for
the production trigger: a future ENOSPC handler on the durable-append path
flips the same flag. That production wiring is a tracked follow-up; this slice
closes the readiness *reason* + its fault-injection test
(`ready_fault_injection::ready_returns_503_when_disk_full_sentinel_is_set`).

## P1.7 — deferred the 32 KiB cheap-route body cap

**Decision:** P1.7 ships the route-specific **timeout** matrix (30 s default,
90 s for `/bounties/{id}/proof`) with a typed `request_timeout` (408) envelope,
the proof-route **8 MiB** body cap (Content-Length + a per-route
`DefaultBodyLimit` override that wins over the global default), and
`spawn_blocking` Lean verification ("each verify on its own task"). The
master plan's 32 KiB cheap-read-route tightening is **deferred**.

**Why:** the 1 MiB default cap already bounds read-route bodies; 32 KiB is a
follow-up micro-tightening, not a safety floor. Shipping it would add a
per-route layer to ~15 read routes for marginal benefit. The route-specific
*matrix* (proof ≠ default for both timeout and body cap) is demonstrated and
fault-tested (`tests/http_fault_matrix.rs`).

## P1.6 — `/verify-answer` and `/submit` are structural exceptions to the signed-envelope model

**Decision:** the P1.6 per-route matrix requires every *mutating* route to carry
a `boole.signed.v1` owner envelope with per-signer nonce burn, `validBefore`,
and `network_id` binding. The six owner/agent/announcer/prover routes
(`/sessions` register + revoke, `/bounties` announce + status + proof,
`/receipts`) satisfy all five columns. Two routes are **exempted with a
documented rationale** rather than retrofitted:

- **`POST /verify-answer`** is *payment*-authenticated, not owner-authenticated.
  Its auth is the x402 `Payment-Signature` header over the canonical request
  hash, because the caller is a paying client, not the resource owner — an owner
  envelope is the wrong primitive. Payment replay/settlement protection is the
  remit of the x402 facilitator + payment nonce ledger in **P3.1**; the current
  build ships only a compile-time mock behind `dev-mock-payment` (see
  `docs/dev-mock-payment.md`), and a release build uniformly returns the typed
  `payment_invalid` envelope.
- **`POST /submit`** (direct PoW) is *permissionless* by design: the
  proof-of-work + canon verification is itself the admission gate, so there is no
  owner identity to sign or per-signer nonce to burn. The submission binds the
  current chain `c`; the session-bound submit path additionally burns a Submit
  nonce on commit (`submit_session_policy::rejected_admission_does_not_burn_submit_nonce`).

**Why exempt rather than force-fit:** adding a mandatory owner envelope to a
payment route or a permissionless-mining route would *break* those designs (a
paying non-owner client, and open PoW submission). This mirrors the P2.5
treatment of structurally-non-`Unified` CLI commands: a documented exception is
the correct closure, not a contortion. The burn-only-on-commit invariant for the
six signed routes is pinned by
`session_route::session_route_register_rejection_does_not_burn_nonce` (cell f: a
business-rejected register leaves the `(signerPk, nonce)` reusable) alongside the
replay/cross-network/auth matrices already in the suite.

## Process note — worktree overlap

The five slices were implemented and per-slice focused-gated in a git worktree
(`slices-batch`) while the P2.1 full gate ran in the main tree. The slices
touch `boole-node` / `boole-lean-runner` and are disjoint from P2.1's
`boole-mcp`-only change, so they merge cleanly. A single consolidated
`self-test.sh` full gate validates the union before push.
