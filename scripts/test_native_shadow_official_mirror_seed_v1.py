#!/usr/bin/env python3
"""Behavior tests for seeding frozen package bytes from Ubuntu mirrors."""

from __future__ import annotations

import hashlib
import importlib.util
import os
import pathlib
import re
import tempfile
import unittest
import urllib.error
import urllib.request
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
CI = ROOT / ".github/workflows/ci.yml"
CLOSED_LOCAL = (
    ROOT / ".github/workflows/native-shadow-closed-local-image-readiness-arm64.yml"
)
X86_REPLAY = ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
ARM64_REPLAY = ROOT / "scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh"
MIRROR_SITE = (
    ROOT
    / "scripts/native_shadow_official_mirror_python_v1"
    / "sitecustomize.py"
)

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

    def test_frozen_runtime_consumer_reuses_only_a_verified_cas_object(self):
        module = self.require_module()

        class FakeAcquirer:
            class AcquisitionError(RuntimeError):
                pass

            def __init__(self, verified: bool):
                self.verified = verified
                self.network_calls = 0

            def _verified_cas_artifact(self, cas, spec):
                if not self.verified:
                    raise self.AcquisitionError("absent or invalid")
                return b"verified"

            def _cas_path(self, cas, digest):
                return pathlib.Path(cas) / "sha256" / digest

            def original(self, cas, spec, allowed_hosts):
                self.network_calls += 1
                return pathlib.Path(cas) / "downloaded"

        spec = {"sha256": "a" * 64, "sizeBytes": 8}
        cached = FakeAcquirer(True)
        self.assertEqual(
            module.cas_first_fetch(
                cached, cached.original, pathlib.Path("cas"), spec, ["snapshot.ubuntu.com"]
            ),
            pathlib.Path("cas/sha256") / ("a" * 64),
        )
        self.assertEqual(cached.network_calls, 0)

        absent = FakeAcquirer(False)
        self.assertEqual(
            module.cas_first_fetch(
                absent, absent.original, pathlib.Path("cas"), spec, ["snapshot.ubuntu.com"]
            ),
            pathlib.Path("cas/downloaded"),
        )
        self.assertEqual(absent.network_calls, 1)

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

    def test_development_and_required_lanes_use_verified_mirrors_without_changing_frozen_replay(self):
        module_name = "native_shadow_official_mirror_seed_v1.py"
        closed = CLOSED_LOCAL.read_text(encoding="utf-8")
        ci = CI.read_text(encoding="utf-8")
        self.assertIn(f"{module_name} boot", closed)
        self.assertLess(closed.index(f"{module_name} boot"), closed.index("ci_payload_acquire"))
        self.assertIn(f"{module_name} boot", ci)
        self.assertLess(ci.index(f"{module_name} boot"), ci.index("ci_payload_acquire"))
        self.assertTrue(MIRROR_SITE.is_file())
        self.assertIn("native_shadow_official_mirror_python_v1", ci)
        self.assertIn("BOOLE_UBUNTU_MIRROR_ARCH=amd64", ci)
        self.assertIn("BOOLE_UBUNTU_MIRROR_ARCH=arm64", ci)
        self.assertEqual(
            hashlib.sha256(X86_REPLAY.read_bytes()).hexdigest(),
            "d04bd92de2b5d2ba86cd2fe0d9990bf106fe94be7237bb23b55b7c30bd1aaea4",
        )
        self.assertEqual(
            hashlib.sha256(ARM64_REPLAY.read_bytes()).hexdigest(),
            "5b4fbde81a538d68fd01e96dcb5e9c02c76628dda75035d2f392a82ef3bdb68d",
        )

    def test_transport_adapter_preserves_the_frozen_snapshot_identity(self):
        self.assertTrue(MIRROR_SITE.is_file())
        spec = importlib.util.spec_from_file_location("boole_mirror_site", MIRROR_SITE)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        with mock.patch.dict(os.environ, {}, clear=True):
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)

        original_url = (
            "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
            "pool/main/t/test/test_arm64.deb"
        )
        calls = []

        class Response:
            status = 200

            def geturl(self):
                return "https://ports.ubuntu.com/ubuntu-ports/pool/main/t/test/test_arm64.deb"

            def read(self, *_args):
                return b"sealed"

            def close(self):
                pass

        def original_open(_opener, url, *args, **kwargs):
            calls.append((url, args, kwargs))
            return Response()

        adapted = module.adapted_open(original_open, "arm64")
        response = adapted(object(), original_url, timeout=60)
        mirror_request = calls[0][0]
        self.assertIsInstance(mirror_request, urllib.request.Request)
        self.assertEqual(
            mirror_request.full_url, module.mirror_url(original_url, "arm64")
        )
        self.assertEqual(
            mirror_request.get_header("User-agent"),
            "boole-official-mirror-seed-v1",
        )
        self.assertEqual(mirror_request.get_header("Accept-encoding"), "identity")
        self.assertEqual(response.geturl(), original_url)
        self.assertEqual(response.read(), b"sealed")

    def test_transport_adapter_falls_back_to_the_frozen_snapshot_when_the_mirror_is_unavailable(self):
        self.assertTrue(MIRROR_SITE.is_file())
        spec = importlib.util.spec_from_file_location("boole_mirror_site", MIRROR_SITE)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        with mock.patch.dict(os.environ, {}, clear=True):
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)

        original_url = (
            "https://snapshot.ubuntu.com/ubuntu/20240425T160000Z/"
            "pool/main/u/ubuntu-keyring/ubuntu-keyring_2023.11.28.1_all.deb"
        )
        calls = []

        class Response:
            status = 200

            def geturl(self):
                return original_url

            def read(self, *_args):
                return b"sealed"

            def close(self):
                pass

        def original_open(_opener, url, *args, **kwargs):
            calls.append((url, args, kwargs))
            if len(calls) == 1:
                raise urllib.error.URLError("official mirror unavailable")
            return Response()

        adapted = module.adapted_open(original_open, "amd64")
        response = adapted(object(), original_url, timeout=60)
        mirror_request = calls[0][0]
        self.assertIsInstance(mirror_request, urllib.request.Request)
        self.assertEqual(
            [mirror_request.full_url, calls[1][0]],
            [module.mirror_url(original_url, "amd64"), original_url],
        )
        self.assertEqual(calls[0][1:], calls[1][1:])
        self.assertEqual(response.geturl(), original_url)
        self.assertEqual(response.read(), b"sealed")


if __name__ == "__main__":
    unittest.main()
