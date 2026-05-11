# Boole Mine

Run Boole proof-to-block mining from a Codex CLI session by invoking the shared wrapper.

Command to run:

```bash
__BOOLE_ROOT__/scripts/boole-agent-mine.sh --runtime codex __BOOLE_ARGS__
```

Report only safe summary fields:

- `ok`
- `skipped` / `reason` when present
- runtime/kind
- `summary.verifyAccepted`
- `summary.sharesAccepted`
- `status.height`
- `status.replayMatchesRuntime`

Do not expose private prompts, keys, credentials, or full agent logs. The Codex runtime is only a candidate proof producer; deterministic verifier/canonicalizer/node replay decides acceptance. This is local controlled-smoke UX, not public mining evidence.
