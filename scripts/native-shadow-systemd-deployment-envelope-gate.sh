#!/usr/bin/env bash
set -euo pipefail

# Materialize the tracked qualification service envelope under a disposable
# root and ask systemd's own parsers/provisioners to validate it. This proves
# unit syntax, service-account shape, and runtime-directory ownership without
# installing or starting the qualification launcher on the CI host.

die() {
  echo "native-shadow systemd deployment envelope gate: $*" >&2
  exit 1
}

[[ $(uname -s) == Linux ]] || die "this gate requires Linux"
[[ ${EUID} -eq 0 ]] || die "this gate requires root for alternate-root ownership"
for command_name in systemd-analyze systemd-sysusers systemd-tmpfiles awk stat; do
  command -v "$command_name" >/dev/null || die "$command_name is unavailable"
done

stage=$(mktemp -d /tmp/boole-native-shadow-systemd-envelope.XXXXXX)
cleanup() {
  rm -rf -- "$stage"
}
trap cleanup EXIT

install -d -m 0755 \
  "$stage/etc" \
  "$stage/usr/lib/systemd/system" \
  "$stage/usr/lib/sysusers.d" \
  "$stage/usr/lib/tmpfiles.d" \
  "$stage/usr/bin" \
  "$stage/usr/libexec/boole"
install -m 0644 \
  native/systemd/boole-native-shadow-launcher.service \
  "$stage/usr/lib/systemd/system/boole-native-shadow-launcher.service"
install -m 0644 \
  native/sysusers.d/boole-native-shadow.conf \
  "$stage/usr/lib/sysusers.d/boole-native-shadow.conf"
install -m 0644 \
  native/tmpfiles.d/boole-native-shadow.conf \
  "$stage/usr/lib/tmpfiles.d/boole-native-shadow.conf"
install -m 0755 /bin/true \
  "$stage/usr/libexec/boole/boole-native-shadow-launcher"
install -m 0755 /bin/true "$stage/usr/bin/true"

for tracked_path in \
  "$stage/usr/lib/systemd/system/boole-native-shadow-launcher.service" \
  "$stage/usr/lib/sysusers.d/boole-native-shadow.conf" \
  "$stage/usr/lib/tmpfiles.d/boole-native-shadow.conf"; do
  config_metadata=$(stat -c %U:%G:%a "$tracked_path")
  [[ "$config_metadata" == root:root:644 ]] \
    || die "staged deployment config is not root:root mode 0644"
done
launcher_metadata=$(stat -c %U:%G:%a \
  "$stage/usr/libexec/boole/boole-native-shadow-launcher")
[[ "$launcher_metadata" == root:root:755 ]] \
  || die "staged launcher is not root:root mode 0755"

# Keep alternate-root verification self-contained. The two valid oneshot
# stubs and four target stubs satisfy only explicit and default systemd
# dependencies; no service is started by this gate.
printf '[Unit]\nDescription=staged sysusers service\n[Service]\nType=oneshot\nExecStart=/usr/bin/true\n' \
  >"$stage/usr/lib/systemd/system/systemd-sysusers.service"
printf '[Unit]\nDescription=staged tmpfiles service\n[Service]\nType=oneshot\nExecStart=/usr/bin/true\n' \
  >"$stage/usr/lib/systemd/system/systemd-tmpfiles-setup.service"
for target in sysinit.target basic.target shutdown.target multi-user.target; do
  printf '[Unit]\nDescription=staged dependency target\n' \
    >"$stage/usr/lib/systemd/system/$target"
done

systemd-analyze --root="$stage" verify boole-native-shadow-launcher.service
systemd-sysusers --root="$stage" "$stage/usr/lib/sysusers.d/boole-native-shadow.conf"

node_uid=$(awk -F: '$1 == "boole-node" { print $3 }' "$stage/etc/passwd")
node_gid=$(awk -F: '$1 == "boole-node" { print $4 }' "$stage/etc/passwd")
node_group_gid=$(awk -F: '$1 == "boole-node" { print $3 }' "$stage/etc/group")
node_home=$(awk -F: '$1 == "boole-node" { print $6 }' "$stage/etc/passwd")
node_shell=$(awk -F: '$1 == "boole-node" { print $7 }' "$stage/etc/passwd")
checker_uid=$(awk -F: '$1 == "boole-native-checker" { print $3 }' "$stage/etc/passwd")
checker_gid=$(awk -F: '$1 == "boole-native-checker" { print $4 }' "$stage/etc/passwd")
checker_group_gid=$(awk -F: '$1 == "boole-native-checker" { print $3 }' "$stage/etc/group")
checker_home=$(awk -F: '$1 == "boole-native-checker" { print $6 }' "$stage/etc/passwd")
checker_shell=$(awk -F: '$1 == "boole-native-checker" { print $7 }' "$stage/etc/passwd")

[[ "$node_uid" =~ ^[0-9]+$ && "$node_gid" =~ ^[0-9]+$ ]] \
  || die "boole-node numeric identity was not created"
[[ "$checker_uid" =~ ^[0-9]+$ && "$checker_gid" =~ ^[0-9]+$ ]] \
  || die "boole-native-checker numeric identity was not created"
[[ "$node_uid" -ne 0 && "$checker_uid" -ne 0 ]] \
  || die "service identities must be non-root"
[[ "$node_gid" -ne 0 && "$checker_gid" -ne 0 ]] \
  || die "service primary groups must be non-root"
[[ "$node_uid" -ne "$checker_uid" ]] \
  || die "service UIDs must be distinct"
[[ "$node_gid" -ne "$checker_gid" ]] \
  || die "service primary GIDs must be distinct"
[[ "$node_gid" -eq "$node_group_gid" ]] \
  || die "boole-node primary group does not match its passwd GID"
[[ "$checker_gid" -eq "$checker_group_gid" ]] \
  || die "boole-native-checker primary group does not match its passwd GID"
[[ "$node_home" == /nonexistent && "$node_shell" == /usr/sbin/nologin ]] \
  || die "boole-node home or shell differs from the frozen contract"
[[ "$checker_home" == /nonexistent && "$checker_shell" == /bin/false ]] \
  || die "boole-native-checker home or shell differs from the frozen contract"
if awk -F: '$4 ~ /(^|,)(boole-node|boole-native-checker)(,|$)/ { found=1 } END { exit !found }' \
  "$stage/etc/group"; then
  die "service identities must have no supplementary groups"
fi

systemd-tmpfiles --root="$stage" --create "$stage/usr/lib/tmpfiles.d/boole-native-shadow.conf"
runtime_parent_metadata=$(stat -c %u:%g:%a "$stage/run/boole")
[[ "$runtime_parent_metadata" == 0:0:755 ]] \
  || die "runtime parent differs from root:root mode 0755"
runtime_mode=$(stat -c %a "$stage/run/boole/native-shadow")
runtime_uid=$(stat -c %u "$stage/run/boole/native-shadow")
runtime_gid=$(stat -c %g "$stage/run/boole/native-shadow")
[[ "$runtime_mode" == 2750 ]] \
  || die "runtime directory mode differs from 2750"
[[ "$runtime_uid" -eq 0 && "$runtime_gid" -eq "$node_gid" ]] \
  || die "runtime directory ownership differs from root:boole-node"

echo "native-shadow-systemd-deployment-envelope-gate-complete"
