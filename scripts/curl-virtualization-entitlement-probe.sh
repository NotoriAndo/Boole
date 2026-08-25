#!/usr/bin/env bash
# CURL.3-PREP — Team-ID-free virtualization entitlement probe.
#
# Answers exactly one question of the CURL.3 canary: does macOS honor
# `com.apple.security.virtualization` in an ad-hoc signature that carries no
# Apple Team ID? It signs the same probe binary twice — once with the
# entitlement, once without — and requires the entitled run to reach
# VZVirtualMachine instantiation while the unentitled run is refused.
#
# Boundaries: no Apple identity, certificate or provisioning profile is used;
# no guest is booted; nothing is installed; the temporary work directory is
# removed on every exit path. Passing this probe is NOT a CURL.3 pass — it
# covers the entitlement ground only, and only on the machine that ran it. The
# full canary additionally requires a clean supported Mac, fixed pinned
# kernel/initrd/root-disk inputs and clean boot, shutdown and reboot
# boundaries.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "curl-virtualization-entitlement-probe: macOS only (found $(uname -s))" >&2
  exit 2
fi

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "curl-virtualization-entitlement-probe: Apple Silicon only (found $(uname -m))" >&2
  exit 2
fi

for tool in /usr/bin/swiftc /usr/bin/codesign; do
  if [[ ! -x "$tool" ]]; then
    echo "curl-virtualization-entitlement-probe: missing $tool" >&2
    exit 2
  fi
done

work_dir="$(mktemp -d)"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

echo "host macOS: $(sw_vers -productVersion) ($(sw_vers -buildVersion)), arch $(uname -m)"

cat > "$work_dir/virtualization.entitlements" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.virtualization</key>
    <true/>
</dict>
</plist>
PLIST

cat > "$work_dir/vzprobe.swift" <<'SWIFT'
import Foundation
import Virtualization

let kernelPath = CommandLine.arguments[1]

print("isSupported=\(VZVirtualMachine.isSupported)")

let configuration = VZVirtualMachineConfiguration()
configuration.cpuCount = 1
configuration.memorySize = 1024 * 1024 * 1024
configuration.bootLoader = VZLinuxBootLoader(kernelURL: URL(fileURLWithPath: kernelPath))

do {
    try configuration.validate()
    print("validate=ok")
} catch {
    print("validate=failed: \(error)")
    exit(2)
}

// Instantiation is where the entitlement is enforced. The machine is never
// started, so no guest runs and no state is left behind.
let machine = VZVirtualMachine(configuration: configuration)
print("instantiate=ok state=\(machine.state.rawValue)")
exit(0)
SWIFT

/usr/bin/swiftc -O -o "$work_dir/vzprobe" "$work_dir/vzprobe.swift"

# A placeholder boot input: the probe never boots, so its contents are
# irrelevant. The real canary pins kernel, initrd and root disk by SHA-256.
head -c 1048576 /dev/zero > "$work_dir/placeholder-kernel"

cp "$work_dir/vzprobe" "$work_dir/vzprobe-entitled"
cp "$work_dir/vzprobe" "$work_dir/vzprobe-unentitled"
/usr/bin/codesign --force --sign - \
  --entitlements "$work_dir/virtualization.entitlements" "$work_dir/vzprobe-entitled" 2>/dev/null
/usr/bin/codesign --force --sign - "$work_dir/vzprobe-unentitled" 2>/dev/null

signature_form="$(/usr/bin/codesign -dvv "$work_dir/vzprobe-entitled" 2>&1 | grep '^Signature=' || true)"
team_identifier="$(/usr/bin/codesign -dvv "$work_dir/vzprobe-entitled" 2>&1 | grep '^TeamIdentifier=' || true)"
echo "entitled binary: ${signature_form:-Signature=unknown}, ${team_identifier:-TeamIdentifier=unknown}"

if [[ "$signature_form" != "Signature=adhoc" ]]; then
  echo "probe: FAIL — the probe binary is not ad-hoc signed" >&2
  exit 1
fi
if [[ "$team_identifier" != "TeamIdentifier=not set" ]]; then
  echo "probe: FAIL — the probe binary carries a Team ID; this is not the Team-ID-free form" >&2
  exit 1
fi

set +e
entitled_output="$("$work_dir/vzprobe-entitled" "$work_dir/placeholder-kernel" 2>&1)"
unentitled_output="$("$work_dir/vzprobe-unentitled" "$work_dir/placeholder-kernel" 2>&1)"
set -e

echo "entitled run: $(echo "$entitled_output" | tr '\n' ' ')"
echo "unentitled run: $(echo "$unentitled_output" | tr '\n' ' ')"

if ! grep -q 'instantiate=ok' <<<"$entitled_output"; then
  echo "probe: FAIL — the ad-hoc entitled binary could not instantiate a virtual machine" >&2
  exit 1
fi

if ! grep -q 'com.apple.security.virtualization' <<<"$unentitled_output"; then
  echo "probe: FAIL — the unentitled binary was not refused; the entitlement is not being enforced" >&2
  exit 1
fi

echo "probe: PASS — macOS honored the Team-ID-free ad-hoc entitlement and refused the unentitled binary"
echo "probe: this covers the entitlement ground only; CURL.3 additionally requires a clean supported Mac,"
echo "probe: fixed pinned boot inputs and clean boot, shutdown and reboot boundaries"
