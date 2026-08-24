#!/usr/bin/env bash
set -euo pipefail

die() {
  echo "native-shadow manager cgroup gate: $*" >&2
  exit 1
}

closed_local_replay_only=false
closed_local_replay_rootfs=''
closed_local_replay_manifest=''
if [[ $# -eq 3 && $1 == --closed-local-replay-rootfs ]]; then
  closed_local_replay_only=true
  closed_local_replay_rootfs=$(readlink -f -- "$2")
  closed_local_replay_manifest=$(readlink -f -- "$3")
  [[ -d "$closed_local_replay_rootfs" && ! -L "$2" ]] \
    || die "closed-local replay rootfs is not one exact nonsymlink directory"
  [[ -f "$closed_local_replay_manifest" && ! -L "$3" ]] \
    || die "closed-local replay manifest is not one exact nonsymlink file"
  for replay_path in "$closed_local_replay_rootfs" "$closed_local_replay_manifest"; do
    [[ "$replay_path" == /tmp/boole-native-shadow-rootfs-replay.*/* ]] \
      || die "closed-local replay input is outside the gate-owned scratch tree: $replay_path"
    [[ "$replay_path" != *:* && "$replay_path" != *$'\n'* ]] \
      || die "closed-local replay input cannot be represented safely in the systemd drop-in"
  done
elif [[ $# -ne 0 ]]; then
  die "usage: $0 [--closed-local-replay-rootfs ROOTFS CONTENT_MANIFEST]"
fi

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"
for command_name in awk cargo cp find findmnt getent grep install journalctl paste python3 readlink sed \
  sha256sum sort stat systemctl systemd-run systemd-tmpfiles tee timeout tr; do
  command -v "$command_name" >/dev/null || die "missing command: $command_name"
done
[[ $(cat /proc/1/comm) == systemd ]] || die "PID 1 is not systemd"
[[ $(stat -fc %T /sys/fs/cgroup) == cgroup2fs ]] || die "/sys/fs/cgroup is not cgroup2fs"

unit_name=boole-native-shadow-launcher.service
unit_path=/run/systemd/system/$unit_name
unit_dropin_directory="/run/systemd/system/${unit_name}.d"
unit_dropin_path="$unit_dropin_directory/10-manager-gate-authority.conf"
node_service_name=boole-native-shadow-replay-node.service
node_service_path=/run/systemd/system/$node_service_name
node_service_dropin_directory="/run/systemd/system/${node_service_name}.d"
node_service_dropin_path="$node_service_dropin_directory/10-manager-gate-authority.conf"
launcher_directory=/usr/libexec/boole
launcher_path=$launcher_directory/boole-native-shadow-launcher
node_qualification_path=$launcher_directory/boole-native-shadow-node-qualification
node_replay_client_path=$launcher_directory/boole-native-shadow-node-replay-client
node_replay_service_path=$launcher_directory/boole-native-shadow-replay-node
http_replay_gate_source=$(readlink -f scripts/native_shadow_http_replay_gate.py)
http_replay_gate_path=$launcher_directory/native-shadow-http-replay-gate.py
http_replay_grant_source=$(readlink -f native/containment/native-shadow-closed-local-replay-grant-v1.json)
http_replay_grant_path=$launcher_directory/native-shadow-http-replay-grant-v1.json
http_replay_fixture_source_directory=$(readlink -f fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history)
http_replay_fixture_directory=$launcher_directory/native-shadow-http-replay-fixtures
authority_stage=''
authority_share=''
authority_parent=''
authority_directory=''
checker_directory=''
fixture_directory=''
toolchain_parent=/opt/boole
toolchain_prefix=$toolchain_parent/native-checker-toolchain
toolchain_stage=''
opt_original_mode=''
runtime_parent=/run/boole
runtime_directory=$runtime_parent/native-shadow
socket_path="$runtime_directory/launcher.sock"
fixed_socket_wait_attempts=2400
mode_path=$runtime_directory/manager-cgroup-gate-mode
recovery_release_path=$runtime_directory/startup-recovery-release
service_root=/sys/fs/cgroup/system.slice/$unit_name
manager_root=$service_root/manager
node_service_root=/sys/fs/cgroup/system.slice/$node_service_name
temp_root=${RUNNER_TEMP:-/tmp}
build_json=$(mktemp "$temp_root/boole-native-shadow-manager-build.XXXXXX")
node_build_json=$(mktemp "$temp_root/boole-native-shadow-node-build.XXXXXX")
replay_client_build_json=$(mktemp "$temp_root/boole-native-shadow-replay-client-build.XXXXXX")
production_launcher_build_json=$(mktemp "$temp_root/boole-native-shadow-production-launcher-build.XXXXXX")
production_node_build_json=$(mktemp "$temp_root/boole-native-shadow-production-node-build.XXXXXX")
log=$(mktemp "$temp_root/boole-native-shadow-manager.XXXXXX")
node_log=$(mktemp "$temp_root/boole-native-shadow-node.XXXXXX")
replay_client_log=$(mktemp "$temp_root/boole-native-shadow-replay-client.XXXXXX")
dropin_source=$(mktemp "$temp_root/boole-native-shadow-manager-dropin.XXXXXX")
node_dropin_source=$(mktemp "$temp_root/boole-native-shadow-node-dropin.XXXXXX")
node_unit=''
node_state_root=/var/lib/boole
node_state_parent=$node_state_root/native-shadow
node_state_directory=$node_state_parent/node-state
node_journal_path=$node_state_directory/replay-v1.ndjson
work_path=/work
mutation_backup=''
mutation_target=''

launcher_directory_created=false
runtime_parent_created=false
runtime_directory_created=false
unit_installed=false
unit_dropin_directory_created=false
unit_dropin_installed=false
launcher_installed=false
node_qualification_installed=false
node_replay_client_installed=false
node_replay_service_installed=false
http_replay_gate_installed=false
http_replay_inputs_install_started=false
node_service_installed=false
node_service_dropin_directory_created=false
node_service_dropin_installed=false
node_state_root_created=false
node_state_parent_created=false
node_state_directory_created=false
toolchain_parent_created=false
toolchain_installed=false
opt_mode_changed=false
checker_installed=false
fixture_installed=false
work_created=false
declare -a installed_authorities=()

cleanup_gate() {
  set +e
  if [[ -n "$mutation_backup" && -n "$mutation_target" && -f "$mutation_backup" ]]; then
    sudo cp --preserve=all "$mutation_backup" "$mutation_target" >/dev/null 2>&1 || :
  fi
  [[ -z "$mutation_backup" ]] || sudo rm -f "$mutation_backup"
  if [[ "$node_service_installed" == true ]]; then
    sudo systemctl stop "$node_service_name" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "$node_service_name" >/dev/null 2>&1 || :
  fi
  if [[ -n "$node_unit" ]]; then
    sudo systemctl stop "${node_unit}.service" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "${node_unit}.service" >/dev/null 2>&1 || :
  fi
  if [[ "$unit_installed" == true ]]; then
    sudo systemctl stop "$unit_name" >/dev/null 2>&1 || :
    sudo systemctl reset-failed "$unit_name" >/dev/null 2>&1 || :
    [[ "$unit_dropin_installed" == true ]] && sudo rm -f "$unit_dropin_path"
    [[ "$unit_dropin_directory_created" == true ]] \
      && sudo rmdir "$unit_dropin_directory" >/dev/null 2>&1 || :
    sudo rm -f "$unit_path"
    sudo systemctl daemon-reload >/dev/null 2>&1 || :
  fi
  if [[ "$node_service_installed" == true ]]; then
    [[ "$node_service_dropin_installed" == true ]] && sudo rm -f "$node_service_dropin_path"
    [[ "$node_service_dropin_directory_created" == true ]] \
      && sudo rmdir "$node_service_dropin_directory" >/dev/null 2>&1 || :
    sudo rm -f "$node_service_path"
  fi
  if [[ "$unit_installed" == true || "$node_service_installed" == true ]]; then
    sudo systemctl daemon-reload >/dev/null 2>&1 || :
  fi
  [[ "$launcher_installed" == true ]] && sudo rm -f "$launcher_path"
  [[ "$node_qualification_installed" == true ]] && sudo rm -f "$node_qualification_path"
  [[ "$node_replay_client_installed" == true ]] && sudo rm -f "$node_replay_client_path"
  [[ "$node_replay_service_installed" == true ]] && sudo rm -f "$node_replay_service_path"
  [[ "$http_replay_gate_installed" == true ]] && sudo rm -f "$http_replay_gate_path"
  if [[ "$http_replay_inputs_install_started" == true ]]; then
    sudo rm -f "$http_replay_grant_path"
    sudo rm -f \
      "$http_replay_fixture_directory/replay-accepted.raw.txt" \
      "$http_replay_fixture_directory/replay-tampered.raw.txt" \
      "$http_replay_fixture_directory/replay-constant.raw.txt"
    sudo rmdir "$http_replay_fixture_directory" >/dev/null 2>&1 || :
  fi
  [[ "$toolchain_installed" == true ]] && sudo rm -rf "$toolchain_prefix"
  [[ "$toolchain_parent_created" == true ]] && sudo rmdir "$toolchain_parent" >/dev/null 2>&1 || :
  [[ "$opt_mode_changed" == true ]] && sudo chmod "$opt_original_mode" /opt
  if [[ "$checker_installed" == true ]]; then
    sudo rm -f \
      "$checker_directory/checker.py" \
      "$checker_directory/policy.json" \
      "$checker_directory/RELEASE-MANIFEST.json"
    sudo rmdir "$checker_directory" >/dev/null 2>&1 || :
    sudo rmdir "$(dirname "$checker_directory")" >/dev/null 2>&1 || :
  fi
  if [[ "$fixture_installed" == true ]]; then
    sudo rm -f "$fixture_directory/task.json" "$fixture_directory/anchor.rs"
    sudo rmdir "$fixture_directory" >/dev/null 2>&1 || :
    sudo rmdir "$(dirname "$fixture_directory")" >/dev/null 2>&1 || :
  fi
  local basename
  for basename in "${installed_authorities[@]}"; do
    sudo rm -f "$authority_directory/$basename"
  done
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$mode_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$recovery_release_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$socket_path"
  [[ "$runtime_directory_created" == true ]] && sudo rm -f "$runtime_directory/launcher.lock"
  [[ "$runtime_directory_created" == true ]] && sudo rmdir "$runtime_directory" >/dev/null 2>&1 || :
  [[ "$runtime_parent_created" == true ]] && sudo rmdir "$runtime_parent" >/dev/null 2>&1 || :
  [[ "$node_state_directory_created" == true ]] && sudo rm -f "$node_journal_path"
  [[ "$node_state_directory_created" == true ]] \
    && sudo rmdir "$node_state_directory" >/dev/null 2>&1 || :
  [[ "$node_state_parent_created" == true ]] \
    && sudo rmdir "$node_state_parent" >/dev/null 2>&1 || :
  [[ "$node_state_root_created" == true ]] \
    && sudo rmdir "$node_state_root" >/dev/null 2>&1 || :
  [[ "$work_created" == true ]] && sudo rmdir "$work_path" >/dev/null 2>&1 || :
  if [[ -n "$authority_stage" ]]; then
    sudo rmdir "$authority_directory" >/dev/null 2>&1 || :
    sudo rmdir "$authority_parent" >/dev/null 2>&1 || :
    sudo rmdir "$authority_share" >/dev/null 2>&1 || :
    sudo rmdir "$authority_stage" >/dev/null 2>&1 || :
  fi
  [[ "$launcher_directory_created" == true ]] && sudo rmdir "$launcher_directory" >/dev/null 2>&1 || :
  [[ -z "$toolchain_stage" ]] || rm -rf "$toolchain_stage"
  rm -f "$build_json" "$node_build_json" "$replay_client_build_json" \
    "$production_launcher_build_json" "$production_node_build_json" \
    "$log" "$node_log" "$replay_client_log" "$dropin_source" "$node_dropin_source"
}
trap cleanup_gate EXIT

for identity in boole-node boole-native-checker; do
  getent passwd "$identity" >/dev/null || die "missing service user: $identity"
  getent group "$identity" >/dev/null || die "missing service group: $identity"
done
[[ -f scripts/native_shadow_http_replay_gate.py \
  && ! -L scripts/native_shadow_http_replay_gate.py ]] \
  || die "HTTP replay gate source is not one exact nonsymlink file"
[[ -f native/containment/native-shadow-closed-local-replay-grant-v1.json \
  && ! -L native/containment/native-shadow-closed-local-replay-grant-v1.json ]] \
  || die "HTTP replay grant source is not one exact nonsymlink file"
[[ -d fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history \
  && ! -L fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history ]] \
  || die "HTTP replay fixture source is not one exact nonsymlink directory"
for http_replay_fixture_name in \
  replay-accepted.raw.txt replay-tampered.raw.txt replay-constant.raw.txt; do
  [[ -f "$http_replay_fixture_source_directory/$http_replay_fixture_name" \
    && ! -L "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/$http_replay_fixture_name" ]] \
    || die "HTTP replay fixture source is not one exact nonsymlink file: $http_replay_fixture_name"
done

load_state=$(systemctl show "$unit_name" --property=LoadState --value 2>/dev/null || :)
[[ "$load_state" == not-found ]] \
  || die "refusing to shadow pre-existing loaded unit: $unit_name ($load_state)"
node_load_state=$(systemctl show "$node_service_name" --property=LoadState --value 2>/dev/null || :)
[[ "$node_load_state" == not-found ]] \
  || die "refusing to shadow pre-existing loaded unit: $node_service_name ($node_load_state)"

for path in "$unit_path" "$unit_dropin_directory" "$launcher_path" \
  "$node_service_path" "$node_service_dropin_directory" \
  "$node_qualification_path" "$node_replay_client_path" "$node_replay_service_path" \
  "$http_replay_gate_path" "$http_replay_grant_path" "$http_replay_fixture_directory" \
  "$runtime_directory" "$node_state_directory" "$node_journal_path" \
  "$service_root" "$toolchain_prefix"; do
  [[ ! -e "$path" && ! -L "$path" ]] || die "refusing to replace pre-existing path: $path"
done

BOOLE_NATIVE_SHADOW_SECCOMP_SPAWN_PROBE=1 \
  cargo test --locked -p boole-native-shadow-launcher \
    --features manager-cgroup-linux-gate --lib \
    seccomp_preserves_the_rust_process_spawn_control_channel -- \
    --nocapture --test-threads=1

cargo test --locked -p boole-native-shadow-launcher \
  --features manager-cgroup-linux-gate \
  --test boole-native-shadow-manager-cgroup-linux \
  --no-run --message-format=json >"$build_json"

mapfile -t executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-manager-cgroup-linux"
        and item.get("executable")
    ):
        print(item["executable"])
' "$build_json"
)
[[ ${#executables[@]} -eq 1 ]] || die "expected one manager harness, got ${#executables[@]}"
harness=${executables[0]}
[[ -x "$harness" ]] || die "manager harness is not executable"

cargo test --locked -p boole-node --lib \
  --no-run --message-format=json >"$node_build_json"

mapfile -t node_executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    target = item.get("target", {})
    if (
        item.get("reason") == "compiler-artifact"
        and target.get("name") == "boole_node"
        and "lib" in target.get("kind", [])
        and item.get("profile", {}).get("test") is True
        and item.get("executable")
    ):
        print(item["executable"])
' "$node_build_json"
)
[[ ${#node_executables[@]} -eq 1 ]] \
  || die "expected one boole-node lib test executable, got ${#node_executables[@]}"
node_qualification_source=${node_executables[0]}
[[ -x "$node_qualification_source" ]] || die "boole-node qualification test is not executable"

cargo test --locked -p boole-native-shadow-launcher \
  --features manager-cgroup-linux-gate \
  --test boole-native-shadow-closed-local-replay-client-linux \
  --no-run --message-format=json >"$replay_client_build_json"

mapfile -t replay_client_executables < <(
  python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-closed-local-replay-client-linux"
        and item.get("executable")
    ):
        print(item["executable"])
' "$replay_client_build_json"
)
[[ ${#replay_client_executables[@]} -eq 1 ]] \
  || die "expected one closed-local replay client, got ${#replay_client_executables[@]}"
node_replay_client_source=${replay_client_executables[0]}
[[ -x "$node_replay_client_source" ]] || die "closed-local replay client is not executable"

production_launcher_source=''
production_node_source=''
if [[ "$closed_local_replay_only" == true ]]; then
  cargo build --locked -p boole-native-shadow-launcher --bin boole-native-shadow-launcher \
    --message-format=json >"$production_launcher_build_json"
  mapfile -t production_launcher_executables < <(
    python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-launcher"
        and item.get("executable")
    ):
        print(item["executable"])
' "$production_launcher_build_json"
  )
  [[ ${#production_launcher_executables[@]} -eq 1 ]] \
    || die "expected one production launcher executable, got ${#production_launcher_executables[@]}"
  production_launcher_source=${production_launcher_executables[0]}
  [[ -x "$production_launcher_source" ]] || die "production launcher is not executable"

  cargo build --locked -p boole-node --features native-shadow-closed-local-replay --bin boole-native-shadow-replay-node \
    --message-format=json >"$production_node_build_json"
  mapfile -t production_node_executables < <(
    python3 -c '
import json
import sys

for line in open(sys.argv[1], encoding="utf-8"):
    item = json.loads(line)
    if (
        item.get("reason") == "compiler-artifact"
        and item.get("target", {}).get("name") == "boole-native-shadow-replay-node"
        and item.get("executable")
    ):
        print(item["executable"])
' "$production_node_build_json"
  )
  [[ ${#production_node_executables[@]} -eq 1 ]] \
    || die "expected one production replay-node executable, got ${#production_node_executables[@]}"
  production_node_source=${production_node_executables[0]}
  [[ -x "$production_node_source" ]] || die "production replay node is not executable"
fi

toolchain_stage=$(mktemp -d "$temp_root/boole-native-shadow-toolchain.XXXXXX")
./scripts/install-native-checker-toolchain.sh "$toolchain_stage"
[[ $(stat -c %U:%G /opt) == root:root ]] \
  || die "fixed /opt ancestor is not root-owned"
opt_original_mode=$(stat -c %a /opt)
if (( (8#$opt_original_mode & 8#022) != 0 )); then
  sudo chmod go-w /opt
  opt_mode_changed=true
fi
[[ $((8#$(stat -c %a /opt) & 8#022)) -eq 0 ]] \
  || die "fixed /opt ancestor remains group/other writable"
if [[ ! -d "$toolchain_parent" ]]; then
  sudo install -d -o root -g root -m 0755 "$toolchain_parent"
  toolchain_parent_created=true
fi
sudo install -d -o root -g root -m 0555 "$toolchain_prefix"
toolchain_installed=true
sudo cp -a "$toolchain_stage/." "$toolchain_prefix/"
sudo chown -R root:root "$toolchain_prefix"
sudo chmod 0555 "$toolchain_prefix" "$toolchain_prefix/bin"
[[ $(sudo stat -c %U:%G:%a "$toolchain_prefix") == root:root:555 ]] \
  || die "installed toolchain root metadata differs"
[[ $(sudo stat -c %U:%G:%a "$toolchain_prefix/bin") == root:root:555 ]] \
  || die "installed toolchain bin metadata differs"
for executable in rustc cargo; do
  [[ $(sudo stat -c %U:%G "$toolchain_prefix/bin/$executable") == root:root ]] \
    || die "installed $executable owner/group differs"
  [[ -x "$toolchain_prefix/bin/$executable" ]] \
    || die "installed $executable is not executable"
done

if [[ ! -d "$launcher_directory" ]]; then
  sudo install -d -o root -g root -m 0755 "$launcher_directory"
  launcher_directory_created=true
fi
[[ $(sudo stat -c %U:%G:%a "$launcher_directory") == root:root:755 ]] \
  || die "launcher staging directory does not match root:root:755"
sudo install -o root -g root -m 0755 "$harness" "$launcher_path"
launcher_installed=true
sudo install -o root -g root -m 0755 \
  "$node_qualification_source" "$node_qualification_path"
node_qualification_installed=true
sudo install -o root -g root -m 0755 \
  "$node_replay_client_source" "$node_replay_client_path"
node_replay_client_installed=true
if [[ "$closed_local_replay_only" == true ]]; then
  sudo install -o root -g root -m 0755 \
    "$production_node_source" "$node_replay_service_path"
  node_replay_service_installed=true
  sudo install -o root -g root -m 0555 "$http_replay_gate_source" "$http_replay_gate_path"
  http_replay_gate_installed=true
  http_replay_inputs_install_started=true
  sudo install -o root -g root -m 0444 "$http_replay_grant_source" "$http_replay_grant_path"
  sudo install -d -o root -g root -m 0755 "$http_replay_fixture_directory"
  for http_replay_fixture_name in \
    replay-accepted.raw.txt replay-tampered.raw.txt replay-constant.raw.txt; do
    sudo install -o root -g root -m 0444 \
      "$http_replay_fixture_source_directory/$http_replay_fixture_name" \
      "$http_replay_fixture_directory/$http_replay_fixture_name"
  done
  sudo chmod 0555 "$http_replay_fixture_directory"
fi
[[ $(sha256sum "$harness" | awk '{ print $1 }') == $(sudo sha256sum "$launcher_path" | awk '{ print $1 }') ]] \
  || die "installed launcher bytes differ from reviewed harness"
[[ $(sudo stat -c %U:%G:%a "$launcher_path") == root:root:755 ]] \
  || die "installed launcher metadata does not match root:root:755"
[[ $(sha256sum "$node_qualification_source" | awk '{ print $1 }') == $(sudo sha256sum "$node_qualification_path" | awk '{ print $1 }') ]] \
  || die "installed boole-node qualification bytes differ from the reviewed test binary"
[[ $(sudo stat -c %U:%G:%a "$node_qualification_path") == root:root:755 ]] \
  || die "installed boole-node qualification metadata does not match root:root:755"
[[ $(sha256sum "$node_replay_client_source" | awk '{ print $1 }') == $(sudo sha256sum "$node_replay_client_path" | awk '{ print $1 }') ]] \
  || die "installed closed-local replay client bytes differ from the reviewed binary"
[[ $(sudo stat -c %U:%G:%a "$node_replay_client_path") == root:root:755 ]] \
  || die "installed closed-local replay client metadata does not match root:root:755"
if [[ "$closed_local_replay_only" == true ]]; then
  [[ $(sha256sum "$production_node_source" | awk '{ print $1 }') == $(sudo sha256sum "$node_replay_service_path" | awk '{ print $1 }') ]] \
    || die "installed production replay-node bytes differ from the reviewed binary"
  [[ $(sudo stat -c %U:%G:%a "$node_replay_service_path") == root:root:755 ]] \
    || die "installed production replay-node metadata does not match root:root:755"
  [[ $(sha256sum "$http_replay_gate_source" | awk '{ print $1 }') == $(sudo sha256sum "$http_replay_gate_path" | awk '{ print $1 }') ]] \
    || die "installed HTTP replay gate bytes differ from the reviewed script"
  [[ $(sudo stat -c %U:%G:%a "$http_replay_gate_path") == root:root:555 ]] \
    || die "installed HTTP replay gate metadata does not match root:root:555"
  [[ $(sha256sum "$http_replay_grant_source" | awk '{ print $1 }') == $(sudo sha256sum "$http_replay_grant_path" | awk '{ print $1 }') ]] \
    || die "installed HTTP replay grant bytes differ from the reviewed authority"
  [[ $(sudo stat -c %U:%G:%a "$http_replay_grant_path") == root:root:444 ]] \
    || die "installed HTTP replay grant metadata does not match root:root:444"
  [[ $(sudo stat -c %U:%G:%a "$http_replay_fixture_directory") == root:root:555 ]] \
    || die "installed HTTP replay fixture directory does not match root:root:555"
  for http_replay_fixture_name in \
    replay-accepted.raw.txt replay-tampered.raw.txt replay-constant.raw.txt; do
    [[ $(sha256sum "$http_replay_fixture_source_directory/$http_replay_fixture_name" | awk '{ print $1 }') == $(sudo sha256sum "$http_replay_fixture_directory/$http_replay_fixture_name" | awk '{ print $1 }') ]] \
      || die "installed HTTP replay fixture bytes differ: $http_replay_fixture_name"
    [[ $(sudo stat -c %U:%G:%a "$http_replay_fixture_directory/$http_replay_fixture_name") == root:root:444 ]] \
      || die "installed HTTP replay fixture metadata does not match root:root:444: $http_replay_fixture_name"
  done
fi

authority_stage=$(sudo mktemp -d /run/boole-native-shadow-manager-authority.XXXXXX)
authority_share="$authority_stage/share"
authority_parent="$authority_share/boole"
authority_directory="$authority_parent/native-shadow"
checker_directory="$authority_directory/checkers/rust-tuple-struct-project-v1"
fixture_directory="$authority_directory/fixtures/a-rooted-native-mining-e2e-v1-real-history"
sudo chmod 0700 "$authority_stage"
sudo install -d -o root -g root -m 0755 "$authority_share"
sudo install -d -o root -g root -m 0755 "$authority_parent"
sudo install -d -o root -g root -m 0555 "$authority_directory"

install_authority() {
  local source=$1
  local basename=$2
  sudo install -o root -g root -m 0444 "$source" "$authority_directory/$basename"
  installed_authorities+=("$basename")
  [[ $(sha256sum "$source" | awk '{ print $1 }') == $(sudo sha256sum "$authority_directory/$basename" | awk '{ print $1 }') ]] \
    || die "installed authority differs: $basename"
}
install_authority fixtures/native-shadow/registry-v1.json registry-v1.json
install_authority native/containment/native-shadow-execution-policy-v1.json execution-policy-v1.json
install_authority native/containment/native-shadow-toolchain-identity-v1.json toolchain-identity-v1.json
install_authority native/containment/native-shadow-local-execution-authority-v1.json local-execution-authority-v1.json
install_authority native/containment/native-shadow-closed-local-replay-grant-v1.json closed-local-replay-grant-v1.json
install_authority native/containment/native-shadow-closed-local-replay-registry-overlay-v1.json closed-local-replay-registry-overlay-v1.json
install_authority native/containment/native-shadow-closed-local-replay-execution-authority-v1.json closed-local-replay-execution-authority-v1.json
sudo install -d -o root -g root -m 0555 "$(dirname "$checker_directory")"
sudo install -d -o root -g root -m 0555 "$checker_directory"
checker_installed=true
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/checker.py "$checker_directory/checker.py"
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/policy.json "$checker_directory/policy.json"
sudo install -o root -g root -m 0444 \
  native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json \
  "$checker_directory/RELEASE-MANIFEST.json"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/checker.py | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/checker.py" | awk '{ print $1 }') ]] \
  || die "installed checker bytes differ"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/policy.json | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/policy.json" | awk '{ print $1 }') ]] \
  || die "installed checker policy bytes differ"
[[ $(sha256sum native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json | awk '{ print $1 }') == $(sudo sha256sum "$checker_directory/RELEASE-MANIFEST.json" | awk '{ print $1 }') ]] \
  || die "installed checker release manifest bytes differ"
sudo install -d -o root -g root -m 0555 "$(dirname "$fixture_directory")"
sudo install -d -o root -g root -m 0555 "$fixture_directory"
fixture_installed=true
sudo install -o root -g root -m 0444 \
  fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/task.json \
  "$fixture_directory/task.json"
sudo install -o root -g root -m 0444 \
  fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/anchor.rs \
  "$fixture_directory/anchor.rs"
for fixture_name in task.json anchor.rs; do
  [[ $(sha256sum "fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/$fixture_name" | awk '{ print $1 }') == $(sudo sha256sum "$fixture_directory/$fixture_name" | awk '{ print $1 }') ]] \
    || die "installed replay fixture differs: $fixture_name"
done

if [[ ! -e "$work_path" && ! -L "$work_path" ]]; then
  sudo install -d -o root -g root -m 0555 "$work_path"
  work_created=true
fi
[[ -d "$work_path" && ! -L "$work_path" ]] \
  || die "fixed workspace mountpoint is not a nonsymlink directory"
[[ $(sudo stat -c %U:%G:%a "$work_path") == root:root:555 ]] \
  || die "fixed workspace mountpoint does not match root:root:0555"

if [[ ! -d "$runtime_parent" ]]; then
  runtime_parent_created=true
fi
if [[ ! -d "$runtime_directory" ]]; then
  runtime_directory_created=true
fi
if [[ ! -d "$node_state_root" ]]; then
  node_state_root_created=true
fi
if [[ ! -d "$node_state_parent" ]]; then
  node_state_parent_created=true
fi
if [[ ! -d "$node_state_directory" ]]; then
  node_state_directory_created=true
fi
tmpfiles_path=$(readlink -f native/tmpfiles.d/boole-native-shadow.conf)
[[ -f "$tmpfiles_path" ]] || die "tracked tmpfiles input is unavailable"
sudo systemd-tmpfiles --create "$tmpfiles_path"
[[ $(stat -c %U:%G:%a "$runtime_directory") == root:boole-node:2750 ]] \
  || die "runtime directory does not match root:boole-node mode 2750"
[[ $(stat -c %U:%G:%a "$node_state_directory") == boole-node:boole-node:700 ]] \
  || die "node-state directory does not match boole-node:boole-node mode 0700"

sudo install -o root -g root -m 0644 native/systemd/boole-native-shadow-launcher.service "$unit_path"
unit_installed=true
[[ $(sha256sum native/systemd/boole-native-shadow-launcher.service | awk '{ print $1 }') == $(sudo sha256sum "$unit_path" | awk '{ print $1 }') ]] \
  || die "installed systemd unit differs from tracked bytes"
sudo install -d -o root -g root -m 0755 "$unit_dropin_directory"
unit_dropin_directory_created=true
expected_dropin=$'[Service]\n'"BindReadOnlyPaths=${authority_share}:/usr/share"
if [[ "$closed_local_replay_only" == true ]]; then
  expected_dropin+=$'\n'"BindReadOnlyPaths=${closed_local_replay_rootfs}:/var/lib/boole/native-shadow/runtime-rootfs"
  expected_dropin+=$'\n'"BindReadOnlyPaths=${closed_local_replay_manifest}:/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json"
fi
printf '%s\n' "$expected_dropin" >"$dropin_source"
unit_dropin_installed=true
sudo install -o root -g root -m 0644 "$dropin_source" "$unit_dropin_path"
[[ $(sudo stat -c %U:%G:%a "$unit_dropin_path") == root:root:644 ]] \
  || die "authority bind drop-in metadata does not match root:root:644"
[[ $(sudo cat "$unit_dropin_path") == "$expected_dropin" ]] \
  || die "authority bind drop-in differs from the exact two-line contract"
sudo systemctl daemon-reload
[[ $(sudo systemctl show "$unit_name" --property=FragmentPath --value) == "$unit_path" ]] \
  || die "systemd did not load the exact tracked unit fragment"
[[ $(sudo systemctl show "$unit_name" --property=DropInPaths --value) == "$unit_dropin_path" ]] \
  || die "systemd did not load exactly the gate-owned authority drop-in"

if [[ "$closed_local_replay_only" == true ]]; then
  sudo install -o root -g root -m 0644 \
    native/systemd/boole-native-shadow-replay-node.service "$node_service_path"
  node_service_installed=true
  [[ $(sha256sum native/systemd/boole-native-shadow-replay-node.service | awk '{ print $1 }') == $(sudo sha256sum "$node_service_path" | awk '{ print $1 }') ]] \
    || die "installed production replay-node unit differs from tracked bytes"
  sudo install -d -o root -g root -m 0755 "$node_service_dropin_directory"
  node_service_dropin_directory_created=true
  printf '[Service]\nBindReadOnlyPaths=%s:/usr/share\n' "$authority_share" \
    >"$node_dropin_source"
  sudo install -o root -g root -m 0644 "$node_dropin_source" "$node_service_dropin_path"
  node_service_dropin_installed=true
  [[ $(sudo stat -c %U:%G:%a "$node_service_dropin_path") == root:root:644 ]] \
    || die "replay-node authority bind drop-in metadata does not match root:root:644"
  sudo systemctl daemon-reload
  [[ $(sudo systemctl show "$node_service_name" --property=FragmentPath --value) == "$node_service_path" ]] \
    || die "systemd did not load the exact tracked replay-node unit fragment"
  [[ $(sudo systemctl show "$node_service_name" --property=DropInPaths --value) == "$node_service_dropin_path" ]] \
    || die "systemd did not load exactly the gate-owned replay-node authority drop-in"
fi

set_mode() {
  local mode=$1
  if [[ "$mode" == normal ]]; then
    sudo rm -f "$mode_path"
  else
    printf '%s\n' "$mode" | sudo tee "$mode_path" >/dev/null
    sudo chown root:root "$mode_path"
    sudo chmod 0600 "$mode_path"
  fi
}

wait_for_state() {
  local expected=$1
  local state=''
  local i
  for ((i = 0; i < 200; i++)); do
    state=$(sudo systemctl show "$unit_name" --property=ActiveState --value 2>/dev/null || :)
    [[ "$state" == "$expected" ]] && return 0
    if [[ "$state" == failed ]]; then
      sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
      die "unit entered failed state while waiting for $expected"
    fi
    sleep 0.05
  done
  die "unit did not reach $expected (last state: $state)"
}

wait_for_cgroup_removal() {
  local i
  for ((i = 0; i < 200; i++)); do
    sudo test ! -e "$service_root" && return 0
    sleep 0.05
  done
  die "service cgroup remained after stop"
}

single_numeric_id() {
  local path=$1
  local -a values=()
  mapfile -t values < <(sudo awk 'NF == 1 { print $1; next } NF > 1 { print "__malformed__" }' "$path")
  [[ ${#values[@]} -eq 1 && ${values[0]} =~ ^[1-9][0-9]*$ ]] || return 1
  printf '%s\n' "${values[0]}"
}

assert_manager_invariants() {
  wait_for_state active
  local pid
  pid=$(sudo systemctl show "$unit_name" --property=MainPID --value)
  [[ "$pid" =~ ^[1-9][0-9]*$ ]] || die "unit has invalid MainPID: $pid"

  local i
  local manager_pid
  for ((i = 0; i < 200; i++)); do
    sudo test -d "$manager_root" || { sleep 0.05; continue; }
    [[ -z $(sudo cat "$service_root/cgroup.procs" | tr -d '[:space:]') ]] \
      || { sleep 0.05; continue; }
    manager_pid=$(single_numeric_id "$manager_root/cgroup.procs" || :)
    [[ "$manager_pid" == "$pid" ]] || { sleep 0.05; continue; }
    break
  done
  [[ $i -lt 200 ]] || die "manager cgroup did not reach the post-move state"

  local manager_tid
  manager_pid=$(single_numeric_id "$manager_root/cgroup.procs") \
    || die "manager cgroup.procs does not contain exactly one numeric ID"
  manager_tid=$(single_numeric_id "$manager_root/cgroup.threads") \
    || die "manager cgroup.threads does not contain exactly one numeric ID"
  [[ "$manager_pid" == "$pid" ]] \
    || die "manager cgroup does not contain exactly the MainPID process"
  [[ "$manager_tid" == "$pid" ]] \
    || die "manager cgroup does not contain exactly the MainPID thread"
  local manager_metadata
  manager_metadata=$(sudo stat -c %U:%G:%a "$manager_root" 2>/dev/null || :)
  if [[ "$manager_metadata" != root:root:700 ]]; then
    sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
    die "manager cgroup metadata does not match root:root:700: $manager_metadata"
  fi
  [[ -z $(sudo cat "$manager_root/cgroup.subtree_control" | tr -d '[:space:]') ]] \
    || die "manager cgroup has residual subtree controllers"
  [[ $(sudo cat "$manager_root/cgroup.type" | tr -d '[:space:]') == domain ]] \
    || die "manager cgroup type is not exact domain after move"
  [[ -z $(sudo find "$manager_root" -mindepth 1 -maxdepth 1 -type d -print -quit) ]] \
    || die "manager cgroup contains a nested child"
  local controllers
  controllers=$(sudo cat "$service_root/cgroup.subtree_control" | tr ' ' '\n' | sed '/^$/d' | sort | paste -sd' ' -)
  [[ "$controllers" == "cpu memory pids" ]] \
    || die "service subtree controllers differ: $controllers"
  [[ $(sudo stat -fc %T "$service_root") == cgroup2fs ]] || die "service root is not cgroup2fs"
  [[ $(sudo readlink -f "/proc/$pid/exe") == "$launcher_path" ]] \
    || die "MainPID does not execute the staged reviewed harness"
  local relative
  relative=$(sudo awk -F: '$1 == "0" { print $3 }' "/proc/$pid/cgroup")
  [[ "$relative" == "/system.slice/$unit_name/manager" ]] \
    || die "MainPID cgroup path differs: $relative"
  printf '%s\n' "$pid"
}

unit_invocation_id() {
  local invocation_id
  invocation_id=$(sudo systemctl show "$unit_name" --property=InvocationID --value)
  [[ "$invocation_id" =~ ^[0-9a-f]{32}$ ]] \
    || die "unit has invalid InvocationID: $invocation_id"
  printf '%s\n' "$invocation_id"
}

wait_for_marker() {
  local marker=$1
  local invocation_id=$2
  local i
  for ((i = 0; i < 200; i++)); do
    if sudo journalctl --no-pager -o cat -u "$unit_name" \
      "_SYSTEMD_INVOCATION_ID=$invocation_id" | grep -Fqx "$marker"; then
      return 0
    fi
    sleep 0.05
  done
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "unit did not emit marker: $marker"
}

journal_marker_count() {
  local marker=$1
  sudo journalctl --sync
  sudo journalctl --no-pager -o cat -u "$unit_name" \
    | grep -Fxc "$marker" || :
}

wait_for_marker_increment() {
  local marker=$1
  local before=$2
  local expected=$((before + 1))
  local observed=0
  local i
  for ((i = 0; i < 200; i++)); do
    observed=$(journal_marker_count "$marker")
    [[ "$observed" -eq "$expected" ]] && return 0
    if (( observed > expected )); then
      die "unit emitted marker more than once: $marker"
    fi
    sleep 0.05
  done
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "unit marker count did not advance exactly once: $marker (before=$before last=$observed)"
}

wait_for_fixed_socket() {
  local metadata=''
  local i
  for ((i = 0; i < fixed_socket_wait_attempts; i++)); do
    if sudo test -S "$socket_path"; then
      metadata=$(sudo stat -c %U:%G:%a "$socket_path")
      [[ "$metadata" == root:boole-node:660 ]] \
        || die "fixed qualification socket metadata differs: $metadata"
      return 0
    fi
    if [[ $(sudo systemctl show "$unit_name" --property=ActiveState --value 2>/dev/null || :) == failed ]]; then
      sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
      die "unit failed before the fixed qualification socket appeared"
    fi
    sleep 0.05
  done
  sudo systemctl show "$unit_name" --property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :
  sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :
  die "fixed qualification socket did not appear"
}

wait_for_leaf_event() {
  local leaf=$1
  local key=$2
  local expected=$3
  local i
  local observed=''
  for ((i = 0; i < 200; i++)); do
    observed=$(sudo awk -v key="$key" '$1 == key && NF == 2 { print $2 }' "$leaf/cgroup.events")
    [[ "$observed" == "$expected" ]] && return 0
    sleep 0.05
  done
  die "leaf event $key did not reach $expected (last: $observed)"
}

create_run_leaf() {
  local leaf=$1
  sudo mkdir "$leaf"
  sudo chmod 0700 "$leaf"
  [[ $(sudo stat -c %U:%G:%a "$leaf") == root:root:700 ]] \
    || die "run leaf metadata does not match root:root:700"
}

start_process_tree() {
  local leaf=$1
  sudo python3 -c '
import os
import signal
import sys

leaf = sys.argv[1]
with open(os.path.join(leaf, "cgroup.procs"), "w", encoding="ascii") as stream:
    stream.write(f"{os.getpid()}\n")
child = os.fork()
signal.pause()
' "$leaf" &
  tree_supervisor_pid=$!
}

wait_for_leaf_process_count() {
  local leaf=$1
  local expected=$2
  local i
  local observed=0
  for ((i = 0; i < 200; i++)); do
    observed=$(sudo awk 'NF == 1 { count += 1 } END { print count + 0 }' "$leaf/cgroup.procs")
    [[ "$observed" == "$expected" ]] && return 0
    sleep 0.05
  done
  die "leaf process count did not reach $expected (last: $observed)"
}

pid_start_time() {
  local pid=$1
  python3 -c '
import pathlib
import sys

try:
    text = pathlib.Path(f"/proc/{sys.argv[1]}/stat").read_text(encoding="ascii")
except FileNotFoundError:
    print("missing")
    raise SystemExit(0)
closing = text.rfind(")")
fields = text[closing + 2:].split()
if closing < 0 or len(fields) <= 19:
    raise SystemExit("malformed proc stat")
print(fields[19])
' "$pid"
}

wait_for_original_process_exit() {
  local pid=$1
  local original_start_time=$2
  local current_start_time=''
  local i
  for ((i = 0; i < 200; i++)); do
    current_start_time=$(pid_start_time "$pid") \
      || die "could not identify recovered process: $pid"
    if [[ "$current_start_time" == missing || "$current_start_time" != "$original_start_time" ]]; then
      return 0
    fi
    sleep 0.05
  done
  die "original recovered process remains visible: $pid"
}

wait_for_background_job() {
  local pid=$1
  local i
  for ((i = 0; i < 200; i++)); do
    if ! jobs -pr | grep -Fqx "$pid"; then
      wait "$pid" 2>/dev/null || :
      return 0
    fi
    sleep 0.05
  done
  die "background fixture job did not exit: $pid"
}

run_expected_rejection() {
  local mode=$1
  local marker=$2
  local marker_count_before
  set_mode "$mode"
  marker_count_before=$(journal_marker_count "$marker")
  sudo systemctl start boole-native-shadow-launcher.service
  wait_for_state inactive
  [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
    || die "$mode rejection harness did not exit successfully"
  wait_for_marker_increment "$marker" "$marker_count_before"
  wait_for_cgroup_removal
}

run_containment_layer_diagnostics() {
  local suffix
  suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
  suffix=${suffix//[^a-zA-Z0-9-]/-}
  local -a diagnostic_modes=(
    closed-local-replay-diagnostic-full
    closed-local-replay-diagnostic-without-landlock
    closed-local-replay-diagnostic-without-seccomp
  )
  local diagnostic_mode
  for diagnostic_mode in "${diagnostic_modes[@]}"; do
    : >"$replay_client_log"
    set_mode "$diagnostic_mode"
    sudo systemctl start "$unit_name"
    local diagnostic_invocation
    diagnostic_invocation=$(unit_invocation_id)
    assert_manager_invariants >/dev/null
    wait_for_fixed_socket

    local diagnostic_label=${diagnostic_mode#closed-local-replay-diagnostic-}
    diagnostic_label=${diagnostic_label//[^a-zA-Z0-9-]/-}
    node_unit="boole-native-shadow-containment-${diagnostic_label}-${suffix}"
    set +e
    timeout --foreground --signal=TERM --kill-after=10s 180s \
      sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
        --property=Type=exec --property=User=boole-node --property=Group=boole-node \
        --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
        --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
        --property=PrivateNetwork=yes --property=RestrictAddressFamilies=AF_UNIX \
        --property=ProtectSystem=strict \
        --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
        --property=WorkingDirectory=/ \
        "$node_replay_client_path" --diagnostic-accepted >"$replay_client_log" 2>&1
    local diagnostic_client_status=$?
    set -e
    cat "$replay_client_log"
    [[ $diagnostic_client_status -eq 0 ]] \
      || die "$diagnostic_mode client failed before one validated Report"
    [[ $(grep -Fc 'native-shadow-containment-layer-diagnostic-report:' "$replay_client_log" || :) -eq 1 ]] \
      || die "$diagnostic_mode did not emit one safe Report diagnostic"
    [[ $(grep -Fxc 'native-shadow-containment-layer-diagnostic-client-complete:launcher_connections=1' "$replay_client_log" || :) -eq 1 ]] \
      || die "$diagnostic_mode client did not complete one connection"

    wait_for_state inactive
    [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
      || die "$diagnostic_mode launcher did not exit successfully"
    [[ $(sudo systemctl show "$unit_name" --property=NRestarts --value) == 0 ]] \
      || die "$diagnostic_mode launcher restarted unexpectedly"
    wait_for_marker native-shadow-containment-layer-diagnostic-complete "$diagnostic_invocation"
    mapfile -t cargo_diagnostics < <(
      sudo journalctl --no-pager -o cat -u "$unit_name" \
        "_SYSTEMD_INVOCATION_ID=$diagnostic_invocation" \
        | grep -E '^boole-native-shadow-checker-cargo-diagnostic:v1;category=(success|wall_limit|output_limit|authority_unavailable|rustc_version_permission_denied|rustc_version_failed|rustc_metadata_permission_denied|rustc_metadata_failed|rustc_link_permission_denied|rustc_linker_failed|rustc_link_failed|cc_alias_permission_denied|cc_alias_failed|gcc_link_permission_denied|gcc_link_failed|gcc_frontend_permission_denied|gcc_frontend_failed|gcc_assembler_permission_denied|gcc_assembler_failed|gcc_final_link_permission_denied|gcc_final_link_failed|rustc_default_linker_permission_denied|rustc_explicit_gcc_permission_denied|rustc_explicit_gcc_failed|rustc_probe_permission_denied|rustc_probe_linker_failed|rustc_probe_failed|workspace_execute_denied|workspace_execute_failed|cargo_test_execute_denied|cargo_rustc_execute_denied|cargo_linker_permission_denied|cargo_temp_permission_denied|cargo_directory_permission_denied|permission_denied|read_only_filesystem|missing_file|cargo_lock_wait|process_spawn_failed|linker_failed|temporary_directory_failed|hidden_test_failed|compiler_error|unknown_nonzero)$' \
        || :
    )
    [[ ${#cargo_diagnostics[@]} -eq 1 ]] \
      || die "$diagnostic_mode did not emit exactly one categorical Cargo diagnostic"
    printf 'native-shadow categorical Cargo diagnostic:%s:%s\n' \
      "$diagnostic_label" "${cargo_diagnostics[0]}"
    sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
      || die "$diagnostic_mode left the fixed socket behind"
    wait_for_cgroup_removal

    local diagnostic_load_state=''
    local diagnostic_wait
    for ((diagnostic_wait = 0; diagnostic_wait < 200; diagnostic_wait++)); do
      diagnostic_load_state=$(sudo systemctl show "${node_unit}.service" \
        --property=LoadState --value 2>/dev/null || :)
      [[ "$diagnostic_load_state" == not-found ]] && break
      sleep 0.05
    done
    [[ "$diagnostic_load_state" == not-found ]] \
      || die "$diagnostic_mode transient client unit was not collected"
    node_unit=''
  done
  echo "native-shadow categorical Cargo diagnostic: COMPLETE"
}

run_closed_local_replay_gate() {
  run_containment_layer_diagnostics
  local mutation_relative
  local mutation_expected_sha
  mutation_relative=$(jq -er \
    '[.entries[] | select(.kind == "file" and (.sizeBytes // 0) > 0 and (.sizeBytes // 0) <= 4096)] | sort_by(.logicalPath) | .[0].logicalPath | ltrimstr("/")' \
    "$closed_local_replay_manifest")
  mutation_expected_sha=$(jq -er --arg path "/$mutation_relative" \
    '.entries[] | select(.logicalPath == $path) | .sha256' \
    "$closed_local_replay_manifest")
  [[ "$mutation_relative" != /* && "$mutation_relative" != *..* && "$mutation_relative" != *$'\n'* ]] \
    || die "frozen mutation target is not one canonical relative path"
  [[ "$mutation_expected_sha" =~ ^[0-9a-f]{64}$ ]] \
    || die "frozen mutation target lacks one exact SHA-256"
  mutation_target="$closed_local_replay_rootfs/$mutation_relative"
  [[ -f "$mutation_target" && ! -L "$mutation_target" ]] \
    || die "frozen mutation target is not one regular file"
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') == "$mutation_expected_sha" ]] \
    || die "frozen mutation target differs before the rejection gate"
  mutation_backup=$(mktemp "$temp_root/boole-native-shadow-rootfs-mutation-backup.XXXXXX")
  sudo cp --preserve=all "$mutation_target" "$mutation_backup"

  # First prove that startup verification is not stale authority.  The
  # listener opens over the exact tree, then one host-side byte changes before
  # Execute. The request-time full-tree rehash must close the connection before
  # a cgroup leaf or checker report can exist.
  set_mode closed-local-replay-three
  sudo systemctl start "$unit_name"
  local drift_invocation
  drift_invocation=$(unit_invocation_id)
  assert_manager_invariants >/dev/null
  wait_for_fixed_socket
  sudo python3 - "$mutation_target" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = bytearray(path.read_bytes())
if not data:
    raise SystemExit("mutation target unexpectedly empty")
data[0] ^= 1
path.write_bytes(data)
PY
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') != "$mutation_expected_sha" ]] \
    || die "rootfs mutation did not change the selected file"

  local suffix
  suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
  suffix=${suffix//[^a-zA-Z0-9-]/-}
  node_unit="boole-native-shadow-node-replay-drift-${suffix}"
  set +e
  timeout --foreground --signal=TERM --kill-after=10s 120s \
    sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
      --property=Type=exec --property=User=boole-node --property=Group=boole-node \
      --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
      --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
      --property=PrivateNetwork=yes --property=RestrictAddressFamilies=AF_UNIX \
      --property=ProtectSystem=strict \
      --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
      --property=WorkingDirectory=/ \
      "$node_replay_client_path" --qualified-all-three >"$replay_client_log" 2>&1
  local drift_client_status=$?
  set -e
  cat "$replay_client_log"
  [[ $drift_client_status -ne 0 && $drift_client_status -ne 124 ]] \
    || die "mutated rootfs did not fail one client promptly before checker execution"
  [[ $(grep -Fc 'native-shadow-closed-local-replay-report:' "$replay_client_log" || :) -eq 0 ]] \
    || die "request-time rootfs mutation produced a checker Report"
  [[ $(grep -Fc 'native-shadow-closed-local-replay-client-complete:' "$replay_client_log" || :) -eq 0 ]] \
    || die "request-time rootfs mutation completed the three-session matrix"
  sudo systemctl stop "$unit_name"
  wait_for_state inactive
  sudo journalctl --sync
  local drift_journal
  drift_journal=$(sudo journalctl --no-pager -o cat -u "$unit_name" \
    "_SYSTEMD_INVOCATION_ID=$drift_invocation")
  grep -F 'runtime rootfs replay identity drifted' <<<"$drift_journal" >/dev/null \
    || die "request-time rootfs mutation was not the launcher rejection reason"
  if sudo test -d "$service_root"; then
    [[ -z $(sudo find "$service_root" -mindepth 1 -maxdepth 1 -type d -name 'run-*' -print -quit) ]] \
      || die "request-time rootfs rejection created a checker cgroup leaf"
  fi
  sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
    || die "request-time rootfs rejection left the fixed socket behind"
  wait_for_cgroup_removal
  sudo systemctl reset-failed "$unit_name" >/dev/null 2>&1 || :
  local drift_load_state=''
  local drift_wait
  for ((drift_wait = 0; drift_wait < 200; drift_wait++)); do
    drift_load_state=$(sudo systemctl show "${node_unit}.service" \
      --property=LoadState --value 2>/dev/null || :)
    [[ "$drift_load_state" == not-found ]] && break
    sleep 0.05
  done
  [[ "$drift_load_state" == not-found ]] \
    || die "rootfs-drift transient client unit was not collected"
  node_unit=''

  sudo cp --preserve=all "$mutation_backup" "$mutation_target"
  [[ $(sha256sum "$mutation_target" | awk '{ print $1 }') == "$mutation_expected_sha" ]] \
    || die "frozen mutation target was not restored exactly"
  sudo rm -f "$mutation_backup"
  mutation_backup=''
  mutation_target=''
  : >"$replay_client_log"

  # Replace the CI harness at the exact production path only after the
  # request-time drift gate has completed.  The final matrix must traverse the
  # installed node HTTP route, one qualified launcher process, and the real
  # contained checker; the old direct Unix three-case client is not used here.
  sudo install -o root -g root -m 0755 "$production_launcher_source" "$launcher_path"
  [[ $(sha256sum "$production_launcher_source" | awk '{ print $1 }') == $(sudo sha256sum "$launcher_path" | awk '{ print $1 }') ]] \
    || die "installed production launcher bytes differ from the reviewed binary"
  set_mode normal
  sudo rm -f "$node_journal_path"

  # Bound the production audit to messages written after this point.  The
  # journal's InvocationID secondary index can lag behind `journalctl --sync`
  # on a just-started unit, so it is not sufficient for the exact peer count.
  # A global cursor gives us one kernel-journal ordering boundary; `_PID`
  # below then proves every matching message came from the qualified launcher
  # process rather than an older invocation of the same unit.
  sudo journalctl --sync
  local launcher_journal_cursor
  launcher_journal_cursor=$(sudo journalctl --no-pager --show-cursor -n 0 \
    | sed -n 's/^-- cursor: //p')
  [[ -n "$launcher_journal_cursor" ]] \
    || die "could not freeze the pre-launcher journal cursor"

  # Starting the node is the only explicit start. Its tracked Wants=/After=
  # relationship starts the launcher, while the node's bounded ENOENT/
  # ECONNREFUSED retry handles the intentional socket-readiness race. Do not
  # pre-start or pre-wait for the launcher socket here.
  sudo systemctl start "$node_service_name"
  local launcher_invocation
  local launcher_pid
  local node_invocation
  local node_pid_before
  launcher_invocation=$(unit_invocation_id)
  launcher_pid=$(assert_manager_invariants)
  node_invocation=$(sudo systemctl show "$node_service_name" --property=InvocationID --value)
  [[ "$node_invocation" =~ ^[0-9a-f]{32}$ ]] \
    || die "production replay node has invalid InvocationID: $node_invocation"
  node_pid_before=$(sudo systemctl show "$node_service_name" --property=MainPID --value)
  [[ "$node_pid_before" =~ ^[1-9][0-9]*$ ]] \
    || die "production replay node has invalid MainPID: $node_pid_before"

  set +e
  timeout --foreground --signal=TERM --kill-after=10s 420s \
    sudo -u boole-node python3 "$http_replay_gate_path" \
      --grant-path "$http_replay_grant_path" \
      --fixture-directory "$http_replay_fixture_directory" \
      --journal-path "$node_journal_path" >"$replay_client_log" 2>&1
  local client_status=$?
  set -e
  cat "$replay_client_log"
  if [[ $client_status -ne 0 ]]; then
    sudo systemctl show "$node_service_name" \
      --property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :
    sudo systemctl show "$unit_name" \
      --property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :
    sudo journalctl --no-pager -o cat -u "$node_service_name" \
      "_SYSTEMD_INVOCATION_ID=$node_invocation" >&2 || :
    sudo journalctl --no-pager -o cat -u "$unit_name" \
      "_SYSTEMD_INVOCATION_ID=$launcher_invocation" >&2 || :
    die "production HTTP replay matrix failed or exceeded its outer deadline"
  fi

  local marker
  for marker in \
    native-shadow-http-replay-case:accepted:PASS \
    native-shadow-http-replay-case:tampered:PASS \
    native-shadow-http-replay-case:constant:PASS \
    native-shadow-http-replay-case:empty:PASS \
    native-shadow-http-replay-journal:PASS \
    native-shadow-http-replay-matrix:PASS; do
    [[ $(grep -Fxc "$marker" "$replay_client_log" || :) -eq 1 ]] \
      || die "production HTTP replay matrix omitted exact marker: $marker"
  done

  # The launcher reads SO_PEERCRED on every checker connection. The HTTP
  # client talks only to the node, so all three kernel-authenticated launcher
  # peers must be the one stable replay-node MainPID, never the Python driver.
  sudo journalctl --sync
  local -a peer_pids=()
  local peer_pid_wait
  for ((peer_pid_wait = 0; peer_pid_wait < 200; peer_pid_wait++)); do
    mapfile -t peer_pids < <(
      sudo journalctl --no-pager -o cat -u "$unit_name" \
        --after-cursor "$launcher_journal_cursor" "_PID=$launcher_pid" \
        | sed -n 's/^native-shadow-active-execution-peer:pid=\([1-9][0-9]*\)$/\1/p'
    )
    [[ ${#peer_pids[@]} -eq 3 ]] && break
    (( ${#peer_pids[@]} < 3 )) \
      || die "launcher emitted more than three active execution peer identities"
    sleep 0.05
  done
  if ! [[ ${#peer_pids[@]} -eq 3 ]]; then
    printf 'native-shadow active execution peer PID count: expected=3 observed=%s\n' \
      "${#peer_pids[@]}" >&2
    printf 'native-shadow active execution peer PID observed:%s\n' \
      "${peer_pids[@]:-none}" >&2
    sudo journalctl --no-pager -o cat -u "$unit_name" \
      --after-cursor "$launcher_journal_cursor" >&2 || :
    die "launcher did not observe exactly three SO_PEERCRED process identities"
  fi
  local peer_pid
  for peer_pid in "${peer_pids[@]}"; do
    [[ "$peer_pid" == "$node_pid_before" ]] \
      || die "launcher SO_PEERCRED PID differs from the one fixed replay-node process"
  done

  # Each successful execution transport validates both SO_PEERCRED launcher
  # PID and launcherInstanceId against the one qualification result. Keep the
  # systemd process/invocation stable too, then publish one auditable gate log.
  [[ $(sudo systemctl show "$node_service_name" --property=MainPID --value) == "$node_pid_before" ]] \
    || die "production replay node process changed during the matrix"
  [[ $(sudo systemctl show "$node_service_name" --property=InvocationID --value) == "$node_invocation" ]] \
    || die "production replay node invocation changed during the matrix"
  [[ $(sudo systemctl show "$node_service_name" --property=NRestarts --value) == 0 ]] \
    || die "production replay node restarted during the matrix"
  grep -F 'execution_launcher_pid_drift' \
    crates/boole-node/src/native_shadow_replay_service.rs >/dev/null \
    || die "production transport lost the qualified launcher PID binding"
  grep -F 'execution_launcher_instance_drift' \
    crates/boole-node/src/native_shadow_replay_service.rs >/dev/null \
    || die "production transport lost the qualified launcher instance binding"
  printf 'native-shadow-production-qualified-binding:launcher_pid=%s;launcher_invocation=%s;node_pid=%s;node_invocation=%s\n' \
    "$launcher_pid" "$launcher_invocation" "$node_pid_before" "$node_invocation"

  wait_for_state inactive
  [[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
    || die "production three-execution launcher did not exit successfully"
  [[ $(sudo systemctl show "$unit_name" --property=NRestarts --value) == 0 ]] \
    || die "production three-execution launcher restarted unexpectedly"
  [[ $(sudo systemctl show "$unit_name" --property=InvocationID --value) == "$launcher_invocation" ]] \
    || die "production launcher invocation changed during the matrix"

  sudo systemctl stop "$node_service_name"
  local i
  local node_state=''
  for ((i = 0; i < 200; i++)); do
    node_state=$(sudo systemctl show "$node_service_name" --property=ActiveState --value 2>/dev/null || :)
    [[ "$node_state" == inactive ]] && break
    sleep 0.05
  done
  [[ "$node_state" == inactive ]] \
    || die "production replay node did not stop cleanly"
  [[ $(sudo systemctl show "$node_service_name" --property=MainPID --value) == 0 ]] \
    || die "production replay node process remained after stop"
  sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
    || die "production HTTP replay left the fixed socket path behind"
  wait_for_cgroup_removal
  sudo test ! -e "$node_service_root" \
    || die "production replay node cgroup remained after stop"
  [[ -z $(sudo find /run/boole/native-shadow -maxdepth 1 -name 'rootfs-*' -print -quit) ]] \
    || die "production HTTP replay left a derived runtime-root path behind"
  [[ -z $(findmnt -rn -o TARGET | grep -F '/run/boole/native-shadow/rootfs-' || :) ]] \
    || die "production HTTP replay left a derived runtime-root mount behind"
  echo "native-shadow production HTTP replay gate: PASS"
}

if [[ "$closed_local_replay_only" == true ]]; then
  run_closed_local_replay_gate
  exit 0
fi

run_expected_rejection nested-reject native-shadow-manager-nested-rejected
run_expected_rejection frozen-reject native-shadow-manager-frozen-rejected
run_expected_rejection multithread-reject native-shadow-manager-multithread-rejected

startup_recovery_mode="startup-recovery"
set_mode "$startup_recovery_mode"
sudo rm -f "$recovery_release_path"
sudo systemctl start boole-native-shadow-launcher.service
startup_recovery_invocation=$(unit_invocation_id)
wait_for_marker native-shadow-startup-recovery-prepared "$startup_recovery_invocation"
assert_manager_invariants >/dev/null

leaf_a="$service_root/run-0000000000000000000000000000000000000000000000000000000000000001"
leaf_b="$service_root/run-0000000000000000000000000000000000000000000000000000000000000002"
leaf_c="$service_root/run-0000000000000000000000000000000000000000000000000000000000000003"
create_run_leaf "$leaf_a"
create_run_leaf "$leaf_b"
create_run_leaf "$leaf_c"
start_process_tree "$leaf_a"
recovered_tree_a=$tree_supervisor_pid
start_process_tree "$leaf_b"
recovered_tree_b=$tree_supervisor_pid
wait_for_leaf_process_count "$leaf_a" 2
wait_for_leaf_process_count "$leaf_b" 2
mapfile -t recovered_cgroup_pids < <(
  {
    sudo cat "$leaf_a/cgroup.procs"
    sudo cat "$leaf_b/cgroup.procs"
  } | sort -n
)
[[ ${#recovered_cgroup_pids[@]} -eq 4 ]] \
  || die "startup recovery fixture does not contain four cgroup processes"
declare -a recovered_pid_starttimes=()
for recovered_pid in "${recovered_cgroup_pids[@]}"; do
  recovered_start_time=$(pid_start_time "$recovered_pid") \
    || die "could not identify recovery fixture process: $recovered_pid"
  [[ "$recovered_start_time" != missing ]] \
    || die "recovery fixture process disappeared before release: $recovered_pid"
  recovered_pid_starttimes+=("$recovered_start_time")
done
printf '1\n' | sudo tee "$leaf_b/cgroup.freeze" >/dev/null
wait_for_leaf_event "$leaf_b" frozen 1
sudo touch "$recovery_release_path"
wait_for_marker native-shadow-startup-recovery-complete:3 "$startup_recovery_invocation"
for recovered_index in "${!recovered_cgroup_pids[@]}"; do
  wait_for_original_process_exit \
    "${recovered_cgroup_pids[$recovered_index]}" \
    "${recovered_pid_starttimes[$recovered_index]}"
done
wait_for_background_job "$recovered_tree_a"
wait_for_background_job "$recovered_tree_b"
[[ -z $(sudo find "$service_root" -mindepth 1 -maxdepth 1 -type d ! -name manager -print -quit) ]] \
  || die "startup recovery left a non-manager cgroup child"
assert_manager_invariants >/dev/null
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

inventory_reject_mode="startup-inventory-reject"
set_mode "$inventory_reject_mode"
sudo rm -f "$recovery_release_path"
sudo systemctl start boole-native-shadow-launcher.service
inventory_reject_invocation=$(unit_invocation_id)
wait_for_marker native-shadow-startup-inventory-prepared "$inventory_reject_invocation"
assert_manager_invariants >/dev/null
leaf="$service_root/run-0000000000000000000000000000000000000000000000000000000000000001"
create_run_leaf "$leaf"
start_process_tree "$leaf"
reject_tree=$tree_supervisor_pid
wait_for_leaf_process_count "$leaf" 2
sudo mkdir "$service_root/zzz-unexpected"
frozen=$(sudo awk '$1 == "frozen" && NF == 2 { print $2 }' "$leaf/cgroup.events")
populated=$(sudo awk '$1 == "populated" && NF == 2 { print $2 }' "$leaf/cgroup.events")
[[ "$frozen" == 0 && "$populated" == 1 ]] \
  || die "inventory reject fixture did not begin live and unfrozen"
before_procs=$(sudo sort -n "$leaf/cgroup.procs")
before_threads=$(sudo sort -n "$leaf/cgroup.threads")
sudo touch "$recovery_release_path"
wait_for_marker native-shadow-startup-inventory-untouched "$inventory_reject_invocation"
after_procs=$(sudo sort -n "$leaf/cgroup.procs")
after_threads=$(sudo sort -n "$leaf/cgroup.threads")
[[ "$before_procs" == "$after_procs" && "$before_threads" == "$after_threads" ]] \
  || die "inventory rejection changed the valid leaf process membership"
frozen=$(sudo awk '$1 == "frozen" && NF == 2 { print $2 }' "$leaf/cgroup.events")
populated=$(sudo awk '$1 == "populated" && NF == 2 { print $2 }' "$leaf/cgroup.events")
[[ "$frozen" == 0 && "$populated" == 1 ]] \
  || die "inventory rejection changed valid leaf events"
sudo test -d "$service_root/zzz-unexpected" \
  || die "inventory rejection removed the unexpected child"
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_background_job "$reject_tree"
wait_for_cgroup_removal

listener_mode="qualification-one-shot"
set_mode "$listener_mode"
sudo systemctl start boole-native-shadow-launcher.service
qualification_invocation=$(unit_invocation_id)
assert_manager_invariants >/dev/null
wait_for_fixed_socket

suffix=${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}-$$
suffix=${suffix//[^a-zA-Z0-9-]/-}
node_unit="boole-native-shadow-node-qualification-${suffix}"
set +e
sudo systemd-run --quiet --pipe --wait --collect --unit="$node_unit" \
  --property=Type=exec --property=User=boole-node --property=Group=boole-node \
  --property=CapabilityBoundingSet= --property=AmbientCapabilities= \
  --property=NoNewPrivileges=yes --property=PrivateMounts=yes \
  --property="BindReadOnlyPaths=${authority_share}:/usr/share" \
  --property=WorkingDirectory=/ \
  "$node_qualification_path" \
  native_shadow_qualification::tests::installed_launcher_round_trip_is_ready_only \
  --ignored --exact --nocapture >"$node_log" 2>&1
node_status=$?
set -e
cat "$node_log"
[[ $node_status -eq 0 ]] || die "boole-node installed qualification round trip failed"
node_marker_count=$(grep -Fxc native-shadow-node-qualification-ready-only "$node_log" || :)
[[ "$node_marker_count" -eq 1 ]] \
  || die "boole-node qualification test did not execute exactly once"
for ((i = 0; i < 200; i++)); do
  node_load_state=$(sudo systemctl show "${node_unit}.service" \
    --property=LoadState --value 2>/dev/null || :)
  [[ "$node_load_state" == not-found ]] && break
  sleep 0.05
done
[[ "$node_load_state" == not-found ]] \
  || die "boole-node qualification transient unit was not collected"
node_unit=''

wait_for_state inactive
[[ $(sudo systemctl show "$unit_name" --property=Result --value) == success ]] \
  || die "one-shot qualification launcher did not exit successfully"
[[ $(sudo systemctl show "$unit_name" --property=NRestarts --value) == 0 ]] \
  || die "one-shot qualification launcher restarted unexpectedly"
wait_for_marker native-shadow-qualification-one-shot-complete "$qualification_invocation"
sudo test ! -e "$socket_path" && sudo test ! -L "$socket_path" \
  || die "one-shot qualification left the fixed socket path behind"
wait_for_cgroup_removal

set_mode safe-reuse
sudo systemctl start boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

set_mode normal
sudo systemctl start boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
first_invocation=$(unit_invocation_id)
sudo systemctl restart boole-native-shadow-launcher.service
assert_manager_invariants >/dev/null
second_invocation=$(unit_invocation_id)
[[ "$first_invocation" != "$second_invocation" ]] \
  || die "explicit restart reused the same InvocationID"
sudo systemctl stop boole-native-shadow-launcher.service
wait_for_state inactive
wait_for_cgroup_removal

echo "native-shadow manager cgroup gate: PASS"
