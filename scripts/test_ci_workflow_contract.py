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
PORTABLE_ROOTFS_REPLAY_GATE = (
    REPO_ROOT / "scripts" / "native-shadow-portable-rootfs-replay-linux.sh"
)
ARM64_ROOTFS_REPLAY_GATE = (
    REPO_ROOT / "scripts" / "native-shadow-portable-rootfs-replay-linux-arm64.sh"
)
NATIVE_SHADOW_HTTP_REPLAY_GATE = (
    REPO_ROOT / "scripts" / "native_shadow_http_replay_gate.py"
)
NATIVE_SHADOW_REPLAY_NODE_UNIT = (
    REPO_ROOT / "native" / "systemd" / "boole-native-shadow-replay-node.service"
)
SELF_TEST = REPO_ROOT / "scripts" / "self-test.sh"
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

    def test_offline_rootfs_builder_runs_in_named_linux_private_network(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "sudo ./scripts/native-shadow-rootfs-builder-linux-gate.sh",
            job,
        )
        gate = (
            REPO_ROOT / "scripts" / "native-shadow-rootfs-builder-linux-gate.sh"
        ).read_text(encoding="utf-8")
        for required in (
            "PrivateNetwork=yes",
            "NoNewPrivileges=yes",
            "ProtectSystem=strict",
            '--setenv="TMPDIR=$scratch"',
            "scripts.test_native_shadow_rootfs_builder",
            "scripts.test_native_shadow_rootfs_oci_verify",
        ):
            self.assertIn(required, gate)
        for forbidden in ("continue-on-error", "|| true", "SKIP"):
            self.assertNotIn(forbidden, gate)

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

    def test_clean_linux_rootfs_replay_is_a_named_non_skippable_gate(self):
        job = self._job("native-shadow-rootfs-replay-linux")
        self.assertIn("runs-on: ubuntu-24.04", job)
        self.assertIn("timeout-minutes: 30", job)
        self.assertIn(
            "sudo ./scripts/native-shadow-portable-rootfs-replay-linux.sh",
            job,
        )
        self.assertNotIn("continue-on-error:", job)
        self.assertNotRegex(job, re.compile(r"\bskip\b", re.IGNORECASE))
        self.assertTrue(PORTABLE_ROOTFS_REPLAY_GATE.is_file())

    def test_clean_linux_rootfs_replay_binds_networked_acquisition_to_offline_probe(self):
        body = PORTABLE_ROOTFS_REPLAY_GATE.read_text(encoding="utf-8")
        manager = (
            REPO_ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        active_execution = (
            REPO_ROOT
            / "crates/boole-native-shadow-launcher/src/active_execution/mod.rs"
        ).read_text(encoding="utf-8")
        http_replay = NATIVE_SHADOW_HTTP_REPLAY_GATE.read_text(encoding="utf-8")
        for command in (
            "native_shadow_rootfs_acquire.py",
            "native_shadow_rootfs_portable_v2.py",
            "native_shadow_rootfs_builder.py",
        ):
            self.assertRegex(body, re.compile(rf'{re.escape(command)}"\s+[a-z-]+'))
        for required in (
            "set -euo pipefail",
            "fetch-metadata",
            "resolve",
            "fetch-payloads",
            "seal",
            "PrivateNetwork=yes",
            "native_shadow_rootfs_oci_verify.py",
            "verify-output",
            "chroot",
            "x86_64-linux-gnu-gcc-13",
            "native-shadow-manager-cgroup-gate.sh",
            "--closed-local-replay-rootfs",
        ):
            self.assertIn(required, body)
        for required in (
            "runtime rootfs replay identity drifted",
            "cargo build --locked -p boole-native-shadow-launcher --bin boole-native-shadow-launcher",
            "cargo build --locked -p boole-node --features native-shadow-closed-local-replay --bin boole-native-shadow-replay-node",
            "native/systemd/boole-native-shadow-replay-node.service",
            "http_replay_gate_source=$(readlink -f scripts/native_shadow_http_replay_gate.py)",
            "http_replay_gate_path=$launcher_directory/native-shadow-http-replay-gate.py",
            "http_replay_grant_path=$launcher_directory/native-shadow-http-replay-grant-v1.json",
            "http_replay_fixture_directory=$launcher_directory/native-shadow-http-replay-fixtures",
            'sudo install -o root -g root -m 0555 "$http_replay_gate_source" "$http_replay_gate_path"',
            '$(sha256sum "$http_replay_gate_source"',
            '$(sudo sha256sum "$http_replay_gate_path"',
            '$(sudo stat -c %U:%G:%a "$http_replay_gate_path") == root:root:555',
            'sudo systemctl start "$node_service_name"',
            '--grant-path "$http_replay_grant_path"',
            '--fixture-directory "$http_replay_fixture_directory"',
            '--journal-path "$node_journal_path"',
            "native-shadow-http-replay-matrix:PASS",
            "native-shadow-http-replay-journal:PASS",
            "native-shadow production HTTP replay gate: PASS",
        ):
            self.assertIn(required, manager)
        self.assertNotIn(
            "launcher_connections=3:empty_connections=0",
            manager,
            "the final three-checker matrix must traverse the production HTTP route, "
            "not the old direct Unix replay client",
        )
        for required in (
            "replay-accepted.raw.txt",
            "replay-tampered.raw.txt",
            "replay-constant.raw.txt",
            '"empty": None',
            "native-shadow-http-replay-case:{}:PASS",
            "native-shadow-http-replay-journal:PASS",
            "native-shadow-http-replay-matrix:PASS",
        ):
            self.assertIn(required, http_replay)
        self.assertTrue(NATIVE_SHADOW_REPLAY_NODE_UNIT.is_file())
        for required in (
            "runtime rootfs replay identity drifted",
            "[[ ${#peer_pids[@]} -eq 3 ]]",
            '[[ "$peer_pid" == "$node_pid_before" ]]',
            "launcher_journal_cursor=$(sudo journalctl --no-pager --show-cursor -n 0",
            '--after-cursor "$launcher_journal_cursor"',
            '"_PID=$launcher_pid"',
        ):
            self.assertIn(required, manager)
        self.assertIn("native-shadow-active-execution-peer:pid={}", active_execution)
        self.assertNotRegex(
            active_execution,
            re.compile(
                r'#\[cfg\(feature = "manager-cgroup-linux-gate"\)\]\s*'
                r'eprintln!\("native-shadow-active-execution-peer:pid=\{\}"'
            ),
            "the production launcher must retain the kernel-authenticated peer audit; "
            "only the CI-only diagnostic entrypoints belong behind the feature gate",
        )
        self.assertNotIn(
            "http_replay_gate_path=$(readlink -f scripts/native_shadow_http_replay_gate.py)",
            manager,
        )
        for forbidden in ("continue-on-error", "|| true", "SKIP"):
            self.assertNotIn(forbidden, body)

    def test_clean_linux_rootfs_replay_runs_the_crash_restart_gate(self):
        manager = (
            REPO_ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        driver_path = REPO_ROOT / "scripts/native_shadow_crash_restart_gate.py"
        self.assertTrue(driver_path.is_file())
        driver = driver_path.read_text(encoding="utf-8")
        for required in (
            'sudo python3 "$crash_gate_source"',
            "native-shadow-crash-restart-case:terminal-redelivery-across-node-kill:PASS",
            "native-shadow-crash-restart-case:unresolved-inflight-fail-closed:PASS",
            "native-shadow-crash-restart-gate:PASS",
            "native-shadow production crash/restart replay gate: PASS",
        ):
            self.assertIn(required, manager)
        # The crash phase must reuse the environment the HTTP matrix installed
        # and run strictly after that matrix has proven the normal path.
        self.assertLess(
            manager.index("native-shadow production HTTP replay gate: PASS"),
            manager.index("run_crash_restart_replay_gate() {"),
        )
        self.assertRegex(
            manager,
            re.compile(
                r"run_closed_local_replay_gate\n\s*run_crash_restart_replay_gate\n\s*exit 0"
            ),
        )
        for required in (
            "remains closed while durable InFlight rows are unresolved",
            '"/proc/{}/cgroup".format(pid)',
            "def verified_unit_main_pid",
            "def deliver_verified_signal",
        ):
            self.assertIn(required, driver)
        self.assertEqual(
            driver.count("os.kill("),
            1,
            "every signal must flow through the one verified-identity call site",
        )
        for forbidden in ("pkill", "killall", "continue-on-error", "|| true"):
            self.assertNotIn(forbidden, driver)
        self.assertNotRegex(driver, re.compile(r"\bskip\b", re.IGNORECASE))

    def test_self_test_requires_all_native_shadow_linux_gates(self):
        job = self._job("self-test")
        self.assertRegex(
            job,
            re.compile(
                r"needs:\s*\[native-shadow-containment-linux,\s*"
                r"native-shadow-rootfs-replay-linux,\s*"
                r"native-shadow-rootfs-replay-linux-arm64\]"
            ),
        )
        self.assertIn("needs.native-shadow-containment-linux.result", job)
        self.assertIn("needs.native-shadow-rootfs-replay-linux.result", job)
        self.assertIn(
            "needs.native-shadow-rootfs-replay-linux-arm64.result", job
        )

    def test_self_test_runs_the_native_shadow_http_replay_helper_contract(self):
        self.assertIn(
            "scripts/test_native_shadow_http_replay_gate.py",
            SELF_TEST.read_text(encoding="utf-8"),
            "the ordinary self-test lane must keep the HTTP matrix parser and "
            "journal contract under test even when the real Linux gate is separate",
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
            "systemd-analyze --root=\"$stage\" verify",
            "boole-native-shadow-launcher.service",
            "boole-native-shadow-replay-node.service",
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

    def test_manager_cgroup_has_a_named_real_systemd_gate(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn(
            "./scripts/native-shadow-manager-cgroup-gate.sh",
            job,
            "the named Linux job must start the tracked service and inspect its real cgroup",
        )

        gate_path = REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh"
        self.assertTrue(gate_path.is_file())
        body = gate_path.read_text(encoding="utf-8")
        for required in (
            "boole-native-shadow-manager-cgroup-linux",
            "native-shadow manager cgroup gate: PASS",
            "native-shadow-manager-frozen-rejected",
            "systemctl start boole-native-shadow-launcher.service",
            "systemctl restart boole-native-shadow-launcher.service",
            "systemctl stop boole-native-shadow-launcher.service",
            "--property=InvocationID",
            "single_numeric_id",
            'root:root:700',
            'root:root:755',
            "manager cgroup has residual subtree controllers",
            "manager cgroup type is not exact domain after move",
            "service_root=/sys/fs/cgroup/system.slice/$unit_name",
            "manager_root=$service_root/manager",
        ):
            self.assertIn(required, body)
        self.assertNotIn(
            '[[ "$first_pid" -ne "$second_pid" ]]',
            body,
            "restart identity must not rely on a PID that the kernel may reuse",
        )

    def test_manager_cgroup_gate_rejects_without_touching_preexisting_service_state(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        for required in (
            '--property=LoadState --value',
            '[[ "$load_state" == not-found ]]',
            'if [[ "$unit_installed" == true ]]; then',
            '[[ "$runtime_directory_created" == true ]] && sudo rm -f "$mode_path"',
            '[[ "$runtime_directory_created" == true ]] && sudo rm -f "$runtime_directory/launcher.lock"',
        ):
            self.assertIn(required, body)
        preflight = body.split("for path in", 1)[1].split("done", 1)[0]
        self.assertIn('"$unit_dropin_directory"', preflight)
        self.assertIn('"$runtime_directory"', preflight)
        self.assertIn('"$service_root"', preflight)
        self.assertLess(
            body.index('--property=LoadState --value'),
            body.index('sudo install -o root -g root -m 0644 native/systemd/'),
            "loaded-unit rejection must happen before this gate installs an override",
        )
        cleanup = body.split("cleanup_gate() {", 1)[1].split("}\ntrap cleanup_gate EXIT", 1)[0]
        cleanup_prefix, owned_unit_cleanup = cleanup.split(
            'if [[ "$unit_installed" == true ]]; then', 1
        )
        owned_unit_cleanup = owned_unit_cleanup.split("fi", 1)[0]
        self.assertNotIn('systemctl stop "$unit_name"', cleanup_prefix)
        self.assertNotIn('systemctl reset-failed "$unit_name"', cleanup_prefix)
        self.assertIn('systemctl stop "$unit_name"', owned_unit_cleanup)
        self.assertIn('systemctl reset-failed "$unit_name"', owned_unit_cleanup)
        self.assertIn('rm -f "$unit_dropin_path"', owned_unit_cleanup)
        self.assertIn('rmdir "$unit_dropin_directory"', owned_unit_cleanup)
        self.assertIn('rm -f "$unit_path"', owned_unit_cleanup)
        self.assertIn("systemctl daemon-reload", owned_unit_cleanup)
        self.assertRegex(
            cleanup,
            re.compile(
                r'if \[\[ "\$unit_installed" == true \]\]; then\n'
                r'\s+sudo systemctl stop "\$unit_name".*\n'
                r'\s+sudo systemctl reset-failed "\$unit_name"',
                re.MULTILINE,
            ),
            "cleanup may stop/reset only the unit installed by this gate",
        )
        self.assertNotIn(
            '\n  sudo rm -f "$mode_path"',
            cleanup,
            "cleanup must never delete a mode file outside a runtime tree this gate created",
        )

    def test_manager_cgroup_gate_passes_tmpfiles_an_absolute_tracked_path(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn(
            "tmpfiles_path=$(readlink -f native/tmpfiles.d/boole-native-shadow.conf)",
            body,
        )
        self.assertIn('sudo systemd-tmpfiles --create "$tmpfiles_path"', body)
        self.assertNotIn(
            "sudo systemd-tmpfiles --create native/tmpfiles.d/boole-native-shadow.conf",
            body,
            "systemd-tmpfiles treats a bare relative argument as a config name, not the repo file",
        )

    def test_manager_cgroup_gate_surfaces_a_failed_unit_journal_immediately(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        self.assertIn('if [[ "$state" == failed ]]; then', body)
        self.assertIn(
            'sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :',
            body,
        )
        self.assertIn('die "unit entered failed state while waiting for $expected"', body)

    def test_manager_cgroup_gate_does_not_reset_a_successfully_collected_unit(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        expected_rejection = body.split("run_expected_rejection() {", 1)[1].split("}\n", 1)[0]
        self.assertNotIn(
            'systemctl reset-failed "$unit_name"',
            expected_rejection,
            "a successful one-shot service may already be garbage-collected",
        )
        safe_reuse = body.split("set_mode safe-reuse", 1)[1].split("set_mode normal", 1)[0]
        self.assertNotIn(
            'systemctl reset-failed "$unit_name"',
            safe_reuse,
            "a cleanly stopped service has no failed state to reset",
        )

    def test_manager_cgroup_rejection_waits_for_exactly_one_new_journal_marker(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        expected_rejection = body.split("run_expected_rejection() {", 1)[1].split(
            "}\n", 1
        )[0]
        for required in (
            "local marker_count_before",
            'marker_count_before=$(journal_marker_count "$marker")',
            'wait_for_marker_increment "$marker" "$marker_count_before"',
        ):
            self.assertIn(required, expected_rejection)
        marker_counter = body.split("journal_marker_count() {", 1)[1].split("}\n", 1)[0]
        self.assertIn("sudo journalctl --sync", marker_counter)
        self.assertIn('grep -Fxc "$marker"', marker_counter)
        increment_wait = body.split("wait_for_marker_increment() {", 1)[1].split(
            "}\n", 1
        )[0]
        self.assertIn("expected=$((before + 1))", increment_wait)
        self.assertIn('[[ "$observed" -eq "$expected" ]]', increment_wait)
        self.assertIn('(( observed > expected ))', increment_wait)

    def test_manager_cgroup_gate_observes_root_only_cgroup_state_as_root(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        for required in (
            'sudo test ! -e "$service_root"',
            "mapfile -t values < <(sudo awk",
            'sudo cat "$service_root/cgroup.procs"',
            'sudo stat -c %U:%G:%a "$manager_root"',
            'sudo cat "$manager_root/cgroup.subtree_control"',
            'sudo cat "$manager_root/cgroup.type"',
            'sudo find "$manager_root" -mindepth 1 -maxdepth 1 -type d',
            'sudo cat "$service_root/cgroup.subtree_control"',
            'sudo stat -fc %T "$service_root"',
            'sudo readlink -f "/proc/$pid/exe"',
            "sudo awk -F: '$1 == \"0\" { print $3 }' \"/proc/$pid/cgroup\"",
        ):
            self.assertIn(required, body)
        self.assertNotIn(
            '<"$manager_root/',
            body,
            "the manager directory is deliberately root-only mode 0700",
        )

    def test_manager_gate_exercises_real_startup_orphan_recovery_fail_closed(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        for required in (
            "native-shadow-startup-recovery-prepared",
            "native-shadow-startup-recovery-complete:3",
            "native-shadow-startup-inventory-untouched",
            'startup_recovery_mode=\"startup-recovery\"',
            'inventory_reject_mode=\"startup-inventory-reject\"',
            'stream.write(f"{os.getpid()}\\n")',
            "child = os.fork()",
            "pid_start_time()",
            "wait_for_original_process_exit()",
            "wait_for_background_job()",
            "recovered_pid_starttimes",
            'sudo tee "$leaf_b/cgroup.freeze"',
            '[[ "$frozen" == 0 && "$populated" == 1 ]]',
            '"_SYSTEMD_INVOCATION_ID=$invocation_id"',
            'before_procs=$(sudo sort -n "$leaf/cgroup.procs")',
            '[[ "$before_procs" == "$after_procs" && "$before_threads" == "$after_threads" ]]',
            '[[ -z $(sudo find "$service_root" -mindepth 1 -maxdepth 1 -type d ! -name manager -print -quit) ]]',
        ):
            self.assertIn(required, body)
        harness = (
            REPO_ROOT
            / "crates"
            / "boole-native-shadow-launcher"
            / "tests"
            / "manager_cgroup_linux.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("for _ in 0..4_000", harness)
        self.assertNotIn(
            "cgroup.kill capability proven by zero-leaf recovery",
            body,
            "the gate must exercise a real populated leaf before claiming cleanup",
        )
        self.assertNotIn('wait "$recovered_tree_a"', body)
        self.assertNotIn('wait "$recovered_tree_b"', body)
        self.assertNotIn('wait "$reject_tree"', body)

    def test_manager_gate_serves_one_fixed_qualification_after_readiness(self):
        job = self._job("native-shadow-containment-linux")
        self.assertIn("timeout-minutes: 15", job)
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        for required in (
            'toolchain_parent=/opt/boole',
            'toolchain_prefix=$toolchain_parent/native-checker-toolchain',
            "opt_original_mode=$(stat -c %a /opt)",
            'sudo chmod go-w /opt',
            'sudo chmod "$opt_original_mode" /opt',
            './scripts/install-native-checker-toolchain.sh "$toolchain_stage"',
            'sudo chown -R root:root "$toolchain_prefix"',
            'sudo chmod 0555 "$toolchain_prefix" "$toolchain_prefix/bin"',
            '[[ $(sudo stat -c %U:%G:%a "$launcher_directory") == root:root:755 ]]',
            'listener_mode="qualification-one-shot"',
            'socket_path="$runtime_directory/launcher.sock"',
            'root:boole-node:660',
            "native_shadow_qualification::tests::installed_launcher_round_trip_is_ready_only",
            "native-shadow-node-qualification-ready-only",
            "node_marker_count",
            "--property=CapabilityBoundingSet=",
            '--property=NRestarts --value',
            "native-shadow-qualification-one-shot-complete",
            'sudo rm -rf "$toolchain_prefix"',
        ):
            self.assertIn(required, body)
        harness = (
            REPO_ROOT
            / "crates"
            / "boole-native-shadow-launcher"
            / "tests"
            / "manager_cgroup_linux.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("verify_fixed_startup_toolchain_compatibility", harness)
        self.assertIn("assemble_fixed_qualification_startup", harness)
        self.assertIn("serve_one_fixed_unix_qualification", harness)
        self.assertIn("native-shadow-qualification-one-shot-complete", harness)

    def test_manager_cgroup_gate_uses_an_owned_read_only_authority_bind(self):
        body = (REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh").read_text(
            encoding="utf-8"
        )
        for required in (
            "sudo mktemp -d /run/boole-native-shadow-manager-authority.XXXXXX",
            'authority_share="$authority_stage/share"',
            'unit_dropin_directory="/run/systemd/system/${unit_name}.d"',
            'BindReadOnlyPaths=${authority_share}:/usr/share',
            "expected_dropin=$'[Service]\\n'",
            'sudo install -o root -g root -m 0644 "$dropin_source" "$unit_dropin_path"',
            '[[ $(sudo cat "$unit_dropin_path") == "$expected_dropin" ]]',
            '--property=FragmentPath --value',
            '--property=DropInPaths --value',
            'systemd did not load exactly the gate-owned authority drop-in',
            'sudo rm -f "$unit_dropin_path"',
            'sudo rmdir "$unit_dropin_directory"',
            'sudo rmdir "$authority_stage"',
        ):
            self.assertIn(required, body)
        self.assertNotIn(
            "authority_parent=/usr/share/boole",
            body,
            "the gate must not install into or repair the hosted runner's unsafe /usr/share",
        )
        self.assertNotRegex(
            body,
            re.compile(r"(?:chmod|chown|install[^\n]*)\s+[^\n]*/usr/share(?:\s|/|$)"),
            "the gate must not mutate host /usr/share metadata or contents",
        )

    def test_launcher_gates_install_the_exact_local_execution_authority(self):
        expected = (
            "native/containment/native-shadow-local-execution-authority-v1.json "
            "local-execution-authority-v1.json"
        )
        for gate in (
            LAUNCHER_PRELOCK_GATE,
            REPO_ROOT / "scripts" / "native-shadow-manager-cgroup-gate.sh",
        ):
            body = gate.read_text(encoding="utf-8")
            self.assertIn(
                expected,
                body,
                f"{gate.name} must stage the exact successor authority beside the "
                "three frozen v1 authority files",
            )

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
            re.compile(
                r"^\s+needs:\s*\[native-shadow-containment-linux,\s*"
                r"native-shadow-rootfs-replay-linux,\s*"
                r"native-shadow-rootfs-replay-linux-arm64\]\s*$",
                re.MULTILINE,
            ),
        )
        self.assertRegex(job, re.compile(r"^\s+if:\s*always\(\)\s*$", re.MULTILINE))
        self.assertIn("needs.native-shadow-containment-linux.result", job)
        self.assertIn("needs.native-shadow-rootfs-replay-linux.result", job)
        self.assertIn(
            "needs.native-shadow-rootfs-replay-linux-arm64.result", job
        )
        self.assertIn(
            "native-shadow containment capability probe did not pass",
            job,
        )
        self.assertIn(
            "native-shadow portable rootfs replay did not pass",
            job,
        )
        self.assertIn(
            "native-shadow arm64 portable rootfs replay did not pass",
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


class NativeShadowArm64RootfsWorkflowContractTest(unittest.TestCase):
    """The MAC.2 authority-parity subgate executes on real Linux/arm64."""

    def setUp(self):
        self.text = WORKFLOW.read_text(encoding="utf-8")

    def _job(self, name: str) -> str:
        match = re.search(
            rf"(?ms)^  {re.escape(name)}:\n(.*?)(?=^  [A-Za-z0-9_-]+:\n|\Z)",
            self.text,
        )
        self.assertIsNotNone(match, f"missing CI job: {name}")
        return match.group(0)

    def test_arm64_rootfs_replay_is_named_native_and_non_skippable(self):
        job = self._job("native-shadow-rootfs-replay-linux-arm64")
        self.assertIn("runs-on: ubuntu-24.04-arm", job)
        self.assertIn("timeout-minutes: 45", job)
        self.assertIn("dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9", job)
        self.assertIn("toolchain: 1.95.0", job)
        self.assertIn("groupadd --system boole-node", job)
        self.assertIn("groupadd --system boole-native-checker", job)
        self.assertIn(
            "sudo ./scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh",
            job,
        )
        for forbidden in ("continue-on-error", "SKIP", "|| true"):
            self.assertNotIn(forbidden, job)

    def test_arm64_global_deadline_is_applied_and_leaves_workflow_reserve(self):
        gate = ARM64_ROOTFS_REPLAY_GATE.read_text(encoding="utf-8")
        job = self._job("native-shadow-rootfs-replay-linux-arm64")

        deadline_match = re.search(
            r"(?m)^arm64_manager_deadline_seconds=([0-9]+)$", gate
        )
        self.assertIsNotNone(deadline_match)
        manager_seconds = int(deadline_match.group(1))
        self.assertEqual(manager_seconds, 2100)
        self.assertEqual(gate.count('"${arm64_manager_deadline_seconds}s"'), 1)
        self.assertLess(
            gate.index("arm64_manager_deadline_seconds=2100"),
            gate.index('"${arm64_manager_deadline_seconds}s"'),
        )

        workflow_match = re.search(r"timeout-minutes: ([0-9]+)", job)
        self.assertIsNotNone(workflow_match)
        workflow_seconds = int(workflow_match.group(1)) * 60
        self.assertEqual(workflow_seconds, 45 * 60)
        self.assertGreaterEqual(workflow_seconds - manager_seconds, 600)
        self.assertIn("global CI orchestration cap", gate)
        self.assertNotIn("frozen inner deadlines total", gate)

    def test_self_test_requires_arm64_rootfs_replay(self):
        job = self._job("self-test")
        self.assertIn("native-shadow-rootfs-replay-linux-arm64", job)
        self.assertIn(
            "needs.native-shadow-rootfs-replay-linux-arm64.result", job
        )
        self.assertIn("arm64 portable rootfs replay did not pass", job)

    def test_arm64_gate_executes_the_frozen_parity_matrix(self):
        gate = ARM64_ROOTFS_REPLAY_GATE.read_text(encoding="utf-8")
        for required in (
            '[[ $(uname -m) == "aarch64" ]]',
            "native_shadow_rootfs_portable_arm64_v1.py",
            "native_shadow_rootfs_oci_verify_arm64_v1.py",
            "accepted.rs",
            "empty.rs",
            "tampered.rs",
            "constant.rs",
            "outside_patch_modified",
            "PrivateNetwork=yes",
            "native-shadow-manager-cgroup-gate.sh",
            "--closed-local-replay-rootfs-arm64",
            "MAC2-RESULT.json",
            '"containmentEnforcementParity": "EXACT"',
            '"mac2Status": "PARTIAL"',
            '"completedSubgate": "CLOSED-LOCAL-LINUX-ARM64-AUTHORITY-PARITY"',
            '"openRequirement": "POST-UPDATE-IMAGE-AND-RUNTIME-AUTHORITY-REVERIFICATION"',
            '"resourcePolicyDocumentParity": "EXACT-EXCEPT-FROZEN-ARCHITECTURE-IDENTITY"',
            '"resourcePolicyEnforcementParity": "EXACT"',
        ):
            self.assertIn(required, gate)
        self.assertNotIn('"resourcePolicyParity": "EXACT', gate)
        self.assertNotIn('"containmentEnforcementParity": "NOT-YET-PROVEN"', gate)
        self.assertNotIn('"mac2Status": "COMPLETE"', gate)
        for forbidden in ("continue-on-error", "SKIP", "|| true"):
            self.assertNotIn(forbidden, gate)

    def test_arm64_exact_rootfs_runs_launcher_before_transient_probe_files(self):
        gate = ARM64_ROOTFS_REPLAY_GATE.read_text(encoding="utf-8")
        offline_build = gate.split('if [[ ${1:-} == "--offline-build" ]]', 1)[1]
        offline_build = offline_build.split('if [[ ${1:-} == "--offline-parity" ]]', 1)[0]
        self.assertIn('"$oci/ROOTFS-CONTENT-MANIFEST.json"', offline_build)
        self.assertIn("builder = json.loads", offline_build)
        self.assertIn("independent = json.loads", offline_build)
        self.assertIn("if builder != independent:", offline_build)
        self.assertNotIn(
            'cmp --silent "$oci/BUILD-RECEIPT.json" "$independent_receipt"',
            offline_build,
        )
        self.assertNotIn("runtime_passwd=", offline_build)
        self.assertNotIn('rootfs/probe', offline_build)
        manager = gate.index("native-shadow-manager-cgroup-gate.sh")
        parity = gate.rindex('systemd-run --quiet --pipe --wait --collect --unit "$parity_unit"')
        self.assertLess(manager, parity)

    def test_arm64_manager_mode_is_explicit_and_scratch_prefix_is_exact(self):
        manager = (
            REPO_ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        for required in (
            "--closed-local-replay-rootfs-arm64",
            "/tmp/boole-native-shadow-arm64-rootfs.*/*",
            "authority_profile=arm64",
            '[[ $(uname -m) == aarch64 ]]',
        ):
            self.assertIn(required, manager)
        self.assertIn(
            "/tmp/boole-native-shadow-rootfs-replay.*/*",
            manager,
            "the established x86 replay mode must keep its exact scratch prefix",
        )

    def test_arm64_manager_mode_binds_features_authorities_and_verified_toolchain(self):
        manager = (
            REPO_ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        for required in (
            "manager-cgroup-linux-gate,linux-arm64-authority",
            "native-shadow-closed-local-replay,linux-arm64-authority",
            "fixtures/native-shadow/registry-arm64-v1.json registry-v1.json",
            "native/containment/native-shadow-execution-policy-arm64-v1.json execution-policy-v1.json",
            "native/containment/native-shadow-toolchain-identity-arm64-v1.json toolchain-identity-v1.json",
            "native/containment/native-shadow-local-execution-authority-arm64-v1.json local-execution-authority-v1.json",
            "native/containment/native-shadow-closed-local-replay-grant-arm64-v1.json closed-local-replay-grant-v1.json",
            "native/containment/native-shadow-closed-local-replay-registry-overlay-arm64-v1.json closed-local-replay-registry-overlay-v1.json",
            "native/containment/native-shadow-closed-local-replay-execution-authority-arm64-v1.json closed-local-replay-execution-authority-v1.json",
            "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST-arm64-v1.json",
            'arm64_toolchain_source="$closed_local_replay_rootfs/opt/boole/native-checker-toolchain"',
            'cp -a "$arm64_toolchain_source/." "$toolchain_stage/"',
            './scripts/install-native-checker-toolchain.sh "$toolchain_stage"',
        ):
            self.assertIn(required, manager)
        for required in (
            "fixtures/native-shadow/registry-v1.json registry-v1.json",
            "native/containment/native-shadow-local-execution-authority-v1.json local-execution-authority-v1.json",
            "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
        ):
            self.assertIn(
                required,
                manager,
                "the default x86 authority/install path must remain present",
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
