import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import types
import unittest
from unittest import mock

from scripts import native_shadow_closed_local_mac_readiness_arm64_v1 as subject
from scripts import native_shadow_mac3_guest_evidence_protocol_arm64_v2 as protocol


class ClosedLocalMacReadinessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.images = {}
        rows = []
        for name, payload in (
            ("guest-kernel", b"kernel"),
            ("guest-initrd", b"initrd"),
            ("guest-root-disk", b"root-disk"),
        ):
            path = self.root / name
            path.write_bytes(payload)
            self.images[name] = path
            rows.append(
                {
                    "name": name,
                    "sha256": hashlib.sha256(payload).hexdigest(),
                    "sizeBytes": len(payload),
                }
            )
        self.comparison = self.root / "comparison.json"
        self.comparison.write_text(
            json.dumps(
                {
                    "activationAllowed": False,
                    "artifactClass": "DISPOSABLE-DEVELOPMENT",
                    "bootVerified": False,
                    "outputs": rows,
                    "productionRelease": False,
                    "schema": (
                        "boole.native-shadow.closed-local-image-replica-comparison."
                        "arm64.v1"
                    ),
                    "status": "TWO-REPLICAS-BYTE-IDENTICAL",
                }
            ),
            encoding="utf-8",
        )

    def tearDown(self):
        self.temp.cleanup()

    def test_comparison_receipt_binds_all_three_exact_files(self):
        bound = subject.bind_images(self.comparison, self.images)
        self.assertEqual([row["name"] for row in bound], list(subject.IMAGE_NAMES))
        self.assertEqual(
            bound[2]["path"], str(self.images["guest-root-disk"].resolve())
        )

        self.images["guest-root-disk"].write_bytes(b"root-fisk")
        with self.assertRaisesRegex(ValueError, "guest-root-disk digest differs"):
            subject.bind_images(self.comparison, self.images)

    def test_receipt_cannot_smuggle_production_or_activation_authority(self):
        raw = json.loads(self.comparison.read_text(encoding="utf-8"))
        for field in ("activationAllowed", "bootVerified", "productionRelease"):
            changed = dict(raw)
            changed[field] = True
            self.comparison.write_text(json.dumps(changed), encoding="utf-8")
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                subject.bind_images(self.comparison, self.images)

    def test_symlinked_image_is_refused_before_it_can_be_resolved(self):
        target = self.images["guest-kernel"]
        link = self.root / "kernel-link"
        link.symlink_to(target)
        linked = dict(self.images)
        linked["guest-kernel"] = link
        with self.assertRaisesRegex(ValueError, "not one regular image file"):
            subject.bind_images(self.comparison, linked)

    def test_isolated_cli_can_bootstrap_without_python_site_paths(self):
        completed = subprocess.run(
            [
                sys.executable,
                "-I",
                "-S",
                str(subject.__file__),
                "--help",
            ],
            cwd=subject.REPO,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("{preflight,boot}", completed.stdout)

    def test_swift_compile_is_bound_to_sdk_target_and_private_cache(self):
        argv = subject.swiftc_argv(
            pathlib.Path("/tool/swiftc"),
            pathlib.Path("/sdk/MacOSX15.4.sdk"),
            pathlib.Path("/work/cache"),
            pathlib.Path("/work/host"),
        )
        self.assertEqual(argv[0], "/tool/swiftc")
        self.assertEqual(argv[argv.index("-sdk") + 1], "/sdk/MacOSX15.4.sdk")
        self.assertEqual(argv[argv.index("-target") + 1], "arm64-apple-macos14.0")
        self.assertEqual(
            argv[argv.index("-module-cache-path") + 1], "/work/cache"
        )
        self.assertIn(str(subject.HOST_STOP_STATE_SOURCE), argv)
        self.assertEqual(argv[argv.index("-o") - 1], str(subject.HOST_SOURCE))

    def test_phase_runner_times_out_a_real_sleeping_subprocess(self):
        started = time.monotonic()
        with self.assertRaisesRegex(ValueError, "compile timed out"):
            subject._run(
                [sys.executable, "-c", "import time; time.sleep(10)"],
                timeout=0.05,
                phase="compile",
            )
        self.assertLess(time.monotonic() - started, 2)

    def test_phase_runner_kills_a_timed_out_child_process_group(self):
        escaped_child_marker = self.root / "escaped-child"
        child_program = (
            "import pathlib, time; time.sleep(1); "
            f"pathlib.Path({str(escaped_child_marker)!r}).write_text('escaped')"
        )
        parent_program = (
            "import subprocess, sys, time; "
            f"subprocess.Popen([sys.executable, '-c', {child_program!r}]); "
            "time.sleep(10)"
        )
        with self.assertRaisesRegex(ValueError, "compile timed out"):
            subject._run(
                [sys.executable, "-c", parent_program],
                timeout=0.05,
                phase="compile",
            )
        time.sleep(1.2)
        self.assertFalse(
            escaped_child_marker.exists(),
            "timeout must terminate subprocess descendants as well as the parent",
        )

    def test_boot_budget_includes_only_a_bounded_shutdown_grace(self):
        self.assertEqual(
            subject.boot_subprocess_timeout(60),
            105,
        )
        self.assertGreaterEqual(
            subject.BOOT_SUBPROCESS_GRACE_SECONDS,
            5 + 15 + 15,
            "the parent must allow the host's request, guest, and forced-stop waits",
        )

    def test_boot_phase_failure_cannot_write_a_success_result(self):
        result = self.root / "result.json"
        args = types.SimpleNamespace(
            comparison=self.comparison,
            kernel=self.images["guest-kernel"],
            initrd=self.images["guest-initrd"],
            root_disk=self.images["guest-root-disk"],
            work=self.root / "work",
            result=result,
            timeout=1,
            swiftc="swiftc",
            sdk=self.root,
            codesign="codesign",
            mode="boot",
        )

        calls = []

        def phase_runner(argv, *, timeout, phase):
            calls.append((argv, timeout, phase))
            if phase == "dry-run":
                console = pathlib.Path(argv[argv.index("--console") + 1])
                receipt = pathlib.Path(argv[argv.index("--receipt") + 1])
                console.write_text("", encoding="utf-8")
                receipt.write_text(
                    json.dumps(
                        {
                            "outcome": "dry-run-configuration-valid",
                            "dryRun": True,
                        }
                    ),
                    encoding="utf-8",
                )
            if phase == "boot":
                raise ValueError("boot timed out after %s seconds" % timeout)

        with (
            mock.patch.object(subject.platform, "system", return_value="Darwin"),
            mock.patch.object(subject.platform, "machine", return_value="arm64"),
            mock.patch.object(subject.platform, "mac_ver", return_value=("14.0", (), "")),
            mock.patch.object(subject, "_run", side_effect=phase_runner),
        ):
            with self.assertRaisesRegex(ValueError, "boot timed out"):
                subject.execute(args)
        self.assertFalse(result.exists(), "a timed-out boot must not record readiness success")
        compile_argv, _, compile_phase = calls[0]
        self.assertEqual(compile_phase, "compile")
        self.assertIn(str(subject.HOST_STOP_STATE_SOURCE), compile_argv)
        self.assertEqual(
            pathlib.Path(compile_argv[compile_argv.index("-o") - 1]).resolve(),
            (args.work / "main.swift").resolve(),
        )

    def test_exact_guest_evidence_and_closed_host_receipt_are_readiness_green(self):
        transcript = "\n".join(
            [
                protocol.format_record(
                    "launcher-executable",
                    {
                        "path": subject.LAUNCHER_GUEST_PATH,
                        "sha256": subject.LAUNCHER_SHA256,
                    },
                ),
                protocol.format_record(
                    "launcher-prerequisites",
                    {
                        "prerequisites": [
                            {"name": name, "resolved": True}
                            for name in protocol.EXACT_PREREQUISITES
                        ]
                    },
                ),
                protocol.format_record(
                    "supervisor-privilege", protocol.EXACT_SUPERVISOR
                ),
                protocol.format_record(
                    "readiness", {"failedUnits": [], "ready": True}
                ),
            ]
        )
        receipt = {
            "dryRun": False,
            "machine": {
                "cpuCount": 2,
                "memoryBytes": 2 * 1024 * 1024 * 1024,
                "networkDevices": 0,
                "sharedDirectories": 0,
                "socketDevices": 0,
                "storageDevices": 1,
                "serialPorts": 1,
            },
            "outcome": "stopped-at-timeout",
            "stopConfirmed": True,
            "rootDisk": {"attachedReadOnly": True},
            "schema": "boole.native-shadow.mac3-closed-local-boot-run.v1",
        }
        assessed = subject.assess_readiness(transcript, receipt)
        self.assertTrue(assessed["ready"])
        self.assertEqual(set(assessed["guestEvidence"]), set(protocol.RECORDS))
        self.assertTrue(all(row["met"] for row in assessed["guestEvidence"].values()))

    def test_missing_guest_record_is_a_failed_readiness_not_a_waiver(self):
        transcript = protocol.format_record(
            "readiness", {"failedUnits": [], "ready": True}
        )
        receipt = {
            "dryRun": False,
            "machine": subject.EXACT_MACHINE,
            "outcome": "stopped-at-timeout",
            "rootDisk": {"attachedReadOnly": True},
            "schema": "boole.native-shadow.mac3-closed-local-boot-run.v1",
        }
        assessed = subject.assess_readiness(transcript, receipt)
        self.assertFalse(assessed["ready"])
        self.assertFalse(assessed["guestEvidence"]["launcher-executable"]["met"])

    def test_success_shaped_receipt_requires_positive_stop_confirmation(self):
        receipt = {
            "dryRun": False,
            "machine": subject.EXACT_MACHINE,
            "outcome": "stopped-at-timeout",
            "rootDisk": {"attachedReadOnly": True},
            "schema": "boole.native-shadow.mac3-closed-local-boot-run.v1",
        }
        for stop_confirmation in (None, False, "true", 1):
            candidate = dict(receipt)
            if stop_confirmation is not None:
                candidate["stopConfirmed"] = stop_confirmation
            with self.subTest(stop_confirmation=stop_confirmation):
                met, detail = subject._host_receipt_matches(candidate)
                self.assertFalse(met)
                self.assertIn("stop", detail)

    def test_result_is_development_only_even_when_readiness_passes(self):
        result = subject.make_result(
            mode="boot",
            images_before=[{"name": name} for name in subject.IMAGE_NAMES],
            images_after=[{"name": name} for name in subject.IMAGE_NAMES],
            host_receipt={"outcome": "stopped-at-timeout"},
            assessment={"ready": True, "guestEvidence": {}},
            transcript_sha256="0" * 64,
        )
        self.assertEqual(result["status"], "CLOSED-LOCAL-MAC-READINESS-PASS")
        self.assertFalse(result["activationAllowed"])
        self.assertFalse(result["productionRelease"])
        self.assertFalse(result["publicMining"])
        self.assertFalse(result["rewardReady"])
        self.assertFalse(result["testnetClaim"])


if __name__ == "__main__":
    unittest.main()
