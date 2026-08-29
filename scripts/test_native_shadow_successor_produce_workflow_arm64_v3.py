#!/usr/bin/env python3
"""Contract tests for the authority-zero launcher-v2 successor workflow."""

from __future__ import annotations

import json
import os
import pathlib
import re
import stat
import subprocess
import sys
import tempfile
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
PREREGISTRATION = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
IMPORT_CORRECTION = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)
PRODUCER = REPO / "scripts/native_shadow_successor_produce_phase_arm64_v3.py"
WRAPPER = REPO / "scripts/native-shadow-successor-produce-arm64-v3.sh"
WORKFLOW = REPO / ".github/workflows/native-shadow-successor-produce-arm64-v3.yml"
HISTORICAL_WRAPPER = "scripts/native-shadow-successor-produce-arm64.sh"
HISTORICAL_READBACK = "scripts/native_shadow_successor_root_disk_readback_arm64_v2.py"


def workflow_job(name: str) -> str:
    source = WORKFLOW.read_text(encoding="utf-8")
    marker = f"  {name}:\n"
    if marker not in source:
        raise AssertionError(f"workflow job is absent: {name}")
    body = source.split(marker, 1)[1]
    following = re.search(r"^  [a-zA-Z0-9_-]+:\n", body, re.MULTILINE)
    return body[: following.start()] if following else body


class GenerationEdgesTests(unittest.TestCase):
    def test_readback_edge_is_declared_but_unreachable_without_future_authority(self) -> None:
        record = json.loads(PREREGISTRATION.read_text(encoding="utf-8"))
        contract = record["futureGeneration"]["readbackV3Contract"]
        files = record["futureGeneration"]["files"]

        workflow = (REPO / files["workflow"]).read_text(encoding="utf-8")
        wrapper = (REPO / files["wrapper"]).read_text(encoding="utf-8")

        self.assertIn(files["wrapper"], workflow)
        self.assertIn(files["producer"], wrapper)
        self.assertIn(files["readback"], wrapper)
        self.assertEqual(wrapper.count("run_readback_v3()"), 1)
        self.assertNotIn("run_readback_v3 --", wrapper)
        self.assertNotIn(files["readback"], workflow)
        for historical in contract["forbiddenHistoricalCallees"]:
            self.assertNotIn(historical, workflow)
            self.assertNotIn(historical, wrapper)


class ProducerCliTests(unittest.TestCase):
    def test_producer_exposes_named_rehearsal_and_production_check_commands(self) -> None:
        completed = subprocess.run(
            ["python3", str(PRODUCER), "--help"],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("rehearsal", completed.stdout)
        self.assertIn("production-check", completed.stdout)

    def test_rehearsal_cli_names_only_the_inputs_needed_for_shared_assembly(self) -> None:
        completed = subprocess.run(
            ["python3", str(PRODUCER), "rehearsal", "--help"],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        for required in (
            "--cas",
            "--gpgv",
            "--zstd",
            "--launcher",
            "--scratch",
            "--result",
        ):
            self.assertIn(required, completed.stdout)
        for forbidden in (
            "--repository-root",
            "--artifact-store",
            "--outputs",
            "--attempt-marker",
            "--image",
        ):
            self.assertNotIn(forbidden, completed.stdout)

    def test_production_check_refuses_the_current_authority_zero_chain(self) -> None:
        completed = subprocess.run(
            [
                "python3",
                str(PRODUCER),
                "production-check",
            ],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("production", completed.stderr.lower())
        self.assertTrue(
            "future production chain is absent" in completed.stderr
            or "future production chain consumption is not implemented"
            in completed.stderr,
            completed.stderr,
        )


class WrapperContractTests(unittest.TestCase):
    def source(self) -> str:
        return WRAPPER.read_text(encoding="utf-8")

    def test_wrapper_is_valid_strict_bash_and_native_arm64_only(self) -> None:
        mode = WRAPPER.lstat().st_mode
        self.assertTrue(stat.S_ISREG(mode))
        self.assertFalse(WRAPPER.is_symlink())
        self.assertTrue(mode & stat.S_IXUSR)
        syntax = subprocess.run(
            ["bash", "-n", str(WRAPPER)],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(syntax.returncode, 0, syntax.stderr)
        source = self.source()
        self.assertIn("set -euo pipefail", source)
        self.assertRegex(source, r"uname -s.+Linux")
        self.assertRegex(source, r"uname -m.+(aarch64|arm64)")

    def test_wrapper_calls_named_producer_cli_and_only_v3_readback(self) -> None:
        source = self.source()
        self.assertIn('python3 -I -S "$PRODUCER" rehearsal', source)
        self.assertIn('python3 -I -S "$PRODUCER" production-check', source)
        self.assertIn('python3 -I -S "$READBACK"', source)
        self.assertIn("native_shadow_successor_root_disk_readback_arm64_v3.py", source)
        self.assertNotIn("native_shadow_successor_root_disk_readback_arm64_v2.py", source)
        self.assertNotIn("native-shadow-successor-produce-arm64.sh", source)

    def test_wrapper_rehashes_all_bound_helpers_before_any_repo_python_import(self) -> None:
        source = self.source()
        gate = source.index("verify_preregistered_bindings")
        first_producer = source.index('python3 -I -S "$PRODUCER"')
        self.assertLess(gate, first_producer)
        self.assertIn(
            "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
            source,
        )
        self.assertIn("20145", source)
        self.assertIn("len(rows) != 23", source)
        self.assertIn(
            "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
            source,
        )
        self.assertIn("10971", source)
        self.assertIn("len(added) != 18", source)
        self.assertIn("len(seen) != 41", source)
        self.assertIn("path.lstat()", source)
        self.assertIn("path.is_symlink()", source)
        self.assertIn("hashlib.sha256(raw).hexdigest()", source)

    def test_wrapper_python_is_isolated_from_ambient_startup_hooks(self) -> None:
        source = self.source()
        self.assertIn("python3 -I -S -c", source)
        self.assertIn('python3 -I -S "$PRODUCER"', source)
        self.assertIn('python3 -I -S "$READBACK"', source)

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            marker = root / "ambient-hook-ran"
            (root / "sitecustomize.py").write_text(
                "import os, pathlib\n"
                "pathlib.Path(os.environ['AMBIENT_HOOK_MARKER']).write_text('ran')\n",
                encoding="utf-8",
            )
            environment = dict(os.environ)
            environment["PYTHONPATH"] = str(root)
            environment["AMBIENT_HOOK_MARKER"] = str(marker)
            completed = subprocess.run(
                ["bash", str(WRAPPER), "--production"],
                cwd=REPO,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertFalse(marker.exists())
            self.assertIn("authority", completed.stderr.lower())

    def test_rehearsal_has_one_create_once_json_result_and_no_output_surface(self) -> None:
        source = self.source()
        self.assertIn('"$internal_result"', source)
        self.assertIn('ln -- "$internal_result" "$result"', source)
        self.assertIn('find "$scratch" -type f', source)
        self.assertNotIn("--outputs \"$", source)
        self.assertNotIn("ATTEMPT-" + "CONSUMED.json", source)
        for tool in ("mke2fs", "mkfs.ext4", "mkinitramfs", "qemu-img"):
            with self.subTest(tool=tool):
                self.assertNotIn(tool, source)

    def test_rehearsal_never_runs_repository_python_as_root(self) -> None:
        source = self.source()
        self.assertNotIn("${EUID}", source)
        self.assertNotIn("the rehearsal isolation must be installed as root", source)
        rehearsal = workflow_job("free-rehearsal")
        self.assertNotIn("sudo ./scripts/native-shadow-successor-produce", rehearsal)

    def test_recursive_cleanup_revalidates_the_private_mktemp_scope(self) -> None:
        source = self.source()
        cleanup = source[source.index("cleanup()") : source.index("trap cleanup EXIT")]
        self.assertIn("expected_scratch_prefix", source)
        self.assertIn("[[ $scratch == \"$expected_scratch_prefix\"* ]]", cleanup)
        self.assertLess(cleanup.index("expected_scratch_prefix"), cleanup.index("rm -rf"))

    def test_production_check_precedes_all_input_and_effect_work(self) -> None:
        source = self.source()
        check = source.index('python3 -I -S "$PRODUCER" production-check')
        for later in ('command -v', 'uname -s', '[[ -d $cas', 'mktemp -d'):
            with self.subTest(later=later):
                self.assertLess(check, source.index(later))

    def test_verify_only_mode_checks_bindings_without_host_or_output_effects(self) -> None:
        completed = subprocess.run(
            ["bash", str(WRAPPER), "--verify-bindings-only"],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn("bindings verified", completed.stderr.lower())

    def test_rehearsal_rejects_an_output_directory_argument_before_host_work(self) -> None:
        completed = subprocess.run(
            [
                "bash",
                str(WRAPPER),
                "--rehearsal-only",
                "--outputs",
                "/tmp/must-not-exist",
            ],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("--rehearsal-only accepts no --outputs", completed.stderr)

    def test_production_is_refused_before_any_scratch_or_output_is_created(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            scratch = root / "scratch"
            binaries = root / "bin"
            scratch.mkdir()
            binaries.mkdir()
            python = binaries / "python3"
            python.write_text(
                "#!/bin/sh\n"
                "case \" $* \" in *\" -c \"*) exec \"$REAL_PYTHON\" \"$@\";; esac\n"
                "printf '%s\\n' \"$*\" > \"$PRODUCER_CALL\"\n"
                "exit 73\n",
                encoding="utf-8",
            )
            python.chmod(0o755)
            call = root / "producer-call"
            environment = dict(os.environ)
            environment["TMPDIR"] = str(scratch)
            environment["PATH"] = f"{binaries}:/usr/bin:/bin"
            environment["PRODUCER_CALL"] = str(call)
            environment["REAL_PYTHON"] = sys.executable
            completed = subprocess.run(
                ["bash", str(WRAPPER), "--production"],
                cwd=REPO,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertEqual(list(scratch.iterdir()), [])
            self.assertIn("production-check", call.read_text(encoding="utf-8"))
            self.assertIn("production", completed.stderr.lower())


class WorkflowContractTests(unittest.TestCase):
    def source(self) -> str:
        return WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_manual_and_exposes_only_rehearsal_or_guarded_production(self) -> None:
        source = self.source()
        self.assertIn("workflow_dispatch:", source)
        self.assertIn("options: [rehearsal, production]", source)
        for automatic in ("pull_request:", "push:", "schedule:"):
            self.assertNotIn(automatic, source)

    def test_workflow_calls_only_the_v3_wrapper(self) -> None:
        source = self.source()
        self.assertEqual(source.count("native-shadow-successor-produce-arm64-v3.sh"), 3)
        self.assertNotIn("native_shadow_successor_produce_phase_arm64_v3.py", source)
        self.assertNotIn("native_shadow_successor_root_disk_readback_arm64_v3.py", source)
        self.assertNotIn(HISTORICAL_WRAPPER, source)
        self.assertNotIn(HISTORICAL_READBACK, source)

    def test_production_job_checks_authority_before_dependencies_or_scratch(self) -> None:
        body = workflow_job("production-authority-guard")
        self.assertIn("--production", body)
        for forbidden in (
            "rust-toolchain",
            "rustdist",
            "payload_acquire",
            "RUNNER_TEMP",
            "mkdir",
            "--cas",
            "--launcher",
            "--result",
            "upload-artifact@",
            "continue-on-error",
            "|| true",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, body)

    def test_rehearsal_uses_native_arm64_and_the_launcher_v2_inputs(self) -> None:
        body = workflow_job("free-rehearsal")
        self.assertIn("runs-on: ubuntu-24.04-arm", body)
        self.assertIn("native_shadow_boot_rustdist_acquire_arm64_v1.py", body)
        self.assertIn("native_shadow_boot_ci_payload_acquire_arm64_v1.py", body)
        self.assertIn("native_shadow_launcher_emit_arm64_v2.py", body)
        wrapper = body.index("--rehearsal-only")
        for dependency in (
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_launcher_emit_arm64_v2.py",
        ):
            self.assertLess(body.index(dependency), wrapper)
        self.assertIn("--rehearsal-only", body[wrapper:])
        self.assertNotIn("--outputs", body)

    def test_workflow_verifies_all_bindings_before_any_repository_python(self) -> None:
        body = workflow_job("free-rehearsal")
        gate = body.index("--verify-bindings-only")
        for repository_python in (
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_launcher_emit_arm64_v2.py",
        ):
            with self.subTest(repository_python=repository_python):
                self.assertLess(gate, body.index(repository_python))

    def test_every_workflow_python_command_uses_isolated_startup(self) -> None:
        source = self.source()
        command_lines = [
            line.strip()
            for line in source.splitlines()
            if re.search(r"(^|\$\()python3 ", line.strip())
        ]
        self.assertTrue(command_lines)
        for line in command_lines:
            with self.subTest(line=line):
                self.assertIn("python3 -I -S ", line)

    def test_rehearsal_keeps_exactly_one_json_artifact_and_no_image_surface(self) -> None:
        body = workflow_job("free-rehearsal")
        self.assertEqual(body.count("upload-artifact@"), 1)
        self.assertIn("rehearsal-artifact/REHEARSAL-RESULT.json", body)
        self.assertIn("if-no-files-found: error", body)
        self.assertIn("member_count", body)
        for forbidden in (
            "guest-kernel",
            "guest-initrd",
            "guest-root-disk",
            "ATTEMPT-" + "CONSUMED.json",
            "mke2fs",
            "mkfs.ext4",
            "mkinitramfs",
            "qemu-img",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, body)

    def test_actions_are_pinned_and_failures_are_not_hidden(self) -> None:
        source = self.source()
        references = re.findall(r"^\s*uses:\s*(\S+)", source, re.MULTILINE)
        self.assertTrue(references)
        for reference in references:
            self.assertRegex(reference, r"@[0-9a-f]{40}$")
        self.assertNotIn("continue-on-error", source)
        self.assertNotIn("|| true", source)

    def test_every_checkout_drops_persisted_credentials(self) -> None:
        source = self.source()
        self.assertEqual(source.count("actions/checkout@"), 2)
        self.assertEqual(source.count("persist-credentials: false"), 2)


if __name__ == "__main__":
    unittest.main()
