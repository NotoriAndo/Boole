#!/usr/bin/env python3
"""Regression tests for Boole one-line installer behavior."""
from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "install.sh"


class InstallScriptTests(unittest.TestCase):
    def run_installer(self, *args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        merged_env = os.environ.copy()
        merged_env.update(
            {
                "ANTHROPIC_API_KEY": "test-secret-anthropic",
                "OPENAI_API_KEY": "test-secret-openai",
                "GOOGLE_API_KEY": "test-secret-google",
                "XAI_API_KEY": "test-secret-xai",
            }
        )
        if env:
            merged_env.update(env)
        return subprocess.run(
            ["bash", str(INSTALLER), *args],
            cwd=ROOT,
            text=True,
            capture_output=True,
            env=merged_env,
            check=False,
            timeout=20,
        )

    def test_elan_fetch_is_tag_pinned_with_checksum(self) -> None:
        # N0-pre.3 — the elan installer must be fetched from an immutable
        # release tag and sha256-verified before execution, not piped from a
        # mutable `master` ref straight into a shell.
        script = INSTALLER.read_text(encoding="utf-8")
        elan_init_sha256 = (
            "a620ff1641616222c8d37c54845492004bb84d6877cdbc944dd65c1aa685bf53"
        )
        self.assertNotIn(
            "leanprover/elan/master/elan-init.sh",
            script,
            "elan must not be fetched from the mutable `master` ref",
        )
        self.assertIn(
            "leanprover/elan/v4.2.3/elan-init.sh",
            script,
            "elan installer must be pinned to the v4.2.3 tag (matching ci.yml)",
        )
        self.assertIn(
            elan_init_sha256,
            script,
            "install.sh must verify the elan installer sha256 before running it",
        )
        self.assertTrue(
            "sha256sum -c" in script or "shasum -a 256 -c" in script,
            "install.sh must run a checksum verification (sha256sum/shasum -c)",
        )

    def test_help_documents_required_dependency_installation_and_safe_modes(self) -> None:
        proc = self.run_installer("--help")
        combined = proc.stdout + proc.stderr
        self.assertEqual(proc.returncode, 0, combined)
        self.assertIn("Boole Installer", combined)
        self.assertIn("--yes", combined)
        self.assertIn("--dry-run", combined)
        self.assertIn("--no-install", combined)
        self.assertIn("--run-safe-preflight", combined)
        self.assertIn("installs required dependencies", combined)
        self.assertIn("never prints API key values", combined)

    def test_dry_run_prints_plan_without_cloning_or_leaking_secret_values(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            install_dir = Path(tmp) / "boole-install-target"
            proc = self.run_installer("--dry-run", "--yes", "--dir", str(install_dir))
            combined = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, combined)
            self.assertIn("DRY RUN", combined)
            self.assertIn("Install required dependencies", combined)
            self.assertIn("Clone or update Boole", combined)
            self.assertIn("Run setup doctor", combined)
            self.assertFalse(install_dir.exists(), "dry-run must not clone or create the target directory")
            for secret in ["test-secret-anthropic", "test-secret-openai", "test-secret-google", "test-secret-xai"]:
                self.assertNotIn(secret, combined)
            self.assertIn("ANTHROPIC_API_KEY: present", combined)
            self.assertIn("OPENAI_API_KEY: present", combined)

    def test_dry_run_yes_does_not_plan_safe_preflight_without_explicit_flag(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            install_dir = Path(tmp) / "boole-install-target"
            proc = self.run_installer("--dry-run", "--yes", "--dir", str(install_dir))
            combined = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, combined)
            self.assertNotIn("Run safe proof-to-block preflight", combined)
            self.assertNotIn("$ bash -lc cd", "\n".join(line for line in combined.splitlines() if "--preset safe --genesis-benchmark" in line))
            self.assertIn("Safe preflight not requested", combined)

    def test_no_install_doctor_uses_existing_checkout_without_system_installs(self) -> None:
        proc = self.run_installer("--no-install", "--doctor", "--dir", str(ROOT))
        combined = proc.stdout + proc.stderr
        self.assertEqual(proc.returncode, 0, combined)
        self.assertIn("Skipping dependency installation", combined)
        self.assertIn("Using existing Boole checkout", combined)
        self.assertIn("Run setup doctor", combined)
        self.assertNotIn("apt-get install", combined)
        self.assertNotIn("brew install", combined)

    def test_existing_checkout_accepts_github_actions_origin_without_dot_git_suffix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            checkout = Path(tmp) / "boole"
            scripts = checkout / "scripts"
            scripts.mkdir(parents=True)
            wizard = scripts / "boole-preflight-wizard.py"
            wizard.write_text("#!/usr/bin/env python3\nprint('fake doctor ok')\n", encoding="utf-8")
            wizard.chmod(0o755)
            subprocess.run(["git", "init"], cwd=checkout, check=True, capture_output=True, text=True)
            subprocess.run(
                ["git", "remote", "add", "origin", "https://github.com/NotoriAndo/Boole"],
                cwd=checkout,
                check=True,
                capture_output=True,
                text=True,
            )

            proc = self.run_installer("--no-install", "--doctor", "--dir", str(checkout))

            combined = proc.stdout + proc.stderr
            self.assertEqual(proc.returncode, 0, combined)
            self.assertIn("Using existing Boole checkout", combined)
            self.assertIn("fake doctor ok", combined)
            self.assertNotIn("existing checkout origin is not", combined)


if __name__ == "__main__":
    unittest.main()
