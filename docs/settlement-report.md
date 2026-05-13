# Settlement report

`boole chain settlement-report` is the operator-facing read-only summary for audited submit receipts.

It answers a narrower question than the full receipt auditor:

- `audit-receipts = full auditor report`
- `settlement-report = read-only reward/reputation summary`

Use it when you already have a persisted block NDJSON log and a submit receipt NDJSON ledger, and you want the reward/reputation deltas implied by receipts that pass the same block/replay audit.

## Command

```bash
boole chain settlement-report \
  --blocks <blocks.ndjson> \
  --receipts <submit-receipts.ndjson> \
  --json
```

The command first runs the same receipt/block/replay checks as `boole chain audit-receipts`. It does not bypass audit logic.

## Output shape

```json
{
  "ok": true,
  "source": "audit-receipts",
  "blocksChecked": 2,
  "receiptsChecked": 1,
  "settlement": {
    "rewardCredits": [
      { "pk": "<rewardRecipient>", "amount": "1" }
    ],
    "reputationDeltas": [
      {
        "agentPk": "<submittedBy>",
        "acceptedSubmits": 1,
        "verifiedRewardAmount": "1"
      }
    ],
    "checks": {
      "rewardCreditsReplayBound": true,
      "reputationBoundToSubmittedBy": true
    }
  }
}
```

## Identity rules

Keep these identities separate:

- `rewardRecipient` is the reward sink. It becomes `settlement.rewardCredits[*].pk`.
- `submittedBy` is the session/work submit identity. It becomes `settlement.reputationDeltas[*].agentPk`.
- `proposerPk` and selected share pks are mining/proof identities. They are not the default reputation identity.

This matters for session-bound work where the accepted proof/mining identity can differ from the fixed reward recipient.

## Failure behavior

`settlement-report` is fail-closed.

If receipt shape, block binding, selected share binding, or reward replay binding fails, the command exits non-zero, writes typed JSON to stderr, and writes no settlement JSON to stdout.

In short: audit failure suppresses settlement output.

Example failure detail includes both the suppression marker and the underlying audit reason:

```json
{
  "ok": false,
  "reason": "internal_error",
  "detail": "settlement suppressed: receipt 0 rewardAmount mismatch ..."
}
```

## Non-mutation guarantee

This command does not mutate reward or reputation ledgers. It only summarizes settlement deltas implied by already-audited local artifacts.

Use future durable ledger commands for actual settlement writes; do not treat this read-only report as a ledger mutation.

## Claim boundary

This is local/read-only audit and settlement-summary evidence. It is not public-network mining, not a paid/API benchmark, and not proof that a durable reputation ledger has been mutated.
