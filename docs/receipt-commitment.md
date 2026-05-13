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

## Claim boundary

This type is a core data contract only. It does not by itself persist receipts, mutate ledgers, verify signed-work lineage, or provide public-network mining evidence. It is not public-network mining evidence. Follow-on work should add node storage/read routes and bind this commitment to audited receipts.
