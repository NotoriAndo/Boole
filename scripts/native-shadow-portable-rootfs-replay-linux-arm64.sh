#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PYTHONDONTWRITEBYTECODE=1
export PYTHONHASHSEED=0
export LANG=C
export LC_ALL=C

die() {
  printf 'native-shadow arm64 rootfs replay: FAIL: %s\n' "$*" >&2
  exit 1
}

if [[ ${1:-} == "--offline-resolve" ]]; then
  [[ $# -eq 2 ]] || die "offline resolution requires one scratch path"
  scratch=$2
  [[ ${EUID} -eq 0 && -d "$scratch/cas" && ! -L "$scratch" ]] \
    || die "offline resolution authority differs"
  mkdir -p "$scratch/tmp"
  export TMPDIR="$scratch/tmp"
  gpgv_path="$(readlink -f "$(command -v gpgv)")"
  zstd_path="$(readlink -f "$(command -v zstd)")"
  python3 "$ROOT/scripts/native_shadow_rootfs_portable_arm64_v1.py" resolve \
    --cas "$scratch/cas" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --runtime-resolution-output "$scratch/runtime-resolution.json"
  printf 'native-shadow arm64 signed resolution: PASS\n'
  exit 0
fi

if [[ ${1:-} == "--offline-build" ]]; then
  [[ $# -eq 2 ]] || die "offline build requires one scratch path"
  scratch=$2
  [[ ${EUID} -eq 0 && -d "$scratch/cas" && ! -L "$scratch" ]] \
    || die "offline build authority differs"
  mkdir -p "$scratch/tmp"
  export TMPDIR="$scratch/tmp"
  gpgv_path="$(readlink -f "$(command -v gpgv)")"
  zstd_path="$(readlink -f "$(command -v zstd)")"
  runtime_lock="$scratch/runtime-lock.json"
  run_receipt="$scratch/run-receipt.json"
  oci="$scratch/oci"
  independent_receipt="$scratch/independent-receipt.json"

  python3 "$ROOT/scripts/native_shadow_rootfs_portable_arm64_v1.py" seal \
    --cas "$scratch/cas" \
    --gpgv "$gpgv_path" \
    --zstd "$zstd_path" \
    --runtime-resolution "$scratch/runtime-resolution.json" \
    --runtime-lock-output "$runtime_lock" \
    --run-receipt-output "$run_receipt"

  python3 "$ROOT/scripts/native_shadow_rootfs_builder_arm64_v1.py" build \
    --lock "$runtime_lock" \
    --artifact-store "$scratch/cas" \
    --repo-root "$ROOT" \
    --output "$oci" >"$scratch/builder-stdout.json"

  runtime_lock_sha="$(jq -er '.runtimeLockSha256' "$run_receipt")"
  builder_sha="$(jq -er '.authority.builderSha256' "$run_receipt")"
  expectation="$ROOT/native/containment/native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
  layer_digest="$(jq -er '.expectedOutput.layerDigest' "$expectation")"
  content_sha="$(jq -er '.expectedOutput.rootfsContentManifestSha256' "$expectation")"
  python3 "$ROOT/scripts/native_shadow_rootfs_oci_verify_arm64_v1.py" verify \
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
  python3 "$ROOT/scripts/native_shadow_rootfs_portable_arm64_v1.py" verify-output \
    --build-receipt "$oci/BUILD-RECEIPT.json" \
    --run-receipt "$run_receipt"

  mkdir -p "$scratch/rootfs"
  layer_blob="$oci/blobs/sha256/${layer_digest#sha256:}"
  [[ -f "$layer_blob" && ! -L "$layer_blob" ]] \
    || die "verified arm64 OCI layer is absent"
  tar --extract --file "$layer_blob" --directory "$scratch/rootfs" --numeric-owner
  install -o 0 -g 0 -m 0444 \
    "$oci/ROOTFS-CONTENT-MANIFEST.json" \
    "$scratch/ROOTFS-CONTENT-MANIFEST.json"
  printf 'native-shadow arm64 exact rootfs build: PASS\n'
  exit 0
fi

if [[ ${1:-} == "--offline-parity" ]]; then
  [[ $# -eq 2 ]] || die "offline parity requires one scratch path"
  scratch=$2
  rootfs="$scratch/rootfs"
  [[ ${EUID} -eq 0 && -d "$rootfs" && ! -L "$rootfs" ]] \
    || die "arm64 parity rootfs differs"
  [[ -f "$scratch/ROOTFS-CONTENT-MANIFEST.json" \
    && ! -L "$scratch/ROOTFS-CONTENT-MANIFEST.json" ]] \
    || die "arm64 parity content manifest differs"

  # Only the direct diagnostic parity phase may add transient probe inputs.
  # The real launcher containment gate has already consumed the exact frozen
  # extraction before execution reaches this branch.
  runtime_passwd="$ROOT/native/containment/native-shadow-runtime-passwd-v2"
  [[ $(sha256sum "$runtime_passwd" | awk '{print $1}') == \
    "0de8ff37fb2dc7fb99e17f761181d87ce4380d6a3fbca2b8c14b44c56e4ca9cf" ]] \
    || die "fixed qualification account file differs"
  mkdir -p "$rootfs/etc" "$rootfs/probe/real" \
    "$rootfs/probe/synthetic" "$rootfs/scratch" "$rootfs/dev" "$rootfs/proc"
  [[ ! -e "$rootfs/etc/passwd" && ! -L "$rootfs/etc/passwd" ]] \
    || die "verified arm64 OCI unexpectedly owns /etc/passwd"
  install -m 0444 -o 0 -g 0 "$runtime_passwd" "$rootfs/etc/passwd"
  cp -a "$ROOT/fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/." \
    "$rootfs/probe/real/"
  cp -a "$ROOT/fixtures/native-shadow/rust-tuple-struct-project-v1/." \
    "$rootfs/probe/synthetic/"
  chown -R 0:0 "$rootfs/probe"
  chmod -R a-w "$rootfs/probe"
  chown 65534:65534 "$rootfs/scratch"
  chmod 0700 "$rootfs/scratch"
  : >"$rootfs/dev/null"

  cleanup_mounts() {
    if mountpoint -q "$rootfs/proc"; then
      umount "$rootfs/proc"
    fi
    if mountpoint -q "$rootfs/dev/null"; then
      umount "$rootfs/dev/null"
    fi
  }
  trap cleanup_mounts EXIT
  mount --bind /dev/null "$rootfs/dev/null"
  mount -t proc -o nosuid,nodev,noexec proc "$rootfs/proc"
  [[ -c "$rootfs/dev/null" && -e "$rootfs/proc/self/exe" ]] \
    || die "private arm64 runtime mounts differ"

  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /usr/bin/python3.12 --version
  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /opt/boole/native-checker-toolchain/bin/rustc -vV \
    | tee "$scratch/rustc-vv.txt"
  grep -Fx 'host: aarch64-unknown-linux-gnu' "$scratch/rustc-vv.txt" >/dev/null \
    || die "arm64 rustc host identity differs"
  chroot --groups='' --userspec=65534:65534 "$rootfs" \
    /opt/boole/native-checker-toolchain/bin/cargo -Vv \
    | tee "$scratch/cargo-vv.txt"

  run_case() {
    name=$1
    task=$2
    submission=$3
    expected_verdict=$4
    expected_reason=$5
    result="$scratch/result-$name.json"
    stderr="$scratch/stderr-$name.txt"
    timing="$scratch/time-$name.txt"
    if /usr/bin/time -f '%e %M' -o "$timing" \
      /usr/bin/env -i LANG=C LC_ALL=C PATH=/usr/bin:/bin:/usr/sbin:/sbin \
      chroot --groups='' --userspec=65534:65534 "$rootfs" \
      /usr/bin/python3.12 -I -S \
      /usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py \
      --task "$task" \
      --submission "$submission" \
      --toolchain-bin /opt/boole/native-checker-toolchain/bin \
      --scratch-root /scratch >"$result" 2>"$stderr"; then
      :
    else
      status=$?
      [[ ! -s "$stderr" ]] || cat "$stderr" >&2
      [[ ! -s "$result" ]] || cat "$result" >&2
      die "checker command failed for $name with status $status"
    fi
    [[ ! -s "$stderr" ]] || die "checker wrote stderr for $name"
    python3 - "$result" "$expected_verdict" "$expected_reason" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
raw = path.read_bytes()
if not raw.endswith(b"\n") or raw.count(b"\n") != 1:
    raise SystemExit("checker output framing differs")
value = json.loads(raw)
if value.get("verdict") != sys.argv[2] or value.get("reasonCode") != sys.argv[3]:
    raise SystemExit(f"checker verdict differs: {value}")
PY
  }

  run_case accepted /probe/real/task.json /probe/real/accepted.rs accepted accepted
  run_case accepted-replay /probe/real/task.json /probe/real/accepted.rs accepted accepted
  cmp --silent "$scratch/result-accepted.json" "$scratch/result-accepted-replay.json" \
    || die "accepted replay bytes differ"
  run_case empty /probe/real/task.json /probe/real/empty.rs \
    deterministic_reject malformed_patch_region
  run_case tampered /probe/real/task.json /probe/real/tampered.rs \
    deterministic_reject compile_or_hidden_test_failed
  run_case constant /probe/real/task.json /probe/real/constant.rs \
    deterministic_reject compile_or_hidden_test_failed
  run_case cross-real-to-synthetic /probe/synthetic/task.json \
    /probe/real/accepted.rs deterministic_reject outside_patch_modified
  run_case cross-synthetic-to-real /probe/real/task.json \
    /probe/synthetic/accepted.rs deterministic_reject outside_patch_modified

  python3 - "$scratch" "$ROOT" <<'PY'
import json
import pathlib
import sys

scratch = pathlib.Path(sys.argv[1])
root = pathlib.Path(sys.argv[2])
build = json.loads((scratch / "oci/BUILD-RECEIPT.json").read_text())
content = json.loads((scratch / "oci/ROOTFS-CONTENT-MANIFEST.json").read_text())
portable_lock = json.loads(
    (
        root
        / "native/containment/native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
    ).read_text()
)
case_names = [
    "accepted",
    "accepted-replay",
    "empty",
    "tampered",
    "constant",
    "cross-real-to-synthetic",
    "cross-synthetic-to-real",
]
measurements = {}
verdicts = {}
for name in case_names:
    wall, rss = (scratch / f"time-{name}.txt").read_text().split()
    measurements[name] = {"wallSeconds": float(wall), "maxRssKiB": int(rss)}
    result = json.loads((scratch / f"result-{name}.json").read_text())
    verdicts[name] = {
        "verdict": result["verdict"],
        "reasonCode": result["reasonCode"],
    }
entry_count = len(content["entries"])
layer_bytes = build["layerSizeBytes"]
authority_bytes = sum(row["sizeBytes"] for row in portable_lock["artifacts"])
if entry_count > 200_000 or layer_bytes > 2 * 1024**3 or authority_bytes > 2 * 1024**3:
    raise SystemExit("MAC.2 frozen guest cap exceeded")
result = {
    "activationAllowed": False,
    "bindingsAndNegativeControls": "EXACT-PARITY",
    "caseVerdicts": verdicts,
    "completedSubgate": "CLOSED-LOCAL-LINUX-ARM64-AUTHORITY-PARITY",
    "containmentEnforcementParity": "EXACT",
    "mac2Status": "PARTIAL",
    "openRequirement": "POST-UPDATE-IMAGE-AND-RUNTIME-AUTHORITY-REVERIFICATION",
    "platform": {"architecture": "arm64", "os": "linux"},
    "productionByteProvenanceComplete": False,
    "replayByteIdentical": True,
    "resourceMeasurements": {
        "authorityInputBytes": authority_bytes,
        "checkerCases": measurements,
        "rootfsContentEntryCount": entry_count,
        "rootfsLayerBytes": layer_bytes,
    },
    "resourcePolicyDocumentParity": "EXACT-EXCEPT-FROZEN-ARCHITECTURE-IDENTITY",
    "resourcePolicyEnforcementParity": "EXACT",
    "schema": "boole.native-shadow.mac2-arm64-parity-result.v1",
    "semanticVerdictParity": "EXACT",
}
(scratch / "MAC2-RESULT.json").write_text(
    json.dumps(result, sort_keys=True, indent=2) + "\n", encoding="utf-8"
)
PY
  cat "$scratch/MAC2-RESULT.json"
  cleanup_mounts
  trap - EXIT
  printf 'native-shadow Linux/arm64 exact verdict parity: PASS\n'
  exit 0
fi

for command_name in awk chroot cmp env getent gpgv grep id install jq mount mountpoint \
  python3 readlink sha256sum sudo systemd-run tar time timeout umount zstd; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done
[[ $# -eq 0 ]] || die "unexpected arguments"
[[ ${EUID} -eq 0 ]] || die "the arm64 rootfs replay must run as root"
[[ $(uname -s) == "Linux" ]] || die "the arm64 rootfs replay requires Linux"
[[ $(uname -m) == "aarch64" ]] || die "the arm64 rootfs replay requires native aarch64"

python3 -m unittest scripts.test_native_shadow_arm64_authority -v
scratch="$(mktemp -d /tmp/boole-native-shadow-arm64-rootfs.XXXXXX)"
cleanup() {
  rm -rf -- "$scratch"
}
trap cleanup EXIT
mkdir -p "$scratch/cas" "$scratch/tmp"
export TMPDIR="$scratch/tmp"
gpgv_path="$(readlink -f "$(command -v gpgv)")"
zstd_path="$(readlink -f "$(command -v zstd)")"

python3 "$ROOT/scripts/native_shadow_rootfs_acquire_arm64_v1.py" fetch-metadata \
  --plan "$ROOT/native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json" \
  --builder "$ROOT/scripts/native_shadow_rootfs_builder_arm64_v1.py" \
  --cas "$scratch/cas"

resolution_unit="boole-native-shadow-arm64-resolution-${RANDOM}-${$}"
systemd-run --quiet --pipe --wait --collect --unit "$resolution_unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh" \
  --offline-resolve "$scratch"

python3 "$ROOT/scripts/native_shadow_rootfs_portable_arm64_v1.py" fetch-payloads \
  --cas "$scratch/cas" \
  --gpgv "$gpgv_path" \
  --zstd "$zstd_path" \
  --runtime-resolution "$scratch/runtime-resolution.json"

build_unit="boole-native-shadow-arm64-build-${RANDOM}-${$}"
systemd-run --quiet --pipe --wait --collect --unit "$build_unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh" \
  --offline-build "$scratch"

run_user=${SUDO_USER:-}
[[ -n "$run_user" && "$run_user" != root ]] \
  || die "the arm64 launcher gate requires the original unprivileged CI user"
id "$run_user" >/dev/null 2>&1 || die "the original CI user no longer resolves"
run_home=$(getent passwd "$run_user" | awk -F: 'NF == 7 { print $6 }')
[[ -n "$run_home" && -x "$run_home/.cargo/bin/cargo" ]] \
  || die "the original CI user lacks the pinned Rust toolchain"
chmod 0711 "$scratch"
# A 1,200-second global cap cut the native ARM run after every diagnostic,
# rootfs-drift rejection and HTTP case had passed, while crash/restart was
# still advancing normally. Use a 2,100-second global CI orchestration cap for
# this manager invocation and leave another 600 seconds in the 45-minute job
# for acquisition, exact rootfs construction and direct parity. This is not a
# claim that one outer cap sums every theoretical nested wait. Every checker,
# HTTP, crash/restart and resource-policy deadline remains independently frozen.
arm64_manager_deadline_seconds=2100
(
  cd "$ROOT"
  timeout --foreground --signal=TERM --kill-after=15s \
    "${arm64_manager_deadline_seconds}s" \
    sudo -u "$run_user" env \
      "HOME=$run_home" \
      "CARGO_HOME=$run_home/.cargo" \
      "RUSTUP_HOME=$run_home/.rustup" \
      "PATH=$run_home/.cargo/bin:$PATH" \
      ./scripts/native-shadow-manager-cgroup-gate.sh \
      --closed-local-replay-rootfs-arm64 \
      "$scratch/rootfs" \
      "$scratch/ROOTFS-CONTENT-MANIFEST.json"
)

parity_unit="boole-native-shadow-arm64-parity-${RANDOM}-${$}"
systemd-run --quiet --pipe --wait --collect --unit "$parity_unit" \
  --property=PrivateNetwork=yes \
  --property=ProtectSystem=strict \
  --property="ReadOnlyPaths=$ROOT" \
  --property="ReadWritePaths=$scratch" \
  --property=NoNewPrivileges=yes \
  --property=PrivateDevices=yes \
  --property=PrivateMounts=yes \
  --property=RestrictAddressFamilies=AF_UNIX \
  /usr/bin/env bash "$ROOT/scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh" \
  --offline-parity "$scratch"

[[ -f "$scratch/MAC2-RESULT.json" ]] || die "MAC2-RESULT.json is absent"
printf 'native-shadow-portable-rootfs-replay-linux-arm64: PASS\n'
