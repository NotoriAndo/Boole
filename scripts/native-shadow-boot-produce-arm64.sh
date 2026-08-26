#!/usr/bin/env bash
# Run the offline half of the arm64 boot image producer on this host.
#
# Two independent jobs are supposed to produce three byte-identical files, which
# they will do only if neither of them decides anything. So this script decides
# nothing either: the transient unit it runs the phase inside is printed by the
# sealed producer authority rather than written out here, the three output names
# come from the same authority by way of the manifest command, and the image size
# is the plan's own floor. Every value this file spells is a value about this
# host -- where the payloads were left, where the launcher was left, where the
# results go.
#
# The network half is deliberately somebody else's. Acquiring the payloads, the
# Rust distribution and the rebuilt launcher all reach the network, and the phase
# below runs with the network taken away, so those steps happen before this
# script is called and are handed to it as paths. A missing input is refused
# here rather than fetched.
#
# Producing the three files is not booting them. Nothing here starts a virtual
# machine.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow arm64 boot produce: FAIL: %s\n' "$*" >&2
  exit 1
}

cas=""
launcher=""
outputs=""
result=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --cas) [[ $# -ge 2 ]] || die "--cas needs a path"; cas=$2; shift 2 ;;
    --launcher) [[ $# -ge 2 ]] || die "--launcher needs a path"; launcher=$2; shift 2 ;;
    --outputs) [[ $# -ge 2 ]] || die "--outputs needs a path"; outputs=$2; shift 2 ;;
    --result) [[ $# -ge 2 ]] || die "--result needs a path"; result=$2; shift 2 ;;
    *) die "unexpected argument: $1" ;;
  esac
done
[[ -n $cas ]] || die "--cas is required: the verified payloads are an input"
[[ -n $launcher ]] || die "--launcher is required: the rebuilt ELF is an input"
[[ -n $outputs ]] || die "--outputs is required"
[[ -n $result ]] || die "--result is required"

for command_name in env gpgv mkdir mktemp mount mountpoint python3 readlink rm \
  systemd-run umount uname zstd; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done

[[ $(uname -s) == "Linux" ]] || die "the produce phase requires Linux"
[[ $(uname -m) == "aarch64" ]] || die "the produce phase requires native aarch64"
# mke2fs -d copies each staged file's owner into the image, so a run that is not
# root produces an image belonging to whoever invoked it.
[[ ${EUID} -eq 0 ]] || die "the produce phase must run as root"

[[ $outputs == /* ]] || die "--outputs must be absolute: $outputs"
[[ $result == "$outputs"/* ]] \
  || die "--result must land under --outputs, the only place the unit may write it"
[[ -d $cas && ! -L $cas ]] || die "the acquired payload store is absent: $cas"
[[ -f $launcher && ! -L $launcher ]] || die "the rebuilt launcher is absent: $launcher"

scratch="$(mktemp -d /tmp/boole-native-shadow-boot-produce.XXXXXX)"
staging="$scratch/staging"
cleanup() {
  if mountpoint -q "$staging" 2>/dev/null; then
    umount "$staging"
  fi
  rm -rf -- "$scratch"
}
trap cleanup EXIT
mkdir -p "$scratch/tmp" "$staging" "$outputs"
export TMPDIR="$scratch/tmp"

# mke2fs walks the staging tree with readdir and never sorts it, so the plan
# names the filesystem that tree has to be built on. Mounting it here also means
# the tree is gone when this run is, and never the runner's own disk order.
mount -t tmpfs -o mode=0755,nodev,nosuid tmpfs "$staging"

gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"

# The sealed authority prints the unit, one argument per line. Writing the same
# arguments here would be a second copy of a frozen list, and a second copy is a
# thing that can weaken without anyone noticing.
isolation=()
while IFS= read -r line; do
  isolation+=("$line")
done < <(
  python3 "$ROOT/scripts/native_shadow_boot_image_produce_arm64_v1.py" isolation-argv \
    --repository-root "$ROOT" \
    --read-write-path "$scratch" \
    --read-write-path "$outputs" \
    -- \
    /usr/bin/env python3 \
    "$ROOT/scripts/native_shadow_boot_produce_phase_arm64_v1.py" produce \
    --scratch "$scratch" \
    --outputs "$outputs" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --launcher "$launcher" \
    --cas "$cas" \
    --result "$result"
)
[[ ${#isolation[@]} -gt 0 ]] || die "the producer authority yielded no transient unit"

"${isolation[@]}"

[[ -f $result && ! -L $result ]] || die "the produce phase wrote no result document"
python3 "$ROOT/scripts/native_shadow_boot_image_produce_arm64_v1.py" manifest \
  --repository-root "$ROOT" \
  --outputs "$outputs"
printf 'native-shadow-boot-produce-arm64: PASS\n'
