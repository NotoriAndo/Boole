#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow portable rootfs replay: FAIL: %s\n' "$*" >&2
  exit 1
}

if [[ ${1:-} == "--resolve-offline" ]]; then
  [[ $# -eq 2 ]] || die "offline resolution requires exactly one scratch path"
  scratch="$2"
  [[ ${EUID} -eq 0 ]] || die "offline resolution must run as root"
  [[ -d "$scratch/cas" && ! -L "$scratch" ]] || die "offline scratch authority differs"
  mkdir -p "$scratch/tmp"
  export TMPDIR="$scratch/tmp"
  gpgv_path="$(readlink -f "$(command -v gpgv)")"
  zstd_path="$(readlink -f "$(command -v zstd)")"
  python3 "$ROOT/scripts/native_shadow_rootfs_portable_v2.py" resolve \
    --cas "$scratch/cas" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --runtime-resolution-output "$scratch/runtime-resolution.json"
  printf 'native-shadow portable rootfs offline resolution: PASS\n'
  exit 0
fi

if [[ ${1:-} == "--offline-build" || ${1:-} == "--offline-probe" ]]; then
  [[ $# -eq 2 ]] || die "offline invocation requires exactly one scratch path"
  offline_phase=$1
  scratch="$2"
  [[ ${EUID} -eq 0 ]] || die "offline replay must run as root"
  [[ -d "$scratch/cas" && ! -L "$scratch" ]] || die "offline scratch authority differs"
  mkdir -p "$scratch/tmp"
  export TMPDIR="$scratch/tmp"

  cas="$scratch/cas"
  runtime_lock="$scratch/runtime-lock.json"
  run_receipt="$scratch/run-receipt.json"
  expectation="$ROOT/native/containment/native-shadow-runtime-rootfs-replay-expectation-v2.json"
  oci="$scratch/oci"
  rootfs="$scratch/rootfs"
  independent_receipt="$scratch/independent-receipt.json"

  if [[ "$offline_phase" == --offline-build ]]; then
    gpgv_path="$(readlink -f "$(command -v gpgv)")"
    zstd_path="$(readlink -f "$(command -v zstd)")"
    python3 "$ROOT/scripts/native_shadow_rootfs_portable_v2.py" seal \
    --cas "$cas" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --runtime-resolution "$scratch/runtime-resolution.json" \
    --runtime-lock-output "$runtime_lock" \
    --run-receipt-output "$run_receipt"

    python3 "$ROOT/scripts/native_shadow_rootfs_builder.py" build \
    --lock "$runtime_lock" \
    --artifact-store "$cas" \
    --repo-root "$ROOT" \
    --output "$oci" >"$scratch/builder-stdout.json"

    runtime_lock_sha="$(jq -er '.runtimeLockSha256' "$run_receipt")"
    builder_sha="$(jq -er '.authority.builderSha256' "$run_receipt")"
    layer_digest="$(jq -er '.expectedOutput.layerDigest' "$expectation")"
    content_sha="$(jq -er '.expectedOutput.rootfsContentManifestSha256' "$expectation")"

    python3 "$ROOT/scripts/native_shadow_rootfs_oci_verify.py" verify \
    --layout "$oci" \
    --expected-source-lock-sha256 "$runtime_lock_sha" \
    --expected-builder-sha256 "$builder_sha" \
    --expected-layer-digest "$layer_digest" \
    --expected-content-manifest-sha256 "$content_sha" >"$independent_receipt"

    python3 - "$oci/BUILD-RECEIPT.json" "$independent_receipt" <<'PY'
import json
import pathlib
import sys

builder = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
independent = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8"))
if builder != independent:
    raise SystemExit("builder and independent verifier receipts differ")
PY

    python3 "$ROOT/scripts/native_shadow_rootfs_portable_v2.py" verify-output \
    --build-receipt "$oci/BUILD-RECEIPT.json" \
    --run-receipt "$run_receipt"

    mkdir -p "$rootfs"
    layer_blob="$oci/blobs/sha256/${layer_digest#sha256:}"
    [[ -f "$layer_blob" && ! -L "$layer_blob" ]] || die "verified OCI layer is unavailable"
    tar --extract --file "$layer_blob" --directory "$rootfs" --numeric-owner
    install -o 0 -g 0 -m 0444 \
      "$oci/ROOTFS-CONTENT-MANIFEST.json" \
      "$scratch/ROOTFS-CONTENT-MANIFEST.json"
    printf 'native-shadow portable rootfs exact build: PASS\n'
    exit 0
  fi

  [[ -d "$rootfs" && ! -L "$rootfs" ]] || die "exact rootfs is unavailable for probing"
  [[ -f "$scratch/ROOTFS-CONTENT-MANIFEST.json" ]] \
    || die "exact rootfs content manifest is unavailable for probing"

  # The verified OCI stays byte-identical to the frozen expectation.  The
  # qualification process nevertheless needs one fixed numeric account record
  # so Cargo can resolve a home after chroot drops to uid 65534 with HOME
  # intentionally absent.  Treat this like the gate-owned /dev/null bind below:
  # an explicit, reviewed runtime input that never becomes rootfs authority.
  runtime_passwd="$ROOT/native/containment/native-shadow-runtime-passwd-v2"
  [[ -f "$runtime_passwd" && ! -L "$runtime_passwd" ]] \
    || die "fixed qualification account file is unavailable"
  passwd_digest="$(sha256sum "$runtime_passwd")"
  [[ "$passwd_digest" == \
    "0de8ff37fb2dc7fb99e17f761181d87ce4380d6a3fbca2b8c14b44c56e4ca9cf  $runtime_passwd" ]] \
    || die "fixed qualification account file differs"
  mkdir -p "$rootfs/etc"
  [[ ! -e "$rootfs/etc/passwd" && ! -L "$rootfs/etc/passwd" ]] \
    || die "verified OCI unexpectedly owns /etc/passwd"
  install -m 0444 -o 0 -g 0 "$runtime_passwd" "$rootfs/etc/passwd"
  cmp --silent "$runtime_passwd" "$rootfs/etc/passwd" \
    || die "qualification account installation differs"

  # The OCI layer intentionally contains no device nodes.  Bind only the
  # minimum kernel devices needed by Python/cargo into this unit's private
  # mount namespace; none survive the unit.
  mkdir -p "$rootfs/dev" "$rootfs/proc"
  : >"$rootfs/dev/null"
  mount --bind /dev/null "$rootfs/dev/null"
  [[ -c "$rootfs/dev/null" ]] || die "runtime /dev/null bind is not a character device"
  # The frozen rustc delegates final linking to its lld wrapper.  That wrapper
  # resolves itself through /proc/self/exe, so give this already-private mount
  # namespace its own read-only-by-policy proc view.  nosuid/nodev/noexec keeps
  # it metadata-only and the explicit umount below proves normal-path cleanup.
  mount -t proc -o nosuid,nodev,noexec proc "$rootfs/proc"
  [[ -e "$rootfs/proc/self/exe" ]] || die "private proc self identity is unavailable"

  mkdir -p "$rootfs/probe" "$rootfs/scratch"
  cp -a "$ROOT/fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/." \
    "$rootfs/probe/"
  cat >"$rootfs/probe/c-probe.c" <<'C'
int main(void) { return 0; }
C
  chown -R 0:0 "$rootfs/probe"
  chmod -R a-w "$rootfs/probe"
  chown 65534:65534 "$rootfs/scratch"
  chmod 0700 "$rootfs/scratch"

  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /usr/bin/python3.12 --version
  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /opt/boole/native-checker-toolchain/bin/rustc -vV
  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /opt/boole/native-checker-toolchain/bin/cargo -Vv
  TMPDIR=/scratch chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /usr/bin/x86_64-linux-gnu-gcc-13 /probe/c-probe.c -o /scratch/c-probe
  chroot --groups='' --userspec=65534:65534 "$rootfs" /scratch/c-probe

  umount "$rootfs/proc"
  umount "$rootfs/dev/null"
  printf 'native-shadow portable rootfs offline probe: PASS\n'
  exit 0
fi

for command_name in awk cmp getent gpgv id install jq mount python3 readlink sha256sum sudo systemd-run tar timeout umount zstd; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done

[[ $# -eq 0 ]] || die "unexpected arguments"
[[ ${EUID} -eq 0 ]] || die "the clean Linux replay must run as root"
[[ $(uname -m) == "x86_64" ]] || die "the frozen rootfs replay requires Linux amd64"

scratch="$(mktemp -d /tmp/boole-native-shadow-rootfs-replay.XXXXXX)"
cleanup() {
  rm -rf -- "$scratch"
}
trap cleanup EXIT

cas="$scratch/cas"
runtime_resolution="$scratch/runtime-resolution.json"
runtime_lock="$scratch/runtime-lock.json"
run_receipt="$scratch/run-receipt.json"
mkdir -p "$cas"

gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"
[[ -f "$gpgv_path" && ! -L "$gpgv_path" ]] || die "gpgv must resolve to a regular file"
[[ -f "$zstd_path" && ! -L "$zstd_path" ]] || die "zstd must resolve to a regular file"

# Only the two frozen fetch operations run outside PrivateNetwork.  Signed
# resolution is fixed offline before payload fetch; sealing, exact-output
# adoption decisions and every executable probe stay in networkless units.
python3 "$ROOT/scripts/native_shadow_rootfs_acquire.py" fetch-metadata \
  --plan "$ROOT/native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json" \
  --cas "$cas"

resolution_unit="boole-native-shadow-rootfs-resolution-${RANDOM}-${$}"
systemd-run \
  --quiet \
  --pipe \
  --wait \
  --collect \
  --unit "$resolution_unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux.sh" \
    --resolve-offline "$scratch"

python3 "$ROOT/scripts/native_shadow_rootfs_portable_v2.py" fetch-payloads \
  --cas "$cas" \
  --gpgv "$gpgv_path" \
  --zstd "$zstd_path" \
  --runtime-resolution "$runtime_resolution"

# A second invocation performs only the exact build and independent
# verification with networking removed by the kernel. The extracted bytes are
# then consumed unchanged by the real launcher service before the separate
# diagnostic probe is allowed to add its transient passwd/dev/probe files.
unit="boole-native-shadow-rootfs-replay-${RANDOM}-${$}"
systemd-run \
  --quiet \
  --pipe \
  --wait \
  --collect \
  --unit "$unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux.sh" \
    --offline-build "$scratch"

run_user=${SUDO_USER:-}
[[ -n "$run_user" && "$run_user" != root ]] \
  || die "the named launcher gate requires the original unprivileged CI user"
id "$run_user" >/dev/null 2>&1 || die "the original CI user no longer resolves"
run_home=$(getent passwd "$run_user" | awk -F: 'NF == 7 { print $6 }')
[[ -n "$run_home" && -x "$run_home/.cargo/bin/cargo" ]] \
  || die "the original CI user lacks the pinned Rust toolchain"
chmod 0711 "$scratch"
(
  cd "$ROOT"
  timeout --foreground --signal=TERM --kill-after=15s 600s \
    sudo -u "$run_user" env \
      "HOME=$run_home" \
      "CARGO_HOME=$run_home/.cargo" \
      "RUSTUP_HOME=$run_home/.rustup" \
      "PATH=$run_home/.cargo/bin:$PATH" \
      ./scripts/native-shadow-manager-cgroup-gate.sh \
      --closed-local-replay-rootfs \
      "$scratch/rootfs" \
      "$scratch/ROOTFS-CONTENT-MANIFEST.json"
)

probe_unit="boole-native-shadow-rootfs-probe-${RANDOM}-${$}"
systemd-run \
  --quiet \
  --pipe \
  --wait \
  --collect \
  --unit "$probe_unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux.sh" \
    --offline-probe "$scratch"

printf 'native-shadow-portable-rootfs-replay-linux: PASS\n'
