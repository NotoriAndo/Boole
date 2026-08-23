#!/usr/bin/env bash
set -euo pipefail

# Exercise the production launcher privilege self-check under the exact
# systemd service shape, then prove that both a missing and an extra
# capability fail closed. This script builds as the unprivileged CI runner and
# executes only a byte-identical, root-owned staged copy inside systemd.

die() {
  echo "native-shadow launcher privilege gate: $*" >&2
  exit 1
}

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"
command -v systemd-run >/dev/null || die "systemd-run is unavailable"
command -v sha256sum >/dev/null || die "sha256sum is unavailable"
command -v python3 >/dev/null || die "python3 is unavailable"

temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-launcher-build.XXXXXX")
launcher_path=''
declare -a logs=()
declare -a units=()

cleanup_privilege_gate() {
  set +e
  local unit log
  for unit in "${units[@]}"; do
    sudo systemctl stop "${unit}.service" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "${unit}.service" >/dev/null 2>&1 || :
  done
  [[ -n "$launcher_path" ]] && sudo rm -f "$launcher_path"
  for log in "${logs[@]}"; do
    rm -f "$log"
  done
  rm -f "$build_json"
}
trap cleanup_privilege_gate EXIT

cargo test --locked -p boole-native-shadow-launcher --lib --no-run \
  --message-format=json >"$build_json"

mapfile -t test_executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole_native_shadow_launcher"
        and item.get("profile", {}).get("test") is True
        and item.get("executable")
    ):
        print(item["executable"])
' "$build_json"
)
[[ ${#test_executables[@]} -eq 1 ]] \
  || die "expected exactly one launcher lib test executable, got ${#test_executables[@]}"
test_executable=${test_executables[0]}
[[ -x "$test_executable" ]] || die "launcher lib test executable is not executable"

launcher_path=$(sudo mktemp /run/boole-native-shadow-launcher-captest.XXXXXX)
sudo install -o root -g root -m 0555 "$test_executable" "$launcher_path"
source_sha=$(sha256sum "$test_executable" | awk '{ print $1 }')
staged_sha=$(sudo sha256sum "$launcher_path" | awk '{ print $1 }')
[[ "$source_sha" == "$staged_sha" ]] \
  || die "staged launcher test bytes differ from the reviewed executable"

suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
suffix=${suffix//[^a-zA-Z0-9-]/-}
test_name=privilege::tests::real_kernel_privilege_matches_frozen_policy

wait_until_collected() {
  local unit=$1
  local i state
  for ((i = 0; i < 100; i++)); do
    state=$(sudo systemctl show "${unit}.service" --property=LoadState --value 2>/dev/null || :)
    [[ "$state" == not-found ]] && return 0
    sleep 0.05
  done
  die "transient unit ${unit}.service was not collected"
}

run_privilege_case() {
  local case_name=$1
  local capability_set=$2
  local expected_result=$3
  local expected_actual=${4:-}
  local unit="boole-native-shadow-cap-${case_name}-${suffix}"
  local log
  log=$(mktemp "$temp_root/boole-native-shadow-cap-${case_name}.XXXXXX")
  logs+=("$log")
  units+=("$unit")

  set +e
  sudo systemd-run --quiet --pipe --wait --collect --unit="$unit" \
    --property=Type=exec --property=Delegate=yes \
    --property=User=root --property=Group=root \
    --property="CapabilityBoundingSet=${capability_set}" \
    --property=AmbientCapabilities= --property=NoNewPrivileges=no \
    --property=WorkingDirectory=/ \
    "$launcher_path" "$test_name" --ignored --exact --nocapture \
    >"$log" 2>&1
  local status=$?
  set -e
  cat "$log"

  case "$expected_result" in
    pass)
      [[ $status -eq 0 ]] || die "$case_name unexpectedly failed"
      ;;
    reject)
      [[ $status -ne 0 ]] || die "$case_name unexpectedly passed"
      grep -qF "capability set mismatch" "$log" \
        || die "$case_name did not fail through the production capability verifier"
      grep -qF "actual ${expected_actual}" "$log" \
        || die "$case_name did not report expected capability mask ${expected_actual}"
      ;;
    *)
      die "unknown expected result: $expected_result"
      ;;
  esac

  wait_until_collected "$unit"
}

run_privilege_case exact \
  'CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' pass
run_privilege_case missing \
  'CAP_SETGID CAP_SETUID CAP_SETPCAP' reject 0x00000000000001c0
run_privilege_case extra \
  'CAP_CHOWN CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' reject 0x00000000002001c1

echo "native-shadow launcher privilege gate: PASS"
