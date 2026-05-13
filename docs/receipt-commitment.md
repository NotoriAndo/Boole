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
  "rewardRecipient": "<hex32>"
}
```

## Boundary

- `receiptId` is computed from canonical JSON over the commitment fields except `receiptId` itself.
- `verifierHashVersion` is part of the ID preimage, so receipts created under verifier hash `v0` remain distinct from receipts created under `v1`.
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

Unknown receipts return a typed `receipt_not_found` 404. Raw answer fields such as `humanAnswer` are rejected and are not appended to the ledger.

## Claim boundary

This type and route surface are local replayable evidence plumbing only. They do not mutate reward or reputation ledgers, verify signed-work lineage, or provide public-network mining evidence. It is not public-network mining evidence. Follow-on work should bind this commitment to audited receipts.
