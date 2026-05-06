# Agent slash-command mining MVP

This document describes the first `/boole mine` foundation. The actual slash-command integration for Claude Code, Codex, OpenCode, or Hermes should call the shared wrapper:

```bash
./scripts/boole-agent-mine.sh
```

The wrapper is intentionally thin. It does not contain consensus logic. It delegates to the existing `boole-miner` + Rust `boole-node` smoke paths so that deterministic verification, canonical proof bytes, share hashing, submit handling, and replay remain the source of truth.

## Core principle

```text
Agent proposes.
Verifier decides.
Node records.
Replay proves it.
```

Agent output is only an untrusted candidate proof. Accepted work must still pass the configured verifier/canonicalizer and `boole-node` replay checks.

## Commands

Deterministic fake-agent transport smoke:

```bash
./scripts/boole-agent-mine.sh --runtime fake
```

Hermes mock-verifier mining smoke:

```bash
./scripts/boole-agent-mine.sh --runtime hermes --verify mock
```

Hermes real Lean verifier + canonical POFP smoke:

```bash
./scripts/boole-agent-mine.sh --runtime hermes --verify real
```

OpenCode/OpenClaw-compatible runtime:

```bash
./scripts/boole-agent-mine.sh --runtime opencode
./scripts/boole-agent-mine.sh --runtime openclaw
```

Claude Code-compatible runtime, using command/args overrides when needed:

```bash
./scripts/boole-agent-mine.sh \
  --runtime claude-code \
  --agent-command claude \
  --agent-args '["-p"]'
```

Codex-compatible runtime, using command/args overrides when needed:

```bash
./scripts/boole-agent-mine.sh \
  --runtime codex \
  --agent-command codex \
  --agent-args '["exec"]'
```

If a runtime is missing, the wrapper emits a JSON `SKIP` result and exits successfully. This keeps slash-command templates and default benchmarks safe on machines where a specific agent CLI is not installed.

## Slash-command shape

A Claude Code/Codex/OpenCode slash command should be a thin wrapper around the shared command, for example:

```text
/boole mine
```

internally maps to:

```bash
./scripts/boole-agent-mine.sh --runtime claude-code
```

A runtime-specific command can use explicit args:

```bash
./scripts/boole-agent-mine.sh \
  --runtime claude-code \
  --agent-command claude \
  --agent-args '["-p"]'
```

The expected user-facing result is the underlying smoke JSON plus stderr PASS/SKIP line. A successful run must include node `height >= 1` and `replayMatchesRuntime: true`; miner counters alone are not sufficient.

## Current support level

- `fake`: deterministic transport path, mock verifier, expected to PASS.
- `hermes --verify mock`: Hermes CLI path, mock verifier, expected to PASS when Hermes is installed.
- `hermes --verify real`: Hermes CLI path, real Lean verifier and LakeCanonicalizer/`boole_emit` POFP canonical package, expected to PASS when Hermes and the Lean toolchain are available.
- `opencode`/`openclaw`: command-detected or override-based OpenCode/OpenClaw-compatible path, mock verifier; SKIP if the CLI is missing.
- `claude-code`/`codex`: command/args-compatible path through the same `agent_cli` mechanism; SKIP if the CLI is missing. Real verifier rows should be promoted only after dedicated live proof-to-block smokes pass.

## Safety notes

- Do not give an agent unrestricted wallet keys.
- Use work identity/session keys with limits when real rewards are introduced.
- Keep slash commands as UX wrappers; consensus-critical acceptance remains in verifier/canonicalizer/node replay.
- Do not market this as reselling unused model subscriptions. Safe framing: user-approved agent runtimes produce verifier-backed work.
