#!/usr/bin/env python3
"""Behavior tests for seeding frozen package bytes from Ubuntu mirrors."""

from __future__ import annotations

import hashlib
import pathlib
import re
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CI = ROOT / ".github/workflows/ci.yml"
CLOSED_LOCAL = (
    ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml"
)
X86_REPLAY = ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
ARM64_REPLAY = ROOT / "scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh"

try:
    from scripts import native_shadow_official_mirror_seed_v1 as seed
except ImportError:
    seed = None


class OfficialMirrorSeedTests(unittest.TestCase):
    def require_module(self):
        self.assertIsNotNone(seed, "official mirror seed module is absent")
        return seed

    def test_snapshot_pool_paths_map_only_to_the_official_architecture_mirror(self):
        module = self.require_module()
        source = (
            "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
            "pool/main/c/cryptsetup/libcryptsetup12_2.7.0-1ubuntu4_arm64.deb"
        )
        self.assertEqual(
            module.mirror_url(source, "arm64"),
            "https://ports.ubuntu.com/ubuntu-ports/pool/main/c/cryptsetup/"
            "libcryptsetup12_2.7.0-1ubuntu4_arm64.deb",
        )
        self.assertIn("archive.ubuntu.com/ubuntu/pool/", module.mirror_url(source, "amd64"))
        self.assertEqual(
            module.mirror_url(
                "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
                "dists/noble/main/binary-arm64/Packages.xz",
                "arm64",
            ),
            "https://ports.ubuntu.com/ubuntu-ports/dists/noble/main/"
            "binary-arm64/Packages.xz",
        )
        self.assertEqual(
            module.mirror_url(
                "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
                "dists/noble/InRelease",
                "amd64",
            ),
            "https://archive.ubuntu.com/ubuntu/dists/noble/InRelease",
        )
        for forbidden in (
            "http://snapshot.ubuntu.com/ubuntu/x/pool/a.deb",
            "https://example.com/ubuntu/x/pool/a.deb",
            "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/dists/noble/Release.gpg",
        ):
            with self.assertRaises(ValueError):
                module.mirror_url(forbidden, "arm64")
        with self.assertRaises(ValueError):
            module.mirror_url(source, "ppc64el")
        with self.assertRaises(ValueError):
            module.mirror_url(
                "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
                "dists/noble/main/binary-arm64/Packages.xz",
                "amd64",
            )

    def test_verified_bytes_enter_the_existing_content_addressed_store(self):
        module = self.require_module()
        body = b"exact frozen package bytes"
        spec = {
            "artifactId": "deb-test",
            "sha256": hashlib.sha256(body).hexdigest(),
            "sizeBytes": len(body),
            "url": "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
            "pool/main/t/test/test_arm64.deb",
        }
        calls = []

        def stream(url: str, expected_size: int):
            calls.append((url, expected_size))
            yield body

        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            first = module.seed_specs(
                cas=cas, specs=[spec], architecture="arm64", stream_factory=stream
            )
            second = module.seed_specs(
                cas=cas,
                specs=[spec],
                architecture="arm64",
                stream_factory=lambda *_: self.fail("verified CAS hit used the network"),
            )
            stored = cas / "sha256" / spec["sha256"]
            self.assertEqual(stored.read_bytes(), body)
            self.assertEqual(first, {"fetched": 1, "reused": 0})
            self.assertEqual(second, {"fetched": 0, "reused": 1})
            self.assertEqual(len(calls), 1)
            self.assertTrue(calls[0][0].startswith("https://ports.ubuntu.com/"))

    def test_wrong_mirror_bytes_never_enter_the_store(self):
        module = self.require_module()
        body = b"expected"
        spec = {
            "artifactId": "deb-test",
            "sha256": hashlib.sha256(body).hexdigest(),
            "sizeBytes": len(body),
            "url": "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
            "pool/main/t/test/test_amd64.deb",
        }
        with tempfile.TemporaryDirectory() as scratch:
            cas = pathlib.Path(scratch) / "cas"
            with self.assertRaises(Exception):
                module.seed_specs(
                    cas=cas,
                    specs=[spec],
                    architecture="amd64",
                    stream_factory=lambda *_: [b"tampered"],
                )
            self.assertFalse((cas / "sha256" / spec["sha256"]).exists())

    def test_tracked_boot_and_runtime_sets_are_covered_without_metadata(self):
        module = self.require_module()
        boot = module.boot_specs()
        self.assertEqual(len(boot), 195)
        self.assertEqual(sum("/pool/" in row["url"] for row in boot), 193)
        self.assertEqual(sum("/dists/" in row["url"] for row in boot), 2)
        self.assertEqual(len({row["sha256"] for row in boot}), 195)

        x86_metadata = module.runtime_metadata_specs(
            ROOT / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json"
        )
        arm_metadata = module.runtime_metadata_specs(
            ROOT
            / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json"
        )
        self.assertEqual(len(x86_metadata), 2)
        self.assertEqual(len(arm_metadata), 2)
        self.assertEqual(x86_metadata[0]["sha256"], arm_metadata[0]["sha256"])

        x86 = module.runtime_package_specs(
            ROOT / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-v1.json",
            ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-v2.json",
        )
        arm = module.runtime_package_specs(
            ROOT / "native/containment/native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json",
            ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-arm64-v1.json",
        )
        self.assertEqual(len(x86), 57)
        self.assertEqual(len(arm), 56)

    def test_development_and_required_lanes_seed_before_frozen_consumers(self):
        module_name = "native_shadow_official_mirror_seed_v1.py"
        closed = CLOSED_LOCAL.read_text(encoding="utf-8")
        ci = CI.read_text(encoding="utf-8")
        x86 = X86_REPLAY.read_text(encoding="utf-8")
        arm = ARM64_REPLAY.read_text(encoding="utf-8")
        self.assertIn(f"{module_name} boot", closed)
        self.assertLess(closed.index(f"{module_name} boot"), closed.index("ci_payload_acquire"))
        self.assertIn(f"{module_name} boot", ci)
        self.assertLess(ci.index(f"{module_name} boot"), ci.index("ci_payload_acquire"))
        for text in (x86, arm):
            self.assertRegex(text, re.escape(module_name) + r'"?\s+runtime-bootstrap')
            self.assertRegex(text, re.escape(module_name) + r'"?\s+runtime-metadata')
            self.assertRegex(text, re.escape(module_name) + r'"?\s+runtime-packages')
            self.assertLess(text.index("runtime-metadata"), text.index("fetch-metadata"))
            self.assertLess(text.index("runtime-bootstrap"), text.index("fetch-metadata"))
            self.assertLess(text.index("runtime-packages"), text.index("fetch-payloads"))


if __name__ == "__main__":
    unittest.main()
