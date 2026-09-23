# Native testnet launch readiness and operator rehearsal

Status: **PREPARATION ONLY — R1 incomplete, R2 not passed, R3 not started.**
This document organizes the remaining decisions and evidence for the selected
native base testnet. It grants no publication, signing, public-network, mining or
wallet authority. Current executable P2P and RPC remain numeric-loopback only;
there is no implemented public-exposure option. Do not use forwarding, a reverse
proxy or a tunnel to turn the local service into an externally reachable one.

The [current status](current-development-status.md) owns the roadmap, and the
[native contract](native-transfer-ledger-contract.md) owns protocol/accounting
details. Historical VM/product, useful-work and local replay evidence must not
be presented as a native public-network launch result.

## Selected initial scope

- Native `boole-native-testnet-1`, producer-only test issuance and fees, with
  useful-work rewards OFF. Existing legacy v3 remains separate; no balances or
  economic rights migrate from it or to any future mainnet.
- Explicit administratively approved transport-public-key membership. Every
  block/transfer remains independently validated; approval of a peer is not
  approval of its declared work, balance, head or data.
- **RPC stays local to each operator's node.** Use the existing numeric-loopback
  REST/CLI surface. Public RPC, submission gateway, faucet, remote diagnostic
  service, admin API, browser wallet and public WebSocket/SSE are excluded from
  this initial scope. Public P2P is a separate unimplemented/unauthorized boundary,
  not permission to expose RPC at the same time.
- A participant uses a separate encrypted spending vault and an unencrypted
  mode-0600 hot transport key. Release/recovery signing keys, transport keys,
  spending keys and work-session keys are different roles. Never enroll one key
  by copying a private key from another operator.
- Peer limit eight is **per configured node**, not a claim that eight independent
  operators were tested or a protocol-wide membership maximum. Proposed topology,
  independently operated participant count and producer roles must be recorded
  before the separately approved R3 experiment.

This narrower RPC scope avoids adding a public account/submission/management
surface merely because P2P participation is being prepared. A later public API
would need its own authentication, transport, abuse, privacy and operating plan.

## R2 evidence checklist — all applicable rows must close

The evidence owner fills a release-specific record with actual public references,
reviewer/approver identifiers and dates. A checkbox, local branch, schema-valid
file, source build or successful diagnostic query is not authority.

| Gate | Required record / acceptance | Current evidence and remaining gap |
|---|---|---|
| Scope and R1 acceptance | Exact exposed services, selected resource/abuse/recovery scenarios, unresolved risks and accepted operating envelope | Native ledger, TLS, wallet and recovery have scoped local evidence. Public P2P and broader mixed-load/other-host acceptance remain; no blanket R1 PASS |
| Network identity | Matched binary reports the intended network, rule/genesis and test-only monetary policy | Policy is pinned in source. Expected network is `boole-native-testnet-1`; genesis is `933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1`. A future release must independently bind and verify these exact values |
| Integrated source and CI | Merged source revision, required green CI, declared build features/toolchain and reproducible artifact provenance | Several hardening changes are local-only while GitHub publication is held. A locally passing downstream tree is not merged/released |
| Authenticated release | Exact supported artifact set, sizes/digests, authenticated release manifest, independent trust root and update/rollback floors | No operational signed native release is supplied. Existing Mac product packaging includes host binaries **and** controller/guest artifacts; it is not an already-qualified standalone native-only package |
| Custody and bootstrap trust | Actual authorized custodians/devices, role separation, independent publication channel, recovery/revocation procedure | [Custody plan validation](operational-key-custody-plan-v1.md) and non-production rehearsals exist. They prove neither real human/device independence nor operational signing authority. Do not invent identities or use deterministic test roots |
| Supported platform | Exact OS/architecture, fresh-install/update/rollback/recovery results for that release | Developer Mac/CI evidence is not clean-Mac CURL.3. That final installation test remains pending, or an explicitly narrower supported-platform decision is required |
| Participants and topology | Approved public key/endpoint mapping, reciprocal pin verification through an independent agreed channel, removal/rotation owner, topology and availability scope | Fixed-key loopback tests exist. External identities/endpoints and real operators are not supplied or enrolled. No TOFU, automatic key replacement or enrollment from a peer advertisement |
| Operational envelope | Planned duration/data volume, storage/disk headroom, observation cadence, incident owner/contact method, maintenance and stop criteria | Current bounded storage is 256MiB/100,000 blocks; full replay is still required. Developer large-state numbers are not a hardware SLA or permission to increase caps/prune history |
| Participant notice and recovery | Versioned technical notice delivered/acknowledged by actual participants; wallet/outbox/chain/transport recovery rehearsal and evidence handling | [Korean technical notice draft](native-testnet-participant-notice-ko.md) and [recovery runbook](native-operator-recovery-runbook.md) are prepared. No actual acknowledgements or legal-contract review are claimed |
| R3 execution authority | Explicit scope/time/participants, public endpoints, mining and test-wallet permissions, preregistered success/failure criteria and stop owner | Not supplied. R2 evidence by itself never starts nodes, opens a firewall, mines or moves a wallet balance |

Keep public record fields distinct from protected material: release/source IDs,
network/genesis, approved public peer and owner keys, expected heads, transaction
IDs, bounded diagnostics and evidence paths may be referenced. Never put a vault,
backup ciphertext, passphrase, transport private key or release secret in the
record, repository, issue, terminal transcript or support message. A public
address or signed transfer can reveal activity; share only the necessary scope.

## Participant preparation, without public activation

1. Read the technical notice and the exact release's supported scope. Verify the
   authenticated artifact and independent root through the selected release
   workflow; an old version label, repository checkout or download URL is not
   enough. Check matched `boole`, `boole-node` and `boole-wallet-agent` binaries.
2. Prepare a private dedicated directory/account and separate role keys only
   under the applicable key-generation/custody authorization. Follow the wallet
   and transport-key contracts; never paste private material into coordination.
3. Verify encrypted wallet backup/restore by exact retained public address, and
   retain passphrase separately. Preserve signed outboxes and an independently
   retained intended head alongside canonical archive recovery material.
4. Perform only the approved closed-local rehearsal first. Check network/genesis,
   actual balance/nonce/status, explicit non-finality, peer identity and ordinary
   readiness. `native diagnostics` explicitly does not check ledger readiness;
   `inspect-transfer` checks signature/format, not current payment status.
5. Stop and record results. External peer enrollment and startup wait for all
   applicable launch gates and explicit execution scope. This document deliberately
   supplies no public bind, firewall, proxy or public mining command.

## Incident, change and rollback ownership

Before an authorized run, name the human role that can stop the affected action,
preserve evidence and coordinate peers. Define an out-of-band contact path without
publishing secrets or assuming the affected node's API remains available.

| Observation | First safe boundary | Not an authorized shortcut |
|---|---|---|
| Unexpected transport key or release identity | Stop the affected connection/update and verify against the independently approved mapping/root | Do not replace a pin, trust first use or import a peer-supplied root |
| Unknown transfer outcome or reorg | Preserve exact outbox/ID, establish intended chain, query status and nonce; follow the recovery runbook | Do not generate a new payment/nonce to compensate for a timeout |
| Missing/corrupt state, ownership conflict or uncertain disk publication | Stop the affected action, retain the stopped original, diagnose and restore only into a fresh destination | Do not delete locks/journals, truncate to a convenient prefix or force a weaker fork |
| Busy/timeout or peer phase failure | Compare bounded process diagnostics with actual readiness and known work; preserve the observation | A phase label does not establish peer malice or disk corruption; do not auto-restart or revoke from that alone |
| Key compromise | Coordinate removal/replacement through the separately approved custody/peer process and retain evidence | Restoring a backup is not rotation; changing transport identity does not recover a spending key |
| Upgrade or rollback | Use only a qualified authenticated artifact and declared data compatibility, preserving original state/outboxes and security floors | A binary rollback is not a chain rollback; never edit balances, downgrade floors or re-sign old payments to make it work |

No native hot-upgrade or arbitrary historical-binary compatibility is asserted.
Stop normally and wait for actual in-flight publication to finish before changing
the executable or state owner. Unstarted peer lock waits are cancellable; running
validation/disk work is not a hard real-time shutdown guarantee. Reopen and audit
the intended head after any selected recovery. Near-limit storage is an explicit
capacity/upgrade decision, not permission to silently raise caps or prune.

## Closed-local three-process rehearsal — preregistered scope

This tests an existing operating workflow; no artificial failing product test is
required. Use one developer host, three real node processes, three disposable
encrypted owner wallets and three separate transport keys. Bind only numeric
loopback and use the actual CLI/agent/RPC/TLS paths. No VM/model/paid API, operator
key/fund, public endpoint, artifact publication or release ceremony is involved.

- A mines ten ordinary test blocks. All three processes agree on the same head
  and public network/genesis. A signs/saves/sends 2 tBOOLE to B; all nodes see the
  same pending ID. Repeating the saved signature through another node is still
  one transfer. B mines height 11. B's restored encrypted vault then signs 1.5
  tBOOLE to C, and C mines height 12; compare inclusion and recipient balances.
- Stop/restart C without peers to create an actual isolated process. A/B extend
  to height 14 while C signs 0.25 tBOOLE to A and includes it at its own height 13.
  Preserve C's outbox and verify its locally confirmed, explicitly non-final ID.
- Rejoin C with the exact original approved transport key/configuration. A/B's
  greater-work branch must win; the orphaned C transfer becomes pending again,
  not a second payment. A mines height 15 and every process confirms that same ID
  there. There must be exactly three canonical transfers and nonce 1 for each
  sender; no orphan reward or duplicate debit survives.
- Stop every process normally. Independent offline audits at the same retained
  final head must agree: 15 blocks, 750,000 tBOOLE issued/balances, 3.75 tBOOLE gross
  transfer amount and 3,000 fee atoms. Expected owner balances are A 649,998.25,
  B 50,000.5 and C 50,001.25 tBOOLE. Check reward locks and spendable amounts too.
- Export from stopped A, import into a new B recovery directory with the retained
  head, and compare the independent audit. Restart that restored process with B's
  original separate transport key. Query/re-submit B's original saved transfer:
  it remains confirmed at 12, with unchanged balances/nonce and no new signature.
  Original node bytes, wallet backup and every outbox remain preserved.
- Each small-state convergence/readiness phase is bounded to 15s, each normal
  process stop to 3s, and the whole operating scenario to 180s after the necessary
  sibling-binary build. Record the first result and any fixture failure; do not
  relax the criteria or retry unchanged code after an acceptance failure.

Even a pass would be one-host process integration, not three independent human
operators, public network/latency behavior, a supported release/install result,
hardware capacity qualification, finality, R2 or R3 completion.
