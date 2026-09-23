# Native operator recovery runbook

Status: **PREPARED — closed-local CLI rehearsal passed.** This is a recovery
procedure for the implemented native testnet, not public-testnet launch or
operator-fund authority. Use the [current status](current-development-status.md)
for release/CI availability and the
[native contract](native-transfer-ledger-contract.md) for exact limits.

The examples require matched, trusted `boole`, `boole-node` and
`boole-wallet-agent` binaries containing these commands. Some work is still
local-only; neither this document nor an old version number proves a released
binary includes it. Do not download an unverified binary or replace a live
installation merely to follow this guide. All current RPC/P2P addresses must be
numeric loopback; public addresses, proxies, remote forwarding and new exposure
options are outside this procedure.

## 1. Preserve independent recovery material

Keep these separately, with access appropriate to each item:

- The authenticated encrypted owner-vault backup and its independently retained
  public address. The passphrase is separate; the software cannot recover a
  missing passphrase or spending key. A backup is not a rotated key.
- Original signed outbox files, including pending/uncertain transactions. They
  contain public spending instructions/signatures, not the owner private key,
  but may reveal activity and can be rebroadcast.
- A verified canonical block archive and an independently recorded expected
  head, network/genesis, timestamp and source. Copying a head from the same
  untrusted archive does not establish freshness or the intended history.
- The entire stopped original node directory when diagnosing damage. Do not
  prune, rename individual journals, delete manifests or truncate files to make
  boot/audit succeed. Preserve original bytes and choose a fresh restore path.
- The separate 0600 transport private key and approved peer-key/address list.
  These are not in the chain archive, not spending keys and not replaced by
  restoring a wallet. A compromised transport key requires coordinated removal
  from the allowlist; do not silently trust a replacement peer key.

The archive contains **canonical blocks only**. It excludes pending-only
transfers, wallet vaults/passphrases, outboxes, transport keys and peer policy.
A canonical restore can be complete while some saved transfers remain unknown.

## 2. Stop and establish the intended source

Stop the exact foreground node normally and wait for process exit. Do not use
broad process-name kill commands. Audit/export requires exclusive state
ownership. A busy-lock error is a reason to locate the owner, not remove locks.

Normal native shutdown closes new mutation/peer admission immediately and gives
existing HTTP client I/O up to five seconds to drain before closing its sockets.
Already admitted validation or durable work still finishes under its existing
barrier; this is not a five-second bound on CPU/disk work or process exit. A
one-second final state-reference cleanup reports an error if another reference
remains. Do not force-restart, remove locks or interpret a truncated response as
a receipt. Wait for the actual old owner to finish and reconcile the original
transaction ID. [Executed shutdown regressions and limits](native-http-shutdown-drain-2026-09.md)
cover delayed input, an unread response, retained ownership and an admitted block.

Record `native info` and relevant transaction IDs before stopping if the node
is still trustworthy and responsive. A previously retained head or independently
verified healthy replica is necessary to detect a valid-prefix rollback. The
audit does not prove that no later history existed, and `confirmed` is not finality.

If ordinary queries time out, return busy, or fail readiness, use the separate
read-only diagnostic before stopping, if the process is still reachable:

```sh
boole native --node http://127.0.0.1:8383 diagnostics
```

Its `ledgerReadiness` is always `not_checked`: success is **not** permission to
send, mine, trust balances or replace a healthy-state check. Inspect `rpc` request
counts/limits, `stopping` and `peers.peers[].lastFailureStage` without waiting for
the ledger lock. Two small diagnostic slots are separate from the eight ordinary
request slots, but all share the bounded connection pool; a dead process or fully
occupied host can still be unreachable. Do not restart automatically from a phase
label or busy counter. Preserve evidence and follow the affected-state procedure.

Examples below use deliberately chosen absolute paths. The source must already
exist. Archive output must be new, outside that source and in an existing direct
non-symlink directory that is not group/world writable. The restore directory
must be a new path, not the only remaining copy of damaged state.

```sh
boole-node native-audit --state-dir /private/native-original \
  --expected-head EXPECTED_64_LOWERCASE_HEX_HEAD

boole-node native-export --state-dir /private/native-original \
  --output /private/native-backups/new-chain.ndjson
```

Compare the audit/export network, genesis and head to the intended values.
Keep returned receipts with the recovery record. These commands preserve source
journal/manifest bytes, but may create/open ordinary ownership lock files. They
do not repair a torn tail, recreate a lost canonical journal or certify a stale
yet internally valid history. If they fail, preserve the source and seek another
verified archive/replica; never lower a bound or delete a destination to retry.
They also reject source history, pending or manifest versions changed after
being read during replay. A file-change error is not permission to restore the
earlier in-memory bytes or erase the modified file. Preserve the source, stop
other writers and investigate before opening a separate verified recovery copy.

## 3. Restore canonical history into a fresh directory

```sh
boole-node native-import --state-dir /private/native-restored \
  --blocks /private/native-backups/new-chain.ndjson \
  --expected-head EXPECTED_64_LOWERCASE_HEX_HEAD

boole-node native-audit --state-dir /private/native-restored \
  --expected-head EXPECTED_64_LOWERCASE_HEX_HEAD
```

Import verifies every block from genesis and uses ordinary fork choice. It is
not a forced rollback or balance checkpoint. Wrong head, bad signature/PoW,
foreign genesis, malformed/oversized/nonregular input fails; invalid source
verification does not create the destination. A later disk/publication failure
can leave a fresh destination partly initialized or fully committed: an absent
success receipt means inspect/recover, not assume success or overwrite it.

At the same intended head, compare source and restored `accounting` and
`confirmedTransfers`, network/genesis, height and head. Audit money/height fields
are decimal strings; archive height is a bounded JSON number. `issuedAtoms`
must equal `balanceAtoms`, be at most `supplyCapAtoms`, and equal locked plus
spendable atoms. Gross transfer/fee totals are not issuance or unique economic
volume. Pending counts/bytes can legitimately differ because pending is not in
the archive; do not require those resource fields to match in a general restore.

Keep the original directory, archive and outboxes after comparison. Do not start
the old and restored paths as competing copies of the same intended service.

## 4. Restore the owner vault without exposing its passphrase

Wallet `backup`, `restore` and `address` read one passphrase line from stdin.
Do not put it in argv, a shell command literal, logs, exported environment or a
support transcript. The CLI's stdin option is not a promise that a terminal
automatically disables echo. Use an approved secret-input mechanism.

For a dedicated interactive **Bash** recovery shell with a private TTY, this
optional helper disables tracing/echo and keeps the value out of child argv and
environment. Use trusted matched sibling binaries and clear stale wallet-agent
overrides in this dedicated shell. The temporary shell variable is unset, but
this does not claim complete process-memory erasure or protection from a
compromised shell/debugger/host.

```bash
unset BOOLE_WALLET_PASSPHRASE BOOLE_WALLET_AGENT_BIN
boole_with_passphrase() {
    set +x
    local boole_recovery_passphrase boole_command_status
    if ! IFS= read -r -s -p 'Vault passphrase: ' boole_recovery_passphrase </dev/tty; then
        printf '\n' >&2
        unset boole_recovery_passphrase
        return 1
    fi
    printf '\n' >&2
    printf '%s\n' "$boole_recovery_passphrase" | "$@"
    boole_command_status=${PIPESTATUS[1]}
    unset boole_recovery_passphrase
    return "$boole_command_status"
}

boole_with_passphrase boole wallet restore \
  --backup /private/wallet-backups/owner.backup.json \
  --vault /private/recovered-owner.vault --json
```

Require a success result and exact equality of `result.address` to the
independently retained owner address before using the restored key. Wrong
password, changed ciphertext, unsupported KDF cost or an existing destination
is not a reason to rewrite the backup, lower KDF settings or delete an existing
vault. Keep both original encrypted material and backup. Do not paste either
encrypted private-key file or the passphrase into issue/PR/chat diagnostics.

Before an incident, the same helper can create an authenticated new backup:

```bash
boole_with_passphrase boole wallet backup \
  --vault /private/owner.vault \
  --output /private/wallet-backups/new-owner.backup.json --json
```

The public address and backup receipt are not spending-key recovery substitutes.
Mnemonic import, OS keychain, key rotation and passphrase recovery are not
implemented by this path.

## 5. Start locally and reconcile every saved outbox

Start the verified restored node in the foreground, initially without peers so
its expected snapshot is easy to compare:

```sh
boole-node run-native-local --state-dir /private/native-restored \
  --addr 127.0.0.1:8383
```

In another terminal, check `info` for the exact network/genesis/head and ready
state, then inspect each outbox **without signing or broadcasting**:

```sh
boole native --node http://127.0.0.1:8383 info
boole native inspect-transfer --file /private/outbox/saved-transfer.json
boole native --node http://127.0.0.1:8383 transaction --txid DERIVED_64_HEX_TXID
boole native --node http://127.0.0.1:8383 account --pk OWNER_64_HEX_PUBLIC_KEY
```

Inspection derives the ID and verifies signature/format only. Its
`chainStatus=not_checked` is deliberate: an expired, unfunded or stale-nonce
transaction may still have a valid signature. Compare its `from`, `to`, exact
amount/fee atoms, nonce and inclusive `validBefore` to the intended payment.

- `confirmed`: verify intended inclusion, recipient balance and owner nonce.
  Do not create a replacement. The `final` field remains false; later reorgs can
  change status. Repeating the same signature cannot create a second debit, but
  routine rebroadcast of an already-confirmed transaction is unnecessary.
- `pending`: the signature is retained locally but not yet in the canonical
  chain. Preserve it; an outbox is not a confirmation receipt. Do not create a
  new nonce automatically because the old response was slow or lost.
- `unknown`: this does not prove the transfer never executed or is safe to
  replace. Archives exclude pending-only transfers, and a restored/stale/forked
  node can disagree with another node. Establish the intended current chain,
  review owner nonce/expiry and reconcile any other saved transaction first.

Only after reconciliation and the applicable owner authorization, resubmit the
exact saved file if that is the selected action:

```sh
boole native --node http://127.0.0.1:8383 submit \
  --file /private/outbox/saved-transfer.json
```

No wallet unlock or new signature is needed. A timeout/connection loss can have
an unknown outcome; the node may still complete its admitted mutation. Query
the same ID again before any further action. Expiry, nonce conflict, insufficient
funds and a full queue are distinct from signature validity; none authorizes an
automatic new payment. A new intentional transfer uses a **new** outbox path,
the recovered node's current nonce and explicit owner signing authority.

After local checks, restore only the deliberately approved peer-key list and
separate transport key. Stop/restart to change that configuration; never accept
an unexpected replacement key by trial-and-error. Inspect `boole native peers`
for successful snapshot agreement and repeated failures. Peer agreement is an
observation, not finality, public-connectivity certification or launch approval.

For `retrying`, inspect `lastFailureStage` together with failure counts and the
last chosen retry interval. `connect` directs attention to the intended local
process/address; `tls_handshake` to both administratively approved endpoint pins
and transport-key setup. Never bypass a pin to make the error disappear. `hello`
requires checking the expected binary/network/genesis configuration, without
adopting the peer's declared chain as authority. `local_state` can also mean the
local ledger lock wait exhausted its round deadline. Preserve the state and
investigate contention, ownership, readiness and storage before deciding on
recovery; this label alone does not prove damage.
`local_snapshot` can be an ordinary race with another successful update; a single
occurrence is not evidence of corruption. Block/fork/pending stages locate data
exchange, validation or publication work; they do not alone establish bad remote
data, an attack, or a broken local disk. See the
[complete phase vocabulary](native-transfer-ledger-contract.md#mutually-authenticated-native-peers).
The value describes the last completed failure, remains during an in-progress
retry and clears to `null` on a successful round. It intentionally includes no
raw error strings or peer-supplied diagnostics. Shutdown can count as a failure;
do not rotate keys, erase journals or repeatedly rebroadcast based only on a
counter or phase label.
When `native peers` cannot pass readiness, the same monitor is available under
`peers` in `native diagnostics`; this does not declare the ledger ready or repair it.

## 5a. Retire a transport identity and explicitly approve its replacement

Use this only under the applicable incident/key-change authority. Membership is
local configuration, not an on-chain revocation registry. Every node that still
loads the old pin can still trust that identity; one operator's change is not
network-wide revocation. Name the change owner and independently approved
old/new public-key-to-endpoint mapping, and record which operators have actually
completed their changes. An unexpected phase failure alone does not authorize
rotation.

1. Preserve the intended network/genesis/head, public peer mappings and relevant
   bounded diagnostics. Stop the affected identity's node and each trusting node
   that must change its allowlist, and wait for actual process exit. Existing
   authenticated connections and loaded private-key material are not revoked by
   editing a file, deleting it or replacing a path while the process runs.
2. Keep the original state, outboxes, owner wallet and old transport evidence
   access-controlled. Do not rotate the spending vault, reset the ledger or
   delete the old private file to make a connection failure disappear. A
   compromised transport key remains sensitive even after its pin is removed.
3. Under key-generation authority, generate a distinct replacement file at a
   new path in a private directory. `peer-keygen --file NEW_ABSOLUTE_PATH`
   refuses overwrite and returns only public identity/path information. Do not
   copy another operator's private key or enroll from an unauthenticated peer
   advertisement. Verify the new public ID through the separately approved
   administrative channel; a channel authenticated only by the compromised old
   identity is not independent verification.
4. Remove the old pin from every affected node's `--peer KEY@ADDRESS` mapping.
   Configure the replacement node's new `--peer-key` path and its independently
   approved counterpart pins. Restart the deliberately selected configurations
   at their existing state directories. Generating or starting a new key does
   not enroll it at other nodes, and a successful one-sided configuration change
   is not reciprocal membership. Record any node not yet updated as incomplete.
5. Check `native peers` for the exact local public ID and approved remote IDs,
   successful rounds and the intended head through ordinary ready-state queries.
   A remaining healthy topology may continue within its authorized scope while
   the affected identity is excluded; this is not a general availability promise.
   Reconcile saved transaction IDs and owner nonce before any retry. Changing
   transport identity neither changes coin ownership nor invalidates otherwise
   valid historical block/transfer signatures.
6. After the selected recovery, stop and audit at the independently established
   intended head when required by the incident plan. Preserve old/new protected
   material and the completed mapping record. Do not reuse or disclose a
   compromised operational private key merely to run a negative probe. The
   automated retired-key probes below use disposable fixture keys only.

The [three-process retirement/re-enrollment regression](native-peer-key-rotation-2026-09.md)
verifies active old-connection closure, old and unapproved-new key refusal,
healthy A/C transfer progress, explicit replacement membership, preserved state
and two exactly-once transfers with equal independent audits. It does not add
hot reload, a global membership authority or external enrollment approval.

## 6. Stop conditions and evidence to retain

Stop the affected action and preserve original material if there is a head/
address/genesis mismatch, signature failure, missing canonical file, torn or
malformed source, ownership conflict, unexpected key/permission change, disk
publication failure, or a result that cannot be classified. Do not bypass a
guard, force a weaker fork, erase a failed destination, or run repair on the only
copy. If a command partly published before failing, its absence of a success
receipt must remain recorded.

Record binary/source identity, network/genesis, intended head, public addresses,
transaction IDs, redacted command arguments, exit status, public receipts and
which source/destination paths were preserved. Do not record passphrases,
plaintext seeds, transport private-key bytes or raw vault/backup contents.
The runtime also checks input/output file versions across validation and durable
publication. A detected change can return an error after a block or transfer was
already written. Preserve all files, stop/recover the affected node and reconcile
the exact saved transaction ID; do not infer rollback or sign a replacement just
because the call failed. The failure fence is not an atomic multi-file rollback.
Keep access-controlled recovery artifacts separately from ordinary diagnostics.

The existing bounds still apply: 256MiB/100,000 canonical blocks, 512 pending
transfers, bounded recent online forks and full replay on startup/audit. Large
developer scenarios currently take about 52–55 seconds to replay; that is not a
hard deadline on other hardware. A valid node reaching its storage limit needs
an explicit future storage/upgrade plan, not unreviewed pruning or a larger cap.

## Closed-local evidence and launch exclusions

The expanded [owner CLI integration test](../crates/boole-cli/tests/native_cli.rs)
passed on its first run in 65.68s (including its nested sibling-binary build).
It uses disposable keys, vaults, loopback processes and actual PoW/owner-signed
blocks: encrypted backup → original vault set aside → restored-owner transfer →
primary process crash/restart → stopped-source audit/export → fresh archive
import → equal audit → recovered node start → old outbox ID/status/retry with
no second debit → a new explicit nonce-1 transfer from the restored vault.
The recipient moves from 1.25 to 1.75 tBOOLE only after that new 0.5 transfer.
A stopped-node replay audit then reports height 12, two confirmed transfers,
175,000,000 gross amount atoms and 2,000 fee atoms. Original node journals/
manifest and the old outbox remain byte-identical. The shell helper above was
syntax-checked, not exercised with an actual operator passphrase.

A later diagnostic extension passed the same end-to-end sequence in 60.56s after
an initial fixture failure (8.80s): restoring a renamed file's bytes did not restore
its recorded ctime, so the live node correctly stayed unavailable. The corrected
rehearsal queries process-only diagnostics, confirms ordinary info still fails,
preserves the original bytes and explicitly restarts before continuing. The node's
file-change/readiness guards were not weakened to pass the test.

This prepares one selected recovery workflow. It does **not** pass R2 or R3.
Use the [release/participant readiness checklist](native-testnet-readiness.md)
and [Korean technical notice draft](native-testnet-participant-notice-ko.md)
to distinguish prepared local recovery from actual onboarding and launch authority.
The checklist's later three-process rehearsal passed twice, including actual
partition/rejoin, same-ID orphan reconfirmation, three matching offline audits
and fresh-node/previously-restored-wallet recovery without a duplicate payment.
It remains one-host loopback evidence, not independent external operation.
Required launch decisions/evidence still include applicable R1 abuse/resource
acceptance and public exposure scope, versioned authenticated release artifacts,
operational signing/key custody, supported-platform qualification (clean-Mac
CURL.3 remains pending or requires an explicitly narrower platform scope),
participant risk/onboarding, incident/upgrade/rollback ownership and explicit
public-network/mining/test-wallet authority. Test coins have no real-money or
future-mainnet entitlement. Useful-work rewards/activation remain OFF/HOLD;
no actual model run, public node, operational mining or operator fund movement
was performed by this rehearsal.
