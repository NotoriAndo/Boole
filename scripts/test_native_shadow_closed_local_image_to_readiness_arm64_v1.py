#!/usr/bin/env python3
"""Behavior tests for the reversible closed-local successor image path."""

from __future__ import annotations

import hashlib
import pathlib
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_closed_local_image_to_readiness_arm64_v1 as dev
from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4
from scripts import native_shadow_successor_produce_phase_arm64_v5 as sealed


class FakeBackend:
    def __init__(self) -> None:
        self.calls: list[str] = []

    def prepare(self, request):
        self.calls.append("prepare")
        return sealed.PreparedProduction(
            measurement={"entries": 17_677},
            build_receipt={"source": "fake"},
            state={},
        )

    def extract_kernel(self, request, prepared):
        self.calls.append("extract-kernel")
        raw = b"kernel"
        (request.outputs / "guest-kernel").write_bytes(raw)
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "kernel": {
                "name": "guest-kernel",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
        }

    def build_initrd(self, request, prepared):
        self.calls.append("build-initrd")
        return b"initrd"

    def build_root_disk(self, request, prepared):
        self.calls.append("build-root-disk")
        raw = b"root-disk"
        (request.outputs / "guest-root-disk").write_bytes(raw)
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "image": {
                "name": "guest-root-disk",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
        }

    def verify_images(self, request, prepared, kernel, initrd, root_disk):
        self.calls.append("verify-images")
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "guestBootVerified": False,
            "passed": True,
        }

    def readback(self, repository_root, outputs, chain):
        self.calls.append("readback")
        raw = (outputs / "guest-root-disk").read_bytes()
        return {
            "activationAllowed": False,
            "bootableClaim": False,
            "guestBootVerified": False,
            "image": {
                "name": "guest-root-disk",
                "sha256": hashlib.sha256(raw).hexdigest(),
                "sizeBytes": len(raw),
            },
            "mayEnterQualification": True,
            "qualifiedForReplicaComparison": True,
            "status": sealed.READBACK_PASS_STATUS,
        }


def fake_chain():
    identity = sealed.FileIdentity("record.json", "0" * 64, 1)
    return types.SimpleNamespace(
        fresh_rehearsal={"measurement": {"entries": 17_677}},
        fingerprint={"status": sealed.F7_STATUS},
        identities={"P4": identity, "R3": identity, "F7": identity},
        import_identities=(),
        output_names=sealed.OUTPUT_NAMES,
    )


class ClosedLocalImageBehaviorTests(unittest.TestCase):
    def test_cli_bootstraps_its_repository_import_under_isolated_python(self):
        completed = subprocess.run(
            [
                sys.executable,
                "-I",
                "-S",
                str(
                    REPOSITORY_ROOT
                    / "scripts/native_shadow_closed_local_image_to_readiness_arm64_v1.py"
                ),
                "--help",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def test_development_chain_accepts_f7_without_creating_or_requiring_a7(self):
        self.assertFalse((REPOSITORY_ROOT / sealed.A7_PATH).exists())
        chain = dev.verify_development_generation_chain(REPOSITORY_ROOT)
        self.assertEqual(chain.fingerprint["status"], sealed.F7_STATUS)
        self.assertNotIn("A7", chain.identities)
        with self.assertRaisesRegex(
            sealed.SuccessorProduceV5Error, "production-authority-arm64-v7"
        ):
            sealed.verify_generation_chain(REPOSITORY_ROOT)

    def test_preflight_prepares_the_real_staging_shape_without_creating_images(self):
        backend = FakeBackend()
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            store = root / "cas"
            scratch = root / "scratch"
            outputs = root / "outputs"
            store.mkdir()
            scratch.mkdir()
            with mock.patch.object(
                dev, "verify_development_generation_chain", return_value=fake_chain()
            ):
                report = dev.preflight(
                    repository_root=REPOSITORY_ROOT,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=pathlib.Path("/usr/bin/gpgv"),
                    zstd=pathlib.Path("/usr/bin/zstd"),
                    launcher=pathlib.Path("launcher"),
                    backend=backend,
                )
            self.assertEqual(backend.calls, ["prepare"])
            self.assertFalse(outputs.exists())
            self.assertEqual(report["status"], "READY-NO-IMAGE-CREATED")
            self.assertFalse(report["authorisations"]["imageProductionAuthorised"])

    def test_build_uses_the_image_backend_but_emits_no_production_or_boot_claim(self):
        backend = FakeBackend()
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            store = root / "cas"
            scratch = root / "scratch"
            outputs = root / "outputs"
            result = root / "result.json"
            store.mkdir()
            scratch.mkdir()
            with mock.patch.object(
                dev, "verify_development_generation_chain", return_value=fake_chain()
            ):
                report = dev.build(
                    repository_root=REPOSITORY_ROOT,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    result=result,
                    gpgv=pathlib.Path("/usr/bin/gpgv"),
                    zstd=pathlib.Path("/usr/bin/zstd"),
                    launcher=pathlib.Path("launcher"),
                    run_label="unit-test",
                    backend=backend,
                )

            self.assertEqual(
                backend.calls,
                [
                    "prepare",
                    "extract-kernel",
                    "build-initrd",
                    "build-root-disk",
                    "verify-images",
                    "readback",
                ],
            )
            self.assertEqual(
                sorted(path.name for path in outputs.iterdir()),
                ["guest-initrd", "guest-kernel", "guest-root-disk"],
            )
            self.assertTrue(result.is_file())
            self.assertEqual(report["status"], "CLOSED-LOCAL-IMAGE-VERIFIED")
            self.assertEqual(report["artifactClass"], "DISPOSABLE-DEVELOPMENT")
            self.assertFalse(report["claims"]["bootVerified"])
            self.assertFalse(report["claims"]["productionRelease"])
            self.assertFalse(report["authorisations"]["activationAllowed"])
            self.assertFalse(report["authorisations"]["imageProductionAuthorised"])
            self.assertNotIn("authoritySha256", report)
            self.assertNotIn("attemptId", report)
            self.assertFalse((outputs / sealed.CONSUMED_MARKER_NAME).exists())

    def test_development_readback_sets_kernel_autoclear_and_keeps_explicit_detach(self):
        calls = []

        class Delegate:
            def setup_loop(self, image):
                calls.append(("setup", image.descriptor))
                return "/dev/loop7"

            def mount(self, device, mountpoint):
                calls.append(("mount", device, mountpoint))

            def read_tree(self, mountpoint):
                return {"mountpoint": str(mountpoint)}

            def unmount(self, mountpoint):
                calls.append(("unmount", mountpoint))

            def detach_loop(self, device):
                calls.append(("detach", device))

        module = types.SimpleNamespace(HostReadbackEffects=Delegate)
        effects = dev.DevelopmentAutoclearReadbackEffects(
            module,
            autoclear_setter=lambda device: calls.append(("autoclear", device)),
        )
        image = types.SimpleNamespace(descriptor=17)

        self.assertEqual(effects.setup_loop(image), "/dev/loop7")
        self.assertEqual(
            calls[:2], [("setup", 17), ("autoclear", "/dev/loop7")]
        )
        effects.detach_loop("/dev/loop7")
        self.assertEqual(calls[-1], ("detach", "/dev/loop7"))

    def test_explicit_detach_accepts_the_exact_already_autocleared_result(self):
        class ReadbackV3Error(RuntimeError):
            pass

        class Delegate:
            def setup_loop(self, _image):
                return "/dev/loop7"

            def detach_loop(self, device):
                raise ReadbackV3Error(
                    "/usr/sbin/losetup failed: losetup: %s: detach failed: "
                    "No such device or address" % device
                )

        module = types.SimpleNamespace(
            HostReadbackEffects=Delegate,
            ReadbackV3Error=ReadbackV3Error,
        )
        effects = dev.DevelopmentAutoclearReadbackEffects(
            module,
            autoclear_setter=lambda _device: None,
        )

        self.assertEqual(effects.setup_loop(types.SimpleNamespace()), "/dev/loop7")
        effects.detach_loop("/dev/loop7")

    def test_explicit_detach_keeps_every_other_cleanup_error_fatal(self):
        class ReadbackV3Error(RuntimeError):
            pass

        class Delegate:
            def setup_loop(self, _image):
                return "/dev/loop7"

            def detach_loop(self, _device):
                raise ReadbackV3Error(
                    "/usr/sbin/losetup failed: permission denied"
                )

        module = types.SimpleNamespace(
            HostReadbackEffects=Delegate,
            ReadbackV3Error=ReadbackV3Error,
        )
        effects = dev.DevelopmentAutoclearReadbackEffects(
            module,
            autoclear_setter=lambda _device: None,
        )

        self.assertEqual(effects.setup_loop(types.SimpleNamespace()), "/dev/loop7")
        with self.assertRaisesRegex(ReadbackV3Error, "permission denied"):
            effects.detach_loop("/dev/loop7")

    def test_autoclear_ioctl_sets_the_kernel_flag_on_the_loop_device(self):
        calls = []

        def opener(path, flags):
            calls.append(("open", path, flags))
            return 19

        def ioctl(descriptor, request, value, mutate=False):
            calls.append(("ioctl", descriptor, request, mutate))
            if request == dev.LOOP_GET_STATUS64:
                struct.pack_into("=I", value, dev.LOOP_FLAGS_OFFSET, 1)
                return 0
            self.assertEqual(request, dev.LOOP_SET_STATUS64)
            self.assertEqual(
                struct.unpack_from("=I", value, dev.LOOP_FLAGS_OFFSET)[0],
                1 | dev.LO_FLAGS_AUTOCLEAR,
            )
            return 0

        dev._set_loop_autoclear(
            "/dev/loop7", opener=opener, closer=lambda fd: calls.append(("close", fd)), ioctl=ioctl
        )
        self.assertEqual(calls[-1], ("close", 19))

    def test_development_backend_scopes_the_compatible_effects_override(self):
        backend = dev._development_backend()
        self.assertIsInstance(backend, dev.DevelopmentRepositoryImageBackend)
        original = sealed.AutoclearReadbackEffects
        with mock.patch.object(
            sealed.RepositoryImageBackend,
            "readback",
            side_effect=lambda *_args: sealed.AutoclearReadbackEffects,
        ):
            observed = backend.readback(REPOSITORY_ROOT, pathlib.Path("outputs"), fake_chain())
        self.assertIs(observed, dev.DevelopmentAutoclearReadbackEffects)
        self.assertIs(sealed.AutoclearReadbackEffects, original)

    def test_development_prepare_makes_the_installed_authority_directory_read_only(self):
        namespace = builder_v4.materialize_staging_tree.__globals__["_IMPL"]
        original = namespace["_ensure_parents"]
        observed = {}

        def prepare(_backend, _request):
            entries = {
                "usr/share/boole/native-shadow/registry-v1.json": {
                    "path": "usr/share/boole/native-shadow/registry-v1.json",
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"{}\n",
                }
            }
            namespace["_ensure_parents"](entries)
            observed.update(entries["usr/share/boole/native-shadow"])
            return "prepared"

        with mock.patch.object(
            sealed.RepositoryImageBackend,
            "prepare",
            autospec=True,
            side_effect=prepare,
        ):
            self.assertEqual(dev._development_backend().prepare(object()), "prepared")

        self.assertEqual(observed["kind"], "directory")
        self.assertEqual(observed["mode"], 0o555)
        self.assertEqual((observed["uid"], observed["gid"]), (0, 0))
        self.assertIs(namespace["_ensure_parents"], original)

    def test_development_readback_rejects_the_boot_observed_0755_authority_directory(self):
        class Delegate:
            def read_tree(self, _mountpoint):
                return {
                    "/usr/share/boole/native-shadow": {
                        "kind": "directory",
                        "mode": 0o755,
                        "uid": 0,
                        "gid": 0,
                    }
                }

        module = types.SimpleNamespace(HostReadbackEffects=Delegate)
        effects = dev.DevelopmentAutoclearReadbackEffects(module)
        with self.assertRaisesRegex(
            dev.ClosedLocalImageError,
            "authority directory must be root:root mode 0555",
        ):
            effects.read_tree(pathlib.Path("mounted-root"))

    def test_development_readback_accepts_the_exact_0555_authority_directory(self):
        expected = {
            "/usr/share/boole/native-shadow": {
                "kind": "directory",
                "mode": 0o555,
                "uid": 0,
                "gid": 0,
            }
        }

        class Delegate:
            def read_tree(self, _mountpoint):
                return expected

        module = types.SimpleNamespace(HostReadbackEffects=Delegate)
        effects = dev.DevelopmentAutoclearReadbackEffects(module)
        self.assertEqual(effects.read_tree(pathlib.Path("mounted-root")), expected)


if __name__ == "__main__":
    unittest.main()
