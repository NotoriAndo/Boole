#!/usr/bin/env python3
"""RED tests for the shell driver that runs the arm64 produce phase.

The driver cannot run on the machine that writes these tests: it needs Linux,
an aarch64 processor, root, and systemd.  So what is pinned here is everything
about it that can be read rather than run -- that it refuses the wrong host,
that it asks the producer authority for the transient unit rather than spelling
the isolation out itself, that the network-touching half is somebody else's job,
and that it names none of the frozen values a second time.

That last one is the point of the whole file.  Two independent jobs produce
byte-identical images only if neither of them decides anything, and a shell
script is the easiest place for a decision to hide.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import unittest

from scripts import native_shadow_boot_image_produce_arm64_v1 as producer
from scripts import native_shadow_boot_produce_phase_arm64_v1 as phase
from scripts import native_shadow_boot_root_disk_readback_arm64_v1 as readback_module


REPO = pathlib.Path(__file__).resolve().parents[1]
DRIVER = REPO / "scripts/native-shadow-boot-produce-arm64.sh"
DRIVER_TEXT = DRIVER.read_text(encoding="utf-8") if DRIVER.exists() else ""
WORKFLOW = REPO / ".github/workflows/native-shadow-boot-produce-arm64.yml"
WORKFLOW_TEXT = WORKFLOW.read_text(encoding="utf-8") if WORKFLOW.exists() else ""
PRODUCER_AUTHORITY = REPO / (
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)


class ShapeTests(unittest.TestCase):
    def test_the_driver_exists_and_is_executable(self) -> None:
        self.assertTrue(DRIVER.is_file(), f"{DRIVER} is absent")
        self.assertTrue(os.access(DRIVER, os.X_OK), f"{DRIVER} is not executable")

    def test_it_is_bash_and_stops_at_the_first_failure(self) -> None:
        """Without `set -e` a failed step is followed by the next one anyway."""

        self.assertTrue(DRIVER_TEXT.startswith("#!/usr/bin/env bash\n"))
        self.assertIn("set -euo pipefail\n", DRIVER_TEXT)

    def test_it_parses(self) -> None:
        completed = subprocess.run(
            ["bash", "-n", str(DRIVER)],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(
            completed.returncode, 0, completed.stderr.decode(errors="replace")
        )


class HostTests(unittest.TestCase):
    """The image answers to the host that wrote it, so the host is checked."""

    def test_it_refuses_a_host_that_is_not_linux_aarch64(self) -> None:
        self.assertIn("uname -s", DRIVER_TEXT)
        self.assertIn("Linux", DRIVER_TEXT)
        self.assertIn("uname -m", DRIVER_TEXT)
        self.assertIn("aarch64", DRIVER_TEXT)

    def test_it_refuses_a_run_that_is_not_root(self) -> None:
        """`mke2fs -d` copies the staged owner, so a non-root run is a different image."""

        self.assertIn("EUID", DRIVER_TEXT)

    def test_every_command_it_uses_is_checked_for_first(self) -> None:
        self.assertIn("command -v", DRIVER_TEXT)
        for name in ("mount", "python3", "systemd-run", "umount"):
            self.assertRegex(DRIVER_TEXT, rf"\b{name}\b")


class IsolationTests(unittest.TestCase):
    """The sealed properties belong to the producer authority, not to bash."""

    def test_the_transient_unit_is_asked_for_rather_than_written_here(self) -> None:
        self.assertIn("isolation-argv", DRIVER_TEXT)

    def test_no_sealed_property_is_restated_in_the_driver(self) -> None:
        for entry in producer.isolation_properties(producer.load_authority(REPO)):
            name = entry.partition("=")[0]
            self.assertNotIn(name, DRIVER_TEXT, f"{entry} is restated in the driver")

    def test_systemd_run_is_never_invoked_by_hand(self) -> None:
        """A hand-written invocation is one the authority cannot constrain."""

        self.assertNotIn("systemd-run --", DRIVER_TEXT)
        self.assertNotIn("systemd-run \\", DRIVER_TEXT)

    def test_the_two_writable_holes_are_the_scratch_and_the_outputs(self) -> None:
        holes = re.findall(r'--read-write-path "([^"]+)"', DRIVER_TEXT)
        self.assertEqual(holes, ['$scratch', '$outputs'])


class OfflineTests(unittest.TestCase):
    """The network half is the caller's; nothing here reaches for it."""

    def test_the_driver_fetches_nothing(self) -> None:
        for forbidden in ("apt-get", "cargo", "curl", "git clone", "pip install", "wget"):
            self.assertNotIn(forbidden, DRIVER_TEXT)

    def test_the_launcher_is_an_input_and_not_a_build(self) -> None:
        self.assertIn("--launcher", DRIVER_TEXT)
        self.assertNotIn("launcher_emit", DRIVER_TEXT)
        self.assertNotIn("launcher_build", DRIVER_TEXT)

    def test_the_verified_payloads_are_an_input_and_not_a_fetch(self) -> None:
        self.assertIn("--cas", DRIVER_TEXT)
        self.assertNotIn("payload_acquire", DRIVER_TEXT)
        self.assertNotIn("rustdist_acquire", DRIVER_TEXT)


class StagingTests(unittest.TestCase):
    """`mke2fs -d` reads the staging tree with readdir and never sorts it."""

    def test_the_staging_tree_sits_on_the_filesystem_the_plan_names(self) -> None:
        root_disk = phase.root_disk
        self.assertIn(f"-t {root_disk.STAGING_FILESYSTEM}", DRIVER_TEXT)

    def test_the_staging_mount_is_taken_down_again(self) -> None:
        self.assertIn("umount", DRIVER_TEXT)
        self.assertIn("trap", DRIVER_TEXT)


class RestatementTests(unittest.TestCase):
    """Every frozen value has one home, and it is not this script."""

    def test_the_output_names_are_not_restated_here(self) -> None:
        for name in phase.output_names():
            self.assertNotIn(name, DRIVER_TEXT)

    def test_no_sealed_record_is_named_here(self) -> None:
        self.assertNotIn("native/containment/", DRIVER_TEXT)

    def test_the_image_size_is_not_chosen_here(self) -> None:
        for token in ("count=", "seek=", "truncate", "fallocate", "dd "):
            self.assertNotIn(token, DRIVER_TEXT)


class ReadbackTests(unittest.TestCase):
    """Writing the image and reading it back are two stages, in that order."""

    def test_the_produced_image_is_read_back_before_it_is_reported(self) -> None:
        readback = DRIVER_TEXT.find("root_disk_readback")
        manifest = DRIVER_TEXT.find('produce_arm64_v1.py" manifest')
        self.assertNotEqual(readback, -1, "the driver never reads the image back")
        self.assertNotEqual(manifest, -1, "the driver never prints the manifest")
        self.assertLess(readback, manifest, "the manifest is printed before the check")

    def test_the_image_is_read_back_outside_the_sealed_unit(self) -> None:
        """The unit seals private devices, and a loop mount is exactly a device."""

        unit = DRIVER_TEXT.find('"${isolation[@]}"')
        self.assertNotEqual(unit, -1)
        self.assertLess(unit, DRIVER_TEXT.find("root_disk_readback"))

    def test_the_checks_are_not_restated_in_the_driver(self) -> None:
        for identifier in readback_module.REQUIRED_CHECKS:
            self.assertNotIn(identifier, DRIVER_TEXT)


class BoundaryTests(unittest.TestCase):
    def test_producing_the_files_is_not_claimed_to_be_a_boot(self) -> None:
        self.assertNotIn("bootable", DRIVER_TEXT)
        self.assertNotIn("boots", DRIVER_TEXT)


class WorkflowTests(unittest.TestCase):
    """The two independent runs, and the comparison that may not be softened."""

    def authority(self) -> dict:
        return json.loads(PRODUCER_AUTHORITY.read_text(encoding="utf-8"))

    def test_the_workflow_exists_and_runs_the_driver(self) -> None:
        self.assertTrue(WORKFLOW.is_file(), f"{WORKFLOW} is absent")
        self.assertTrue(
            DRIVER.name in WORKFLOW_TEXT, f"{WORKFLOW.name} never runs {DRIVER.name}"
        )

    def test_it_runs_on_the_runner_the_authority_sealed(self) -> None:
        runner = self.authority()["buildIsolation"]["runner"]
        self.assertIn(f"runs-on: {runner}", WORKFLOW_TEXT)

    def test_two_independent_jobs_come_from_one_definition(self) -> None:
        """A copied job definition is a second thing that can drift."""

        self.assertIs(self.authority()["buildIsolation"]["separateJobs"], True)
        self.assertIn("matrix:", WORKFLOW_TEXT)
        self.assertIn("replica: [1, 2]", WORKFLOW_TEXT)
        self.assertEqual(WORKFLOW_TEXT.count(f"./scripts/{DRIVER.name}"), 1)

    def test_the_sealed_acquisition_record_is_reproved_and_never_rewritten(self) -> None:
        """The acquirer writes its result once, so a fresh checkout already has one.

        Setting the sealed record aside and requiring the runner's own fetch to
        reproduce it byte for byte is the only reading of that file that is both
        runnable twice and honest: the seal in the repository is never touched,
        and a difference stops the run instead of becoming the new seal.
        """

        sealed = (
            "native/containment/"
            "native-shadow-boot-rustdist-acquisition-result-arm64-v1.json"
        )
        self.assertTrue(sealed in WORKFLOW_TEXT, "the sealed record is never re-proved")
        self.assertIn("git diff --exit-code", WORKFLOW_TEXT)
        for forbidden in ("git add", "git checkout", "git stash"):
            self.assertNotIn(forbidden, WORKFLOW_TEXT)

    def test_nothing_is_already_in_the_store_when_the_rust_archives_are_fetched(
        self,
    ) -> None:
        """The sealed record says three fetched and no store hits, so it goes first."""

        rust = WORKFLOW_TEXT.find("rustdist_acquire")
        payloads = WORKFLOW_TEXT.find("payload_acquire")
        self.assertNotEqual(rust, -1)
        self.assertNotEqual(payloads, -1)
        self.assertLess(rust, payloads)

    def test_the_comparison_runs_and_cannot_be_softened(self) -> None:
        self.assertIn("compare", WORKFLOW_TEXT)
        for forbidden in ("continue-on-error", "|| true", "if: always()"):
            self.assertNotIn(forbidden, WORKFLOW_TEXT)

    def test_the_second_run_still_happens_when_the_first_one_fails(self) -> None:
        """Two independent runs are two pieces of evidence, not one retry."""

        self.assertIn("fail-fast: false", WORKFLOW_TEXT)

    def test_every_action_is_pinned_to_an_immutable_commit(self) -> None:
        uses = re.findall(r"^\s*uses:\s*(\S+)", WORKFLOW_TEXT, re.MULTILINE)
        self.assertTrue(uses, "the workflow uses no actions at all")
        for reference in uses:
            self.assertRegex(reference, r"@[0-9a-f]{40}$", reference)

    def test_the_token_can_only_read(self) -> None:
        self.assertIn("permissions:\n  contents: read\n", WORKFLOW_TEXT)

    def test_the_images_are_kept_as_artifacts_and_never_published(self) -> None:
        self.assertIn("upload-artifact", WORKFLOW_TEXT)
        for forbidden in ("gh release", "action-gh-release", "git push", "git commit"):
            self.assertNotIn(forbidden, WORKFLOW_TEXT)

    def test_it_is_asked_for_rather_than_run_on_every_change(self) -> None:
        """Two multi-gigabyte images per push is not what this evidence is for."""

        self.assertIn("workflow_dispatch:", WORKFLOW_TEXT)
        self.assertNotIn("pull_request:", WORKFLOW_TEXT)
        self.assertNotIn("push:", WORKFLOW_TEXT)


if __name__ == "__main__":
    unittest.main()
