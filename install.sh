#!/usr/bin/env bash
set -euo pipefail

REPO_URL="https://github.com/NotoriAndo/Boole.git"
RAW_INSTALL_URL="https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh"
DEFAULT_BRANCH="main"
DEFAULT_DIR="${BOOLE_HOME:-$HOME/boole}"
RUST_TOOLCHAIN="1.95.0"
LEAN_TOOLCHAIN="leanprover/lean4:v4.29.1"

INSTALL_DIR="$DEFAULT_DIR"
BRANCH="$DEFAULT_BRANCH"
YES=0
DRY_RUN=0
NO_INSTALL=0
DOCTOR=0
RUN_SAFE_PREFLIGHT=0
DEV=0

usage() {
  cat <<'EOF'
Boole Installer
===============

One-line bootstrapper for Boole. It installs required dependencies, clones or
updates Boole, then runs the local setup doctor.

Usage:
  bash install.sh [options]
  curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/NotoriAndo/Boole/main/install.sh | bash -s -- --yes --run-safe-preflight

Options:
  --yes                 Do not prompt for confirmations.
  --dry-run             Print the plan without installing, cloning, or running checks.
  --no-install          Do not install missing dependencies; only use existing tools.
  --doctor              Run setup doctor only after checkout discovery/update step.
  --run-safe-preflight  After doctor, run API-free safe genesis preflight.
  --dev                 Also install optional developer/audit tools where supported.
  --dir PATH            Install/update Boole at PATH. Default: $BOOLE_HOME or ~/boole.
  --branch REF          Git branch/tag/ref to clone or update. Default: main.
  -h, --help            Show this help.

Safety:
  - This installer installs required dependencies for local Boole preflight.
  - It never asks for wallet seed phrases or private keys.
  - It never prints API key values; only present/missing status.
  - It never runs paid API benchmarks without explicit confirmation.
  - It never starts public mining.
EOF
}

log() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

quote() { printf '%q' "$1"; }

run_cmd() {
  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '$'
    local arg
    for arg in "$@"; do
      printf ' %q' "$arg"
    done
    printf '\n'
    return 0
  fi
  "$@"
}

confirm() {
  local prompt="$1"
  if [[ "$YES" -eq 1 ]]; then
    return 0
  fi
  local answer
  read -r -p "$prompt [y/N]: " answer
  case "${answer,,}" in
    y|yes) return 0 ;;
    *) return 1 ;;
  esac
}

have() { command -v "$1" >/dev/null 2>&1; }

normalize_repo_url() {
  local raw="$1"
  raw="${raw%/}"
  printf '%s' "${raw%.git}"
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --yes) YES=1 ;;
      --dry-run) DRY_RUN=1 ;;
      --no-install) NO_INSTALL=1 ;;
      --doctor) DOCTOR=1 ;;
      --run-safe-preflight) RUN_SAFE_PREFLIGHT=1 ;;
      --dev) DEV=1 ;;
      --dir)
        [[ $# -ge 2 ]] || fail "--dir requires a path"
        INSTALL_DIR="$2"
        shift
        ;;
      --branch)
        [[ $# -ge 2 ]] || fail "--branch requires a ref"
        BRANCH="$2"
        shift
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *) fail "unknown option: $1" ;;
    esac
    shift
  done
}

print_banner() {
  cat <<EOF
Boole Installer
===============

Boole is an experimental proof-mined L1 for verified AI-agent work.

This installer will:
  ✓ install required local dependencies when missing
  ✓ clone or update Boole from $REPO_URL
  ✓ install Rust $RUST_TOOLCHAIN plus rustfmt/clippy
  ✓ install Lean toolchain $LEAN_TOOLCHAIN via elan
  ✓ run Boole setup doctor

This installer will NOT:
  ✗ ask for wallet seed phrases or private keys
  ✗ print API key values
  ✗ run paid API benchmarks without explicit confirmation
  ✗ start public mining

EOF
  if [[ "$DRY_RUN" -eq 1 ]]; then
    log "DRY RUN: printing plan only; no files will be created or modified."
    log ""
  fi
}

print_credentials_status() {
  log "Credential environment"
  local name
  for name in ANTHROPIC_API_KEY OPENAI_API_KEY GOOGLE_API_KEY XAI_API_KEY; do
    if [[ -n "${!name:-}" ]]; then
      log "- $name: present"
    else
      log "- $name: missing"
    fi
  done
  log ""
}

detect_platform() {
  local os pm
  os="$(uname -s 2>/dev/null || true)"
  pm="none"
  case "$os" in
    Darwin)
      if have brew; then pm="brew"; fi
      ;;
    Linux)
      if have apt-get; then pm="apt"; fi
      ;;
    *) fail "unsupported OS: $os. Use macOS, Linux, or WSL." ;;
  esac
  printf '%s:%s' "$os" "$pm"
}

install_required_system_packages() {
  local platform os pm
  platform="$(detect_platform)"
  os="${platform%%:*}"
  pm="${platform##*:}"

  log "Install required dependencies"
  log "- OS: $os"
  log "- package manager: $pm"

  if [[ "$NO_INSTALL" -eq 1 ]]; then
    log "Skipping dependency installation (--no-install)."
    log ""
    return 0
  fi

  case "$pm" in
    brew)
      local packages=(git curl python3)
      if [[ "$DEV" -eq 1 ]]; then packages+=(gitleaks); fi
      log "- installing/checking: ${packages[*]}"
      run_cmd brew install "${packages[@]}"
      ;;
    apt)
      local packages=(git curl ca-certificates build-essential pkg-config libssl-dev python3)
      if [[ "$DEV" -eq 1 ]]; then packages+=(gitleaks); fi
      log "- installing/checking: ${packages[*]}"
      if [[ "$DRY_RUN" -eq 0 && "$YES" -eq 0 ]]; then
        confirm "This may require sudo. Continue" || fail "cancelled"
      fi
      run_cmd sudo apt-get update
      run_cmd sudo apt-get install -y "${packages[@]}"
      ;;
    none)
      warn "No supported package manager detected. Please install git, curl, python3, and build tools manually."
      ;;
  esac
  log ""
}

install_rust() {
  log "Install Rust toolchain"
  if [[ "$NO_INSTALL" -eq 1 ]]; then
    log "Skipping Rust installation (--no-install)."
    log ""
    return 0
  fi

  if ! have rustup; then
    log "- rustup missing; installing rustup"
    run_cmd sh -c 'curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal'
  else
    log "- rustup: present"
  fi

  export PATH="$HOME/.cargo/bin:$PATH"
  run_cmd rustup toolchain install "$RUST_TOOLCHAIN" --component rustfmt --component clippy
  run_cmd rustup default "$RUST_TOOLCHAIN"
  log ""
}

install_lean() {
  log "Install Lean toolchain"
  if [[ "$NO_INSTALL" -eq 1 ]]; then
    log "Skipping Lean installation (--no-install)."
    log ""
    return 0
  fi

  if ! have elan; then
    log "- elan missing; installing elan"
    # N0-pre.3 — pin the elan installer to an immutable release tag (matching
    # ci.yml's v4.2.3) and verify its sha256 before executing, instead of
    # piping a mutable `master` script straight into a shell.
    run_cmd sh -c '
      set -eu
      elan_init="$(mktemp)"
      curl -sSfL https://raw.githubusercontent.com/leanprover/elan/v4.2.3/elan-init.sh -o "$elan_init"
      expected="a620ff1641616222c8d37c54845492004bb84d6877cdbc944dd65c1aa685bf53"
      if command -v sha256sum >/dev/null 2>&1; then
        echo "$expected  $elan_init" | sha256sum -c -
      else
        echo "$expected  $elan_init" | shasum -a 256 -c -
      fi
      sh "$elan_init" -y --default-toolchain none
      rm -f "$elan_init"
    '
  else
    log "- elan: present"
  fi

  export PATH="$HOME/.elan/bin:$PATH"
  run_cmd elan toolchain install "$LEAN_TOOLCHAIN"
  log ""
}

clone_or_update_repo() {
  log "Clone or update Boole"
  log "- target: $INSTALL_DIR"
  log "- ref: $BRANCH"

  if [[ -d "$INSTALL_DIR/.git" ]]; then
    log "Using existing Boole checkout: $INSTALL_DIR"
    if [[ "$DRY_RUN" -eq 0 ]]; then
      local remote
      remote="$(git -C "$INSTALL_DIR" remote get-url origin 2>/dev/null || true)"
      if [[ "$(normalize_repo_url "$remote")" != "$(normalize_repo_url "$REPO_URL")" ]]; then
        fail "existing checkout origin is not $REPO_URL: $remote"
      fi
      if [[ -n "$(git -C "$INSTALL_DIR" status --porcelain)" ]]; then
        warn "Local changes detected; installer will not overwrite them. Skipping update."
      else
        run_cmd git -C "$INSTALL_DIR" fetch --prune origin "$BRANCH"
        run_cmd git -C "$INSTALL_DIR" checkout "$BRANCH"
        run_cmd git -C "$INSTALL_DIR" pull --ff-only origin "$BRANCH"
      fi
    else
      run_cmd git -C "$INSTALL_DIR" fetch --prune origin "$BRANCH"
      run_cmd git -C "$INSTALL_DIR" checkout "$BRANCH"
      run_cmd git -C "$INSTALL_DIR" pull --ff-only origin "$BRANCH"
    fi
  else
    if [[ -e "$INSTALL_DIR" ]]; then
      fail "target exists but is not a git checkout: $INSTALL_DIR"
    fi
    run_cmd git clone --branch "$BRANCH" "$REPO_URL" "$INSTALL_DIR"
  fi
  log ""
}

run_doctor() {
  log "Run setup doctor"
  local wizard="$INSTALL_DIR/scripts/boole-preflight-wizard.py"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    run_cmd bash -lc "cd $(quote "$INSTALL_DIR") && ./scripts/boole-preflight-wizard.py --doctor"
    log ""
    return 0
  fi
  [[ -x "$wizard" || -f "$wizard" ]] || fail "wizard not found: $wizard"
  run_cmd bash -lc "cd $(quote "$INSTALL_DIR") && ./scripts/boole-preflight-wizard.py --doctor"
  log ""
}

run_safe_preflight() {
  if [[ "$RUN_SAFE_PREFLIGHT" -ne 1 ]]; then
    if [[ "$DOCTOR" -eq 1 || "$YES" -eq 1 || "$DRY_RUN" -eq 1 || ! -t 0 ]]; then
      log "Safe preflight not requested."
      log "Run later: cd $(quote "$INSTALL_DIR") && ./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes"
      return 0
    fi
    if ! confirm "Run API-free safe proof-to-block preflight now"; then
      log "Skipped safe preflight."
      log "Run later: cd $(quote "$INSTALL_DIR") && ./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes"
      return 0
    fi
  fi

  log "Run safe proof-to-block preflight"
  run_cmd bash -lc "cd $(quote "$INSTALL_DIR") && ./scripts/boole-preflight-wizard.py --preset safe --genesis-benchmark --yes"
  log ""
}

main() {
  parse_args "$@"
  print_banner
  print_credentials_status
  install_required_system_packages
  install_rust
  install_lean
  clone_or_update_repo
  run_doctor
  run_safe_preflight

  cat <<EOF
Boole installer complete.

Location:
  $INSTALL_DIR

Next commands:
  cd $(quote "$INSTALL_DIR")
  ./scripts/boole-preflight-wizard.py

Review-before-run installer URL:
  curl -fsSL $RAW_INSTALL_URL -o install.sh
  less install.sh
  bash install.sh
EOF
}

main "$@"
