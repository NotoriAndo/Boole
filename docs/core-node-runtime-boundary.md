# Core / Node Runtime Boundary

This document records the D4 boundary closeout for local runtime IO in Boole.

D4 status: CLOSED

D4 is closed when `boole-core` has no production runtime file/directory loader ownership for the moved stores/catalogs, `boole-node` owns those runtime loaders, and the preflight guards plus full self-test pass. New runtime-boundary work should start as a new slice rather than reopening D4 unless a regression reintroduces core-owned runtime IO.

## Ownership rule

- boole-core owns deterministic domain contracts, typed decisions, pure replay, schema/domain validation, and in-memory registries.
- boole-node owns local runtime IO: file reads, directory walking, NDJSON append/recover stores, boot-time recovery, and route/runtime integration.

The practical test is simple: if code must touch the local filesystem or process runtime to load operational state, it belongs in `boole-node`, not `boole-core`.

## Closed D4 boundary moves

- `FileBountyEventLedger`
  - `boole-core`: keeps `BountyEventLedger` plus `validate_bounty_ledger_event` as pure validation/domain logic.
  - `boole-node`: owns the file-backed `FileBountyEventLedger` append/recover store.

- Family manifest directory loading
  - `boole-core`: keeps `FamilyManifestRegistry` as an in-memory registry and keeps manifest parsing/validation.
  - `boole-node`: owns `load_family_manifest_registry_from_dir`, including directory walking, file reads, and skip-and-warn boot policy.

- Work manifest catalog loading
  - `boole-core`: keeps `WorkManifest`, `WorkManifestList`, and `work_manifests_from_list` for decoded schema/version validation.
  - `boole-node`: owns `load_work_manifests_from_path`, including local JSON file reads for the `/work` boot catalog.

- Bounty catalog loading
  - `boole-core`: keeps `Bounty`, `BountyList`, `BountyRegistry`, and `bounties_from_list` for decoded schema/version validation and domain mutation rules.
  - `boole-node`: owns `load_bounties_from_path`, including local JSON file reads for the `/bounties` boot catalog.

## Guarded non-goals

Do not reintroduce compatibility aliases or convenience wrappers such as:

- `boole_core::FileBountyEventLedger`
- `FamilyManifestRegistry::load_from_dir`
- `boole_core::load_work_manifests`
- `boole_core::load_bounties`

Those names make runtime IO look like core domain API again and blur the architecture boundary.

## Current verification

The boundary is guarded by `scripts/test_preflight_orchestration.py`:

- core must not export the node-owned file-backed bounty event ledger.
- core family manifest registry must not contain directory loading or filesystem reads.
- core work manifest module must not contain file loading or filesystem reads.
- core bounty registry module must not contain file loading or filesystem reads.
- node must export the runtime-owned loaders/stores listed above.

`boole-core` tests may still use `include_str!` for compile-time fixture embedding. That is test fixture embedding, not runtime filesystem IO.
