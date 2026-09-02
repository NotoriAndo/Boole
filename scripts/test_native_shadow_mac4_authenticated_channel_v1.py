#!/usr/bin/env python3
"""Behavior gates for the first MAC.4 authenticated host/guest channel."""

from __future__ import annotations

import hashlib
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as image
from scripts import native_shadow_closed_local_image_to_readiness_arm64_v3 as current_image

CONTRACT = (
    ROOT
    / "native/containment/native-shadow-mac4-authenticated-channel-contract-v1.json"
)
PROXY_CONTRACT = (
    ROOT
    / "native/containment/native-shadow-mac4-execution-proxy-contract-v1.json"
)
CONTROLLER_CONTRACT = (
    ROOT
    / "native/containment/native-shadow-mac4-host-controller-contract-v1.json"
)
RELAY_MANIFEST = ROOT / "native/mac4/relay/Cargo.toml"
SERVICE = ROOT / "native/systemd/boole-native-shadow-mac4-relay-v2.service"
MAC_HOST = ROOT / "native/mac4/boole-mac4-auth-channel.swift"
WORKFLOW = (
    ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml"
)
SELF_TEST = ROOT / "scripts/self-test.sh"


class Mac4AuthenticatedChannelBehaviorTests(unittest.TestCase):
    @staticmethod
    def _controller_request(command: int, frames: list[bytes]) -> tuple[bytes, bytes]:
        payload = b"".join(struct.pack(">I", len(frame)) + frame for frame in frames)
        request_id = hashlib.sha256(
            bytes([command])
            + b"".join(struct.pack(">I", len(frame)) + frame for frame in frames)
        ).digest()
        header = (
            b"BOOLE4C1"
            + bytes([1, command])
            + struct.pack(">H", len(frames))
            + struct.pack(">I", len(payload))
            + request_id
            + bytes.fromhex(hashlib.sha256(CONTROLLER_CONTRACT.read_bytes()).hexdigest())
            + bytes(16)
        )
        return header + payload, request_id

    @staticmethod
    def _controller_response(data: bytes, offset: int) -> tuple[dict[str, object], int]:
        header = data[offset : offset + 96]
        if len(header) != 96:
            raise AssertionError("controller response header is truncated")
        frame_count = int.from_bytes(header[10:12], "big")
        payload_length = int.from_bytes(header[12:16], "big")
        payload = data[offset + 96 : offset + 96 + payload_length]
        frames: list[bytes] = []
        cursor = 0
        for _ in range(frame_count):
            length = int.from_bytes(payload[cursor : cursor + 4], "big")
            cursor += 4
            frames.append(payload[cursor : cursor + length])
            cursor += length
        if cursor != len(payload):
            raise AssertionError("controller response has trailing payload")
        return (
            {
                "magic": header[:8],
                "version": header[8],
                "kind": header[9],
                "request_id": header[16:48],
                "contract": header[48:80],
                "peer": struct.unpack(">III", header[80:92]),
                "reserved": header[92:96],
                "frames": frames,
            },
            offset + 96 + payload_length,
        )

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
        self.assertIn("9 passed", completed.stdout)

    def test_protocol_binary_is_bound_to_the_complete_tracked_contract(self):
        digest = hashlib.sha256(CONTRACT.read_bytes()).hexdigest()
        source = (ROOT / "native/mac4/relay/src/lib.rs").read_text(encoding="utf-8")
        self.assertIn(f'"{digest}"', source)
        self.assertEqual(digest, image.MAC4_CONTRACT_SHA256)

        proxy_digest = hashlib.sha256(PROXY_CONTRACT.read_bytes()).hexdigest()
        self.assertIn(f'"{proxy_digest}"', source)

        controller_digest = hashlib.sha256(CONTROLLER_CONTRACT.read_bytes()).hexdigest()
        self.assertIn(f'"{controller_digest}"', MAC_HOST.read_text(encoding="utf-8"))

    def test_development_overlay_installs_the_proxy_capable_relay_service(self):
        relay = b"arm64-linux-relay"
        with current_image._proxy_relay_service_contract():
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
        self.assertIn(
            b"RestrictAddressFamilies=AF_VSOCK AF_UNIX",
            entries[image.MAC4_SERVICE_STAGING_PATH]["raw"],
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

    def test_service_is_nonprivileged_vsock_and_fixed_unix_proxy_only(self):
        unit = SERVICE.read_text(encoding="utf-8")
        for required in (
            "User=boole-node",
            "Group=boole-node",
            "NoNewPrivileges=yes",
            "CapabilityBoundingSet=",
            "ProtectSystem=strict",
            "RestrictAddressFamilies=AF_VSOCK AF_UNIX",
            "IPAddressDeny=any",
            "ReadOnlyPaths=/usr/share/boole/native-shadow/mac4-channel-contract-v1.json",
            "ReadOnlyPaths=/usr/share/boole/native-shadow/mac4-execution-proxy-contract-v1.json",
        ):
            self.assertIn(required, unit)
        self.assertNotIn("ReadWritePaths=", unit)
        self.assertNotIn("AF_INET", unit)

    def test_mac_host_binds_qualification_and_execution_to_one_launcher_peer(self):
        source = MAC_HOST.read_text(encoding="utf-8")
        self.assertIn("qualificationPeer == executionPeer", source)
        self.assertIn("proxy launcher peer changed after qualification", source)
        self.assertIn('"launcherPeer"', source)
        self.assertIn("func runPersistentController()", source)
        self.assertIn("nonceBase: request.requestID", source)
        self.assertIn("controller input ended before explicit shutdown", source)
        self.assertIn("--controller-stdio requires the inherited runtime lease", source)
        self.assertIn("boole-mac4-controller-command:execution", source)

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
        self.assertIn('--depmod "$(command -v depmod)"', workflow)
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
            temporary_path = pathlib.Path(temporary)
            output = temporary_path / "mac4-host"
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
            signed = subprocess.run(
                [
                    "codesign",
                    "--force",
                    "--sign",
                    "-",
                    "--entitlements",
                    str(
                        ROOT
                        / "native/mac3/boole-mac3-closed-local-boot.entitlements"
                    ),
                    str(output),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(signed.returncode, 0, signed.stderr)
            kernel = temporary_path / "kernel"
            root_disk = temporary_path / "root-disk"
            console = temporary_path / "console"
            receipt = temporary_path / "receipt.json"
            qualification = temporary_path / "qualification.frame"
            execution_hello = temporary_path / "execution-hello.frame"
            execution_request = temporary_path / "execution-request.frame"
            kernel.write_bytes(b"kernel")
            root_disk.write_bytes(b"\0" * (1024 * 1024))
            for path, payload in (
                (qualification, b"qualification"),
                (execution_hello, b"execution-hello"),
                (execution_request, b"execution-request"),
            ):
                path.write_bytes(len(payload).to_bytes(4, "big") + payload)
            dry_run = subprocess.run(
                [
                    str(output),
                    "--dry-run",
                    "--proxy-dry-run",
                    "--kernel",
                    str(kernel),
                    "--root-disk",
                    str(root_disk),
                    "--console",
                    str(console),
                    "--receipt",
                    str(receipt),
                    "--cmdline",
                    "console=hvc0",
                    "--kernel-sha256",
                    hashlib.sha256(kernel.read_bytes()).hexdigest(),
                    "--root-disk-sha256",
                    hashlib.sha256(root_disk.read_bytes()).hexdigest(),
                    "--nonce-hex",
                    "11" * 32,
                    "--boot-binding-hex",
                    "22" * 32,
                    "--timeout",
                    "10",
                    "--proxy-qualification-hello",
                    str(qualification),
                    "--proxy-execution-hello",
                    str(execution_hello),
                    "--proxy-execution-request",
                    str(execution_request),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(dry_run.returncode, 0, dry_run.stderr)
            self.assertEqual(
                json.loads(receipt.read_text(encoding="utf-8"))["executionProxy"],
                {
                    "configured": True,
                    "persistentController": False,
                    "port": 4051,
                    "sessions": ["qualification", "execution"],
                },
            )

            qualification_frame = struct.pack(">I", 13) + b"qualification"
            hello_frame = struct.pack(">I", 5) + b"hello"
            request_frame = struct.pack(">I", 7) + b"request"
            qualification_request, qualification_id = self._controller_request(
                1, [qualification_frame]
            )
            execution_request, execution_id = self._controller_request(
                2, [hello_frame, request_frame]
            )
            shutdown_request, shutdown_id = self._controller_request(3, [])
            protocol = subprocess.run(
                [str(output), "--controller-protocol-dry-run"],
                cwd=ROOT,
                input=qualification_request + execution_request + shutdown_request,
                capture_output=True,
                check=False,
            )
            self.assertEqual(protocol.returncode, 0, protocol.stderr.decode())
            self.assertEqual(protocol.stderr, b"")
            qualification_response, offset = self._controller_response(protocol.stdout, 0)
            execution_response, offset = self._controller_response(protocol.stdout, offset)
            shutdown_response, offset = self._controller_response(protocol.stdout, offset)
            self.assertEqual(offset, len(protocol.stdout))
            contract_digest = hashlib.sha256(CONTROLLER_CONTRACT.read_bytes()).digest()
            for response, kind, request_id, peer in (
                (qualification_response, 0x81, qualification_id, (4242, 0, 0)),
                (execution_response, 0x82, execution_id, (4242, 0, 0)),
                (shutdown_response, 0x83, shutdown_id, (0, 0, 0)),
            ):
                self.assertEqual(response["magic"], b"BOOLE4C1")
                self.assertEqual(response["version"], 1)
                self.assertEqual(response["kind"], kind)
                self.assertEqual(response["request_id"], request_id)
                self.assertEqual(response["contract"], contract_digest)
                self.assertEqual(response["peer"], peer)
                self.assertEqual(response["reserved"], bytes(4))
            self.assertEqual(qualification_response["frames"][1], qualification_frame)
            self.assertEqual(execution_response["frames"][1:], [hello_frame, request_frame])
            self.assertEqual(shutdown_response["frames"], [])
        source = MAC_HOST.read_text(encoding="utf-8")
        self.assertIn("VZVirtioSocketDeviceConfiguration()", source)
        self.assertIn("connectGuest(port: VSOCK_PORT)", source)
        self.assertIn("connect(toPort: port)", source)
        self.assertIn("configuration.networkDevices = []", source)
        self.assertIn("configuration.directorySharingDevices = []", source)


if __name__ == "__main__":
    unittest.main()
