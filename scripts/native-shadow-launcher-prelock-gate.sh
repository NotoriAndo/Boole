#!/usr/bin/env bash
set -euo pipefail

# Exercise the argument-free production composition of launcher privilege,
# the three installed authority files, and the two fixed NSS identities. The
# reviewed test executable and authority bytes are staged root-owned before a
# transient exact-capability service calls the production verifier.

die() {
  echo "native-shadow launcher pre-lock gate: $*" >&2
  exit 1
}

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"
command -v systemd-run >/dev/null || die "systemd-run is unavailable"
command -v sha256sum >/dev/null || die "sha256sum is unavailable"
command -v python3 >/dev/null || die "python3 is unavailable"

temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-launcher-prelock-build.XXXXXX")
launcher_path=''
unit=''
log=''
authority_stage=''
authority_share=''
authority_parent=''
authority_directory=''
declare -a installed_basenames=()

cleanup_prelock_gate() {
  set +e
  if [[ -n "$unit" ]]; then
    sudo systemctl stop "${unit}.service" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "${unit}.service" >/dev/null 2>&1 || :
  fi
  [[ -n "$launcher_path" ]] && sudo rm -f "$launcher_path"
  local basename
  for basename in "${installed_basenames[@]}"; do
    sudo rm -f "$authority_directory/$basename"
  done
  if [[ -n "$authority_stage" ]]; then
    sudo rmdir "$authority_directory" >/dev/null 2>&1 || :
    sudo rmdir "$authority_parent" >/dev/null 2>&1 || :
    sudo rmdir "$authority_share" >/dev/null 2>&1 || :
    sudo rmdir "$authority_stage" >/dev/null 2>&1 || :
  fi
  [[ -n "$log" ]] && rm -f "$log"
  rm -f "$build_json"
}
trap cleanup_prelock_gate EXIT

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

launcher_path=$(sudo mktemp /run/boole-native-shadow-launcher-prelock.XXXXXX)
sudo install -o root -g root -m 0555 "$test_executable" "$launcher_path"
source_sha=$(sha256sum "$test_executable" | awk '{ print $1 }')
staged_sha=$(sudo sha256sum "$launcher_path" | awk '{ print $1 }')
[[ "$source_sha" == "$staged_sha" ]] \
  || die "staged launcher test bytes differ from the reviewed executable"

authority_stage=$(sudo mktemp -d /run/boole-native-shadow-authority.XXXXXX)
authority_share="$authority_stage/share"
authority_parent="$authority_share/boole"
authority_directory="$authority_parent/native-shadow"
sudo chmod 0700 "$authority_stage"
sudo install -d -o root -g root -m 0755 "$authority_share"
sudo install -d -o root -g root -m 0755 "$authority_parent"
sudo install -d -o root -g root -m 0555 "$authority_directory"

install_authority() {
  local source=$1
  local basename=$2
  local destination="$authority_directory/$basename"
  local source_sha installed_sha
  sudo install -o root -g root -m 0444 "$source" "$destination"
  installed_basenames+=("$basename")
  source_sha=$(sha256sum "$source" | awk '{ print $1 }')
  installed_sha=$(sudo sha256sum "$destination" | awk '{ print $1 }')
  [[ "$source_sha" == "$installed_sha" ]] \
    || die "installed authority bytes differ for $basename"
}

install_authority fixtures/native-shadow/registry-v1.json registry-v1.json
install_authority \
  native/containment/native-shadow-execution-policy-v1.json \
  execution-policy-v1.json
install_authority \
  native/containment/native-shadow-toolchain-identity-v1.json \
  toolchain-identity-v1.json

suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
suffix=${suffix//[^a-zA-Z0-9-]/-}
unit="boole-native-shadow-prelock-${suffix}"
log=$(mktemp "$temp_root/boole-native-shadow-prelock.XXXXXX")
test_name=startup::tests::real_linux_prelock_prerequisites_match_the_frozen_host_contract

set +e
sudo systemd-run --quiet --pipe --wait --collect --unit="$unit" \
  --property=Type=exec --property=Delegate=yes \
  --property=User=root --property=Group=root \
  --property='CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' \
  --property=AmbientCapabilities= --property=NoNewPrivileges=no \
  --property=PrivateMounts=yes \
  --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
  --property=WorkingDirectory=/ \
  "$launcher_path" "$test_name" --ignored --exact --nocapture \
  >"$log" 2>&1
status=$?
set -e
cat "$log"
[[ $status -eq 0 ]] || die "production pre-lock prerequisite composition failed"

for ((i = 0; i < 100; i++)); do
  state=$(sudo systemctl show "${unit}.service" --property=LoadState --value 2>/dev/null || :)
  [[ "$state" == not-found ]] && break
  sleep 0.05
done
[[ "$state" == not-found ]] \
  || die "transient unit ${unit}.service was not collected"

echo "native-shadow launcher pre-lock gate: PASS"
