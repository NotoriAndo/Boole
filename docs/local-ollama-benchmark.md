# Optional local Ollama Proof-to-Block benchmark

This guide is for a **manual, optional local Ollama** smoke run after the safe Boole preflight path is already working. It does not use paid APIs, wallets, public mining, or live network state.

## Safety boundary

- No automatic model pull.
- No automatic daemon start.
- Missing command, unreachable daemon, or absent model is recorded as `setup-required` / blocked guidance, not a false benchmark failure.
- Local model-generated proof attempts are evaluated by Boole's verifier and recorded as accepted/rejected/setup-required benchmark rows.
- This is local preflight evidence, not public-network mining and not a token/reward claim.

## Manual prerequisites

Install Ollama using the official installer for your platform, then start the daemon yourself:

```bash
ollama serve
```

In another terminal, pull the model you want to test. Boole does not do this automatically:

```bash
ollama pull qwen2.5-coder:7b
```

Check what Boole sees:

```bash
./scripts/boole-preflight-wizard.py --list-models
```

Expected readiness states:

- `ready`: command, daemon, and selected model are available.
- `setup-required`: Ollama is reachable but the selected model is missing.
- `blocked`: command is missing or the daemon is unreachable.

## One-command local benchmark smoke

Run one generated proof attempt through the local wizard path:

```bash
./scripts/boole-preflight-wizard.py \
  --preset safe \
  --genesis-benchmark \
  --model-preset ollama \
  --ollama-model qwen2.5-coder:7b \
  --attempts-per-model 1 \
  --yes
```

The wizard writes these evidence files next to the preflight evidence directory:

- `wizard-report.md`
- `wizard-leaderboard.md`
- `wizard-summary.redacted.json`
- `provider-model-live-leaderboard.md`
- `provider-model-live-spec.json`

Per-model benchmark bundles are written below the evidence artifact root and include:

- `benchmark-summary.json`
- `benchmark-rows.ndjson`
- `leaderboard.md`
- `replay-report.json`

## How to read the result

Accepted row:

```text
The generated proof candidate passed the verifier path, produced a verified share/block row, and replay stayed safe.
```

Rejected row:

```text
The model generated a proof attempt, but Boole's verifier did not accept it. This is a valid benchmark outcome.
```

Setup-required row:

```text
The local runtime was not ready, for example the model was missing. Follow the printed fix/retry guidance.
```

The public claim should stay narrow:

```text
Local model-generated proof attempts are evaluated by Boole's verifier and recorded as accepted/rejected/setup-required benchmark rows.
```

Do not claim that a local model mined public Boole blocks.
