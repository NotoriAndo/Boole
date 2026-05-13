# ReceiptCommitment

`ReceiptCommitment` is the replayable core commitment for a verified answer receipt. It records only verification-critical commitments and settlement identities; it does **not** store raw human answers, model text, prompts, or proof artifacts.

## Purpose

The commitment lets later node/store/audit slices point to the same verified-work result without reintroducing raw agent data on-chain.

Required fields:

```json
{
  "receiptId": "<sha256 canonical commitment fields>",
  "agentPk": "<hex32>",
  "familyId": "v1-lenbound",
  "verifierId": "lean-runner-v01",
  "verifierHashVersion": "v0",
  "artifactHash": "<hex32>",
  "requestHash": "<hex32>",
  "result": "accepted",
  "feeCharged": "1",
  "rewardRecipient": "<hex32>",
  "x402Version": "x402.draft-2"
}
```

## Boundary

- `receiptId` is computed from canonical JSON over the commitment fields except `receiptId` itself.
- `verifierHashVersion` is part of the ID preimage, so receipts created under verifier hash `v0` remain distinct from receipts created under `v1`.
- `x402Version` is optional for non-payment receipts and is part of the ID preimage when present. The mock `/verify-answer` flow pins `x402.draft-2` from `fixtures/protocol/x402/versions.json`; adding a new accepted version requires a fixture change, not silent code drift.
- `agentPk`, `artifactHash`, `requestHash`, and `rewardRecipient` must be lowercase hex32 strings.
- Unknown fields are rejected. In particular, fields such as `humanAnswer`, `rawAnswer`, `prompt`, or raw artifact bodies do not belong in this core commitment.

## Node storage/read surface

`boole-node run-local` can opt into a local ReceiptCommitment NDJSON store with either:

```bash
boole-node run-local \
  --receipt-commitment-ledger <receipt-commitments.ndjson>
```

or:

```bash
BOOLE_RECEIPT_COMMITMENT_LEDGER_PATH=<receipt-commitments.ndjson> boole-node run-local
```

When configured, the node serves:

- `GET /receipts/{receiptId}` — returns `{ "ok": true, "receiptCommitment": ... }` for a stored commitment.
- `POST /receipts` — local MVP append path for a `ReceiptCommitment` JSON object.
- `POST /verify-answer` — mock/local pay-before-verification path. Without `Payment-Signature: boole-native-test:paid`, it returns HTTP 402 `payment_required` with `scheme: "boole-native-test"`, `amount: "1"`, `requestHash`, `payTo`, and `x402Version: "x402.draft-2"`. With the valid fake payment header it returns a mock verified result and appends only the `ReceiptCommitment` row.

Unknown receipts return a typed `receipt_not_found` 404. Unsupported mock x402 versions return `x402_version_unsupported`. Raw answer fields such as `humanAnswer` are rejected and are not appended to the ledger.

## Agent passport primitive events

`ReceiptCommitment` rows can be projected into replayable primitive facts for a future agent passport indexer. This remains an event surface, not rich on-chain passport state.

Primitive event schema:

```text
boole.agent.event.v1
```

Current event kinds:

- `workAccepted` — `{ agentPk, familyId, receiptId }` for an accepted verified-answer receipt.
- `workRejected` — `{ agentPk, familyId, receiptId }` for a rejected verified-answer receipt.
- `rewardCredited` — `{ rewardRecipient, amount, reason }` for the accepted mock verify-answer fee credit.

`rewardCredited.rewardRecipient` is the receipt's pinned `rewardRecipient`, not a session key or caller-supplied temporary key. The mock `/verify-answer` response returns these primitive `agentEvents`, and the same event list can be reconstructed from the local `ReceiptCommitment` ledger.

## Focused local gate

The implemented wallet/session/receipt surface is covered by:

```bash
./scripts/wallet-session-receipt-gate.sh
```

The gate runs the focused `session_policy`, receipt, key/signing/session CLI, session store/route, submit session policy, receipt route, verify-answer route, and agent passport event tests. `./scripts/self-test.sh` also runs this gate before the workspace-wide clippy/test checks.

## Claim boundary

This type and route surface are local replayable evidence plumbing only. The `/verify-answer` flow is mock/local only and is not real x402 settlement. They do not mutate reward or reputation ledgers, verify signed-work lineage, or provide public-network mining evidence. It is not public-network mining evidence. Follow-on work should bind this commitment to audited receipts.
