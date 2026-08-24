#!/usr/bin/env python3
"""Contract tests for host-independent native-shadow rootfs authority v2."""

from __future__ import annotations

import copy
import hashlib
import json
import pathlib
import re
import tempfile
import unittest

from scripts import native_shadow_rootfs_portable_v2 as portable
from scripts import native_shadow_rootfs_acquire as acquire
from scripts import native_shadow_rootfs_builder as rootfs


ROOT = pathlib.Path(__file__).resolve().parents[1]
PORTABLE_PLAN = ROOT / "native/containment/native-shadow-runtime-rootfs-portable-plan-v2.json"
PORTABLE_RESOLUTION = ROOT / "native/containment/native-shadow-runtime-rootfs-resolution-v2.json"
PORTABLE_LOCK = ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-v2.json"
REPLAY_EXPECTATION = ROOT / "native/containment/native-shadow-runtime-rootfs-replay-expectation-v2.json"


def _v1_candidate(gpgv_path: str, gpgv_sha256: str, zstd_path: str, zstd_sha256: str):
    return {
        "schema": "boole.native-shadow.runtime-rootfs-source-lock.v1",
        "release": "NATIVE-SHADOW-RUNTIME-ROOTFS-SOURCE-CLOSURE-COMPLETE-NOT-ACTIVATABLE",
        "activationAllowed": False,
        "ubuntu": {
            "verification": {
                "gpgvPath": gpgv_path,
                "gpgvSha256": gpgv_sha256,
            }
        },
        "buildRecipe": {
            "zstdPath": zstd_path,
            "zstdSha256": zstd_sha256,
        },
    }


class NativeShadowRootfsPortableV2Tests(unittest.TestCase):
    def test_portable_successor_supplies_runtime_loader_aliases(self) -> None:
        candidate = _v1_candidate(
            "/ignored/gpgv",
            "a" * 64,
            "/ignored/zstd",
            "b" * 64,
        )
        candidate["derivedEntries"] = []

        portable_lock = portable.portable_source_lock_from_v1(candidate)

        self.assertEqual(
            portable_lock["derivedEntries"],
            [
                {
                    "logicalPath": "/lib",
                    "kind": "symlink",
                    "target": "usr/lib",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/lib64",
                    "kind": "symlink",
                    "target": "usr/lib64",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/bin/as",
                    "kind": "symlink",
                    "target": "x86_64-linux-gnu-as",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/bin/ld",
                    "kind": "symlink",
                    "target": "x86_64-linux-gnu-ld",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/lib/x86_64-linux-gnu/libLLVM.so.22.1-rust-1.99.0-nightly",
                    "kind": "symlink",
                    "target": "../../../opt/boole/native-checker-toolchain/lib/libLLVM.so.22.1-rust-1.99.0-nightly",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
                {
                    "logicalPath": "/usr/lib/x86_64-linux-gnu/librustc_driver-da0d54ffe246e605.so",
                    "kind": "symlink",
                    "target": "../../../opt/boole/native-checker-toolchain/lib/librustc_driver-da0d54ffe246e605.so",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                },
            ],
        )
        runtime_lock = copy.deepcopy(portable_lock)
        self.assertEqual(
            portable.runtime_lock_v1_equivalent(runtime_lock)["derivedEntries"],
            [],
        )

        runtime_lock["derivedEntries"][0]["target"] = "wrong/lib64"
        with self.assertRaisesRegex(
            portable.PortableAuthorityError, "successor aliases"
        ):
            portable.runtime_lock_v1_equivalent(runtime_lock)

    def test_linux_replay_installs_the_fixed_qualification_account_before_chroot(self) -> None:
        passwd = (
            ROOT / "native/containment/native-shadow-runtime-passwd-v2"
        ).read_bytes()
        self.assertEqual(
            passwd,
            b"nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n",
        )

        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")
        install = (
            'install -m 0444 -o 0 -g 0 "$runtime_passwd" '
            '"$rootfs/etc/passwd"'
        )
        self.assertIn(install, replay)
        self.assertLess(replay.index(install), replay.index("chroot --groups=''"))
        self.assertIn('cmp --silent "$runtime_passwd" "$rootfs/etc/passwd"', replay)

    def test_linux_replay_delegates_checker_adjudication_to_the_real_launcher_service(self) -> None:
        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        manager_call = "./scripts/native-shadow-manager-cgroup-gate.sh"
        self.assertIn(manager_call, replay)
        self.assertIn("--closed-local-replay-rootfs", replay)
        self.assertLess(
            replay.rindex('--offline-build "$scratch"'),
            replay.rindex(manager_call),
        )
        self.assertLess(
            replay.rindex(manager_call),
            replay.rindex('--offline-probe "$scratch"'),
        )
        self.assertIn(
            "consumed unchanged by the real launcher service",
            replay,
        )
        self.assertIn(
            "native-shadow-closed-local-replay-report:accepted:accepted:accepted:cleanup=true",
            manager,
        )
        self.assertIn(
            "native-shadow-closed-local-replay-report:tampered:deterministic_reject:compile_or_hidden_test_failed:cleanup=true",
            manager,
        )
        self.assertIn(
            "native-shadow-closed-local-replay-report:constant:deterministic_reject:compile_or_hidden_test_failed:cleanup=true",
            manager,
        )

    def test_linux_replay_rejects_rootfs_drift_before_any_checker_report(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        mutation = 'sudo python3 - "$mutation_target"'
        report_guard = "request-time rootfs mutation produced a checker Report"
        drift_reason = "runtime rootfs replay identity drifted"
        restore = 'sudo cp --preserve=all "$mutation_backup" "$mutation_target"'
        self.assertIn(mutation, manager)
        self.assertIn(report_guard, manager)
        self.assertIn(drift_reason, manager)
        self.assertIn(restore, manager)
        mutation_index = manager.index(mutation)
        report_guard_index = manager.index(report_guard, mutation_index)
        restore_index = manager.index(restore, report_guard_index)
        self.assertLess(mutation_index, report_guard_index)
        self.assertLess(report_guard_index, restore_index)

    def test_linux_replay_socket_timeout_preserves_the_service_failure_reason(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")

        timeout_reason = "fixed qualification socket did not appear"
        timeout_index = manager.index(timeout_reason)
        diagnostic = (
            'sudo systemctl show "$unit_name" '
            '--property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :'
        )
        journal = 'sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :'
        socket_wait = manager[
            manager.index("wait_for_fixed_socket() {") : manager.index(
                "wait_for_leaf_event() {"
            )
        ]
        state_wait = manager[
            manager.index("wait_for_state() {") : manager.index(
                "wait_for_cgroup_removal() {"
            )
        ]
        self.assertIn("fixed_socket_wait_attempts=2400", manager)
        self.assertIn(
            "for ((i = 0; i < fixed_socket_wait_attempts; i++)); do",
            socket_wait,
        )
        self.assertIn("for ((i = 0; i < 200; i++)); do", state_wait)
        self.assertIn(diagnostic, manager)
        self.assertIn(journal, manager)
        self.assertLess(manager.index(diagnostic), timeout_index)
        self.assertLess(manager.index(journal, manager.index(diagnostic)), timeout_index)

    def test_manager_metadata_race_preserves_the_launcher_journal(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        invariants = manager[
            manager.index("assert_manager_invariants() {") : manager.index(
                "wait_for_leaf_event() {"
            )
        ]
        metadata = 'manager_metadata=$(sudo stat -c %U:%G:%a "$manager_root" 2>/dev/null || :)'
        journal = 'sudo journalctl --no-pager -o cat -u "$unit_name" >&2 || :'
        failure = 'die "manager cgroup metadata does not match root:root:700: $manager_metadata"'
        self.assertIn(metadata, invariants)
        self.assertIn(journal, invariants)
        self.assertIn(failure, invariants)
        self.assertLess(invariants.index(metadata), invariants.index(journal))
        self.assertLess(invariants.index(journal), invariants.index(failure))

    def test_linux_replay_client_failure_dumps_the_exact_launcher_session_error(self) -> None:
        manager = (
            ROOT / "scripts/native-shadow-manager-cgroup-gate.sh"
        ).read_text(encoding="utf-8")
        client_start = manager.index('local client_status=$?')
        client_end = manager.index('local client_complete', client_start)
        failure_block = manager[client_start:client_end]

        self.assertIn('if [[ $client_status -ne 0 ]]; then', failure_block)
        self.assertIn(
            'sudo systemctl show "$unit_name" \\\n'
            '      --property=ActiveState,SubState,Result,ExecMainStatus,NRestarts >&2 || :',
            failure_block,
        )
        self.assertIn(
            'sudo journalctl --no-pager -o cat -u "$unit_name" \\\n'
            '      "_SYSTEMD_INVOCATION_ID=$launcher_invocation" >&2 || :',
            failure_block,
        )
        self.assertIn(
            'die "closed-local replay client failed or exceeded its outer deadline"',
            failure_block,
        )

    def test_linux_replay_mounts_a_private_proc_for_the_frozen_lld_wrapper(self) -> None:
        replay = (
            ROOT / "scripts/native-shadow-portable-rootfs-replay-linux.sh"
        ).read_text(encoding="utf-8")

        mount_proc = 'mount -t proc -o nosuid,nodev,noexec proc "$rootfs/proc"'
        first_checker = 'chroot --groups=\'\' --userspec=65534:65534 "$rootfs"'
        unmount_proc = 'umount "$rootfs/proc"'
        self.assertIn(mount_proc, replay)
        self.assertIn(unmount_proc, replay)
        self.assertLess(replay.index(mount_proc), replay.index(first_checker))
        self.assertLess(replay.index(first_checker), replay.index(unmount_proc))

    def test_portable_source_lock_bytes_ignore_runtime_tool_path_and_digest(self) -> None:
        first = _v1_candidate(
            "/host-a/bin/gpgv",
            "a" * 64,
            "/host-a/bin/zstd",
            "b" * 64,
        )
        second = _v1_candidate(
            "/different-host/tools/gpgv",
            "c" * 64,
            "/different-host/tools/zstd",
            "d" * 64,
        )

        first_portable = portable.portable_source_lock_from_v1(copy.deepcopy(first))
        second_portable = portable.portable_source_lock_from_v1(copy.deepcopy(second))

        self.assertEqual(
            rootfs.canonical_json(first_portable),
            rootfs.canonical_json(second_portable),
        )
        self.assertFalse(first_portable["activationAllowed"])
        self.assertEqual(
            first_portable["ubuntu"]["verification"],
            {"toolRole": "gpgv"},
        )
        self.assertEqual(first_portable["buildRecipe"]["zstdToolRole"], "zstd")
        self.assertNotIn("zstdPath", first_portable["buildRecipe"])
        self.assertNotIn("zstdSha256", first_portable["buildRecipe"])

    def test_runtime_tools_are_bound_only_in_the_run_receipt(self) -> None:
        portable_lock = portable.portable_source_lock_from_v1(
            _v1_candidate("/ignored/gpgv", "a" * 64, "/ignored/zstd", "b" * 64)
        )
        portable_raw = rootfs.canonical_json(portable_lock)
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            gpgv = root / "gpgv"
            zstd = root / "zstd"
            gpgv_raw = b"#!/bin/sh\necho 'gpgv test 1.0'\n"
            zstd_raw = b"#!/bin/sh\necho 'zstd test 2.0'\n"
            gpgv.write_bytes(gpgv_raw)
            zstd.write_bytes(zstd_raw)
            gpgv.chmod(0o500)
            zstd.chmod(0o500)

            runtime_lock, receipt = portable.materialize_runtime_lock(
                portable_lock,
                portable_raw,
                gpgv,
                zstd,
            )

        self.assertEqual(rootfs.canonical_json(portable_lock), portable_raw)
        self.assertEqual(
            receipt["portableSourceLockSha256"], hashlib.sha256(portable_raw).hexdigest()
        )
        self.assertEqual(receipt["tools"]["gpgv"]["path"], str(gpgv.resolve()))
        self.assertEqual(
            receipt["tools"]["gpgv"]["sha256"],
            hashlib.sha256(gpgv_raw).hexdigest(),
        )
        self.assertEqual(receipt["tools"]["gpgv"]["version"], "gpgv test 1.0")
        self.assertEqual(receipt["tools"]["zstd"]["path"], str(zstd.resolve()))
        self.assertEqual(receipt["tools"]["zstd"]["version"], "zstd test 2.0")
        self.assertFalse(receipt["activationAllowed"])
        self.assertEqual(runtime_lock["schema"], rootfs.LOCK_SCHEMA)
        self.assertEqual(runtime_lock["ubuntu"]["verification"]["gpgvPath"], str(gpgv.resolve()))
        self.assertEqual(runtime_lock["buildRecipe"]["zstdPath"], str(zstd.resolve()))
        self.assertNotIn(str(gpgv.resolve()), portable_raw.decode("utf-8"))
        self.assertNotIn(str(zstd.resolve()), portable_raw.decode("utf-8"))

    def test_launcher_manifest_pin_matches_the_frozen_replay_expectation(self) -> None:
        expectation = json.loads(REPLAY_EXPECTATION.read_text(encoding="utf-8"))
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/runtime_rootfs_replay.rs"
        ).read_text(encoding="utf-8")
        self.assertIn(
            expectation["expectedOutput"]["rootfsContentManifestSha256"],
            source,
        )

    def test_tracked_portable_successor_is_exact_inactive_and_host_independent(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )

        self.assertFalse(authority["plan"]["activationAllowed"])
        self.assertFalse(authority["sourceLock"]["activationAllowed"])
        self.assertFalse(authority["expectation"]["activationAllowed"])
        self.assertFalse(
            authority["expectation"]["productionByteProvenanceComplete"]
        )
        self.assertEqual(
            authority["plan"]["bootstrapAuthority"]["acquisitionPlanV1Sha256"],
            "8d8ac1a4fd82370c1f0c12a270bd38b9b2b78f0c1a155432298b4d654a0fb06e",
        )
        self.assertEqual(
            authority["plan"]["bootstrapAuthority"]["completeSourceLockV1Sha256"],
            "40880be22275155346dab292644943d06817f08f90bb9dee592659aa1fe0588c",
        )
        forbidden = (
            b"/opt/homebrew/",
            b"/Users/",
            b"f1c71affd4ce40e3c5a53b8cb0ac9601fbcd31d6834b732dd0c7b0145dce1995",
            b"aff8169fb421bb925fb16c44a7e0143fa2c7a941dc45cce76b15062a2ce54917",
        )
        for path in (
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
        ):
            raw = path.read_bytes()
            for token in forbidden:
                self.assertNotIn(token, raw, f"host tool identity leaked into {path.name}")

    def test_replay_expectation_rejects_output_mismatch_without_adoption(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )
        build_receipt = portable.expected_build_receipt(authority["expectation"])
        portable.verify_replay_output(authority["expectation"], build_receipt)

        changed = copy.deepcopy(build_receipt)
        changed["layerDigest"] = "sha256:" + "f" * 64
        with self.assertRaisesRegex(portable.PortableAuthorityError, "layerDigest"):
            portable.verify_replay_output(authority["expectation"], changed)

    def test_source_lock_package_bytes_are_cross_bound_to_signed_resolution(self) -> None:
        source_lock = rootfs.load_json_exact(
            PORTABLE_LOCK.read_bytes(), "portable source lock", require_canonical=True
        )
        expectation = rootfs.load_json_exact(
            REPLAY_EXPECTATION.read_bytes(), "replay expectation", require_canonical=True
        )
        deb = next(item for item in source_lock["artifacts"] if item["kind"] == "deb")
        deb["sha256"] = "f" * 64
        changed_raw = rootfs.canonical_json(source_lock)
        expectation["authority"]["portableSourceLockSha256"] = hashlib.sha256(
            changed_raw
        ).hexdigest()

        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            changed_lock = root / "source-lock.json"
            changed_expectation = root / "expectation.json"
            changed_lock.write_bytes(changed_raw)
            changed_expectation.write_bytes(rootfs.canonical_json(expectation))
            with self.assertRaisesRegex(
                portable.PortableAuthorityError, "package closure"
            ):
                portable.load_authority_set(
                    PORTABLE_PLAN,
                    PORTABLE_RESOLUTION,
                    changed_lock,
                    changed_expectation,
                    ROOT / "scripts/native_shadow_rootfs_builder.py",
                )

    def test_runtime_resolution_tool_identity_normalizes_to_one_portable_resolution(self) -> None:
        plan = rootfs.load_json_exact(
            PORTABLE_PLAN.read_bytes(), "portable plan", require_canonical=True
        )
        v1_resolution = {
            "schema": acquire.RESOLUTION_SCHEMA,
            "snapshotId": "20240425T160000Z",
            "snapshotTime": "2024-04-25T16:00:00Z",
            "planSha256": "a" * 64,
            "keyring": {"artifactId": "keyring", "sha256": "b" * 64, "sizeBytes": 1},
            "packages": [],
            "seedPackageIds": [],
        }
        second = copy.deepcopy(v1_resolution)
        second["planSha256"] = "c" * 64

        first_portable = portable.portable_resolution_from_runtime(
            v1_resolution,
            plan,
            PORTABLE_PLAN.read_bytes(),
        )
        second_portable = portable.portable_resolution_from_runtime(
            second,
            plan,
            PORTABLE_PLAN.read_bytes(),
        )
        self.assertEqual(
            rootfs.canonical_json(first_portable),
            rootfs.canonical_json(second_portable),
        )
        self.assertEqual(
            first_portable["bootstrapResolutionV1Sha256"],
            plan["bootstrapAuthority"]["signedResolutionV1Sha256"],
        )

    def test_replay_receipts_bind_builder_and_ephemeral_lock_to_portable_authority(self) -> None:
        authority = portable.load_authority_set(
            PORTABLE_PLAN,
            PORTABLE_RESOLUTION,
            PORTABLE_LOCK,
            REPLAY_EXPECTATION,
            ROOT / "scripts/native_shadow_rootfs_builder.py",
        )
        expectation = authority["expectation"]
        build_receipt = {
            **portable.expected_build_receipt(expectation),
            "activationAllowed": False,
            "productionByteProvenanceComplete": False,
            "builderSha256": expectation["authority"]["builderSha256"],
            "sourceLockSha256": "a" * 64,
        }
        run_receipt = {
            "activationAllowed": False,
            "productionByteProvenanceComplete": False,
            "ephemeralRuntimeLock": True,
            "runtimeLockSha256": "a" * 64,
            "portableSourceLockSha256": expectation["authority"][
                "portableSourceLockSha256"
            ],
            "authority": {
                key: expectation["authority"][key]
                for key in (
                    "builderSha256",
                    "portablePlanSha256",
                    "portableResolutionSha256",
                    "portableSourceLockSha256",
                )
            },
        }
        portable.verify_replay_receipts(expectation, build_receipt, run_receipt)

        changed = copy.deepcopy(build_receipt)
        changed["builderSha256"] = "f" * 64
        with self.assertRaisesRegex(portable.PortableAuthorityError, "builder"):
            portable.verify_replay_receipts(expectation, changed, run_receipt)

    def test_contained_child_setup_reports_the_exact_failed_stage(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        for stage in (
            "derive-runtime-root",
            "mount-private-filesystems",
            "materialize-task",
            "materialize-anchor",
            "materialize-submission",
            "create-scratch",
            "set-working-directory",
            "install-stdio",
            "drop-privileges",
            "verify-privileges",
            "verify-runtime-identity-lookup",
            "apply-rlimits",
            "install-landlock",
            "install-seccomp",
            "exec-checker",
        ):
            self.assertRegex(source, rf'setup_stage\(\s*"{re.escape(stage)}"')

    def test_runtime_root_derivation_reports_the_exact_failed_substage(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        for stage in (
            "derive-make-root-private",
            "derive-check-lower",
            "derive-mount-staging",
            "derive-create-staging",
            "derive-materialize-passwd",
            "derive-verify-fixed-lower-path",
            "derive-bind-lower",
            "derive-remount-lower-read-only",
            "derive-verify-bound-lower",
            "derive-mount-overlay",
            "derive-verify-overlay",
            "derive-enter-overlay",
            "derive-pivot-root",
            "derive-detach-old-root",
            "derive-enter-new-root",
            "derive-verify-entered-root",
        ):
            self.assertRegex(source, rf'setup_stage\(\s*"{re.escape(stage)}"')

    def test_private_filesystem_setup_reports_the_exact_failed_substage(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        for stage in (
            "mount-private-proc",
            "mount-private-work",
            "mount-private-tmp",
            "mount-private-dev",
            "bind-private-dev-null",
            "verify-private-work",
        ):
            self.assertRegex(source, rf'setup_stage\(\s*"{re.escape(stage)}"')

    def test_scratch_keeps_parent_setgid_without_post_create_chmod(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        create = source.split("fn create_scratch", 1)[1].split(
            "fn set_working_directory", 1
        )[0]
        self.assertIn("let previous_umask = unsafe { libc::umask(0) };", create)
        self.assertIn(
            'libc::mkdir(c"/work/scratch".as_ptr(), 0o770)', create
        )
        self.assertIn("libc::umask(previous_umask)", create)
        self.assertNotIn("libc::chmod", create)
        self.assertIn("metadata.st_mode & 0o7777 != 0o2770", create)

    def test_seccomp_allows_only_the_local_socketpair_needed_to_spawn_rustc(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        seccomp = source.split("fn build_seccomp_program", 1)[1].split(
            "fn apply_landlock", 1
        )[0]
        unconditional = seccomp.split("let syscalls = [", 1)[1].split("]", 1)[0]

        self.assertNotIn("libc::SYS_socketpair", unconditional)
        self.assertIn("libc::SYS_socketpair", seccomp)
        self.assertIn("SeccompCmpArgLen::Dword", seccomp)
        self.assertIn("SeccompCmpOp::Ne", seccomp)
        self.assertIn("libc::AF_UNIX as u64", seccomp)
        self.assertIn("libc::SYS_socket,", unconditional)

    def test_checker_child_replaces_service_umask_before_exec(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        setup = source.split("fn child_setup_and_exec", 1)[1].split(
            "fn setup_stage", 1
        )[0]
        self.assertIn('setup_stage("set-checker-umask", set_checker_umask())?', setup)
        self.assertLess(
            setup.index('"set-checker-umask"'), setup.index('"exec-checker"')
        )
        setter = source.split("fn set_checker_umask", 1)[1].split(
            "fn exec_checker", 1
        )[0]
        self.assertIn("libc::umask(CHECKER_UMASK)", setter)
        self.assertIn("CHECKER_UMASK: libc::mode_t = 0o077", source)

    def test_landlock_allows_only_the_verified_dev_null_write_sink(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        landlock = source.split("fn apply_landlock", 1)[1].split(
            "fn set_checker_umask", 1
        )[0]
        self.assertIn('PathFd::new("/dev/null")', landlock)
        self.assertIn("AccessFs::WriteFile", landlock)
        self.assertNotIn('PathFd::new("/dev")', landlock)

    def test_landlock_pins_the_cc1_path_from_the_frozen_ubuntu_package(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        landlock = source.split("fn apply_landlock", 1)[1].split(
            "fn set_checker_umask", 1
        )[0]

        self.assertIn(
            'PathFd::new("/usr/libexec/gcc/x86_64-linux-gnu/13/cc1")',
            landlock,
        )
        self.assertIn(
            'PathFd::new("/usr/libexec/gcc/x86_64-linux-gnu/13/collect2")',
            landlock,
        )
        self.assertNotIn("/usr/lib/gcc-cross/", landlock)

    def test_host_dev_null_is_bound_before_the_old_root_is_detached(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        derive = source.split("fn derive_and_enter_runtime_root", 1)[1].split(
            "fn create_runtime_staging_tree", 1
        )[0]
        self.assertIn('"mount-private-dev"', derive)
        self.assertIn('"bind-private-dev-null"', derive)
        self.assertLess(
            derive.index('"mount-private-dev"'), derive.index('"derive-pivot-root"')
        )
        self.assertLess(
            derive.index('"bind-private-dev-null"'),
            derive.index('"derive-detach-old-root"'),
        )

    def test_private_dev_bind_reopens_dev_null_inside_the_child_namespace(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        bind = source.split("fn bind_and_verify_dev_null", 1)[1].split(
            "fn verify_bound_dev_null", 1
        )[0]
        self.assertNotIn('format!("/proc/self/fd/{source_fd}', bind)
        self.assertIn(
            "let child_local = open_verified_child_dev_null(source_fd)?;", bind
        )
        self.assertIn("let child_local_fd = child_local.as_raw_fd();", bind)
        self.assertIn(
            'format!("/proc/self/fd/{child_local_fd}")',
            bind,
        )
        self.assertLess(
            bind.index("open_verified_child_dev_null(source_fd)?"),
            bind.index('format!("/proc/self/fd/{child_local_fd}")'),
        )
        self.assertLess(
            bind.index('mount_raw(Some(&source), target, None, libc::MS_BIND, None)?'),
            bind.rindex("verify_bound_dev_null(source_fd, target)"),
        )

    def test_overlay_binds_only_the_fixed_path_and_rechecks_the_verified_fd(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        self.assertIn('const RUNTIME_LOWER: &CStr = c"/run/boole/native-shadow/rootfs-lower";', source)
        self.assertIn(
            'const RUNTIME_ADDITIONS: &CStr = '
            'c"/run/boole/native-shadow/rootfs-additions";',
            source,
        )
        self.assertIn(
            'const VERIFIED_RUNTIME_ROOTFS_PATH: &CStr = '
            'c"/var/lib/boole/native-shadow/runtime-rootfs";',
            source,
        )
        self.assertRegex(
            source,
            r'setup_stage\(\s*"derive-verify-fixed-lower-path",\s*'
            r'verify_fixed_runtime_root_path\(rootfs_fd\)',
        )
        self.assertRegex(
            source,
            r'mount_raw\(\s*Some\(VERIFIED_RUNTIME_ROOTFS_PATH\),\s*'
            r'RUNTIME_LOWER,\s*None,\s*libc::MS_BIND',
        )
        self.assertIn("verify_bound_lower(rootfs_fd)", source)
        self.assertIn('"lowerdir={}:{}"', source)
        self.assertIn("RUNTIME_ADDITIONS.to_string_lossy()", source)
        self.assertIn("RUNTIME_LOWER.to_string_lossy()", source)
        self.assertNotIn("upperdir=", source)
        self.assertNotIn("workdir=", source)
        self.assertNotIn("RUNTIME_WORK", source)
        self.assertRegex(
            source,
            r'mount_raw\(\s*Some\(c"overlay"\),\s*RUNTIME_ROOT,\s*'
            r'Some\(c"overlay"\),\s*libc::MS_RDONLY\s*\|\s*libc::MS_NOSUID\s*\|\s*libc::MS_NODEV',
        )
        self.assertNotIn('"lowerdir=/proc/self/fd/{rootfs_fd}', source)
        self.assertNotIn("let lower_source = CString", source)
        self.assertNotIn("libc::SYS_open_tree", source)
        self.assertNotIn("libc::SYS_move_mount", source)
        self.assertNotIn("libc::AT_EMPTY_PATH", source)

    def test_overlay_root_remains_traversable_after_checker_privilege_drop(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("mkdir_fixed(RUNTIME_ADDITIONS, 0o755)?;", source)
        self.assertIn("runtime_root_metadata_is_exact", source)

    def test_runtime_root_materializes_the_dynamic_checker_passwd_before_overlay(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        child = source.split("fn child_setup_and_exec", 1)[1].split(
            "fn setup_stage", 1
        )[0]
        derive = source.split("fn derive_and_enter_runtime_root", 1)[1].split(
            "fn create_runtime_staging_tree", 1
        )[0]
        record = source.split("fn runtime_passwd_record", 1)[1].split(
            "fn materialize_runtime_passwd", 1
        )[0]
        materialize = source.split("fn materialize_runtime_passwd", 1)[1].split(
            "fn verify_runtime_passwd", 1
        )[0]
        verify = source.split("fn verify_runtime_passwd", 1)[1].split(
            "fn verify_fixed_runtime_root_path", 1
        )[0]

        self.assertRegex(
            child,
            r"derive_and_enter_runtime_root\(\s*setup\.rootfs_fd,\s*"
            r"setup\.dev_null_fd,\s*setup\.checker_uid,\s*setup\.checker_gid,?\s*\)",
        )
        self.assertIn('"derive-materialize-passwd"', derive)
        self.assertIn(
            "materialize_runtime_passwd(setup_checker_uid, setup_checker_gid)",
            derive,
        )
        self.assertLess(
            derive.index('"derive-materialize-passwd"'),
            derive.index('"derive-mount-overlay"'),
        )
        self.assertIn("RUNTIME_ADDITIONS_PASSWD", materialize)
        self.assertIn("checker_uid", record)
        self.assertIn("checker_gid", record)
        self.assertIn("/work/scratch", record)
        self.assertIn("libc::O_EXCL", materialize)
        self.assertIn("libc::O_NOFOLLOW", materialize)
        self.assertIn("libc::fchmod", materialize)
        self.assertIn("0o444", materialize)
        self.assertIn("libc::fstat", verify)
        self.assertIn("st_uid != 0", verify)
        self.assertIn("st_gid != 0", verify)
        self.assertIn("st_nlink != 1", verify)
        self.assertIn("runtime passwd metadata mismatch", verify)
        self.assertIn("runtime passwd bytes mismatch", verify)
        self.assertNotIn("VERIFIED_RUNTIME_ROOTFS_PATH", materialize)
        self.assertNotIn("RUNTIME_LOWER", materialize)

        lookup = source.split("fn verify_runtime_identity_lookup", 1)[1].split(
            "fn require_status_ids", 1
        )[0]
        self.assertIn("libc::getpwuid_r", lookup)
        self.assertIn('c"boole-native-checker"', lookup)
        self.assertIn('c"/work/scratch"', lookup)
        self.assertLess(
            child.index('"verify-privileges"'),
            child.index('"verify-runtime-identity-lookup"'),
        )
        self.assertLess(
            child.index('"verify-runtime-identity-lookup"'),
            child.index('"apply-rlimits"'),
        )

    def test_overlay_top_level_permits_only_the_reviewed_etc_overlap(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        compatibility = source.split(
            "fn runtime_additions_are_compatible_with_lower", 1
        )[1].split("fn derived_runtime_top_level_is_exact", 1)[0]
        derived = source.split("fn derived_runtime_top_level_is_exact", 1)[1].split(
            "fn mkdir_fixed", 1
        )[0]

        self.assertIn('OsString::from("etc")', compatibility)
        for exclusive in ("work", "proc", "dev", "tmp"):
            self.assertIn(f'"{exclusive}"', compatibility)
        self.assertIn("runtime_additions_are_compatible_with_lower(lower)", derived)
        self.assertIn('"etc"', derived)

    def test_read_only_overlay_uses_same_dot_pivot_without_old_root_path(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "libc::syscall(libc::SYS_pivot_root, c\".\".as_ptr(), c\".\".as_ptr())",
            source,
        )
        self.assertIn("libc::umount2(c\".\".as_ptr(), libc::MNT_DETACH)", source)
        self.assertNotIn(".old-root", source)
        self.assertNotIn("libc::rmdir", source)

    def test_entered_root_must_match_the_pre_pivot_overlay_identity(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        self.assertRegex(
            source,
            r"let derived_root_identity = setup_stage\(\s*"
            r'"derive-verify-overlay",\s*verify_derived_runtime_root\(\s*'
            r"rootfs_fd,\s*setup_checker_uid,\s*setup_checker_gid,?\s*\),\s*\)\?;",
        )
        self.assertIn(
            "verify_entered_runtime_root(derived_root_identity)",
            source,
        )
        self.assertIn("root.dev() != expected.device", source)
        self.assertIn("root.ino() != expected.inode", source)

    def test_overlay_staging_filesystem_remains_exec_capable_for_the_merged_root(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        staging_mount = re.search(
            r'"derive-mount-staging",\s*mount_raw\((.*?)\),\s*\)\?;',
            source,
            re.DOTALL,
        )
        self.assertIsNotNone(staging_mount)
        self.assertIn("libc::MS_NOSUID | libc::MS_NODEV", staging_mount.group(1))
        self.assertNotIn(
            "libc::MS_NOEXEC",
            staging_mount.group(1),
            "an executable merged root cannot be backed by a noexec upper filesystem",
        )
        self.assertIn("overlay_mount_failure_context", source)

    def test_landlock_does_not_require_an_untracked_bin_alias(self) -> None:
        source = (
            ROOT
            / "crates/boole-native-shadow-launcher/src/per_request_containment/linux.rs"
        ).read_text(encoding="utf-8")
        self.assertNotIn('        "/bin",\n', source)


if __name__ == "__main__":
    unittest.main()
