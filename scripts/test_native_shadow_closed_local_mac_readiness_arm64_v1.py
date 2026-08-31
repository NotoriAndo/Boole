import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

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
