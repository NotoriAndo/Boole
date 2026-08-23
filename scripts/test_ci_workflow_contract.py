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
LAUNCHER_PRIVILEGE_GATE = (
    REPO_ROOT / "scripts" / "native-shadow-launcher-privilege-gate.sh"
)
LAUNCHER_PRELOCK_GATE = (
    REPO_ROOT / "scripts" / "native-shadow-launcher-prelock-gate.sh"
)
SYSTEMD_DEPLOYMENT_GATE = (
    REPO_ROOT / "scripts" / "native-shadow-systemd-deployment-envelope-gate.sh"
)
LAUNCHER_LIFETIME_LOCK_SOURCE = (
    REPO_ROOT
    / "crates"
    / "boole-native-shadow-launcher"
    / "src"
    / "lifetime_lock.rs"
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
        self.assertIn(
            "cargo build --locked -p boole-lean-runner --bin sandbox_probe",
            job,
            "the lib tests spawn the sibling sandbox_probe binary; a clean runner must "
            "build it explicitly before the filtered --lib test",
        )

    def test_named_linux_job_proves_the_fixed_service_accounts_via_libc(self):
        job = self._job("native-shadow-containment-linux")
        for required in (
            "groupadd --system boole-node",
            "useradd --system --gid boole-node --home-dir /nonexistent --no-create-home --shell /usr/sbin/nologin boole-node",
            "groupadd --system boole-native-checker",
            "useradd --system --gid boole-native-checker --home-dir /nonexistent --no-create-home --shell /bin/false boole-native-checker",
            "real_fixed_accounts_resolve_in_named_linux_gate",
            "--ignored --exact",
        ):
            self.assertIn(required, job)

    def test_named_linux_job_exercises_the_real_launcher_unix_session(self):
        job = self._job("native-shadow-containment-linux")
        for required in (
            "boole-native-shadow-launcher",
            "real_kernel_stream_round_trip_observes_peer_and_half_close",
            "--ignored --exact",
        ):
            self.assertIn(required, job)

    def test_named_linux_job_exercises_the_real_launcher_privilege_self_check(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "./scripts/native-shadow-launcher-privilege-gate.sh",
            job,
            "the named Linux job must run the production root/capability self-check",
        )
        self.assertTrue(
            LAUNCHER_PRIVILEGE_GATE.is_file(),
            "the launcher privilege gate must be a tracked reviewable script",
        )

    def test_launcher_privilege_gate_uses_one_production_verifier_for_positive_and_negative_cases(self):
        body = LAUNCHER_PRIVILEGE_GATE.read_text(encoding="utf-8")
        for required in (
            "set -euo pipefail",
            '[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"',
            "cargo test --locked -p boole-native-shadow-launcher --lib --no-run",
            "privilege::tests::real_kernel_privilege_matches_frozen_policy",
            "sudo mktemp /run/boole-native-shadow-launcher-captest.XXXXXX",
            'sudo install -o root -g root -m 0555 "$test_executable" "$launcher_path"',
            '[[ "$source_sha" == "$staged_sha" ]]',
            "--pipe --wait --collect",
            "--property=User=root --property=Group=root",
            "--property=AmbientCapabilities= --property=NoNewPrivileges=no",
            "CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN",
            "CAP_SETGID CAP_SETUID CAP_SETPCAP' reject 0x00000000000001c0",
            "CAP_CHOWN CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN' reject 0x00000000002001c1",
            'sudo rm -f "$launcher_path"',
            "transient unit ${unit}.service was not collected",
        ):
            self.assertIn(required, body)

        self.assertEqual(
            body.count('"$launcher_path" "$test_name" --ignored --exact --nocapture'),
            1,
            "all three service shapes must execute the same production verifier path",
        )

    def test_named_linux_job_exercises_the_real_launcher_prelock_lock_and_instance_identity(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "./scripts/native-shadow-launcher-prelock-gate.sh",
            job,
            "the named Linux job must compose prerequisites, lock, and instance identity",
        )
        self.assertTrue(
            LAUNCHER_PRELOCK_GATE.is_file(),
            "the launcher pre-lock gate must be a tracked reviewable script",
        )

    def test_named_linux_job_exercises_the_tracked_systemd_deployment_envelope(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "./scripts/native-shadow-systemd-deployment-envelope-gate.sh",
            job,
            "the named Linux job must materialize and verify the tracked unit, "
            "service accounts, and runtime directory",
        )
        self.assertTrue(
            SYSTEMD_DEPLOYMENT_GATE.is_file(),
            "the systemd deployment-envelope gate must be tracked and reviewable",
        )

    def test_systemd_deployment_gate_uses_real_systemd_tools_and_exact_postconditions(self):
        body = SYSTEMD_DEPLOYMENT_GATE.read_text(encoding="utf-8")
        for required in (
            "set -euo pipefail",
            "systemd-analyze --root=\"$stage\" verify boole-native-shadow-launcher.service",
            "systemd-sysusers --root=\"$stage\" \"$stage/usr/lib/sysusers.d/boole-native-shadow.conf\"",
            "systemd-tmpfiles --root=\"$stage\" --create \"$stage/usr/lib/tmpfiles.d/boole-native-shadow.conf\"",
            "for target in sysinit.target basic.target shutdown.target multi-user.target; do",
            '[[ "$node_uid" -ne 0 && "$checker_uid" -ne 0 ]]',
            '[[ "$node_gid" -ne 0 && "$checker_gid" -ne 0 ]]',
            '[[ "$node_uid" -ne "$checker_uid" ]]',
            '[[ "$node_gid" -ne "$checker_gid" ]]',
            '[[ "$config_metadata" == root:root:644 ]]',
            '[[ "$launcher_metadata" == root:root:755 ]]',
            '[[ "$runtime_parent_metadata" == 0:0:755 ]]',
            '[[ "$runtime_mode" == 2750 ]]',
            '[[ "$runtime_uid" -eq 0 && "$runtime_gid" -eq "$node_gid" ]]',
            "native-shadow-systemd-deployment-envelope-gate-complete",
        ):
            self.assertIn(required, body)
        self.assertNotIn("curl ", body)
        self.assertNotIn("wget ", body)

    def test_launcher_prelock_gate_calls_the_production_instance_identity_path(self):
        body = LAUNCHER_PRELOCK_GATE.read_text(encoding="utf-8")
        lock_source = LAUNCHER_LIFETIME_LOCK_SOURCE.read_text(encoding="utf-8")
        completion_marker = "native-shadow-launcher-instance-identity-gate-complete"
        for required in (
            "set -euo pipefail",
            '[[ ${EUID} -ne 0 ]] || die "build phase must run as the unprivileged CI user"',
            "cargo test --locked -p boole-native-shadow-launcher --lib --no-run",
            "lifetime_lock::unix::tests::real_linux_fixed_launcher_lifetime_lock_is_single_instance",
            "sudo mktemp -d /run/boole-native-shadow-authority.XXXXXX",
            'authority_share="$authority_stage/share"',
            "fixtures/native-shadow/registry-v1.json registry-v1.json",
            "native/containment/native-shadow-execution-policy-v1.json",
            "native/containment/native-shadow-toolchain-identity-v1.json",
            'sudo install -o root -g root -m 0444 "$source" "$destination"',
            '[[ "$source_sha" == "$installed_sha" ]]',
            '[[ ! -e "$runtime_parent" && ! -L "$runtime_parent" ]]',
            'sudo install -d -o root -g root -m 0755 "$runtime_parent"',
            'sudo install -d -o root -g boole-node -m 2750 "$runtime_directory"',
            "--property=User=root --property=Group=root",
            'CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN',
            "--property=AmbientCapabilities= --property=NoNewPrivileges=no",
            "--property=PrivateMounts=yes",
            '--property="BindReadOnlyPaths=${authority_share}:/usr/share"',
            'sudo rm -f "$authority_directory/$basename"',
            'sudo rmdir "$authority_directory"',
            'sudo rmdir "$authority_parent"',
            'sudo rmdir "$authority_share"',
            'sudo rmdir "$authority_stage"',
            'sudo rm -f "$runtime_directory/launcher.lock"',
            'sudo rmdir "$runtime_directory"',
            'sudo rmdir "$runtime_parent"',
            "transient unit ${unit}.service was not collected",
            "instance-identity composition failed",
            f'success_marker={completion_marker}',
            'marker_count=$(grep -Fxc "$success_marker" "$log" || :)',
            '[[ "$marker_count" -eq 1 ]]',
        ):
            self.assertIn(required, body)
        self.assertIn(
            f'"{completion_marker}"',
            lock_source,
            "the exact production parent test must define the frozen completion marker",
        )
        self.assertIn(
            'println!("{REAL_PARENT_COMPLETE_MARKER}")',
            lock_source,
            "the exact production parent test must emit its marker only after all postconditions",
        )
        self.assertNotIn(
            "sudo install -d -o root -g root -m 0755 /usr/share/boole",
            body,
            "the gate must not rewrite a hosted runner's unsafe /usr/share hierarchy",
        )
        self.assertNotRegex(
            body,
            re.compile(r"(^|[;&|\s])flock\s"),
            "the shell gate must exercise the Rust production lock, not take its own lock",
        )

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
            "--propagation unchanged",
            "MS_REC|MS_PRIVATE",
            "tmpfs",
            "nosuid,nodev",
            "--bounding-set=-all",
            "--inh-caps=-all",
            "--ambient-caps=-all",
            "NoNewPrivs",
            "privileged-launcher",
            "CapabilityBoundingSet=CAP_SETGID CAP_SETUID CAP_SETPCAP CAP_SYS_ADMIN",
            "failure-injection",
            "cleanup_cgroup_leaf_strict",
            "cleanup_cgroup_leaf_best_effort",
            "dropped child did not set all UID slots",
            "dropped child did not set all GID slots",
            "00000000002001c0",
            "privileged launcher has unexpected ${field}",
            "cleanup failure injection left a live child",
            "fail-before-ready",
            "expected-failure transient service unexpectedly succeeded",
            "expected-failure transient cgroup was not removed",
            "expected-failure namespace temp tree was not removed",
        ):
            self.assertIn(required, body, f"probe is missing required operation {required!r}")

        for forbidden in (
            "--map-root-user",
            "--map-auto",
            "--mount-proc",
            "/etc/subuid",
            'User=$probe_user',
        ):
            self.assertNotIn(
                forbidden,
                body,
                "the actual Ubuntu gate rejected the unprivileged-userns design; "
                "the successor must probe a separate minimal privileged launcher",
            )

        self.assertNotIn(
            '[[ ! -s "$delegated/cgroup.procs" ]]',
            body,
            "cgroupfs virtual files report stat size zero even when they contain PIDs; "
            "the probe must read cgroup.procs rather than inspect st_size",
        )
        self.assertIn('$(<"$delegated/cgroup.procs")', body)
        self.assertRegex(
            body,
            re.compile(
                r"mount --make-rprivate /\s+"
                r"mount -t proc -o nosuid,nodev,noexec proc /proc\s+"
                r"mount -t tmpfs"
            ),
            "the child mount namespace must become recursively private before its "
            "first new mount, including the private /proc mount",
        )
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
                r'die "mount/PID namespace probe failed before signaling readiness"'
            ),
            "a namespace that never becomes ready must enter the EXIT cleanup trap "
            "immediately instead of blocking in wait",
        )
        self.assertNotIn(
            "trap cleanup_leaf EXIT",
            body,
            "an EXIT trap runs after function-local variables are gone; cgroup cleanup "
            "must capture its leaf path instead of closing over a local",
        )
        self.assertIn("cleanup_cgroup_leaf", body)

    def test_probe_stages_the_trusted_launcher_outside_runner_home(self):
        body = NATIVE_CONTAINMENT_PROBE.read_text(encoding="utf-8")
        self.assertIn(
            "launcher_path=$(mktemp /run/boole-native-shadow-launcher.XXXXXX)",
            body,
            "a capability-bounded root service cannot rely on DAC_OVERRIDE to "
            "traverse the GitHub runner home; stage the reviewed launcher in /run",
        )
        self.assertIn(
            'install -o root -g root -m 0555 "$script_path" "$launcher_path"',
            body,
            "the staged launcher must be root-owned and immutable to the checker identity",
        )
        self.assertIn(
            '[[ "$(sha256sum "$launcher_path" | awk \'{ print $1 }\')" == "$(sha256sum "$script_path" | awk \'{ print $1 }\')" ]]',
            body,
            "the service must execute the exact reviewed script bytes",
        )
        self.assertIn(
            "trap cleanup_outer_probe EXIT",
            body,
            "one non-overwritten outer cleanup trap must remove the trusted staged copy",
        )
        self.assertRegex(
            body,
            re.compile(r"cleanup_outer_probe\(\).*?rm -f \"\$launcher_path\"", re.DOTALL),
            "outer cleanup must remove the staged launcher on success and failure",
        )
        self.assertEqual(
            body.count('"$launcher_path" privileged-launcher'),
            2,
            "both the injected-failure and normal systemd services must start the "
            "staged launcher instead of traversing runner home",
        )
        self.assertEqual(
            body.count('privileged-launcher "$failure_report" "$launcher_path"')
            + body.count('privileged-launcher "$report" "$launcher_path"'),
            2,
            "both services must also pass the staged path into recursive launcher calls",
        )

    def test_spec_uses_the_kernel_valid_unthrottled_cpu_wire_value(self):
        body = NATIVE_CONTAINMENT_SPEC.read_text(encoding="utf-8")
        self.assertNotIn(
            "`max max`",
            body,
            "cpu.max PERIOD must be numeric even when MAX is unlimited",
        )
        self.assertIn("`max 100000`", body)
        self.assertIn("boole-node` remains unprivileged", body)
        self.assertIn("CLONE_NEWPID", body)
        self.assertIn("private `/proc`", body)
        self.assertNotIn(
            "`boole-node` itself directly calls\n   `setrlimit(2)`",
            body,
            "the separate launcher, not the unprivileged node, owns pre-exec RLIMIT setup",
        )
        self.assertNotIn(
            "`boole-node`'s own outer",
            body,
            "outer execution ceilings belong to the separate launcher boundary",
        )
        self.assertNotIn(
            "then irreversibly becomes the checker identity",
            body,
            "the root monitor launcher stays outside; only its child drops to the checker identity",
        )
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
