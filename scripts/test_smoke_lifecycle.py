"""Smokes must preserve existing caller data and bound owned-child cleanup."""
import os
import json
import shlex
import shutil
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


class SmokeLifecycleTests(unittest.TestCase):
    def test_benchmark_invokes_runtime_consumer_from_a_checkout_with_spaces(self):
        # Execute the shipped benchmark without modifying its command. The
        # runtime consumer intentionally stops here, before Cargo/Lean work.
        with tempfile.TemporaryDirectory(prefix="boole benchmark ") as temp:
            checkout = Path(temp) / "Boole Space"
            scripts = checkout / "scripts"
            scripts.mkdir(parents=True)
            for name in ["proof-to-block-benchmark.sh", "smoke-lifecycle.sh"]:
                shutil.copy2(ROOT / "scripts" / name, scripts / name)
            runtime = scripts / "runtime-smoke-all.sh"
            runtime.write_text('#!/bin/bash\nprintf "runtime-consumer-reached\\n" >&2\nexit 73\n')
            runtime.chmod(0o700)
            result = subprocess.run(
                ["/bin/bash", str(scripts / "proof-to-block-benchmark.sh")],
                env={**os.environ, "BLOCK_STORE_DIR": str(checkout / "fresh-output")},
                capture_output=True, text=True, timeout=5,
            )
            self.assertEqual(result.returncode, 73, result.stderr)
            self.assertIn("runtime-consumer-reached", result.stderr)

    def run_helper(self, body):
        return subprocess.run(
            ["/bin/bash", "-c", f'set -euo pipefail\nsource "{ROOT}/scripts/smoke-lifecycle.sh"\n' + body],
            capture_output=True, text=True, timeout=12,
        )

    def test_build_uses_cargos_reported_executable_not_a_fixed_target_directory(self):
        expected = "/private/custom target/debug/boole-node"
        artifact = json.dumps({"reason": "compiler-artifact", "target": {"name": "boole-node"}, "executable": expected})
        result = self.run_helper(
            "cargo() { printf '%s\\n' " + shlex.quote(artifact) + "; }\n"
            "smoke_build_binary boole-node boole-node"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), expected)

    def test_build_failure_cannot_be_hidden_by_artifact_parser(self):
        result = self.run_helper("cargo() { return 73; }\nsmoke_build_binary boole-node boole-node")
        self.assertNotEqual(result.returncode, 0)

    def test_each_invocation_owns_a_distinct_directory_and_cleans_it(self):
        first = self.run_helper('printf "%s" "$SMOKE_WORK_DIR"')
        second = self.run_helper('printf "%s" "$SMOKE_WORK_DIR"')
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertNotEqual(first.stdout, second.stdout)
        self.assertFalse(Path(first.stdout).exists())
        self.assertFalse(Path(second.stdout).exists())

    def test_explicit_dangling_link_is_preserved_and_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            link = Path(temp) / "output"
            link.symlink_to(Path(temp) / "missing")
            result = self.run_helper(f'smoke_fresh_path "{link}"')
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(link.is_symlink())

    def test_clean_child_exit_is_waited_without_request_count_assumptions(self):
        result = self.run_helper(
            'bash -c \'trap "exit 0" TERM; while :; do sleep 0.05; done\' &\n'
            'child=$!\nsleep 0.1\nsmoke_stop_and_wait "$child"'
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_stubborn_child_is_killed_and_timeout_is_not_success(self):
        started = time.monotonic()
        result = self.run_helper(
            'bash -c \'trap "" TERM; while :; do sleep 0.05; done\' &\n'
            'child=$!\nsleep 0.1\nsmoke_stop_and_wait "$child"'
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not stop within", result.stderr)
        self.assertLess(time.monotonic() - started, 8)

    def test_existing_paths_are_rejected_without_deleting_caller_data(self):
        starts = {
            "smoke-testnet-faucet-to-block.sh": "\n# --- mock faucet",
            "local-node-smoke.sh": "\n# Finish compilation",
            "local-mining-smoke.sh": "\n# Finish compilation",
            "runtime-smoke.sh": '\nif [[ -n "${BOOLE_NODE_BIN',
            "runtime-smoke-all.sh": "\npython3",
            "proof-to-block-benchmark.sh": "\nSMOKE_JSON=",
            "boole-miner-agent-cli-smoke.sh": "\n# Build before",
            "boole-miner-hermes-cli-smoke.sh": "\ncommand -v hermes",
            "boole-miner-hermes-real-verify-smoke.sh": "\ncommand -v hermes",
            "boole-miner-ollama-gemma-smoke.sh": "\ncommand -v ollama",
            "boole-miner-opencode-cli-smoke.sh": '\nif [[ -n "${AGENT_RUNTIME_COMMAND',
            "boole-miner-smoke.sh": "\n# Build before",
            "provider-model-smoke.sh": "\n# Build before",
        }
        for name, marker in starts.items():
            with self.subTest(script=name), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                block = root / "blocks.ndjson"
                block.write_text("caller-owned\n")
                state = root / "state"
                state.mkdir()
                sentinel = state / "keep"
                sentinel.write_text("caller-owned\n")
                body = (ROOT / "scripts" / name).read_text().split(marker, 1)[0]
                body = body.replace('ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"', f'ROOT="{ROOT}"')
                result = subprocess.run(
                    ["/bin/bash", "-c", body],
                    env={**os.environ, "BLOCK_STORE": str(block), "STATE_DIR": str(state), "BLOCK_STORE_DIR": str(state),
                         "REWARD_LEDGER": str(root / "rewards.ndjson"),
                         "REWARD_STORE": str(root / "rewards.ndjson"),
                         "PROOF_DEDUP_LEDGER": str(root / "dedup.ndjson"), "TMPDIR": temp},
                    capture_output=True, text=True, timeout=10,
                )
                self.assertTrue(block.exists(), "smoke deleted an existing caller block store")
                self.assertEqual(block.read_text(), "caller-owned\n")
                self.assertEqual(sentinel.read_text(), "caller-owned\n")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("existing", result.stderr)

    def test_existing_runtime_output_rejection_stops_before_directory_creation(self):
        # A shell export declaration can hide a failed command substitution.
        # Observe the next filesystem action directly: rejection must stop the
        # script before it tries to create an output directory, even if that
        # action would otherwise succeed.
        with tempfile.TemporaryDirectory() as temp:
            body = (ROOT / "scripts/runtime-smoke-all.sh").read_text().split("\npython3", 1)[0]
            body = body.replace('ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"', f'ROOT="{ROOT}"')
            result = subprocess.run(
                ["/bin/bash", "-c", 'mkdir() { printf "unexpected_output_creation\\n" >&2; }\n' + body],
                env={**os.environ, "BLOCK_STORE_DIR": temp},
                capture_output=True, text=True, timeout=10,
            )
            self.assertNotEqual(result.returncode, 0, result.stderr)
            self.assertNotIn("unexpected_output_creation", result.stderr)
            self.assertIn("existing output path", result.stderr)


if __name__ == "__main__":
    unittest.main()
