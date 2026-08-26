#!/usr/bin/env python3
"""Tests for the arm64 produce-phase isolation, manifest and byte comparison."""

from __future__ import annotations

import json
import pathlib
import unittest

from scripts import native_shadow_boot_image_produce_arm64_v1 as mod


AUTHORITY_PATH = pathlib.Path(
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)


def authority() -> dict:
    return json.loads(AUTHORITY_PATH.read_text(encoding="utf-8"))


class AuthorityTests(unittest.TestCase):
    def test_the_frozen_producer_authority_loads_and_is_the_pinned_release(self) -> None:
        document = mod.load_authority(pathlib.Path("."))
        self.assertEqual(document["release"], authority()["release"])

    def test_the_runner_must_be_the_arm64_linux_one_the_authority_names(self) -> None:
        document = authority()
        document["buildIsolation"]["runner"] = "ubuntu-latest"
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(document, command=["true"])

    def test_the_two_jobs_must_stay_separate(self) -> None:
        document = authority()
        document["buildIsolation"]["separateJobs"] = False
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(document, command=["true"])

    def test_a_produce_phase_allowed_to_reach_the_network_is_refused(self) -> None:
        document = authority()
        for phase in document["buildIsolation"]["phases"]:
            if phase["name"] == "produce":
                phase["networkAllowed"] = True
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(document, command=["true"])


class IsolationTests(unittest.TestCase):
    def test_the_properties_come_from_the_authority_rather_than_a_second_copy(self) -> None:
        """A restated list can drift from the sealed one and the drift is invisible."""

        document = authority()
        document["buildIsolation"]["systemdRunProperties"].append("MemoryMax=4G")
        argv = mod.isolation_argv(document, command=["true"])
        self.assertIn("--property=MemoryMax=4G", argv)

    def test_every_sealed_property_reaches_the_argv(self) -> None:
        document = authority()
        argv = mod.isolation_argv(document, command=["true"])
        for prop in document["buildIsolation"]["systemdRunProperties"]:
            self.assertIn(f"--property={prop}", argv)

    def test_dropping_the_network_property_from_the_document_is_refused(self) -> None:
        """Deriving from the document must not mean inheriting a weakened one."""

        document = authority()
        document["buildIsolation"]["systemdRunProperties"] = [
            prop
            for prop in document["buildIsolation"]["systemdRunProperties"]
            if not prop.startswith("PrivateNetwork=")
        ]
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(document, command=["true"])

    def test_turning_the_network_property_off_is_refused(self) -> None:
        document = authority()
        document["buildIsolation"]["systemdRunProperties"] = [
            "PrivateNetwork=no" if prop.startswith("PrivateNetwork=") else prop
            for prop in document["buildIsolation"]["systemdRunProperties"]
        ]
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(document, command=["true"])

    def test_the_command_is_separated_from_the_options(self) -> None:
        argv = mod.isolation_argv(authority(), command=["python3", "-c", "pass"])
        self.assertEqual(argv[-3:], ["python3", "-c", "pass"])
        self.assertEqual(argv[-4], "--")

    def test_an_empty_command_is_refused(self) -> None:
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(authority(), command=[])

    def test_the_unit_waits_and_reports_its_own_exit_status(self) -> None:
        """A fire-and-forget unit would let a failed produce read as a pass."""

        argv = mod.isolation_argv(authority(), command=["true"])
        self.assertIn("--wait", argv)
        self.assertIn("--pipe", argv)
        self.assertIn("--collect", argv)


class WritablePathTests(unittest.TestCase):
    """`ProtectSystem=strict` needs an explicit hole for the outputs."""

    def test_a_named_output_directory_becomes_a_read_write_path(self) -> None:
        argv = mod.isolation_argv(
            authority(), command=["true"], read_write_paths=[pathlib.Path("/tmp/out")]
        )
        self.assertIn("--property=ReadWritePaths=/tmp/out", argv)

    def test_widening_the_hole_to_the_whole_filesystem_is_refused(self) -> None:
        for candidate in ("/", "/usr", "/etc", "/usr/lib"):
            with self.subTest(candidate=candidate):
                with self.assertRaises(mod.ProduceError):
                    mod.isolation_argv(
                        authority(),
                        command=["true"],
                        read_write_paths=[pathlib.Path(candidate)],
                    )

    def test_a_relative_read_write_path_is_refused(self) -> None:
        with self.assertRaises(mod.ProduceError):
            mod.isolation_argv(
                authority(), command=["true"], read_write_paths=[pathlib.Path("out")]
            )


class ManifestTests(unittest.TestCase):
    def test_the_manifest_is_sha256sum_text_sorted_by_name(self) -> None:
        text = mod.manifest_text({"b": "11" * 32, "a": "22" * 32})
        self.assertEqual(text, f"{'22' * 32}  a\n{'11' * 32}  b\n")

    def test_the_manifest_round_trips(self) -> None:
        entries = {"guest-initrd": "33" * 32, "guest-kernel": "44" * 32}
        self.assertEqual(mod.parse_manifest(mod.manifest_text(entries)), entries)

    def test_a_manifest_line_without_the_two_space_separator_is_refused(self) -> None:
        with self.assertRaises(mod.ProduceError):
            mod.parse_manifest(f"{'55' * 32} guest-kernel\n")

    def test_a_manifest_digest_that_is_not_lowercase_hex_is_refused(self) -> None:
        with self.assertRaises(mod.ProduceError):
            mod.parse_manifest(f"{'AA' * 32}  guest-kernel\n")

    def test_a_duplicate_name_is_refused_rather_than_last_one_wins(self) -> None:
        with self.assertRaises(mod.ProduceError):
            mod.parse_manifest(f"{'66' * 32}  guest-kernel\n{'77' * 32}  guest-kernel\n")


class OutputTests(unittest.TestCase):
    def setUp(self) -> None:
        self.names = mod.output_names(authority())

    def test_the_output_names_and_their_order_come_from_the_authority(self) -> None:
        self.assertEqual(self.names, tuple(row["name"] for row in authority()["outputs"]))
        self.assertEqual(self.names, ("guest-kernel", "guest-initrd", "guest-root-disk"))

    def test_a_missing_output_stops_with_the_named_abort_condition(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as raw:
            directory = pathlib.Path(raw)
            for name in self.names[:-1]:
                (directory / name).write_bytes(b"x")
            with self.assertRaises(mod.ProduceError) as caught:
                mod.manifest_from_directory(directory, self.names)
        self.assertIn("output-missing-or-empty", str(caught.exception))

    def test_a_zero_byte_output_is_a_failure_rather_than_a_pass(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as raw:
            directory = pathlib.Path(raw)
            for name in self.names:
                (directory / name).write_bytes(b"" if name == "guest-initrd" else b"x")
            with self.assertRaises(mod.ProduceError) as caught:
                mod.manifest_from_directory(directory, self.names)
        self.assertIn("output-missing-or-empty", str(caught.exception))

    def test_a_complete_directory_produces_one_digest_per_output(self) -> None:
        import hashlib
        import tempfile

        with tempfile.TemporaryDirectory() as raw:
            directory = pathlib.Path(raw)
            for index, name in enumerate(self.names):
                (directory / name).write_bytes(bytes([index]) * 16)
            entries = mod.manifest_from_directory(directory, self.names)
        self.assertEqual(sorted(entries), sorted(self.names))
        self.assertEqual(entries["guest-kernel"], hashlib.sha256(b"\x00" * 16).hexdigest())


class ComparisonTests(unittest.TestCase):
    """Condition 6: differing bytes stop the run; they never get reconciled."""

    def test_identical_manifests_pass(self) -> None:
        entries = {"guest-kernel": "88" * 32}
        mod.compare_manifests(entries, dict(entries))

    def test_differing_digests_stop_and_report_both(self) -> None:
        left = {"guest-kernel": "88" * 32}
        right = {"guest-kernel": "99" * 32}
        with self.assertRaises(mod.ProduceError) as caught:
            mod.compare_manifests(left, right)
        message = str(caught.exception)
        self.assertIn("independent-builds-differ", message)
        self.assertIn("88" * 32, message)
        self.assertIn("99" * 32, message)

    def test_an_output_present_in_only_one_job_stops_the_run(self) -> None:
        with self.assertRaises(mod.ProduceError) as caught:
            mod.compare_manifests({"guest-kernel": "88" * 32}, {})
        self.assertIn("guest-kernel", str(caught.exception))

    def test_comparison_takes_no_knob_that_could_force_a_match(self) -> None:
        import inspect

        signature = inspect.signature(mod.compare_manifests)
        self.assertEqual(len(signature.parameters), 2)
        for parameter in signature.parameters.values():
            self.assertIs(parameter.default, inspect.Parameter.empty)


class BoundaryTests(unittest.TestCase):
    def test_isolating_a_command_is_not_producing_or_booting_an_image(self) -> None:
        self.assertIs(mod.BOOTABLE_CLAIM, False)
        self.assertIs(mod.ACTIVATION_ALLOWED, False)
        self.assertIs(mod.GUEST_IMAGE_BUILT, False)

    def test_the_mismatch_action_is_the_one_the_authority_sealed(self) -> None:
        self.assertEqual(
            mod.MISMATCH_ACTION, authority()["determinism"]["mismatchAction"]
        )
        self.assertEqual(mod.MISMATCH_ACTION, "report-the-difference-never-force-a-match")

    def test_nothing_here_runs_the_command_it_builds(self) -> None:
        source = pathlib.Path(mod.__file__).read_text(encoding="utf-8")
        for forbidden in ("subprocess.run", "subprocess.Popen", "os.execv", "os.system"):
            self.assertNotIn(forbidden, source)


if __name__ == "__main__":
    unittest.main()
