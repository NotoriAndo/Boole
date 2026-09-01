#!/usr/bin/env python3
"""Behavior tests for the closed-local Mac MAC.4 channel runner."""

from __future__ import annotations

import hashlib
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from scripts import native_shadow_mac4_authenticated_channel_arm64_v1 as runner


def images():
    return [
        {"name": "guest-kernel", "path": "/k", "sha256": "11" * 32, "sizeBytes": 1},
        {"name": "guest-initrd", "path": "/i", "sha256": "22" * 32, "sizeBytes": 2},
        {"name": "guest-root-disk", "path": "/r", "sha256": "33" * 32, "sizeBytes": 3},
    ]


class Mac4ChannelRunnerBehaviorTests(unittest.TestCase):
    def test_boot_tuple_binding_covers_all_three_ordered_artifacts(self):
        expected = hashlib.sha256(
            b"boole.mac4.boot-tuple.v1\0"
            + bytes.fromhex("11" * 32)
            + bytes.fromhex("22" * 32)
            + bytes.fromhex("33" * 32)
        ).hexdigest()
        self.assertEqual(runner.boot_tuple_binding(images()), expected)
        mutated = images()
        mutated[1] = dict(mutated[1], sha256="44" * 32)
        self.assertNotEqual(runner.boot_tuple_binding(mutated), expected)

    def test_host_arguments_bind_fresh_nonce_tuple_and_read_only_boot_inputs(self):
        argv = runner.host_argv(
            pathlib.Path("/host"),
            images(),
            nonce_hex="55" * 32,
            binding_hex="66" * 32,
            console=pathlib.Path("/console"),
            receipt=pathlib.Path("/receipt"),
            timeout=60,
            dry_run=False,
        )
        self.assertIn("--nonce-hex", argv)
        self.assertIn("55" * 32, argv)
        self.assertIn("--boot-binding-hex", argv)
        self.assertIn("66" * 32, argv)
        self.assertIn("/k", argv)
        self.assertIn("/r", argv)
        self.assertNotIn("/i", argv)

    def test_result_requires_authenticated_receipt_and_unchanged_images(self):
        before = images()
        receipt = {
            "schema": "boole.native-shadow.mac4-authenticated-channel-run.v1",
            "outcome": "authenticated-channel-pass",
            "dryRun": False,
            "nonceHex": "55" * 32,
            "bootTupleBindingHex": runner.boot_tuple_binding(before),
            "contractSha256": runner.CONTRACT_SHA256,
            "machine": runner.EXACT_MACHINE,
            "rootDisk": {"attachedReadOnly": True},
            "vsock": {"port": 4050, "handshakeComplete": True},
        }
        result = runner.make_result(
            mode="boot",
            images_before=before,
            images_after=before,
            host_receipt=receipt,
            console_sha256="77" * 32,
        )
        self.assertEqual(result["status"], "MAC4-AUTHENTICATED-CHANNEL-PASS")
        self.assertTrue(result["channelAuthenticated"])
        self.assertTrue(result["imagesUnchanged"])
        self.assertFalse(result["nodeConnected"])
        self.assertFalse(result["activationAllowed"])

        changed = [dict(row) for row in before]
        changed[2]["sha256"] = "99" * 32
        refused = runner.make_result(
            mode="boot",
            images_before=before,
            images_after=changed,
            host_receipt=receipt,
            console_sha256="77" * 32,
        )
        self.assertEqual(refused["status"], "MAC4-AUTHENTICATED-CHANNEL-FAIL")

    def test_preflight_never_claims_a_live_channel(self):
        result = runner.make_result(
            mode="preflight",
            images_before=images(),
            images_after=images(),
            host_receipt={
                "schema": "boole.native-shadow.mac4-authenticated-channel-run.v1",
                "outcome": "dry-run-configuration-valid",
                "dryRun": True,
            },
            console_sha256="88" * 32,
        )
        self.assertEqual(result["status"], "MAC4-CHANNEL-PREFLIGHT-PASS")
        self.assertFalse(result["channelAuthenticated"])
        self.assertEqual(result["machinesStarted"], 0)


if __name__ == "__main__":
    unittest.main()
