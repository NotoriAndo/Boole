#!/usr/bin/env python3
"""Behavior tests for the reversible closed-local successor image path."""

from __future__ import annotations

import hashlib
import json
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


SECOND_MAC_RESULT = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v2.json"
)
THIRD_MAC_RESULT = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v3.json"
)
FOURTH_MAC_RESULT = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v4.json"
)
FIFTH_MAC_RESULT = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v5.json"
)


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


def exact_development_readback_tree():
    tree = {
        path: {
            "kind": "directory",
            "mode": 0o555,
            "uid": 0,
            "gid": 0,
        }
        for path in (*dev.AUTHORITY_MOUNTED_PATHS, *dev.TOOLCHAIN_MOUNTED_PATHS)
    }
    for material in dev.DEVELOPMENT_REPLAY_MATERIALS:
        tree["/" + material.staging_path] = {
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "sha256": material.sha256,
        }
    for path in dev.DEVELOPMENT_SYSTEMD_MASK_PATHS:
        tree["/" + path] = {
            "kind": "symlink",
            "mode": 0o777,
            "uid": 0,
            "gid": 0,
            "target": dev.DEVELOPMENT_SYSTEMD_MASK_TARGET,
        }
    tree["/" + dev.MAC4_MODULE_LOAD_STAGING_PATH] = {
        "kind": "file",
        "mode": 0o444,
        "uid": 0,
        "gid": 0,
        "sha256": hashlib.sha256(dev.MAC4_MODULE_LOAD_BYTES).hexdigest(),
    }
    for path, raw in dev._static_test_module_index_entries().items():
        tree["/" + path] = {
            "kind": "file",
            "mode": 0o444,
            "uid": 0,
            "gid": 0,
            "sha256": hashlib.sha256(raw["raw"]).hexdigest(),
        }
    return tree


class ClosedLocalImageBehaviorTests(unittest.TestCase):
    def test_host_depmod_generator_materializes_modules_and_returns_all_required_indexes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            depmod = root / "depmod"
            depmod.write_bytes(b"tool")
            depmod.chmod(0o555)
            scratch = root / "scratch"
            scratch.mkdir()
            object_paths = tuple(
                dev.MAC4_MODULE_DIRECTORY + "/" + relative
                for relative in dev.MAC4_REQUIRED_MODULE_OBJECTS
            )
            entries = {
                dev.MAC4_MODULE_DIRECTORY: {
                    "path": dev.MAC4_MODULE_DIRECTORY,
                    "kind": "directory",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                },
                **{
                    path: {
                        "path": path,
                        "kind": "file",
                        "mode": 0o444,
                        "uid": 0,
                        "gid": 0,
                        "raw": b"module:" + path.encode(),
                    }
                    for path in object_paths
                },
            }

            def runner(command, **_kwargs):
                basedir = pathlib.Path(command[command.index("-b") + 1])
                module_root = (
                    basedir / "lib/modules" / dev.MAC4_KERNEL_RELEASE
                )
                for path in object_paths:
                    relative = path.removeprefix(dev.MAC4_MODULE_DIRECTORY + "/")
                    self.assertTrue((module_root / relative).is_file())
                for name in dev.MAC4_REQUIRED_MODULE_INDEX_NAMES:
                    (module_root / name).write_bytes((name + "\n").encode())
                return types.SimpleNamespace(returncode=0, stdout="", stderr="")

            generator = dev.HostDepmodModuleIndexGenerator(
                depmod=depmod, scratch=scratch, runner=runner
            )
            generated = generator(entries)

        self.assertEqual(
            set(generated),
            {
                dev.MAC4_MODULE_DIRECTORY + "/" + name
                for name in dev.MAC4_REQUIRED_MODULE_INDEX_NAMES
            },
        )
        self.assertTrue(all(row["mode"] == 0o444 for row in generated.values()))

    def test_host_depmod_generator_preserves_the_depmod_multicall_name(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            kmod = root / "kmod"
            kmod.write_bytes(b"tool")
            kmod.chmod(0o555)
            depmod = root / "depmod"
            depmod.symlink_to("kmod")
            generator = dev.HostDepmodModuleIndexGenerator(
                depmod=depmod,
                scratch=root,
                runner=lambda *_args, **_kwargs: None,
            )
        self.assertEqual(generator._depmod, depmod)

    def test_module_index_generator_rejects_a_missing_vsock_object(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            depmod = root / "depmod"
            depmod.write_bytes(b"tool")
            depmod.chmod(0o555)
            scratch = root / "scratch"
            scratch.mkdir()
            generator = dev.HostDepmodModuleIndexGenerator(
                depmod=depmod,
                scratch=scratch,
                runner=lambda *_args, **_kwargs: self.fail("depmod must not run"),
            )
            with self.assertRaisesRegex(
                dev.ClosedLocalImageError,
                "sealed kernel module tree is absent|required vsock module object is absent",
            ):
                generator({})

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
        backend._module_metadata = dev._module_metadata_identities(
            dev._static_test_module_index_entries()
        )
        original = sealed.AutoclearReadbackEffects
        with mock.patch.object(
            sealed.RepositoryImageBackend,
            "readback",
            side_effect=lambda *_args: sealed.AutoclearReadbackEffects,
        ):
            observed = backend.readback(REPOSITORY_ROOT, pathlib.Path("outputs"), fake_chain())
        self.assertTrue(callable(observed))
        produced = observed(types.SimpleNamespace(HostReadbackEffects=lambda: object()))
        self.assertIsInstance(produced, dev.DevelopmentAutoclearReadbackEffects)
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
                },
                "opt/boole/native-checker-toolchain/bin/rustc": {
                    "path": "opt/boole/native-checker-toolchain/bin/rustc",
                    "kind": "file",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"rustc",
                },
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

    def test_development_prepare_makes_the_installed_toolchain_directories_read_only(self):
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
                },
                "opt/boole/native-checker-toolchain/bin/rustc": {
                    "path": "opt/boole/native-checker-toolchain/bin/rustc",
                    "kind": "file",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"rustc",
                }
            }
            namespace["_ensure_parents"](entries)
            for path in (
                "opt/boole/native-checker-toolchain",
                "opt/boole/native-checker-toolchain/bin",
            ):
                observed[path] = dict(entries[path])
            return "prepared"

        with mock.patch.object(
            sealed.RepositoryImageBackend,
            "prepare",
            autospec=True,
            side_effect=prepare,
        ):
            self.assertEqual(dev._development_backend().prepare(object()), "prepared")

        for path, row in observed.items():
            with self.subTest(path=path):
                self.assertEqual(row["kind"], "directory")
                self.assertEqual(row["mode"], 0o555)
                self.assertEqual((row["uid"], row["gid"]), (0, 0))
        self.assertIs(namespace["_ensure_parents"], original)

    def test_development_prepare_stages_the_complete_closed_local_replay_materials(self):
        namespace = builder_v4.materialize_staging_tree.__globals__["_IMPL"]
        original_assemble = namespace["_assemble_entries"]
        observed = {}

        def base_assemble(*_args, **_kwargs):
            entries = {
                "usr/share/boole/native-shadow/registry-v1.json": {
                    "path": "usr/share/boole/native-shadow/registry-v1.json",
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"{}\n",
                },
                "usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py": {
                    "path": "usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py",
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"checker",
                },
                "opt/boole/native-checker-toolchain/bin/rustc": {
                    "path": "opt/boole/native-checker-toolchain/bin/rustc",
                    "kind": "file",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"rustc",
                },
            }
            namespace["_ensure_parents"](entries)
            return entries

        def prepare(_backend, _request):
            entries = namespace["_assemble_entries"]()
            observed.update(entries)
            return "prepared"

        namespace["_assemble_entries"] = base_assemble
        try:
            with mock.patch.object(
                sealed.RepositoryImageBackend,
                "prepare",
                autospec=True,
                side_effect=prepare,
            ):
                self.assertEqual(dev._development_backend().prepare(object()), "prepared")
        finally:
            namespace["_assemble_entries"] = original_assemble

        expected = {
            "usr/share/boole/native-shadow/closed-local-replay-registry-overlay-v1.json": (
                "2962adef8d1aea9ba1c8466b8e014b71f1ec3c9555ce8b685d58ede6b631fe74",
                5_461,
            ),
            "usr/share/boole/native-shadow/closed-local-replay-grant-v1.json": (
                "bd5cd9fc87e5e47a23e6fa12844ec0c47bdb01ee34090cddff24568c18d7236f",
                4_548,
            ),
            "usr/share/boole/native-shadow/closed-local-replay-execution-authority-v1.json": (
                "d220d20b7adaa22357929729d2f0666a8c9cbe50ce8031f90539ba1309950c6b",
                2_106,
            ),
            "usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history/task.json": (
                "f25a8a6d92ac556937eaacbec6d12d9d09be675878eb7d942952b35838ee7c82",
                1_303,
            ),
            "usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history/anchor.rs": (
                "693f62acfa0626a0831c9133a26fcfc1dbb30922c1ab2036231c42a363cfd7fe",
                181,
            ),
        }
        for path, (digest, size) in expected.items():
            with self.subTest(path=path):
                row = observed[path]
                self.assertEqual(row["kind"], "file")
                self.assertEqual(row["mode"], 0o444)
                self.assertEqual((row["uid"], row["gid"]), (0, 0))
                self.assertEqual(len(row["raw"]), size)
                self.assertEqual(hashlib.sha256(row["raw"]).hexdigest(), digest)

        for path in (
            "usr/share/boole/native-shadow/checkers",
            "usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1",
            "usr/share/boole/native-shadow/fixtures",
            "usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history",
        ):
            with self.subTest(path=path):
                row = observed[path]
                self.assertEqual(row["kind"], "directory")
                self.assertEqual(row["mode"], 0o555)
                self.assertEqual((row["uid"], row["gid"]), (0, 0))

    def test_development_prepare_masks_units_the_closed_read_only_guest_cannot_use(self):
        namespace = builder_v4.materialize_staging_tree.__globals__["_IMPL"]
        original_assemble = namespace["_assemble_entries"]
        observed = {}

        def base_assemble(*_args, **_kwargs):
            entries = {
                "usr/share/boole/native-shadow/registry-v1.json": {
                    "path": "usr/share/boole/native-shadow/registry-v1.json",
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"{}\n",
                },
                "usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py": {
                    "path": "usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py",
                    "kind": "file",
                    "mode": 0o444,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"checker",
                },
                "opt/boole/native-checker-toolchain/bin/rustc": {
                    "path": "opt/boole/native-checker-toolchain/bin/rustc",
                    "kind": "file",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                    "raw": b"rustc",
                },
                "etc/systemd/system": {
                    "path": "etc/systemd/system",
                    "kind": "directory",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                },
            }
            namespace["_ensure_parents"](entries)
            return entries

        def prepare(_backend, _request):
            entries = namespace["_assemble_entries"]()
            observed.update(entries)
            return "prepared"

        namespace["_assemble_entries"] = base_assemble
        try:
            with mock.patch.object(
                sealed.RepositoryImageBackend,
                "prepare",
                autospec=True,
                side_effect=prepare,
            ):
                self.assertEqual(dev._development_backend().prepare(object()), "prepared")
        finally:
            namespace["_assemble_entries"] = original_assemble

        self.assertEqual(
            {
                path: {
                    key: observed[path][key]
                    for key in ("kind", "mode", "uid", "gid", "target")
                }
                for path in (
                    "etc/systemd/system/ldconfig.service",
                    "etc/systemd/system/getty-static.service",
                    "etc/systemd/system/getty@.service",
                    "etc/systemd/system/serial-getty@.service",
                )
            },
            {
                path: {
                    "kind": "symlink",
                    "mode": 0o777,
                    "uid": 0,
                    "gid": 0,
                    "target": "/dev/null",
                }
                for path in (
                    "etc/systemd/system/ldconfig.service",
                    "etc/systemd/system/getty-static.service",
                    "etc/systemd/system/getty@.service",
                    "etc/systemd/system/serial-getty@.service",
                )
            },
        )

    def test_development_measurement_preserves_the_sealed_base_and_counts_the_overlay(self):
        historical = {
            "usr/bin/example": {
                "path": "usr/bin/example",
                "kind": "file",
                "mode": 0o555,
                "uid": 0,
                "gid": 0,
                "raw": b"historical",
            }
        }
        full = dict(historical)
        full.update(dev._development_replay_entries(REPOSITORY_ROOT))
        full.update(dev._development_systemd_mask_entries())
        full.update(dev._development_mac4_entries(REPOSITORY_ROOT, b"relay"))
        full.update(dev._static_test_module_index_entries())
        for path in dev.DEVELOPMENT_DERIVED_DIRECTORY_PATHS:
            full[path] = {
                "path": path,
                "kind": "directory",
                "mode": 0o555,
                "uid": 0,
                "gid": 0,
            }
        expected_base = dev.staging_measure.builder_totals(historical)
        expected_full = dev.staging_measure.builder_totals(full)

        with mock.patch.object(
            builder_v4, "materialize_staging_tree", return_value=full
        ):
            prepared = dev._development_prepare_staging(
                validated={},
                repository_root=REPOSITORY_ROOT,
                artifact_store=pathlib.Path("cas"),
                launcher_binary=b"launcher",
                nested_tree={},
                preregistration={
                    "expectedPreflight": {"measurement": expected_base}
                },
            )

        self.assertEqual(prepared.entries, full)
        self.assertEqual(prepared.measurement, expected_full)
        self.assertEqual(
            prepared.measurement["entries"],
            expected_base["entries"]
            + len(dev.DEVELOPMENT_REPLAY_MATERIALS)
            + len(dev.DEVELOPMENT_SYSTEMD_MASK_PATHS)
            + len(dev.MAC4_OVERLAY_PATHS)
            + len(dev.MAC4_REQUIRED_MODULE_INDEX_NAMES)
            + 2,
        )

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

    def test_development_readback_rejects_a_missing_closed_local_replay_material(self):
        observed = {
            path: {
                "kind": "directory",
                "mode": 0o555,
                "uid": 0,
                "gid": 0,
            }
            for path in (
                "/usr/share/boole/native-shadow",
                "/usr/share/boole/native-shadow/checkers",
                "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1",
                "/usr/share/boole/native-shadow/fixtures",
                "/usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history",
                "/opt/boole/native-checker-toolchain",
                "/opt/boole/native-checker-toolchain/bin",
            )
        }

        class Delegate:
            def read_tree(self, _mountpoint):
                return observed

        module = types.SimpleNamespace(HostReadbackEffects=Delegate)
        effects = dev.DevelopmentAutoclearReadbackEffects(module)
        with self.assertRaisesRegex(
            dev.ClosedLocalImageError,
            "closed-local replay material",
        ):
            effects.read_tree(pathlib.Path("mounted-root"))

    def test_development_readback_accepts_the_exact_0555_authority_directory(self):
        expected = exact_development_readback_tree()

        class Delegate:
            def read_tree(self, _mountpoint):
                return expected

        module = types.SimpleNamespace(HostReadbackEffects=Delegate)
        effects = dev.DevelopmentAutoclearReadbackEffects(module)
        self.assertEqual(effects.read_tree(pathlib.Path("mounted-root")), expected)

    def test_development_readback_rejects_a_missing_vsock_module_index(self):
        exact = exact_development_readback_tree()
        missing = "/" + dev.MAC4_MODULE_DIRECTORY + "/modules.dep.bin"
        del exact[missing]

        class Delegate:
            def read_tree(self, _mountpoint):
                return exact

        expected = dev._module_metadata_identities(
            dev._static_test_module_index_entries()
        )
        effects = dev.DevelopmentAutoclearReadbackEffects(
            types.SimpleNamespace(HostReadbackEffects=Delegate),
            expected_module_metadata=expected,
        )
        with self.assertRaisesRegex(
            dev.ClosedLocalImageError, "MAC.4 module metadata differs"
        ):
            effects.read_tree(pathlib.Path("mounted-root"))

    def test_development_readback_rejects_a_missing_or_retargeted_systemd_mask(self):
        exact = exact_development_readback_tree()
        mounted = "/etc/systemd/system/ldconfig.service"
        for mutation in ("missing", "retargeted"):
            with self.subTest(mutation=mutation):
                observed = {name: dict(row) for name, row in exact.items()}
                if mutation == "missing":
                    del observed[mounted]
                else:
                    observed[mounted]["target"] = "/usr/bin/true"

                class Delegate:
                    def read_tree(self, _mountpoint):
                        return observed

                module = types.SimpleNamespace(HostReadbackEffects=Delegate)
                effects = dev.DevelopmentAutoclearReadbackEffects(module)
                with self.assertRaisesRegex(
                    dev.ClosedLocalImageError,
                    "closed-local systemd mask differs",
                ):
                    effects.read_tree(pathlib.Path("mounted-root"))

    def test_development_readback_rejects_boot_observed_writable_toolchain_directories(self):
        exact = exact_development_readback_tree()
        for path in (
            "/opt/boole/native-checker-toolchain",
            "/opt/boole/native-checker-toolchain/bin",
        ):
            with self.subTest(path=path):
                observed = {name: dict(row) for name, row in exact.items()}
                observed[path]["mode"] = 0o755

                class Delegate:
                    def read_tree(self, _mountpoint):
                        return observed

                module = types.SimpleNamespace(HostReadbackEffects=Delegate)
                effects = dev.DevelopmentAutoclearReadbackEffects(module)
                with self.assertRaisesRegex(
                    dev.ClosedLocalImageError,
                    "installed toolchain directory must be root:root mode 0555",
                ):
                    effects.read_tree(pathlib.Path("mounted-root"))

    def test_second_mac_observation_records_the_exact_fail_closed_boundary(self):
        document = json.loads(SECOND_MAC_RESULT.read_text(encoding="utf-8"))
        self.assertEqual(
            document["schema"],
            "boole.native-shadow.closed-local-image-mac-readiness-result.arm64.v2",
        )
        self.assertEqual(
            document["status"],
            "IMAGE-REPLICAS-GREEN-MAC-BOOT-REACHED-LAUNCHER-READINESS-FAILED",
        )
        self.assertEqual(document["imageBuild"]["runId"], 33458786844)
        self.assertEqual(
            document["imageBuild"]["comparisonStatus"],
            "TWO-REPLICAS-BYTE-IDENTICAL",
        )
        outputs = {
            row["name"]: (row["sha256"], row["sizeBytes"])
            for row in document["imageBuild"]["outputs"]
        }
        self.assertEqual(
            outputs,
            {
                "guest-kernel": (
                    "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336",
                    57_860_488,
                ),
                "guest-initrd": (
                    "62a914b2d6a160379884181df55586c974a475b972f27346214891f0ba26f883",
                    1_776_452_408,
                ),
                "guest-root-disk": (
                    "75c571bc1c53f85b7af6e7fab38344f78c41355f9d411d114ba906279cb5297a",
                    2_035_650_560,
                ),
            },
        )
        self.assertEqual(
            document["imageBuild"]["rawOutputBytesAcrossTwoReplicas"],
            2 * sum(size for _digest, size in outputs.values()),
        )
        self.assertEqual(document["macObservation"]["boot"]["attempts"], 1)
        self.assertEqual(
            document["macObservation"]["boot"]["observedFailure"]["path"],
            "/opt/boole/native-checker-toolchain",
        )
        self.assertEqual(
            document["macObservation"]["boot"]["observedFailure"]["reason"],
            "directory mode differs from fixed contract",
        )
        self.assertEqual(
            document["predecessor"]["sha256"],
            "2d8cd7c70e1c105da6eb657992c56fea6818ef4992e022f33a0ed22eef19042c",
        )
        self.assertTrue(document["macObservation"]["boot"]["imagesUnchanged"])
        self.assertFalse(any(document["boundaries"].values()))

    def test_third_mac_observation_records_the_missing_replay_material_and_fix(self):
        document = json.loads(THIRD_MAC_RESULT.read_text(encoding="utf-8"))
        self.assertEqual(
            document["schema"],
            "boole.native-shadow.closed-local-image-mac-readiness-result.arm64.v3",
        )
        self.assertEqual(
            document["status"],
            "IMAGE-REPLICAS-GREEN-MAC-BOOT-REACHED-LAUNCHER-REPLAY-MATERIAL-ABSENT",
        )
        self.assertEqual(document["imageBuild"]["runId"], 33466531840)
        self.assertEqual(
            document["imageBuild"]["comparisonStatus"],
            "TWO-REPLICAS-BYTE-IDENTICAL",
        )
        self.assertEqual(
            document["rootCause"]["firstMissingInstalledPath"],
            "/usr/share/boole/native-shadow/closed-local-replay-registry-overlay-v1.json",
        )
        self.assertEqual(
            len(document["correction"]["addedReplayMaterials"]), 5
        )
        self.assertEqual(
            document["correction"]["addedReplayMaterialBytes"], 13_599
        )
        self.assertTrue(document["correction"]["sealedBaseMeasurementPreserved"])
        self.assertEqual(document["macObservation"]["boot"]["attempts"], 1)
        self.assertFalse(document["macObservation"]["boot"]["readiness"])
        self.assertTrue(document["macObservation"]["boot"]["imagesUnchanged"])
        self.assertEqual(
            document["predecessor"],
            {
                "path": "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v2.json",
                "sha256": "11178786987241e207ebfeb574e2c0369778c40bcd6ad1491fefe99475d6b779",
                "sizeBytes": 5_638,
            },
        )
        self.assertFalse(any(document["boundaries"].values()))

    def test_fourth_mac_observation_records_only_headless_unit_failures_and_the_fix(self):
        document = json.loads(FOURTH_MAC_RESULT.read_text(encoding="utf-8"))
        self.assertEqual(
            document["schema"],
            "boole.native-shadow.closed-local-image-mac-readiness-result.arm64.v4",
        )
        self.assertEqual(
            document["status"],
            "IMAGE-REPLICAS-GREEN-MAC-BOOT-PREREQUISITES-MET-HEADLESS-UNITS-FAILED",
        )
        self.assertEqual(document["imageBuild"]["runId"], 33471902181)
        self.assertEqual(
            document["imageBuild"]["comparisonStatus"],
            "TWO-REPLICAS-BYTE-IDENTICAL",
        )
        outputs = {
            row["name"]: (row["sha256"], row["sizeBytes"])
            for row in document["imageBuild"]["outputs"]
        }
        self.assertEqual(
            outputs,
            {
                "guest-kernel": (
                    "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336",
                    57_860_488,
                ),
                "guest-initrd": (
                    "3857918bda74cdc460e958b3447667b10cdb0ce749d98df349c3b5ed34c3169e",
                    1_776_467_320,
                ),
                "guest-root-disk": (
                    "b669ab0f0f7d26b10b2f19fc8c8ca4cc1a1799df954c2de3e1232b0d09f344c6",
                    2_035_691_520,
                ),
            },
        )
        evidence = document["macObservation"]["boot"]["guestEvidence"]
        self.assertTrue(evidence["launcher-executable"])
        self.assertTrue(evidence["launcher-prerequisites"])
        self.assertTrue(evidence["supervisor-privilege"])
        self.assertFalse(evidence["readiness"])
        self.assertEqual(
            document["rootCause"]["failedUnits"],
            [
                "getty@tty2.service",
                "getty@tty3.service",
                "getty@tty4.service",
                "getty@tty5.service",
                "getty@tty6.service",
                "ldconfig.service",
                "serial-getty@hvc0.service",
            ],
        )
        self.assertEqual(
            document["correction"]["systemdMasks"],
            list(dev.DEVELOPMENT_SYSTEMD_MASK_PATHS),
        )
        self.assertEqual(document["macObservation"]["boot"]["attempts"], 1)
        self.assertFalse(document["macObservation"]["boot"]["retryPerformed"])
        self.assertTrue(document["macObservation"]["boot"]["imagesUnchanged"])
        self.assertEqual(
            document["predecessor"],
            {
                "path": "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v3.json",
                "sha256": "4e463d18e217f25ae8cf9877596e8a2a3304d461b2bc6ee8e49ed0d0dcb84723",
                "sizeBytes": 5_860,
            },
        )
        self.assertFalse(any(document["boundaries"].values()))

    def test_fifth_mac_observation_records_exact_closed_readiness_pass(self):
        document = json.loads(FIFTH_MAC_RESULT.read_text(encoding="utf-8"))
        self.assertEqual(
            document["schema"],
            "boole.native-shadow.closed-local-image-mac-readiness-result.arm64.v5",
        )
        self.assertEqual(
            document["status"],
            "IMAGE-REPLICAS-GREEN-MAC-CLOSED-READINESS-PASS",
        )
        self.assertEqual(document["imageBuild"]["runId"], 33485969541)
        self.assertEqual(
            document["imageBuild"]["headSha"],
            "0d437f226331a76636ef15fc9f033eb0a4ac2199",
        )
        self.assertEqual(
            document["imageBuild"]["comparisonStatus"],
            "TWO-REPLICAS-BYTE-IDENTICAL",
        )
        outputs = {
            row["name"]: (row["sha256"], row["sizeBytes"])
            for row in document["imageBuild"]["outputs"]
        }
        self.assertEqual(
            outputs,
            {
                "guest-kernel": (
                    "d29e317d66517190f6437b9b9bd2cedd26a424fe6da7b1a28451247a13fe1336",
                    57_860_488,
                ),
                "guest-initrd": (
                    "bf64b980e36643d21bab0b3e20e668b1c216d7ed0d1e8d5ea69ceba64ca71888",
                    1_776_467_964,
                ),
                "guest-root-disk": (
                    "dffd60d09803bc44882c2508398735b79fb9271d403a41846b9a87b6a24842fe",
                    2_035_691_520,
                ),
            },
        )
        self.assertEqual(
            document["imageBuild"]["rawOutputBytesAcrossTwoReplicas"],
            2 * sum(size for _digest, size in outputs.values()),
        )
        boot = document["macObservation"]["boot"]
        self.assertEqual(boot["attempts"], 1)
        self.assertFalse(boot["retryPerformed"])
        self.assertEqual(boot["status"], "CLOSED-LOCAL-MAC-READINESS-PASS")
        self.assertTrue(boot["imagesUnchanged"])
        self.assertTrue(boot["hostIsolationMet"])
        self.assertTrue(boot["readiness"])
        self.assertEqual(boot["failedUnits"], [])
        self.assertEqual(
            boot["guestEvidence"],
            {
                "launcher-executable": True,
                "launcher-prerequisites": True,
                "readiness": True,
                "supervisor-privilege": True,
            },
        )
        self.assertFalse(boot["submissionsObserved"])
        self.assertEqual(
            document["predecessor"],
            {
                "path": "native/containment/native-shadow-closed-local-image-mac-readiness-result-arm64-v4.json",
                "sha256": "f7db899f59bb391f6625e483d2686447845a62fcdc31fca90b8b648aab71bee5",
                "sizeBytes": 5_946,
            },
        )
        self.assertFalse(any(document["boundaries"].values()))


if __name__ == "__main__":
    unittest.main()
