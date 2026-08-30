#!/usr/bin/env python3
"""Contract tests for the production-only launcher-v2 successor generation."""

from __future__ import annotations

import ast
import hashlib
import inspect
import io
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import tarfile
import types
import unittest
from unittest import mock

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts import native_shadow_successor_produce_phase_arm64_v4 as phase


P3_RELATIVE = (
    "native/containment/native-shadow-mac3-launcher-v2-successor-production-"
    "dispatch-fence-correction-arm64-v1.json"
)



def canonical(document: object) -> bytes:
    return (json.dumps(document, indent=2, sort_keys=True) + "\n").encode()


def identity(root: pathlib.Path, relative: str) -> dict[str, object]:
    raw = (root / relative).read_bytes()
    return {
        "path": relative,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "sizeBytes": len(raw),
    }


def execution_envelope() -> dict[str, object]:
    return {
        "cgroupV2": {
            "equalAtBeforeAndAfterObservations": True,
            "leafControlsKernelObserved": True,
            "limitEventsKernelObserved": True,
            "memoryHighEvents": 0,
            "memoryMaxBytes": 8 * 1024 * 1024 * 1024,
            "memoryMaxEvents": 0,
            "memoryOomEvents": 0,
            "memoryOomKillEvents": 0,
            "memorySwapMaxBytes": 0,
            "pidsMaxEvents": 0,
            "pidsMax": 128,
            "requestedUnitMembershipMatched": True,
        },
        "systemdRuntimeMaxSec": {
            "evidence": "source-pinned-request-and-exact-unit-membership-at-exec",
            "execReachedRequestedUnit": True,
            "kernelObserved": False,
            "managerValueQueried": False,
            "requestedSeconds": 1200,
            "sourcePinnedRequestPresent": True,
        },
    }


def cgroup_observation() -> tuple[str, dict[str, int]]:
    return (
        "/system.slice/boole-nsv4-rehearsal-ABC123.service",
        {
            "memoryHighEvents": 0,
            "memoryMaxBytes": 8 * 1024 * 1024 * 1024,
            "memoryMaxEvents": 0,
            "memoryOomEvents": 0,
            "memoryOomKillEvents": 0,
            "memorySwapMaxBytes": 0,
            "pidsMaxEvents": 0,
            "pidsMax": 128,
        },
    )


def readback_document(root_disk: bytes = b"root-disk") -> dict[str, object]:
    checks = [
        {
            "detail": f"fixture passed {check_id}",
            "id": check_id,
            "ok": True,
        }
        for check_id in (
            "kernel-is-arm64",
            "launcher-digest-matches-seal",
            "launcher-service-is-enabled",
            "modes-owners-and-paths-match-the-lock",
            "pid1-is-systemd",
            "replay-node-absent",
            "runtime-mount-points-present",
        )
    ]
    return {
        "activationAllowed": False,
        "artifactClass": "QUALIFIED-READBACK",
        "bootableClaim": False,
        "entryCount": 17_677,
        "guestBootVerified": False,
        "image": {
            "name": "guest-root-disk",
            "sha256": hashlib.sha256(root_disk).hexdigest(),
            "sizeBytes": len(root_disk),
        },
        "importClosureCorrection": {
            "path": (
                "native/containment/native-shadow-mac3-launcher-v2-successor-"
                "producer-import-closure-correction-arm64-v1.json"
            ),
            "sha256": "b199fb616029e2e38169b4d5f7a82cb7d9962be56fb8bd25dd6b17309131a498",
        },
        "launcherResult": {
            "path": "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
            "sha256": "0ffa4035b8f7f3e698c2ac57eead4b8122cb0c462ab2cb170a87c1973bb01b08",
            "launcherSha256": "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd",
        },
        "mayEnterQualification": True,
        "producerPreregistration": {
            "path": phase.P1_PATH,
            "sha256": phase.P1_SHA256,
        },
        "qualifiedForReplicaComparison": True,
        "release": "NATIVE-SHADOW-SUCCESSOR-ROOT-DISK-READBACK-ARM64-V3",
        "schema": "boole.native-shadow.successor-root-disk-readback.arm64.v3",
        "sourceLock": {
            "path": phase.SOURCE_LOCK_PATH,
            "sha256": phase.SOURCE_LOCK_SHA256,
        },
        "status": phase.READBACK_PASS_STATUS,
        "verification": {
            "activationAllowed": False,
            "bootableClaim": False,
            "checks": checks,
            "guestBootVerified": False,
            "passed": True,
        },
    }


class GenerationChainTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        for relative in (phase.P2_PATH, phase.R1_PATH, phase.F5_PATH):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPOSITORY_ROOT / relative, destination)
        p3_destination = self.root / P3_RELATIVE
        p3_destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(REPOSITORY_ROOT / P3_RELATIVE, p3_destination)
        for relative in (
            phase.P1_PATH,
            phase.SOURCE_LOCK_PATH,
            phase.BUILDER_AUTHORITY_PATH,
        ):
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(REPOSITORY_ROOT / relative, destination)
        for relative in phase.V4_PATHS:
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((relative + "\n").encode())
        for relative in phase.REUSED_PINNED_PATHS:
            destination = self.root / relative
            if not destination.exists():
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(REPOSITORY_ROOT / relative, destination)
        gate = self.root / phase.R2_GATE_PATH
        gate.parent.mkdir(parents=True, exist_ok=True)
        gate.write_bytes(b"fresh-r2-gate\n")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def mock_live_recovery_mount(self, mount_identity: dict[str, object]) -> None:
        patcher = mock.patch.object(
            phase,
            "_read_live_recovery_mount_identity",
            return_value=mount_identity,
        )
        patcher.start()
        self.addCleanup(patcher.stop)

    def mock_absent_recovery_mount(self) -> None:
        patcher = mock.patch.object(
            phase,
            "_read_live_recovery_mount_matches",
            return_value=[],
        )
        patcher.start()
        self.addCleanup(patcher.stop)

    def write_chain(self) -> None:
        p2 = identity(self.root, phase.P2_PATH)
        r1 = identity(self.root, phase.R1_PATH)
        f5 = identity(self.root, phase.F5_PATH)
        p1 = json.loads((self.root / phase.P1_PATH).read_text())
        staging_measurement = p1["expectedPreflight"]["measurement"]
        generation = [identity(self.root, path) for path in phase.V4_PATHS]
        reused = [identity(self.root, path) for path in phase.REUSED_PINNED_PATHS]
        r2 = {
            "activationAllowed": False,
            "authorisations": dict(phase.ZERO_AUTHORISATIONS),
            "bootableClaim": False,
            "boundInputs": [p2, r1, f5, *generation, *reused],
            "effects": dict(phase.ZERO_EFFECTS),
            "executionEnvelope": execution_envelope(),
            "generationFiles": generation,
            "measurement": staging_measurement,
            "predecessors": [p2, r1, f5],
            "productionDispatchFenceCorrection": identity(self.root, P3_RELATIVE),
            "repeatable": True,
            "reusedPinnedUpstream": reused,
            "schema": phase.R2_SCHEMA,
            "status": phase.R2_STATUS,
        }
        r2_path = self.root / phase.R2_PATH
        r2_path.parent.mkdir(parents=True, exist_ok=True)
        r2_path.write_bytes(canonical(r2))
        r2_identity = identity(self.root, phase.R2_PATH)
        f6 = {
            "authorisations": dict(phase.ZERO_AUTHORISATIONS),
            "boundaries": {
                "activationAllowed": False,
                "bootableClaim": False,
                "servingClaim": False,
            },
            "files": generation,
            "predecessors": [p2, r1, f5, r2_identity],
            "productionDispatchFenceCorrection": identity(self.root, P3_RELATIVE),
            "rehearsalGate": identity(self.root, phase.R2_GATE_PATH),
            "schema": phase.F6_SCHEMA,
            "status": phase.F6_STATUS,
            "subject": "Pin the exact production-only v4 generation after fresh R2.",
            "whatThisRecordDoesNotEstablish": [
                "image production authority",
                "guest boot authority",
            ],
        }
        f6_path = self.root / phase.F6_PATH
        f6_path.write_bytes(canonical(f6))
        f6_identity = identity(self.root, phase.F6_PATH)
        a6 = {
            "authorisations": dict(
                phase.ZERO_AUTHORISATIONS,
                imageProductionAuthorised=True,
                imageProductionRunsAllowed=1,
            ),
            "boundaries": {
                "bootableClaim": False,
                "activationAllowed": False,
                "servingClaim": False,
            },
            "grant": {
                "attemptId": "mac3-launcher-v2-successor-v4-attempt-1",
                "outputNames": list(phase.OUTPUT_NAMES),
                "replicas": 2,
                "resultPath": phase.RESULT_V6_PATH,
                "workflowDispatchesAllowed": 1,
                "workflowPath": phase.V4_WORKFLOW_PATH,
            },
            "predecessors": [p2, r1, f5, r2_identity, f6_identity],
            "productionDispatchFenceCorrection": identity(self.root, P3_RELATIVE),
            "runs": dict(phase.ZERO_RUNS, imageProductionRunsAllowed=1),
            "schema": phase.A6_SCHEMA,
            "status": phase.A6_STATUS,
            "subject": "Grant exactly one named production dispatch and no boot.",
        }
        (self.root / phase.A6_PATH).write_bytes(canonical(a6))

    def produce_pending_fixture(self, readback):
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class Backend:
            def prepare(self, request):
                preregistration = json.loads(
                    (self_outer.root / phase.P1_PATH).read_text()
                )
                measurement = preregistration["expectedPreflight"]["measurement"]
                return phase.PreparedProduction(measurement, {}, {})

            def extract_kernel(self, request, prepared):
                (request.outputs / "guest-kernel").write_bytes(b"kernel")
                return {
                    "activationAllowed": False,
                    "bootableClaim": False,
                    "kernel": {
                        "architecture": "aarch64",
                        "magicOffset": 0x38,
                        "name": "guest-kernel",
                        "sha256": hashlib.sha256(b"kernel").hexdigest(),
                        "sizeBytes": len(b"kernel"),
                    },
                }

            def build_initrd(self, request, prepared):
                return b"initrd"

            def build_root_disk(self, request, prepared):
                (request.outputs / "guest-root-disk").write_bytes(b"root-disk")
                return {
                    "activationAllowed": False,
                    "bootableClaim": False,
                    "image": {
                        "name": "guest-root-disk",
                        "sha256": hashlib.sha256(b"root-disk").hexdigest(),
                        "sizeBytes": len(b"root-disk"),
                    },
                }

            def verify_images(self, request, prepared, kernel, initrd, root_disk):
                return readback_document()["verification"]

            def readback(self, repository_root, output_root, chain):
                return readback(repository_root, output_root, chain)

        self_outer = self
        backend = Backend()
        phase.produce(
            repository_root=self.root,
            artifact_store=store,
            outputs=outputs,
            scratch=scratch,
            gpgv=self.root / "gpgv",
            zstd=self.root / "zstd",
            launcher=launcher,
            backend=backend,
            dispatch_capability=self.dispatch_tag_fixture(),
        )
        return outputs, backend

    def qualified_replica_fixture(self) -> tuple[pathlib.Path, pathlib.Path]:
        def persist_readback(repository_root, output_root, chain):
            del repository_root, chain
            document = readback_document()
            (output_root / phase.READBACK_RESULT_NAME).write_bytes(canonical(document))
            return document

        outputs, backend = self.produce_pending_fixture(persist_readback)
        phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=outputs / phase.QUALIFIED_RESULT_NAME,
            backend=backend,
        )
        left = self.root / "replica-1"
        right = self.root / "replica-2"
        outputs.rename(left)
        shutil.copytree(left, right, copy_function=shutil.copyfile)
        return left, right

    def synthetic_dispatch_tag_fixture(self):
        chain = phase.verify_generation_chain(self.root)
        run_id = "33299900001"
        head_sha = "e" * 40
        head_a6_sha256 = chain.identities["A6"].sha256
        claim_ref = phase.dispatch_claim_ref(chain.attempt_id)
        message = phase.dispatch_claim_message(
            chain,
            github_run_id=run_id,
            github_run_attempt="1",
            workflow_path=phase.V4_WORKFLOW_PATH,
            head_sha=head_sha,
            head_a6_sha256=head_a6_sha256,
        )
        short_name = claim_ref.removeprefix("refs/tags/")
        raw_tag = (
            f"object {head_sha}\ntype commit\ntag {short_name}\n"
            "tagger NotoriAndo <281125350+NotoriAndo@users.noreply.github.com> "
            "0 +0000\n\n"
        ).encode("utf-8") + message
        tag_sha = hashlib.sha1(
            b"tag " + str(len(raw_tag)).encode("ascii") + b"\0" + raw_tag
        ).hexdigest()
        return {
            "claim_ref": claim_ref,
            "github_run_id": run_id,
            "github_run_attempt": "1",
            "workflow_path": phase.V4_WORKFLOW_PATH,
            "head_sha": head_sha,
            "head_a6_sha256": head_a6_sha256,
            "ref_object_sha": tag_sha,
            "tag_object_sha": tag_sha,
            "raw_tag_object": raw_tag,
        }

    def dispatch_tag_fixture(self):
        chain = phase.verify_generation_chain(self.root)
        git = "/usr/bin/git"
        if not (self.root / ".git").exists():
            subprocess.run(
                [git, "init", "-q", str(self.root)],
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            subprocess.run(
                [git, "-C", str(self.root), "add", "--", phase.A6_PATH],
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            commit_environment = {
                **os.environ,
                "GIT_AUTHOR_DATE": "1970-01-01T00:00:01Z",
                "GIT_COMMITTER_DATE": "1970-01-01T00:00:01Z",
            }
            subprocess.run(
                [
                    git,
                    "-C",
                    str(self.root),
                    "-c",
                    "user.name=NotoriAndo",
                    "-c",
                    "user.email=281125350+NotoriAndo@users.noreply.github.com",
                    "commit",
                    "-q",
                    "--no-gpg-sign",
                    "-m",
                    "test: bind live A6 fixture",
                ],
                check=True,
                env=commit_environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
        head_sha = subprocess.run(
            [git, "-C", str(self.root), "rev-parse", "HEAD^{commit}"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout.decode("ascii").strip()
        run_id = "33299900001"
        head_a6_sha256 = chain.identities["A6"].sha256
        claim_ref = phase.dispatch_claim_ref(chain.attempt_id)
        message = phase.dispatch_claim_message(
            chain,
            github_run_id=run_id,
            github_run_attempt="1",
            workflow_path=phase.V4_WORKFLOW_PATH,
            head_sha=head_sha,
            head_a6_sha256=head_a6_sha256,
        )
        short_name = claim_ref.removeprefix("refs/tags/")
        raw_tag = (
            f"object {head_sha}\ntype commit\ntag {short_name}\n"
            "tagger NotoriAndo <281125350+NotoriAndo@users.noreply.github.com> "
            "0 +0000\n\n"
        ).encode("utf-8") + message
        tag_sha = subprocess.run(
            [git, "-C", str(self.root), "hash-object", "-t", "tag", "-w", "--stdin"],
            input=raw_tag,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        ).stdout.decode("ascii").strip()
        subprocess.run(
            [git, "-C", str(self.root), "update-ref", claim_ref, tag_sha],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        return {
            "claim_ref": claim_ref,
            "github_run_id": run_id,
            "github_run_attempt": "1",
            "workflow_path": phase.V4_WORKFLOW_PATH,
            "head_sha": head_sha,
            "head_a6_sha256": head_a6_sha256,
            "ref_object_sha": tag_sha,
            "tag_object_sha": tag_sha,
            "raw_tag_object": raw_tag,
        }

    def provenanced_replica_bundles(self):
        left, right = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        bundles = []
        for ordinal, source in ((1, left), (2, right)):
            bundle = self.root / f"bundle-{ordinal}"
            bundle.mkdir()
            outputs = bundle / "outputs"
            source.rename(outputs)
            document = phase.replica_provenance_document(
                repository_root=self.root,
                outputs=outputs,
                replica_ordinal=ordinal,
                strategy_job_index=ordinal - 1,
                strategy_job_total=2,
                github_job="produce",
                artifact_name=f"native-shadow-successor-v4-replica-{ordinal}",
                **dispatch,
            )
            (bundle / phase.REPLICA_PROVENANCE_NAME).write_bytes(
                canonical(document)
            )
            bundles.append(bundle)
        return bundles[0], bundles[1], dispatch

    def publish_and_seal_fixture(
        self,
        parent: pathlib.Path,
        outputs: pathlib.Path,
        dispatch,
        *,
        parent_identity: tuple[int, int] | None = None,
    ):
        info = parent.stat()
        return phase.publish_and_seal_replica_bundle(
            parent=parent,
            expected_parent_identity=(info.st_dev, info.st_ino)
            if parent_identity is None
            else parent_identity,
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            result=parent / phase.REPLICA_PROVENANCE_NAME,
            repository_root=self.root,
            outputs=outputs,
            replica_ordinal=1,
            strategy_job_index=0,
            strategy_job_total=2,
            github_job="produce",
            artifact_name=phase.REPLICA_ARTIFACT_PREFIX + "1",
            **dispatch,
        )

    def test_strict_chain_reaches_one_named_unrun_authority(self) -> None:
        self.write_chain()

        chain = phase.verify_generation_chain(self.root)

        self.assertEqual(
            chain.attempt_id, "mac3-launcher-v2-successor-v4-attempt-1"
        )
        self.assertEqual(chain.output_names, phase.OUTPUT_NAMES)
        self.assertEqual(chain.authority["runs"]["imageProductionRunsPerformed"], 0)

    def test_preregistered_generation_requires_the_dispatch_fence_correction(self) -> None:
        (self.root / P3_RELATIVE).unlink()

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "P3"):
            phase.verify_preregistered_generation(self.root)

    def test_preregistered_generation_rejects_a_tampered_dispatch_fence(self) -> None:
        correction = self.root / P3_RELATIVE
        correction.write_bytes(correction.read_bytes() + b"\n")

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "P3"):
            phase.verify_preregistered_generation(self.root)

    def test_r2_must_directly_bind_the_dispatch_fence_correction(self) -> None:
        self.write_chain()
        r2_path = self.root / phase.R2_PATH
        r2 = json.loads(r2_path.read_text())
        del r2["productionDispatchFenceCorrection"]
        r2_path.write_bytes(canonical(r2))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "R2 keys differ"
        ):
            phase.verify_generation_chain(self.root)

    def test_r2_rejects_a_requested_cap_masquerading_as_kernel_observation(
        self,
    ) -> None:
        self.write_chain()
        r2_path = self.root / phase.R2_PATH
        r2 = json.loads(r2_path.read_text())
        r2["executionEnvelope"]["cgroupV2"]["memoryMaxBytes"] = (
            "requested:8589934592"
        )
        r2_path.write_bytes(canonical(r2))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "R2 execution envelope"
        ):
            phase.verify_generation_chain(self.root)

    def test_live_cgroup_v2_caps_are_read_from_the_kernel_files_not_arguments(
        self,
    ) -> None:
        cgroup_root = self.root / "cgroup"
        unit_name = "boole-nsv4-rehearsal-ABC123.service"
        unit = cgroup_root / "system.slice" / unit_name
        unit.mkdir(parents=True)
        (unit / "memory.max").write_text("8589934592\n", encoding="ascii")
        (unit / "memory.swap.max").write_text("0\n", encoding="ascii")
        (unit / "memory.events.local").write_text(
            "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n",
            encoding="ascii",
        )
        (unit / "pids.max").write_text("128\n", encoding="ascii")
        (unit / "pids.events").write_text("max 0\n", encoding="ascii")
        proc_cgroup = self.root / "proc-self-cgroup"
        proc_cgroup.write_text(f"0::/system.slice/{unit_name}\n", encoding="ascii")
        mountinfo = self.root / "mountinfo"
        mountinfo.write_text(
            f"29 23 0:26 / {cgroup_root} rw,nosuid,nodev,noexec - cgroup2 cgroup rw\n",
            encoding="ascii",
        )

        observed = phase._read_cgroup_execution_observation(
            expected_systemd_unit=unit_name,
            proc_cgroup_path=proc_cgroup,
            cgroup_root=cgroup_root,
            mountinfo_path=mountinfo,
        )

        self.assertEqual(
            observed,
            (
                f"/system.slice/{unit_name}",
                {
                    "memoryHighEvents": 0,
                    "memoryMaxBytes": 8 * 1024 * 1024 * 1024,
                    "memoryMaxEvents": 0,
                    "memoryOomEvents": 0,
                    "memoryOomKillEvents": 0,
                    "memorySwapMaxBytes": 0,
                    "pidsMaxEvents": 0,
                    "pidsMax": 128,
                },
            ),
        )

    def test_live_cgroup_v2_caps_reject_unlimited_mismatched_and_symlinked_values(
        self,
    ) -> None:
        for case in ("unlimited", "mismatch", "symlink"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root = pathlib.Path(temporary)
                cgroup_root = root / "cgroup"
                unit_name = "boole-nsv4-rehearsal-ABC123.service"
                unit = cgroup_root / "system.slice" / unit_name
                unit.mkdir(parents=True)
                (unit / "memory.max").write_text("8589934592\n", encoding="ascii")
                (unit / "memory.swap.max").write_text("0\n", encoding="ascii")
                (unit / "memory.events.local").write_text(
                    "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n"
                    "oom_group_kill 0\n",
                    encoding="ascii",
                )
                (unit / "pids.max").write_text("128\n", encoding="ascii")
                (unit / "pids.events").write_text("max 0\n", encoding="ascii")
                proc_cgroup = root / "proc-self-cgroup"
                proc_cgroup.write_text(
                    f"0::/system.slice/{unit_name}\n", encoding="ascii"
                )
                mountinfo = root / "mountinfo"
                mountinfo.write_text(
                    f"29 23 0:26 / {cgroup_root} rw,nosuid,nodev,noexec - cgroup2 cgroup rw\n",
                    encoding="ascii",
                )
                if case == "unlimited":
                    (unit / "memory.max").write_text("max\n", encoding="ascii")
                elif case == "mismatch":
                    (unit / "pids.max").write_text("129\n", encoding="ascii")
                else:
                    target = root / "foreign-memory-max"
                    target.write_text("8589934592\n", encoding="ascii")
                    (unit / "memory.max").unlink()
                    (unit / "memory.max").symlink_to(target)

                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "cgroup|memory|max|pids|symlink|limit",
                ):
                    phase._read_cgroup_execution_observation(
                        expected_systemd_unit=unit_name,
                        proc_cgroup_path=proc_cgroup,
                        cgroup_root=cgroup_root,
                        mountinfo_path=mountinfo,
                    )

    def test_live_cgroup_v2_caps_reject_any_memory_or_pid_limit_event(
        self,
    ) -> None:
        for event_file, event_name in (
            ("memory.events.local", "high"),
            ("memory.events.local", "max"),
            ("memory.events.local", "oom"),
            ("memory.events.local", "oom_kill"),
            ("pids.events", "max"),
        ):
            with self.subTest(event_file=event_file, event_name=event_name), tempfile.TemporaryDirectory() as temporary:
                root = pathlib.Path(temporary)
                cgroup_root = root / "cgroup"
                unit_name = "boole-nsv4-rehearsal-ABC123.service"
                unit = cgroup_root / "system.slice" / unit_name
                unit.mkdir(parents=True)
                (unit / "memory.max").write_text("8589934592\n", encoding="ascii")
                (unit / "memory.swap.max").write_text("0\n", encoding="ascii")
                memory_events = {
                    "low": 0,
                    "high": 0,
                    "max": 0,
                    "oom": 0,
                    "oom_kill": 0,
                    "oom_group_kill": 0,
                }
                pids_events = {"max": 0}
                if event_file == "memory.events.local":
                    memory_events[event_name] = 1
                else:
                    pids_events[event_name] = 1
                (unit / "memory.events.local").write_text(
                    "".join(f"{name} {value}\n" for name, value in memory_events.items()),
                    encoding="ascii",
                )
                (unit / "pids.max").write_text("128\n", encoding="ascii")
                (unit / "pids.events").write_text(
                    "".join(f"{name} {value}\n" for name, value in pids_events.items()),
                    encoding="ascii",
                )
                proc_cgroup = root / "proc-self-cgroup"
                proc_cgroup.write_text(
                    f"0::/system.slice/{unit_name}\n", encoding="ascii"
                )
                mountinfo = root / "mountinfo"
                mountinfo.write_text(
                    f"29 23 0:26 / {cgroup_root} rw,nosuid,nodev,noexec - cgroup2 cgroup rw\n",
                    encoding="ascii",
                )

                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "limit event",
                ):
                    phase._read_cgroup_execution_observation(
                        expected_systemd_unit=unit_name,
                        proc_cgroup_path=proc_cgroup,
                        cgroup_root=cgroup_root,
                        mountinfo_path=mountinfo,
                    )

    def test_a6_attempt_id_must_match_the_dispatch_fence_pattern(self) -> None:
        self.write_chain()
        authority_path = self.root / phase.A6_PATH
        authority = json.loads(authority_path.read_text())
        authority["grant"]["attemptId"] = (
            "MAC3-LAUNCHER-V2-SUCCESSOR-V4-ATTEMPT-1"
        )
        authority_path.write_bytes(canonical(authority))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "A6 attempt ID"
        ):
            phase.verify_generation_chain(self.root)

    def test_a6_attempt_id_honours_the_exact_length_and_character_boundary(self) -> None:
        self.write_chain()
        authority_path = self.root / phase.A6_PATH
        authority = json.loads(authority_path.read_text())
        authority["grant"]["attemptId"] = "a" * 128
        authority_path.write_bytes(canonical(authority))
        self.assertEqual(
            phase.verify_generation_chain(self.root).attempt_id, "a" * 128
        )

        for invalid in ("a" * 129, ".a", "a/", "a\n", "A"):
            with self.subTest(invalid=repr(invalid)):
                self.write_chain()
                authority = json.loads(authority_path.read_text())
                authority["grant"]["attemptId"] = invalid
                authority_path.write_bytes(canonical(authority))
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "A6 attempt ID"
                ):
                    phase.verify_generation_chain(self.root)

    def test_f6_must_directly_bind_the_dispatch_fence_correction(self) -> None:
        self.write_chain()
        f6_path = self.root / phase.F6_PATH
        f6 = json.loads(f6_path.read_text())
        del f6["productionDispatchFenceCorrection"]
        f6_path.write_bytes(canonical(f6))

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "F6 keys differ"):
            phase.verify_generation_chain(self.root)

    def test_a6_must_directly_bind_the_dispatch_fence_correction(self) -> None:
        self.write_chain()
        a6_path = self.root / phase.A6_PATH
        a6 = json.loads(a6_path.read_text())
        del a6["productionDispatchFenceCorrection"]
        a6_path.write_bytes(canonical(a6))

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "A6 keys differ"):
            phase.verify_generation_chain(self.root)

    def test_each_future_record_requires_the_exact_p3_identity_shape(self) -> None:
        for label, relative in (
            ("R2", phase.R2_PATH),
            ("F6", phase.F6_PATH),
            ("A6", phase.A6_PATH),
        ):
            with self.subTest(label=label):
                self.write_chain()
                path = self.root / relative
                document = json.loads(path.read_text())
                document["productionDispatchFenceCorrection"]["extra"] = False
                path.write_bytes(canonical(document))
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    f"{label} dispatch-fence correction differs",
                ):
                    phase.verify_generation_chain(self.root)

    def test_dispatch_claim_is_exact_canonical_and_bound_to_live_a6(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        run_id = "33299900001"
        run_attempt = "1"
        workflow_path = phase.V4_WORKFLOW_PATH
        head_sha = "a" * 40
        head_a6_sha256 = chain.identities["A6"].sha256

        document = phase.dispatch_claim_document(
            chain,
            github_run_id=run_id,
            github_run_attempt=run_attempt,
            workflow_path=workflow_path,
            head_sha=head_sha,
            head_a6_sha256=head_a6_sha256,
        )
        expected = {
            "a6Sha256": chain.identities["A6"].sha256,
            "attemptId": chain.attempt_id,
            "githubRunId": run_id,
            "headSha": head_sha,
            "schema": (
                "boole.native-shadow.mac3.successor-production-dispatch-"
                "claim.arm64.v1"
            ),
            "workflowPath": phase.V4_WORKFLOW_PATH,
        }
        self.assertEqual(document, expected)
        raw = phase.dispatch_claim_message(
            chain,
            github_run_id=run_id,
            github_run_attempt=run_attempt,
            workflow_path=workflow_path,
            head_sha=head_sha,
            head_a6_sha256=head_a6_sha256,
        )
        self.assertEqual(
            raw,
            json.dumps(expected, sort_keys=True, separators=(",", ":")).encode(),
        )
        self.assertFalse(raw.endswith(b"\n"))
        self.assertEqual(
            phase.verify_dispatch_claim(
                chain,
                claim_ref=phase.dispatch_claim_ref(chain.attempt_id),
                raw_message=raw,
                github_run_id=run_id,
                github_run_attempt=run_attempt,
                workflow_path=workflow_path,
                head_sha=head_sha,
                head_a6_sha256=head_a6_sha256,
            ),
            expected,
        )

    def test_dispatch_claim_rejects_git_ref_liveness_traps(self) -> None:
        for attempt_id in ("a..b", "a.lock", "a."):
            with self.subTest(attempt_id=attempt_id):
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "Git tag"
                ):
                    phase.dispatch_claim_ref(attempt_id)

    def test_dispatch_claim_rejects_message_or_runtime_context_drift(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        run_id = "33299900001"
        head_sha = "b" * 40
        head_a6_sha256 = chain.identities["A6"].sha256
        raw = phase.dispatch_claim_message(
            chain,
            github_run_id=run_id,
            github_run_attempt="1",
            workflow_path=phase.V4_WORKFLOW_PATH,
            head_sha=head_sha,
            head_a6_sha256=head_a6_sha256,
        )
        mutations = (
            (raw + b"\n", run_id, "1", phase.V4_WORKFLOW_PATH, head_sha, head_a6_sha256),
            (raw, "33299900002", "1", phase.V4_WORKFLOW_PATH, head_sha, head_a6_sha256),
            (raw, run_id, "2", phase.V4_WORKFLOW_PATH, head_sha, head_a6_sha256),
            (raw, run_id, "1", ".github/workflows/other.yml", head_sha, head_a6_sha256),
            (raw, run_id, "1", phase.V4_WORKFLOW_PATH, "c" * 40, head_a6_sha256),
            (raw, run_id, "1", phase.V4_WORKFLOW_PATH, head_sha, "d" * 64),
        )
        for message, observed_run, attempt, workflow, observed_head, observed_a6 in mutations:
            with self.subTest(run=observed_run, attempt=attempt, workflow=workflow):
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "dispatch claim"
                ):
                    phase.verify_dispatch_claim(
                        chain,
                        claim_ref=phase.dispatch_claim_ref(chain.attempt_id),
                        raw_message=message,
                        github_run_id=observed_run,
                        github_run_attempt=attempt,
                        workflow_path=workflow,
                        head_sha=observed_head,
                        head_a6_sha256=observed_a6,
                    )

    def test_dispatch_claim_cli_emits_exact_bytes_without_newline_or_effect(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        run_id = "33299900001"
        head_sha = "d" * 40
        head_a6_sha256 = chain.identities["A6"].sha256
        before = {
            path.relative_to(self.root).as_posix(): hashlib.sha256(
                path.read_bytes()
            ).hexdigest()
            for path in self.root.rglob("*")
            if path.is_file()
        }
        output = types.SimpleNamespace(buffer=io.BytesIO())

        with mock.patch.object(phase.sys, "stdout", output):
            code = phase.main(
                [
                    "dispatch-claim-message",
                    "--repository-root",
                    str(self.root),
                    "--github-run-id",
                    run_id,
                    "--github-run-attempt",
                    "1",
                    "--workflow-path",
                    phase.V4_WORKFLOW_PATH,
                    "--head-sha",
                    head_sha,
                    "--head-a6-sha256",
                    head_a6_sha256,
                ]
            )

        self.assertEqual(code, 0)
        self.assertEqual(
            output.buffer.getvalue(),
            phase.dispatch_claim_message(
                chain,
                github_run_id=run_id,
                github_run_attempt="1",
                workflow_path=phase.V4_WORKFLOW_PATH,
                head_sha=head_sha,
                head_a6_sha256=head_a6_sha256,
            ),
        )
        self.assertFalse(output.buffer.getvalue().endswith(b"\n"))
        after = {
            path.relative_to(self.root).as_posix(): hashlib.sha256(
                path.read_bytes()
            ).hexdigest()
            for path in self.root.rglob("*")
            if path.is_file()
        }
        self.assertEqual(after, before)

    def test_dispatch_claim_verifier_binds_ref_tag_object_and_target_commit(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        dispatch = self.dispatch_tag_fixture()
        run_id = dispatch["github_run_id"]
        head_sha = dispatch["head_sha"]
        head_a6_sha256 = dispatch["head_a6_sha256"]
        claim_ref = dispatch["claim_ref"]
        raw_tag = dispatch["raw_tag_object"]
        tag_sha = dispatch["tag_object_sha"]
        short_name = claim_ref.removeprefix("refs/tags/")
        stdin = types.SimpleNamespace(buffer=io.BytesIO(raw_tag))
        accepted = io.StringIO()
        with mock.patch.object(phase.sys, "stdin", stdin), mock.patch.object(
            phase.sys, "stdout", accepted
        ):
            accepted_code = phase.main(
                [
                    "dispatch-claim-verify",
                    "--repository-root",
                    str(self.root),
                    "--claim-ref",
                    claim_ref,
                    "--ref-object-sha",
                    tag_sha,
                    "--tag-object-sha",
                    tag_sha,
                    "--github-run-id",
                    run_id,
                    "--github-run-attempt",
                    "1",
                    "--workflow-path",
                    phase.V4_WORKFLOW_PATH,
                    "--head-sha",
                    head_sha,
                    "--head-a6-sha256",
                    head_a6_sha256,
                ]
            )
        self.assertEqual(accepted_code, 0)
        self.assertIn("dispatch claim verified", accepted.getvalue())

        mutations = (
            (claim_ref, tag_sha, "f" * 40, raw_tag),
            (claim_ref, "f" * 40, tag_sha, raw_tag),
            (claim_ref, tag_sha, tag_sha, raw_tag.replace(b"type commit", b"type blob")),
            (claim_ref, tag_sha, tag_sha, raw_tag.replace(short_name.encode(), b"other")),
            (claim_ref, tag_sha, tag_sha, raw_tag.replace(head_sha.encode(), ("9" * 40).encode(), 1)),
        )
        for observed_ref, ref_sha, object_sha, observed_raw in mutations:
            with self.subTest(ref=observed_ref, ref_sha=ref_sha, object_sha=object_sha):
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "dispatch claim"
                ):
                    phase.verify_dispatch_tag_object(
                        chain,
                        repository_root=self.root,
                        claim_ref=observed_ref,
                        ref_object_sha=ref_sha,
                        tag_object_sha=object_sha,
                        raw_tag_object=observed_raw,
                        github_run_id=run_id,
                        github_run_attempt="1",
                        workflow_path=phase.V4_WORKFLOW_PATH,
                        head_sha=head_sha,
                        head_a6_sha256=head_a6_sha256,
                    )

    def test_dispatch_claim_rejects_self_consistent_tag_without_live_ref(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        synthetic = self.synthetic_dispatch_tag_fixture()

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "dispatch claim.*(?:repository|live|ref)",
        ):
            phase.verify_dispatch_tag_object(
                chain,
                repository_root=self.root,
                **synthetic,
            )

    def test_dispatch_claim_rejects_a_symlinked_repository_root(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        dispatch = self.dispatch_tag_fixture()
        linked_root = pathlib.Path(self.temporary.name) / "linked-repository-root"
        linked_root.symlink_to(self.root, target_is_directory=True)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "dispatch claim live repository (?:root )?differs",
        ):
            phase.verify_dispatch_tag_object(
                chain,
                repository_root=linked_root,
                **dispatch,
            )

    def test_dispatch_git_reader_is_truly_bounded_while_the_child_runs(self) -> None:
        source = pathlib.Path(phase.__file__).read_text(encoding="utf-8")
        start = source.index("def _git_dispatch_read(")
        end = source.index("\ndef _git_dispatch_scalar(", start)
        reader = source[start:end]

        self.assertIn("subprocess.Popen(", reader)
        self.assertIn("selectors.DefaultSelector()", reader)
        self.assertIn("time.monotonic()", reader)
        self.assertIn("process.kill()", reader)
        self.assertNotIn("subprocess.run(", reader)

    def test_dispatch_claim_rejects_repository_replacement_mid_verification(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        dispatch = self.dispatch_tag_fixture()
        displaced = self.root.with_name(self.root.name + "-displaced-live-repository")
        self.addCleanup(shutil.rmtree, displaced, True)
        real_scalar = phase._git_dispatch_scalar
        replaced = False

        def replace_after_head(repository_root, arguments, context):
            nonlocal replaced
            value = real_scalar(repository_root, arguments, context)
            if context == "HEAD" and not replaced:
                replaced = True
                self.root.rename(displaced)
                shutil.copytree(displaced, self.root, copy_function=shutil.copyfile)
            return value

        with mock.patch.object(
            phase, "_git_dispatch_scalar", side_effect=replace_after_head
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "dispatch claim live repository changed",
            ):
                phase.verify_dispatch_tag_object(
                    chain,
                    repository_root=self.root,
                    **dispatch,
                )

        self.assertTrue(replaced)

    def test_production_requires_dispatch_capability_before_backend_prepare(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class NeverPrepare:
            calls = 0

            def prepare(self, request):
                self.calls += 1
                raise AssertionError("backend.prepare ran without a dispatch capability")

        backend = NeverPrepare()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "dispatch capability"
        ):
            phase.produce(
                repository_root=self.root,
                artifact_store=store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=backend,
            )

        self.assertEqual(backend.calls, 0)
        self.assertFalse(outputs.exists())

    def test_production_rejects_synthetic_dispatch_before_backend_prepare(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs-synthetic-dispatch"
        scratch = self.root / "scratch-synthetic-dispatch"
        store = self.root / "cas-synthetic-dispatch"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher-synthetic-dispatch"
        launcher.write_bytes(b"launcher")

        class NeverPrepare:
            calls = 0

            def prepare(self, request):
                self.calls += 1
                raise AssertionError("backend.prepare ran for a synthetic Git tag")

        backend = NeverPrepare()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "dispatch claim.*(?:repository|live|ref)",
        ):
            phase.produce(
                repository_root=self.root,
                artifact_store=store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=backend,
                dispatch_capability=self.synthetic_dispatch_tag_fixture(),
            )

        self.assertEqual(backend.calls, 0)
        self.assertFalse(outputs.exists())

    def test_real_backend_requires_root_before_scratch_or_output_effects(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs-nonroot"
        scratch = self.root / "scratch-nonroot"
        store = self.root / "cas-nonroot"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher-nonroot"
        launcher.write_bytes(b"launcher")

        with mock.patch.object(phase.os, "geteuid", return_value=501):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "real production backend requires root",
            ):
                phase.produce(
                    repository_root=self.root,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=self.root / "gpgv",
                    zstd=self.root / "zstd",
                    launcher=launcher,
                    dispatch_capability=self.dispatch_tag_fixture(),
                )

        self.assertFalse(outputs.exists())
        self.assertEqual(tuple(scratch.iterdir()), ())

    def test_production_cli_cannot_parse_without_the_exact_dispatch_context(self) -> None:
        with self.assertRaises(SystemExit):
            phase._parser().parse_args(
                [
                    "produce",
                    "--cas",
                    "/tmp/cas",
                    "--launcher",
                    "/tmp/launcher",
                    "--scratch",
                    "/tmp/scratch",
                    "--gpgv",
                    "/usr/bin/gpgv",
                    "--zstd",
                    "/usr/bin/zstd",
                    "--result",
                    "/tmp/outputs/PRODUCE-RESULT-PENDING-READBACK-V4.json",
                    "--outputs",
                    "/tmp/outputs",
                ]
            )

    def test_production_rechecks_dispatch_capability_immediately_before_marker(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")
        dispatch = self.dispatch_tag_fixture()

        class PreparedOnly:
            calls = 0

            def prepare(self, request):
                self.calls += 1
                return phase.PreparedProduction({}, {}, {})

        backend = PreparedOnly()
        real_verify = phase.verify_dispatch_tag_object
        verifications = 0

        def expire_before_marker(*args, **kwargs):
            nonlocal verifications
            verifications += 1
            if verifications == 2:
                raise phase.SuccessorProduceV4Error(
                    "dispatch capability expired before marker"
                )
            return real_verify(*args, **kwargs)

        with mock.patch.object(
            phase, "verify_dispatch_tag_object", side_effect=expire_before_marker
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "expired before marker"
            ):
                phase.produce(
                    repository_root=self.root,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=self.root / "gpgv",
                    zstd=self.root / "zstd",
                    launcher=launcher,
                    backend=backend,
                    dispatch_capability=dispatch,
                )

        self.assertEqual(backend.calls, 1)
        self.assertEqual(verifications, 2)
        self.assertFalse(outputs.exists())

    def test_production_rechecks_the_live_ref_immediately_before_marker(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs-live-ref-race"
        scratch = self.root / "scratch-live-ref-race"
        store = self.root / "cas-live-ref-race"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher-live-ref-race"
        launcher.write_bytes(b"launcher")
        dispatch = self.dispatch_tag_fixture()

        class RefDeletingBackend:
            calls = 0

            def prepare(inner_self, request):
                inner_self.calls += 1
                subprocess.run(
                    [
                        "/usr/bin/git",
                        "-C",
                        str(self.root),
                        "update-ref",
                        "-d",
                        dispatch["claim_ref"],
                    ],
                    check=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                return phase.PreparedProduction({}, {}, {})

        backend = RefDeletingBackend()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "dispatch claim live repository ref differs",
        ):
            phase.produce(
                repository_root=self.root,
                artifact_store=store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=backend,
                dispatch_capability=dispatch,
            )

        self.assertEqual(backend.calls, 1)
        self.assertFalse(outputs.exists())

    def test_production_marks_before_images_and_qualifies_only_after_readback(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        artifact_store = self.root / "cas"
        scratch.mkdir()
        artifact_store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher-v2")
        events: list[str] = []
        preregistration = json.loads((self.root / phase.P1_PATH).read_text())
        staging_measurement = preregistration["expectedPreflight"]["measurement"]

        class FakeBackend:
            def prepare(self, request):
                self.assert_output_absent = not request.outputs.exists()
                events.append("prepare")
                return phase.PreparedProduction(
                    measurement=staging_measurement,
                    build_receipt={"layerDigest": "sha256:" + "1" * 64},
                    state={"lock": "sealed"},
                )

            def extract_kernel(self, request, prepared):
                self.assert_marker = (request.outputs / phase.CONSUMED_MARKER_NAME).is_file()
                events.append("kernel")
                (request.outputs / "guest-kernel").write_bytes(b"kernel")
                return {
                    "activationAllowed": False,
                    "bootableClaim": False,
                    "kernel": {
                        "architecture": "aarch64",
                        "magicOffset": 0x38,
                        "name": "guest-kernel",
                        "sha256": hashlib.sha256(b"kernel").hexdigest(),
                        "sizeBytes": len(b"kernel"),
                    },
                }

            def build_initrd(self, request, prepared):
                events.append("initrd")
                return b"initrd"

            def build_root_disk(self, request, prepared):
                events.append("root-disk")
                (request.outputs / "guest-root-disk").write_bytes(b"root-disk")
                return {
                    "activationAllowed": False,
                    "bootableClaim": False,
                    "image": {
                        "name": "guest-root-disk",
                        "sha256": hashlib.sha256(b"root-disk").hexdigest(),
                        "sizeBytes": len(b"root-disk"),
                    },
                }

            def verify_images(self, request, prepared, kernel, initrd, root_disk):
                events.append("verify")
                return readback_document()["verification"]

            def readback(self, repository_root, outputs, chain):
                self.assert_pending = (outputs / phase.PENDING_RESULT_NAME).is_file()
                events.append("readback")
                return readback_document()

        backend = FakeBackend()
        real_verify_dispatch = phase.verify_dispatch_tag_object

        def observe_dispatch(*args, **kwargs):
            events.append("dispatch")
            return real_verify_dispatch(*args, **kwargs)

        with mock.patch.object(
            phase,
            "verify_dispatch_tag_object",
            side_effect=observe_dispatch,
        ):
            pending = phase.produce(
                repository_root=self.root,
                artifact_store=artifact_store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=backend,
                dispatch_capability=self.dispatch_tag_fixture(),
            )

        self.assertTrue(backend.assert_output_absent)
        self.assertTrue(backend.assert_marker)
        self.assertEqual(
            events,
            [
                "dispatch",
                "prepare",
                "dispatch",
                "kernel",
                "initrd",
                "root-disk",
                "verify",
            ],
        )
        self.assertEqual(pending["status"], phase.PRODUCTION_PENDING_STATUS)
        self.assertFalse(pending["qualifiedForReplicaComparison"])

        result_path = outputs / "PRODUCE-RESULT.json"
        outcome = phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=result_path,
            backend=backend,
        )

        self.assertTrue(backend.assert_pending)
        self.assertEqual(events[-1], "readback")
        self.assertEqual(outcome["status"], phase.PRODUCTION_QUALIFIED_STATUS)
        self.assertTrue(outcome["qualifiedForReplicaComparison"])
        self.assertTrue(result_path.is_file())

        resumed = phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=result_path,
            backend=backend,
        )
        self.assertEqual(resumed, outcome)
        self.assertEqual(events.count("readback"), 1)

    def test_readback_pass_is_bound_to_current_root_disk_identity(self) -> None:
        document = readback_document()
        document["image"]["sha256"] = "0" * 64
        outputs, backend = self.produce_pending_fixture(
            lambda repository_root, output_root, chain: document
        )

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "readback-v3 image identity differs"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=outputs / phase.QUALIFIED_RESULT_NAME,
                backend=backend,
            )

        self.assertFalse((outputs / phase.QUALIFIED_RESULT_NAME).exists())

    def test_readback_rejects_empty_verification_check_set(self) -> None:
        document = readback_document()
        document["verification"]["checks"] = []
        outputs, backend = self.produce_pending_fixture(
            lambda repository_root, output_root, chain: document
        )

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "verification check count differs"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=outputs / phase.QUALIFIED_RESULT_NAME,
                backend=backend,
            )

        self.assertFalse((outputs / phase.QUALIFIED_RESULT_NAME).exists())

    def test_readback_rejects_entry_count_not_derived_from_sealed_staging(self) -> None:
        document = readback_document()
        document["entryCount"] = 17_676
        outputs, backend = self.produce_pending_fixture(
            lambda repository_root, output_root, chain: document
        )

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "readback-v3 entry count differs"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=outputs / phase.QUALIFIED_RESULT_NAME,
                backend=backend,
            )

        self.assertFalse((outputs / phase.QUALIFIED_RESULT_NAME).exists())

    def test_readback_verification_requires_exact_seven_rows(self) -> None:
        mutations = {
            "missing": lambda checks: checks[:-1],
            "extra": lambda checks: [
                *checks,
                {"detail": "extra", "id": "unexpected-extra", "ok": True},
            ],
            "wrong-id": lambda checks: [
                {**checks[0], "id": "wrong-check-id"}, *checks[1:]
            ],
            "integer-ok": lambda checks: [
                {**checks[0], "ok": 1}, *checks[1:]
            ],
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                document = readback_document()
                checks = document["verification"]["checks"]
                document["verification"]["checks"] = mutate(checks)
                root_identity = phase.FileIdentity(
                    "guest-root-disk",
                    hashlib.sha256(b"root-disk").hexdigest(),
                    len(b"root-disk"),
                )
                with self.assertRaises(phase.SuccessorProduceV4Error):
                    phase._assert_readback_pass(document, root_identity, 17_677)

    def test_output_pin_checks_each_of_the_three_image_names(self) -> None:
        for target in phase.OUTPUT_NAMES:
            with self.subTest(target=target):
                output_root = self.root / f"pin-{target}"
                output_root.mkdir()
                for name in phase.OUTPUT_NAMES:
                    (output_root / name).write_bytes(name.encode())
                with phase._pinned_outputs(output_root, phase.OUTPUT_NAMES) as pinned:
                    (output_root / target).write_bytes(b"changed")
                    with self.assertRaisesRegex(
                        phase.SuccessorProduceV4Error,
                        "qualification output identity changed",
                    ):
                        phase._assert_pinned_outputs_unchanged(output_root, pinned)

    def test_output_pin_rejects_two_image_names_for_one_inode(self) -> None:
        output_root = self.root / "pin-hardlink-alias"
        output_root.mkdir()
        (output_root / "guest-kernel").write_bytes(b"shared-image")
        os.link(
            output_root / "guest-kernel", output_root / "guest-initrd"
        )
        (output_root / "guest-root-disk").write_bytes(b"root-disk")

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "distinct inodes"
        ):
            with phase._pinned_outputs(output_root, phase.OUTPUT_NAMES):
                pass

    def test_pinned_metadata_rejects_name_replacement_before_parse(self) -> None:
        output_root = self.root / "pinned-metadata-replacement"
        output_root.mkdir()
        name = "metadata.json"
        original = {"source": "original"}
        replacement = {"source": "replacement"}
        (output_root / name).write_bytes(canonical(original))
        replacement_path = output_root / "replacement.json"
        replacement_path.write_bytes(canonical(replacement))

        with phase._pinned_outputs(output_root, (name,)) as pinned:
            os.replace(replacement_path, output_root / name)
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed while it was pinned"
            ):
                phase._load_canonical_output(
                    output_root, name, "metadata", pinned=pinned
                )

    def test_pinned_metadata_rejects_same_inode_in_place_mutation(self) -> None:
        output_root = self.root / "pinned-metadata-mutation"
        output_root.mkdir()
        name = "metadata.json"
        (output_root / name).write_bytes(canonical({"value": "A"}))

        with phase._pinned_outputs(output_root, (name,)) as pinned:
            (output_root / name).write_bytes(canonical({"value": "B"}))
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed while it was pinned"
            ):
                phase._load_canonical_output(
                    output_root, name, "metadata", pinned=pinned
                )

    def test_read_regular_uses_a_bounded_read_for_bounded_metadata(self) -> None:
        bound = self.root / "bounded-metadata"
        bound.write_bytes(b"12345678")
        real_fdopen = os.fdopen
        requested_sizes: list[int] = []

        class RecordingHandle:
            def __init__(self, descriptor: int, mode: str):
                self._handle = real_fdopen(descriptor, mode)

            def __enter__(self):
                self._handle.__enter__()
                return self

            def __exit__(self, *arguments):
                return self._handle.__exit__(*arguments)

            def fileno(self) -> int:
                return self._handle.fileno()

            def read(self, size: int = -1) -> bytes:
                requested_sizes.append(size)
                return self._handle.read(size)

        with mock.patch.object(
            phase.os,
            "fdopen",
            side_effect=lambda descriptor, mode: RecordingHandle(descriptor, mode),
        ):
            _, raw = phase._read_regular(
                self.root, "bounded-metadata", max_bytes=8
            )

        self.assertEqual(raw, b"12345678")
        self.assertEqual(requested_sizes, [9])

    def test_read_regular_byte_limit_survives_growth_after_open(self) -> None:
        bound = self.root / "growing-metadata"
        bound.write_bytes(b"12345678")
        real_fdopen = os.fdopen
        requested_sizes: list[int] = []

        class GrowingHandle:
            def __init__(self, descriptor: int, mode: str):
                self._handle = real_fdopen(descriptor, mode)

            def __enter__(self):
                self._handle.__enter__()
                return self

            def __exit__(self, *arguments):
                return self._handle.__exit__(*arguments)

            def fileno(self) -> int:
                return self._handle.fileno()

            def read(self, size: int = -1) -> bytes:
                requested_sizes.append(size)
                bound.write_bytes(b"0123456789")
                return self._handle.read(size)

        with mock.patch.object(
            phase.os,
            "fdopen",
            side_effect=lambda descriptor, mode: GrowingHandle(descriptor, mode),
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "metadata exceeds byte limit"
            ):
                phase._read_regular(
                    self.root, "growing-metadata", max_bytes=8
                )

        self.assertEqual(requested_sizes, [9])

    def test_read_regular_identity_includes_mode_and_nanosecond_times(self) -> None:
        bound = self.root / "strong-identity"
        bound.write_bytes(b"same-size")
        real_stat = os.stat
        observed = real_stat(bound)
        for field, value in (
            ("st_mode", observed.st_mode ^ stat.S_IXUSR),
            ("st_mtime_ns", observed.st_mtime_ns - 1),
            ("st_ctime_ns", observed.st_ctime_ns - 1),
        ):
            with self.subTest(field=field):
                values = {
                    "st_mode": observed.st_mode,
                    "st_dev": observed.st_dev,
                    "st_ino": observed.st_ino,
                    "st_size": observed.st_size,
                    "st_mtime_ns": observed.st_mtime_ns,
                    "st_ctime_ns": observed.st_ctime_ns,
                }
                values[field] = value
                deceptive = types.SimpleNamespace(**values)

                def deceptive_stat(path, *arguments, **keywords):
                    if (
                        path == "strong-identity"
                        and keywords.get("dir_fd") is not None
                    ):
                        return deceptive
                    return real_stat(path, *arguments, **keywords)

                with mock.patch.object(
                    phase.os, "stat", side_effect=deceptive_stat
                ):
                    with self.assertRaisesRegex(
                        phase.SuccessorProduceV4Error,
                        "changed between inspection and open",
                    ):
                        phase._read_regular(self.root, "strong-identity")

    def test_read_regular_rejects_same_size_timestamp_mutation_while_reading(
        self,
    ) -> None:
        bound = self.root / "same-size-mutation"
        bound.write_bytes(b"same-size")
        observed = bound.stat()

        def metadata(*, mtime_ns: int, ctime_ns: int):
            return types.SimpleNamespace(
                st_mode=observed.st_mode,
                st_dev=observed.st_dev,
                st_ino=observed.st_ino,
                st_size=observed.st_size,
                st_mtime_ns=mtime_ns,
                st_ctime_ns=ctime_ns,
            )

        with mock.patch.object(
            phase.os,
            "fstat",
            side_effect=(
                metadata(
                    mtime_ns=observed.st_mtime_ns,
                    ctime_ns=observed.st_ctime_ns,
                ),
                metadata(
                    mtime_ns=observed.st_mtime_ns + 1,
                    ctime_ns=observed.st_ctime_ns + 1,
                ),
            ),
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed while it was read"
            ):
                phase._read_regular(self.root, "same-size-mutation")

    def test_marker_rejects_integer_alias_for_consumed_boolean(self) -> None:
        outputs, backend = self.produce_pending_fixture(
            lambda repository_root, output_root, chain: readback_document()
        )
        marker_path = outputs / phase.CONSUMED_MARKER_NAME
        marker = json.loads(marker_path.read_text())
        marker["consumed"] = 1
        marker_path.chmod(0o644)
        marker_path.write_bytes(canonical(marker))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "consumed marker differs from A6"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=outputs / phase.QUALIFIED_RESULT_NAME,
                backend=backend,
            )

    def test_qualification_rejects_pending_changed_during_readback(self) -> None:
        def mutate_pending(repository_root, output_root, chain):
            del repository_root, chain
            pending = output_root / phase.PENDING_RESULT_NAME
            pending.chmod(0o644)
            pending.write_bytes(pending.read_bytes() + b"\n")
            return readback_document()

        outputs, backend = self.produce_pending_fixture(mutate_pending)
        result = outputs / phase.QUALIFIED_RESULT_NAME

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "qualification output identity changed"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=result,
                backend=backend,
            )

        self.assertFalse(result.exists())

    def test_qualification_rejects_output_changed_by_readback_backend(self) -> None:
        def mutate_after_observing(repository_root, output_root, chain):
            root_disk = output_root / "guest-root-disk"
            original = root_disk.read_bytes()
            document = readback_document(original)
            root_disk.chmod(0o644)
            root_disk.write_bytes(b"mutated-after-readback-observation")
            return document

        outputs, backend = self.produce_pending_fixture(mutate_after_observing)
        result = outputs / phase.QUALIFIED_RESULT_NAME

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "qualification output identity changed"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=result,
                backend=backend,
            )

        self.assertFalse(result.exists())

    def test_qualification_resumes_from_existing_readback_after_outer_publish_crash(self) -> None:
        calls = 0

        def readback(repository_root, output_root, chain):
            nonlocal calls
            calls += 1
            if calls != 1:
                raise AssertionError("readback was repeated after a passing result existed")
            document = readback_document()
            (output_root / phase.READBACK_RESULT_NAME).write_bytes(canonical(document))
            return document

        outputs, backend = self.produce_pending_fixture(readback)
        result = outputs / phase.QUALIFIED_RESULT_NAME
        real_publish = phase._publish_json_once

        def fail_outer_publish(path, document):
            if pathlib.Path(path) == result:
                raise OSError("injected outer publication crash")
            return real_publish(path, document)

        with mock.patch.object(
            phase, "_publish_json_once", side_effect=fail_outer_publish
        ):
            with self.assertRaisesRegex(OSError, "outer publication crash"):
                phase.qualify(
                    repository_root=self.root,
                    outputs=outputs,
                    pending=outputs / phase.PENDING_RESULT_NAME,
                    result=result,
                    backend=backend,
                )

        self.assertFalse(result.exists())
        resumed = phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=result,
            backend=backend,
        )

        self.assertEqual(calls, 1)
        self.assertEqual(resumed["status"], phase.PRODUCTION_QUALIFIED_STATUS)
        self.assertTrue(result.is_file())

    def test_qualification_resumes_when_outer_name_exists_after_fsync_crash(self) -> None:
        calls = 0

        def readback(repository_root, output_root, chain):
            nonlocal calls
            calls += 1
            return readback_document()

        outputs, backend = self.produce_pending_fixture(readback)
        result = outputs / phase.QUALIFIED_RESULT_NAME
        real_publish = phase._publish_json_once

        def publish_then_crash(path, document):
            real_publish(path, document)
            if pathlib.Path(path) == result:
                raise OSError("injected post-link fsync crash")

        with mock.patch.object(
            phase, "_publish_json_once", side_effect=publish_then_crash
        ):
            with self.assertRaisesRegex(OSError, "post-link fsync crash"):
                phase.qualify(
                    repository_root=self.root,
                    outputs=outputs,
                    pending=outputs / phase.PENDING_RESULT_NAME,
                    result=result,
                    backend=backend,
                )

        self.assertTrue(result.is_file())
        resumed = phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=result,
            backend=backend,
        )
        self.assertEqual(calls, 1)
        self.assertEqual(resumed, json.loads(result.read_text()))

    def test_existing_qualified_result_is_strictly_reverified(self) -> None:
        outputs, backend = self.produce_pending_fixture(
            lambda repository_root, output_root, chain: readback_document()
        )
        result = outputs / phase.QUALIFIED_RESULT_NAME
        phase.qualify(
            repository_root=self.root,
            outputs=outputs,
            pending=outputs / phase.PENDING_RESULT_NAME,
            result=result,
            backend=backend,
        )
        tampered = json.loads(result.read_text())
        tampered["quietlyAddedClaim"] = True
        result.chmod(0o644)
        result.write_bytes(canonical(tampered))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "qualified result differs"
        ):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=result,
                backend=backend,
            )

    def test_replica_comparison_requires_two_exact_qualified_directories(self) -> None:
        left, right = self.qualified_replica_fixture()

        result = phase.compare_qualified_replicas(
            repository_root=self.root,
            left=left,
            right=right,
        )

        self.assertEqual(result["status"], "TWO-QUALIFIED-REPLICAS-IDENTICAL")
        self.assertEqual(result["attemptId"], "mac3-launcher-v2-successor-v4-attempt-1")
        self.assertEqual(
            [row["name"] for row in result["outputs"]], list(phase.OUTPUT_NAMES)
        )
        self.assertEqual(result["replicasCompared"], 2)
        self.assertFalse(result["activationAllowed"])
        self.assertFalse(result["bootableClaim"])

    def test_replica_comparison_rejects_extra_missing_or_unqualified_members(self) -> None:
        base_left, _ = self.qualified_replica_fixture()
        mutations = {
            "extra": lambda right: (right / "unexpected").write_bytes(b"extra"),
            "missing": lambda right: (right / phase.READBACK_RESULT_NAME).unlink(),
            "unqualified": lambda right: (right / phase.QUALIFIED_RESULT_NAME).unlink(),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                left = self.root / f"{label}-left"
                right = self.root / f"{label}-right"
                shutil.copytree(base_left, left, copy_function=shutil.copyfile)
                shutil.copytree(base_left, right, copy_function=shutil.copyfile)
                mutate(right)
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "qualified replica members differ"
                ):
                    phase.compare_qualified_replicas(
                        repository_root=self.root,
                        left=left,
                        right=right,
                    )
                shutil.rmtree(left)
                shutil.rmtree(right)

    def test_replica_comparison_rejects_byte_drift_and_directory_reuse(self) -> None:
        left, right = self.qualified_replica_fixture()
        root_disk = right / "guest-root-disk"
        root_disk.write_bytes(b"different-root-disk")
        replacement_digest = hashlib.sha256(root_disk.read_bytes()).hexdigest()
        pending_path = right / phase.PENDING_RESULT_NAME
        pending = json.loads(pending_path.read_text())
        next(
            row
            for row in pending["outputManifest"]
            if row["name"] == "guest-root-disk"
        )["sha256"] = replacement_digest
        pending["rootDisk"]["image"] = {
            "name": "guest-root-disk",
            "sha256": replacement_digest,
            "sizeBytes": len(b"different-root-disk"),
        }
        pending_path.write_bytes(canonical(pending))
        readback_path = right / phase.READBACK_RESULT_NAME
        readback = json.loads(readback_path.read_text())
        readback["image"]["sha256"] = replacement_digest
        readback["image"]["sizeBytes"] = len(b"different-root-disk")
        readback_path.write_bytes(canonical(readback))
        result_path = right / phase.QUALIFIED_RESULT_NAME
        result = json.loads(result_path.read_text())
        result["outputManifest"] = pending["outputManifest"]
        result["pendingResult"]["sha256"] = hashlib.sha256(
            canonical(pending)
        ).hexdigest()
        result["readback"] = readback
        result_path.write_bytes(canonical(result))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "qualified replica contents differ"
        ):
            phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=right,
            )

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "distinct directories"
        ):
            phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=left,
            )

    def test_replica_comparison_rejects_matching_false_embedded_claims(self) -> None:
        base_left, _ = self.qualified_replica_fixture()

        def kernel_claim(document):
            document["kernel"]["kernel"]["sha256"] = "0" * 64

        def root_disk_claim(document):
            document["rootDisk"]["image"]["sizeBytes"] += 1

        def verification_claim(document):
            document["verification"]["checks"] = []

        def schema_claim(document):
            document["schema"] = "boole.forged.pending.v1"

        def build_receipt_claim(document):
            document["buildReceipt"] = {"falseClaim": True}

        def builder_measurement_claim(document):
            document["builderMeasurement"]["falseClaim"] = True

        def kernel_extra_claim(document):
            document["kernel"]["falseClaim"] = True

        def root_disk_extra_claim(document):
            document["rootDisk"]["falseClaim"] = True

        for label, mutate, message in (
            ("kernel", kernel_claim, "pending kernel identity differs"),
            ("root-disk", root_disk_claim, "pending root-disk identity differs"),
            ("verification", verification_claim, "pending verification"),
            ("schema", schema_claim, "pending result schema differs"),
            ("build-receipt", build_receipt_claim, "pending result keys differ"),
            (
                "builder-measurement",
                builder_measurement_claim,
                "pending builder measurement differs",
            ),
            ("kernel-extra", kernel_extra_claim, "pending kernel evidence differs"),
            (
                "root-disk-extra",
                root_disk_extra_claim,
                "pending root-disk evidence differs",
            ),
        ):
            with self.subTest(label=label):
                left = self.root / f"claim-{label}-left"
                right = self.root / f"claim-{label}-right"
                for replica in (left, right):
                    shutil.copytree(
                        base_left, replica, copy_function=shutil.copyfile
                    )
                    pending_path = replica / phase.PENDING_RESULT_NAME
                    pending = json.loads(pending_path.read_text())
                    mutate(pending)
                    pending_path.write_bytes(canonical(pending))
                    result_path = replica / phase.QUALIFIED_RESULT_NAME
                    result = json.loads(result_path.read_text())
                    result["pendingResult"]["sha256"] = hashlib.sha256(
                        canonical(pending)
                    ).hexdigest()
                    result_path.write_bytes(canonical(result))

                with self.assertRaisesRegex(phase.SuccessorProduceV4Error, message):
                    phase.compare_qualified_replicas(
                        repository_root=self.root,
                        left=left,
                        right=right,
                    )

    def test_replica_comparison_rejects_oversized_metadata_before_json_decode(self) -> None:
        left, right = self.qualified_replica_fixture()
        pending = left / phase.PENDING_RESULT_NAME
        pending.chmod(0o644)
        pending.write_bytes(b"{" + b" " * (phase.MAX_METADATA_BYTES + 1))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "metadata exceeds byte limit"
        ):
            phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=right,
            )

    def test_replica_metadata_is_parsed_from_pinned_descriptors(self) -> None:
        left, right = self.qualified_replica_fixture()
        metadata_names = set(phase.QUALIFIED_REPLICA_NAMES) - set(
            phase.OUTPUT_NAMES
        )
        real_read = phase._read_regular

        def reject_metadata_path_reopen(root, relative, **kwargs):
            if pathlib.Path(root) in (left, right) and relative in metadata_names:
                raise AssertionError(f"metadata path was reopened: {relative}")
            return real_read(root, relative, **kwargs)

        with mock.patch.object(
            phase, "_read_regular", side_effect=reject_metadata_path_reopen
        ):
            result = phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=right,
            )

        self.assertEqual(result["replicasCompared"], 2)

    def test_provenance_metadata_is_parsed_from_pinned_descriptors(self) -> None:
        left, right, dispatch = self.provenanced_replica_bundles()
        real_read = phase._read_regular

        def reject_provenance_path_reopen(root, relative, **kwargs):
            if pathlib.Path(root) in (left, right) and relative == phase.REPLICA_PROVENANCE_NAME:
                raise AssertionError("replica provenance path was reopened")
            return real_read(root, relative, **kwargs)

        with mock.patch.object(
            phase, "_read_regular", side_effect=reject_provenance_path_reopen
        ):
            result = phase.compare_provenanced_replicas(
                repository_root=self.root,
                left_bundle=left,
                right_bundle=right,
                **dispatch,
            )

        self.assertTrue(result["logicalReplicaJobsVerified"])

    def test_qualification_and_comparison_do_not_load_image_bytes_into_memory(self) -> None:
        def persist_readback(repository_root, output_root, chain):
            del repository_root, chain
            document = readback_document()
            (output_root / phase.READBACK_RESULT_NAME).write_bytes(canonical(document))
            return document

        outputs, backend = self.produce_pending_fixture(persist_readback)
        real_read = phase._read_regular

        def reject_image_reads(root, relative, **kwargs):
            if relative in phase.OUTPUT_NAMES:
                raise AssertionError(f"image was read into memory: {relative}")
            return real_read(root, relative, **kwargs)

        with mock.patch.object(phase, "_read_regular", side_effect=reject_image_reads):
            phase.qualify(
                repository_root=self.root,
                outputs=outputs,
                pending=outputs / phase.PENDING_RESULT_NAME,
                result=outputs / phase.QUALIFIED_RESULT_NAME,
                backend=backend,
            )
            left = self.root / "replica-1"
            right = self.root / "replica-2"
            outputs.rename(left)
            shutil.copytree(left, right, copy_function=shutil.copyfile)
            phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=right,
            )

    def test_production_manifest_streams_instead_of_loading_image_bytes(self) -> None:
        real_read = phase._read_regular
        expected_outputs = self.root / "outputs"

        def reject_image_reads(root, relative, **kwargs):
            if pathlib.Path(root) == expected_outputs and relative in phase.OUTPUT_NAMES:
                raise AssertionError(f"image was read into memory: {relative}")
            return real_read(root, relative, **kwargs)

        with mock.patch.object(phase, "_read_regular", side_effect=reject_image_reads):
            outputs, _ = self.produce_pending_fixture(
                lambda repository_root, output_root, chain: readback_document()
            )

        self.assertTrue((outputs / phase.PENDING_RESULT_NAME).is_file())

    def test_replica_comparison_pins_both_directories_at_the_same_time(self) -> None:
        left, right = self.qualified_replica_fixture()
        real_validate = phase._validate_qualified_replica

        def mutate_left_when_right_starts(
            repository_root, outputs, chain, count, pinned=None
        ):
            if pathlib.Path(outputs) == right:
                left_kernel = left / "guest-kernel"
                left_kernel.chmod(0o644)
                left_kernel.write_bytes(b"changed-after-left-check")
            return real_validate(repository_root, outputs, chain, count, pinned)

        with mock.patch.object(
            phase, "_validate_qualified_replica", side_effect=mutate_left_when_right_starts
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "changed during replica comparison",
            ):
                phase.compare_qualified_replicas(
                    repository_root=self.root,
                    left=left,
                    right=right,
                )

    def test_replica_comparison_rejects_cross_replica_hardlink_reuse(self) -> None:
        left, right = self.qualified_replica_fixture()
        right_kernel = right / "guest-kernel"
        right_kernel.unlink()
        os.link(left / "guest-kernel", right_kernel)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "replica files must not share inodes",
        ):
            phase.compare_qualified_replicas(
                repository_root=self.root,
                left=left,
                right=right,
            )

    def test_provenanced_comparison_requires_distinct_matrix_replica_envelopes(self) -> None:
        left, right, dispatch = self.provenanced_replica_bundles()

        result = phase.compare_provenanced_replicas(
            repository_root=self.root,
            left_bundle=left,
            right_bundle=right,
            **dispatch,
        )

        self.assertEqual(
            result["status"],
            "TWO-DISTINCT-MATRIX-REPLICA-JOBS-QUALIFIED-AND-IDENTICAL",
        )
        self.assertTrue(result["logicalReplicaJobsVerified"])
        self.assertFalse(result["physicalRunnerIndependenceClaim"])

        copied = (left / phase.REPLICA_PROVENANCE_NAME).read_bytes()
        target = right / phase.REPLICA_PROVENANCE_NAME
        target.write_bytes(copied)
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "replica provenance differs"
        ):
            phase.compare_provenanced_replicas(
                repository_root=self.root,
                left_bundle=left,
                right_bundle=right,
                **dispatch,
            )

    def test_provenanced_comparison_rejects_bundle_or_matrix_drift(self) -> None:
        base_left, base_right, dispatch = self.provenanced_replica_bundles()
        mutations = {
            "extra-member": lambda left, right: (right / "extra").write_text("x"),
            "wrong-index": lambda left, right: self._mutate_provenance(
                right, lambda row: row["replica"].__setitem__("strategyJobIndex", 0)
            ),
            "wrong-total": lambda left, right: self._mutate_provenance(
                right, lambda row: row["replica"].__setitem__("strategyJobTotal", 3)
            ),
            "wrong-artifact": lambda left, right: self._mutate_provenance(
                right, lambda row: row.__setitem__("artifactName", "copied")
            ),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label):
                left = self.root / f"provenance-{label}-left"
                right = self.root / f"provenance-{label}-right"
                shutil.copytree(base_left, left, copy_function=shutil.copyfile)
                shutil.copytree(base_right, right, copy_function=shutil.copyfile)
                mutate(left, right)
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "replica (bundle|provenance) differs",
                ):
                    phase.compare_provenanced_replicas(
                        repository_root=self.root,
                        left_bundle=left,
                        right_bundle=right,
                        **dispatch,
                    )

    @staticmethod
    def _dispatch_cli_arguments(dispatch: dict[str, object]) -> list[str]:
        return [
            "--claim-ref",
            str(dispatch["claim_ref"]),
            "--ref-object-sha",
            str(dispatch["ref_object_sha"]),
            "--tag-object-sha",
            str(dispatch["tag_object_sha"]),
            "--github-run-id",
            str(dispatch["github_run_id"]),
            "--github-run-attempt",
            str(dispatch["github_run_attempt"]),
            "--workflow-path",
            str(dispatch["workflow_path"]),
            "--head-sha",
            str(dispatch["head_sha"]),
            "--head-a6-sha256",
            str(dispatch["head_a6_sha256"]),
        ]

    def test_replica_provenance_has_no_unsealed_public_cli(self) -> None:
        with mock.patch.object(phase.sys, "stderr", io.StringIO()):
            with self.assertRaises(SystemExit):
                phase._parser().parse_args(["replica-provenance"])

    def test_failure_bundle_seal_accepts_only_the_exact_progress_states(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected_marker = phase._marker_document(chain)
        expected_marker_raw = canonical(expected_marker)
        consumed = phase.CONSUMED_MARKER_NAME
        kernel, initrd, root_disk = phase.OUTPUT_NAMES
        diagnostic = phase.UNQUALIFIED_MARKER_NAME
        pending = phase.PENDING_RESULT_NAME
        readback = phase.READBACK_RESULT_NAME
        qualified = phase.QUALIFIED_RESULT_NAME
        allowed = (
            {consumed},
            {consumed, kernel},
            {consumed, kernel, initrd},
            {consumed, kernel, initrd, root_disk},
            {consumed, diagnostic},
            {consumed, kernel, diagnostic},
            {consumed, kernel, initrd, diagnostic},
            {consumed, kernel, initrd, root_disk, diagnostic},
            {consumed, kernel, initrd, root_disk, pending},
            {consumed, kernel, initrd, root_disk, pending, readback},
            {
                consumed,
                kernel,
                initrd,
                root_disk,
                pending,
                readback,
                qualified,
            },
        )
        for index, names in enumerate(allowed):
            with self.subTest(allowed=sorted(names)):
                parent = self.root / f"failure-seal-allowed-{index}"
                outputs = parent / "outputs"
                outputs.mkdir(parents=True)
                parent.chmod(0o700)
                for name in names:
                    (outputs / name).write_bytes(
                        expected_marker_raw
                        if name == consumed
                        else name.encode("utf-8")
                    )
                parent_info = parent.stat()
                phase.seal_collectable_replica_bundle(
                    parent=parent,
                    successful=False,
                    expected_parent_identity=(
                        parent_info.st_dev,
                        parent_info.st_ino,
                    ),
                    expected_uid=os.geteuid(),
                    expected_gid=os.getegid(),
                    expected_failure_marker=expected_marker,
                )
                self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o711)
                self.assertEqual(stat.S_IMODE(outputs.stat().st_mode), 0o555)
                for name in names:
                    self.assertEqual(
                        stat.S_IMODE((outputs / name).stat().st_mode), 0o444
                    )

        invalid = (
            {consumed, initrd},
            {consumed, diagnostic, pending},
            {consumed, "unexpected"},
        )
        for index, names in enumerate(invalid):
            with self.subTest(invalid=sorted(names)):
                parent = self.root / f"failure-seal-invalid-{index}"
                outputs = parent / "outputs"
                outputs.mkdir(parents=True)
                parent.chmod(0o700)
                for name in names:
                    (outputs / name).write_bytes(
                        expected_marker_raw
                        if name == consumed
                        else name.encode("utf-8")
                    )
                parent_info = parent.stat()
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "bundle|member set"
                ):
                    phase.seal_collectable_replica_bundle(
                        parent=parent,
                        successful=False,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        expected_failure_marker=expected_marker,
                    )

    def test_output_recovery_converges_marker_crash_states_and_binds_claim(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected_marker = canonical(phase._marker_document(chain))

        empty_parent = self.root / "output-recovery-empty"
        empty_outputs = empty_parent / "outputs"
        empty_outputs.mkdir(parents=True, mode=0o700)
        empty_parent.chmod(0o700)
        empty_outputs.chmod(0o700)
        empty_info = empty_parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=empty_parent,
                expected_parent_identity=(empty_info.st_dev, empty_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "unconsumed",
        )
        self.assertFalse(empty_outputs.exists())

        partial_parent = self.root / "output-recovery-partial"
        partial_outputs = partial_parent / "outputs"
        partial_outputs.mkdir(parents=True, mode=0o700)
        partial_parent.chmod(0o700)
        partial_outputs.chmod(0o700)
        partial = partial_outputs / f".{phase.CONSUMED_MARKER_NAME}.partial.abcdefgh"
        partial.write_bytes(expected_marker[:17])
        partial.chmod(0o600)
        partial_info = partial_parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=partial_parent,
                expected_parent_identity=(partial_info.st_dev, partial_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "unconsumed",
        )
        self.assertFalse(partial_outputs.exists())

        linked_parent = self.root / "output-recovery-linked"
        linked_outputs = linked_parent / "outputs"
        linked_outputs.mkdir(parents=True, mode=0o700)
        linked_parent.chmod(0o700)
        linked_outputs.chmod(0o700)
        marker = linked_outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(expected_marker)
        marker.chmod(0o444)
        linked_partial = linked_outputs / f".{phase.CONSUMED_MARKER_NAME}.partial.abcdefgh"
        os.link(marker, linked_partial)
        linked_info = linked_parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=linked_parent,
                expected_parent_identity=(linked_info.st_dev, linked_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )
        self.assertFalse(linked_partial.exists())
        self.assertEqual(marker.stat().st_nlink, 1)

        wrong_parent = self.root / "output-recovery-wrong-claim"
        wrong_outputs = wrong_parent / "outputs"
        wrong_outputs.mkdir(parents=True, mode=0o700)
        wrong_parent.chmod(0o700)
        wrong_outputs.chmod(0o700)
        wrong_marker = wrong_outputs / phase.CONSUMED_MARKER_NAME
        wrong_document = phase._marker_document(chain)
        wrong_document["attemptId"] = "another-attempt"
        wrong_marker.write_bytes(canonical(wrong_document))
        wrong_marker.chmod(0o444)
        wrong_info = wrong_parent.stat()
        before = wrong_marker.read_bytes()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "marker|claim|attempt|A6|output"
        ):
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=wrong_parent,
                expected_parent_identity=(wrong_info.st_dev, wrong_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            )
        self.assertEqual(wrong_marker.read_bytes(), before)
        self.assertEqual(stat.S_IMODE(wrong_parent.stat().st_mode), 0o700)

    def test_output_recovery_converges_one_known_output_publication_partial(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected_marker = canonical(phase._marker_document(chain))

        partial_only_parent = self.root / "output-recovery-data-partial-only"
        partial_only_outputs = partial_only_parent / "outputs"
        partial_only_outputs.mkdir(parents=True, mode=0o700)
        partial_only_parent.chmod(0o700)
        marker = partial_only_outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(expected_marker)
        marker.chmod(0o444)
        kernel = partial_only_outputs / phase.OUTPUT_NAMES[0]
        kernel.write_bytes(b"complete kernel")
        kernel.chmod(0o444)
        partial = (
            partial_only_outputs
            / f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh"
        )
        partial.write_bytes(b"incomplete initrd")
        partial.chmod(0o600)
        parent_info = partial_only_parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=partial_only_parent,
                expected_parent_identity=(
                    parent_info.st_dev,
                    parent_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )
        self.assertFalse(partial.exists())
        self.assertEqual(
            frozenset(os.listdir(partial_only_outputs)),
            frozenset((phase.CONSUMED_MARKER_NAME, phase.OUTPUT_NAMES[0])),
        )

        linked_parent = self.root / "output-recovery-data-linked-partial"
        linked_outputs = linked_parent / "outputs"
        linked_outputs.mkdir(parents=True, mode=0o700)
        linked_parent.chmod(0o700)
        linked_marker = linked_outputs / phase.CONSUMED_MARKER_NAME
        linked_marker.write_bytes(expected_marker)
        linked_marker.chmod(0o444)
        linked_kernel = linked_outputs / phase.OUTPUT_NAMES[0]
        linked_kernel.write_bytes(b"complete kernel")
        linked_kernel.chmod(0o444)
        initrd = linked_outputs / phase.OUTPUT_NAMES[1]
        initrd.write_bytes(b"complete initrd")
        initrd.chmod(0o444)
        linked_partial = (
            linked_outputs
            / f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh"
        )
        os.link(initrd, linked_partial)
        linked_info = linked_parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=linked_parent,
                expected_parent_identity=(
                    linked_info.st_dev,
                    linked_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )
        self.assertFalse(linked_partial.exists())
        self.assertEqual(initrd.stat().st_nlink, 1)

    def test_output_recovery_rejects_unknown_or_split_output_partials(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected_marker = canonical(phase._marker_document(chain))

        for index, shape in enumerate(
            ("unknown", "split", "multiple", "wrong-stage")
        ):
            with self.subTest(shape=shape):
                parent = self.root / f"output-recovery-data-invalid-{index}"
                outputs = parent / "outputs"
                outputs.mkdir(parents=True, mode=0o700)
                parent.chmod(0o700)
                marker = outputs / phase.CONSUMED_MARKER_NAME
                marker.write_bytes(expected_marker)
                marker.chmod(0o444)
                first_name = (
                    ".unknown.partial.abcdefgh"
                    if shape == "unknown"
                    else f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh"
                )
                first = outputs / first_name
                first.write_bytes(b"partial")
                first.chmod(0o600)
                if shape != "wrong-stage":
                    kernel = outputs / phase.OUTPUT_NAMES[0]
                    kernel.write_bytes(b"kernel")
                    kernel.chmod(0o444)
                if shape == "split":
                    final = outputs / phase.OUTPUT_NAMES[1]
                    final.write_bytes(b"different inode")
                    final.chmod(0o444)
                if shape == "multiple":
                    second = (
                        outputs
                        / f".{phase.PENDING_RESULT_NAME}.partial.abcdefgh"
                    )
                    second.write_bytes(b"second partial")
                    second.chmod(0o600)
                before = {
                    name: (outputs / name).read_bytes()
                    for name in os.listdir(outputs)
                }
                parent_info = parent.stat()
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "partial|member set|output",
                ):
                    phase.reconcile_production_output_state(
                        repository_root=self.root,
                        parent=parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                    )
                self.assertEqual(
                    {
                        name: (outputs / name).read_bytes()
                        for name in os.listdir(outputs)
                    },
                    before,
                )

    def test_output_recovery_covers_each_real_create_once_publication(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        marker_raw = canonical(phase._marker_document(chain))
        full_images = (
            phase.CONSUMED_MARKER_NAME,
            *phase.OUTPUT_NAMES,
        )
        pending = (*full_images, phase.PENDING_RESULT_NAME)
        cases = (
            (
                phase.OUTPUT_NAMES[1],
                f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh",
                (phase.CONSUMED_MARKER_NAME, phase.OUTPUT_NAMES[0]),
            ),
            (
                phase.PENDING_RESULT_NAME,
                f".{phase.PENDING_RESULT_NAME}.partial.abcdefgh",
                full_images,
            ),
            (
                phase.UNQUALIFIED_MARKER_NAME,
                f".{phase.UNQUALIFIED_MARKER_NAME}.partial.abcdefgh",
                (phase.CONSUMED_MARKER_NAME,),
            ),
            (
                phase.QUALIFIED_RESULT_NAME,
                f".{phase.QUALIFIED_RESULT_NAME}.partial.abcdefgh",
                (*pending, phase.READBACK_RESULT_NAME),
            ),
            (
                phase.READBACK_RESULT_NAME,
                phase.READBACK_PRIVATE_PENDING_NAME,
                pending,
            ),
        )

        for index, (target, partial_name, predecessors) in enumerate(cases):
            for linked in (False, True):
                with self.subTest(target=target, linked=linked):
                    parent = self.root / f"output-publication-{index}-{linked}"
                    outputs = parent / "outputs"
                    outputs.mkdir(parents=True, mode=0o700)
                    parent.chmod(0o700)
                    for name in predecessors:
                        path = outputs / name
                        path.write_bytes(
                            marker_raw
                            if name == phase.CONSUMED_MARKER_NAME
                            else name.encode("utf-8")
                        )
                        path.chmod(0o444)
                    partial = outputs / partial_name
                    if linked:
                        final = outputs / target
                        final.write_bytes(b"complete publication")
                        final.chmod(0o444)
                        os.link(final, partial)
                    else:
                        partial.write_bytes(b"incomplete publication")
                        partial.chmod(
                            0o444
                            if partial_name
                            == phase.READBACK_PRIVATE_PENDING_NAME
                            else 0o600
                        )
                    if target in (
                        phase.READBACK_RESULT_NAME,
                        phase.QUALIFIED_RESULT_NAME,
                    ):
                        outputs.chmod(0o755)
                    parent_info = parent.stat()
                    expected_state = (
                        "success-pending-seal"
                        if target == phase.QUALIFIED_RESULT_NAME and linked
                        else "consumed"
                    )
                    self.assertEqual(
                        phase.reconcile_production_output_state(
                            repository_root=self.root,
                            parent=parent,
                            expected_parent_identity=(
                                parent_info.st_dev,
                                parent_info.st_ino,
                            ),
                            expected_uid=os.geteuid(),
                            expected_gid=os.getegid(),
                        ),
                        expected_state,
                    )
                    self.assertFalse(partial.exists())
                    if linked:
                        self.assertEqual((outputs / target).stat().st_nlink, 1)

    def test_output_recovery_accepts_the_collectable_readback_failure_state(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        parent = self.root / "output-recovery-readback-failure"
        outputs = parent / "outputs"
        outputs.mkdir(parents=True, mode=0o700)
        parent.chmod(0o700)
        names = (
            phase.CONSUMED_MARKER_NAME,
            *phase.OUTPUT_NAMES,
            phase.PENDING_RESULT_NAME,
            phase.UNQUALIFIED_MARKER_NAME,
        )
        for name in names:
            path = outputs / name
            path.write_bytes(
                canonical(phase._marker_document(chain))
                if name == phase.CONSUMED_MARKER_NAME
                else name.encode("utf-8")
            )
            path.chmod(0o444)
        outputs.chmod(0o755)
        parent_info = parent.stat()

        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=parent,
                expected_parent_identity=(
                    parent_info.st_dev,
                    parent_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )

    def test_output_recovery_rejects_unsafe_known_publication_partials(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        marker_raw = canonical(phase._marker_document(chain))
        external = self.root / "external-publication-inode"
        external.write_bytes(b"external")
        external.chmod(0o600)

        for index, shape in enumerate(("symlink", "fifo", "external-link")):
            with self.subTest(shape=shape):
                parent = self.root / f"unsafe-known-partial-{index}"
                outputs = parent / "outputs"
                outputs.mkdir(parents=True, mode=0o700)
                parent.chmod(0o700)
                marker = outputs / phase.CONSUMED_MARKER_NAME
                marker.write_bytes(marker_raw)
                marker.chmod(0o444)
                kernel = outputs / phase.OUTPUT_NAMES[0]
                kernel.write_bytes(b"kernel")
                kernel.chmod(0o444)
                partial = (
                    outputs
                    / f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh"
                )
                if shape == "symlink":
                    partial.symlink_to(external)
                elif shape == "fifo":
                    os.mkfifo(partial, 0o600)
                else:
                    os.link(external, partial)
                before = os.lstat(partial)
                parent_info = parent.stat()
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "partial|identity|output",
                ):
                    phase.reconcile_production_output_state(
                        repository_root=self.root,
                        parent=parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                    )
                after = os.lstat(partial)
                self.assertEqual(
                    (after.st_dev, after.st_ino, after.st_mode, after.st_nlink),
                    (before.st_dev, before.st_ino, before.st_mode, before.st_nlink),
                )

    def test_output_partial_cleanup_is_reentrant_after_directory_fsync_failure(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        parent = self.root / "output-partial-fsync-recovery"
        outputs = parent / "outputs"
        outputs.mkdir(parents=True, mode=0o700)
        parent.chmod(0o700)
        marker = outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(canonical(phase._marker_document(chain)))
        marker.chmod(0o444)
        kernel = outputs / phase.OUTPUT_NAMES[0]
        kernel.write_bytes(b"kernel")
        kernel.chmod(0o444)
        partial = outputs / f".{phase.OUTPUT_NAMES[1]}.partial.abcdefgh"
        partial.write_bytes(b"partial")
        partial.chmod(0o600)
        parent_info = parent.stat()

        with mock.patch.object(
            phase.os,
            "fsync",
            side_effect=OSError("injected directory fsync failure"),
        ):
            with self.assertRaises(OSError):
                phase.reconcile_production_output_state(
                    repository_root=self.root,
                    parent=parent,
                    expected_parent_identity=(
                        parent_info.st_dev,
                        parent_info.st_ino,
                    ),
                    expected_uid=os.geteuid(),
                    expected_gid=os.getegid(),
                )
        self.assertFalse(partial.exists())
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=parent,
                expected_parent_identity=(
                    parent_info.st_dev,
                    parent_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )

    def test_failure_seal_rejects_a_consumed_marker_for_another_claim(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        parent = self.root / "failure-seal-wrong-claim"
        outputs = parent / "outputs"
        outputs.mkdir(parents=True, mode=0o700)
        parent.chmod(0o700)
        outputs.chmod(0o700)
        wrong = phase._marker_document(chain)
        wrong["attemptId"] = "another-attempt"
        marker = outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(canonical(wrong))
        marker.chmod(0o444)
        parent_info = parent.stat()
        before = marker.read_bytes()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "marker|claim|attempt|A6|output"
        ):
            phase.seal_collectable_replica_bundle(
                parent=parent,
                successful=False,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                expected_failure_marker=phase._marker_document(chain),
            )
        self.assertEqual(marker.read_bytes(), before)
        self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o700)

    def test_failure_seal_requires_and_finally_rechecks_the_exact_marker(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected = phase._marker_document(chain)

        missing_parent = self.root / "failure-seal-missing-expectation"
        missing_outputs = missing_parent / "outputs"
        missing_outputs.mkdir(parents=True, mode=0o700)
        missing_parent.chmod(0o700)
        missing_marker = missing_outputs / phase.CONSUMED_MARKER_NAME
        missing_marker.write_bytes(canonical(expected))
        missing_marker.chmod(0o600)
        missing_info = missing_parent.stat()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "marker|claim|required"
        ):
            phase.seal_collectable_replica_bundle(
                parent=missing_parent,
                successful=False,
                expected_parent_identity=(
                    missing_info.st_dev,
                    missing_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            )
        self.assertEqual(stat.S_IMODE(missing_parent.stat().st_mode), 0o700)

        race_parent = self.root / "failure-seal-marker-race"
        race_outputs = race_parent / "outputs"
        race_outputs.mkdir(parents=True, mode=0o700)
        race_parent.chmod(0o700)
        race_marker = race_outputs / phase.CONSUMED_MARKER_NAME
        race_marker.write_bytes(canonical(expected))
        race_marker.chmod(0o600)
        retained = os.open(race_marker, os.O_RDWR)
        marker_inode = os.fstat(retained).st_ino
        wrong = dict(expected)
        wrong["attemptId"] = "mac3-launcher-v2-successor-v4-attempt-2"
        wrong_raw = canonical(wrong)
        self.assertEqual(len(wrong_raw), len(canonical(expected)))
        real_fchmod = phase.os.fchmod
        mutated = False

        def mutate_before_first_marker_seal(descriptor, mode):
            nonlocal mutated
            if not mutated and os.fstat(descriptor).st_ino == marker_inode:
                mutated = True
                os.lseek(retained, 0, os.SEEK_SET)
                os.write(retained, wrong_raw)
                os.ftruncate(retained, len(wrong_raw))
                os.fsync(retained)
            return real_fchmod(descriptor, mode)

        race_info = race_parent.stat()
        try:
            with mock.patch.object(
                phase.os, "fchmod", side_effect=mutate_before_first_marker_seal
            ):
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "marker|claim|changed|content"
                ):
                    phase.seal_collectable_replica_bundle(
                        parent=race_parent,
                        successful=False,
                        expected_parent_identity=(
                            race_info.st_dev,
                            race_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        expected_failure_marker=expected,
                    )
        finally:
            os.close(retained)
        self.assertTrue(mutated)
        self.assertEqual(stat.S_IMODE(race_parent.stat().st_mode), 0o700)

    def test_failure_seal_rejects_unverified_provenance_without_mutation(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        parent = self.root / "failure-seal-unverified-provenance"
        outputs = parent / "outputs"
        outputs.mkdir(parents=True, mode=0o700)
        parent.chmod(0o700)
        marker = outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(canonical(phase._marker_document(chain)))
        marker.chmod(0o600)
        provenance = parent / phase.REPLICA_PROVENANCE_NAME
        provenance.write_bytes(b"not provenance")
        provenance.chmod(0o600)
        parent_info = parent.stat()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "provenance|parent members|bundle"
        ):
            phase.seal_collectable_replica_bundle(
                parent=parent,
                successful=False,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                expected_failure_marker=phase._marker_document(chain),
            )
        self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(provenance.stat().st_mode), 0o600)

    def test_output_recovery_rejects_unsafe_sealed_members_and_rebinds_parent(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        expected_marker = canonical(phase._marker_document(chain))

        unsafe_parent = self.root / "output-recovery-unsafe-sealed"
        unsafe_outputs = unsafe_parent / "outputs"
        unsafe_outputs.mkdir(parents=True, mode=0o700)
        unsafe_marker = unsafe_outputs / phase.CONSUMED_MARKER_NAME
        unsafe_marker.write_bytes(expected_marker)
        unsafe_marker.chmod(0o444)
        unsafe_kernel = unsafe_outputs / phase.OUTPUT_NAMES[0]
        unsafe_kernel.write_bytes(b"kernel")
        unsafe_kernel.chmod(0o666)
        unsafe_outputs.chmod(0o555)
        unsafe_parent.chmod(0o711)
        unsafe_info = unsafe_parent.stat()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "sealed|mode|member|bundle|output"
        ):
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=unsafe_parent,
                expected_parent_identity=(unsafe_info.st_dev, unsafe_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            )

        provenance_parent = self.root / "output-recovery-provenance-symlink"
        provenance_outputs = provenance_parent / "outputs"
        provenance_outputs.mkdir(parents=True, mode=0o700)
        provenance_marker = provenance_outputs / phase.CONSUMED_MARKER_NAME
        provenance_marker.write_bytes(expected_marker)
        provenance_marker.chmod(0o444)
        target = self.root / "outside-provenance"
        target.write_bytes(b"outside")
        (provenance_parent / phase.REPLICA_PROVENANCE_NAME).symlink_to(target)
        provenance_outputs.chmod(0o555)
        provenance_parent.chmod(0o711)
        provenance_info = provenance_parent.stat()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "provenance|sealed|symlink|member"
        ):
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=provenance_parent,
                expected_parent_identity=(
                    provenance_info.st_dev,
                    provenance_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            )

        race_parent = self.root / "output-recovery-parent-race"
        race_outputs = race_parent / "outputs"
        race_outputs.mkdir(parents=True, mode=0o700)
        race_marker = race_outputs / phase.CONSUMED_MARKER_NAME
        race_marker.write_bytes(expected_marker)
        race_marker.chmod(0o444)
        race_outputs.chmod(0o555)
        race_parent.chmod(0o711)
        race_info = race_parent.stat()
        moved = self.root / "output-recovery-parent-race-moved"
        real_reader = phase._read_exact_output_marker_at
        replaced = False

        def replace_parent_after_marker(*args, **kwargs):
            nonlocal replaced
            observed = real_reader(*args, **kwargs)
            if not replaced:
                replaced = True
                race_parent.rename(moved)
                race_parent.mkdir(mode=0o711)
                race_parent.chmod(0o711)
            return observed

        with mock.patch.object(
            phase,
            "_read_exact_output_marker_at",
            side_effect=replace_parent_after_marker,
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "parent|path|identity|changed"
            ):
                phase.reconcile_production_output_state(
                    repository_root=self.root,
                    parent=race_parent,
                    expected_parent_identity=(race_info.st_dev, race_info.st_ino),
                    expected_uid=os.geteuid(),
                    expected_gid=os.getegid(),
                )
        self.assertTrue(replaced)

    def test_output_recovery_resumes_after_outputs_seal_before_parent_seal(
        self,
    ) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        parent = self.root / "output-recovery-mid-seal"
        outputs = parent / "outputs"
        outputs.mkdir(parents=True, mode=0o700)
        parent.chmod(0o700)
        marker = outputs / phase.CONSUMED_MARKER_NAME
        marker.write_bytes(canonical(phase._marker_document(chain)))
        marker.chmod(0o444)
        outputs.chmod(0o555)
        parent_info = parent.stat()
        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "consumed",
        )

    def test_output_recovery_revalidates_one_complete_success_bundle(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "output-recovery-qualified-sealed"
        parent.mkdir(mode=0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        self.publish_and_seal_fixture(parent, outputs, dispatch)
        parent_info = parent.stat()

        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=parent,
                expected_parent_identity=(
                    parent_info.st_dev,
                    parent_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "sealed",
        )

    def test_success_seal_recovers_each_provenance_publication_state(self) -> None:
        qualified_template, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        for index, shape in enumerate(
            ("partial-prefix", "complete-final-0400", "final-plus-partial")
        ):
            with self.subTest(shape=shape):
                parent = self.root / f"success-provenance-recovery-{index}"
                parent.mkdir(mode=0o700)
                outputs = parent / "outputs"
                shutil.copytree(
                    qualified_template,
                    outputs,
                    copy_function=shutil.copyfile,
                )
                outputs.chmod(0o700)
                document = phase.replica_provenance_document(
                    repository_root=self.root,
                    outputs=outputs,
                    replica_ordinal=1,
                    strategy_job_index=0,
                    strategy_job_total=2,
                    github_job="produce",
                    artifact_name=phase.REPLICA_ARTIFACT_PREFIX + "1",
                    **dispatch,
                )
                raw = canonical(document)
                provenance = parent / phase.REPLICA_PROVENANCE_NAME
                partial = parent / f".{phase.REPLICA_PROVENANCE_NAME}.partial"
                if shape == "partial-prefix":
                    partial.write_bytes(raw[: max(1, len(raw) // 3)])
                    partial.chmod(0o400)
                else:
                    provenance.write_bytes(raw)
                    provenance.chmod(0o400 if shape == "complete-final-0400" else 0o444)
                    if shape == "final-plus-partial":
                        os.link(provenance, partial)

                self.publish_and_seal_fixture(parent, outputs, dispatch)
                self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o711)
                self.assertEqual(stat.S_IMODE(outputs.stat().st_mode), 0o555)
                self.assertEqual(provenance.read_bytes(), raw)
                self.assertEqual(stat.S_IMODE(provenance.stat().st_mode), 0o444)
                self.assertFalse(partial.exists())

    def test_success_seal_recovers_every_output_chmod_prefix(self) -> None:
        qualified_names = tuple(
            sorted(phase.QUALIFIED_REPLICA_NAMES, key=os.fsencode)
        )
        qualified_template, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        for prefix_length in range(len(qualified_names) + 1):
            with self.subTest(prefix_length=prefix_length):
                parent = self.root / f"success-output-prefix-{prefix_length}"
                parent.mkdir(mode=0o700)
                outputs = parent / "outputs"
                shutil.copytree(
                    qualified_template,
                    outputs,
                    copy_function=shutil.copyfile,
                )
                outputs.chmod(0o700)
                for index, name in enumerate(qualified_names):
                    (outputs / name).chmod(
                        0o444 if index < prefix_length else 0o600
                    )
                document = phase.replica_provenance_document(
                    repository_root=self.root,
                    outputs=outputs,
                    replica_ordinal=1,
                    strategy_job_index=0,
                    strategy_job_total=2,
                    github_job="produce",
                    artifact_name=phase.REPLICA_ARTIFACT_PREFIX + "1",
                    **dispatch,
                )
                provenance = parent / phase.REPLICA_PROVENANCE_NAME
                provenance.write_bytes(canonical(document))
                provenance.chmod(0o400)

                self.publish_and_seal_fixture(parent, outputs, dispatch)
                self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o711)
                self.assertEqual(stat.S_IMODE(outputs.stat().st_mode), 0o555)
                for name in qualified_names:
                    self.assertEqual(
                        stat.S_IMODE((outputs / name).stat().st_mode),
                        0o444,
                    )
                self.assertEqual(stat.S_IMODE(provenance.stat().st_mode), 0o444)

    def test_output_recovery_distinguishes_qualified_success_before_seal(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        parent = self.root / "output-recovery-qualified-before-seal"
        parent.mkdir(mode=0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        for name in phase.QUALIFIED_REPLICA_NAMES:
            (outputs / name).chmod(0o444)
        outputs.chmod(0o755)
        parent_info = parent.stat()

        self.assertEqual(
            phase.reconcile_production_output_state(
                repository_root=self.root,
                parent=parent,
                expected_parent_identity=(
                    parent_info.st_dev,
                    parent_info.st_ino,
                ),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
            ),
            "success-pending-seal",
        )

    def test_success_bundle_seal_requires_and_rechecks_exact_provenance(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "success-seal"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        provenance = phase.publish_replica_provenance(
            repository_root=self.root,
            outputs=outputs,
            result=parent / phase.REPLICA_PROVENANCE_NAME,
            replica_ordinal=1,
            strategy_job_index=0,
            strategy_job_total=2,
            github_job="produce",
            artifact_name=phase.REPLICA_ARTIFACT_PREFIX + "1",
            **dispatch,
        )
        parent_info = parent.stat()
        phase.seal_collectable_replica_bundle(
            parent=parent,
            successful=True,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            expected_provenance=provenance,
        )
        self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o711)
        self.assertEqual(stat.S_IMODE(outputs.stat().st_mode), 0o555)
        self.assertEqual(
            stat.S_IMODE(
                (parent / phase.REPLICA_PROVENANCE_NAME).stat().st_mode
            ),
            0o444,
        )
        for name in phase.QUALIFIED_REPLICA_NAMES:
            self.assertEqual(stat.S_IMODE((outputs / name).stat().st_mode), 0o444)

    def test_success_seal_rechecks_live_claim_after_provenance_calculation(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "success-seal-live-claim-race"
        parent.mkdir(mode=0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        parent_info = parent.stat()
        original_modes = {
            name: stat.S_IMODE((outputs / name).stat().st_mode)
            for name in phase.QUALIFIED_REPLICA_NAMES
        }
        real_document = phase.replica_provenance_document
        deleted = False

        def delete_claim_after_document(**arguments):
            nonlocal deleted
            document = real_document(**arguments)
            if not deleted:
                subprocess.run(
                    [
                        "/usr/bin/git",
                        "-C",
                        str(self.root),
                        "update-ref",
                        "-d",
                        dispatch["claim_ref"],
                    ],
                    check=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                )
                deleted = True
            return document

        with mock.patch.object(
            phase,
            "replica_provenance_document",
            side_effect=delete_claim_after_document,
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "dispatch claim live repository ref differs",
            ):
                self.publish_and_seal_fixture(parent, outputs, dispatch)

        self.assertTrue(deleted)
        self.assertFalse((parent / phase.REPLICA_PROVENANCE_NAME).exists())
        self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o700)
        self.assertEqual(
            {
                name: stat.S_IMODE((outputs / name).stat().st_mode)
                for name in phase.QUALIFIED_REPLICA_NAMES
            },
            original_modes,
        )

    def test_success_bundle_seal_rejects_missing_or_changed_provenance(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        parent = self.root / "success-seal-missing"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        qualified.rename(parent / "outputs")
        parent_info = parent.stat()
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "bundle|provenance"
        ):
            phase.seal_collectable_replica_bundle(
                parent=parent,
                successful=True,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                expected_provenance={"expected": True},
            )

    def test_integrated_seal_rejects_wrong_parent_before_provenance_or_mode_effects(
        self,
    ) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "integrated-wrong-parent"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        before_modes = {
            name: stat.S_IMODE((outputs / name).stat().st_mode)
            for name in phase.QUALIFIED_REPLICA_NAMES
        }
        info = parent.stat()

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "parent identity"
        ):
            self.publish_and_seal_fixture(
                parent,
                outputs,
                dispatch,
                parent_identity=(info.st_dev, info.st_ino + 1),
            )

        self.assertFalse((parent / phase.REPLICA_PROVENANCE_NAME).exists())
        self.assertEqual(stat.S_IMODE(parent.stat().st_mode), 0o700)
        self.assertEqual(
            {
                name: stat.S_IMODE((outputs / name).stat().st_mode)
                for name in phase.QUALIFIED_REPLICA_NAMES
            },
            before_modes,
        )

    def test_integrated_seal_rejects_same_name_output_inode_replacement(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "integrated-output-swap"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        target_name = sorted(phase.QUALIFIED_REPLICA_NAMES, key=os.fsencode)[0]
        target = outputs / target_name
        replacement_source = self.root / "held-original-output"
        real_fchmod = phase.os.fchmod
        swapped = False

        def replace_on_first_file(descriptor, mode):
            nonlocal swapped
            real_fchmod(descriptor, mode)
            if not swapped and stat.S_ISREG(os.fstat(descriptor).st_mode):
                swapped = True
                target.rename(replacement_source)
                target.write_bytes(replacement_source.read_bytes())

        with mock.patch.object(phase.os, "fchmod", side_effect=replace_on_first_file):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed|identity|seal"
            ):
                self.publish_and_seal_fixture(parent, outputs, dispatch)

    def test_integrated_seal_rejects_outputs_directory_replacement(self) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "integrated-directory-swap"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        displaced = self.root / "displaced-qualified-outputs"
        real_fchmod = phase.os.fchmod
        swapped = False

        def replace_on_directory_seal(descriptor, mode):
            nonlocal swapped
            metadata = os.fstat(descriptor)
            if (
                not swapped
                and stat.S_ISDIR(metadata.st_mode)
                and mode == phase.COLLECTABLE_OUTPUT_DIRECTORY_MODE
            ):
                swapped = True
                outputs.rename(displaced)
                shutil.copytree(displaced, outputs, copy_function=shutil.copyfile)
            real_fchmod(descriptor, mode)

        with mock.patch.object(phase.os, "fchmod", side_effect=replace_on_directory_seal):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed|identity|seal"
            ):
                self.publish_and_seal_fixture(parent, outputs, dispatch)

    def test_integrated_seal_rejects_in_place_content_mutation_during_seal(
        self,
    ) -> None:
        qualified, _ = self.qualified_replica_fixture()
        dispatch = self.dispatch_tag_fixture()
        parent = self.root / "integrated-content-mutation"
        parent.mkdir(mode=0o700)
        parent.chmod(0o700)
        outputs = parent / "outputs"
        qualified.rename(outputs)
        target = outputs / "guest-kernel"
        target.chmod(0o600)
        retained = os.open(target, os.O_RDWR)
        target_inode = os.fstat(retained).st_ino
        real_fchmod = phase.os.fchmod
        mutated = False

        def mutate_after_seal(descriptor, mode):
            nonlocal mutated
            real_fchmod(descriptor, mode)
            if not mutated and os.fstat(descriptor).st_ino == target_inode:
                mutated = True
                os.lseek(retained, 0, os.SEEK_SET)
                os.write(retained, b"KERNEL")
                os.fsync(retained)

        try:
            with mock.patch.object(phase.os, "fchmod", side_effect=mutate_after_seal):
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "changed|content|seal"
                ):
                    self.publish_and_seal_fixture(parent, outputs, dispatch)
        finally:
            os.close(retained)

    def test_public_cli_cannot_claim_success_or_publish_unsealed_provenance(
        self,
    ) -> None:
        parser = phase._parser()
        with mock.patch.object(phase.sys, "stderr", io.StringIO()):
            with self.assertRaises(SystemExit):
                parser.parse_args(
                    [
                        "seal-replica-bundle",
                        "--parent",
                        "/tmp/no",
                        "--parent-device",
                        "1",
                        "--parent-inode",
                        "2",
                        "--successful",
                        "yes",
                    ]
                )
            with self.assertRaises(SystemExit):
                parser.parse_args(["replica-provenance"])

    def test_recovery_record_is_create_once_and_binds_mount_scratch_and_parent(
        self,
    ) -> None:
        scratch = self.root / "recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "a" * 40 + "-r1"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "100",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "99",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)

        document = phase.publish_production_recovery_record(
            scratch=scratch,
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )

        record = scratch / phase.RECOVERY_RECORD_NAME
        self.assertEqual(record.read_bytes(), canonical(document))
        self.assertEqual(stat.S_IMODE(record.stat().st_mode), 0o444)
        observed = phase.verify_production_recovery_record(
            scratch=scratch,
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        self.assertEqual(observed, document)
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "create-once|already exists"
        ):
            phase.publish_production_recovery_record(
                scratch=scratch,
                expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
                expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )

    def test_pre_record_recovery_converges_known_crash_boundaries(self) -> None:
        stem = "boole-nsv4-" + "e" * 40 + "-r1"
        recovery_root = self.root / "pre-record-recovery-root"
        recovery_root.mkdir(mode=0o700)
        recovery_root.chmod(0o700)

        for shape in ("scratch-only", "staging-only", "partial-only"):
            with self.subTest(shape=shape):
                scratch = recovery_root / stem
                scratch.mkdir(mode=0o700)
                scratch.chmod(0o700)
                if shape != "scratch-only":
                    staging = scratch / "staging"
                    staging.mkdir(mode=0o700)
                    staging.chmod(0o700)
                if shape == "partial-only":
                    partial = scratch / f".{phase.RECOVERY_RECORD_NAME}.partial"
                    partial.write_bytes(b'{"schema":')
                    partial.chmod(0o400)
                outputs_parent = self.root / f"pre-record-output-{shape}"
                outputs_parent.mkdir(mode=0o700)
                outputs_parent.chmod(0o700)
                parent_info = outputs_parent.stat()
                self.assertEqual(
                    phase.discard_incomplete_production_recovery(
                        scratch=scratch,
                        outputs_parent=outputs_parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        recovery_stem=stem,
                    ),
                    "discarded-incomplete",
                )
                self.assertFalse(scratch.exists())
                self.assertEqual(
                    phase.discard_incomplete_production_recovery(
                        scratch=scratch,
                        outputs_parent=outputs_parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        recovery_stem=stem,
                    ),
                    "already-absent",
                )

    def test_live_pre_record_recovery_distinguishes_incomplete_and_linked_record(
        self,
    ) -> None:
        stem = "boole-nsv4-" + "f" * 40 + "-r2"
        scratch = self.root / stem
        staging = scratch / "staging"
        outputs_parent = self.root / "live-pre-record-output"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "300",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "299",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        shared = {
            "scratch": scratch,
            "outputs_parent": outputs_parent,
            "expected_parent_identity": (parent_info.st_dev, parent_info.st_ino),
            "expected_uid": os.geteuid(),
            "expected_gid": os.getegid(),
            "recovery_stem": stem,
            "mount_identity": mount_identity,
        }
        self.assertEqual(
            phase.reconcile_production_recovery_record_publication(**shared),
            "incomplete-no-record",
        )
        phase.publish_production_recovery_record(
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            **shared,
        )
        record = scratch / phase.RECOVERY_RECORD_NAME
        partial = scratch / f".{phase.RECOVERY_RECORD_NAME}.partial"
        os.link(record, partial)
        self.assertEqual(record.stat().st_nlink, 2)
        self.assertEqual(
            phase.reconcile_production_recovery_record_publication(**shared),
            "record-ready",
        )
        self.assertFalse(partial.exists())
        self.assertEqual(record.stat().st_nlink, 1)

    def test_completed_recovery_removal_uses_a_reentrant_tombstone(self) -> None:
        stem = "boole-nsv4-" + "9" * 40 + "-r1"
        recovery_root = self.root / "completed-recovery-root"
        scratch = recovery_root / stem
        staging = scratch / "staging"
        outputs_parent = self.root / "completed-recovery-output"
        staging.mkdir(parents=True, mode=0o700)
        recovery_root.chmod(0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "400",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "399",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        self.mock_absent_recovery_mount()
        shared = {
            "scratch": scratch,
            "outputs_parent": outputs_parent,
            "expected_parent_identity": (parent_info.st_dev, parent_info.st_ino),
            "expected_uid": os.geteuid(),
            "expected_gid": os.getegid(),
            "recovery_stem": stem,
        }
        phase.publish_production_recovery_record(
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            mount_identity=mount_identity,
            **shared,
        )
        phase.publish_production_cleanup_checkpoint(
            mount_identity=mount_identity,
            **shared,
        )
        real_unlink = phase.os.unlink
        failed = False

        def fail_after_tombstone(path, *args, **kwargs):
            nonlocal failed
            if path == phase.RECOVERY_CLEANUP_CHECKPOINT_NAME and not failed:
                failed = True
                raise OSError("injected cleanup interruption")
            return real_unlink(path, *args, **kwargs)

        with mock.patch.object(phase.os, "unlink", side_effect=fail_after_tombstone):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "cleanup|recovery|remove"
            ):
                phase.remove_verified_production_recovery(**shared)
        tombstone = recovery_root / f".{stem}.cleanup"
        self.assertFalse(scratch.exists())
        self.assertTrue(tombstone.is_dir())
        self.assertEqual(
            phase.remove_verified_production_recovery(**shared),
            "removed-verified",
        )
        self.assertFalse(tombstone.exists())
        self.assertEqual(
            phase.remove_verified_production_recovery(**shared),
            "already-absent",
        )

    def test_recovery_record_rejects_mount_or_parent_drift_without_rewriting(self) -> None:
        scratch = self.root / "recovery-drift-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "recovery-drift-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "b" * 40 + "-r2"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "200",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "199",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        document = phase.publish_production_recovery_record(
            scratch=scratch,
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        original = canonical(document)
        drifted = dict(mount_identity)
        drifted["mountId"] = "201"
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "mount|recovery"
        ):
            phase.verify_production_recovery_record(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=drifted,
            )
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "parent|recovery"
        ):
            phase.verify_production_recovery_record(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino + 1),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )
        self.assertEqual(
            (scratch / phase.RECOVERY_RECORD_NAME).read_bytes(), original
        )

    def test_recovery_record_rejects_unsafe_mount_facts_before_publication(
        self,
    ) -> None:
        scratch = self.root / "unsafe-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "unsafe-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "c" * 40 + "-r1"
        base = {
            "fileSystemType": "tmpfs",
            "majorMinor": "1:4",
            "mountId": "300",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "299",
            "root": "/",
            "source": stem,
            "superOptions": ["rw", "size=4096k"],
        }
        unsafe = []
        wrong_source = dict(base)
        wrong_source["source"] = "tmpfs"
        unsafe.append(wrong_source)
        wrong_target = dict(base)
        wrong_target["mountPoint"] = str(scratch / "other")
        unsafe.append(wrong_target)
        missing_nodev = dict(base)
        missing_nodev["mountOptions"] = ["nosuid", "rw"]
        unsafe.append(missing_nodev)
        extra_field = dict(base)
        extra_field["untrusted"] = True
        unsafe.append(extra_field)
        misplaced_flags = dict(base)
        misplaced_flags["mountOptions"] = ["rw"]
        misplaced_flags["superOptions"] = ["nodev", "nosuid", "rw"]
        unsafe.append(misplaced_flags)
        contradictory_flags = dict(base)
        contradictory_flags["mountOptions"] = [
            "dev",
            "nodev",
            "nosuid",
            "rw",
            "suid",
        ]
        unsafe.append(contradictory_flags)

        for mount_identity in unsafe:
            with self.subTest(mount_identity=mount_identity), self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "mount|recovery"
            ):
                phase.publish_production_recovery_record(
                    scratch=scratch,
                    expected_scratch_identity=(
                        scratch_info.st_dev,
                        scratch_info.st_ino,
                    ),
                    expected_staging_identity=(
                        staging_info.st_dev,
                        staging_info.st_ino,
                    ),
                    outputs_parent=outputs_parent,
                    expected_parent_identity=(
                        parent_info.st_dev,
                        parent_info.st_ino,
                    ),
                    expected_uid=os.geteuid(),
                    expected_gid=os.getegid(),
                    recovery_stem=stem,
                    mount_identity=mount_identity,
                )
            self.assertFalse((scratch / phase.RECOVERY_RECORD_NAME).exists())

    def test_recovery_record_rejects_a_plain_directory_claimed_as_tmpfs(self) -> None:
        scratch = self.root / "fictional-mount-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "fictional-mount-output"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "9" * 40 + "-r1"
        fictional = {
            "fileSystemType": "tmpfs",
            "majorMinor": (
                f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}"
            ),
            "mountId": "999",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "998",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "live|mount|tmpfs|recovery"
        ):
            phase.publish_production_recovery_record(
                scratch=scratch,
                expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
                expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=fictional,
            )

    def test_recovery_record_requires_empty_staging_and_a_separate_output_tree(
        self,
    ) -> None:
        for ordinal, nested_output in enumerate((False, True), start=1):
            with self.subTest(nested_output=nested_output):
                scratch = self.root / f"staging-boundary-scratch-{ordinal}"
                staging = scratch / "staging"
                staging.mkdir(parents=True, mode=0o700)
                scratch.chmod(0o700)
                staging.chmod(0o700)
                if nested_output:
                    outputs_parent = staging / "nested-output"
                    outputs_parent.mkdir(mode=0o700)
                else:
                    (staging / "foreign").mkdir(mode=0o700)
                    outputs_parent = self.root / f"separate-output-{ordinal}"
                    outputs_parent.mkdir(mode=0o700)
                outputs_parent.chmod(0o700)
                scratch_info = scratch.stat()
                staging_info = staging.stat()
                parent_info = outputs_parent.stat()
                stem = "boole-nsv4-" + f"{ordinal + 3}" * 40 + "-r2"
                mount_identity = {
                    "fileSystemType": "tmpfs",
                    "majorMinor": (
                        f"{os.major(staging_info.st_dev)}:"
                        f"{os.minor(staging_info.st_dev)}"
                    ),
                    "mountId": str(1000 + ordinal),
                    "mountOptions": ["nodev", "nosuid", "rw"],
                    "mountPoint": str(staging),
                    "parentId": str(900 + ordinal),
                    "root": "/",
                    "source": stem,
                    "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
                }
                with mock.patch.object(
                    phase,
                    "_read_live_recovery_mount_identity",
                    return_value=mount_identity,
                    create=True,
                ), self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "staging|empty|output|overlap|recovery",
                ):
                    phase.publish_production_recovery_record(
                        scratch=scratch,
                        expected_scratch_identity=(
                            scratch_info.st_dev,
                            scratch_info.st_ino,
                        ),
                        expected_staging_identity=(
                            staging_info.st_dev,
                            staging_info.st_ino,
                        ),
                        outputs_parent=outputs_parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        recovery_stem=stem,
                        mount_identity=mount_identity,
                    )

    def test_recovery_record_rejects_a_symlinked_ancestor(self) -> None:
        real_root = self.root / "real-recovery-root"
        real_root.mkdir(mode=0o700)
        alias = self.root / "recovery-root-alias"
        alias.symlink_to(real_root, target_is_directory=True)
        scratch = alias / "scratch"
        staging = scratch / "staging"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent = self.root / "symlink-ancestor-output"
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "8" * 40 + "-r2"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": (
                f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}"
            ),
            "mountId": "1100",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "1099",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        with mock.patch.object(
            phase,
            "_read_live_recovery_mount_identity",
            return_value=mount_identity,
            create=True,
        ), self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "symlink|path|directory|recovery"
        ):
            phase.publish_production_recovery_record(
                scratch=scratch,
                expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
                expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )

    def test_recovery_record_rejects_unexpected_scratch_members(self) -> None:
        scratch = self.root / "occupied-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "occupied-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        (scratch / "foreign").write_text("not ours\n")
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "d" * 40 + "-r2"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": "1:5",
            "mountId": "400",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "399",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "member|scratch|recovery"
        ):
            phase.publish_production_recovery_record(
                scratch=scratch,
                expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
                expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )
        self.assertFalse((scratch / phase.RECOVERY_RECORD_NAME).exists())

    def test_recovery_record_verification_rejects_mode_drift(self) -> None:
        scratch = self.root / "mode-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "mode-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "e" * 40 + "-r1"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "500",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "499",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        phase.publish_production_recovery_record(
            scratch=scratch,
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        (scratch / phase.RECOVERY_RECORD_NAME).chmod(0o600)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "mode|recovery"
        ):
            phase.verify_production_recovery_record(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )

    def test_recovery_record_requires_an_empty_dedicated_output_parent(self) -> None:
        scratch = self.root / "parent-members-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "parent-members-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        (outputs_parent / "foreign").write_text("not ours\n")
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "f" * 40 + "-r2"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "600",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "599",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "output parent|member|dedicated"
        ):
            phase.publish_production_recovery_record(
                scratch=scratch,
                expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
                expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )
        self.assertFalse((scratch / phase.RECOVERY_RECORD_NAME).exists())

    def test_recovery_record_verification_rejects_hardlink_or_staging_replacement(
        self,
    ) -> None:
        scratch = self.root / "replacement-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "replacement-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "1" * 40 + "-r1"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "700",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "699",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        phase.publish_production_recovery_record(
            scratch=scratch,
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        record = scratch / phase.RECOVERY_RECORD_NAME
        extra_link = scratch / "record-hardlink"
        os.link(record, extra_link)
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "identity|link|recovery"
        ):
            phase.verify_production_recovery_record(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )
        extra_link.unlink()
        old_staging = scratch / "old-staging"
        staging.rename(old_staging)
        staging.mkdir(mode=0o700)
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "staging|mount|recovery"
        ):
            phase.verify_production_recovery_record(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
                mount_identity=mount_identity,
            )

    def test_recovery_record_rechecks_live_directory_names_after_read(self) -> None:
        original_reader = phase._read_recovery_record_at

        for ordinal, replaced_name in enumerate(
            ("scratch", "staging", "output parent"), start=1
        ):
            with self.subTest(replaced_name=replaced_name):
                scratch = self.root / f"late-replacement-scratch-{ordinal}"
                staging = scratch / "staging"
                outputs_parent = self.root / f"late-replacement-output-{ordinal}"
                staging.mkdir(parents=True, mode=0o700)
                scratch.chmod(0o700)
                staging.chmod(0o700)
                outputs_parent.mkdir(mode=0o700)
                outputs_parent.chmod(0o700)
                scratch_info = scratch.stat()
                staging_info = staging.stat()
                parent_info = outputs_parent.stat()
                stem = "boole-nsv4-" + f"{ordinal}" * 40 + "-r1"
                mount_identity = {
                    "fileSystemType": "tmpfs",
                    "majorMinor": (
                        f"{os.major(staging_info.st_dev)}:"
                        f"{os.minor(staging_info.st_dev)}"
                    ),
                    "mountId": str(900 + ordinal),
                    "mountOptions": ["nodev", "nosuid", "rw"],
                    "mountPoint": str(staging),
                    "parentId": str(800 + ordinal),
                    "root": "/",
                    "source": stem,
                    "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
                }
                with mock.patch.object(
                    phase,
                    "_read_live_recovery_mount_identity",
                    return_value=mount_identity,
                ):
                    phase.publish_production_recovery_record(
                        scratch=scratch,
                        expected_scratch_identity=(
                            scratch_info.st_dev,
                            scratch_info.st_ino,
                        ),
                        expected_staging_identity=(
                            staging_info.st_dev,
                            staging_info.st_ino,
                        ),
                        outputs_parent=outputs_parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        recovery_stem=stem,
                        mount_identity=mount_identity,
                    )

                def read_then_replace(*args: object, **kwargs: object) -> dict[str, object]:
                    document = original_reader(*args, **kwargs)
                    if replaced_name == "scratch":
                        scratch.rename(self.root / f"old-scratch-{ordinal}")
                    elif replaced_name == "staging":
                        staging.rename(self.root / f"old-staging-{ordinal}")
                        staging.mkdir(mode=0o700)
                    else:
                        outputs_parent.rename(self.root / f"old-output-{ordinal}")
                        outputs_parent.mkdir(mode=0o700)
                        outputs_parent.chmod(0o700)
                    return document

                with mock.patch.object(
                    phase,
                    "_read_live_recovery_mount_identity",
                    return_value=mount_identity,
                ), mock.patch.object(
                    phase,
                    "_read_recovery_record_at",
                    side_effect=read_then_replace,
                ), self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error,
                    "identity|path|directory|staging|parent|scratch|recovery",
                ):
                    phase.verify_production_recovery_record(
                        scratch=scratch,
                        outputs_parent=outputs_parent,
                        expected_parent_identity=(
                            parent_info.st_dev,
                            parent_info.st_ino,
                        ),
                        expected_uid=os.geteuid(),
                        expected_gid=os.getegid(),
                        recovery_stem=stem,
                        mount_identity=mount_identity,
                    )
    def test_recovery_record_cli_round_trip_is_bounded_and_uses_process_owner(
        self,
    ) -> None:
        scratch = self.root / "cli-recovery-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "cli-recovery-output-parent"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "2" * 40 + "-r2"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}",
            "mountId": "800",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "799",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        shared = [
            "--scratch",
            str(scratch),
            "--outputs-parent",
            str(outputs_parent),
            "--parent-device",
            str(parent_info.st_dev),
            "--parent-inode",
            str(parent_info.st_ino),
            "--recovery-stem",
            stem,
        ]
        publish = [
            "publish-recovery-record",
            *shared,
            "--scratch-device",
            str(scratch_info.st_dev),
            "--scratch-inode",
            str(scratch_info.st_ino),
            "--staging-device",
            str(staging_info.st_dev),
            "--staging-inode",
            str(staging_info.st_ino),
        ]
        stdin = types.SimpleNamespace(buffer=io.BytesIO(canonical(mount_identity)))
        with mock.patch.object(phase.sys, "stdin", stdin), mock.patch.object(
            phase.sys, "stdout", io.StringIO()
        ):
            self.assertEqual(phase.main(publish), 0)
        verify = ["verify-recovery-record", *shared]
        stdin = types.SimpleNamespace(buffer=io.BytesIO(canonical(mount_identity)))
        stdout = io.StringIO()
        with mock.patch.object(phase.sys, "stdin", stdin), mock.patch.object(
            phase.sys, "stdout", stdout
        ):
            self.assertEqual(phase.main(verify), 0)
        self.assertIn("recovery record verified", stdout.getvalue())

    def test_cleanup_checkpoint_allows_idempotent_post_unmount_recovery(self) -> None:
        scratch = self.root / "cleanup-checkpoint-scratch"
        staging = scratch / "staging"
        outputs_parent = self.root / "cleanup-checkpoint-output"
        staging.mkdir(parents=True, mode=0o700)
        scratch.chmod(0o700)
        staging.chmod(0o700)
        outputs_parent.mkdir(mode=0o700)
        outputs_parent.chmod(0o700)
        scratch_info = scratch.stat()
        staging_info = staging.stat()
        parent_info = outputs_parent.stat()
        stem = "boole-nsv4-" + "7" * 40 + "-r1"
        mount_identity = {
            "fileSystemType": "tmpfs",
            "majorMinor": (
                f"{os.major(staging_info.st_dev)}:{os.minor(staging_info.st_dev)}"
            ),
            "mountId": "1200",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "1199",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.mock_live_recovery_mount(mount_identity)
        phase.publish_production_recovery_record(
            scratch=scratch,
            expected_scratch_identity=(scratch_info.st_dev, scratch_info.st_ino),
            expected_staging_identity=(staging_info.st_dev, staging_info.st_ino),
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        checkpoint = phase.publish_production_cleanup_checkpoint(
            scratch=scratch,
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        checkpoint_path = scratch / phase.RECOVERY_CLEANUP_CHECKPOINT_NAME
        original = checkpoint_path.read_bytes()
        self.assertEqual(original, canonical(checkpoint))
        with mock.patch.object(
            phase, "_read_live_recovery_mount_matches", return_value=[]
        ):
            observed = phase.verify_production_recovery_after_unmount(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
            )
        self.assertEqual(observed, checkpoint)
        with mock.patch.object(
            phase, "_read_live_recovery_mount_matches", return_value=[]
        ):
            observed_again = phase.verify_production_recovery_after_unmount(
                scratch=scratch,
                outputs_parent=outputs_parent,
                expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
                expected_uid=os.geteuid(),
                expected_gid=os.getegid(),
                recovery_stem=stem,
            )
        self.assertEqual(observed_again, checkpoint)
        self.assertEqual(checkpoint_path.read_bytes(), original)

        partial = scratch / f".{phase.RECOVERY_CLEANUP_CHECKPOINT_NAME}.partial"
        os.link(checkpoint_path, partial)
        recovered_link = phase.publish_production_cleanup_checkpoint(
            scratch=scratch,
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        self.assertEqual(recovered_link, checkpoint)
        self.assertFalse(partial.exists())
        self.assertEqual(checkpoint_path.stat().st_nlink, 1)

        checkpoint_path.rename(partial)
        recovered_partial = phase.publish_production_cleanup_checkpoint(
            scratch=scratch,
            outputs_parent=outputs_parent,
            expected_parent_identity=(parent_info.st_dev, parent_info.st_ino),
            expected_uid=os.geteuid(),
            expected_gid=os.getegid(),
            recovery_stem=stem,
            mount_identity=mount_identity,
        )
        self.assertEqual(recovered_partial, checkpoint)
        self.assertFalse(partial.exists())
        self.assertEqual(checkpoint_path.read_bytes(), original)

    def test_post_unmount_requires_zero_exact_mountinfo_matches(self) -> None:
        target = pathlib.Path("/run/boole/native-shadow-successor-v4/staging")
        other = pathlib.Path("/run/boole/native-shadow-successor-v4/other")

        def record(mount_point: pathlib.Path) -> bytes:
            return (
                f"91 22 0:55 / {mount_point} rw,nosuid,nodev - "
                "tmpfs boole-nsv4-fixture rw,size=4096,nr_inodes=16\n"
            ).encode()

        self.assertEqual(
            phase._parse_live_recovery_mountinfo(record(other), target),
            [],
        )
        matches = phase._parse_live_recovery_mountinfo(record(target), target)
        self.assertEqual(len(matches), 1)
        with mock.patch.object(
            phase, "_read_live_recovery_mount_matches", return_value=[]
        ):
            phase._require_absent_recovery_mount(target)
        with mock.patch.object(
            phase, "_read_live_recovery_mount_matches", return_value=matches
        ), self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "remains after unmount",
        ):
            phase._require_absent_recovery_mount(target)

    def test_post_unmount_mountinfo_rejects_malformed_or_oversized_input(
        self,
    ) -> None:
        target = pathlib.Path("/run/boole/native-shadow-successor-v4/staging")
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "line has no separator",
        ):
            phase._parse_live_recovery_mountinfo(b"malformed\n", target)
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "exceeds byte limit",
        ):
            phase._parse_live_recovery_mountinfo(
                b"x" * (phase.MAX_MOUNTINFO_BYTES + 1),
                target,
            )

    def test_recovery_mount_identity_requires_the_exact_tmpfs_caps(self) -> None:
        staging = pathlib.Path("/run/boole/native-shadow-successor-v4/staging")
        stem = "boole-nsv4-" + "a" * 40 + "-r1"
        base = {
            "fileSystemType": "tmpfs",
            "majorMinor": "0:55",
            "mountId": "91",
            "mountOptions": ["nodev", "nosuid", "rw"],
            "mountPoint": str(staging),
            "parentId": "22",
            "root": "/",
            "source": stem,
            "superOptions": ["nr_inodes=600000", "rw", "size=6291456k"],
        }
        self.assertEqual(
            phase._normalise_recovery_mount_identity(
                base,
                recovery_stem=stem,
                staging_path=staging,
            ),
            base,
        )
        byte_form = dict(base)
        byte_form["superOptions"] = [
            "nr_inodes=600000",
            "rw",
            "size=6442450944",
        ]
        self.assertEqual(
            phase._normalise_recovery_mount_identity(
                byte_form,
                recovery_stem=stem,
                staging_path=staging,
            ),
            byte_form,
        )

        for options in (
            ["rw", "size=6291456k"],
            ["nr_inodes=600000", "rw"],
            ["nr_inodes=599999", "rw", "size=6291456k"],
            ["nr_inodes=600000", "rw", "size=6291455k"],
            ["nr_inodes=600000", "rw", "size=6291457k"],
            ["nr_inodes=600000", "rw", "size=6291456k", "size=6442450944"],
        ):
            changed = dict(base)
            changed["superOptions"] = options
            with self.subTest(options=options), self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "tmpfs cap|superOptions",
            ):
                phase._normalise_recovery_mount_identity(
                    changed,
                    recovery_stem=stem,
                    staging_path=staging,
                )

    def test_production_scratch_rejects_every_unknown_member_before_prepare(
        self,
    ) -> None:
        scratch = self.root / "exact-empty-production-scratch"
        scratch.mkdir(mode=0o700)
        phase._require_empty_real_directory(scratch, "production scratch")
        (scratch / "unknown-member").write_bytes(b"unexpected")
        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error,
            "must be exactly empty",
        ):
            phase._require_empty_real_directory(scratch, "production scratch")
        for entrypoint in (phase.rehearse, phase.preflight, phase.produce):
            source = inspect.getsource(entrypoint)
            empty = source.index("_require_empty_real_directory(scratch_root")
            temporary = source.index("with _pinned_temporary_directory(scratch_root)")
            prepare = source.index("selected.prepare(request)", temporary)
            self.assertLess(empty, temporary)
            self.assertLess(temporary, prepare)

    def test_provenanced_comparison_cli_accepts_only_the_two_bound_envelopes(
        self,
    ) -> None:
        left, right, dispatch = self.provenanced_replica_bundles()
        stdin = types.SimpleNamespace(
            buffer=io.BytesIO(dispatch["raw_tag_object"])
        )
        stdout = io.StringIO()

        with mock.patch.object(phase.sys, "stdin", stdin), mock.patch.object(
            phase.sys, "stdout", stdout
        ):
            code = phase.main(
                [
                    "compare-provenanced-replicas",
                    "--repository-root",
                    str(self.root),
                    "--left-bundle",
                    str(left),
                    "--right-bundle",
                    str(right),
                    *self._dispatch_cli_arguments(dispatch),
                ]
            )

        self.assertEqual(code, 0)
        self.assertIn("provenanced replica comparison PASS", stdout.getvalue())
        with self.assertRaises(SystemExit):
            with mock.patch.object(phase.sys, "stderr", io.StringIO()):
                phase._parser().parse_args(["compare-replicas"])

    @staticmethod
    def _mutate_provenance(bundle, mutate):
        path = bundle / phase.REPLICA_PROVENANCE_NAME
        document = json.loads(path.read_text())
        mutate(document)
        path.write_bytes(canonical(document))

    def test_repository_backend_uses_only_v3_prepare_staging_before_low_level_effects(self) -> None:
        self.write_chain()
        chain = phase.verify_generation_chain(self.root)
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")
        request = phase.ProductionRequest(
            repository_root=self.root,
            artifact_store=store,
            outputs=self.root / "outputs",
            scratch=scratch,
            gpgv=self.root / "gpgv",
            zstd=self.root / "zstd",
            launcher=launcher,
            launcher_binary=b"launcher",
            chain=chain,
        )
        calls: list[str] = []

        def build_layout(lock, lock_raw, repository_root, artifact_store, output, **kwargs):
            calls.append("build-layout")
            output.mkdir(parents=True)
            raw = io.BytesIO()
            with tarfile.open(fileobj=raw, mode="w:") as archive:
                member = tarfile.TarInfo("etc/example")
                member.size = 1
                archive.addfile(member, io.BytesIO(b"x"))
            layer = raw.getvalue()
            digest = hashlib.sha256(layer).hexdigest()
            blob = output / "blobs" / "sha256" / digest
            blob.parent.mkdir(parents=True)
            blob.write_bytes(layer)
            return {"layerDigest": "sha256:" + digest}

        def extract_kernel(**kwargs):
            calls.append("kernel.extract")
            (kwargs["out_dir"] / "guest-kernel").write_bytes(b"kernel")
            return (
                {
                    "activationAllowed": False,
                    "bootableClaim": False,
                    "kernel": {
                        "architecture": "aarch64",
                        "magicOffset": 0x38,
                        "name": "guest-kernel",
                        "sha256": hashlib.sha256(b"kernel").hexdigest(),
                        "sizeBytes": len(b"kernel"),
                    }
                },
                "matched-the-seal",
            )

        def root_disk_plan(**kwargs):
            calls.append("root-disk.plan")
            return {"image": kwargs["image"]}

        def execute_root_disk(plan, layer, tree, writer_tree):
            calls.append("root-disk.execute")
            pathlib.Path(plan["image"]).write_bytes(b"root-disk")
            return {
                "activationAllowed": False,
                "bootableClaim": False,
                "image": {"sha256": hashlib.sha256(b"root-disk").hexdigest()},
                "fsck": {"passed": True},
                "loaderEvidence": {},
                "timeAudit": {"passed": True},
                "toolDigests": {},
                "writerTime": 1,
            }

        modules = {
            phase.MODULE_GATE: types.SimpleNamespace(
                materialize_runtime_lock=lambda sealed, raw, gpgv, zstd: (sealed, {})
            ),
            phase.MODULE_BASE: types.SimpleNamespace(
                normalized_runtime_lock=lambda runtime: (runtime, canonical(runtime), {})
            ),
            phase.MODULE_BUILDER_V4: types.SimpleNamespace(
                load_json_exact=lambda raw, context, require_canonical: json.loads(raw),
                validate_source_lock=lambda lock, raw, root, cas, require_complete: {
                    "lock": lock
                },
                nested_runtime_tree=lambda root, cas, gpgv, zstd: {},
                build_oci_layout=build_layout,
            ),
            phase.MODULE_V3: types.SimpleNamespace(
                prepare_staging=lambda **kwargs: (
                    calls.append("v3.prepare_staging")
                    or types.SimpleNamespace(entries={"etc/example": {}}, measurement={"entries": 1})
                )
            ),
            phase.MODULE_WRITER: types.SimpleNamespace(
                WRITER_TREE_PATH="usr/sbin/mke2fs",
                materialize=lambda **kwargs: calls.append("writer.materialize") or {"ok": True},
            ),
            phase.MODULE_ROOT_DISK: types.SimpleNamespace(
                E2FSCK_MEMBER_PATH="./usr/sbin/e2fsck",
                layer_entries=lambda layer: [{}],
                required_bytes=lambda entries: 4096,
                root_disk_plan=root_disk_plan,
            ),
            phase.MODULE_ROOT_EXECUTE: types.SimpleNamespace(execute=execute_root_disk),
            phase.MODULE_KERNEL: types.SimpleNamespace(extract=extract_kernel),
            phase.MODULE_INITRD: types.SimpleNamespace(
                initrd_bytes=lambda layer: calls.append("initrd.bytes") or b"initrd"
            ),
            phase.MODULE_IMAGE_VERIFY: types.SimpleNamespace(
                tree_from_initrd=lambda raw: {"etc/example": {}},
                expectations_from_lock=lambda lock: {},
                verify_tree=lambda **kwargs: calls.append("image.verify")
                or {"passed": True, "checks": []},
                assert_passed=lambda report: None,
            ),
            phase.MODULE_READBACK_V3: types.SimpleNamespace(
                HostReadbackEffects=lambda: types.SimpleNamespace(),
                verify=lambda **kwargs: calls.append("readback-v3.verify")
                or readback_document()
            ),
        }
        loads: dict[str, int] = {}

        def load_module(name):
            loads[name] = loads.get(name, 0) + 1
            return modules[name]

        backend = phase.RepositoryImageBackend(module_loader=load_module)

        prepared = backend.prepare(request)

        self.assertEqual(
            calls, ["v3.prepare_staging", "build-layout", "writer.materialize"]
        )
        self.assertEqual(prepared.measurement, {"entries": 1})
        self.assertEqual(prepared.state["layerEntryCount"], 1)

        request.outputs.mkdir()
        kernel = backend.extract_kernel(request, prepared)
        initrd = backend.build_initrd(request, prepared)
        (request.outputs / "guest-initrd").write_bytes(initrd)
        disk = backend.build_root_disk(request, prepared)
        report = backend.verify_images(request, prepared, kernel, initrd, disk)
        readback = backend.readback(
            request.repository_root, request.outputs, request.chain
        )

        self.assertTrue(report["passed"])
        self.assertEqual(readback["status"], phase.READBACK_PASS_STATUS)
        self.assertEqual(set(loads.values()), {1})
        self.assertEqual(
            calls[-6:],
            [
                "kernel.extract",
                "initrd.bytes",
                "root-disk.plan",
                "root-disk.execute",
                "image.verify",
                "readback-v3.verify",
            ],
        )

    def test_v4_readback_effects_make_loop_autoclear_without_weakening_cleanup(
        self,
    ) -> None:
        calls = []

        class Delegate:
            def unmet_requirements(self):
                return []

            def mount(self, device, mountpoint):
                calls.append(("mount", device, mountpoint))

            def read_tree(self, mountpoint):
                return {"mountpoint": str(mountpoint)}

            def unmount(self, mountpoint):
                calls.append(("unmount", mountpoint))

            def detach_loop(self, device):
                calls.append(("detach", device))

        def run(argv, **kwargs):
            calls.append(("run", tuple(argv), kwargs))
            return b"/dev/loop7\n"

        module = types.SimpleNamespace(
            HostReadbackEffects=Delegate,
            LOSETUP="/usr/sbin/losetup",
            ReadbackV3Error=RuntimeError,
            _run=run,
        )
        effects = phase.AutoclearReadbackEffects(module)
        image = types.SimpleNamespace(descriptor=17)

        self.assertEqual(effects.setup_loop(image), "/dev/loop7")
        self.assertEqual(
            calls[0],
            (
                "run",
                (
                    "/usr/sbin/losetup",
                    "--find",
                    "--show",
                    "--read-only",
                    "--autoclear",
                    "/proc/self/fd/17",
                ),
                {"pass_fds": (17,)},
            ),
        )
        effects.unmount(pathlib.Path("/mnt/readback"))
        effects.detach_loop("/dev/loop7")
        self.assertEqual(calls[-2][0], "unmount")
        self.assertEqual(calls[-1], ("detach", "/dev/loop7"))

    def test_controlled_import_rejects_unbound_transitive_before_side_effect(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        sentinel = self.root / "unbound-ran"
        allowed = scripts / "allowed.py"
        allowed.write_text("import scripts.unbound\n")
        (scripts / "unbound.py").write_text(
            f"from pathlib import Path\nPath({str(sentinel)!r}).write_text('ran')\n"
        )
        row = identity(self.root, "scripts/allowed.py")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        for name in ("scripts.allowed", "scripts.unbound"):
            sys.modules.pop(name, None)

        try:
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "unbound repository import"
            ):
                phase.RepositoryImageBackend()._controlled_imports(
                    request, ("scripts.allowed",)
                )
        finally:
            for name in ("scripts.allowed", "scripts.unbound"):
                sys.modules.pop(name, None)

        self.assertFalse(sentinel.exists())

    def test_isolated_python_opens_repo_imports_only_after_binding_verification(self) -> None:
        source = (
            "import pathlib,runpy,sys;"
            f"root=pathlib.Path({str(REPOSITORY_ROOT)!r});"
            "assert str(root) not in sys.path;"
            "g=runpy.run_path(str(root/'scripts/native_shadow_successor_produce_phase_arm64_v4.py'));"
            "assert str(root) not in sys.path;"
            "chain=g['verify_preregistered_generation'](root);"
            "request=g['RepositoryImportRequest'](root,chain);"
            "modules=g['RepositoryImageBackend']()._controlled_imports(request,g['LOW_LEVEL_MODULES']);"
            "assert len(modules)==10;"
            "assert str(root) not in sys.path"
        )
        environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")

        completed = subprocess.run(
            [sys.executable, "-I", "-S", "-c", source],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env=environment,
            timeout=30,
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode())

    def test_script_entrypoint_owns_the_preregistered_v4_bootstrap_before_r2(
        self,
    ) -> None:
        source = (
            "import importlib.util,pathlib,sys;"
            f"root=pathlib.Path({str(REPOSITORY_ROOT)!r});"
            "path=root/'scripts/native_shadow_successor_produce_phase_arm64_v4.py';"
            "spec=importlib.util.spec_from_file_location('v4_under_test',path);"
            "module=importlib.util.module_from_spec(spec);"
            "sys.modules[spec.name]=module;"
            "spec.loader.exec_module(module);"
            "sys.modules.pop(spec.name);"
            "module.__name__='__main__';"
            "sys.modules['__main__'].__file__=str(path);"
            "chain=module.verify_preregistered_generation(root);"
            "request=module.RepositoryImportRequest(root,chain);"
            "loaded=module.RepositoryImageBackend()._controlled_imports("
            "request,module.LOW_LEVEL_MODULES);"
            "assert len(loaded)==10"
        )
        environment = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")

        completed = subprocess.run(
            [sys.executable, "-I", "-S", "-c", source],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            env=environment,
            timeout=30,
        )

        self.assertEqual(completed.returncode, 0, completed.stderr.decode())

    def test_controlled_import_rejects_preloaded_bound_module(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        allowed = scripts / "preloaded.py"
        allowed.write_text("VALUE = 1\n")
        row = identity(self.root, "scripts/preloaded.py")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        name = "scripts.preloaded"
        sys.modules[name] = types.ModuleType(name)
        try:
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "already loaded"
            ):
                phase.RepositoryImageBackend()._controlled_imports(request, (name,))
        finally:
            sys.modules.pop(name, None)

    def test_controlled_import_rejects_any_preloaded_repository_module(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        allowed = scripts / "allowed.py"
        allowed.write_text("import scripts.unbound_preloaded\nVALUE = 1\n")
        unbound = scripts / "unbound_preloaded.py"
        unbound.write_text("VALUE = 2\n")
        row = identity(self.root, "scripts/allowed.py")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        name = "scripts.unbound_preloaded"
        module = types.ModuleType(name)
        module.__file__ = str(unbound)
        sys.modules[name] = module
        try:
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "preloaded repository module is not backend-owned",
            ):
                phase.RepositoryImageBackend()._controlled_imports(
                    request, ("scripts.allowed",)
                )
        finally:
            sys.modules.pop("scripts.allowed", None)
            sys.modules.pop(name, None)

    def test_bootstrap_path_does_not_authorise_a_preloaded_alias(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        bootstrap = self.root / phase.V4_PATHS[0]
        bootstrap.write_text("VALUE = 'sealed bootstrap bytes'\n")
        requested = scripts / "requested_after_alias.py"
        requested.write_text("VALUE = 1\n")
        rows = [
            identity(self.root, phase.V4_PATHS[0]),
            identity(self.root, "scripts/requested_after_alias.py"),
        ]
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(
                rehearsal={"boundInputs": [rows[1]]},
                fresh_rehearsal={"generationFiles": [rows[0]]},
            ),
        )
        alias = "scripts.attacker_chosen_bootstrap_alias"
        module = types.ModuleType(alias)
        module.__file__ = str(bootstrap)
        sys.modules[alias] = module
        try:
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "preloaded repository module is not backend-owned",
            ):
                phase.RepositoryImageBackend()._controlled_imports(
                    request, ("scripts.requested_after_alias",)
                )
        finally:
            sys.modules.pop(alias, None)
            sys.modules.pop("scripts.requested_after_alias", None)

    def test_controlled_import_rejects_tampered_bound_module_before_loader(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        allowed = scripts / "tampered.py"
        allowed.write_text("VALUE = 1\n")
        row = identity(self.root, "scripts/tampered.py")
        allowed.write_text("VALUE = 2\n")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        loader = mock.Mock(side_effect=AssertionError("loader must not run"))
        backend = phase.RepositoryImageBackend()
        backend._module_loader = loader

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "bound repository module identity differs"
        ):
            backend._controlled_imports(request, ("scripts.tampered",))

        loader.assert_not_called()

    def test_controlled_import_preverifies_unrequested_bound_python_too(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        requested = scripts / "requested.py"
        requested.write_text("VALUE = 1\n")
        unrequested = scripts / "unrequested.py"
        unrequested.write_text("VALUE = 1\n")
        rows = [
            identity(self.root, "scripts/requested.py"),
            identity(self.root, "scripts/unrequested.py"),
        ]
        unrequested.write_text("VALUE = 2\n")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": rows}),
        )
        loader = mock.Mock(side_effect=AssertionError("loader must not run"))
        backend = phase.RepositoryImageBackend()
        backend._module_loader = loader

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "bound repository module identity differs"
        ):
            backend._controlled_imports(request, ("scripts.requested",))

        loader.assert_not_called()

    def test_controlled_import_rejects_module_that_changes_its_own_bytes(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        mutating = scripts / "mutating.py"
        mutating.write_text(
            "from pathlib import Path\n"
            "Path(__file__).write_text('VALUE = 2\\n')\n"
        )
        row = identity(self.root, "scripts/mutating.py")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        name = "scripts.mutating"
        sys.modules.pop(name, None)
        try:
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "post-import repository module identity differs"
            ):
                phase.RepositoryImageBackend()._controlled_imports(request, (name,))
        finally:
            sys.modules.pop(name, None)

    def test_failed_controlled_import_removes_new_repository_modules(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        (scripts / "helper.py").write_text("VALUE = 1\n")
        (scripts / "failing.py").write_text(
            "import scripts.helper\nraise RuntimeError('injected import failure')\n"
        )
        rows = [
            identity(self.root, "scripts/failing.py"),
            identity(self.root, "scripts/helper.py"),
        ]
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": rows}),
        )
        names = ("scripts.failing", "scripts.helper")
        for name in names:
            sys.modules.pop(name, None)
        backend = phase.RepositoryImageBackend()

        with self.assertRaisesRegex(RuntimeError, "injected import failure"):
            backend._controlled_imports(request, ("scripts.failing",))

        self.assertEqual([name for name in names if name in sys.modules], [])
        self.assertEqual(backend._owned_modules, set())

    def test_failed_controlled_import_removes_new_module_without_file(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        (scripts / "failing_without_file.py").write_text(
            "import sys, types\n"
            "sys.modules['injected_without_file'] = types.ModuleType('injected_without_file')\n"
            "raise RuntimeError('injected import failure after module insertion')\n"
        )
        row = identity(self.root, "scripts/failing_without_file.py")
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": [row]}),
        )
        target = "scripts.failing_without_file"
        for name in (target, "injected_without_file"):
            sys.modules.pop(name, None)
        backend = phase.RepositoryImageBackend()

        with self.assertRaisesRegex(RuntimeError, "after module insertion"):
            backend._controlled_imports(request, (target,))

        self.assertNotIn(target, sys.modules)
        self.assertNotIn("injected_without_file", sys.modules)
        self.assertEqual(backend._owned_modules, set())

    def test_same_backend_may_reuse_its_verified_transitive_module(self) -> None:
        scripts = self.root / "scripts"
        scripts.mkdir(exist_ok=True)
        (scripts / "second.py").write_text("VALUE = 2\n")
        (scripts / "first.py").write_text("import scripts.second\nVALUE = 1\n")
        rows = [
            identity(self.root, "scripts/first.py"),
            identity(self.root, "scripts/second.py"),
        ]
        request = types.SimpleNamespace(
            repository_root=self.root,
            chain=types.SimpleNamespace(rehearsal={"boundInputs": rows}),
        )
        names = ("scripts.first", "scripts.second")
        for name in names:
            sys.modules.pop(name, None)
        backend = phase.RepositoryImageBackend()
        try:
            first = backend._controlled_imports(request, ("scripts.first",))
            second = backend._controlled_imports(request, ("scripts.second",))
        finally:
            for name in names:
                sys.modules.pop(name, None)

        self.assertEqual(first["scripts.first"].VALUE, 1)
        self.assertEqual(second["scripts.second"].VALUE, 2)

    def test_fresh_rehearsal_can_seal_r2_without_future_authority_or_outputs(self) -> None:
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class RehearsalBackend:
            def prepare(self, request):
                self.output_was_absent = not request.outputs.exists()
                self.tempdir = pathlib.Path(tempfile.gettempdir())
                self.environment_tempdirs = tuple(
                    pathlib.Path(os.environ[name]) for name in ("TMPDIR", "TMP", "TEMP")
                )
                return phase.PreparedProduction(
                    measurement={"entries": 17677, "payloadBytes": 1773456499},
                    build_receipt={"scratchOnly": True},
                    state={},
                )

        backend = RehearsalBackend()
        previous_environment = {
            name: os.environ.get(name) for name in ("TMPDIR", "TMP", "TEMP")
        }
        with mock.patch.object(
            phase,
            "_read_cgroup_execution_observation",
            side_effect=(cgroup_observation(), cgroup_observation()),
        ) as read_observation:
            result = phase.rehearse(
                repository_root=self.root,
                artifact_store=store,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=backend,
                expected_systemd_unit="boole-nsv4-rehearsal-ABC123.service",
            )

        self.assertTrue(backend.output_was_absent)
        self.assertTrue(backend.tempdir.is_relative_to(scratch))
        self.assertEqual(
            backend.environment_tempdirs,
            (backend.tempdir, backend.tempdir, backend.tempdir),
        )
        self.assertEqual(
            {name: os.environ.get(name) for name in previous_environment},
            previous_environment,
        )
        self.assertEqual(result["schema"], phase.R2_SCHEMA)
        self.assertEqual(result["status"], phase.R2_STATUS)
        self.assertEqual(result["effects"], phase.ZERO_EFFECTS)
        self.assertEqual(result["executionEnvelope"], execution_envelope())
        self.assertEqual(read_observation.call_count, 2)
        read_observation.assert_has_calls(
            [
                mock.call(
                    expected_systemd_unit=(
                        "boole-nsv4-rehearsal-ABC123.service"
                    )
                ),
                mock.call(
                    expected_systemd_unit=(
                        "boole-nsv4-rehearsal-ABC123.service"
                    )
                ),
            ]
        )
        self.assertEqual(
            result["productionDispatchFenceCorrection"],
            identity(self.root, P3_RELATIVE),
        )
        self.assertFalse((self.root / phase.R2_PATH).exists())
        self.assertFalse((self.root / phase.CONSUMED_MARKER_NAME).exists())

    def test_production_check_cli_verifies_authority_without_loading_backend(self) -> None:
        self.write_chain()
        with mock.patch.object(
            phase.importlib,
            "import_module",
            side_effect=AssertionError("production-check imported a repository module"),
        ):
            code = phase.main(
                ["production-check", "--repository-root", str(self.root)]
            )

        self.assertEqual(code, 0)

    def test_produce_cli_rejects_wrong_result_path_before_produce_effect(self) -> None:
        outputs = self.root / "outputs"
        wrong = self.root / "wrong-result.json"
        with mock.patch.object(phase, "produce", return_value={}) as produce:
            code = phase.main(
                [
                    "produce",
                    "--repository-root",
                    str(self.root),
                    "--cas",
                    str(self.root / "cas"),
                    "--launcher",
                    str(self.root / "launcher"),
                    "--scratch",
                    str(self.root / "scratch"),
                    "--gpgv",
                    str(self.root / "gpgv"),
                    "--zstd",
                    str(self.root / "zstd"),
                    "--outputs",
                    str(outputs),
                    "--result",
                    str(wrong),
                    "--claim-ref",
                    "refs/tags/claim",
                    "--ref-object-sha",
                    "1" * 40,
                    "--tag-object-sha",
                    "1" * 40,
                    "--github-run-id",
                    "1",
                    "--github-run-attempt",
                    "1",
                    "--workflow-path",
                    phase.V4_WORKFLOW_PATH,
                    "--head-sha",
                    "2" * 40,
                    "--head-a6-sha256",
                    "3" * 64,
                ]
            )

        self.assertEqual(code, 1)
        produce.assert_not_called()

    def test_terminal_authority_extra_key_is_rejected_before_backend_effect(self) -> None:
        self.write_chain()
        authority_path = self.root / phase.A6_PATH
        authority = json.loads(authority_path.read_text())
        authority["quietlyAddedAuthority"] = True
        authority_path.write_bytes(canonical(authority))

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "A6 keys differ"):
            phase.verify_generation_chain(self.root)

    def test_a6_integer_budget_rejects_boolean_alias(self) -> None:
        self.write_chain()
        authority_path = self.root / phase.A6_PATH
        authority = json.loads(authority_path.read_text())
        authority["grant"]["workflowDispatchesAllowed"] = True
        authority_path.write_bytes(canonical(authority))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "A6 dispatch count differs"
        ):
            phase.verify_generation_chain(self.root)

    def test_a6_boolean_authority_rejects_integer_alias(self) -> None:
        self.write_chain()
        authority_path = self.root / phase.A6_PATH
        authority = json.loads(authority_path.read_text())
        authority["authorisations"]["miningActivated"] = 0
        authority_path.write_bytes(canonical(authority))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "A6 grants more or less"
        ):
            phase.verify_generation_chain(self.root)

    def test_r2_integer_effect_rejects_boolean_alias(self) -> None:
        self.write_chain()
        r2_path = self.root / phase.R2_PATH
        r2 = json.loads(r2_path.read_text())
        r2["effects"]["imageOutputsCreated"] = False
        r2_path.write_bytes(canonical(r2))
        r2_identity = identity(self.root, phase.R2_PATH)
        f6_path = self.root / phase.F6_PATH
        f6 = json.loads(f6_path.read_text())
        f6["predecessors"][-1] = r2_identity
        f6_path.write_bytes(canonical(f6))
        a6_path = self.root / phase.A6_PATH
        a6 = json.loads(a6_path.read_text())
        a6["predecessors"][-2:] = [r2_identity, identity(self.root, phase.F6_PATH)]
        a6_path.write_bytes(canonical(a6))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "R2 effect accounting differs"
        ):
            phase.verify_generation_chain(self.root)

    def test_f6_false_boundary_rejects_integer_alias(self) -> None:
        self.write_chain()
        f6_path = self.root / phase.F6_PATH
        f6 = json.loads(f6_path.read_text())
        f6["boundaries"]["bootableClaim"] = 0
        f6_path.write_bytes(canonical(f6))
        a6_path = self.root / phase.A6_PATH
        a6 = json.loads(a6_path.read_text())
        a6["predecessors"][-1] = identity(self.root, phase.F6_PATH)
        a6_path.write_bytes(canonical(a6))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "F6 boundaries differs"
        ):
            phase.verify_generation_chain(self.root)

    def test_dangling_withdrawn_reservation_is_not_treated_as_absent(self) -> None:
        self.write_chain()
        withdrawn = self.root / phase.WITHDRAWN_A5_PATH
        withdrawn.symlink_to("does-not-exist")

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "withdrawn authority-v5 must remain absent"
        ):
            phase.verify_generation_chain(self.root)

    def test_bound_file_swap_between_lstat_and_open_is_rejected(self) -> None:
        bound = self.root / "bound"
        replacement = self.root / "replacement"
        bound.write_bytes(b"first")
        replacement.write_bytes(b"other")
        real_open = os.open

        def swapped_open(path, flags, *args, **kwargs):
            if pathlib.Path(path).name == "bound" and kwargs.get("dir_fd") is not None:
                return real_open(str(replacement), flags, *args, **kwargs)
            return real_open(path, flags, *args, **kwargs)

        with mock.patch.object(phase.os, "open", side_effect=swapped_open):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "changed between inspection and open"
            ):
                phase._read_regular(self.root, "bound")

    def test_bound_file_rejects_symlinked_parent_component(self) -> None:
        real = self.root / "real"
        real.mkdir()
        (real / "binding").write_bytes(b"value")
        (self.root / "alias").symlink_to(real, target_is_directory=True)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "parent component"
        ):
            phase._read_regular(self.root, "alias/binding")

    def test_launcher_reader_rejects_a_swap_between_inspection_and_open(self) -> None:
        launcher = self.root / "launcher"
        replacement = self.root / "replacement-launcher"
        launcher.write_bytes(b"original-launcher")
        replacement.write_bytes(b"replacement-launcher")
        real_open = os.open

        def swapped_open(path, flags, *arguments, **keywords):
            if path == "launcher" and keywords.get("dir_fd") is not None:
                return real_open(str(replacement), flags, *arguments, **keywords)
            return real_open(path, flags, *arguments, **keywords)

        with mock.patch.object(phase.os, "open", side_effect=swapped_open):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "changed between inspection and open",
            ):
                phase._launcher_bytes(launcher)

    def test_launcher_reader_has_a_fixed_memory_ceiling(self) -> None:
        launcher = self.root / "oversized-launcher"
        with launcher.open("wb") as handle:
            handle.truncate(phase.MAX_LAUNCHER_BYTES + 1)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "metadata exceeds byte limit"
        ):
            phase._launcher_bytes(launcher)

    def test_repository_backend_requires_the_exact_sealed_launcher(self) -> None:
        result_path = (
            self.root
            / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
        )
        result_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(
            REPOSITORY_ROOT
            / "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
            result_path,
        )
        document = json.loads(result_path.read_text())
        self.assertEqual(
            phase.SEALED_LAUNCHER_SHA256, document["launcher"]["sha256"]
        )
        self.assertEqual(
            phase.SEALED_LAUNCHER_SIZE_BYTES,
            document["launcher"]["sizeBytes"],
        )
        launcher = self.root / "wrong-launcher"
        launcher.write_bytes(b"not-the-sealed-launcher")

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "launcher-v2 sealed identity differs"
        ):
            phase._launcher_bytes(launcher, require_sealed=True)

    def test_layer_rejects_absolute_symlink_target(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            member = tarfile.TarInfo("escape")
            member.type = tarfile.SYMTYPE
            member.linkname = "/outside"
            archive.addfile(member)

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "link escapes"
        ):
            phase.RepositoryImageBackend._extract_layer(
                raw.getvalue(), self.root / "tree"
            )

    def test_layer_rejects_parent_traversing_symlink_target(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            member = tarfile.TarInfo("inside/escape")
            member.type = tarfile.SYMTYPE
            member.linkname = "../../outside"
            archive.addfile(member)

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "link escapes"):
            phase.RepositoryImageBackend._extract_layer(
                raw.getvalue(), self.root / "tree"
            )

    def test_layer_rejects_regular_member_below_symlink_parent(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            link = tarfile.TarInfo("alias")
            link.type = tarfile.SYMTYPE
            link.linkname = "real"
            archive.addfile(link)
            payload = tarfile.TarInfo("alias/payload")
            payload.size = 1
            archive.addfile(payload, io.BytesIO(b"x"))

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "traverses a symlink parent"
        ):
            phase.RepositoryImageBackend._extract_layer(
                raw.getvalue(), self.root / "tree"
            )

    def test_layer_rejects_special_member(self) -> None:
        for index, member_type in enumerate(
            (tarfile.CHRTYPE, tarfile.BLKTYPE, tarfile.FIFOTYPE, b"s")
        ):
            with self.subTest(member_type=member_type):
                raw = io.BytesIO()
                with tarfile.open(fileobj=raw, mode="w:") as archive:
                    special = tarfile.TarInfo(f"special-{index}")
                    special.type = member_type
                    archive.addfile(special)

                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "contains special member"
                ):
                    phase.RepositoryImageBackend._extract_layer(
                        raw.getvalue(), self.root / f"tree-{index}"
                    )

    def test_layer_rejects_hardlink_through_symlink(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            payload = tarfile.TarInfo("real/payload")
            payload.size = 1
            archive.addfile(payload, io.BytesIO(b"x"))
            link = tarfile.TarInfo("alias")
            link.type = tarfile.SYMTYPE
            link.linkname = "real"
            archive.addfile(link)
            hardlink = tarfile.TarInfo("copy")
            hardlink.type = tarfile.LNKTYPE
            hardlink.linkname = "alias/payload"
            archive.addfile(hardlink)

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "hardlink"):
            phase.RepositoryImageBackend._extract_layer(
                raw.getvalue(), self.root / "tree"
            )

    def test_layer_rejects_symlink_cycle_before_extraction(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            first = tarfile.TarInfo("a")
            first.type = tarfile.SYMTYPE
            first.linkname = "b"
            archive.addfile(first)
            second = tarfile.TarInfo("b")
            second.type = tarfile.SYMTYPE
            second.linkname = "a"
            archive.addfile(second)

        with self.assertRaisesRegex(phase.SuccessorProduceV4Error, "symlink cycle"):
            phase.RepositoryImageBackend._extract_layer(
                raw.getvalue(), self.root / "tree"
            )

    def test_layer_accepts_safe_symlink_chain_within_tree(self) -> None:
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w:") as archive:
            real = tarfile.TarInfo("real")
            real.type = tarfile.DIRTYPE
            real.mode = 0o755
            archive.addfile(real)
            payload = tarfile.TarInfo("real/payload")
            payload.size = 1
            payload.mode = 0o644
            archive.addfile(payload, io.BytesIO(b"x"))
            middle = tarfile.TarInfo("middle")
            middle.type = tarfile.SYMTYPE
            middle.linkname = "real"
            archive.addfile(middle)
            outer = tarfile.TarInfo("outer")
            outer.type = tarfile.SYMTYPE
            outer.linkname = "middle/payload"
            archive.addfile(outer)

        destination = self.root / "tree"
        count = phase.RepositoryImageBackend._extract_layer(
            raw.getvalue(), destination
        )

        self.assertEqual(count, 4)
        self.assertEqual((destination / "outer").read_bytes(), b"x")

    def test_failure_after_marker_is_kept_and_disowned(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class FailingBackend:
            def prepare(self, request):
                return phase.PreparedProduction({}, {}, {})

            def extract_kernel(self, request, prepared):
                (request.outputs / "guest-kernel").write_bytes(b"kernel")
                return {"sha256": hashlib.sha256(b"kernel").hexdigest()}

            def build_initrd(self, request, prepared):
                return b"initrd"

            def build_root_disk(self, request, prepared):
                raise RuntimeError("injected disk failure")

        with self.assertRaisesRegex(RuntimeError, "injected disk failure"):
            phase.produce(
                repository_root=self.root,
                artifact_store=store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=FailingBackend(),
                dispatch_capability=self.dispatch_tag_fixture(),
            )

        marker = json.loads((outputs / phase.CONSUMED_MARKER_NAME).read_text())
        diagnostic = json.loads((outputs / phase.UNQUALIFIED_MARKER_NAME).read_text())
        self.assertTrue(marker["consumed"])
        self.assertEqual(diagnostic["status"], "UNQUALIFIED-DIAGNOSTIC")
        self.assertFalse(diagnostic["mayBeAdopted"])
        self.assertFalse(diagnostic["mayBeBooted"])
        self.assertEqual(
            diagnostic["filesKept"], ["guest-initrd", "guest-kernel"]
        )
        self.assertFalse((outputs / phase.PENDING_RESULT_NAME).exists())

    def test_output_parent_fsync_failure_precedes_marker_and_image_effects(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class Backend:
            image_effects = 0

            def prepare(self, request):
                return phase.PreparedProduction({}, {}, {})

            def extract_kernel(self, request, prepared):
                self.image_effects += 1
                raise AssertionError("image effect ran before parent fsync")

        real_fsync = phase._fsync_directory

        def fail_parent(directory):
            if pathlib.Path(directory).resolve() == outputs.parent.resolve():
                raise phase.SuccessorProduceV4Error("injected output-parent fsync failure")
            return real_fsync(directory)

        backend = Backend()
        with mock.patch.object(phase, "_fsync_directory", side_effect=fail_parent):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error, "output-parent fsync failure"
            ):
                phase.produce(
                    repository_root=self.root,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=self.root / "gpgv",
                    zstd=self.root / "zstd",
                    launcher=launcher,
                    backend=backend,
                    dispatch_capability=self.dispatch_tag_fixture(),
                )

        self.assertEqual(backend.image_effects, 0)
        self.assertFalse((outputs / phase.CONSUMED_MARKER_NAME).exists())

    def test_all_images_are_synced_before_pending_result_is_published(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class Backend:
            def prepare(self, request):
                return phase.PreparedProduction({}, {}, {})

            def extract_kernel(self, request, prepared):
                (request.outputs / "guest-kernel").write_bytes(b"kernel")
                return {"sha256": hashlib.sha256(b"kernel").hexdigest()}

            def build_initrd(self, request, prepared):
                return b"initrd"

            def build_root_disk(self, request, prepared):
                (request.outputs / "guest-root-disk").write_bytes(b"root-disk")
                return {"image": {"sha256": hashlib.sha256(b"root-disk").hexdigest()}}

            def verify_images(self, request, prepared, kernel, initrd, root_disk):
                return {"passed": True}

        synced: list[int] = []
        real_fdatasync = getattr(phase.os, "fdatasync", phase.os.fsync)
        real_publish = phase._publish_json_once

        def observe_sync(descriptor):
            synced.append(descriptor)
            return real_fdatasync(descriptor)

        def require_sync_before_pending(path, document):
            if pathlib.Path(path).name == phase.PENDING_RESULT_NAME:
                self.assertEqual(len(synced), len(phase.OUTPUT_NAMES))
            return real_publish(path, document)

        with mock.patch.object(
            phase.os, "fdatasync", side_effect=observe_sync, create=True
        ), mock.patch.object(
            phase, "_publish_json_once", side_effect=require_sync_before_pending
        ):
            phase.produce(
                repository_root=self.root,
                artifact_store=store,
                outputs=outputs,
                scratch=scratch,
                gpgv=self.root / "gpgv",
                zstd=self.root / "zstd",
                launcher=launcher,
                backend=Backend(),
                dispatch_capability=self.dispatch_tag_fixture(),
            )

        self.assertEqual(len(synced), len(phase.OUTPUT_NAMES))

    def test_image_sync_failure_prevents_pending_result_publication(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class Backend:
            def prepare(self, request):
                return phase.PreparedProduction({}, {}, {})

            def extract_kernel(self, request, prepared):
                (request.outputs / "guest-kernel").write_bytes(b"kernel")
                return {"sha256": hashlib.sha256(b"kernel").hexdigest()}

            def build_initrd(self, request, prepared):
                return b"initrd"

            def build_root_disk(self, request, prepared):
                (request.outputs / "guest-root-disk").write_bytes(b"root-disk")
                return {"image": {"sha256": hashlib.sha256(b"root-disk").hexdigest()}}

            def verify_images(self, request, prepared, kernel, initrd, root_disk):
                return {"passed": True}

        with mock.patch.object(
            phase.os,
            "fdatasync",
            side_effect=OSError("injected power-loss sync failure"),
            create=True,
        ):
            with self.assertRaisesRegex(
                phase.SuccessorProduceV4Error,
                "cannot make produced image durable",
            ):
                phase.produce(
                    repository_root=self.root,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=self.root / "gpgv",
                    zstd=self.root / "zstd",
                    launcher=launcher,
                    backend=Backend(),
                    dispatch_capability=self.dispatch_tag_fixture(),
                )

        self.assertFalse((outputs / phase.PENDING_RESULT_NAME).exists())

    def test_collectability_failure_preserves_original_failure(self) -> None:
        self.write_chain()
        outputs = self.root / "outputs"
        scratch = self.root / "scratch"
        store = self.root / "cas"
        scratch.mkdir()
        store.mkdir()
        launcher = self.root / "launcher"
        launcher.write_bytes(b"launcher")

        class Backend:
            def prepare(self, request):
                return phase.PreparedProduction({}, {}, {})

            def extract_kernel(self, request, prepared):
                kernel = request.outputs / "guest-kernel"
                kernel.write_bytes(b"kernel")
                kernel.chmod(0o600)
                return {"sha256": hashlib.sha256(b"kernel").hexdigest()}

            def build_initrd(self, request, prepared):
                return b"initrd"

            def build_root_disk(self, request, prepared):
                raise RuntimeError("original root-disk failure")

        with mock.patch.object(
            phase,
            "_make_outputs_collectable",
            side_effect=PermissionError("injected chmod failure"),
        ):
            with self.assertRaises(phase.SuccessorProduceV4Error) as raised:
                phase.produce(
                    repository_root=self.root,
                    artifact_store=store,
                    outputs=outputs,
                    scratch=scratch,
                    gpgv=self.root / "gpgv",
                    zstd=self.root / "zstd",
                    launcher=launcher,
                    backend=Backend(),
                    dispatch_capability=self.dispatch_tag_fixture(),
                )

        self.assertIn("output collectability failed", str(raised.exception))
        self.assertIn("original root-disk failure", str(raised.exception))
        self.assertIn("injected chmod failure", str(raised.exception))

    def test_collectability_rejects_external_hardlinks_without_changing_them(self) -> None:
        outputs = self.root / "outputs"
        outputs.mkdir()
        external = self.root / "external"
        external.write_bytes(b"kernel")
        external.chmod(0o600)
        os.link(external, outputs / "guest-kernel")
        (outputs / phase.CONSUMED_MARKER_NAME).write_bytes(b"{}\n")

        with self.assertRaisesRegex(
            phase.SuccessorProduceV4Error, "hardlink|link count"
        ):
            phase._make_outputs_collectable(outputs)

        self.assertEqual(stat.S_IMODE(external.stat().st_mode), 0o600)
        self.assertEqual(external.stat().st_nlink, 2)

    def test_collectability_rejects_special_or_executable_members(self) -> None:
        for unsafe_mode in (0o1644, 0o755, 0o666, 0o620):
            with self.subTest(mode=oct(unsafe_mode)):
                outputs = self.root / f"outputs-{unsafe_mode:o}"
                outputs.mkdir()
                member = outputs / phase.CONSUMED_MARKER_NAME
                member.write_bytes(b"{}\n")
                member.chmod(unsafe_mode)
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "unsafe mode"
                ):
                    phase._make_outputs_collectable(outputs)

    def test_collectability_rejects_setid_bits_seen_on_the_open_descriptor(
        self,
    ) -> None:
        # APFS may clear set-id bits as soon as an unprivileged test process
        # writes the file.  Inject the kernel-observed fstat bit instead of
        # silently turning this security regression into a platform skip.
        real_fstat = phase.os.fstat
        for unsafe_bit in (stat.S_ISUID, stat.S_ISGID):
            with self.subTest(bit=oct(unsafe_bit)):
                outputs = self.root / f"outputs-setid-{unsafe_bit:o}"
                outputs.mkdir()
                member = outputs / phase.CONSUMED_MARKER_NAME
                member.write_bytes(b"{}\n")

                def fstat_with_setid(descriptor):
                    info = real_fstat(descriptor)
                    if stat.S_ISREG(info.st_mode):
                        values = list(info)
                        values[0] |= unsafe_bit
                        return os.stat_result(values)
                    return info

                with mock.patch.object(
                    phase.os, "fstat", side_effect=fstat_with_setid
                ), mock.patch.object(phase.os, "fchmod") as changed:
                    with self.assertRaisesRegex(
                        phase.SuccessorProduceV4Error, "unsafe mode"
                    ):
                        phase._make_outputs_collectable(outputs)
                    changed.assert_not_called()

    def test_collectability_rejects_unknown_or_nested_output_members(self) -> None:
        for member_name, is_directory in (("unknown", False), ("nested", True)):
            with self.subTest(member=member_name):
                outputs = self.root / f"outputs-{member_name}"
                outputs.mkdir()
                (outputs / phase.CONSUMED_MARKER_NAME).write_bytes(b"{}\n")
                member = outputs / member_name
                if is_directory:
                    member.mkdir()
                else:
                    member.write_bytes(b"x")
                with self.assertRaisesRegex(
                    phase.SuccessorProduceV4Error, "member set|unsafe file kind"
                ):
                    phase._make_outputs_collectable(outputs)

    def test_module_top_level_is_stdlib_only_and_has_no_old_producer_fallback(self) -> None:
        source = pathlib.Path(phase.__file__).read_text()
        tree = ast.parse(source)
        imports = []
        for node in tree.body:
            if isinstance(node, ast.Import):
                imports.extend(alias.name.split(".")[0] for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                imports.append((node.module or "").split(".")[0])
        self.assertEqual(
            set(imports),
            {
                "__future__",
                "argparse",
                "contextlib",
                "dataclasses",
                "hashlib",
                "importlib",
                "io",
                "json",
                "os",
                "pathlib",
                "re",
                "selectors",
                "stat",
                "subprocess",
                "sys",
                "tarfile",
                "tempfile",
                "time",
                "typing",
            },
        )
        for forbidden in (
            "native_shadow_successor_produce_phase_arm64_v2",
            "native_shadow_boot_produce_phase_arm64_v1",
            "native_shadow_boot_image_produce_arm64_v1",
            "native_shadow_successor_root_disk_readback_arm64_v2",
            "native-shadow-successor-produce-arm64-v3.sh",
        ):
            self.assertNotIn(forbidden, source)
        for forbidden_constant in (
            "R2_SHA256",
            "F6_SHA256",
            "A6_SHA256",
            "RESULT_V6_SHA256",
        ):
            self.assertNotIn(forbidden_constant, source)


if __name__ == "__main__":
    unittest.main()
