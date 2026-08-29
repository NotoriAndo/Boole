#!/usr/bin/env bash
# Run the launcher-v2 staging preflight inside the same sealed Linux isolation
# used by image production, while exposing no image-production entry point.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow launcher-v2 image preflight: FAIL: %s\n' "$*" >&2
  exit 1
}

cas=""
launcher=""
result=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --cas) [[ $# -ge 2 ]] || die "--cas needs a path"; cas=$2; shift 2 ;;
    --launcher) [[ $# -ge 2 ]] || die "--launcher needs a path"; launcher=$2; shift 2 ;;
    --result) [[ $# -ge 2 ]] || die "--result needs a path"; result=$2; shift 2 ;;
    *) die "unexpected argument: $1" ;;
  esac
done

[[ -n $cas ]] || die "--cas is required"
[[ -n $launcher ]] || die "--launcher is required"
[[ -n $result ]] || die "--result is required"

for command_name in dirname env find gpgv mkdir mktemp python3 readlink rm systemd-run uname zstd; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done
[[ $(uname -s) == "Linux" ]] || die "the preflight requires Linux"
[[ $(uname -m) == "aarch64" || $(uname -m) == "arm64" ]] \
  || die "the preflight requires native arm64"
[[ ${EUID} -eq 0 ]] || die "the preflight isolation must be installed as root"

[[ $result == /* ]] || die "--result must be absolute: $result"
result_parent="$(dirname -- "$result")"
[[ -d $cas && ! -L $cas ]] || die "the verified payload store is absent: $cas"
[[ -f $launcher && ! -L $launcher ]] || die "the launcher-v2 ELF is absent: $launcher"
[[ -d $result_parent && ! -L $result_parent ]] \
  || die "the result parent must be an existing directory"
[[ ! -e $result && ! -L $result ]] || die "the result name already exists: $result"

scratch="$(mktemp -d "$result_parent/.boole-launcher-v2-preflight.XXXXXX")"
work="$scratch/work"
internal_result="$work/PREFLIGHT-RESULT.json"
cleanup() {
  rm -rf -- "$scratch"
}
trap cleanup EXIT
mkdir -p "$work"

gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"

isolation=()
while IFS= read -r line; do
  isolation+=("$line")
done < <(
  python3 "$ROOT/scripts/native_shadow_boot_image_produce_arm64_v1.py" isolation-argv \
    --repository-root "$ROOT" \
    --read-write-path "$scratch" \
    -- \
    /usr/bin/env python3 \
    "$ROOT/scripts/native_shadow_launcher_v2_image_preflight_arm64_v1.py" \
    --repo-root "$ROOT" \
    --cas "$cas" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --staging "$work/staging" \
    --launcher "$launcher" \
    --result "$internal_result"
)
[[ ${#isolation[@]} -gt 0 ]] || die "the isolation authority yielded no command"
"${isolation[@]}"

# Read the zero-output boundary from its exact preregistration and inspect the
# only writable subtree before cleanup can erase evidence.  Names are data from
# the prior record, not a second hand-written list in this wrapper.
forbidden_names=()
while IFS= read -r name; do
  forbidden_names+=("$name")
done < <(
  python3 -c 'import json,sys; print("\n".join(json.load(open(sys.argv[1], encoding="utf-8"))["preflight"]["forbiddenNames"]))' \
    "$ROOT/native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
)
[[ ${#forbidden_names[@]} -eq 4 ]] || die "the preregistration names no exact boundary"
expression=()
for name in "${forbidden_names[@]}"; do
  expression+=(-o -name "$name")
done
found="$(find "$scratch" \( "${expression[@]:1}" \) -print)"
[[ -z $found ]] || die "the preflight created a forbidden output: $found"

[[ -f $internal_result && ! -L $internal_result ]] \
  || die "the isolated preflight wrote no result"

# Copy the canonical report out with create-once semantics.  The result is the
# only retained file; a race, symlink, or existing path is refused rather than
# replaced.
python3 - "$internal_result" "$result" <<'PY'
import errno
import os
import pathlib
import sys
import tempfile

source = pathlib.Path(sys.argv[1])
destination = pathlib.Path(sys.argv[2])
raw = source.read_bytes()
descriptor, temporary_name = tempfile.mkstemp(
    prefix=".boole-launcher-v2-preflight.", dir=destination.parent
)
temporary = pathlib.Path(temporary_name)
try:
    with os.fdopen(descriptor, "wb", closefd=True) as handle:
        handle.write(raw)
        handle.flush()
        os.fsync(handle.fileno())
    temporary.chmod(0o444)
    try:
        os.link(temporary, destination)
    except OSError as exc:
        if exc.errno == errno.EEXIST:
            raise SystemExit("result name appeared during preflight; refusing replacement")
        raise
    directory = os.open(destination.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
finally:
    temporary.unlink(missing_ok=True)
PY

printf 'native-shadow launcher-v2 image preflight: PASS: %s\n' "$result"
