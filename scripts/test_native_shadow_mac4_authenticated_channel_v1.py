#!/usr/bin/env python3
"""Behavior gates for the first MAC.4 authenticated host/guest channel."""

from __future__ import annotations

import hashlib
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as image

CONTRACT = (
    ROOT
    / "native/containment/native-shadow-mac4-authenticated-channel-contract-v1.json"
)
RELAY_MANIFEST = ROOT / "native/mac4/relay/Cargo.toml"
SERVICE = ROOT / "native/systemd/boole-native-shadow-mac4-relay.service"
MAC_HOST = ROOT / "native/mac4/boole-mac4-auth-channel.swift"
WORKFLOW = (
    ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml"
)
SELF_TEST = ROOT / "scripts/self-test.sh"


class Mac4AuthenticatedChannelBehaviorTests(unittest.TestCase):
    def test_public_handshake_rejects_wrong_attempt_image_and_protocol(self):
        completed = subprocess.run(
            [
                "cargo",
                "test",
                "--manifest-path",
                str(RELAY_MANIFEST),
                "--test",
                "handshake",
                "--",
                "--nocapture",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("5 passed", completed.stdout)

    def test_protocol_binary_is_bound_to_the_complete_tracked_contract(self):
        digest = hashlib.sha256(CONTRACT.read_bytes()).hexdigest()
        source = (ROOT / "native/mac4/relay/src/lib.rs").read_text(encoding="utf-8")
        self.assertIn(f'"{digest}"', source)
        self.assertEqual(digest, image.MAC4_CONTRACT_SHA256)

    def test_development_overlay_installs_relay_service_and_exact_vsock_load_contract(self):
        relay = b"arm64-linux-relay"
        entries = image._development_mac4_entries(ROOT, relay)
        self.assertEqual(
            set(entries),
            {
                image.MAC4_RELAY_STAGING_PATH,
                image.MAC4_SERVICE_STAGING_PATH,
                image.MAC4_SERVICE_ENABLEMENT_PATH,
                image.MAC4_CONTRACT_STAGING_PATH,
                image.MAC4_MODULE_LOAD_STAGING_PATH,
            },
        )
        self.assertEqual(entries[image.MAC4_RELAY_STAGING_PATH]["raw"], relay)
        self.assertEqual(entries[image.MAC4_RELAY_STAGING_PATH]["mode"], 0o555)
        self.assertEqual(
            entries[image.MAC4_SERVICE_STAGING_PATH]["raw"], SERVICE.read_bytes()
        )
        self.assertEqual(entries[image.MAC4_SERVICE_STAGING_PATH]["mode"], 0o444)
        self.assertEqual(
            entries[image.MAC4_CONTRACT_STAGING_PATH]["raw"], CONTRACT.read_bytes()
        )
        self.assertEqual(
            entries[image.MAC4_SERVICE_ENABLEMENT_PATH]["target"],
            "/" + image.MAC4_SERVICE_STAGING_PATH,
        )
        self.assertEqual(
            entries[image.MAC4_MODULE_LOAD_STAGING_PATH]["raw"],
            b"vsock\nvmw_vsock_virtio_transport_common\n"
            b"vmw_vsock_virtio_transport\n",
        )
        self.assertEqual(
            entries[image.MAC4_MODULE_LOAD_STAGING_PATH]["mode"], 0o444
        )

    def test_image_cli_requires_the_relay_in_both_reversible_modes(self):
        help_result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v1.py"), "build", "--help"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(help_result.returncode, 0, help_result.stderr)
        self.assertIn("--mac4-relay", help_result.stdout)

    def test_service_is_nonprivileged_vsock_only_and_cannot_write_the_guest(self):
        unit = SERVICE.read_text(encoding="utf-8")
        for required in (
            "User=boole-node",
            "Group=boole-node",
            "NoNewPrivileges=yes",
            "CapabilityBoundingSet=",
            "ProtectSystem=strict",
            "RestrictAddressFamilies=AF_VSOCK",
            "IPAddressDeny=any",
            "ReadOnlyPaths=/usr/share/boole/native-shadow/mac4-channel-contract-v1.json",
        ):
            self.assertIn(required, unit)
        self.assertNotIn("ReadWritePaths=", unit)
        self.assertNotIn("AF_INET", unit)
        self.assertNotIn("AF_UNIX", unit)

    def test_standalone_relay_does_not_change_the_root_cargo_workspace(self):
        root_manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        self.assertNotIn("boole-native-shadow-mac4-relay", root_manifest)
        relay_manifest = RELAY_MANIFEST.read_text(encoding="utf-8")
        self.assertIn("[workspace]", relay_manifest)
        self.assertNotIn("dependencies", relay_manifest)

    def test_arm64_image_lane_builds_and_passes_the_exact_standalone_relay(self):
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "cargo build --locked --release --manifest-path native/mac4/relay/Cargo.toml",
            workflow,
        )
        self.assertIn("readelf -h", workflow)
        self.assertIn('mac4_relay="$run_root/boole-native-shadow-mac4-relay"', workflow)
        self.assertIn('--mac4-relay "$mac4_relay"', workflow)
        self.assertIn('--depmod "$(readlink -f "$(command -v depmod)")"', workflow)
        self.assertIn(
            '"$RUNNER_TEMP/mac4-relay/boole-native-shadow-mac4-relay" "$mac4_relay"',
            workflow,
        )
        self.assertIn("test_native_shadow_mac4_authenticated_channel_v1", workflow)

    def test_mac4_gates_run_in_the_required_full_self_test(self):
        self_test = SELF_TEST.read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac4-authenticated-channel", self_test
        )
        self.assertIn(
            "scripts/test_native_shadow_mac4_authenticated_channel_v1.py", self_test
        )
        self.assertIn(
            "scripts/test_native_shadow_mac4_channel_runner_arm64_v1.py", self_test
        )

    @unittest.skipUnless(sys.platform == "darwin", "Swift Virtualization host is macOS-only")
    def test_mac_host_compiles_and_has_one_vsock_without_ip_or_shares(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary) / "mac4-host"
            compatible = pathlib.Path(
                "/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk"
            )
            sdk = (
                str(compatible)
                if compatible.is_dir()
                else subprocess.run(
                    ["xcrun", "--sdk", "macosx", "--show-sdk-path"],
                    capture_output=True,
                    text=True,
                    check=True,
                ).stdout.strip()
            )
            module_cache = pathlib.Path(temporary) / "module-cache"
            completed = subprocess.run(
                [
                    "swiftc",
                    "-sdk",
                    sdk,
                    "-target",
                    "arm64-apple-macos14.0",
                    "-module-cache-path",
                    str(module_cache),
                    "-framework",
                    "Virtualization",
                    str(MAC_HOST),
                    "-o",
                    str(output),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
        source = MAC_HOST.read_text(encoding="utf-8")
        self.assertIn("VZVirtioSocketDeviceConfiguration()", source)
        self.assertIn("connect(toPort: VSOCK_PORT", source)
        self.assertIn("configuration.networkDevices = []", source)
        self.assertIn("configuration.directorySharingDevices = []", source)


if __name__ == "__main__":
    unittest.main()
