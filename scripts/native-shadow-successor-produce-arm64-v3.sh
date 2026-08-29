#!/usr/bin/env bash
# Authority-zero wrapper for the launcher-v2 successor generation.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PRODUCER="$ROOT/scripts/native_shadow_successor_produce_phase_arm64_v3.py"
READBACK="$ROOT/scripts/native_shadow_successor_root_disk_readback_arm64_v3.py"

export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow launcher-v2 successor v3: FAIL: %s\n' "$*" >&2
  exit 1
}

# Verify every pre-registered repository byte before Python can import any of
# the bound builder/readback helpers.  This small verifier lives inside the
# future-fingerprinted wrapper itself and imports no repository module.
verify_preregistered_bindings() {
  python3 -I -S -c '
import hashlib
import json
import pathlib
import stat
import sys

root = pathlib.Path(sys.argv[1]).resolve()

def fixed_record(relative_text, size, digest, label):
    relative = pathlib.PurePosixPath(relative_text)
    path = root.joinpath(*relative.parts)
    metadata = path.lstat()
    if path.is_symlink() or not stat.S_ISREG(metadata.st_mode):
        raise SystemExit(label + " is not a regular non-symlink file")
    path.resolve().relative_to(root)
    raw = path.read_bytes()
    if len(raw) != size:
        raise SystemExit(label + " size differs")
    if hashlib.sha256(raw).hexdigest() != digest:
        raise SystemExit(label + " digest differs")
    return json.loads(raw.decode("utf-8"))

document = fixed_record(
    "native/containment/native-shadow-mac3-launcher-v2-successor-"
    "producer-preregistration-arm64-v1.json",
    20145,
    "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
    "producer preregistration",
)
correction = fixed_record(
    "native/containment/native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json",
    10971,
    "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
    "producer import-closure correction",
)
rows = document.get("bindings")
if not isinstance(rows, list) or len(rows) != 23:
    raise SystemExit("producer preregistration must bind exactly twenty-three inputs")
added = correction.get("addedBindings")
if not isinstance(added, list) or len(added) != 18:
    raise SystemExit("producer correction must add exactly eighteen inputs")
if correction.get("predecessor") != {
    "bindingCount": 23,
    "path": (
        "native/containment/native-shadow-mac3-launcher-v2-successor-"
        "producer-preregistration-arm64-v1.json"
    ),
    "preservedByteUnchanged": True,
    "sha256": "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
    "sizeBytes": 20145,
}:
    raise SystemExit("producer correction predecessor differs")
if correction.get("authorisations") != document.get("authorisations"):
    raise SystemExit("producer correction authority differs")
if correction.get("runs") != document.get("runs"):
    raise SystemExit("producer correction run ledger differs")
if correction.get("effectiveBinding") != {
    "addedMissingBindings": 18,
    "bindingVerificationBeforeRepositoryPythonImport": True,
    "effectiveUniqueBindings": 41,
    "predecessorBindings": 23,
    "unionRequired": True,
}:
    raise SystemExit("producer correction effective union differs")
rows = rows + added
seen = set()
for row in rows:
    if not isinstance(row, dict):
        raise SystemExit("producer binding is not an object")
    name = row.get("path")
    pure = pathlib.PurePosixPath(name) if isinstance(name, str) else None
    if (
        pure is None
        or pure.is_absolute()
        or ".." in pure.parts
        or pure.as_posix() != name
        or name in seen
    ):
        raise SystemExit("producer binding path is unsafe or repeated")
    seen.add(name)
    candidate = root.joinpath(*pure.parts)
    info = candidate.lstat()
    if candidate.is_symlink() or not stat.S_ISREG(info.st_mode):
        raise SystemExit("producer binding is not a regular non-symlink file: " + name)
    try:
        candidate.resolve().relative_to(root)
    except ValueError:
        raise SystemExit("producer binding leaves the repository: " + name)
    bound = candidate.read_bytes()
    if type(row.get("sizeBytes")) is not int or len(bound) != row["sizeBytes"]:
        raise SystemExit("producer binding size differs: " + name)
    if hashlib.sha256(bound).hexdigest() != row.get("sha256"):
        raise SystemExit("producer binding digest differs: " + name)
if len(seen) != 41:
    raise SystemExit("producer effective binding union is not forty-one inputs")
' "$ROOT"
}

mode=""
cas=""
launcher=""
result=""
outputs_seen="no"
while [[ $# -gt 0 ]]; do
  case $1 in
    --verify-bindings-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="verify"
      shift
      ;;
    --rehearsal-only)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="rehearsal"
      shift
      ;;
    --production)
      [[ -z $mode ]] || die "choose exactly one mode"
      mode="production"
      shift
      ;;
    --cas)
      [[ $# -ge 2 ]] || die "--cas needs a path"
      cas=$2
      shift 2
      ;;
    --launcher)
      [[ $# -ge 2 ]] || die "--launcher needs a path"
      launcher=$2
      shift 2
      ;;
    --result)
      [[ $# -ge 2 ]] || die "--result needs a path"
      result=$2
      shift 2
      ;;
    --outputs)
      outputs_seen="yes"
      [[ $# -ge 2 ]] || die "--outputs needs a path"
      shift 2
      ;;
    *) die "unexpected argument: $1" ;;
  esac
done

[[ -n $mode ]] \
  || die "one of --verify-bindings-only, --rehearsal-only or --production is required"
if [[ $mode == "rehearsal" && $outputs_seen == "yes" ]]; then
  die "--rehearsal-only accepts no --outputs"
fi
if [[ $mode == "verify" && ( -n $cas || -n $launcher || -n $result || $outputs_seen == "yes" ) ]]; then
  die "--verify-bindings-only accepts no other input"
fi

verify_preregistered_bindings

if [[ $mode == "verify" ]]; then
  printf 'native-shadow launcher-v2 successor v3: bindings verified\n' >&2
  exit 0
fi

# This check is deliberately before command discovery, host checks, input-path
# checks and mktemp.  The current record grants zero production runs; a manual
# production dispatch therefore cannot download dependencies or create even an
# empty output/scratch directory before the producer has rejected it.
if [[ $mode == "production" ]]; then
  printf 'native-shadow launcher-v2 successor v3: production authority check\n' >&2
  exec python3 -I -S "$PRODUCER" production-check
fi

# The executable edges are intentionally visible here.  Rehearsal and the
# future authority check call the producer directly; only a future qualified
# production may continue to this generation's readback consumer.
run_readback_v3() {
  python3 -I -S "$READBACK" "$@"
}

[[ -n $cas ]] || die "--cas is required for --rehearsal-only"
[[ -n $launcher ]] || die "--launcher is required for --rehearsal-only"
[[ -n $result ]] || die "--result is required for --rehearsal-only"

for command_name in dirname find gpgv ln mkdir mktemp python3 readlink rm uname zstd; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done
[[ $(uname -s) == "Linux" ]] || die "the rehearsal requires Linux"
[[ $(uname -m) == "aarch64" || $(uname -m) == "arm64" ]] \
  || die "the rehearsal requires native arm64"

[[ $result == /* ]] || die "--result must be absolute"
result_parent="$(dirname -- "$result")"
[[ -d $result_parent && ! -L $result_parent ]] \
  || die "the result parent must be an existing directory"
result_parent="$(readlink -f "$result_parent")"
[[ ! -e $result && ! -L $result ]] || die "the result name already exists"
[[ -d $cas && ! -L $cas ]] || die "the verified payload store is absent"
[[ -f $launcher && ! -L $launcher ]] || die "the launcher-v2 ELF is absent"

scratch="$(mktemp -d "$result_parent/.boole-successor-v3-rehearsal.XXXXXX")"
expected_scratch_prefix="$result_parent/.boole-successor-v3-rehearsal."
[[ $scratch == "$expected_scratch_prefix"* ]] \
  || die "mktemp returned a scratch path outside the fixed private prefix"
work="$scratch/work"
internal_result="$scratch/REHEARSAL-RESULT.json"
cleanup() {
  [[ $scratch == "$expected_scratch_prefix"* ]] \
    || die "refusing recursive cleanup outside the fixed private prefix"
  [[ -d $scratch && ! -L $scratch ]] \
    || die "refusing recursive cleanup of a non-directory scratch path"
  rm -rf -- "$scratch"
}
trap cleanup EXIT
mkdir -p "$work"

gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"

python3 -I -S "$PRODUCER" rehearsal \
  --cas "$cas" \
  --gpgv "$gpgv_path" \
  --zstd "$zstd_path" \
  --launcher "$launcher" \
  --scratch "$work" \
  --result "$internal_result"

[[ -f $internal_result && ! -L $internal_result ]] \
  || die "the producer wrote no canonical rehearsal result"
mapfile -t retained < <(find "$scratch" -type f -print)
[[ ${#retained[@]} -eq 1 && ${retained[0]} == "$internal_result" ]] \
  || die "the rehearsal retained something other than one JSON result"

# A hard link publishes with create-once semantics: an existing destination or
# a path that appears in a race is refused by ln rather than replaced.
ln -- "$internal_result" "$result"
chmod 0444 "$result"
printf 'native-shadow launcher-v2 successor v3: rehearsal PASS: %s\n' "$result"
