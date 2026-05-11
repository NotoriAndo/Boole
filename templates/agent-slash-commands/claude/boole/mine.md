---
description: Run Boole proof-to-block mining through the shared agent wrapper.
allowed-tools: Bash(__BOOLE_ROOT__/scripts/boole-agent-mine.sh:*)
---

Run Boole mining from this Claude Code session.

Use the shared wrapper, not custom verifier or submit logic:

```bash
__BOOLE_ROOT__/scripts/boole-agent-mine.sh --runtime claude-code $ARGUMENTS
```

Execute it with Bash and report only the safe summary fields:

- `ok`
- `skipped` / `reason` when present
- runtime/kind
- verifier/canonicalizer path when present
- `summary.verifyAccepted`
- `summary.sharesAccepted`
- `status.height`
- `status.replayMatchesRuntime`

Do not expose private prompts, keys, credentials, or full agent logs. The agent is only a candidate proof producer; deterministic verifier/canonicalizer/node replay decides acceptance. This is local controlled-smoke UX, not public mining evidence.
