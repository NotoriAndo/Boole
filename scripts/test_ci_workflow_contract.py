"""N0-pre.2 -- CI workflow supply-chain contract.

Every third-party action must be pinned to a full 40-hex commit SHA (a
mutable tag reassignment is undetected arbitrary code execution in CI),
and the workflow must declare a least-privilege top-level ``permissions``
block so the default GITHUB_TOKEN cannot write to the repository.
"""

import pathlib
import re
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
VERDICT_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "verdict-corpus.yml"
NATIVE_CONTAINMENT_PROBE = (
    REPO_ROOT / "scripts" / "native-shadow-containment-capability-probe.sh"
)
NATIVE_CONTAINMENT_SPEC = (
    REPO_ROOT / "docs" / "node-native-shadow-binding-containment-implementation-spec-v1.md"
)

USES_RE = re.compile(r"^\s*uses:\s*(\S+)", re.MULTILINE)
SHA_PIN_RE = re.compile(r"@[0-9a-f]{40}$")


class CiWorkflowContractTest(unittest.TestCase):
    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def test_ci_actions_are_sha_pinned(self):
        uses = USES_RE.findall(self.text)
        self.assertTrue(uses, "ci.yml must contain at least one uses: action")
        unpinned = [ref for ref in uses if not SHA_PIN_RE.search(ref)]
        self.assertEqual(
            unpinned,
            [],
            "every action must be pinned to a 40-hex commit SHA; "
            f"mutable refs found: {unpinned}",
        )

    def test_ci_declares_least_privilege_permissions(self):
        self.assertRegex(
            self.text,
            re.compile(r"^permissions:\n\s+contents:\s*read\b", re.MULTILINE),
            "ci.yml must declare a top-level least-privilege permissions "
            "block (contents: read)",
        )

    def test_ci_installs_the_exact_native_checker_commit_artifacts(self):
        self.assertIn(
            './scripts/install-native-checker-toolchain.sh "$native_toolchain"',
            self.text,
            "the clean-runner native checker gate must install the exact frozen "
            "per-commit artifacts; a date-based nightly can point at another commit",
        )
        self.assertIn(
            'echo "BOOLE_NATIVE_TOOLCHAIN_BIN=$native_toolchain/bin" >> "$GITHUB_ENV"',
            self.text,
            "self-test must receive the absolute bin directory of the verified toolchain",
        )


class NativeShadowContainmentWorkflowContractTest(unittest.TestCase):
    """Phase 3B.1 -- a real, non-skippable Linux containment capability gate."""

    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def _job(self, name: str) -> str:
        match = re.search(
            rf"^  {re.escape(name)}:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)",
            self.text,
            re.MULTILINE | re.DOTALL,
        )
        self.assertIsNotNone(match, f"ci.yml must declare the {name!r} job")
        return match.group("body")

    def test_named_linux_job_is_pinned_and_non_skippable(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "runs-on: ubuntu-24.04",
            job,
            "the containment capability contract needs the named Ubuntu 24.04 VM, "
            "not a moving generic runner label",
        )
        self.assertIn(
            "./scripts/native-shadow-containment-capability-probe.sh",
            job,
            "the named job must execute the real kernel capability probe",
        )
        self.assertNotIn("continue-on-error:", job)
        self.assertNotRegex(job, re.compile(r"\bskip\b", re.IGNORECASE))

    def test_required_self_test_cannot_hide_a_failed_probe(self):
        job = self._job("self-test")
        self.assertRegex(
            job,
            re.compile(r"^\s+needs:\s*native-shadow-containment-linux\s*$", re.MULTILINE),
        )
        self.assertRegex(job, re.compile(r"^\s+if:\s*always\(\)\s*$", re.MULTILINE))
        self.assertIn("needs.native-shadow-containment-linux.result", job)
        self.assertIn(
            "native-shadow containment capability probe did not pass",
            job,
        )

    def test_probe_contract_names_the_real_kernel_operations(self):
        self.assertTrue(
            NATIVE_CONTAINMENT_PROBE.is_file(),
            "the named Linux job must call a tracked, reviewable probe script",
        )
        body = NATIVE_CONTAINMENT_PROBE.read_text(encoding="utf-8")
        for required in (
            "set -euo pipefail",
            "cgroup2fs",
            "cgroup.subtree_control",
            "pids.max",
            "memory.max",
            "memory.swap.max",
            "memory.oom.group",
            "cpu.max",
            "max 100000",
            "cgroup.freeze",
            "cgroup.kill",
            "populated 0",
            "unshare",
            "--map-auto",
            "MS_REC|MS_PRIVATE",
            "tmpfs",
            "nosuid,nodev",
            "--bounding-set=-all",
            "--inh-caps=-all",
            "--ambient-caps=-all",
            "NoNewPrivs",
        ):
            self.assertIn(required, body, f"probe is missing required operation {required!r}")

        self.assertNotIn(
            '[[ ! -s "$delegated/cgroup.procs" ]]',
            body,
            "cgroupfs virtual files report stat size zero even when they contain PIDs; "
            "the probe must read cgroup.procs rather than inspect st_size",
        )
        self.assertIn('$(<"$delegated/cgroup.procs")', body)
        self.assertIn(
            "trap cleanup_namespace_probe EXIT",
            body,
            "the probe must release/kill its namespace child and remove its temp tree "
            "on both success and failure",
        )
        self.assertIn('kill "$namespace_pid"', body)
        self.assertRegex(
            body,
            re.compile(
                r'if \[\[ ! -e "\$ready" \]\]; then\s+'
                r'die "user/mount namespace probe failed before signaling readiness"'
            ),
            "a namespace that never becomes ready must enter the EXIT cleanup trap "
            "immediately instead of blocking in wait",
        )

    def test_spec_uses_the_kernel_valid_unthrottled_cpu_wire_value(self):
        body = NATIVE_CONTAINMENT_SPEC.read_text(encoding="utf-8")
        self.assertNotIn(
            "`max max`",
            body,
            "cpu.max PERIOD must be numeric even when MAX is unlimited",
        )
        self.assertIn("`max 100000`", body)
        self.assertNotIn(
            "tmpfs** mount over a loopback device",
            body,
            "tmpfs and a loopback-backed filesystem are mutually exclusive choices",
        )


class VerdictCorpusWorkflowContractTest(unittest.TestCase):
    """SC.9c (ADR-0016 (a-1)) -- the cross-platform verdict corpus gate.

    Four concrete jobs (Linux/macOS x debug/release) compare one golden
    verdict digest, behind an always-created aggregate ``verdict-corpus``
    status that branch protection requires. A platform- or
    profile-divergent verdict is a fork vector; merely running the corpus
    inside the Ubuntu self-test is explicitly insufficient.
    """

    def setUp(self):
        self.assertTrue(
            VERDICT_WORKFLOW.is_file(),
            "SC.9c requires .github/workflows/verdict-corpus.yml",
        )
        self.text = VERDICT_WORKFLOW.read_text(encoding="utf-8")

    def test_actions_are_sha_pinned_and_least_privilege(self):
        uses = USES_RE.findall(self.text)
        self.assertTrue(uses, "verdict-corpus.yml must use pinned actions")
        unpinned = [ref for ref in uses if not SHA_PIN_RE.search(ref)]
        self.assertEqual(unpinned, [], f"mutable refs found: {unpinned}")
        self.assertRegex(
            self.text,
            re.compile(r"^permissions:\n\s+contents:\s*read\b", re.MULTILINE),
            "verdict-corpus.yml must declare least-privilege permissions",
        )

    def test_matrix_covers_both_platforms_and_profiles(self):
        for token in ("ubuntu-latest", "macos-latest"):
            self.assertIn(
                token,
                self.text,
                f"the corpus matrix must include {token} (ADR-0016 (a-1))",
            )
        self.assertRegex(
            self.text,
            re.compile(r"profile:\s*\[\s*debug\s*,\s*release\s*\]"),
            "the corpus matrix must cover debug AND release profiles",
        )

    def test_aggregate_verdict_corpus_status_always_runs(self):
        self.assertRegex(
            self.text,
            re.compile(r"^\s{2}verdict-corpus:\n", re.MULTILINE),
            "an aggregate job id `verdict-corpus` must exist -- it is the "
            "branch-protection required check name",
        )
        self.assertRegex(
            self.text,
            re.compile(r"if:\s*always\(\)"),
            "the aggregate must be created even when a matrix job fails, "
            "so the required status can never be silently absent",
        )

    def test_workflow_is_not_path_filtered(self):
        self.assertNotIn(
            "paths:",
            self.text,
            "a required check must run on every PR -- path filters would "
            "hang PRs that do not touch the filtered paths (contrast the "
            "non-required macos-isolation canary, ADR-0016 (a-1))",
        )

    def test_corpus_runs_the_verdict_corpus_test_in_both_profiles(self):
        self.assertIn(
            "--test verdict_corpus",
            self.text,
            "the matrix jobs must run the boole-lean-runner verdict_corpus test",
        )
        self.assertIn(
            "--release",
            self.text,
            "the release-profile job must actually test the release profile",
        )


if __name__ == "__main__":
    unittest.main()
