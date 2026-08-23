#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_user="${SUDO_USER:-$(id -un)}"
run_group="$(id -gn "$run_user")"
unit="boole-native-shadow-rootfs-builder-$RANDOM-$$"
scratch="$(mktemp -d /tmp/boole-native-shadow-rootfs-builder.XXXXXX)"

cleanup() {
  rm -rf -- "$scratch"
}
trap cleanup EXIT

command -v systemd-run >/dev/null
chown "$run_user:$run_group" "$scratch"
chmod 0700 "$scratch"

systemd-run --quiet --pipe --wait --collect --unit="$unit" \
  --property=Type=oneshot \
  --property="User=$run_user" \
  --property="Group=$run_group" \
  --property="WorkingDirectory=$ROOT" \
  --property=PrivateNetwork=yes \
  --property=NoNewPrivileges=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --setenv="TMPDIR=$scratch" \
  --setenv="PYTHONPYCACHEPREFIX=$scratch/pycache" \
  /usr/bin/python3 -m unittest \
    scripts.test_native_shadow_rootfs_builder \
    scripts.test_native_shadow_rootfs_oci_verify

printf 'native-shadow-rootfs-builder-linux-gate: PASS\n'
