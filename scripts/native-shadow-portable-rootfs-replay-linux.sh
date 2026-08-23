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

if [[ ${1:-} == "--offline" ]]; then
  [[ $# -eq 2 ]] || die "offline invocation requires exactly one scratch path"
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

  checker=/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py
  toolchain=/opt/boole/native-checker-toolchain/bin
  chroot --groups='' --userspec=65534:65534 "$rootfs" /usr/bin/python3.12 "$checker" \
    --task /probe/task.json \
    --submission /probe/accepted.rs \
    --toolchain-bin "$toolchain" \
    --scratch-root /scratch >"$rootfs/scratch/accepted-result.json"
  chroot --groups='' --userspec=65534:65534 "$rootfs" /usr/bin/python3.12 "$checker" \
    --task /probe/task.json \
    --submission /probe/tampered.rs \
    --toolchain-bin "$toolchain" \
    --scratch-root /scratch >"$rootfs/scratch/tampered-result.json"

  if [[ "$(jq -er '[.verdict, .reasonCode] | join("/")' \
    "$rootfs/scratch/accepted-result.json")" != "accepted/accepted" ]]; then
    printf '%s\n' \
      'DIAGNOSTIC-ONLY: the second run is not adjudication; the first verdict remains authoritative' \
      >&2
    chroot --groups='' --userspec=65534:65534 "$rootfs" \
      /usr/bin/python3.12 - \
      "$checker" /probe/task.json /probe/accepted.rs "$toolchain" /scratch <<'PY' >&2
import importlib.util
import pathlib
import sys

checker_path, task_path, submission_path, toolchain_path, scratch_path = sys.argv[1:]
spec = importlib.util.spec_from_file_location("boole_checker_diagnostic", checker_path)
if spec is None or spec.loader is None:
    raise SystemExit("diagnostic checker import unavailable")
checker_module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = checker_module
spec.loader.exec_module(checker_module)
original_run_contained = checker_module._run_contained


def traced_run_contained(command, cwd, env, limits):
    code, output = original_run_contained(command, cwd, env, limits)
    print(f"DIAGNOSTIC-ONLY contained exit code: {code}", file=sys.stderr)
    sys.stderr.buffer.write(output[:65536])
    if output and not output.endswith(b"\n"):
        sys.stderr.buffer.write(b"\n")
    return code, output


checker_module._run_contained = traced_run_contained
sys.argv = [
    checker_path,
    "--task", task_path,
    "--submission", submission_path,
    "--toolchain-bin", toolchain_path,
    "--scratch-root", scratch_path,
]
raise SystemExit(checker_module.main())
PY
  fi

  python3 - \
    "$rootfs/probe/task.json" \
    "$rootfs/scratch/accepted-result.json" \
    "$rootfs/scratch/tampered-result.json" <<'PY'
import hashlib
import json
import pathlib
import sys

task_path = pathlib.Path(sys.argv[1])
task_raw = task_path.read_bytes()
task = json.loads(task_raw)
expected_binding = {
    "checkerTaskId": task["checkerTaskId"],
    "taskDigest": hashlib.sha256(task_raw).hexdigest(),
}
cases = (
    (sys.argv[2], "accepted", "accepted"),
    (sys.argv[3], "deterministic_reject", "compile_or_hidden_test_failed"),
)
for path, verdict, reason in cases:
    result = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    if result.get("verdict") != verdict or result.get("reasonCode") != reason:
        raise SystemExit(
            f"checker verdict differs: {path}: "
            f"verdict={result.get('verdict')!r} "
            f"reasonCode={result.get('reasonCode')!r}"
        )
    for key, value in expected_binding.items():
        if result.get(key) != value:
            raise SystemExit(f"checker binding differs: {path}: {key}")
PY

  umount "$rootfs/proc"
  umount "$rootfs/dev/null"
  printf 'native-shadow portable rootfs offline probe: PASS\n'
  exit 0
fi

for command_name in cmp gpgv install jq mount python3 readlink sha256sum systemd-run tar umount zstd; do
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

# A second invocation performs the build, independent verification and real
# compiler/checker probes with networking removed by the kernel.  The tracked
# repository is read-only and only this gate-owned scratch directory is writable.
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
    --offline "$scratch"

printf 'native-shadow-portable-rootfs-replay-linux: PASS\n'
