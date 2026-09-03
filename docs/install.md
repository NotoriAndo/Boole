# Boole installer

Status: **SOURCE BOOTSTRAP AVAILABLE / CURL PRODUCT PATH IMPLEMENTED — NO PUBLIC SIGNED RELEASE**.

The command below is the current developer/source bootstrap. It clones or updates the repository
and installs development toolchains. It is the only public one-line install command today and
**must not be presented as the finished Mac product installer**.

The curl-first product path is now implemented and closed-local tested. It verifies independent
product and Linux-guest signatures, installs immutable prebuilt macOS arm64 host binaries plus the
guest atomically, runs and checks the hidden guest, updates and rolls back without lowering either
security floor, recovers from a corrupt active release, preserves the journal and wallet across a
runtime reset, inspects installed bytes, and packages already-signed first or successor releases.
Its main command surface is:

```text
boole product package-direct-boot
boole product verify-operational-custody-plan
boole product package-trust-bootstrap
boole product adopt-trust-bootstrap
boole product install-direct-boot
boole product run-direct-boot
boole product status-direct-boot
boole product inspect-direct-boot
boole product rollback-direct-boot
boole product recover-direct-boot
boole product update-trust-policy
boole product reset-direct-boot
```

This is not yet a public product release. The offline packager can now verify a canonical release
policy authorized by exactly two of three recovery keys and derive separate product and guest
public roots from it. The implementation uses deterministic non-production test keys only. No
operational private signing keys or public recovery root have been created or published, and no
official signed bundle or production one-line entrypoint exists.

Before a real key ceremony is prepared, the operator must provide and validate the custody and
independent-publication plan described in
[`operational-key-custody-plan-v1.md`](operational-key-custody-plan-v1.md):

```text
boole product verify-operational-custody-plan \
  --plan operational-custody-plan.json
```

This command reads public labels and URLs only. It generates no key, publishes nothing and grants
no production authority.

The repository also has a **non-production rehearsal** for the public half of an operational key
ceremony. It verifies that the active product key, guest key and all three recovery keys signed one
canonical ceremony transcript, then atomically emits only the five public JSON documents. This
proves possession of five distinct private keys during the rehearsal; it cannot prove that separate
people, devices or physical sites held those keys. The command rejects any environment other than
`non-production-kat` and never receives private-key material:

```text
boole product package-trust-bootstrap \
  --recovery-root recovery-root.json \
  --trust-policy trust-policy.json \
  --trust-policy-signatures trust-policy-signatures.json \
  --key-ceremony key-ceremony.json \
  --key-ceremony-signatures key-ceremony-signatures.json \
  --output-root public-trust-bootstrap
```

Before release transport, an installer may adopt that public package only when the recovery-root
SHA-256 was obtained through an independent channel. Do not copy this value from the package or
download location being authenticated; that would let the untrusted channel choose its own trust
anchor. Adoption stores generation one durably but downloads no release:

```text
boole product adopt-trust-bootstrap \
  --bootstrap-root public-trust-bootstrap \
  --install-root PATH \
  --expected-recovery-root-sha256 SHA256_FROM_AN_INDEPENDENT_CHANNEL
```

After adoption, the ordinary `install-direct-boot` command reopens the stored policy and therefore
needs no recovery-root, policy or direct release-root arguments. The independent publication
channel, operational key creation and physical custody procedure are not supplied by these
commands and remain release-operator work.

Policy-backed development packaging supplies all three public files together:

```text
boole product package-direct-boot \
  --recovery-root recovery-root.json \
  --trust-policy trust-policy.json \
  --trust-policy-signatures trust-policy-signatures.json \
  ...
```

The same policy can now be adopted by the installed product. The first install bootstraps the
public recovery root and generation-one policy before downloading a release:

```text
boole product install-direct-boot \
  --base-url http://127.0.0.1:PORT/ \
  --install-root PATH \
  --recovery-root recovery-root.json \
  --trust-policy trust-policy.json \
  --trust-policy-signatures trust-policy-signatures.json \
  --first-product-minimum 1 \
  --first-guest-minimum 1
```

After that bootstrap, install/update, run, inspect, rollback and recovery reopen the persisted
policy chain and need no release-root arguments. The recovery role can adopt the exact next policy
without downloading a product release, which allows a compromised online role to be disabled
immediately:

```text
boole product update-trust-policy \
  --install-root PATH \
  --trust-policy successor-policy.json \
  --trust-policy-signatures successor-signatures.json

boole product install-direct-boot \
  --base-url http://127.0.0.1:PORT/ \
  --install-root PATH \
  --first-product-minimum 1 \
  --first-guest-minimum 1

boole product inspect-direct-boot --install-root PATH
boole product rollback-direct-boot --install-root PATH
boole product recover-direct-boot --install-root PATH
boole product run-direct-boot --install-root PATH
```

The recovery root, every accepted policy generation and the small atomic chain-head record are
stored below the install root. Every lifecycle command reconstructs and cryptographically verifies
the complete chain before using a product or guest release key. A replayed successor, broken chain,
changed policy file or attempt to supply direct roots to a policy-backed installation is rejected
without silently falling back.

The four direct product/guest root arguments remain a development compatibility mode for an
installation that has never adopted a policy. Neither mode infers trust from a download URL,
accepts private keys, signs a release or uploads files. Use `boole product --help` for the current
development interface, and never substitute the closed-local test keys for an operational release
identity.

Boole currently provides a one-line source bootstrapper for developers and local evaluators who
do not want to clone the repository or prepare every toolchain manually.

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

## Optional cargo-audit security scan

`cargo-audit` is an optional local security scan for Rust dependency advisories. It is not part of the default installer or self-test gate because the first-pass safe preflight should remain reproducible even when optional security tools are not installed.

To run it manually:

```bash
cargo install cargo-audit
cargo audit
```

Treat any advisory as a dependency/security triage item. Do not interpret this scan as public mining evidence, paid/API benchmark evidence, or proof that runtime protocol invariants are correct.

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

The wizard renders a seven-step guided plan (`Step 1/7` through `Step 7/7`) and writes three user-facing artifacts into the evidence directory after a successful run. It includes a Hermes-style model/runtime picker so users can inspect available targets and choose exactly what to run. The `Ollama readiness` section reports whether the command is installed, whether the daemon is reachable, which local models are available, and whether each requested Ollama target is `ready`, `setup-required`, or `blocked`; it never auto-pulls large models. Missing local dependencies are shown as friendly recovery blocks instead of opaque failures: `Diagnostics and recovery` includes `status`, `why`, `fix`, and `retry` lines such as `fix: ollama serve`, `fix: ollama pull qwen2.5-coder:7b`, or `fix: install/configure hermes`.

```bash
# Show all detected target rows, credential status, cost class, and action.
./scripts/boole-preflight-wizard.py --list-models

# Non-interactive safe target selection.
./scripts/boole-preflight-wizard.py --target safe-core --preset safe --genesis-benchmark --yes

# Local model rows stay API-free, but require the local runtime/model to be installed.
./scripts/boole-preflight-wizard.py --target ollama:qwen2.5-coder:7b --preset local-models --yes
```

Report artifacts:

- `wizard-report.md`: safe public wording and replay/invalid/divergence metrics.
- `wizard-leaderboard.md`: local agent/runtime rows ranked by verifier/replay-backed score.
- `wizard-summary.redacted.json`: machine-readable summary with local paths redacted.

Frontier/API model rows require explicit cost acknowledgement:

```bash
./scripts/boole-preflight-wizard.py --preset frontier --allow-paid-api --yes
```

Without `--allow-paid-api`, frontier/all rows fail fast before execution.
