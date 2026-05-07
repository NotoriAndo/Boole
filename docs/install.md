# Boole installer

Boole provides a one-line bootstrapper for users who should not need to manually clone the repository or install every toolchain by hand.

```bash
curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh | bash
```

For review-before-run:

```bash
curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh -o install.sh
less install.sh
bash install.sh
```

## What it installs

The installer installs or prepares the required dependencies for the local safe proof-to-block preflight path:

- Git and curl.
- Python 3.
- C/C++ build tools required by Rust crates.
- Rust `1.95.0` via `rustup`.
- Rust components `rustfmt` and `clippy`.
- Lean `leanprover/lean4:v4.29.1` via `elan`.

Supported package managers in the first installer slice:

- macOS: Homebrew.
- Linux: `apt-get`.

Other Linux package managers can still use `--no-install` after installing the listed dependencies manually.

## Safety boundaries

The installer is a local bootstrapper. It does not perform security-sensitive or paid actions by default.

It will not:

- Ask for wallet seed phrases.
- Ask for private keys.
- Print API key values. It only reports API key environment variables as `present` or `missing`.
- Run paid API/model benchmarks without explicit confirmation.
- Start public mining.
- Overwrite local Git changes with `git reset` or `git clean`.

If an existing Boole checkout has local modifications, the installer keeps the checkout and skips destructive updates.

## Common options

```bash
# Print the plan without installing, cloning, or running checks.
bash install.sh --dry-run

# Install required dependencies without prompts.
bash install.sh --yes

# Use an existing dependency setup; do not install packages/toolchains.
bash install.sh --no-install

# Use a specific target directory.
bash install.sh --dir ~/projects/Boole

# Run the API-free safe proof-to-block preflight after setup.
bash install.sh --yes --run-safe-preflight
```

Default install location:

```text
~/boole
```

Override with either `--dir` or `BOOLE_HOME`:

```bash
BOOLE_HOME=~/projects/Boole bash install.sh
```

## After install

```bash
cd ~/boole
./scripts/boole-preflight-wizard.py
```

For the deterministic API-free local evidence path:

```bash
./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes
```

The safe preflight produces local, replay-checkable proof-to-block evidence. It is not public-network mining, not a token/reward claim, and not a paid model benchmark.

The wizard renders a seven-step guided plan (`Step 1/7` through `Step 7/7`) and writes three user-facing artifacts into the evidence directory after a successful run:

- `wizard-report.md`: safe public wording and replay/invalid/divergence metrics.
- `wizard-leaderboard.md`: local agent/runtime rows ranked by verifier/replay-backed score.
- `wizard-summary.redacted.json`: machine-readable summary with local paths redacted.

Frontier/API model rows require explicit cost acknowledgement:

```bash
./scripts/boole-preflight-wizard.py --preset frontier --allow-paid-api --yes
```

Without `--allow-paid-api`, frontier/all rows fail fast before execution.
