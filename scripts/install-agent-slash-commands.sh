#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="all"
TARGET_DIR=""
BOOLE_ROOT="$ROOT"
DRY_RUN=0
FORCE=0
CODEX_ARGS=""

usage() {
  cat <<'EOF'
Usage: install-agent-slash-commands.sh [--profile claude|codex|all] [--target-dir DIR] [--boole-root DIR] [--codex-args JSON_OR_TEXT] [--dry-run] [--force]

Installs thin slash/prompt command templates that call scripts/boole-agent-mine.sh.
The templates do not implement verifier, submit, or replay logic.

Defaults:
  claude target: ~/.claude/commands
  codex target:  ~/.codex/prompts

Examples:
  ./scripts/install-agent-slash-commands.sh --dry-run
  ./scripts/install-agent-slash-commands.sh --profile claude
  ./scripts/install-agent-slash-commands.sh --profile claude --target-dir .claude/commands --force
  ./scripts/install-agent-slash-commands.sh --profile codex --target-dir /tmp/boole-codex-prompts
EOF
}

json_report() {
  python3 - "$@" <<'PY'
import json, sys
kind, profile, target, boole_root, dry_run, force, written = sys.argv[1:8]
print(json.dumps({
    "ok": True,
    "kind": kind,
    "profile": profile,
    "targetDir": target,
    "booleRoot": boole_root,
    "dryRun": dry_run == "1",
    "force": force == "1",
    "written": [] if not written else written.split("\n"),
}, separators=(",", ":")))
PY
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="${2:?missing --profile value}"
      shift 2
      ;;
    --target-dir)
      TARGET_DIR="${2:?missing --target-dir value}"
      shift 2
      ;;
    --boole-root)
      BOOLE_ROOT="${2:?missing --boole-root value}"
      shift 2
      ;;
    --codex-args)
      CODEX_ARGS="${2:?missing --codex-args value}"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'install-agent-slash-commands: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

case "$PROFILE" in
  claude|codex|all) ;;
  *)
    printf 'install-agent-slash-commands: --profile must be claude, codex, or all\n' >&2
    exit 64
    ;;
esac

if [[ ! -x "$BOOLE_ROOT/scripts/boole-agent-mine.sh" ]]; then
  printf 'install-agent-slash-commands: boole-agent-mine.sh is not executable under %s\n' "$BOOLE_ROOT" >&2
  exit 66
fi

render_file() {
  local src="$1"
  local dest="$2"
  local args_value="$CODEX_ARGS"
  if [[ -z "$args_value" ]]; then
    args_value='$ARGUMENTS'
  fi

  if [[ "$DRY_RUN" == "1" ]]; then
    printf '%s\n' "$dest"
    return 0
  fi

  if [[ -e "$dest" && "$FORCE" != "1" ]]; then
    printf 'install-agent-slash-commands: refusing to overwrite %s without --force\n' "$dest" >&2
    exit 73
  fi
  mkdir -p "$(dirname "$dest")"
  python3 - "$src" "$dest" "$BOOLE_ROOT" "$args_value" <<'PY'
from pathlib import Path
import sys
src, dest, root, args = sys.argv[1:5]
text = Path(src).read_text()
text = text.replace("__BOOLE_ROOT__", root)
text = text.replace("__BOOLE_ARGS__", args)
Path(dest).write_text(text)
PY
  printf '%s\n' "$dest"
}

install_claude() {
  local base="$TARGET_DIR"
  if [[ -z "$base" ]]; then
    base="$HOME/.claude/commands"
  fi
  local written=""
  written+="$(render_file "$ROOT/templates/agent-slash-commands/claude/boole/mine.md" "$base/boole/mine.md")"
  written+=$'\n'
  written+="$(render_file "$ROOT/templates/agent-slash-commands/claude/boole/status.md" "$base/boole/status.md")"
  json_report "boole-agent-slash-install" "claude" "$base" "$BOOLE_ROOT" "$DRY_RUN" "$FORCE" "$written"
}

install_codex() {
  local base="$TARGET_DIR"
  if [[ -z "$base" ]]; then
    base="$HOME/.codex/prompts"
  fi
  local written=""
  written+="$(render_file "$ROOT/templates/agent-slash-commands/codex/boole-mine.md" "$base/boole-mine.md")"
  written+=$'\n'
  written+="$(render_file "$ROOT/templates/agent-slash-commands/codex/boole-status.md" "$base/boole-status.md")"
  json_report "boole-agent-slash-install" "codex" "$base" "$BOOLE_ROOT" "$DRY_RUN" "$FORCE" "$written"
}

case "$PROFILE" in
  claude)
    install_claude
    ;;
  codex)
    install_codex
    ;;
  all)
    install_claude
    install_codex
    ;;
esac
