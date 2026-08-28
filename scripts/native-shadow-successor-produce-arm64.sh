#!/usr/bin/env bash
# Run the offline half of the successor arm64 boot image producer on this host.
#
# The wrapper beside this one runs the predecessor phase from the first source
# lock and is untouched. This one runs the successor phase from the second, and
# is a separate file for the same reason the phases are separate modules: the
# two differ in which lock they build and in which of them requires the nested
# runtime tree, and a single script with a flag would be one edit away from
# producing the wrong image from the right authority.
#
# Everything else is deliberately identical, because the two images have to be
# comparable. Two independent jobs produce byte-identical files only if neither
# of them decides anything, so this script decides nothing: the transient unit
# comes from the sealed producer authority, the three output names come from the
# same authority, and the image size is the plan's own floor. Every value spelled
# here is a value about this host -- where the payloads were left, where the
# launcher was left, where the results go.
#
# The network half is somebody else's. The payloads, the Rust distribution, the
# writer set and the rebuilt launcher are all acquired before this is called and
# handed in as paths; the phase below runs with the network taken away. A missing
# input is refused here rather than fetched.
#
# Producing the three files is not booting them. Nothing here starts a virtual
# machine, and nothing here claims the image boots or serves.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow arm64 successor produce: FAIL: %s\n' "$*" >&2
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
# The image writer copies each staged file's owner into the image, so a run that
# is not root produces an image belonging to whoever invoked it.
[[ ${EUID} -eq 0 ]] || die "the produce phase must run as root"

[[ $outputs == /* ]] || die "--outputs must be absolute: $outputs"
[[ $result == "$outputs"/* ]] \
  || die "--result must land under --outputs, the only place the unit may write it"
[[ -d $cas && ! -L $cas ]] || die "the acquired payload store is absent: $cas"
[[ -f $launcher && ! -L $launcher ]] || die "the rebuilt launcher is absent: $launcher"

# The one allowed attempt is spent by the existence of an output, so an outputs
# directory that already holds one is refused before anything runs. The phase
# refuses to overwrite its own result too; this is the earlier and cheaper half
# of the same rule, and it is checked before the tmpfs is even mounted.
[[ ! -e $result ]] || die "a successor result is already here and is not replaced: $result"

scratch="$(mktemp -d /tmp/boole-native-shadow-successor-produce.XXXXXX)"
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

# The image writer walks the staging tree with readdir and never sorts it, so
# the plan names the filesystem that tree has to be built on. Mounting it here
# also means the tree is gone when this run is, and never the runner's own disk
# order.
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
    "$ROOT/scripts/native_shadow_successor_produce_phase_arm64_v2.py" produce \
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

# The image is opened and read here rather than inside the unit above. That unit
# is sealed with private devices and a loop mount is exactly a device, so the
# reading has to be its own stage -- which is also the honest arrangement, since
# the stage that checks the work is then not the stage that did it.
python3 "$ROOT/scripts/native_shadow_boot_root_disk_readback_arm64_v1.py" verify \
  --outputs "$outputs" \
  --mountpoint "$scratch/readback" \
  --result "$outputs/ROOT-DISK-READBACK.json"

python3 "$ROOT/scripts/native_shadow_boot_image_produce_arm64_v1.py" manifest \
  --repository-root "$ROOT" \
  --outputs "$outputs"
printf 'native-shadow-successor-produce-arm64: PASS\n'
